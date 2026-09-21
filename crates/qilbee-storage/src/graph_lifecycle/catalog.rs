use super::*;

impl StorageEngine {
    // Caller owns mutation_lock, including while a legacy catalog is migrated.
    fn graph_catalog_locked(&self) -> Result<Catalog> {
        let Some(bytes) = self.get_meta(CATALOG)? else {
            if !self
                .scan_meta_keys("graph/v2/identity/", None, 1)?
                .0
                .is_empty()
            {
                return Err(invalid());
            }
            return Ok(Catalog {
                schema_version: 2,
                graphs: BTreeMap::new(),
            });
        };
        let value: serde_json::Value = decode(&bytes)?;
        if value.is_array() {
            if !self
                .scan_meta_keys("graph/v2/identity/", None, 1)?
                .0
                .is_empty()
            {
                return Err(invalid());
            }
            let names: Vec<String> = decode(&bytes)?;
            let mut catalog = Catalog {
                schema_version: 2,
                graphs: BTreeMap::new(),
            };
            let mut ids = HashSet::new();
            let mut batch = WriteBatch::default();
            for name in names {
                validate_name(&name).map_err(|_| invalid())?;
                let id = GraphId::from_name(&name);
                if !ids.insert(id) || catalog.graphs.insert(name.clone(), id).is_some() {
                    return Err(invalid());
                }
                if self.stored_graph_identity(id)?.is_some() {
                    return Err(invalid());
                }
                let identity = GraphIdentity {
                    schema_version: 1,
                    name,
                    id,
                    active: true,
                };
                self.stage_meta(&mut batch, &identity_key(id), &encode(&identity)?)?;
                let water = self.graph_high_water(id, true)?;
                self.stage_meta(&mut batch, &allocation::water_key(id), &encode(&water)?)?;
            }
            self.stage_meta(&mut batch, CATALOG, &encode(&catalog)?)?;
            self.sync_graph_batch(batch)?;
            return Ok(catalog);
        }
        let catalog: Catalog = decode(&bytes)?;
        if catalog.schema_version != 2 {
            return Err(invalid());
        }
        let mut ids = HashSet::new();
        for (name, id) in &catalog.graphs {
            validate_name(name).map_err(|_| invalid())?;
            if !ids.insert(*id) {
                return Err(invalid());
            }
            let identity = self.stored_graph_identity(*id)?.ok_or_else(invalid)?;
            if identity.name != *name || !identity.active {
                return Err(invalid());
            }
        }
        Ok(catalog)
    }
    /// Return authoritative active graph identities; migrate legacy name lists
    /// atomically without changing their established physical graph IDs.
    pub fn named_graphs(&self) -> Result<Vec<GraphIdentity>> {
        let _guard = self
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Storage mutation lock poisoned".into()))?;
        let catalog = self.graph_catalog_locked()?;
        catalog
            .graphs
            .into_iter()
            .map(|(name, id)| {
                Ok(GraphIdentity {
                    schema_version: 1,
                    name,
                    id,
                    active: true,
                })
            })
            .collect()
    }
    pub fn open_named_graph(
        &self,
        name: &str,
        max_graphs: usize,
        create_only: bool,
    ) -> Result<GraphIdentity> {
        validate_name(name)?;
        let _guard = self
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Storage mutation lock poisoned".into()))?;
        let mut catalog = self.graph_catalog_locked()?;
        if let Some(id) = catalog.graphs.get(name) {
            if create_only {
                return Err(Error::InvalidGraphOperation(format!(
                    "Graph '{name}' already exists"
                )));
            }
            return Ok(GraphIdentity {
                schema_version: 1,
                name: name.into(),
                id: *id,
                active: true,
            });
        }
        if catalog.graphs.len() >= max_graphs {
            return Err(Error::InvalidGraphOperation(format!(
                "Maximum number of graphs ({max_graphs}) reached"
            )));
        }
        // Randomness proposes IDs; durable reservation and retained-prefix checks
        // enforce uniqueness. Exhaustion/collisions fail without reusing a prefix.
        let mut selected = None;
        for _ in 0..64 {
            let id = GraphId::new();
            if self.graph_id_available(id)? {
                selected = Some(id);
                break;
            }
        }
        let id = selected
            .ok_or_else(|| Error::InvalidGraphOperation("No fresh graph ID available".into()))?;
        let identity = GraphIdentity {
            schema_version: 1,
            name: name.into(),
            id,
            active: true,
        };
        catalog.graphs.insert(name.into(), id);
        let mut batch = WriteBatch::default();
        self.stage_meta(&mut batch, CATALOG, &encode(&catalog)?)?;
        self.stage_meta(&mut batch, &identity_key(id), &encode(&identity)?)?;
        self.stage_meta(
            &mut batch,
            &allocation::water_key(id),
            &encode(&self.graph_high_water(id, true)?)?,
        )?;
        self.sync_graph_batch(batch)?;
        Ok(identity)
    }
    pub fn retire_named_graph(&self, name: &str) -> Result<Option<GraphIdentity>> {
        validate_name(name)?;
        let _guard = self
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Storage mutation lock poisoned".into()))?;
        let mut catalog = self.graph_catalog_locked()?;
        let Some(id) = catalog.graphs.remove(name) else {
            return Ok(None);
        };
        let mut identity = self.stored_graph_identity(id)?.ok_or_else(invalid)?;
        identity.active = false;
        let mut batch = WriteBatch::default();
        self.stage_meta(&mut batch, CATALOG, &encode(&catalog)?)?;
        self.stage_meta(&mut batch, &identity_key(id), &encode(&identity)?)?;
        self.sync_graph_batch(batch)?;
        Ok(Some(identity))
    }
    pub(super) fn graph_id_available(&self, id: GraphId) -> Result<bool> {
        if self.get_meta(&identity_key(id))?.is_some()
            || self.get_meta(&allocation::water_key(id))?.is_some()
        {
            return Ok(false);
        }
        // Include legacy orphaned indexes as well as canonical entities.
        for (family, kind) in [
            (cf::NODES, 1),
            (cf::RELATIONSHIPS, 2),
            (cf::LABEL_INDEX, 3),
            (cf::ADJACENCY_OUT, 4),
            (cf::ADJACENCY_IN, 5),
            (cf::PROPERTY_INDEX, 6),
            (cf::SCHEMA, 7),
            (cf::SCHEMA, 9),
            (cf::MEMORY, 0x10),
            (cf::MEMORY, 0x11),
            (cf::MEMORY, 0x12),
        ] {
            let mut prefix = vec![kind];
            prefix.extend(id.as_internal().to_be_bytes());
            let mut iter = self.db.raw_iterator_cf(self.cf(family)?);
            iter.seek(&prefix);
            iter.status().map_err(|e| Error::Storage(e.to_string()))?;
            if iter.key().is_some_and(|key| key.starts_with(&prefix)) {
                return Ok(false);
            }
        }
        Ok(true)
    }
}
