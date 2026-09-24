use super::semantic_tests::{create, open};
use super::*;
use tempfile::TempDir;

const FAMILIES: [&str; 4] = [
    "default",
    "memory_episodes",
    "memory_episode_index",
    "memory_agent_meta",
];

fn anchor(namespace: &str, sequence: u64) -> Vec<u8> {
    let mut key = record_prefix(0x27, namespace);
    key.extend_from_slice(&sequence.to_be_bytes());
    key
}

fn raw(path: &std::path::Path) -> rocksdb::DB {
    let mut options = rocksdb::Options::default();
    options.create_if_missing(false);
    rocksdb::DB::open_cf(&options, path, FAMILIES).unwrap()
}

fn meta(db: &rocksdb::DB) -> &rocksdb::ColumnFamily {
    db.cf_handle("memory_agent_meta").unwrap()
}

fn verify(path: &std::path::Path) -> Result<MemoryJournalVerification> {
    RocksDbMemoryStorage::open_read_only(path)?.verify_memory_journals()
}

fn fixture() -> TempDir {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    for n in 0..3 {
        create(&db, "scope-a", &format!("a-{n}"));
    }
    for n in 0..2 {
        create(&db, "scope-b", &format!("b-{n}"));
    }
    drop(db);
    dir
}

#[test]
fn read_only_store_walks_every_journal_from_baseline_to_tip() {
    let dir = fixture();
    let report = verify(dir.path()).unwrap();
    assert_eq!(
        report,
        MemoryJournalVerification {
            namespaces: 2,
            legacy_journals: 2,
            verified_journals: 2,
            links_checked: 5,
        }
    );
    // An empty store has nothing to walk and is not an error.
    let empty = TempDir::new().unwrap();
    drop(open(empty.path()));
    assert_eq!(
        verify(empty.path()).unwrap(),
        MemoryJournalVerification {
            namespaces: 0,
            legacy_journals: 0,
            verified_journals: 0,
            links_checked: 0,
        }
    );
}

#[test]
fn journal_verification_walks_more_than_one_page() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    for n in 0..300 {
        create(&db, "scope", &format!("event-{n}"));
    }
    drop(db);
    let report = verify(dir.path()).unwrap();
    assert_eq!(report.verified_journals, 1);
    assert_eq!(report.links_checked, 300);
}

#[test]
fn journal_verification_fails_closed_on_broken_chains_and_orphans() {
    let dir = fixture();

    // A tampered anchor breaks the digest of that link.
    let db = raw(dir.path());
    let original = db.get_cf(meta(&db), anchor("scope-a", 2)).unwrap().unwrap();
    let mut tampered = original.clone();
    let position = tampered.len() / 2;
    tampered[position] ^= 0x01;
    db.put_cf(meta(&db), anchor("scope-a", 2), &tampered)
        .unwrap();
    drop(db);
    assert!(matches!(verify(dir.path()), Err(Error::DataCorruption(_))));

    // A missing anchor breaks the walk.
    let db = raw(dir.path());
    db.delete_cf(meta(&db), anchor("scope-a", 2)).unwrap();
    drop(db);
    assert!(matches!(verify(dir.path()), Err(Error::DataCorruption(_))));

    // Restore, then add a dangling anchor past the recorded tip.
    let db = raw(dir.path());
    db.put_cf(meta(&db), anchor("scope-a", 2), &original)
        .unwrap();
    db.put_cf(meta(&db), anchor("scope-a", 4), &original)
        .unwrap();
    drop(db);
    assert!(matches!(verify(dir.path()), Err(Error::DataCorruption(_))));

    // Remove the dangling anchor; the store verifies again.
    let db = raw(dir.path());
    db.delete_cf(meta(&db), anchor("scope-a", 4)).unwrap();
    drop(db);
    assert_eq!(verify(dir.path()).unwrap().links_checked, 5);

    // Anchors whose state record disappeared are orphans, not an empty namespace.
    let db = raw(dir.path());
    db.delete_cf(meta(&db), record_prefix(0x26, "scope-b"))
        .unwrap();
    drop(db);
    assert!(matches!(verify(dir.path()), Err(Error::DataCorruption(_))));
}
