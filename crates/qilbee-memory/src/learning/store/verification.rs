//! Offline verification of a stopped learning store.
//!
//! A read-only handle never rebuilds or repairs anything. It reports the
//! authoritative inventory and checks that the persisted knowledge index is
//! exactly what the ledgers imply, using the same audited enumeration that the
//! server runs when it opens the store for writing.
use super::knowledge_index::{GENERATION_KEY, INDEX_ROOT, KnowledgeIndexSink};
use super::*;
use qilbee_storage::verification::{self as offline, FamilyInventory};

mod strategy_locators;
mod withdrawals;
pub use withdrawals::ExperienceWithdrawalVerification;

/// Result of comparing the persisted derived index with the authoritative ledgers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeIndexVerification {
    pub generation: uuid::Uuid,
    pub knowledge_receipts: u64,
    pub combined_origins: u64,
    pub active_entries: u64,
    /// Persisted derived entries including the generation marker.
    pub derived_entries: u64,
}

impl LearningMemory {
    /// Open a stopped learning store read-only. The store must have been closed
    /// by a binary that published a knowledge index generation; nothing is
    /// rebuilt, and another process holding the store is rejected.
    pub fn open_read_only(path: impl AsRef<Path>) -> Result<Self> {
        let db = offline::open_read_only(path.as_ref(), &["default"])?;
        match db.get(SCHEMA_KEY).map_err(storage_error)? {
            Some(version) if version.as_slice() == SCHEMA_VERSION => {}
            Some(_) => {
                return Err(Error::Configuration(
                    "Unsupported learning schema version".into(),
                ));
            }
            None => {
                return Err(Error::Configuration(
                    "Not a learning store: schema marker is missing".into(),
                ));
            }
        }
        let generation = db
            .get(GENERATION_KEY)
            .map_err(storage_error)?
            .ok_or_else(|| {
                Error::DataCorruption(
                    "Knowledge index generation is missing; the store was never published by a compatible binary".into(),
                )
            })?;
        let generation = uuid::Uuid::from_slice(&generation)
            .map_err(|_| Error::DataCorruption("Knowledge index generation is malformed".into()))?;
        if generation.get_version_num() != 4 {
            return Err(Error::DataCorruption(
                "Knowledge index generation has an unexpected format".into(),
            ));
        }
        Ok(Self {
            inner: Arc::new(Inner {
                knowledge_generation: generation,
                db,
                mutation_lock: Mutex::new(()),
            }),
        })
    }

    /// Open under a retained writer exclusion; the caller must finish it before publishing results.
    pub fn open_read_only_guarded(
        path: impl AsRef<Path>,
        guard: &mut qilbee_storage::verification::WriterExclusion,
    ) -> Result<Self> {
        let db = offline::open_read_only_guarded(path.as_ref(), &["default"], guard)?;
        match db.get(SCHEMA_KEY).map_err(storage_error)? {
            Some(version) if version.as_slice() == SCHEMA_VERSION => {}
            Some(_) => {
                return Err(Error::Configuration(
                    "Unsupported learning schema version".into(),
                ));
            }
            None => {
                return Err(Error::Configuration(
                    "Not a learning store: schema marker is missing".into(),
                ));
            }
        }
        let generation = db
            .get(GENERATION_KEY)
            .map_err(storage_error)?
            .ok_or_else(|| {
                Error::DataCorruption(
                    "Knowledge index generation is missing; the store was never published by a compatible binary".into(),
                )
            })?;
        let generation = uuid::Uuid::from_slice(&generation)
            .map_err(|_| Error::DataCorruption("Knowledge index generation is malformed".into()))?;
        if generation.get_version_num() != 4 {
            return Err(Error::DataCorruption(
                "Knowledge index generation has an unexpected format".into(),
            ));
        }
        Ok(Self {
            inner: Arc::new(Inner {
                knowledge_generation: generation,
                db,
                mutation_lock: Mutex::new(()),
            }),
        })
    }

