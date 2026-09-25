//! Evidence-bound knowledge; no tool code registration or execution authority.
use super::*;
use crate::security::identity::ResourceScope;
use qilbee_memory::learning::{
    ExternalToolIdentity, KnowledgeOriginKind, KnowledgeProposal, KnowledgeSelectRequest,
    KnowledgeSelectRequestV3, KnowledgeSelectRequestV4, KnowledgeSelectionWorkLimits,
};
pub(super) fn routes() -> Router<PlatformState> {
    Router::new()
        .route("/api/v1/learning/knowledge/proposals", post(propose))
        .route(
            "/api/v1/learning/knowledge/experience-proposals",
            post(propose_combined),
        )
        .route(
            "/api/v1/learning/knowledge/experience-proposals/read",
            post(read_combined),
        )
        .route(
            "/api/v1/learning/knowledge/experience-proposals/inspect",
            post(inspect_combined),
        )
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
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SelectRequestV3 {
    contract_version: u32,
    selection_version: String,
    scope: ResourceScope,
    policy_id: String,
    context_id: String,
    max_instruction_bytes: usize,
    external_tool_identities: Vec<ExternalToolIdentity>,
    work_limits: KnowledgeSelectionWorkLimits,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SelectRequestV4 {
    contract_version: u32,
    selection_version: String,
    accepted_origin_kinds: Vec<KnowledgeOriginKind>,
    scope: ResourceScope,
    policy_id: String,
    context_id: String,
    max_instruction_bytes: usize,
    external_tool_identities: Vec<ExternalToolIdentity>,
    work_limits: KnowledgeSelectionWorkLimits,
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
    body: Result<Json<Value>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let learning = state.learning.clone();
    let memory = state.memory.clone();
    let limits = state.retrieval_limits.clone();
    state.run(headers, move |identity, token, _| {
        let value = json_body(body)?;
        let invalid = |message: &'static str| ApiError::new(StatusCode::BAD_REQUEST, "invalid_request", message);
        match value.get("contract_version").and_then(Value::as_u64) {
            Some(2) => {
                let request: SelectRequest = serde_json::from_value(value)
                    .map_err(|_| invalid("Invalid knowledge selection request fields"))?;
                version2(request.contract_version)?;
                let scope = identity.authorize(token, Capability::MemoryRead, &request.scope).map_err(ApiError::operation)?;
                let _permit = limits.acquire()?;
                let selection = learning.select_knowledge(&memory, &scope.tenant_id, &scope.storage_namespace,
                    KnowledgeSelectRequest { policy_id: request.policy_id.clone(), context_id: request.context_id.clone(),
                        max_instruction_bytes: request.max_instruction_bytes, candidate_limit: request.candidate_limit,
                        external_tool_identities: request.external_tool_identities }).map_err(ApiError::operation)?;
                Ok(Json(json!({"contract_version":2,"policy_id":request.policy_id,"context_id":request.context_id,"result":selection})))
            }
            Some(3) => {
                let request: SelectRequestV3 = serde_json::from_value(value)
                    .map_err(|_| invalid("Invalid knowledge selection request fields"))?;
                let scope = identity.authorize(token, Capability::MemoryRead, &request.scope).map_err(ApiError::operation)?;
                let _permit = limits.acquire()?;
                let selection = learning.select_knowledge_v3(&memory, &scope.tenant_id, &scope.storage_namespace,
                    KnowledgeSelectRequestV3 { selection_version: request.selection_version,
                        policy_id: request.policy_id.clone(), context_id: request.context_id.clone(),
                        max_instruction_bytes: request.max_instruction_bytes,
                        external_tool_identities: request.external_tool_identities,
                        work_limits: request.work_limits }).map_err(ApiError::operation)?;
                Ok(Json(json!({"contract_version":request.contract_version,"scope":request.scope,
                    "policy_id":request.policy_id,"context_id":request.context_id,
                    "selection_version":selection.selection_version,"evaluated_at_millis":selection.evaluated_at_millis,
                    "index_generation":selection.index_generation,"result":selection.result,
                    "coverage":selection.coverage,"work":selection.work})))
            }
            Some(4) => {
                let request: SelectRequestV4 = serde_json::from_value(value)
                    .map_err(|_| invalid("Invalid knowledge selection request fields"))?;
                let scope = identity.authorize(token, Capability::MemoryRead, &request.scope).map_err(ApiError::operation)?;
                let _permit = limits.acquire()?;
                let selection = learning.select_knowledge_v4(&memory, &scope.tenant_id, &scope.storage_namespace,
                    KnowledgeSelectRequestV4 { selection_version: request.selection_version, accepted_origin_kinds: request.accepted_origin_kinds,
                        policy_id: request.policy_id.clone(), context_id: request.context_id.clone(),
                        max_instruction_bytes: request.max_instruction_bytes,
                        external_tool_identities: request.external_tool_identities,
                        work_limits: request.work_limits }).map_err(ApiError::operation)?;
                Ok(Json(json!({"contract_version":request.contract_version,"scope":request.scope,
                    "policy_id":request.policy_id,"context_id":request.context_id,
                    "selection_version":selection.selection_version,"evaluated_at_millis":selection.evaluated_at_millis,
                    "index_generation":selection.index_generation,"result":selection.result,
                    "accepted_origin_kinds":selection.accepted_origin_kinds,"coverage":selection.coverage,"work":selection.work})))
            }
            _ => Err(invalid("Knowledge selection requires contract_version 2, 3 or 4")),
        }
    }).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CombinedProposalRequest {
    contract_version: u32,
    scope: ResourceScope,
    proposal: qilbee_memory::learning::CombinedKnowledgeInput,
}

async fn propose_combined(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<CombinedProposalRequest>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let learning = state.learning.clone();
    let memory = state.memory.clone();
    let limits = state.retrieval_limits.clone();
    state
        .run(headers, move |identity, token, _| {
            let request = json_body(body)?;
            version(request.contract_version)?;
            identity
                .authorize(token, Capability::MemoryRead, &request.scope)
                .map_err(ApiError::operation)?;
            identity
                .authorize(token, Capability::ExperienceRead, &request.scope)
                .map_err(ApiError::operation)?;
            let scope = identity
                .authorize(token, Capability::ProcedurePropose, &request.scope)
                .map_err(ApiError::operation)?;
            let _permit = limits.acquire()?;
            let receipt = learning
                .propose_combined_knowledge(
                    memory.as_ref(),
                    &scope.tenant_id,
                    &scope.storage_namespace,
                    request.proposal,
                    &format!("{}:{}", scope.credential_id, scope.subject_id),
                )
                .map_err(ApiError::operation)?;
            Ok(Json(json!({"contract_version":1,"receipt":receipt})))
        })
        .await
}

async fn read_combined(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<InspectRequest>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let learning = state.learning.clone();
    let limits = state.retrieval_limits.clone();
    state
        .run(headers, move |identity, token, _| {
            let request = json_body(body)?;
            version(request.contract_version)?;
            identity
                .authorize(token, Capability::ExperienceRead, &request.scope)
                .map_err(ApiError::operation)?;
            let scope = identity
                .authorize(token, Capability::MemoryRead, &request.scope)
                .map_err(ApiError::operation)?;
            let _permit = limits.acquire()?;
            let receipt = learning
                .combined_knowledge_receipt(
                    &scope.tenant_id,
                    &scope.storage_namespace,
                    &request.procedure_id,
                )
                .map_err(ApiError::operation)?
                .ok_or_else(|| {
                    ApiError::new(
                        StatusCode::NOT_FOUND,
                        "record_not_found",
                        "No combined knowledge exists in the authorized scope",
                    )
                })?;
            Ok(Json(json!({"contract_version":1,"receipt":receipt})))
        })
        .await
}

async fn inspect_combined(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<InspectRequest>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let learning = state.learning.clone();
    let memory = state.memory.clone();
    let limits = state.retrieval_limits.clone();
    state.run(headers, move |identity, token, _| {
        let request = json_body(body)?;
        version(request.contract_version)?;
        let scope = identity.authorize(token, Capability::MemoryRead, &request.scope).map_err(ApiError::operation)?;
        let _permit = limits.acquire()?;
        let (inspection, origin) = learning.inspect_knowledge_with_origin(
            &memory, &scope.tenant_id, &scope.storage_namespace, &request.procedure_id,
        ).map_err(ApiError::operation)?.ok_or_else(|| ApiError::new(
            StatusCode::NOT_FOUND, "record_not_found", "No knowledge exists in the authorized scope",
        ))?;
        if !matches!(origin, qilbee_memory::learning::KnowledgeOriginDescriptor::ExperienceMemory { .. }) {
            return Err(ApiError::new(StatusCode::CONFLICT, "unsupported_knowledge_origin", "This route requires combined knowledge origin"));
        }
        Ok(Json(json!({"contract_version":1,"scope":request.scope,"inspection":inspection,"origin":origin})))
    }).await
}
