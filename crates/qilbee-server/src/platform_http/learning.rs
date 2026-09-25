//! Authenticated administrative contracts and scoped procedural learning.
use super::*;
use crate::security::identity::ResourceScope;
use qilbee_memory::learning::{
    EvaluationActor, EvaluationContext, EvaluationSubmission, PolicyDefinition, RegisteredProposal,
};
mod strategies;
mod metadata;

pub(super) fn routes() -> Router<PlatformState> {
    Router::new()
        .merge(strategies::routes())
        .route("/api/v1/learning/metadata/query", post(metadata::query))
        .route("/api/v1/learning/policies", post(register_policy))
        .route("/api/v1/learning/policies/:id", get(policy))
        .route("/api/v1/learning/contexts", post(register_context))
        .route("/api/v1/learning/contexts/:id", get(context))
        .route("/api/v1/learning/proposals", post(propose))
        .route("/api/v1/learning/procedures/read", post(read))
        .route("/api/v1/learning/evaluations", post(evaluate))
        .route("/api/v1/learning/evaluations/read", post(receipt))
        .route("/api/v1/learning/select", post(select))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PolicyRequest {
    contract_version: u32,
    id: String,
    definition: PolicyDefinition,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ContextRequest {
    contract_version: u32,
    id: String,
    context: EvaluationContext,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposalRequest {
    contract_version: u32,
    scope: ResourceScope,
    proposal: RegisteredProposal,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadRequest {
    contract_version: u32,
    scope: ResourceScope,
    procedure_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EvaluationRequest {
    contract_version: u32,
    scope: ResourceScope,
    procedure_id: String,
    submission: EvaluationSubmission,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReceiptRequest {
    contract_version: u32,
    scope: ResourceScope,
    procedure_id: String,
    case_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SelectRequest {
    contract_version: u32,
    scope: ResourceScope,
    policy_id: String,
    context_id: String,
    max_instruction_bytes: usize,
}
fn require_policy_admin(actor: &CredentialView) -> ApiResult<()> {
    if actor.spec.capabilities.contains(&Capability::PolicyAdmin) {
        Ok(())
    } else {
        Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "forbidden",
            "Policy administration is not granted",
        ))
    }
}
fn require_metadata_read(actor: &CredentialView) -> ApiResult<()> {
    if actor.spec.capabilities.contains(&Capability::LearningMetadataRead)
        || actor.spec.capabilities.contains(&Capability::PolicyAdmin)
    {
        Ok(())
    } else {
        Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "forbidden",
            "Learning metadata read capability is required",
        ))
    }
}
fn author(actor: &CredentialView) -> String {
    format!("{}:{}", actor.id, actor.spec.subject_id)
}
fn missing() -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        "record_not_found",
        "No learning record exists in the authorized namespace",
    )
}
async fn register_policy(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<PolicyRequest>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let learning = state.learning.clone();
    state
        .run(headers, move |_, _, actor| {
            require_policy_admin(&actor)?;
            let request = json_body(body)?;
            version(request.contract_version)?;
            let entry = learning
                .register_policy(
                    &actor.tenant_id,
                    &request.id,
                    request.definition,
                    &author(&actor),
                )
                .map_err(ApiError::operation)?;
            Ok(Json(json!({"contract_version":1,"entry":entry})))
        })
        .await
}
async fn register_context(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<ContextRequest>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let learning = state.learning.clone();
    state
        .run(headers, move |_, _, actor| {
            require_policy_admin(&actor)?;
            let request = json_body(body)?;
            version(request.contract_version)?;
            let entry = learning
                .register_context(
                    &actor.tenant_id,
                    &request.id,
                    request.context,
                    &author(&actor),
                )
                .map_err(ApiError::operation)?;
            Ok(Json(json!({"contract_version":1,"entry":entry})))
        })
        .await
}
async fn policy(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let learning = state.learning.clone();
    state
        .run(headers, move |_, _, actor| {
            require_metadata_read(&actor)?;
            let entry = learning
                .policy(&actor.tenant_id, &id)
                .map_err(ApiError::operation)?
                .ok_or_else(missing)?;
            Ok(Json(json!({"contract_version":1,"entry":entry})))
        })
        .await
}
async fn context(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let learning = state.learning.clone();
    state
        .run(headers, move |_, _, actor| {
            require_metadata_read(&actor)?;
            let entry = learning
                .context(&actor.tenant_id, &id)
                .map_err(ApiError::operation)?
                .ok_or_else(missing)?;
            Ok(Json(json!({"contract_version":1,"entry":entry})))
        })
        .await
}
async fn propose(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<ProposalRequest>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let learning = state.learning.clone();
    state
        .run(headers, move |identity, token, actor| {
            let request = json_body(body)?;
            version(request.contract_version)?;
            let scope = identity
                .authorize(token, Capability::ProcedurePropose, &request.scope)
                .map_err(ApiError::operation)?;
            let receipt = learning
                .propose_registered(
                    &scope.tenant_id,
                    &scope.storage_namespace,
                    request.proposal,
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
    let learning = state.learning.clone();
    state
        .run(headers, move |identity, token, _| {
            let request = json_body(body)?;
            version(request.contract_version)?;
            let scope = identity
                .authorize(token, Capability::MemoryRead, &request.scope)
                .map_err(ApiError::operation)?;
            let procedure = learning
                .registered_procedure(
                    &scope.tenant_id,
                    &scope.storage_namespace,
                    &request.procedure_id,
                )
                .map_err(ApiError::operation)?
                .ok_or_else(missing)?;
            Ok(Json(json!({"contract_version":1,"procedure":procedure})))
        })
        .await
}
async fn evaluate(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<EvaluationRequest>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let learning = state.learning.clone();
    state
        .run(headers, move |identity, token, _| {
            let request = json_body(body)?;
            version(request.contract_version)?;
            let scope = identity
                .authorize(token, Capability::ProcedureEvaluate, &request.scope)
                .map_err(ApiError::operation)?;
            let receipt = learning
                .admit_evaluation(
                    &scope.tenant_id,
                    &scope.storage_namespace,
                    &request.procedure_id,
                    request.submission,
                    EvaluationActor {
                        subject_id: scope.subject_id,
                        credential_id: scope.credential_id.to_string(),
                    },
                )
                .map_err(ApiError::operation)?;
            Ok(Json(json!({"contract_version":1,"receipt":receipt})))
        })
        .await
}
async fn receipt(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<ReceiptRequest>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let learning = state.learning.clone();
    state
        .run(headers, move |identity, token, _| {
            let request = json_body(body)?;
            version(request.contract_version)?;
            let scope = identity
                .authorize(token, Capability::MemoryRead, &request.scope)
                .map_err(ApiError::operation)?;
            let receipt = learning
                .admission(
                    &scope.tenant_id,
                    &scope.storage_namespace,
                    &request.procedure_id,
                    &request.case_id,
                )
                .map_err(ApiError::operation)?
                .ok_or_else(missing)?;
            Ok(Json(json!({"contract_version":1,"receipt":receipt})))
        })
        .await
}
async fn select(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<SelectRequest>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let learning = state.learning.clone();
    let limits = state.retrieval_limits.clone();
    state.run(headers,move|identity,token,_| {
        let request=json_body(body)?;version(request.contract_version)?;
        if request.max_instruction_bytes>64*1024 { return Err(ApiError::new(StatusCode::BAD_REQUEST,"invalid_request","Instruction byte budget must be at most 65536")); }
        let scope=identity.authorize(token,Capability::MemoryRead,&request.scope).map_err(ApiError::operation)?;
        let _permit=limits.acquire()?;
        let selected=learning.select_registered(&scope.tenant_id,&scope.storage_namespace,&request.policy_id,&request.context_id,request.max_instruction_bytes).map_err(ApiError::operation)?;
        let selection=match selected {
            Some(procedure)=>json!({"type":"procedure","procedure":procedure}),
            None=>{
                let context=learning.context(&scope.tenant_id,&request.context_id).map_err(ApiError::operation)?.ok_or_else(missing)?;
                json!({"type":"baseline","baseline_revision":context.payload.baseline_revision,"reason":"no_qualified_compatible_procedure"})
            }
        };
        Ok(Json(json!({"contract_version":1,"policy_id":request.policy_id,"context_id":request.context_id,"selection":selection})))
    }).await
}
