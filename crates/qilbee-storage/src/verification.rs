//! Offline, read-only verification helpers for stopped RocksDB stores.
//!
//! These helpers never write to a store. They refuse to inspect a store that
//! another process holds open, require the exact column-family set a binary
//! expects, and compute deterministic per-family inventories that operators can
//! compare between a source and its restored or migrated copy.

use qilbee_core::{Error, Result};
use rocksdb::{DB, IteratorMode, Options};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

/// Domain separator hashed before any inventory bytes.
pub const INVENTORY_DOMAIN: &[u8] = b"qilbee-store-inventory-v1\0";

/// Deterministic summary of one column family.
///
/// `sha256` covers every authoritative record as
/// `len(key) || key || len(value) || value` (big-endian u64 lengths) after the
/// domain separator. Derived entries selected by the caller are counted in
/// `derived_entries` and excluded from `records` and the digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FamilyInventory {
    pub family: String,
    pub records: u64,
    pub sha256: String,
    pub derived_entries: u64,
}

/// Refuse to continue while another process holds the RocksDB store lock.
///
/// RocksDB guards a store with a POSIX record lock on its `LOCK` file. This
/// probe uses `F_GETLK`, which acquires and releases nothing, so it cannot
/// disturb a live server. POSIX record locks are per process: a store opened by
/// the calling process itself is not detected and remains the caller's error.
pub fn require_stopped(path: &Path) -> Result<()> {
    let lock_path = path.join("LOCK");
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&lock_path)
        .map_err(|error| {
            Error::Storage(format!(
                "Store lock file {} is unavailable: {error}",
                lock_path.display()
            ))
        })?;
    probe_writer(&file).and_then(|holder| match holder {
        None => Ok(()),
        Some(pid) => Err(Error::Storage(format!(
            "Store at {} is open by process {pid}; stop it before verification",
            path.display()
        ))),
    })
}

#[cfg(unix)]
fn probe_writer(file: &std::fs::File) -> Result<Option<i32>> {
    use std::os::unix::io::AsRawFd;
    // SAFETY: `flock` is plain data; every field is initialized before use and
    // the descriptor stays open for the duration of the call.
    let mut lock: libc::flock = unsafe { std::mem::zeroed() };
    lock.l_type = libc::F_WRLCK as libc::c_short;
    lock.l_whence = libc::SEEK_SET as libc::c_short;
    lock.l_start = 0;
    lock.l_len = 0;
    let status = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETLK, &mut lock) };
    if status == -1 {
        return Err(Error::Storage(format!(
            "Store lock probe failed: {}",
            std::io::Error::last_os_error()
        )));
    }
    if lock.l_type == libc::F_UNLCK as libc::c_short {
        Ok(None)
    } else {
        Ok(Some(lock.l_pid as i32))
    }
}

#[cfg(not(unix))]
fn probe_writer(_file: &std::fs::File) -> Result<Option<i32>> {
    Err(Error::Configuration(
        "Offline store verification requires a POSIX platform".into(),
    ))
}

/// List the column families of a stopped store and require exactly `expected`.
///
/// `expected` must include RocksDB's `default` family. Missing families mean an
/// incomplete or older store; extra families mean a newer or foreign writer.
/// Both are rejected: verification never guesses a compatible subset.
pub fn require_families(path: &Path, expected: &[&str]) -> Result<Vec<String>> {
    if !path.join("CURRENT").is_file() {
        return Err(Error::Storage(format!(
            "No RocksDB store at {}",
            path.display()
        )));
    }
    let mut actual = DB::list_cf(&Options::default(), path).map_err(storage_error)?;
    actual.sort();
    let mut wanted: Vec<String> = expected.iter().map(|name| (*name).to_owned()).collect();
    wanted.sort();
    wanted.dedup();
    if actual != wanted {
        return Err(Error::DataCorruption(format!(
            "Store at {} has column families {:?}; this binary expects {:?}",
            path.display(),
            actual,
            wanted
        )));
    }
    Ok(actual)
}

/// Open a stopped store read-only with the exact expected column families.
pub fn open_read_only(path: &Path, expected: &[&str]) -> Result<DB> {
    require_stopped(path)?;
    let families = require_families(path, expected)?;
    let mut options = Options::default();
    options.create_if_missing(false);
    options.create_missing_column_families(false);
    // Read-only opens still replay the write-ahead log. The default
    // point-in-time recovery silently drops everything after a damaged record,
    // which would let a truncated copy verify. Any WAL damage is a failure.
    options.set_wal_recovery_mode(rocksdb::DBRecoveryMode::AbsoluteConsistency);
    options.set_paranoid_checks(true);
    DB::open_cf_for_read_only(&options, path, families, false).map_err(storage_error)
}

