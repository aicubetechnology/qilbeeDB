//! Operator-owned execution limits, independent of ranking and tenant policy.
use super::*;
use qilbee_memory::storage::platform::{MAX_EMBEDDING_DIMENSIONS, MAX_RETRIEVAL_SCAN_BYTES};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

#[cfg(test)]
mod http_tests;

pub(super) const VECTOR_BODY_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone)]
pub(super) struct RetrievalLimits {
    max_dimensions: usize,
    max_scan_bytes: usize,
    max_concurrent: usize,
    permits: Arc<Semaphore>,
}

impl RetrievalLimits {
    pub(super) fn from_env() -> qilbee_core::Result<Self> {
        Self::from_lookup(|key| std::env::var_os(key).map(|v| v.to_string_lossy().into_owned()))
    }

    fn from_lookup(read: impl Fn(&str) -> Option<String>) -> qilbee_core::Result<Self> {
        let parse = |key: &str, default: usize, minimum: usize, maximum: usize| {
            let value = match read(key) {
                None => default,
                Some(raw) => raw.parse::<usize>().map_err(|_| {
                    Error::Configuration(format!(
                        "{key} must be an integer in {minimum}..={maximum}"
                    ))
                })?,
            };
            if !(minimum..=maximum).contains(&value) {
                return Err(Error::Configuration(format!(
                    "{key} must be an integer in {minimum}..={maximum}"
                )));
            }
            Ok(value)
        };
        let max_dimensions = parse(
            "QILBEE_MAX_EMBEDDING_DIMENSIONS",
            MAX_EMBEDDING_DIMENSIONS,
            1,
            MAX_EMBEDDING_DIMENSIONS,
        )?;
        // The ceiling must admit the existing 8 MiB per-request default.
        let max_scan_bytes = parse(
            "QILBEE_MAX_RETRIEVAL_SCAN_BYTES",
            64 * 1024 * 1024,
            8 * 1024 * 1024,
            MAX_RETRIEVAL_SCAN_BYTES,
        )?;
        let max_concurrent = parse("QILBEE_MAX_CONCURRENT_RETRIEVALS", 2, 1, 64)?;
        Ok(Self {
            max_dimensions,
            max_scan_bytes,
            max_concurrent,
            permits: Arc::new(Semaphore::new(max_concurrent)),
        })
    }

    pub(super) fn dimensions(&self, dimensions: usize) -> ApiResult<()> {
        if dimensions == 0 || dimensions > self.max_dimensions {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "embedding_dimension_limit",
                "Embedding dimensions exceed the server configuration or are zero",
            ));
        }
        Ok(())
    }

    pub(super) fn scan_bytes(&self, bytes: usize) -> ApiResult<()> {
        if bytes == 0 || bytes > self.max_scan_bytes {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "retrieval_scan_limit",
                "Scan bytes exceed the server configuration or are zero",
            ));
        }
        Ok(())
    }

    /// Called only after current credential and scope authorization. The permit
    /// lives inside the blocking operation even if its HTTP caller disconnects.
    pub(super) fn acquire(&self) -> ApiResult<OwnedSemaphorePermit> {
        self.permits.clone().try_acquire_owned().map_err(|_| {
            ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "retrieval_busy",
                "The server has no available retrieval slot; retry with backoff",
            )
        })
    }

    pub(super) fn metadata(&self) -> Value {
        json!({
            "max_embedding_dimensions": self.max_dimensions,
            "max_scan_bytes": self.max_scan_bytes,
            "default_scan_bytes": 8_388_608,
            "max_concurrent_retrievals": self.max_concurrent,
            "vector_request_body_bytes": VECTOR_BODY_BYTES,
            "scope": "server_instance"
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_preserve_scan_default_and_validate_operator_configuration() {
        let limits = RetrievalLimits::from_lookup(|_| None).unwrap();
        assert!(limits.dimensions(32768).is_ok());
        assert!(limits.dimensions(32769).is_err());
        assert!(limits.scan_bytes(67_108_864).is_ok());
        assert!(limits.scan_bytes(134_217_728).is_err());
        let configured = RetrievalLimits::from_lookup(|key| match key {
            "QILBEE_MAX_EMBEDDING_DIMENSIONS" => Some("8192".into()),
            "QILBEE_MAX_RETRIEVAL_SCAN_BYTES" => Some("134217728".into()),
            _ => None,
        })
        .unwrap();
        assert!(configured.dimensions(8193).is_err());
        assert!(configured.scan_bytes(134_217_728).is_ok());
        for (key, value) in [
            ("QILBEE_MAX_EMBEDDING_DIMENSIONS", "0"),
            ("QILBEE_MAX_EMBEDDING_DIMENSIONS", "32769"),
            ("QILBEE_MAX_RETRIEVAL_SCAN_BYTES", "8388607"),
            ("QILBEE_MAX_RETRIEVAL_SCAN_BYTES", "268435457"),
            ("QILBEE_MAX_CONCURRENT_RETRIEVALS", "0"),
            ("QILBEE_MAX_CONCURRENT_RETRIEVALS", "65"),
            ("QILBEE_MAX_CONCURRENT_RETRIEVALS", "invalid"),
        ] {
            assert!(
                RetrievalLimits::from_lookup(|name| (name == key).then(|| value.into())).is_err()
            );
        }
    }

    #[test]
    fn admission_is_shared_nonblocking_and_released_after_error_or_disconnect() {
        let limits = RetrievalLimits::from_lookup(|key| {
            (key == "QILBEE_MAX_CONCURRENT_RETRIEVALS").then(|| "1".into())
        })
        .unwrap();
        let worker = limits.clone();
        let permit = worker.acquire().ok().unwrap();
        assert_eq!(
            limits.acquire().err().unwrap().status,
            StatusCode::SERVICE_UNAVAILABLE
        );
        drop(worker);
        assert!(limits.acquire().is_err());
        drop(permit);
        assert!(limits.acquire().is_ok());
    }
}
