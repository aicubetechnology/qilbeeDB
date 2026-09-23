//! Canonical bytes for new knowledge-origin hash domains only.
//!
//! Existing knowledge receipts retain their typed-field serialization. Never
//! pass an existing receipt through this encoder to replace its established hash.
use qilbee_core::{Error, Result};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

pub(super) enum OriginHashDomain {
    Origin,
    Receipt,
    CatalogBinding,
}

impl OriginHashDomain {
    fn label(&self) -> &'static str {
        match self {
            Self::Origin => "qilbee.experience-knowledge.origin.v1",
            Self::Receipt => "qilbee.experience-knowledge.receipt.v1",
            Self::CatalogBinding => "qilbee.learning.catalog.origin-binding.v2",
        }
    }
}

/// Call only with validated typed payloads, never unvalidated HTTP JSON.
/// Sorting is explicit and does not depend on serde_json's map feature flags.
pub(super) fn digest<T: Serialize>(domain: OriginHashDomain, payload: &T) -> Result<String> {
    let value =
        serde_json::to_value(payload).map_err(|error| Error::Serialization(error.to_string()))?;
    let mut bytes = Vec::new();
    bytes.push(b'[');
    encode_string(domain.label(), &mut bytes)?;
    bytes.push(b',');
    encode_value(&value, &mut bytes)?;
    bytes.push(b']');
    Ok(format!("{:x}", Sha256::digest(&bytes)))
}

fn encode_string(value: &str, output: &mut Vec<u8>) -> Result<()> {
    serde_json::to_writer(output, value).map_err(|error| Error::Serialization(error.to_string()))
}

fn encode_value(value: &Value, output: &mut Vec<u8>) -> Result<()> {
    match value {
        Value::Null => output.extend_from_slice(b"null"),
        Value::Bool(true) => output.extend_from_slice(b"true"),
        Value::Bool(false) => output.extend_from_slice(b"false"),
        Value::Number(number) => {
            let integer = number.as_u64().ok_or_else(|| {
                Error::ValidationError("Origin hashes require unsigned integer numbers".into())
            })?;
            output.extend_from_slice(integer.to_string().as_bytes());
        }
        Value::String(text) => encode_string(text, output)?,
        Value::Array(values) => {
            output.push(b'[');
            for (index, item) in values.iter().enumerate() {
                if index != 0 {
                    output.push(b',');
                }
                encode_value(item, output)?;
            }
            output.push(b']');
        }
        Value::Object(values) => {
            let mut entries: Vec<_> = values.iter().collect();
            entries.sort_unstable_by(|(left, _), (right, _)| left.as_bytes().cmp(right.as_bytes()));
            output.push(b'{');
            for (index, (key, item)) in entries.into_iter().enumerate() {
                if !key.is_ascii() {
                    return Err(Error::ValidationError(
                        "Origin hash keys must be fixed ASCII identifiers".into(),
                    ));
                }
                if index != 0 {
                    output.push(b',');
                }
                encode_string(key, output)?;
                output.push(b':');
                encode_value(item, output)?;
            }
            output.push(b'}');
        }
    }
    Ok(())
}
