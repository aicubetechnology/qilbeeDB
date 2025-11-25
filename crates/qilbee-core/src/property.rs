//! Property types for graph entities
//!
//! Provides property values and collections for nodes and relationships.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A property value that can be stored on nodes and relationships
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PropertyValue {
    /// Null/missing value
    Null,

    /// Boolean value
    Boolean(bool),

    /// 64-bit signed integer
    Integer(i64),

    /// 64-bit floating point
    Float(f64),

    /// UTF-8 string
    String(String),

    /// Array of property values (homogeneous)
    Array(Vec<PropertyValue>),

    /// Map of string keys to property values
    Map(HashMap<String, PropertyValue>),

    /// Binary data (for embeddings, etc.)
    Bytes(Vec<u8>),

    /// Date (days since epoch)
    Date(i32),

    /// Time (nanoseconds since midnight)
    Time(i64),

    /// DateTime (milliseconds since Unix epoch)
    DateTime(i64),

    /// Duration in nanoseconds
    Duration(i64),

    /// Point in 2D space
    Point2D { x: f64, y: f64, srid: u32 },

    /// Point in 3D space
    Point3D { x: f64, y: f64, z: f64, srid: u32 },
}

impl PropertyValue {
    /// Returns true if the value is null
    pub fn is_null(&self) -> bool {
        matches!(self, PropertyValue::Null)
    }

    /// Returns true if the value is a boolean
    pub fn is_boolean(&self) -> bool {
        matches!(self, PropertyValue::Boolean(_))
    }

    /// Returns true if the value is an integer
    pub fn is_integer(&self) -> bool {
        matches!(self, PropertyValue::Integer(_))
    }

    /// Returns true if the value is a float
    pub fn is_float(&self) -> bool {
        matches!(self, PropertyValue::Float(_))
    }

    /// Returns true if the value is a string
    pub fn is_string(&self) -> bool {
        matches!(self, PropertyValue::String(_))
    }

    /// Returns true if the value is an array
    pub fn is_array(&self) -> bool {
        matches!(self, PropertyValue::Array(_))
    }

    /// Returns true if the value is a map
    pub fn is_map(&self) -> bool {
        matches!(self, PropertyValue::Map(_))
    }

    /// Try to get as boolean
    pub fn as_boolean(&self) -> Option<bool> {
        match self {
            PropertyValue::Boolean(b) => Some(*b),
            _ => None,
        }
    }

    /// Try to get as integer
    pub fn as_integer(&self) -> Option<i64> {
        match self {
            PropertyValue::Integer(i) => Some(*i),
            _ => None,
        }
    }

    /// Try to get as float
    pub fn as_float(&self) -> Option<f64> {
        match self {
            PropertyValue::Float(f) => Some(*f),
            PropertyValue::Integer(i) => Some(*i as f64),
            _ => None,
        }
    }

    /// Try to get as string reference
    pub fn as_str(&self) -> Option<&str> {
        match self {
            PropertyValue::String(s) => Some(s),
            _ => None,
        }
    }

    /// Try to get as array reference
    pub fn as_array(&self) -> Option<&Vec<PropertyValue>> {
        match self {
            PropertyValue::Array(arr) => Some(arr),
            _ => None,
        }
    }

    /// Try to get as map reference
    pub fn as_map(&self) -> Option<&HashMap<String, PropertyValue>> {
        match self {
            PropertyValue::Map(map) => Some(map),
            _ => None,
        }
    }

    /// Try to get as bytes reference
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            PropertyValue::Bytes(bytes) => Some(bytes),
            _ => None,
        }
    }

    /// Get the type name of this value
    pub fn type_name(&self) -> &'static str {
        match self {
            PropertyValue::Null => "null",
            PropertyValue::Boolean(_) => "boolean",
            PropertyValue::Integer(_) => "integer",
            PropertyValue::Float(_) => "float",
            PropertyValue::String(_) => "string",
            PropertyValue::Array(_) => "array",
            PropertyValue::Map(_) => "map",
            PropertyValue::Bytes(_) => "bytes",
            PropertyValue::Date(_) => "date",
            PropertyValue::Time(_) => "time",
            PropertyValue::DateTime(_) => "datetime",
            PropertyValue::Duration(_) => "duration",
            PropertyValue::Point2D { .. } => "point2d",
            PropertyValue::Point3D { .. } => "point3d",
        }
    }
}

// Convenience From implementations
impl From<bool> for PropertyValue {
    fn from(v: bool) -> Self {
        PropertyValue::Boolean(v)
    }
}

impl From<i64> for PropertyValue {
    fn from(v: i64) -> Self {
        PropertyValue::Integer(v)
    }
}

impl From<i32> for PropertyValue {
    fn from(v: i32) -> Self {
        PropertyValue::Integer(v as i64)
    }
}

impl From<f64> for PropertyValue {
    fn from(v: f64) -> Self {
        PropertyValue::Float(v)
    }
}

