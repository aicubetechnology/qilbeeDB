//! Versioned, equality-compatible property fingerprints and startup reconstruction.
use super::{StorageEngine, cf};
use crate::keys::{KeyBuilder, prefix};
use qilbee_core::{EntityId, Error, GraphId, Node, PropertyValue, Result};
use rocksdb::{IteratorMode, WriteBatch, WriteOptions};

const VERSION: &[u8] = b"canonical-fnv1a64-v1";
// Separate from logical metadata (0x08) and its ordered lookup keys (0xf0).
const MARKER: &[u8] = b"\xf1property-index-format";
const BATCH_BYTES: usize = 1024 * 1024;

/// A stable candidate fingerprint, never an equality proof or security digest.
/// Tags, lengths and fixed-endian numbers make boundaries explicit. Equal signed
/// zeros normalize recursively; map entries use UTF-8 key order, arrays retain order.
pub(super) fn hash_property_value(value: &PropertyValue) -> u64 {
    fn bytes(h: &mut u64, data: &[u8]) {
        for byte in data {
            *h = (*h ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
        }
    }
    fn number(h: &mut u64, n: u64) {
        bytes(h, &n.to_be_bytes());
    }
    fn sized(h: &mut u64, data: &[u8]) {
        number(h, data.len() as u64);
        bytes(h, data);
    }
    fn float(h: &mut u64, n: f64) {
        number(h, if n == 0.0 { 0 } else { n.to_bits() });
    }
    fn visit(h: &mut u64, v: &PropertyValue) {
        use PropertyValue::*;
        let tag = match v {
            Null => 0,
            Boolean(_) => 1,
            Integer(_) => 2,
            Float(_) => 3,
            String(_) => 4,
            Array(_) => 5,
            Map(_) => 6,
            Bytes(_) => 7,
            Date(_) => 8,
            Time(_) => 9,
            DateTime(_) => 10,
            Duration(_) => 11,
            Point2D { .. } => 12,
            Point3D { .. } => 13,
        };
        bytes(h, &[tag]);
        match v {
            Null => {}
            Boolean(b) => bytes(h, &[u8::from(*b)]),
            Integer(n) => number(h, *n as u64),
            Float(n) => float(h, *n),
            String(s) => sized(h, s.as_bytes()),
            Bytes(b) => sized(h, b),
            Array(a) => {
                number(h, a.len() as u64);
                for v in a {
                    visit(h, v);
                }
            }
            Map(m) => {
                number(h, m.len() as u64);
                let mut entries = m.iter().collect::<Vec<_>>();
                entries.sort_unstable_by(|a, b| a.0.cmp(b.0));
                for (k, v) in entries {
                    sized(h, k.as_bytes());
                    visit(h, v);
                }
            }
            Date(n) => bytes(h, &n.to_be_bytes()),
            Time(n) | DateTime(n) | Duration(n) => number(h, *n as u64),
            Point2D { x, y, srid } => {
                float(h, *x);
                float(h, *y);
                bytes(h, &srid.to_be_bytes());
            }
            Point3D { x, y, z, srid } => {
                float(h, *x);
                float(h, *y);
                float(h, *z);
                bytes(h, &srid.to_be_bytes());
            }
        }
    }
    let mut h = 0xcbf29ce484222325;
    visit(&mut h, value);
    h
}

impl StorageEngine {
    /// Run before returning an engine handle. RocksDB excludes another opener;
    /// no callers can observe a partially reconstructed index. A missing marker
    /// always restarts reconstruction from authoritative nodes, clearing partial
    /// output. Each batch is WAL-backed and synced, regardless of runtime options.
    pub(super) fn ensure_property_index(&self) -> Result<()> {
        let meta = self.cf(cf::META)?;
        let marker = MARKER;
        if let Some(version) = self
            .db
            .get_cf(meta, &marker)
            .map_err(|e| Error::Storage(e.to_string()))?
        {
            if version == VERSION {
                return Ok(());
            }
            return Err(Error::Storage(
                "Unsupported property index format; use a compatible database version".into(),
            ));
        }
        tracing::info!("Reconstructing legacy property index before readiness");
        let index = self.cf(cf::PROPERTY_INDEX)?;
        let mut options = WriteOptions::default();
        options.disable_wal(false);
        options.set_sync(true);
        let mut batch = WriteBatch::default();
        for row in self.db.iterator_cf(index, IteratorMode::Start) {
            let (key, _) = row.map_err(|e| Error::Storage(e.to_string()))?;
            batch.delete_cf(index, key);
            if batch.size_in_bytes() >= BATCH_BYTES {
                self.db
                    .write_opt(batch, &options)
                    .map_err(|e| Error::Storage(e.to_string()))?;
                batch = WriteBatch::default();
            }
        }
        self.db
            .write_opt(batch, &options)
            .map_err(|e| Error::Storage(e.to_string()))?;
        let mut batch = WriteBatch::default();
        for row in self
            .db
            .iterator_cf(self.cf(cf::NODES)?, IteratorMode::Start)
        {
            let (key, value) = row.map_err(|e| Error::Storage(e.to_string()))?;
            if key.len() != 17 || key[0] != prefix::NODE {
                return Err(Error::Storage(
                    "Invalid node key during property index reconstruction".into(),
                ));
            }
            let graph = GraphId::from_internal(u64::from_be_bytes(key[1..9].try_into().unwrap()));
            let node: Node =
                bincode::deserialize(&value).map_err(|e| Error::Deserialization(e.to_string()))?;
            if key.as_ref() != KeyBuilder::node(graph, node.id) {
                return Err(Error::Storage(
                    "Node identity mismatch during property index reconstruction".into(),
                ));
            }
            for label in &node.labels {
                for (name, value) in node.properties.iter() {
                    batch.put_cf(
                        index,
                        KeyBuilder::property_index(
                            graph,
                            label.name(),
                            name,
                            hash_property_value(value),
                            node.id.as_internal(),
                        ),
                        bincode::serialize(value)
                            .map_err(|e| Error::Serialization(e.to_string()))?,
                    );
                    if batch.size_in_bytes() >= BATCH_BYTES {
                        self.db
                            .write_opt(batch, &options)
                            .map_err(|e| Error::Storage(e.to_string()))?;
                        #[cfg(test)]
                        migration_test_barrier();
                        batch = WriteBatch::default();
                    }
                }
            }
        }
        batch.put_cf(meta, marker, VERSION);
        self.db
            .write_opt(batch, &options)
            .map_err(|e| Error::Storage(e.to_string()))?;
        tracing::info!("Canonical property index reconstruction completed");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StorageOptions;
    use qilbee_core::NodeId;
    use std::collections::HashMap;

    fn value(reverse: bool) -> PropertyValue {
        let mut pairs = (0..16)
            .map(|i| {
                (
                    format!("key-{i}"),
                    PropertyValue::Array(vec![
                        PropertyValue::Float(if reverse { 0.0 } else { -0.0 }),
                        PropertyValue::Integer(i),
                    ]),
                )
            })
            .collect::<Vec<_>>();
        if reverse {
            pairs.reverse();
        }
        PropertyValue::Map(pairs.into_iter().collect::<HashMap<_, _>>())
    }
    #[test]
    fn format_marker_is_outside_application_metadata_namespace() {
        let dir = tempfile::tempdir().unwrap();
        let engine = StorageEngine::open(StorageOptions::new(dir.path())).unwrap();
        assert!(
            engine
                .scan_meta_keys("property-", None, 100)
                .unwrap()
                .0
                .is_empty()
        );
        engine
            .put_meta("property-index-format", b"application value")
            .unwrap();
        engine.ensure_property_index().unwrap();
        assert_eq!(
            engine.get_meta("property-index-format").unwrap().unwrap(),
            b"application value"
        );
        assert_eq!(
            engine
                .db
                .get_cf(engine.cf(cf::META).unwrap(), MARKER)
                .unwrap()
                .unwrap(),
            VERSION
        );
    }

    #[test]
    fn recursive_fingerprints_preserve_equality_and_type_boundaries() {
        assert_eq!(
            hash_property_value(&value(false)),
            hash_property_value(&value(true))
        );
        for (a, b) in [
            (
                PropertyValue::Point2D {
                    x: -0.0,
                    y: 0.0,
                    srid: 1,
                },
                PropertyValue::Point2D {
                    x: 0.0,
                    y: -0.0,
                    srid: 1,
                },
            ),
            (
                PropertyValue::Point3D {
                    x: -0.0,
                    y: 0.0,
                    z: -0.0,
                    srid: 1,
                },
                PropertyValue::Point3D {
                    x: 0.0,
                    y: -0.0,
                    z: 0.0,
                    srid: 1,
                },
            ),
        ] {
            assert_eq!(a, b);
            assert_eq!(hash_property_value(&a), hash_property_value(&b));
        }
        assert_ne!(
            hash_property_value(&PropertyValue::Integer(0)),
            hash_property_value(&PropertyValue::Float(0.0))
        );
        assert_ne!(
            hash_property_value(&PropertyValue::Array(vec![1.into(), 2.into()])),
            hash_property_value(&PropertyValue::Array(vec![2.into(), 1.into()]))
        );
        // The byte stream and fingerprint are persistent format, not DefaultHasher.
        assert_eq!(
            hash_property_value(&PropertyValue::Null),
            0xaf63bd4c8601b7df
        );
    }
    #[test]
    fn legacy_and_partial_indexes_rebuild_then_updates_and_deletes_remain_clean() {
        let dir = tempfile::tempdir().unwrap();
        let options = StorageOptions::new(dir.path());
        let graph = GraphId::from_internal(42);
        let id = NodeId::from_internal(8);
        {
            let engine = StorageEngine::open(options.clone()).unwrap();
            let mut node = Node::with_labels(id, ["Evidence"]);
            node.set_property("attributes", value(false));
            engine.put_node(graph, &node).unwrap();
            // Simulate arbitrary legacy map hash and an interrupted rebuild's stale row.
            let cf = engine.cf(cf::PROPERTY_INDEX).unwrap();
            engine
                .db
                .delete_cf(
                    cf,
                    KeyBuilder::property_index(
                        graph,
                        "Evidence",
                        "attributes",
                        hash_property_value(&value(false)),
                        id.as_internal(),
                    ),
                )
                .unwrap();
            engine
                .db
                .put_cf(
                    cf,
                    KeyBuilder::property_index(
                        graph,
                        "Evidence",
                        "attributes",
                        123,
                        id.as_internal(),
                    ),
                    b"legacy",
                )
                .unwrap();
            engine
                .db
                .put_cf(
                    cf,
                    KeyBuilder::property_index(graph, "Evidence", "obsolete", 456, 999),
                    b"partial",
                )
                .unwrap();
            engine
                .db
                .delete_cf(engine.cf(cf::META).unwrap(), MARKER)
                .unwrap();
        }
        {
            let engine = StorageEngine::open(options.clone()).unwrap();
            let found = engine
                .get_nodes_by_property(graph, "Evidence", "attributes", &value(true))
                .unwrap();
            assert_eq!(found.len(), 1);
            assert_eq!(
                engine
                    .db
                    .iterator_cf(engine.cf(cf::PROPERTY_INDEX).unwrap(), IteratorMode::Start)
                    .count(),
                1
            );
            let mut node = found[0].clone();
            node.set_property("attributes", PropertyValue::Integer(99));
            engine.put_node(graph, &node).unwrap();
            assert!(
                engine
                    .get_nodes_by_property(graph, "Evidence", "attributes", &value(false))
                    .unwrap()
                    .is_empty()
            );
            assert_eq!(
                engine
                    .db
                    .iterator_cf(engine.cf(cf::PROPERTY_INDEX).unwrap(), IteratorMode::Start)
                    .count(),
                1
            );
            engine.delete_node(graph, id).unwrap();
        }
        let engine = StorageEngine::open(options).unwrap();
        assert_eq!(
            engine
                .db
                .iterator_cf(engine.cf(cf::PROPERTY_INDEX).unwrap(), IteratorMode::Start)
                .count(),
            0
        );
    }
    #[test]
    fn unknown_format_and_corrupt_source_fail_closed_without_completion_marker() {
        let dir = tempfile::tempdir().unwrap();
        let options = StorageOptions::new(dir.path());
        {
            let engine = StorageEngine::open(options.clone()).unwrap();
            engine
                .db
                .put_cf(engine.cf(cf::META).unwrap(), MARKER, b"future-format")
                .unwrap();
            assert!(engine.ensure_property_index().is_err());
        }
        assert!(StorageEngine::open(options.clone()).is_err());
        // Repair the test fixture directly; production repair requires operator review.
        let descriptors = super::super::COLUMN_FAMILIES
            .iter()
            .map(|n| rocksdb::ColumnFamilyDescriptor::new(*n, rocksdb::Options::default()));
        {
            let db = rocksdb::DB::open_cf_descriptors(
                &rocksdb::Options::default(),
                dir.path(),
                descriptors,
            )
            .unwrap();
            db.delete_cf(db.cf_handle(cf::META).unwrap(), MARKER)
                .unwrap();
            db.put_cf(
                db.cf_handle(cf::NODES).unwrap(),
                KeyBuilder::node(GraphId::from_internal(1), NodeId::from_internal(1)),
                b"invalid",
            )
            .unwrap();
        }
        assert!(StorageEngine::open(options.clone()).is_err());
        assert!(StorageEngine::open(options).is_err());
    }
}

#[cfg(test)]
fn migration_test_barrier() {
    if let Some(path) = std::env::var_os("QILBEEDB_TEST_MIGRATION_BARRIER") {
        std::fs::write(path, b"durable partial batch").unwrap();
        loop {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
}

#[cfg(test)]
mod crash_tests {
    use super::*;
    use crate::StorageOptions;
    use qilbee_core::NodeId;
    use std::process::{Command, Stdio};
    #[test]
    fn migration_child() {
        let Some(path) = std::env::var_os("QILBEEDB_TEST_MIGRATION_DB") else {
            return;
        };
        // Parent kills this process only after a synced partial reconstruction.
        StorageEngine::open(StorageOptions::new(path)).unwrap();
        panic!("Child did not stop at migration barrier");
    }
    #[test]
    fn killed_partial_reconstruction_restarts_and_preserves_authoritative_nodes() {
        let dir = tempfile::tempdir().unwrap();
        let options = StorageOptions::new(dir.path().join("db"));
        let graph = GraphId::from_internal(73);
        let content = PropertyValue::String("x".repeat(8192));
        {
            let engine = StorageEngine::open(options.clone()).unwrap();
            let mut batch = WriteBatch::default();
            for id in 1..=300 {
                let mut node = Node::with_labels(NodeId::from_internal(id), ["Evidence"]);
                node.set_property("payload", content.clone());
                // A pre-upgrade fixture with authoritative nodes and missing index.
                batch.put_cf(
                    engine.cf(cf::NODES).unwrap(),
                    KeyBuilder::node(graph, node.id),
                    bincode::serialize(&node).unwrap(),
                );
            }
            batch.delete_cf(engine.cf(cf::META).unwrap(), MARKER);
            let mut sync = WriteOptions::default();
            sync.set_sync(true);
            engine.db.write_opt(batch, &sync).unwrap();
        }
        let barrier = dir.path().join("barrier");
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "engine::property_index::crash_tests::migration_child",
                "--nocapture",
            ])
            .env("QILBEEDB_TEST_MIGRATION_DB", &options.path)
            .env("QILBEEDB_TEST_MIGRATION_BARRIER", &barrier)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(45);
        while !barrier.exists() {
            if let Some(status) = child.try_wait().unwrap() {
                panic!("Migration child exited before barrier: {status}");
            }
            if std::time::Instant::now() > deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("Migration child did not reach barrier");
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        child.kill().unwrap();
        assert!(!child.wait().unwrap().success());
        let engine = StorageEngine::open(options.clone()).unwrap();
        let nodes = engine
            .get_nodes_by_property(graph, "Evidence", "payload", &content)
            .unwrap();
        assert_eq!(nodes.len(), 300);
        assert_eq!(
            engine
                .db
                .iterator_cf(engine.cf(cf::PROPERTY_INDEX).unwrap(), IteratorMode::Start)
                .count(),
            300
        );
        assert_eq!(
            engine
                .db
                .get_cf(engine.cf(cf::META).unwrap(), MARKER)
                .unwrap()
                .unwrap(),
            VERSION
        );
        drop(engine);
        let reopened = StorageEngine::open(options).unwrap();
        assert_eq!(
            reopened
                .get_nodes_by_property(graph, "Evidence", "payload", &content)
                .unwrap()
                .len(),
            300
        );
    }
}
