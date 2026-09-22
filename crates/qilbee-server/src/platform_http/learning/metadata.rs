//! Company metadata discovery without memory scope grants.
use super::*;
use qilbee_memory::learning::LearningMetadataQuery;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct MetadataRequest {
    contract_version: u32,
    query: LearningMetadataQuery,
}

pub(super) async fn query(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<MetadataRequest>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let learning = state.learning.clone();
    let limits = state.retrieval_limits.clone();
    state
        .run(headers, move |_, _, actor| {
            require_metadata_read(&actor)?;
            let request = json_body(body)?;
            version(request.contract_version)?;
            let _permit = limits.acquire()?;
            let page = learning
                .company_learning_metadata(&actor.tenant_id, &request.query)
                .map_err(ApiError::operation)?;
            Ok(Json(
                json!({"contract_version":1,"company_id":actor.tenant_id,"page":page}),
            ))
        })
        .await
}
