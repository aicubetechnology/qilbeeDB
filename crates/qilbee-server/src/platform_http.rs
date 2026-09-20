//! Versioned platform HTTP API with explicit, durable credential authority.

use crate::security::identity::{Capability, CredentialSpec, CredentialView, IdentityStore};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State, rejection::JsonRejection},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use qilbee_core::Error;
use qilbee_graph::Database;
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;

mod memory;

#[derive(Clone)]
pub(crate) struct PlatformState {
    identity: Arc<IdentityStore>,
    memory: Arc<qilbee_memory::RocksDbMemoryStorage>,
}

impl PlatformState {
    async fn run<T, F>(&self, headers: HeaderMap, operation: F) -> ApiResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&IdentityStore, &str, CredentialView) -> ApiResult<T> + Send + 'static,
    {
        let token = bearer(&headers)?;
        let identity = self.identity.clone();
        tokio::task::spawn_blocking(move || {
            let principal = identity
                .authenticate(&token)
                .map_err(ApiError::authentication)?;
            operation(&identity, &token, principal)
        })
        .await
        .map_err(|_| ApiError::internal())?
    }
}

/// The default router exposes only platform contracts. Legacy APIs require an
/// explicit, separate router and never supply credentials for this authority.
pub fn create_router(database: Arc<Database>) -> qilbee_core::Result<Router> {
    let memory_path = database.storage().path().join("agent-memory");
    let memory_path = memory_path
        .to_str()
        .ok_or_else(|| Error::Configuration("Memory path must be valid UTF-8".into()))?;
    let memory = Arc::new(qilbee_memory::RocksDbMemoryStorage::open(
        qilbee_memory::MemoryStorageConfig {
            path: memory_path.into(),
            enable_wal: true,
            sync_writes: true,
            ..Default::default()
        },
    )?);
    let state = PlatformState {
        memory,
        identity: Arc::new(IdentityStore::new(Arc::new(database.storage().clone()))),
    };
    Ok(Router::new()
        .merge(memory::routes())
        .route("/health", get(health))
        .route("/api/v1/identity", get(who_am_i))
        .route("/api/v1/credentials", post(issue))
        .route("/api/v1/credentials/:id", get(inspect))
        .route("/api/v1/credentials/:id/rotate", post(rotate))
        .route("/api/v1/credentials/:id/revoke", post(revoke))
        .fallback(|| async { ApiError::new(StatusCode::NOT_FOUND, "not_found", "Route not found") })
        .method_not_allowed_fallback(|| async {
            ApiError::new(
                StatusCode::METHOD_NOT_ALLOWED,
                "method_not_allowed",
                "Method is not supported for this route",
            )
        })
        .layer(DefaultBodyLimit::max(64 * 1024))
        .layer(axum::middleware::from_fn(response_headers))
        .with_state(state))
}

