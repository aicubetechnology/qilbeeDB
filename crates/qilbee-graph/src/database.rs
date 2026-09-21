//! Database management for QilbeeDB

use crate::graph::Graph;
use qilbee_core::{Error, Result};
use qilbee_storage::{GraphIdentity, StorageEngine, StorageOptions};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, RwLock};
use tracing::info;

/// The main database instance for QilbeeDB
///
/// Manages multiple graphs and provides access to the storage engine.
pub struct Database {
    /// Storage engine
    storage: StorageEngine,

    /// Active graphs by name
    graphs: Arc<RwLock<HashMap<String, Graph>>>,

    /// Database configuration
    config: DatabaseConfig,
}

/// Configuration for the database
#[derive(Debug, Clone)]
pub struct DatabaseConfig {
    /// Maximum number of graphs
    pub max_graphs: usize,

    /// Default graph name
    pub default_graph: String,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            max_graphs: 10000,
            default_graph: "default".to_string(),
        }
    }
}

impl Database {
    /// Create a new database from an existing storage engine
    pub fn new(storage: StorageEngine) -> Self {
        Self {
            storage,
            graphs: Arc::new(RwLock::new(HashMap::new())),
            config: DatabaseConfig::default(),
        }
    }

    /// Get access to the storage engine
    pub fn storage(&self) -> &StorageEngine {
        &self.storage
    }

    /// Open or create a database at the given path
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        Self::open_with_config(path, DatabaseConfig::default())
    }

    /// Open or create a database with custom configuration
    pub fn open_with_config<P: AsRef<Path>>(path: P, config: DatabaseConfig) -> Result<Self> {
        let storage_opts = StorageOptions::new(path.as_ref());
        let storage = StorageEngine::open(storage_opts)?;

        info!("Opened database at {:?}", path.as_ref());

        let db = Self {
            storage,
            graphs: Arc::new(RwLock::new(HashMap::new())),
            config,
        };

        // Load existing graphs from metadata
        db.load_graphs()?;

        Ok(db)
    }

    /// Open a database with testing configuration
    pub fn open_for_testing<P: AsRef<Path>>(path: P) -> Result<Self> {
        let storage_opts = StorageOptions::for_testing(path.as_ref());
        let storage = StorageEngine::open(storage_opts)?;

        Ok(Self {
            storage,
            graphs: Arc::new(RwLock::new(HashMap::new())),
            config: DatabaseConfig::default(),
        })
    }

    /// Create or obtain the authoritative active generation for this name.
    pub fn graph(&self, name: &str) -> Result<Graph> {
        let identity = self
            .storage
            .open_named_graph(name, self.config.max_graphs, false)?;
        self.cache_graph(identity)
    }

    /// Create a fresh graph generation. Existing names are rejected atomically.
    pub fn create_graph(&self, name: &str) -> Result<Graph> {
        let identity = self
            .storage
            .open_named_graph(name, self.config.max_graphs, true)?;
        self.cache_graph(identity)
    }

    pub fn default_graph(&self) -> Result<Graph> {
        self.graph(&self.config.default_graph)
    }

    /// List authoritative active names in lexical order.
    pub fn list_graphs(&self) -> Result<Vec<String>> {
        Ok(self
            .storage
            .named_graphs()?
            .into_iter()
            .map(|graph| graph.name().to_owned())
            .collect())
    }

    /// Retire a generation durably. Its old handles become unusable; retained
    /// physical records are not erased and a new graph receives a fresh ID.
    pub fn delete_graph(&self, name: &str) -> Result<bool> {
        let Some(retired) = self.storage.retire_named_graph(name)? else {
            return Ok(false);
        };
        let mut graphs = self
            .graphs
            .write()
            .map_err(|_| Error::Internal("Failed to acquire graphs lock".into()))?;
        if graphs
            .get(name)
            .is_some_and(|graph| graph.id() == retired.id())
        {
            graphs.remove(name);
        }
        Ok(true)
    }

    pub fn graph_exists(&self, name: &str) -> Result<bool> {
        Ok(self
            .storage
            .named_graphs()?
            .iter()
            .any(|graph| graph.name() == name))
    }

    pub fn graph_count(&self) -> Result<usize> {
        Ok(self.storage.named_graphs()?.len())
    }

    fn cache_graph(&self, identity: GraphIdentity) -> Result<Graph> {
        let mut graphs = self
            .graphs
            .write()
            .map_err(|_| Error::Internal("Failed to acquire graphs lock".into()))?;
        if let Some(graph) = graphs
            .get(identity.name())
            .filter(|graph| graph.id() == identity.id())
        {
            return Ok(graph.clone());
        }
        let name = identity.name().to_owned();
        let graph = Graph::from_identity(identity, self.storage.clone());
        graphs.insert(name, graph.clone());
        Ok(graph)
    }

    /// Flush all data to disk
    pub fn flush(&self) -> Result<()> {
        self.storage.flush()
    }

    /// Compact the database
    pub fn compact(&self) -> Result<()> {
        self.storage.compact()
    }

    /// Get storage statistics
    pub fn stats(&self) -> String {
        self.storage.stats()
    }

    // ========== Private Methods ==========

    fn load_graphs(&self) -> Result<()> {
        for identity in self.storage.named_graphs()? {
            self.cache_graph(identity)?;
        }
        Ok(())
    }
}

