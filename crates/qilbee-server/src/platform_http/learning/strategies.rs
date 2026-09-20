//! Strategy extraction stays external; the server validates and binds its declared evidence.
use super::*;
use qilbee_memory::learning::StrategyCandidateRequest;
pub(super) fn routes() -> Router<PlatformState> {
    Router::new()
        .route("/api/v1/learning/strategies", post(create))
        .route("/api/v1/learning/strategies/read", post(read))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateRequest {
    contract_version: u32,
    scope: ResourceScope,
    strategy: StrategyCandidateRequest,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadRequest {
    contract_version: u32,
    scope: ResourceScope,
    strategy_id: String,
}
async fn create(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<CreateRequest>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let store = state.learning.clone();
    state
        .run(headers, move |identity, token, actor| {
            let request = json_body(body)?;
            version(request.contract_version)?;
            let scope = identity
                .authorize(token, Capability::ProcedurePropose, &request.scope)
                .map_err(ApiError::operation)?;
            identity
                .authorize(token, Capability::ExperienceRead, &request.scope)
                .map_err(ApiError::operation)?;
            let receipt = store
                .propose_strategy(
                    &scope.tenant_id,
                    &scope.storage_namespace,
                    request.strategy,
                    &author(&actor),
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
                .authorize(token, Capability::MemoryRead, &request.scope)
                .map_err(ApiError::operation)?;
            identity
                .authorize(token, Capability::ExperienceRead, &request.scope)
                .map_err(ApiError::operation)?;
            let receipt = store
                .strategy_candidate(
                    &scope.tenant_id,
                    &scope.storage_namespace,
                    &request.strategy_id,
                )
                .map_err(ApiError::operation)?
                .ok_or_else(missing)?;
            Ok(Json(json!({"contract_version":1,"receipt":receipt})))
        })
        .await
}
