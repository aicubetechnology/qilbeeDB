//! Bounded, read-only audit of the current graph property-index format.
use super::*;
use bincode::Options as _;
use std::collections::BTreeSet;
#[path = "property_index_fingerprint.rs"]
mod fingerprint;
const RECORD_LIMIT: usize = 16 * 1024 * 1024;
const TOTAL_LIMIT: usize = 1024 * 1024 * 1024;
const ENTRY_LIMIT: usize = 2_000_000;
const KEY_LIMIT: usize = 256 * 1024 * 1024;
const MARKER: &[u8] = b"\xf1property-index-format";
const FORMAT: &[u8] = b"canonical-fnv1a64-v1";

/// Counts from a complete current-format property-index audit.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PropertyIndexVerification {
    pub nodes: u64,
    pub index_entries: u64,
}
fn invalid() -> Error {
    Error::DataCorruption(
        "Property index is inconsistent or exceeds offline verification limits".into(),
    )
}
fn storage(error: rocksdb::Error) -> Error {
    Error::Storage(error.to_string())
}

// Preflight the fixed-width bincode grammar before derived deserialization can
// allocate or recurse. Reject duplicate map/label entries rather than allowing
// deserialization into a set/map to silently discard persisted data.
struct Wire<'a> {
    bytes: &'a [u8],
    offset: usize,
    items: usize,
}
impl<'a> Wire<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.offset.checked_add(n).ok_or_else(invalid)?;
        let value = self.bytes.get(self.offset..end).ok_or_else(invalid)?;
        self.offset = end;
        Ok(value)
    }
    fn count(&mut self) -> Result<usize> {
        let n = u64::from_le_bytes(self.take(8)?.try_into().map_err(|_| invalid())?);
        let n = usize::try_from(n).map_err(|_| invalid())?;
        self.items = self.items.checked_add(n).ok_or_else(invalid)?;
        if self.items > 1_000_000 {
            return Err(invalid());
        }
        Ok(n)
    }
    fn blob(&mut self) -> Result<&'a [u8]> {
        let n = u64::from_le_bytes(self.take(8)?.try_into().map_err(|_| invalid())?);
        self.take(usize::try_from(n).map_err(|_| invalid())?)
    }
    fn string(&mut self) -> Result<&'a [u8]> {
        let b = self.blob()?;
        std::str::from_utf8(b).map_err(|_| invalid())?;
        Ok(b)
    }
    fn map(&mut self, depth: usize) -> Result<()> {
        let n = self.count()?;
        let mut keys = BTreeSet::new();
        for _ in 0..n {
            if !keys.insert(self.string()?) {
                return Err(invalid());
            }
            self.value(depth + 1)?;
        }
        Ok(())
    }
    fn value(&mut self, depth: usize) -> Result<()> {
        if depth > 64 {
            return Err(invalid());
        }
        match u32::from_le_bytes(self.take(4)?.try_into().map_err(|_| invalid())?) {
            0 => (),
            1 => {
                if self.take(1)?[0] > 1 {
                    return Err(invalid());
                }
            }
            2 | 3 | 9 | 10 | 11 => {
                self.take(8)?;
            }
            4 => {
                self.string()?;
            }
            5 => {
                let n = self.count()?;
                for _ in 0..n {
                    self.value(depth + 1)?;
                }
            }
            6 => self.map(depth)?,
            7 => {
                self.blob()?;
            }
            8 => {
                self.take(4)?;
            }
            12 => {
                self.take(20)?;
            }
            13 => {
                self.take(28)?;
            }
            _ => return Err(invalid()),
        }
        Ok(())
    }
}
fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8], node: bool) -> Result<T> {
    if bytes.len() > RECORD_LIMIT {
        return Err(invalid());
    }
    let mut wire = Wire {
        bytes,
        offset: 0,
        items: 0,
    };
    if node {
        wire.take(8)?;
        let n = wire.count()?;
        let mut labels = BTreeSet::new();
        for _ in 0..n {
            if !labels.insert(wire.string()?) {
                return Err(invalid());
            }
        }
        wire.map(0)?;
        wire.string()?;
        wire.string()?;
    } else {
        wire.value(0)?;
    }
    if wire.offset != bytes.len() {
        return Err(invalid());
    }
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_limit(RECORD_LIMIT as u64)
        .reject_trailing_bytes()
        .deserialize(bytes)
        .map_err(|_| invalid())
}
fn same(a: &PropertyValue, b: &PropertyValue) -> bool {
    use PropertyValue::*;
    match (a, b) {
        (Float(x), Float(y)) => x.to_bits() == y.to_bits(),
        (Array(x), Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(a, b)| same(a, b)),
        (Map(x), Map(y)) => {
            x.len() == y.len() && x.iter().all(|(k, a)| y.get(k).is_some_and(|b| same(a, b)))
        }
        (
            Point2D {
                x: a,
                y: b,
                srid: c,
            },
            Point2D {
                x: d,
                y: e,
                srid: f,
            },
        ) => a.to_bits() == d.to_bits() && b.to_bits() == e.to_bits() && c == f,
        (
            Point3D {
                x: a,
                y: b,
                z: c,
                srid: d,
            },
            Point3D {
                x: e,
                y: f,
                z: g,
                srid: h,
            },
        ) => {
            a.to_bits() == e.to_bits()
                && b.to_bits() == f.to_bits()
                && c.to_bits() == g.to_bits()
                && d == h
        }
        _ => a == b,
    }
}
fn charge(total: &mut usize, n: usize) -> Result<()> {
    *total = total.checked_add(n).ok_or_else(invalid)?;
    if *total > TOTAL_LIMIT {
        return Err(invalid());
    }
    Ok(())
}
impl StorageEngine {
    /// Audit current-format keys and values without repair. The caller must hold
    /// writer exclusion throughout this audit and publication of its result.
    /// Limits: 16 MiB per value, 1 GiB examined bytes, 2 million nodes/entries,
    /// 256 MiB expected keys, nesting depth 64 and 1 million decoded items/value.
    pub fn verify_property_index(&self) -> Result<PropertyIndexVerification> {
        if self
            .db
            .get_cf(self.cf("meta")?, MARKER)
            .map_err(storage)?
            .as_deref()
            != Some(FORMAT)
        {
            return Err(invalid());
        }
        let index = self.cf("property_index")?;
        let mut expected = BTreeSet::new();
        let mut key_bytes = 0usize;
        let mut total = 0usize;
        let mut nodes = 0usize;
        for row in self
            .db
            .iterator_cf(self.cf("nodes")?, rocksdb::IteratorMode::Start)
        {
            let (key, bytes) = row.map_err(storage)?;
            charge(&mut total, key.len())?;
            charge(&mut total, bytes.len())?;
            nodes += 1;
            if nodes > ENTRY_LIMIT || key.len() != 17 || key[0] != crate::keys::prefix::NODE {
                return Err(invalid());
            }
            let graph = GraphId::from_internal(u64::from_be_bytes(
                key[1..9].try_into().map_err(|_| invalid())?,
            ));
            let node: Node = decode(&bytes, true)?;
            if key.as_ref() != KeyBuilder::node(graph, node.id) {
                return Err(invalid());
            }
            for label in &node.labels {
                for (name, value) in node.properties.iter() {
                    let key = KeyBuilder::property_index(
                        graph,
                        label.name(),
                        name,
                        fingerprint::hash_property_value(value),
                        node.id.as_internal(),
                    );
                    if expected.len() >= ENTRY_LIMIT {
                        return Err(invalid());
                    }
                    key_bytes = key_bytes.checked_add(key.len()).ok_or_else(invalid)?;
                    if key_bytes > KEY_LIMIT || !expected.insert(key.clone()) {
                        return Err(invalid());
                    }
                    let bytes = self
                        .db
                        .get_cf(index, &key)
                        .map_err(storage)?
                        .ok_or_else(invalid)?;
                    charge(&mut total, key.len())?;
                    charge(&mut total, bytes.len())?;
                    let indexed: PropertyValue = decode(&bytes, false)?;
                    if !same(value, &indexed) {
                        return Err(invalid());
                    }
                }
            }
        }
        let count = expected.len();
        for row in self.db.iterator_cf(index, rocksdb::IteratorMode::Start) {
            let (key, bytes) = row.map_err(storage)?;
            charge(&mut total, key.len())?;
            charge(&mut total, bytes.len())?;
            if !expected.remove(key.as_ref()) {
                return Err(invalid());
            }
        }
        if !expected.is_empty() {
            return Err(invalid());
        }
        Ok(PropertyIndexVerification {
            nodes: nodes as u64,
            index_entries: count as u64,
        })
    }
}
