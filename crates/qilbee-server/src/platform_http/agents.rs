//! Observe external agent identities only after successful authorized requests.
use super::*;
use crate::security::identity::{AuthorizedScope, ResourceScope};
use axum::extract::{Query, rejection::QueryRejection};
use qilbee_memory::storage::platform::{AgentObservation, RecordAuthor};
use std::{cell::RefCell, collections::BTreeMap, ops::Deref};

pub(super) struct RequestIdentity<'a> {
    inner: &'a IdentityStore,
    observations: RefCell<BTreeMap<(String, String), AgentObservation>>,
}

impl<'a> RequestIdentity<'a> {
    pub(super) fn new(inner: &'a IdentityStore) -> Self {
        Self {
            inner,
            observations: RefCell::new(BTreeMap::new()),
        }
    }
    pub(super) fn authorize(
        &self,
        token: &str,
        capability: Capability,
        scope: &ResourceScope,
    ) -> qilbee_core::Result<AuthorizedScope> {
        let authorized = self.inner.authorize(token, capability, scope)?;
        let observation = observation(&authorized, scope);
        self.observations
            .borrow_mut()
            .entry((observation.company_id.clone(), observation.agent_id.clone()))
            .or_insert(observation);
        Ok(authorized)
    }
    pub(super) fn record_success(
        &self,
        memory: &qilbee_memory::RocksDbMemoryStorage,
    ) -> ApiResult<()> {
        for observation in self.observations.borrow().values() {
            memory
                .observe_agent(observation)
                .map_err(ApiError::operation)?;
        }
        Ok(())
    }
}
impl Deref for RequestIdentity<'_> {
    type Target = IdentityStore;
    fn deref(&self) -> &Self::Target {
        self.inner
    }
}

pub(super) fn observation(authorized: &AuthorizedScope, scope: &ResourceScope) -> AgentObservation {
    AgentObservation {
        company_id: authorized.tenant_id.clone(),
        agent_id: scope.agent_id.clone(),
        namespace: authorized.storage_namespace.clone(),
        author: RecordAuthor {
            credential_id: authorized.credential_id,
            subject_id: authorized.subject_id.clone(),
        },
    }
}

pub(super) fn routes() -> Router<PlatformState> {
    Router::new().route("/api/v1/agents", get(directory))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DirectoryQuery {
    contract_version: u32,
    #[serde(default = "default_limit")]
    limit: usize,
    after_agent_id: Option<String>,
}
fn default_limit() -> usize {
    25
}

async fn directory(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    query: Result<Query<DirectoryQuery>, QueryRejection>,
) -> ApiResult<Json<Value>> {
    let memory = state.memory.clone();
    state.run(headers, move |_, _, principal| {
        require_admin(&principal)?;
        let Query(query) = query.map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "invalid_request", "Invalid agent directory query"))?;
        version(query.contract_version)?;
        let page = memory.list_observed_agents(&principal.tenant_id, query.after_agent_id.as_deref(), query.limit).map_err(ApiError::operation)?;
        let mut entries = Vec::with_capacity(page.agents.len());
        for agent in page.agents {
            let encoded = agent.first_namespace.strip_prefix("qdb:scope:v1:").ok_or_else(ApiError::internal)?;
            let (company, scope, private_subject): (String, ResourceScope, Option<String>) = serde_json::from_str(encoded).map_err(|_| ApiError::internal())?;
            let expected_subject = match scope.visibility {
                crate::security::identity::Visibility::Private => Some(agent.registered_by.subject_id.clone()),
                crate::security::identity::Visibility::Shared => None,
            };
            if company != principal.tenant_id || agent.company_id != company || agent.agent_id != scope.agent_id || private_subject != expected_subject {
                return Err(ApiError::internal());
            }
            entries.push(json!({"agent_id":agent.agent_id,"registered_at_millis":agent.registered_at_millis,"registered_by":agent.registered_by,"first_scope":scope,"private_subject_id":private_subject,"trigger":agent.trigger}));
        }
        Ok(Json(json!({"contract_version":1,"company_id":principal.tenant_id,"page":{"agents":entries,"next_after_agent_id":page.next_after_agent_id}})))
    }).await
}