    /// Authoritative inventory: derived knowledge index entries and the
    /// generation marker are counted separately and excluded from the digest,
    /// so digests compare across index generations.
    pub fn inventory(&self) -> Result<Vec<FamilyInventory>> {
        self.verify_experience_withdrawals()?;
        self.verify_strategy_locators()?;
        Ok(vec![offline::family_inventory(
            &self.inner.db,
            "default",
            |key| {
                key.starts_with(INDEX_ROOT)
                    || key == GENERATION_KEY
                    || key.first() == Some(&26)
                    || key == super::strategies::STRATEGY_LOCATOR_MARKER
                    || key == super::strategies::STRATEGY_LOCATOR_PROGRESS
            },
        )?])
    }

    /// Compare the persisted knowledge index with a fresh audited enumeration.
    /// Every implied entry must exist with identical bytes, every persisted
    /// entry must belong to the published generation, and the counts must match.
    pub fn verify_knowledge_index(&self) -> Result<KnowledgeIndexVerification> {
        self.verify_experience_withdrawals()?;
        self.verify_strategy_locators()?;
        struct Comparator<'a> {
            db: &'a DB,
            expected: u64,
        }
        impl KnowledgeIndexSink for Comparator<'_> {
            fn entry(&mut self, key: Vec<u8>, value: Vec<u8>) -> Result<()> {
                match self.db.get(&key).map_err(storage_error)? {
                    Some(persisted) if persisted == value => {
                        self.expected += 1;
                        Ok(())
                    }
                    Some(_) => Err(Error::DataCorruption(
                        "Knowledge index entry differs from the authoritative ledger".into(),
                    )),
                    None => Err(Error::DataCorruption(
                        "Knowledge index entry is missing".into(),
                    )),
                }
            }
            fn receipts_complete(&mut self) -> Result<()> {
                Ok(())
            }
        }
        let mut comparator = Comparator {
            db: &self.inner.db,
            expected: 0,
        };
        let summary = self.audited_knowledge_entries(&mut comparator)?;
        let mut generation_prefix = INDEX_ROOT.to_vec();
        generation_prefix.extend_from_slice(self.inner.knowledge_generation.as_bytes());
        let mut persisted = 0u64;
        for item in self
            .inner
            .db
            .iterator(IteratorMode::From(INDEX_ROOT, Direction::Forward))
        {
            let (key, _) = item.map_err(storage_error)?;
            if !key.starts_with(INDEX_ROOT) {
                break;
            }
            if !key.starts_with(&generation_prefix) {
                return Err(Error::DataCorruption(
                    "Knowledge index contains an entry from an unpublished generation".into(),
                ));
            }
            persisted += 1;
        }
        if persisted != comparator.expected {
            return Err(Error::DataCorruption(format!(
                "Knowledge index has {persisted} persisted entries but the ledgers imply {}",
                comparator.expected
            )));
        }
        Ok(KnowledgeIndexVerification {
            generation: self.inner.knowledge_generation,
            knowledge_receipts: summary.knowledge_receipts,
            combined_origins: summary.combined_origins,
            active_entries: summary.active_entries,
            derived_entries: persisted + 1,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::knowledge::KnowledgeReceipt;
    use super::super::knowledge::tests::{install_contracts, proposal};
    use super::*;
    use crate::{MemoryStorageConfig, RocksDbMemoryStorage};

    fn fixture() -> (tempfile::TempDir, KnowledgeReceipt) {
        let dir = tempfile::TempDir::new().unwrap();
        let db = LearningMemory::open(dir.path()).unwrap();
        install_contracts(&db);
        let receipt = db
            .propose_knowledge("company", "scope", proposal(), "author")
            .unwrap();
        // Synthetic active state exercises the ranked projection without an
        // evaluation campaign; the next open rebuilds the derived index.
        let mut active = receipt.record.clone();
        active.state = ProcedureState::Active;
        active.lower_improvement_bound = Some(0.25);
        db.inner
            .db
            .put(
                procedure_key(&active.scope, &active.proposal.id),
                encode(&active).unwrap(),
            )
            .unwrap();
        drop(db);
        drop(LearningMemory::open(dir.path()).unwrap());
        (dir, receipt)
    }

    fn raw(path: &Path) -> DB {
        let mut options = rocksdb::Options::default();
        options.create_if_missing(false);
        DB::open(&options, path).unwrap()
    }

    fn verify(path: &Path) -> Result<KnowledgeIndexVerification> {
        LearningMemory::open_read_only(path)?.verify_knowledge_index()
    }

    #[test]
    fn read_only_store_reports_inventory_and_verifies_a_published_index() {
        let (dir, receipt) = fixture();
        let store = LearningMemory::open_read_only(dir.path()).unwrap();
        let verification = store.verify_knowledge_index().unwrap();
        assert_eq!(verification.knowledge_receipts, 1);
        assert_eq!(verification.active_entries, 1);
        assert_eq!(verification.combined_origins, 0);
        assert_eq!(verification.derived_entries, 3);
        assert_eq!(verification.generation.get_version_num(), 4);
        let inventory = store.inventory().unwrap();
        assert_eq!(inventory.len(), 1);
        assert_eq!(inventory[0].derived_entries, 3);
        // Schema marker, policy, context, procedure, registered binding and
        // receipt are authoritative; locator, active entry and generation are not.
        assert_eq!(inventory[0].records, 6);
        assert_eq!(
            store
                .knowledge_receipt("company", "scope", &receipt.request.id)
                .unwrap(),
            Some(receipt.clone())
        );
        // Repeating the same proposal is idempotent and reads only; a new
        // proposal must write and is refused by the read-only handle.
        assert_eq!(
            store
                .propose_knowledge("company", "scope", proposal(), "writer")
                .unwrap(),
            receipt
        );
        let mut fresh = proposal();
        fresh.id = "knowledge-r2".into();
        let refused = store.propose_knowledge("company", "scope", fresh, "writer");
        assert!(matches!(refused, Err(Error::Storage(_))), "{refused:?}");
        drop(store);
        // Reopening for writing republishes a new generation; digests are stable.
        drop(LearningMemory::open(dir.path()).unwrap());
        let reopened = LearningMemory::open_read_only(dir.path()).unwrap();
        assert_eq!(reopened.inventory().unwrap()[0].sha256, inventory[0].sha256);
        assert_ne!(
            reopened.verify_knowledge_index().unwrap().generation,
            verification.generation
        );
    }

    #[test]
    fn read_only_open_rejects_unpublished_or_foreign_stores() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut options = rocksdb::Options::default();
        options.create_if_missing(true);
        let db = DB::open(&options, dir.path()).unwrap();
        db.put(b"unrelated", b"1").unwrap();
        drop(db);
        assert!(matches!(
            LearningMemory::open_read_only(dir.path()),
            Err(Error::Configuration(_))
        ));
        let db = raw(dir.path());
        db.delete(b"unrelated").unwrap();
        db.put(SCHEMA_KEY, SCHEMA_VERSION).unwrap();
        drop(db);
        assert!(matches!(
            LearningMemory::open_read_only(dir.path()),
            Err(Error::DataCorruption(message)) if message.contains("generation is missing")
        ));
        let db = raw(dir.path());
        db.put(GENERATION_KEY, b"not-a-uuid").unwrap();
        drop(db);
        assert!(matches!(
            LearningMemory::open_read_only(dir.path()),
            Err(Error::DataCorruption(_))
        ));
    }

    #[test]
    fn verification_fails_closed_on_missing_changed_extra_or_stale_index_entries() {
        let (dir, receipt) = fixture();
        let store = LearningMemory::open_read_only(dir.path()).unwrap();
        let current = store
            .registered_procedure("company", "scope", &receipt.request.id)
            .unwrap()
            .unwrap()
            .record;
        let locator = store.locator_key(&current);
        let active = store.active_key(&receipt, &current).unwrap();
        let mut stale = INDEX_ROOT.to_vec();
        stale.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
        stale.push(0);
        drop(store);

        let db = raw(dir.path());
        let locator_value = db.get(&locator).unwrap().unwrap();
        db.delete(&locator).unwrap();
        drop(db);
        assert!(matches!(
            verify(dir.path()),
            Err(Error::DataCorruption(message)) if message.contains("missing")
        ));

        let db = raw(dir.path());
        db.put(&locator, b"{\"tampered\":true}").unwrap();
        drop(db);
        assert!(matches!(
            verify(dir.path()),
            Err(Error::DataCorruption(message)) if message.contains("differs")
        ));

        let db = raw(dir.path());
        db.put(&locator, &locator_value).unwrap();
        let active_value = db.get(&active).unwrap().unwrap();
        db.delete(&active).unwrap();
        drop(db);
        assert!(matches!(
            verify(dir.path()),
            Err(Error::DataCorruption(message)) if message.contains("missing")
        ));

        let db = raw(dir.path());
        db.put(&active, &active_value).unwrap();
        db.put(&stale, b"{}").unwrap();
        drop(db);
        assert!(matches!(
            verify(dir.path()),
            Err(Error::DataCorruption(message)) if message.contains("unpublished generation")
        ));

        let db = raw(dir.path());
        db.delete(&stale).unwrap();
        let mut extra = locator.clone();
        extra.push(0xff);
        db.put(&extra, b"{}").unwrap();
        drop(db);
        assert!(matches!(
            verify(dir.path()),
            Err(Error::DataCorruption(message)) if message.contains("ledgers imply")
        ));

        let db = raw(dir.path());
        db.delete(&extra).unwrap();
        drop(db);
        assert!(verify(dir.path()).is_ok());
    }

    #[test]
    fn verification_fails_closed_on_tampered_ledgers() {
        let (dir, receipt) = fixture();
        let receipt_key =
            super::super::knowledge::key("company", "scope", &receipt.request.id).unwrap();
        let db = raw(dir.path());
        let original = db.get(&receipt_key).unwrap().unwrap();
        let mut forged = receipt.clone();
        forged.request.instructions = "tampered".into();
        db.put(&receipt_key, encode(&forged).unwrap()).unwrap();
        drop(db);
        assert!(matches!(verify(dir.path()), Err(Error::DataCorruption(_))));

        let db = raw(dir.path());
        db.put(&receipt_key, &original).unwrap();
        // An origin record without its ordinary receipt is an orphan.
        let orphan =
            super::super::knowledge_origin::origin_key("company", "scope", "ghost").unwrap();
        db.put(&orphan, b"{}").unwrap();
        drop(db);
        assert!(matches!(verify(dir.path()), Err(Error::DataCorruption(_))));

        let db = raw(dir.path());
        db.delete(&orphan).unwrap();
        // A bound procedure whose receipt was removed must not verify.
        db.delete(&receipt_key).unwrap();
        drop(db);
        assert!(matches!(verify(dir.path()), Err(Error::DataCorruption(_))));
    }

    #[test]
    fn memory_storage_read_only_inventory_requires_exact_families_and_never_writes() {
        let dir = tempfile::TempDir::new().unwrap();
        drop(RocksDbMemoryStorage::open(MemoryStorageConfig::for_testing(dir.path())).unwrap());
        let store = RocksDbMemoryStorage::open_read_only(dir.path()).unwrap();
        let inventory = store.inventory().unwrap();
        let mut families: Vec<&str> = inventory.iter().map(|f| f.family.as_str()).collect();
        families.sort();
        assert_eq!(
            families,
            vec![
                "default",
                "memory_agent_meta",
                "memory_episode_index",
                "memory_episodes"
            ]
        );
        drop(store);
        let learning = tempfile::TempDir::new().unwrap();
        drop(LearningMemory::open(learning.path()).unwrap());
        assert!(matches!(
            RocksDbMemoryStorage::open_read_only(learning.path()),
            Err(Error::DataCorruption(_))
        ));
        assert!(matches!(
            LearningMemory::open_read_only(dir.path()),
            Err(Error::DataCorruption(_))
        ));
    }
}