/// Inventory one column family; `derived` selects entries to count but not hash.
pub fn family_inventory(
    db: &DB,
    family: &str,
    derived: impl Fn(&[u8]) -> bool,
) -> Result<FamilyInventory> {
    // A store opened without explicit descriptors exposes no handle for
    // `default`; its rows are still reachable through the plain iterator.
    let iterator = match (db.cf_handle(family), family) {
        (Some(handle), _) => db.iterator_cf(handle, IteratorMode::Start),
        (None, "default") => db.iterator(IteratorMode::Start),
        (None, _) => {
            return Err(Error::Internal(format!(
                "Column family not found: {family}"
            )));
        }
    };
    let mut digest = Sha256::new();
    digest.update(INVENTORY_DOMAIN);
    let mut records = 0u64;
    let mut derived_entries = 0u64;
    for item in iterator {
        let (key, value) = item.map_err(storage_error)?;
        if derived(&key) {
            derived_entries += 1;
            continue;
        }
        digest.update((key.len() as u64).to_be_bytes());
        digest.update(&key);
        digest.update((value.len() as u64).to_be_bytes());
        digest.update(&value);
        records += 1;
    }
    Ok(FamilyInventory {
        family: family.to_owned(),
        records,
        sha256: format!("{:x}", digest.finalize()),
        derived_entries,
    })
}

