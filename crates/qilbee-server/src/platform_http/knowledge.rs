//! Evidence-bound knowledge; no tool code registration or execution authority.
use super::*;
use crate::security::identity::ResourceScope;
use qilbee_memory::learning::{ExternalToolIdentity, KnowledgeProposal, KnowledgeSelectRequest};
pub(super) fn routes() -> Router<PlatformState> {
    Router::new()
        .route("/api/v1/learning/knowledge/proposals", post(propose))
        .route("/api/v1/learning/knowledge/inspect", post(inspect))
        .route("/api/v1/learning/knowledge/select", post(select))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposalRequest {
    contract_version: u32,
    scope: ResourceScope,
    proposal: KnowledgeProposal,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InspectRequest {
    contract_version: u32,
    scope: ResourceScope,
    procedure_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SelectRequest {
    contract_version: u32,
    scope: ResourceScope,
    policy_id: String,
    context_id: String,
    max_instruction_bytes: usize,
    candidate_limit: usize,
    external_tool_identities: Vec<ExternalToolIdentity>,
}
fn version2(version: u32) -> ApiResult<()> {
    if version == 2 {
        Ok(())
    } else {
        Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Knowledge requires contract_version 2",
        ))
    }
}
async fn propose(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<ProposalRequest>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let learning = state.learning.clone();
    state
        .run(headers, move |identity, token, principal| {
            if !principal
                .spec
                .capabilities
                .contains(&Capability::MemoryRead)
            {
                return Err(ApiError::new(
                    StatusCode::FORBIDDEN,
                    "forbidden",
                    "Memory read capability is required",
                ));
            }
            let request = json_body(body)?;
            version2(request.contract_version)?;
            let scope = identity
                .authorize(token, Capability::ProcedurePropose, &request.scope)
                .map_err(ApiError::operation)?;
            let receipt = learning
                .propose_knowledge(
                    &scope.tenant_id,
                    &scope.storage_namespace,
                    request.proposal,
                    &format!("{}:{}", scope.credential_id, scope.subject_id),
                )
                .map_err(ApiError::operation)?;
            Ok(Json(json!({"contract_version":2,"receipt":receipt})))
        })
        .await
}
async fn inspect(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<InspectRequest>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let learning = state.learning.clone();
    let memory = state.memory.clone();
    let limits = state.retrieval_limits.clone();
    state
        .run(headers, move |identity, token, _| {
            let request = json_body(body)?;
            version2(request.contract_version)?;
            let scope = identity
                .authorize(token, Capability::MemoryRead, &request.scope)
                .map_err(ApiError::operation)?;
            let _permit = limits.acquire()?;
            let inspection = learning
                .inspect_knowledge(
                    &memory,
                    &scope.tenant_id,
                    &scope.storage_namespace,
                    &request.procedure_id,
                )
                .map_err(ApiError::operation)?
                .ok_or_else(|| {
                    ApiError::new(
                        StatusCode::NOT_FOUND,
                        "record_not_found",
                        "No knowledge proposal exists in the authorized namespace",
                    )
                })?;
            Ok(Json(json!({"contract_version":2,"inspection":inspection})))
        })
        .await
}
async fn select(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<SelectRequest>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let learning = state.learning.clone();
    let memory = state.memory.clone();
    let limits = state.retrieval_limits.clone();
    state.run(headers,move|identity,token,_| {
        let request=json_body(body)?;version2(request.contract_version)?;
        let scope=identity.authorize(token,Capability::MemoryRead,&request.scope).map_err(ApiError::operation)?;
        let _permit=limits.acquire()?;
        let selection=learning.select_knowledge(&memory,&scope.tenant_id,&scope.storage_namespace,KnowledgeSelectRequest {policy_id:request.policy_id.clone(),context_id:request.context_id.clone(),max_instruction_bytes:request.max_instruction_bytes,candidate_limit:request.candidate_limit,external_tool_identities:request.external_tool_identities}).map_err(ApiError::operation)?;
        Ok(Json(json!({"contract_version":2,"policy_id":request.policy_id,"context_id":request.context_id,"result":selection})))
    }).await
}
