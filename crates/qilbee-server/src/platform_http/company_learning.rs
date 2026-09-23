//! Native company learning catalogs. Caller-selected tenants are never accepted.
use super::*;
use qilbee_memory::learning::{
    LearningCatalogQuery, LearningCatalogQueryV2, LearningEvidenceQuery, LearningEvidenceRef,
    LearningResourceRef,
};

pub(super) fn routes() -> Router<PlatformState> {
    Router::new()
        .route("/api/v1/company/learning/query", post(query))
        .route("/api/v1/company/learning/read", post(read))
        .route(
            "/api/v1/company/learning/knowledge/inspect",
            post(inspect_knowledge),
        )
        .route(
            "/api/v1/company/learning/evidence/query",
            post(evidence_query),
        )
        .route(
            "/api/v1/company/learning/evidence/read",
            post(evidence_read),
        )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogRequest {
    contract_version: u32,
    query: Value,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InspectionRequest {
    contract_version: u32,
    resource: LearningResourceRef,
}

async fn query(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<CatalogRequest>, JsonRejection>,
) -> ApiResult<Response> {
    let learning = state.learning.clone();
    let limits = state.retrieval_limits.clone();
    state
        .run(headers, move |_, _, principal| {
            require_admin(&principal)?;
            let request = json_body(body)?;
            let _permit = limits.acquire()?;
            match request.contract_version {
                1 => {
                    let query: LearningCatalogQuery = serde_json::from_value(request.query)
                        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "invalid_request", "Invalid version 1 catalog query"))?;
                    let page = learning.company_learning_catalog(&principal.tenant_id, &query).map_err(ApiError::operation)?;
                    Ok(Json(json!({"contract_version":1,"company_id":principal.tenant_id,"page":page})).into_response())
                }
                2 => {
                    let query: LearningCatalogQueryV2 = serde_json::from_value(request.query)
                        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "invalid_request", "Invalid version 2 catalog query"))?;
                    let accepted = query.canonical_origins().map_err(ApiError::operation)?;
                    let page = learning.company_learning_catalog_with_origin(&principal.tenant_id, &query).map_err(ApiError::operation)?;
                    Ok(Json(json!({"contract_version":2,"company_id":principal.tenant_id,"accepted_origin_kinds":accepted,"page":page})).into_response())
                }
                _ => Err(ApiError::new(StatusCode::BAD_REQUEST, "invalid_request", "Learning catalog requires contract_version 1 or 2")),
            }
        })
        .await
}

async fn read(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<InspectionRequest>, JsonRejection>,
) -> ApiResult<Response> {
    let learning = state.learning.clone();
    let limits = state.retrieval_limits.clone();
    state.run(headers, move |_, _, principal| {
        require_admin(&principal)?;
        let request = json_body(body)?;
        if !matches!(request.contract_version, 1 | 2) {
            return Err(ApiError::new(StatusCode::BAD_REQUEST, "invalid_request", "Learning resource read requires contract_version 1 or 2"));
        }
        let _permit = limits.acquire()?;
        if request.contract_version == 2 {
            let (details, origin) = learning.inspect_company_learning_resource_with_origin(&principal.tenant_id, &request.resource)
                .map_err(ApiError::operation)?.ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "record_not_found", "No learning resource exists in the authorized company"))?;
            return Ok(Json(json!({"contract_version":2,"company_id":principal.tenant_id,"resource":request.resource,"details":details,"origin":origin})).into_response());
        }
        let details = learning.inspect_company_learning_resource(&principal.tenant_id, &request.resource)
            .map_err(ApiError::operation)?.ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "record_not_found", "No learning resource exists in the authorized company"))?;
        Ok(Json(json!({"contract_version":1,"company_id":principal.tenant_id,"resource":request.resource,"details":details})).into_response())
    }).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EvidenceQueryRequest {
    contract_version: u32,
    query: LearningEvidenceQuery,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EvidenceReadRequest {
    contract_version: u32,
    evidence: LearningEvidenceRef,
}
async fn evidence_query(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<EvidenceQueryRequest>, JsonRejection>,
) -> ApiResult<Response> {
    let learning = state.learning.clone();
    let limits = state.retrieval_limits.clone();
    state
        .run(headers, move |_, _, principal| {
            require_admin(&principal)?;
            let request = json_body(body)?;
            version(request.contract_version)?;
            let _permit = limits.acquire()?;
            let page = learning
                .company_learning_evidence(&principal.tenant_id, &request.query)
                .map_err(ApiError::operation)?;
            Ok(
                Json(json!({"contract_version":1,"company_id":principal.tenant_id,"page":page}))
                    .into_response(),
            )
        })
        .await
}
async fn evidence_read(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<EvidenceReadRequest>, JsonRejection>,
) -> ApiResult<Response> {
    let learning = state.learning.clone();
    let limits = state.retrieval_limits.clone();
    state.run(headers, move |_, _, principal| {
        require_admin(&principal)?;
        let request = json_body(body)?; version(request.contract_version)?;
        let _permit = limits.acquire()?;
        let details = learning.inspect_company_learning_evidence(&principal.tenant_id, &request.evidence).map_err(ApiError::operation)?.ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND,"record_not_found","No evidence exists for the selected company resource"))?;
        Ok(Json(json!({"contract_version":1,"company_id":principal.tenant_id,"evidence":request.evidence,"details":details})).into_response())
    }).await
}

async fn inspect_knowledge(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<InspectionRequest>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let learning = state.learning.clone();
    let memory = state.memory.clone();
    let limits = state.retrieval_limits.clone();
    state.run(headers,move|_,_,principal| {
        require_admin(&principal)?;
        let request=json_body(body)?;
        if !matches!(request.contract_version, 2 | 3) {
            return Err(ApiError::new(StatusCode::BAD_REQUEST,"invalid_request","Knowledge inspection requires contract_version 2 or 3"));
        }
        let _permit=limits.acquire()?;
        if request.contract_version == 3 {
            let (inspection, origin) = learning.inspect_company_knowledge_with_origin(
                &memory, &principal.tenant_id, &request.resource,
            ).map_err(ApiError::operation)?.ok_or_else(|| ApiError::new(
                StatusCode::NOT_FOUND, "record_not_found", "No knowledge proposal exists in the authorized company",
            ))?;
            return Ok(Json(json!({"contract_version":3,"company_id":principal.tenant_id,"resource":request.resource,"inspection":inspection,"origin":origin})));
        }
        let inspection=learning.inspect_company_knowledge(&memory,&principal.tenant_id,&request.resource).map_err(ApiError::operation)?.ok_or_else(||ApiError::new(StatusCode::NOT_FOUND,"record_not_found","No knowledge proposal exists in the authorized company"))?;
        Ok(Json(json!({"contract_version":2,"company_id":principal.tenant_id,"resource":request.resource,"inspection":inspection})))
    }).await
}