fn storage_error(error: rocksdb::Error) -> Error {
    Error::Storage(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{StorageEngine, StorageOptions};
    use std::time::{Duration, Instant};

    const LOCK_HOLDER_FIXTURE: &str = "QILBEEDB_STORAGE_LOCK_HOLDER_FIXTURE";

    fn write(db: &DB, key: &[u8], value: &[u8]) {
        db.put(key, value).unwrap();
    }

    fn expected_digest(entries: &[(&[u8], &[u8])]) -> String {
        let mut digest = Sha256::new();
        digest.update(INVENTORY_DOMAIN);
        for (key, value) in entries {
            digest.update((key.len() as u64).to_be_bytes());
            digest.update(key);
            digest.update((value.len() as u64).to_be_bytes());
            digest.update(value);
        }
        format!("{:x}", digest.finalize())
    }

    #[test]
    fn require_stopped_rejects_directories_without_a_store() {
        let dir = tempfile::TempDir::new().unwrap();
        assert!(matches!(
            require_stopped(dir.path()),
            Err(Error::Storage(message)) if message.contains("lock file")
        ));
        assert!(matches!(
            require_families(dir.path(), &["default"]),
            Err(Error::Storage(message)) if message.contains("No RocksDB store")
        ));
    }

    #[test]
    fn require_stopped_detects_a_writer_in_another_process() {
        if let Some(directory) = std::env::var_os(LOCK_HOLDER_FIXTURE) {
            let directory = std::path::PathBuf::from(directory);
            let _engine =
                StorageEngine::open(StorageOptions::for_testing(directory.join("store"))).unwrap();
            std::fs::write(directory.join("ready"), b"1").unwrap();
            while !directory.join("stop").exists() {
                std::thread::sleep(Duration::from_millis(20));
            }
            std::process::exit(0);
        }
        let dir = tempfile::TempDir::new().unwrap();
        let store = dir.path().join("store");
        drop(StorageEngine::open(StorageOptions::for_testing(&store)).unwrap());
        assert!(require_stopped(&store).is_ok());
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "verification::tests::require_stopped_detects_a_writer_in_another_process",
                "--nocapture",
            ])
            .env(LOCK_HOLDER_FIXTURE, dir.path())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(60);
        while !dir.path().join("ready").exists() {
            assert!(Instant::now() < deadline, "lock holder never became ready");
            std::thread::sleep(Duration::from_millis(20));
        }
        let held = require_stopped(&store);
        let opened = open_read_only(&store, &["default"]);
        std::fs::write(dir.path().join("stop"), b"1").unwrap();
        let status = child.wait().unwrap();
        assert!(status.success());
        assert!(
            matches!(&held, Err(Error::Storage(message)) if message.contains("open by process")),
            "{held:?}"
        );
        assert!(matches!(opened, Err(Error::Storage(_))));
        assert!(require_stopped(&store).is_ok());
    }

    #[test]
    fn read_only_open_requires_the_exact_family_set_and_never_writes() {
        let dir = tempfile::TempDir::new().unwrap();
        drop(StorageEngine::open(StorageOptions::for_testing(dir.path())).unwrap());
        let mut families = vec!["default"];
        families.extend_from_slice(crate::engine::COLUMN_FAMILIES);
        let db = open_read_only(dir.path(), &families).unwrap();
        assert!(
            db.put(b"k", b"v").is_err(),
            "read-only handles must refuse writes"
        );
        drop(db);
        let missing = open_read_only(dir.path(), &["default", "nodes"]);
        assert!(
            matches!(missing, Err(Error::DataCorruption(_))),
            "{missing:?}"
        );
        let mut extra = families.clone();
        extra.push("unknown_family");
        let extra = open_read_only(dir.path(), &extra);
        assert!(matches!(extra, Err(Error::DataCorruption(_))), "{extra:?}");
        // Same-process writer: documented limitation, must not deadlock or panic.
        let engine = StorageEngine::open(StorageOptions::for_testing(dir.path())).unwrap();
        assert!(require_stopped(dir.path()).is_ok());
        drop(engine);
    }

    #[test]
    fn inventory_digest_is_deterministic_order_independent_and_sensitive() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut options = Options::default();
        options.create_if_missing(true);
        let db = DB::open(&options, dir.path()).unwrap();
        write(&db, b"b", b"2");
        write(&db, b"a", b"1");
        write(&db, b"\0derived", b"x");
        let inventory = family_inventory(&db, "default", |key| key.starts_with(b"\0")).unwrap();
        assert_eq!(
            inventory,
            FamilyInventory {
                family: "default".into(),
                records: 2,
                sha256: expected_digest(&[(b"a", b"1"), (b"b", b"2")]),
                derived_entries: 1,
            }
        );
        let physical = family_inventory(&db, "default", |_| false).unwrap();
        assert_eq!(physical.records, 3);
        assert_eq!(physical.derived_entries, 0);
        assert_ne!(physical.sha256, inventory.sha256);
        write(&db, b"a", b"changed");
        let changed = family_inventory(&db, "default", |key| key.starts_with(b"\0")).unwrap();
        assert_eq!(changed.records, 2);
        assert_ne!(changed.sha256, inventory.sha256);
        assert!(family_inventory(&db, "absent", |_| false).is_err());
        drop(db);
        let reopened = open_read_only(dir.path(), &["default"]).unwrap();
        assert_eq!(
            family_inventory(&reopened, "default", |key| key.starts_with(b"\0")).unwrap(),
            changed
        );
    }

    #[test]
    fn read_only_open_rejects_a_damaged_write_ahead_log_instead_of_truncating() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut options = Options::default();
        options.create_if_missing(true);
        let db = DB::open(&options, dir.path()).unwrap();
        for number in 0..64u32 {
            write(&db, &number.to_be_bytes(), &[b'v'; 512]);
        }
        drop(db);
        let intact = open_read_only(dir.path(), &["default"]).unwrap();
        assert_eq!(
            family_inventory(&intact, "default", |_| false)
                .unwrap()
                .records,
            64
        );
        drop(intact);
        // Everything still lives in the write-ahead log; damage its middle.
        let wal = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| path.extension().is_some_and(|ext| ext == "log"))
            .expect("write-ahead log present");
        let size = std::fs::metadata(&wal).unwrap().len();
        assert!(size > 4096, "fixture must span several WAL records");
        {
            use std::io::{Seek, Write};
            let mut file = std::fs::OpenOptions::new().write(true).open(&wal).unwrap();
            file.seek(std::io::SeekFrom::Start(size / 2)).unwrap();
            file.write_all(&[0xff; 64]).unwrap();
        }
        let damaged = open_read_only(dir.path(), &["default"]);
        assert!(
            matches!(&damaged, Err(Error::Storage(_))),
            "a damaged log must not verify as a shorter store: {damaged:?}"
        );
    }

    #[test]
    fn storage_engine_read_only_inventory_covers_every_family() {
        let dir = tempfile::TempDir::new().unwrap();
        drop(StorageEngine::open(StorageOptions::for_testing(dir.path())).unwrap());
        let engine = StorageEngine::open_read_only(dir.path()).unwrap();
        let inventory = engine.inventory().unwrap();
        let mut names: Vec<&str> = inventory.iter().map(|f| f.family.as_str()).collect();
        names.sort();
        let mut expected = vec!["default"];
        expected.extend_from_slice(crate::engine::COLUMN_FAMILIES);
        expected.sort();
        assert_eq!(names, expected);
        assert!(inventory.iter().all(|f| f.derived_entries == 0));
    }
}
