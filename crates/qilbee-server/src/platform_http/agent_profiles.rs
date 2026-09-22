//! Company-owned labels for externally registered agent identities.
use super::*;
use qilbee_memory::storage::platform::{
    AgentDirectoryQuery, AgentProfileCommand, CompanyMemoryAddress, NamedAgent, RecordAuthor,
};

pub(super) fn routes() -> Router<PlatformState> {
    Router::new()
        .route("/api/v1/company/agents/query", post(query))
        .route("/api/v1/company/agents/read", post(read))
        .route("/api/v1/company/agents/commands", post(command))
        .route("/api/v1/company/agents/history", post(history))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QueryRequest {
    contract_version: u32,
    query: AgentDirectoryQuery,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadRequest {
    contract_version: u32,
    agent_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CommandRequest {
    contract_version: u32,
    command: AgentProfileCommand,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoryRequest {
    contract_version: u32,
    agent_id: String,
    #[serde(default)]
    after_revision: u64,
    #[serde(default = "default_limit")]
    limit: usize,
}
fn default_limit() -> usize {
    25
}
fn view(agent: NamedAgent) -> ApiResult<Value> {
    let r = agent.registration;
    let address = CompanyMemoryAddress::from_namespace(&r.first_namespace)
        .map_err(ApiError::operation)?
        .ok_or_else(ApiError::internal)?;
    let expected_subject = match address.scope.visibility {
        qilbee_memory::storage::platform::MemoryVisibility::Private => {
            Some(r.registered_by.subject_id.clone())
        }
        qilbee_memory::storage::platform::MemoryVisibility::Shared => None,
    };
    if address.company_id != r.company_id
        || address.scope.agent_id != r.agent_id
        || address.private_subject_id != expected_subject
    {
        return Err(ApiError::internal());
    }
    Ok(
        json!({"agent_id":r.agent_id,"registered_at_millis":r.registered_at_millis,"registered_by":r.registered_by,"first_scope":address.scope,"private_subject_id":address.private_subject_id,"trigger":r.trigger,"profile":agent.profile}),
    )
}
async fn query(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<QueryRequest>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let memory = state.memory.clone();
    let limits = state.retrieval_limits.clone();
    state.run(headers,move|_,_,principal|{
        require_admin(&principal)?;let body=json_body(body)?;version(body.contract_version)?;let _permit=limits.acquire()?;
        let page=memory.query_named_agents(&principal.tenant_id,&body.query).map_err(ApiError::operation)?;
        let agents=page.agents.into_iter().map(view).collect::<ApiResult<Vec<_>>>()?;
        Ok(Json(json!({"contract_version":1,"company_id":principal.tenant_id,"page":{"agents":agents,"next_cursor":page.next_cursor,"scanned_agents":page.scanned_agents,"observed_at_millis":page.observed_at_millis}})))
    }).await
}
async fn read(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<ReadRequest>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let memory = state.memory.clone();
    let limits = state.retrieval_limits.clone();
    state
        .run(headers, move |_, _, principal| {
            require_admin(&principal)?;
            let body = json_body(body)?;
            version(body.contract_version)?;
            let _permit = limits.acquire()?;
            let agent = memory
                .named_agent(&principal.tenant_id, &body.agent_id)
                .map_err(ApiError::operation)?
                .ok_or_else(|| ApiError::operation(Error::KeyNotFound("Unknown agent".into())))?;
            Ok(Json(
                json!({"contract_version":1,"company_id":principal.tenant_id,"agent":view(agent)?}),
            ))
        })
        .await
}
async fn command(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<CommandRequest>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let memory = state.memory.clone();
    state
        .run(headers, move |_, _, principal| {
            require_admin(&principal)?;
            let body = json_body(body)?;
            version(body.contract_version)?;
            let actor = RecordAuthor {
                credential_id: principal.id,
                subject_id: principal.spec.subject_id.clone(),
            };
            let result = memory
                .apply_agent_profile_command(&principal.tenant_id, &actor, &body.command)
                .map_err(ApiError::operation)?;
            Ok(Json(
                json!({"contract_version":1,"company_id":principal.tenant_id,"result":result}),
            ))
        })
        .await
}
async fn history(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<HistoryRequest>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let memory = state.memory.clone();
    let limits = state.retrieval_limits.clone();
    state.run(headers,move|_,_,principal|{
        require_admin(&principal)?;let body=json_body(body)?;version(body.contract_version)?;let _permit=limits.acquire()?;
        let page=memory.agent_profile_history(&principal.tenant_id,&body.agent_id,body.after_revision,body.limit).map_err(ApiError::operation)?;
        Ok(Json(json!({"contract_version":1,"company_id":principal.tenant_id,"agent_id":body.agent_id,"page":page})))
    }).await
}
