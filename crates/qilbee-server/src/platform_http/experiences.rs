//! Authenticated experience receipts, separate from procedure qualification.
use super::*;
use crate::security::identity::{AuthorizedScope, ResourceScope, Visibility};
use qilbee_memory::learning::*;

pub(super) fn routes() -> Router<PlatformState> {
    Router::new()
        .route("/api/v1/experiences", post(create))
        .route("/api/v1/experiences/read", post(read))
        .route("/api/v1/experiences/events", post(observe))
        .route("/api/v1/experiences/events/read", post(event))
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
