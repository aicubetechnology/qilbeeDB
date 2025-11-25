//! Storage configuration options

use std::path::PathBuf;

/// Options for configuring the storage engine
#[derive(Debug, Clone)]
pub struct StorageOptions {
    /// Path to the database directory
    pub path: PathBuf,

    /// Whether to create the database if it doesn't exist
    pub create_if_missing: bool,

    /// Maximum size of the write buffer (memtable) in bytes
    pub write_buffer_size: usize,

    /// Maximum number of write buffers
    pub max_write_buffer_number: i32,

    /// Target file size for level-1 SST files
    pub target_file_size_base: u64,

    /// Maximum number of bytes for level-1
    pub max_bytes_for_level_base: u64,

    /// Number of background compaction threads
    pub max_background_jobs: i32,

    /// Enable compression
    pub enable_compression: bool,

    /// Block cache size in bytes
    pub block_cache_size: usize,

    /// Enable bloom filters
    pub enable_bloom_filter: bool,

    /// Bloom filter bits per key
    pub bloom_filter_bits_per_key: i32,

    /// Enable WAL (Write-Ahead Log)
    pub enable_wal: bool,

    /// Sync WAL on every write (slower but safer)
    pub sync_wal: bool,
}

impl StorageOptions {
    /// Create options for a new database at the given path
    pub fn new<P: Into<PathBuf>>(path: P) -> Self {
        Self {
            path: path.into(),
            ..Default::default()
        }
    }

    /// Create options optimized for development/testing
    pub fn for_testing<P: Into<PathBuf>>(path: P) -> Self {
        Self {
            path: path.into(),
            create_if_missing: true,
            write_buffer_size: 4 * 1024 * 1024, // 4MB
            max_write_buffer_number: 2,
            target_file_size_base: 4 * 1024 * 1024, // 4MB
            max_bytes_for_level_base: 16 * 1024 * 1024, // 16MB
            max_background_jobs: 2,
            enable_compression: false,
            block_cache_size: 8 * 1024 * 1024, // 8MB
            enable_bloom_filter: true,
            bloom_filter_bits_per_key: 10,
            enable_wal: true,
            sync_wal: false, // Faster for tests
        }
    }

    /// Create options optimized for production
    pub fn for_production<P: Into<PathBuf>>(path: P) -> Self {
        Self {
            path: path.into(),
            create_if_missing: true,
            write_buffer_size: 64 * 1024 * 1024, // 64MB
            max_write_buffer_number: 4,
            target_file_size_base: 64 * 1024 * 1024, // 64MB
            max_bytes_for_level_base: 256 * 1024 * 1024, // 256MB
            max_background_jobs: 4,
            enable_compression: true,
            block_cache_size: 512 * 1024 * 1024, // 512MB
            enable_bloom_filter: true,
            bloom_filter_bits_per_key: 10,
            enable_wal: true,
            sync_wal: true,
        }
    }

    /// Set the write buffer size
    pub fn write_buffer_size(mut self, size: usize) -> Self {
        self.write_buffer_size = size;
        self
    }

    /// Set the block cache size
    pub fn block_cache_size(mut self, size: usize) -> Self {
        self.block_cache_size = size;
        self
    }

    /// Enable or disable WAL sync
    pub fn sync_wal(mut self, sync: bool) -> Self {
        self.sync_wal = sync;
        self
    }

    /// Enable or disable compression
    pub fn compression(mut self, enabled: bool) -> Self {
        self.enable_compression = enabled;
        self
    }
}

impl Default for StorageOptions {
    fn default() -> Self {
        Self {
            path: PathBuf::from("./data"),
            create_if_missing: true,
            write_buffer_size: 32 * 1024 * 1024, // 32MB
            max_write_buffer_number: 3,
            target_file_size_base: 32 * 1024 * 1024, // 32MB
            max_bytes_for_level_base: 128 * 1024 * 1024, // 128MB
            max_background_jobs: 4,
            enable_compression: true,
            block_cache_size: 128 * 1024 * 1024, // 128MB
            enable_bloom_filter: true,
            bloom_filter_bits_per_key: 10,
            enable_wal: true,
            sync_wal: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_options() {
        let opts = StorageOptions::default();
        assert!(opts.create_if_missing);
        assert!(opts.enable_wal);
        assert!(opts.enable_bloom_filter);
    }

    #[test]
    fn test_testing_options() {
        let opts = StorageOptions::for_testing("/tmp/test");
        assert!(!opts.sync_wal);
        assert!(!opts.enable_compression);
    }

    #[test]
    fn test_production_options() {
        let opts = StorageOptions::for_production("/var/lib/qilbeedb");
        assert!(opts.sync_wal);
        assert!(opts.enable_compression);
        assert!(opts.block_cache_size >= 512 * 1024 * 1024);
    }

    #[test]
    fn test_builder_pattern() {
        let opts = StorageOptions::new("/data")
            .write_buffer_size(128 * 1024 * 1024)
            .sync_wal(true)
            .compression(false);

        assert_eq!(opts.write_buffer_size, 128 * 1024 * 1024);
        assert!(opts.sync_wal);
        assert!(!opts.enable_compression);
    }
}
