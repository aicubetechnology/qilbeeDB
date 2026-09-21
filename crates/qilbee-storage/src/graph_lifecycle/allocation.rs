use super::*;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct HighWater {
    schema_version: u32,
    graph_id: GraphId,
    node: u64,
    relationship: u64,
}
pub(super) fn water_key(id: GraphId) -> String {
    format!("graph/v2/allocator/{:016x}", id.as_internal())
}
impl StorageEngine {
    fn greatest_entity_id(&self, family: &str, kind: u8, id: GraphId) -> Result<u64> {
        let mut prefix = vec![kind];
        prefix.extend(id.as_internal().to_be_bytes());
        let mut end = prefix.clone();
        end.extend([255; 8]);
        let mut iter = self.db.raw_iterator_cf(self.cf(family)?);
        iter.seek_for_prev(end);
        iter.status().map_err(|e| Error::Storage(e.to_string()))?;
        let Some(key) = iter.key().filter(|key| key.starts_with(&prefix)) else {
            return Ok(0);
        };
        if key.len() != 17 {
            return Err(invalid());
        }
        Ok(u64::from_be_bytes(
            key[9..].try_into().map_err(|_| invalid())?,
        ))
    }
    pub(super) fn graph_high_water(&self, id: GraphId, allow_missing: bool) -> Result<HighWater> {
        let node = self.greatest_entity_id(cf::NODES, 1, id)?;
        let relationship = self.greatest_entity_id(cf::RELATIONSHIPS, 2, id)?;
        if let Some(bytes) = self.get_meta(&water_key(id))? {
            let water: HighWater = decode(&bytes)?;
            if water.schema_version != 1
                || water.graph_id != id
                || water.node < node
                || water.relationship < relationship
            {
                return Err(invalid());
            }
            Ok(water)
        } else if allow_missing {
            Ok(HighWater {
                schema_version: 1,
                graph_id: id,
                node,
                relationship,
            })
        } else {
            Err(invalid())
        }
    }
    // Every entity mutation, including trusted direct writes, preserves allocation
    // high-water marks in its own entity/index batch. Deletes never lower them.
    pub(in crate::engine) fn stage_graph_allocator(
        &self,
        batch: &mut WriteBatch,
        id: GraphId,
        operations: &[TransactionOperation],
    ) -> Result<bool> {
        let identity = self.stored_graph_identity(id)?;
        if identity.as_ref().is_some_and(|value| !value.active) {
            return Err(inactive());
        }
        let mut water = self.graph_high_water(id, identity.is_none())?;
        for operation in operations {
            match operation {
                TransactionOperation::PutNode(node) => {
                    water.node = water.node.max(node.id.as_internal())
                }
                TransactionOperation::PutRelationship(rel) => {
                    water.relationship = water.relationship.max(rel.id.as_internal())
                }
                _ => (),
            }
        }
        self.stage_meta(batch, &water_key(id), &encode(&water)?)?;
        Ok(identity.is_some())
    }
    /// Allocate and publish a node with its indexes and durable high-water mark.
    /// The validation callback runs under the shared writer lock. It may read
    /// storage but must not mutate this engine or acquire the writer lock again.
    pub fn create_graph_node(
        &self,
        identity: &GraphIdentity,
        mut node: Node,
        validate: impl FnOnce(&Node) -> Result<()>,
    ) -> Result<Node> {
        let _guard = self
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Storage mutation lock poisoned".into()))?;
        self.validate_graph_identity(identity)?;
        let next = self
            .graph_high_water(identity.id, false)?
            .node
            .checked_add(1)
            .ok_or_else(|| Error::InvalidGraphOperation("Node ID space exhausted".into()))?;
        node.id = NodeId::from_internal(next);
        validate(&node)?;
        self.apply_operations_locked(identity.id, &[TransactionOperation::PutNode(node.clone())])?;
        Ok(node)
    }
    /// Validate and replace an existing node under the shared writer lock.
    /// The callback may read storage but must not mutate this engine.
    pub fn update_graph_node(
        &self,
        identity: &GraphIdentity,
        node: &Node,
        validate: impl FnOnce(&Node) -> Result<()>,
    ) -> Result<()> {
        let _guard = self
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Storage mutation lock poisoned".into()))?;
        self.validate_graph_identity(identity)?;
        if self.get_node(identity.id, node.id)?.is_none() {
            return Err(Error::NodeNotFound(format!("{:?}", node.id)));
        }
        validate(node)?;
        self.apply_operations_locked(identity.id, &[TransactionOperation::PutNode(node.clone())])
    }
    pub fn create_graph_relationship(
        &self,
        identity: &GraphIdentity,
        mut rel: Relationship,
    ) -> Result<Relationship> {
        let _guard = self
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Storage mutation lock poisoned".into()))?;
        self.validate_graph_identity(identity)?;
        for id in [rel.source, rel.target] {
            if self.get_node(identity.id, id)?.is_none() {
                return Err(Error::NodeNotFound(format!("{id:?}")));
            }
        }
        let next = self
            .graph_high_water(identity.id, false)?
            .relationship
            .checked_add(1)
            .ok_or_else(|| {
                Error::InvalidGraphOperation("Relationship ID space exhausted".into())
            })?;
        rel.id = RelationshipId::from_internal(next);
        self.apply_operations_locked(
            identity.id,
            &[TransactionOperation::PutRelationship(rel.clone())],
        )?;
        Ok(rel)
    }
    pub fn update_graph_relationship(
        &self,
        identity: &GraphIdentity,
        rel: &Relationship,
    ) -> Result<()> {
        let _guard = self
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Storage mutation lock poisoned".into()))?;
        self.validate_graph_identity(identity)?;
        if self.get_relationship(identity.id, rel.id)?.is_none() {
            return Err(Error::RelationshipNotFound(format!("{:?}", rel.id)));
        }
        for id in [rel.source, rel.target] {
            if self.get_node(identity.id, id)?.is_none() {
                return Err(Error::NodeNotFound(format!("{id:?}")));
            }
        }
        self.apply_operations_locked(
            identity.id,
            &[TransactionOperation::PutRelationship(rel.clone())],
        )
    }
    pub fn delete_graph_node(
        &self,
        identity: &GraphIdentity,
        id: NodeId,
        detach: bool,
    ) -> Result<bool> {
        let _guard = self
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Storage mutation lock poisoned".into()))?;
        self.validate_graph_identity(identity)?;
        let exists = self.get_node(identity.id, id)?.is_some();
        let outgoing = self.get_outgoing_relationships(identity.id, id)?;
        let incoming = self.get_incoming_relationships(identity.id, id)?;
        if !detach && (!outgoing.is_empty() || !incoming.is_empty()) {
            return Err(Error::InvalidGraphOperation(
                "Node has relationships; use detach_delete_node".into(),
            ));
        }
        let mut operations: Vec<_> = outgoing
            .into_iter()
            .chain(incoming)
            .map(|rel| TransactionOperation::DeleteRelationship(rel.id))
            .collect();
        operations.push(TransactionOperation::DeleteNode(id));
        self.apply_operations_locked(identity.id, &operations)?;
        Ok(exists)
    }
    pub fn delete_graph_relationship(
        &self,
        identity: &GraphIdentity,
        id: RelationshipId,
    ) -> Result<bool> {
        let _guard = self
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Storage mutation lock poisoned".into()))?;
        self.validate_graph_identity(identity)?;
        let exists = self.get_relationship(identity.id, id)?.is_some();
        self.apply_operations_locked(identity.id, &[TransactionOperation::DeleteRelationship(id)])?;
        Ok(exists)
    }
    pub(crate) fn apply_graph_operations(
        &self,
        identity: &GraphIdentity,
        operations: &[TransactionOperation],
    ) -> Result<()> {
        let _guard = self
            .mutation_lock
            .lock()
            .map_err(|_| Error::Internal("Storage mutation lock poisoned".into()))?;
        self.validate_graph_identity(identity)?;
        self.apply_operations_locked(identity.id, operations)
    }
}
