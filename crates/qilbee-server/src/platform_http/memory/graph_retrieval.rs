//! Experimental typed-path retrieval keeps authorization ahead of all candidate work.
use super::*;
use qilbee_memory::storage::platform::{GraphRankingVersion, GraphRetrievalQuery};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    contract_version: u32,
    scope: ResourceScope,
    query: GraphRetrievalQuery,
}
pub(super) fn routes() -> Router<PlatformState> {
    Router::new()
        .route(
            "/api/v1/memory/search/graph",
            post(search).layer(DefaultBodyLimit::max(
                super::super::retrieval_limits::VECTOR_BODY_BYTES,
            )),
        )
        .route("/api/v1/memory/graph-ranking-profiles", get(profiles))
}
async fn profiles(
    State(state): State<PlatformState>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    let limits = state.retrieval_limits.clone();
    state.run(headers, move |_, _, principal| {
        if !principal.spec.capabilities.contains(&Capability::MemoryRead) {
            return Err(ApiError::new(StatusCode::FORBIDDEN, "forbidden", "Memory read is not granted"));
        }
        let profiles: Vec<_> = GraphRankingVersion::ALL.into_iter().map(|v| v.profile()).collect();
        Ok(Json(json!({"contract_version":1,"graph_profiles":profiles,"execution_limits":limits.metadata()})))
    }).await
}
async fn search(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<Request>, JsonRejection>,
) -> ApiResult<Response> {
    let memory = state.memory.clone();
    let limits = state.retrieval_limits.clone();
    state
        .run(headers, move |identity, token, _| {
            let request = json_body(body)?;
            version(request.contract_version)?;
            let scope = identity
                .authorize(token, Capability::MemoryRead, &request.scope)
                .map_err(ApiError::operation)?;
            if let Some(space) = request.query.seed.embedding_space() {
                limits.dimensions(space.dimensions)?;
            }
            limits.scan_bytes(request.query.scan_bytes_limit)?;
            let _permit = limits.acquire()?;
            let retrieval_started = std::time::Instant::now();
            let page = memory
                .search_memory_graph(&scope.storage_namespace, &request.query)
                .map_err(ApiError::operation)?;
            let retrieval_micros = retrieval_started
                .elapsed()
                .as_micros()
                .min(u64::MAX as u128) as u64;
            let mut response =
                Json(json!({"contract_version":1,"scope":request.scope,"page":page}))
                    .into_response();
            response.headers_mut().insert(
                "x-qilbee-retrieval-micros",
                HeaderValue::from_str(&retrieval_micros.to_string())
                    .map_err(|_| ApiError::internal())?,
            );
            Ok(response)
        })
        .await
}