impl Clone for Database {
    fn clone(&self) -> Self {
        Self {
            storage: self.storage.clone(),
            graphs: Arc::clone(&self.graphs),
            config: self.config.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_db() -> (Database, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let db = Database::open_for_testing(temp_dir.path()).unwrap();
        (db, temp_dir)
    }

    #[test]
    fn test_open_database() {
        let (_db, _dir) = create_test_db();
    }

    #[test]
    fn test_create_graph() {
        let (db, _dir) = create_test_db();

        let graph = db.create_graph("test").unwrap();
        assert_eq!(graph.name(), "test");

        assert!(db.graph_exists("test").unwrap());
        assert!(!db.graph_exists("other").unwrap());
    }

    #[test]
    fn test_get_or_create_graph() {
        let (db, _dir) = create_test_db();

        let graph1 = db.graph("test").unwrap();
        let graph2 = db.graph("test").unwrap();

        assert_eq!(graph1.id(), graph2.id());
    }

    #[test]
    fn test_list_graphs() {
        let (db, _dir) = create_test_db();

        db.create_graph("graph1").unwrap();
        db.create_graph("graph2").unwrap();
        db.create_graph("graph3").unwrap();

        let names = db.list_graphs().unwrap();
        assert_eq!(names.len(), 3);
        assert!(names.contains(&"graph1".to_string()));
        assert!(names.contains(&"graph2".to_string()));
        assert!(names.contains(&"graph3".to_string()));
    }

    #[test]
    fn test_delete_graph() {
        let (db, _dir) = create_test_db();

        db.create_graph("test").unwrap();
        assert!(db.graph_exists("test").unwrap());

        assert!(db.delete_graph("test").unwrap());
        assert!(!db.graph_exists("test").unwrap());

        // Deleting non-existent graph returns false
        assert!(!db.delete_graph("test").unwrap());
    }

    #[test]
    fn test_graph_count() {
        let (db, _dir) = create_test_db();

        assert_eq!(db.graph_count().unwrap(), 0);

        db.create_graph("graph1").unwrap();
        assert_eq!(db.graph_count().unwrap(), 1);

        db.create_graph("graph2").unwrap();
        assert_eq!(db.graph_count().unwrap(), 2);
    }

    #[test]
    fn test_duplicate_graph_error() {
        let (db, _dir) = create_test_db();

        db.create_graph("test").unwrap();
        let result = db.create_graph("test");

        assert!(result.is_err());
    }

    #[test]
    fn test_default_graph() {
        let (db, _dir) = create_test_db();

        let graph = db.default_graph().unwrap();
        assert_eq!(graph.name(), "default");
    }
}

#[cfg(test)]
#[path = "lifecycle_tests.rs"]
mod lifecycle_regressions;
