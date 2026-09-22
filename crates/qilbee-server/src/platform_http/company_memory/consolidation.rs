//! Company-wide consolidation supervision with the authenticated administrator in audit.
use super::*;
use qilbee_memory::storage::platform::RecordAuthor;
use qilbee_memory::storage::platform::{
    AdminConsolidationQuery, ConsolidationCommand, ConsolidationOperation,
};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QueryRequest {
    contract_version: u32,
    workspace_id: String,
    query: AdminConsolidationQuery,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InspectRequest {
    contract_version: u32,
    workspace_id: String,
    owner_id: String,
    job_id: Uuid,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RevisionRequest {
    contract_version: u32,
    workspace_id: String,
    owner_id: String,
    job_id: Uuid,
    revision: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CancelRequest {
    contract_version: u32,
    workspace_id: String,
    owner_id: String,
    job_id: Uuid,
    expected_revision: u64,
    idempotency_key: String,
    evidence_ref: String,
}
pub(super) fn routes() -> Router<PlatformState> {
    Router::new()
        .route("/api/v1/company/memory/consolidation/query", post(query))
        .route(
            "/api/v1/company/memory/consolidation/inspect",
            post(inspect),
        )
        .route(
            "/api/v1/company/memory/consolidation/revision",
            post(revision),
        )
        .route("/api/v1/company/memory/consolidation/cancel", post(cancel))
}
async fn query(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<QueryRequest>, JsonRejection>,
) -> ApiResult<Response> {
    let memory = state.memory.clone();
    let limits = state.retrieval_limits.clone();
    state.run(headers, move |_, _, principal| {
  require_admin(&principal)?;
  let request = json_body(body)?; version(request.contract_version)?;
  let _permit = limits.acquire()?;
  let workspace = memory.company_memory_workspace(&principal.tenant_id, &request.workspace_id).map_err(ApiError::operation)?.ok_or_else(unavailable)?;
  let namespace = workspace.address().and_then(|a| a.namespace()).map_err(ApiError::operation)?;
  let value = memory.query_admin_consolidation_jobs(&namespace, &request.query).map_err(ApiError::operation)?;
  Ok(Json(json!({"contract_version":1,"company_id":principal.tenant_id,"workspace":workspace,"page":value})).into_response())
 }).await
}
async fn inspect(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<InspectRequest>, JsonRejection>,
) -> ApiResult<Response> {
    let memory = state.memory.clone();
    let limits = state.retrieval_limits.clone();
    state.run(headers, move |_, _, principal| {
  require_admin(&principal)?;
  let request = json_body(body)?; version(request.contract_version)?;
  let _permit = limits.acquire()?;
  let workspace = memory.company_memory_workspace(&principal.tenant_id, &request.workspace_id).map_err(ApiError::operation)?.ok_or_else(unavailable)?;
  let namespace = workspace.address().and_then(|a| a.namespace()).map_err(ApiError::operation)?;
  let value = memory.inspect_consolidation_job(&namespace, &request.owner_id, request.job_id).map_err(ApiError::operation)?.ok_or_else(unavailable)?;
  Ok(Json(json!({"contract_version":1,"company_id":principal.tenant_id,"workspace":workspace,"inspection":value})).into_response())
 }).await
}
async fn revision(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<RevisionRequest>, JsonRejection>,
) -> ApiResult<Response> {
    let memory = state.memory.clone();
    let limits = state.retrieval_limits.clone();
    state.run(headers, move |_, _, principal| {
  require_admin(&principal)?;
  let request = json_body(body)?; version(request.contract_version)?;
  let _permit = limits.acquire()?;
  let workspace = memory.company_memory_workspace(&principal.tenant_id, &request.workspace_id).map_err(ApiError::operation)?.ok_or_else(unavailable)?;
  let namespace = workspace.address().and_then(|a| a.namespace()).map_err(ApiError::operation)?;
  let value = memory.consolidation_job_revision(&namespace, &request.owner_id, request.job_id, request.revision).map_err(ApiError::operation)?.ok_or_else(unavailable)?;
  Ok(Json(json!({"contract_version":1,"company_id":principal.tenant_id,"workspace":workspace,"history":value})).into_response())
 }).await
}
async fn cancel(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<CancelRequest>, JsonRejection>,
) -> ApiResult<Response> {
    let memory = state.memory.clone();
    let limits = state.retrieval_limits.clone();
    state.run(headers, move |_, _, principal| {
  require_admin(&principal)?;
  let request = json_body(body)?; version(request.contract_version)?;
  let _permit = limits.acquire()?;
  let workspace = memory.company_memory_workspace(&principal.tenant_id, &request.workspace_id).map_err(ApiError::operation)?.ok_or_else(unavailable)?;
  let namespace = workspace.address().and_then(|a| a.namespace()).map_err(ApiError::operation)?;
  let value = memory.cancel_admin_consolidation_job(&namespace, &request.owner_id,
 &RecordAuthor { credential_id: principal.id, subject_id: principal.spec.subject_id },
 &ConsolidationCommand { contract_version: 1, idempotency_key: request.idempotency_key,
 operation: ConsolidationOperation::Cancel { job_id: request.job_id, expected_revision: request.expected_revision, evidence_ref: request.evidence_ref } }).map_err(ApiError::operation)?;
  Ok(Json(json!({"contract_version":1,"company_id":principal.tenant_id,"workspace":workspace,"receipt":value})).into_response())
 }).await
}
