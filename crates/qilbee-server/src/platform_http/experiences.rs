//! Authenticated experience receipts, separate from procedure qualification.
use super::*;
use crate::security::identity::{AuthorizedScope, ResourceScope, Visibility};
use qilbee_memory::learning::*;

pub(super) fn routes() -> Router<PlatformState> {
    Router::new()
        .route("/api/v1/experiences", post(create))
        .route(
            "/api/v1/experiences/withdrawals",
            post(withdraw).layer(axum::extract::DefaultBodyLimit::max(16_384)),
        )
        .route(
            "/api/v1/experiences/withdrawals/inspect",
            post(inspect_withdrawal).layer(axum::extract::DefaultBodyLimit::max(16_384)),
        )
        .route("/api/v1/experiences/read", post(read))
        .route("/api/v1/experiences/events", post(observe))
        .route("/api/v1/experiences/events/read", post(event))
        .route("/api/v1/experiences/history", post(history))
        .route("/api/v1/experiences/export", post(export))
        .route("/api/v1/experiences/lineage", post(lineage))
        .route("/api/v1/experiences/artifacts", post(bind_artifact))
        .route(
            "/api/v1/experiences/artifacts/read",
            post(read_artifact_binding),
        )
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateRequest {
    contract_version: u32,
    scope: ResourceScope,
    request: ExperienceRequest,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadRequest {
    contract_version: u32,
    scope: ResourceScope,
    attempt_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ObserveRequest {
    contract_version: u32,
    scope: ResourceScope,
    attempt_id: String,
    command: ExperienceCommand,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EventRequest {
    contract_version: u32,
    scope: ResourceScope,
    attempt_id: String,
    event_id: String,
}
fn author(scope: &AuthorizedScope) -> ExperienceActor {
    ExperienceActor {
        subject_id: scope.subject_id.clone(),
        credential_id: scope.credential_id.to_string(),
    }
}
fn missing() -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        "record_not_found",
        "No experience exists in the authorized scope",
    )
}
async fn create(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<CreateRequest>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let store = state.learning.clone();
    state
        .run(headers, move |identity, token, _| {
            let request = json_body(body)?;
            version(request.contract_version)?;
            let scope = identity
                .authorize(token, Capability::ExperienceWrite, &request.scope)
                .map_err(ApiError::operation)?;
            let reporter = &request.request.reporter_subject_id;
            if reporter.len() > 256 || reporter.chars().any(char::is_control) {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "Reporter subjects allow at most 256 UTF-8 bytes and no control characters",
                ));
            }
            if request.scope.visibility == Visibility::Private && reporter != &scope.subject_id {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "A private attempt requires its owning subject as reporter",
                ));
            }
            let receipt = store
                .create_experience(
                    &scope.tenant_id,
                    &scope.storage_namespace,
                    request.request,
                    author(&scope),
                )
                .map_err(ApiError::operation)?;
            Ok(Json(json!({"contract_version":1,"receipt":receipt})))
        })
        .await
}
async fn read(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<ReadRequest>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let store = state.learning.clone();
    state
        .run(headers, move |identity, token, _| {
            let request = json_body(body)?;
            version(request.contract_version)?;
            let scope = identity
                .authorize(token, Capability::ExperienceRead, &request.scope)
                .map_err(ApiError::operation)?;
            let record = store
                .experience(
                    &scope.tenant_id,
                    &scope.storage_namespace,
                    &request.attempt_id,
                )
                .map_err(ApiError::operation)?
                .ok_or_else(missing)?;
            Ok(Json(json!({"contract_version":1,"experience":record})))
        })
        .await
}
async fn observe(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<ObserveRequest>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let store = state.learning.clone();
    state
        .run(headers, move |identity, token, _| {
            let request = json_body(body)?;
            version(request.contract_version)?;
            let scope = identity
                .authorize(token, Capability::ExperienceReport, &request.scope)
                .map_err(ApiError::operation)?;
            let event = store
                .observe_experience(
                    &scope.tenant_id,
                    &scope.storage_namespace,
                    &request.attempt_id,
                    request.command,
                    author(&scope),
                )
                .map_err(ApiError::operation)?;
            Ok(Json(json!({"contract_version":1,"event":event})))
        })
        .await
}
async fn event(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<EventRequest>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let store = state.learning.clone();
    state
        .run(headers, move |identity, token, _| {
            let request = json_body(body)?;
            version(request.contract_version)?;
            let scope = identity
                .authorize(token, Capability::ExperienceRead, &request.scope)
                .map_err(ApiError::operation)?;
            let event = store
                .experience_event(
                    &scope.tenant_id,
                    &scope.storage_namespace,
                    &request.attempt_id,
                    &request.event_id,
                )
                .map_err(ApiError::operation)?
                .ok_or_else(missing)?;
            Ok(Json(json!({"contract_version":1,"event":event})))
        })
        .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoryRequest {
    contract_version: u32,
    scope: ResourceScope,
    attempt_id: String,
    query: ExperienceHistoryQuery,
}
async fn history(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<HistoryRequest>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let store = state.learning.clone();
    state
        .run(headers, move |identity, token, _| {
            let request = json_body(body)?;
            version(request.contract_version)?;
            let scope = identity
                .authorize(token, Capability::ExperienceRead, &request.scope)
                .map_err(ApiError::operation)?;
            let page = store
                .experience_history(
                    &scope.tenant_id,
                    &scope.storage_namespace,
                    &request.attempt_id,
                    request.query,
                )
                .map_err(ApiError::operation)?;
            Ok(Json(json!({"contract_version":1,"page":page})))
        })
        .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BindArtifactRequest {
    contract_version: u32,
    scope: ResourceScope,
    attempt_id: String,
    binding: ExperienceArtifactRequest,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadArtifactBindingRequest {
    contract_version: u32,
    scope: ResourceScope,
    attempt_id: String,
    binding_id: String,
}
async fn bind_artifact(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<BindArtifactRequest>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let store = state.learning.clone();
    state
        .run(headers, move |identity, token, _| {
            let request = json_body(body)?;
            version(request.contract_version)?;
            let scope = identity
                .authorize(token, Capability::ExperienceReport, &request.scope)
                .map_err(ApiError::operation)?;
            identity
                .authorize(token, Capability::ToolRead, &request.scope)
                .map_err(ApiError::operation)?;
            let binding = store
                .bind_experience_artifact(
                    &scope.tenant_id,
                    &scope.storage_namespace,
                    &request.attempt_id,
                    request.binding,
                    author(&scope),
                )
                .map_err(ApiError::operation)?;
            Ok(Json(json!({"contract_version":1,"binding":binding})))
        })
        .await
}
async fn read_artifact_binding(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<ReadArtifactBindingRequest>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let store = state.learning.clone();
    state
        .run(headers, move |identity, token, _| {
            let request = json_body(body)?;
            version(request.contract_version)?;
            let scope = identity
                .authorize(token, Capability::ExperienceRead, &request.scope)
                .map_err(ApiError::operation)?;
            identity
                .authorize(token, Capability::ToolRead, &request.scope)
                .map_err(ApiError::operation)?;
            let binding = store
                .experience_artifact_binding(
                    &scope.tenant_id,
                    &scope.storage_namespace,
                    &request.attempt_id,
                    &request.binding_id,
                )
                .map_err(ApiError::operation)?
                .ok_or_else(missing)?;
            Ok(Json(json!({"contract_version":1,"binding":binding})))
        })
        .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LineageRequest {
    contract_version: u32,
    scope: ResourceScope,
    attempt_id: String,
    max_depth: usize,
}
async fn lineage(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<LineageRequest>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let store = state.learning.clone();
    state
        .run(headers, move |identity, token, _| {
            let request = json_body(body)?;
            version(request.contract_version)?;
            let scope = identity
                .authorize(token, Capability::ExperienceRead, &request.scope)
                .map_err(ApiError::operation)?;
            let lineage = store
                .experience_lineage(
                    &scope.tenant_id,
                    &scope.storage_namespace,
                    &request.attempt_id,
                    request.max_depth,
                )
                .map_err(ApiError::operation)?;
            Ok(Json(json!({"contract_version":1,"lineage":lineage})))
        })
        .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExportRequest {
    contract_version: u32,
    scope: ResourceScope,
    selection: ExperienceExportRequest,
}
async fn export(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<ExportRequest>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let store = state.learning.clone();
    state
        .run(headers, move |identity, token, _| {
            let request = json_body(body)?;
            version(request.contract_version)?;
            let scope = identity
                .authorize(token, Capability::ExperienceRead, &request.scope)
                .map_err(ApiError::operation)?;
            let export = store
                .export_experiences(
                    &scope.tenant_id,
                    &scope.storage_namespace,
                    request.selection,
                )
                .map_err(ApiError::operation)?;
            Ok(Json(json!({"contract_version":1,"export":export})))
        })
        .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WithdrawRequest {
    contract_version: u32,
    scope: ResourceScope,
    idempotency_key: String,
    observation: ExperienceExportRef,
    reason: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InspectWithdrawalRequest {
    contract_version: u32,
    scope: ResourceScope,
    observation: ExperienceExportRef,
}
async fn withdraw(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<WithdrawRequest>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let store = state.learning.clone();
    state
        .run(headers, move |identity, token, _| {
            let request = json_body(body)?;
            version(request.contract_version)?;
            let scope = identity
                .authorize(token, Capability::ExperienceEvidenceAdmin, &request.scope)
                .map_err(ApiError::operation)?;
            identity
                .authorize(token, Capability::ExperienceRead, &request.scope)
                .map_err(ApiError::operation)?;
            let actor = author(&scope);
            let receipt = store
                .withdraw_experience(
                    &scope.tenant_id,
                    &scope.storage_namespace,
                    ExperienceWithdrawalCommand {
                        idempotency_key: request.idempotency_key,
                        observation: request.observation,
                        reason: request.reason,
                    },
                    &actor,
                )
                .map_err(ApiError::operation)?;
            Ok(Json(json!({"contract_version":1,"receipt":receipt})))
        })
        .await
}
async fn inspect_withdrawal(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<InspectWithdrawalRequest>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let store = state.learning.clone();
    let limits = state.retrieval_limits.clone();
    state
        .run(headers, move |identity, token, _| {
            let request = json_body(body)?;
            version(request.contract_version)?;
            let scope = identity
                .authorize(token, Capability::ExperienceRead, &request.scope)
                .map_err(ApiError::operation)?;
            let _permit = limits.acquire()?;
            let inspection = store
                .inspect_experience_withdrawal(
                    &scope.tenant_id,
                    &scope.storage_namespace,
                    request.observation,
                )
                .map_err(ApiError::operation)?;
            Ok(Json(json!({"contract_version":1,
            "evaluated_at_millis":inspection.evaluated_at_millis,
            "observation":inspection.observation,"status":inspection.status,
            "receipt":inspection.receipt})))
        })
        .await
}
