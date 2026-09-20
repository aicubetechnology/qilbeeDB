//! Scoped declarations and external-worker receipts for learned tools.
use super::*;
use crate::security::identity::{AuthorizedScope, ResourceScope};
use qilbee_memory::learning::*;

pub(super) fn routes() -> Router<PlatformState> {
    Router::new()
        .route("/api/v1/tools/artifacts", post(register_artifact))
        .route("/api/v1/tools/artifacts/read", post(artifact))
        .route("/api/v1/tools/executors", post(register_executor))
        .route("/api/v1/tools/executors/:id", get(executor))
        .route(
            "/api/v1/tools/development/requests",
            post(create_development),
        )
        .route("/api/v1/tools/development/read", post(development))
        .route("/api/v1/tools/development/commands", post(command))
        .route("/api/v1/tools/development/events/read", post(event))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ArtifactRequest {
    contract_version: u32,
    scope: ResourceScope,
    artifact: ToolArtifactProposal,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ArtifactRead {
    contract_version: u32,
    scope: ResourceScope,
    artifact_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExecutorRequest {
    contract_version: u32,
    profile: ToolExecutorProfile,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DevelopmentRequest {
    contract_version: u32,
    scope: ResourceScope,
    request: ToolDevelopmentRequest,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DevelopmentRead {
    contract_version: u32,
    scope: ResourceScope,
    request_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CommandRequest {
    contract_version: u32,
    scope: ResourceScope,
    request_id: String,
    command: ToolDevelopmentCommand,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EventRead {
    contract_version: u32,
    scope: ResourceScope,
    request_id: String,
    event_id: String,
}
fn author(scope: &AuthorizedScope) -> ToolActor {
    ToolActor {
        subject_id: scope.subject_id.clone(),
        credential_id: scope.credential_id.to_string(),
    }
}
fn require_tool_admin(principal: &CredentialView) -> ApiResult<()> {
    if principal.spec.capabilities.contains(&Capability::ToolAdmin) {
        Ok(())
    } else {
        Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "forbidden",
            "Tool administration is not granted",
        ))
    }
}
fn missing() -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        "record_not_found",
        "No tool record exists in the authorized namespace",
    )
}
async fn register_artifact(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<ArtifactRequest>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let store = state.learning.clone();
    state
        .run(headers, move |identity, token, _| {
            let request = json_body(body)?;
            version(request.contract_version)?;
            let scope = identity
                .authorize(token, Capability::ToolDevelop, &request.scope)
                .map_err(ApiError::operation)?;
            let artifact = store
                .register_tool_artifact(
                    &scope.tenant_id,
                    &scope.storage_namespace,
                    request.artifact,
                    author(&scope),
                )
                .map_err(ApiError::operation)?;
            Ok(Json(json!({"contract_version":1,"artifact":artifact})))
        })
        .await
}
async fn artifact(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<ArtifactRead>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let store = state.learning.clone();
    state
        .run(headers, move |identity, token, _| {
            let request = json_body(body)?;
            version(request.contract_version)?;
            let scope = identity
                .authorize(token, Capability::ToolRead, &request.scope)
                .map_err(ApiError::operation)?;
            let artifact = store
                .tool_artifact(
                    &scope.tenant_id,
                    &scope.storage_namespace,
                    &request.artifact_id,
                )
                .map_err(ApiError::operation)?
                .ok_or_else(missing)?;
            Ok(Json(json!({"contract_version":1,"artifact":artifact})))
        })
        .await
}
async fn register_executor(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<ExecutorRequest>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let store = state.learning.clone();
    state
        .run(headers, move |_, _, principal| {
            require_tool_admin(&principal)?;
            let request = json_body(body)?;
            version(request.contract_version)?;
            let actor = ToolActor {
                subject_id: principal.spec.subject_id,
                credential_id: principal.id.to_string(),
            };
            let executor = store
                .register_tool_executor(&principal.tenant_id, request.profile, actor)
                .map_err(ApiError::operation)?;
            Ok(Json(json!({"contract_version":1,"executor":executor})))
        })
        .await
}
async fn executor(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let store = state.learning.clone();
    state
        .run(headers, move |_, _, principal| {
            require_tool_admin(&principal)?;
            let executor = store
                .tool_executor(&principal.tenant_id, &id)
                .map_err(ApiError::operation)?
                .ok_or_else(missing)?;
            Ok(Json(json!({"contract_version":1,"executor":executor})))
        })
        .await
}
async fn create_development(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<DevelopmentRequest>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let store = state.learning.clone();
    state
        .run(headers, move |identity, token, _| {
            let request = json_body(body)?;
            version(request.contract_version)?;
            let scope = identity
                .authorize(token, Capability::ToolDevelop, &request.scope)
                .map_err(ApiError::operation)?;
            let receipt = store
                .create_tool_development(
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
async fn development(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<DevelopmentRead>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let store = state.learning.clone();
    state
        .run(headers, move |identity, token, _| {
            let request = json_body(body)?;
            version(request.contract_version)?;
            let scope = identity
                .authorize(token, Capability::ToolRead, &request.scope)
                .map_err(ApiError::operation)?;
            let development = store
                .tool_development(
                    &scope.tenant_id,
                    &scope.storage_namespace,
                    &request.request_id,
                )
                .map_err(ApiError::operation)?
                .ok_or_else(missing)?;
            Ok(Json(
                json!({"contract_version":1,"development":development}),
            ))
        })
        .await
}
async fn command(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<CommandRequest>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let store = state.learning.clone();
    state
        .run(headers, move |identity, token, _| {
            let request = json_body(body)?;
            version(request.contract_version)?;
            let capability = match &request.command.action {
                ToolDevelopmentAction::Report { .. } => Capability::ToolReport,
                ToolDevelopmentAction::RequestCancellation { .. } => Capability::ToolDevelop,
            };
            let scope = identity
                .authorize(token, capability, &request.scope)
                .map_err(ApiError::operation)?;
            let event = store
                .apply_tool_development(
                    &scope.tenant_id,
                    &scope.storage_namespace,
                    &request.request_id,
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
    body: Result<Json<EventRead>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let store = state.learning.clone();
    state
        .run(headers, move |identity, token, _| {
            let request = json_body(body)?;
            version(request.contract_version)?;
            let scope = identity
                .authorize(token, Capability::ToolRead, &request.scope)
                .map_err(ApiError::operation)?;
            let event = store
                .tool_development_event(
                    &scope.tenant_id,
                    &scope.storage_namespace,
                    &request.request_id,
                    &request.event_id,
                )
                .map_err(ApiError::operation)?
                .ok_or_else(missing)?;
            Ok(Json(json!({"contract_version":1,"event":event})))
        })
        .await
}