impl From<String> for PropertyValue {
    fn from(v: String) -> Self {
        PropertyValue::String(v)
    }
}

impl From<&str> for PropertyValue {
    fn from(v: &str) -> Self {
        PropertyValue::String(v.to_string())
    }
}

impl From<Vec<u8>> for PropertyValue {
    fn from(v: Vec<u8>) -> Self {
        PropertyValue::Bytes(v)
    }
}

impl<T: Into<PropertyValue>> From<Vec<T>> for PropertyValue {
    fn from(v: Vec<T>) -> Self {
        PropertyValue::Array(v.into_iter().map(Into::into).collect())
    }
}

/// A collection of properties
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Property {
    inner: HashMap<String, PropertyValue>,
}

impl Property {
    /// Create an empty property collection
    pub fn new() -> Self {
        Self {
            inner: HashMap::new(),
        }
    }

    /// Create with a single property
    pub fn with<K: Into<String>, V: Into<PropertyValue>>(key: K, value: V) -> Self {
        let mut props = Self::new();
        props.set(key, value);
        props
    }

    /// Set a property value
    pub fn set<K: Into<String>, V: Into<PropertyValue>>(&mut self, key: K, value: V) {
        self.inner.insert(key.into(), value.into());
    }

    /// Get a property value
    pub fn get(&self, key: &str) -> Option<&PropertyValue> {
        self.inner.get(key)
    }

    /// Remove a property
    pub fn remove(&mut self, key: &str) -> Option<PropertyValue> {
        self.inner.remove(key)
    }

    /// Check if a property exists
    pub fn contains(&self, key: &str) -> bool {
        self.inner.contains_key(key)
    }

    /// Get the number of properties
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Iterate over properties
    pub fn iter(&self) -> impl Iterator<Item = (&String, &PropertyValue)> {
        self.inner.iter()
    }

    /// Get property keys
    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.inner.keys()
    }

    /// Merge with another property collection (other takes precedence)
    pub fn merge(&mut self, other: Property) {
        self.inner.extend(other.inner);
    }

    /// Convert to HashMap
    pub fn into_inner(self) -> HashMap<String, PropertyValue> {
        self.inner
    }
}

impl IntoIterator for Property {
    type Item = (String, PropertyValue);
    type IntoIter = std::collections::hash_map::IntoIter<String, PropertyValue>;

    fn into_iter(self) -> Self::IntoIter {
        self.inner.into_iter()
    }
}

impl FromIterator<(String, PropertyValue)> for Property {
    fn from_iter<I: IntoIterator<Item = (String, PropertyValue)>>(iter: I) -> Self {
        Self {
            inner: iter.into_iter().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_property_value_types() {
        assert!(PropertyValue::Null.is_null());
        assert!(PropertyValue::Boolean(true).is_boolean());
        assert!(PropertyValue::Integer(42).is_integer());
        assert!(PropertyValue::Float(3.14).is_float());
        assert!(PropertyValue::String("test".into()).is_string());
    }

    #[test]
    fn test_property_value_conversions() {
        assert_eq!(PropertyValue::Boolean(true).as_boolean(), Some(true));
        assert_eq!(PropertyValue::Integer(42).as_integer(), Some(42));
        assert_eq!(PropertyValue::Float(3.14).as_float(), Some(3.14));
        assert_eq!(PropertyValue::Integer(42).as_float(), Some(42.0));
        assert_eq!(
            PropertyValue::String("test".into()).as_str(),
            Some("test")
        );
    }

    #[test]
    fn test_property_from_impls() {
        let _: PropertyValue = true.into();
        let _: PropertyValue = 42i64.into();
        let _: PropertyValue = 3.14f64.into();
        let _: PropertyValue = "test".into();
        let _: PropertyValue = String::from("test").into();
    }

    #[test]
    fn test_property_collection() {
        let mut props = Property::new();
        props.set("name", "Alice");
        props.set("age", 30i64);

        assert_eq!(props.len(), 2);
        assert!(props.contains("name"));
        assert_eq!(props.get("name").and_then(|v| v.as_str()), Some("Alice"));
        assert_eq!(props.get("age").and_then(|v| v.as_integer()), Some(30));
    }

    #[test]
    fn test_property_with() {
        let props = Property::with("key", "value");
        assert_eq!(props.len(), 1);
        assert_eq!(props.get("key").and_then(|v| v.as_str()), Some("value"));
    }

    #[test]
    fn test_property_merge() {
        let mut props1 = Property::with("a", "1");
        let mut props2 = Property::new();
        props2.set("b", "2");
        props2.set("a", "overwritten");

        props1.merge(props2);

        assert_eq!(props1.get("a").and_then(|v| v.as_str()), Some("overwritten"));
        assert_eq!(props1.get("b").and_then(|v| v.as_str()), Some("2"));
    }
}