async fn response_headers(request: axum::extract::Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    response
}
async fn health() -> Json<Value> {
    Json(json!({"contract_version":1,"status":"healthy","version":env!("CARGO_PKG_VERSION")}))
}
async fn who_am_i(
    State(state): State<PlatformState>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    state
        .run(headers, |_, _, credential| {
            Ok(Json(json!({"contract_version":1,"credential":credential})))
        })
        .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IssueRequest {
    contract_version: u32,
    spec: CredentialSpec,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChangeRequest {
    contract_version: u32,
    expected_revision: u64,
}

async fn issue(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<IssueRequest>, JsonRejection>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    state.run(headers, move |identity, token, principal| {
        require_admin(&principal)?;
        let request = json_body(body)?;
        version(request.contract_version)?;
        let issued = identity.issue(token, request.spec).map_err(ApiError::operation)?;
        Ok((StatusCode::CREATED, Json(json!({"contract_version":1,"credential":issued.credential,"secret":issued.secret}))))
    }).await
}
async fn inspect(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    state
        .run(headers, move |identity, token, principal| {
            require_admin(&principal)?;
            let credential = identity
                .inspect(token, identifier(&id)?)
                .map_err(ApiError::operation)?;
            Ok(Json(json!({"contract_version":1,"credential":credential})))
        })
        .await
}
async fn rotate(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Result<Json<ChangeRequest>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    state
        .run(headers, move |identity, token, principal| {
            require_admin(&principal)?;
            let request = json_body(body)?;
            version(request.contract_version)?;
            let issued = identity
                .rotate(token, identifier(&id)?, request.expected_revision)
                .map_err(ApiError::operation)?;
            Ok(Json(
                json!({"contract_version":1,"credential":issued.credential,"secret":issued.secret}),
            ))
        })
        .await
}
async fn revoke(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Result<Json<ChangeRequest>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    state
        .run(headers, move |identity, token, principal| {
            require_admin(&principal)?;
            let request = json_body(body)?;
            version(request.contract_version)?;
            let credential = identity
                .revoke(token, identifier(&id)?, request.expected_revision)
                .map_err(ApiError::operation)?;
            Ok(Json(json!({"contract_version":1,"credential":credential})))
        })
        .await
}
fn require_admin(principal: &CredentialView) -> ApiResult<()> {
    if principal
        .spec
        .capabilities
        .contains(&Capability::CredentialAdmin)
    {
        Ok(())
    } else {
        Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "forbidden",
            "Credential administration is not granted",
        ))
    }
}
fn bearer(headers: &HeaderMap) -> ApiResult<String> {
    let unauthorized = || {
        ApiError::new(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "A valid platform bearer credential is required",
        )
    };
    let mut values = headers.get_all(header::AUTHORIZATION).iter();
    let value = values.next().ok_or_else(unauthorized)?;
    if values.next().is_some() {
        return Err(unauthorized());
    }
    let value = value.to_str().map_err(|_| unauthorized())?;
    let (scheme, token) = value.split_once(' ').ok_or_else(unauthorized)?;
    if !scheme.eq_ignore_ascii_case("bearer")
        || token.is_empty()
        || token.len() > 128
        || token.contains(char::is_whitespace)
    {
        return Err(unauthorized());
    }
    Ok(token.to_owned())
}
fn identifier(value: &str) -> ApiResult<Uuid> {
    Uuid::parse_str(value).map_err(|_| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Expected a credential UUID",
        )
    })
}
fn version(value: u32) -> ApiResult<()> {
    if value == 1 {
        Ok(())
    } else {
        Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_contract_version",
            "Supported contract version is 1",
        ))
    }
}
fn json_body<T>(body: Result<Json<T>, JsonRejection>) -> ApiResult<T> {
    body.map(|Json(value)| value).map_err(|rejection| {
        let status = if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
            StatusCode::PAYLOAD_TOO_LARGE
        } else {
            StatusCode::BAD_REQUEST
        };
        ApiError::new(
            status,
            "invalid_request",
            "Request must match the JSON contract and transport size limit",
        )
    })
}
pub(crate) type ApiResult<T> = Result<T, ApiError>;
pub(crate) struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: &'static str,
}
impl ApiError {
    fn new(status: StatusCode, code: &'static str, message: &'static str) -> Self {
        Self {
            status,
            code,
            message,
        }
    }
    fn internal() -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "The operation could not be completed",
        )
    }
    fn authentication(error: Error) -> Self {
        match error {
            Error::Unauthorized(_) => Self::new(
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "A valid platform bearer credential is required",
            ),
            _ => Self::internal(),
        }
    }
    fn operation(error: Error) -> Self {
        match error {
            Error::Unauthorized(_) => Self::new(
                StatusCode::FORBIDDEN,
                "forbidden",
                "The operation is not authorized",
            ),
            Error::ValidationError(_) => Self::new(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "Invalid request fields",
            ),
            Error::KeyNotFound(_) => Self::new(
                StatusCode::NOT_FOUND,
                "record_not_found",
                "No current record exists in the authorized scope",
            ),
            Error::ConstraintViolation(_) => Self::new(
                StatusCode::CONFLICT,
                "idempotency_conflict",
                "The idempotency key was already used for a different command",
            ),
            Error::DataCorruption(_) => Self::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "storage_inconsistency",
                "A stored record, index or receipt has an unsupported or inconsistent version",
            ),
            Error::TransactionConflict(_) => Self::new(
                StatusCode::CONFLICT,
                "revision_conflict",
                "The expected revision or authorizing credential changed",
            ),
            _ => Self::internal(),
        }
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({"contract_version":1,"error":{"code":self.code,"message":self.message}})),
        )
            .into_response()
    }
}
