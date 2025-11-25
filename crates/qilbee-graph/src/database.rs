//! Database management for QilbeeDB

use crate::graph::Graph;
use qilbee_core::{Error, GraphId, Result};
use qilbee_storage::{StorageEngine, StorageOptions};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, RwLock};
use tracing::{info, warn};

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

    /// Create or get a graph by name
    pub fn graph(&self, name: &str) -> Result<Graph> {
        // Check if graph already exists
        {
            let graphs = self.graphs.read().map_err(|_| {
                Error::Internal("Failed to acquire graphs lock".to_string())
            })?;

            if let Some(graph) = graphs.get(name) {
                return Ok(graph.clone());
            }
        }

        // Create new graph
        self.create_graph(name)
    }

    /// Create a new graph
    pub fn create_graph(&self, name: &str) -> Result<Graph> {
        let mut graphs = self.graphs.write().map_err(|_| {
            Error::Internal("Failed to acquire graphs lock".to_string())
        })?;

        // Check if already exists
        if graphs.contains_key(name) {
            return Err(Error::InvalidGraphOperation(format!(
                "Graph '{}' already exists",
                name
            )));
        }

        // Check max graphs limit
        if graphs.len() >= self.config.max_graphs {
            return Err(Error::InvalidGraphOperation(format!(
                "Maximum number of graphs ({}) reached",
                self.config.max_graphs
            )));
        }

        let graph = Graph::new(name.to_string(), self.storage.clone());
        graphs.insert(name.to_string(), graph.clone());

        // Store graph metadata (collect names while holding lock)
        let graph_names: Vec<String> = graphs.keys().cloned().collect();
        drop(graphs); // Release lock before I/O

        let data = serde_json::to_vec(&graph_names)
            .map_err(|e| Error::Serialization(e.to_string()))?;
        self.storage.put_meta("graphs", &data)?;

        info!("Created graph '{}'", name);
        Ok(graph)
    }

    /// Get the default graph
    pub fn default_graph(&self) -> Result<Graph> {
        self.graph(&self.config.default_graph)
    }

    /// List all graph names
    pub fn list_graphs(&self) -> Result<Vec<String>> {
        let graphs = self.graphs.read().map_err(|_| {
            Error::Internal("Failed to acquire graphs lock".to_string())
        })?;

        Ok(graphs.keys().cloned().collect())
    }

    /// Delete a graph
    pub fn delete_graph(&self, name: &str) -> Result<bool> {
        let mut graphs = self.graphs.write().map_err(|_| {
            Error::Internal("Failed to acquire graphs lock".to_string())
        })?;

        if graphs.remove(name).is_some() {
            // Update graph metadata (collect names while holding lock)
            let graph_names: Vec<String> = graphs.keys().cloned().collect();
            drop(graphs); // Release lock before I/O

            let data = serde_json::to_vec(&graph_names)
                .map_err(|e| Error::Serialization(e.to_string()))?;
            self.storage.put_meta("graphs", &data)?;

            info!("Deleted graph '{}'", name);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Check if a graph exists
    pub fn graph_exists(&self, name: &str) -> Result<bool> {
        let graphs = self.graphs.read().map_err(|_| {
            Error::Internal("Failed to acquire graphs lock".to_string())
        })?;

        Ok(graphs.contains_key(name))
    }

    /// Get the number of graphs
    pub fn graph_count(&self) -> Result<usize> {
        let graphs = self.graphs.read().map_err(|_| {
            Error::Internal("Failed to acquire graphs lock".to_string())
        })?;

        Ok(graphs.len())
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
        // Load graph list from metadata
        if let Some(data) = self.storage.get_meta("graphs")? {
            let graph_names: Vec<String> = serde_json::from_slice(&data)
                .map_err(|e| Error::Deserialization(e.to_string()))?;

            let mut graphs = self.graphs.write().map_err(|_| {
                Error::Internal("Failed to acquire graphs lock".to_string())
            })?;

            for name in graph_names {
                let graph = Graph::new(name.clone(), self.storage.clone());
                graphs.insert(name, graph);
            }
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
