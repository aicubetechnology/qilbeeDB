//! Conditional metadata mutations for durable platform control-plane records.

/// Exact value expected at one metadata key; `None` requires absence.
#[derive(Debug, Clone)]
pub struct MetadataCondition {
    pub key: String,
    pub expected: Option<Vec<u8>>,
}

/// Mutation applied with all other writes; `None` deletes the key.
#[derive(Debug, Clone)]
pub struct MetadataWrite {
    pub key: String,
    pub value: Option<Vec<u8>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{StorageEngine, StorageOptions};
    use std::sync::{Arc, Barrier};
    use tempfile::TempDir;

    fn condition(key: &str, value: Option<&[u8]>) -> MetadataCondition {
        MetadataCondition {
            key: key.into(),
            expected: value.map(Vec::from),
        }
    }
    fn write(key: &str, value: Option<&[u8]>) -> MetadataWrite {
        MetadataWrite {
            key: key.into(),
            value: value.map(Vec::from),
        }
    }
    #[test]
    fn metadata_conditional_batch_is_all_or_nothing_and_survives_reopen() {
        let dir = TempDir::new().unwrap();
        {
            let db = StorageEngine::open(StorageOptions::for_testing(dir.path())).unwrap();
            assert!(
                db.compare_and_write_meta(
                    &[condition("a", None), condition("b", None)],
                    &[write("a", Some(b"1")), write("b", Some(b"2"))]
                )
                .unwrap()
            );
            assert!(
                !db.compare_and_write_meta(
                    &[condition("a", Some(b"wrong")), condition("b", Some(b"2"))],
                    &[write("a", None), write("b", Some(b"3"))]
                )
                .unwrap()
            );
            assert_eq!(db.get_meta("a").unwrap().as_deref(), Some(b"1".as_slice()));
        }
        let db = StorageEngine::open(StorageOptions::for_testing(dir.path())).unwrap();
        assert_eq!(db.get_meta("b").unwrap().as_deref(), Some(b"2".as_slice()));
        assert!(
            db.compare_and_write_meta(
                &[condition("a", Some(b"1")), condition("b", Some(b"2"))],
                &[write("b", None)]
            )
            .unwrap()
        );
        assert!(db.get_meta("b").unwrap().is_none());
    }
    #[test]
    fn metadata_concurrent_conditional_writers_have_one_winner() {
        let dir = TempDir::new().unwrap();
        let db = StorageEngine::open(StorageOptions::for_testing(dir.path())).unwrap();
        let gate = Arc::new(Barrier::new(12));
        let threads: Vec<_> = (0..12)
            .map(|_| {
                let db = db.clone();
                let gate = gate.clone();
                std::thread::spawn(move || {
                    gate.wait();
                    db.compare_and_write_meta(
                        &[condition("winner", None)],
                        &[write("winner", Some(b"yes"))],
                    )
                    .unwrap()
                })
            })
            .collect();
        assert_eq!(
            threads
                .into_iter()
                .map(|t| t.join().unwrap())
                .filter(|won| *won)
                .count(),
            1
        );
    }
    #[test]
    fn metadata_rejects_unguarded_and_ambiguous_writes() {
        let dir = TempDir::new().unwrap();
        let db = StorageEngine::open(StorageOptions::for_testing(dir.path())).unwrap();
        assert!(
            db.compare_and_write_meta(&[], &[write("a", Some(b"1"))])
                .is_err()
        );
        assert!(
            db.compare_and_write_meta(
                &[condition("a", None)],
                &[write("a", None), write("a", Some(b"1"))]
            )
            .is_err()
        );
        assert!(
            db.compare_and_write_meta(&[condition("a", None), condition("a", None)], &[])
                .is_err()
        );
        assert!(db.get_meta("a").unwrap().is_none());
    }
}
