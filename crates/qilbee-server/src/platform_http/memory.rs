//! Scoped transport adapter for atomic versioned memory commands.
use super::*;
use crate::security::identity::{ResourceScope, Visibility};
use axum::extract::{Query, rejection::QueryRejection};
use qilbee_memory::storage::platform::{MemoryCommand, MemoryOperation, MemoryQuery, RecordAuthor};

pub(super) fn routes() -> Router<PlatformState> {
    Router::new()
        .route("/api/v1/memory/commands", post(command))
        .route("/api/v1/memory/records/:id", get(read))
        .route("/api/v1/memory/query", post(query))
        .route(
            "/api/v1/memory/embeddings",
            post(embedding).layer(DefaultBodyLimit::max(
                super::retrieval_limits::VECTOR_BODY_BYTES,
            )),
        )
        .route(
            "/api/v1/memory/search",
            post(semantic_search).layer(DefaultBodyLimit::max(
                super::retrieval_limits::VECTOR_BODY_BYTES,
            )),
        )
        .route("/api/v1/memory/search/lexical", post(lexical_search))
        .route(
            "/api/v1/memory/search/hybrid",
            post(hybrid_search).layer(DefaultBodyLimit::max(
                super::retrieval_limits::VECTOR_BODY_BYTES,
            )),
        )
        .route("/api/v1/memory/ranking-profiles", get(ranking_profiles))
}

async fn ranking_profiles(
    State(state): State<PlatformState>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    let limits = state.retrieval_limits.clone();
    state
        .run(headers, move |_, _, principal| {
            if !principal
                .spec
                .capabilities
                .contains(&Capability::MemoryRead)
            {
                return Err(ApiError::new(
                    StatusCode::FORBIDDEN,
                    "forbidden",
                    "Memory read is not granted",
                ));
            }
            let profiles: Vec<_> = qilbee_memory::storage::platform::HybridRankingVersion::ALL
                .into_iter()
                .map(|version| version.profile())
                .collect();
            Ok(Json(json!({
                "contract_version": 1,
                "component_versions": {"lexical": "bm25_v1", "semantic": "cosine_exact_v1"},
                "hybrid_profiles": profiles,
                "execution_limits": limits.metadata()
            })))
        })
        .await
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CommandRequest {
    contract_version: u32,
    idempotency_key: String,
    scope: ResourceScope,
    operation: MemoryOperation,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QueryRequest {
    contract_version: u32,
    scope: ResourceScope,
    filter: MemoryQuery,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadQuery {
    contract_version: u32,
    project_id: String,
    mission_id: Option<String>,
    agent_id: String,
    visibility: Visibility,
}
async fn command(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<CommandRequest>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let memory = state.memory.clone();
    state
        .run(headers, move |identity, token, _| {
            let request = json_body(body)?;
            version(request.contract_version)?;
            let scope = identity
                .authorize(token, Capability::MemoryWrite, &request.scope)
                .map_err(ApiError::operation)?;
            let receipt = memory
                .apply_memory_command(
                    &scope.storage_namespace,
                    &RecordAuthor {
                        credential_id: scope.credential_id,
                        subject_id: scope.subject_id,
                    },
                    &MemoryCommand {
                        contract_version: request.contract_version,
                        idempotency_key: request.idempotency_key,
                        operation: request.operation,
                    },
                )
                .map_err(ApiError::operation)?;
            Ok(Json(json!({"contract_version":1,"receipt":receipt})))
        })
        .await
}
async fn read(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    request: Result<Query<ReadQuery>, QueryRejection>,
) -> ApiResult<Json<Value>> {
    let memory = state.memory.clone();
    state
        .run(headers, move |identity, token, _| {
            let Query(request) = request.map_err(|_| {
                ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "Invalid memory scope query",
                )
            })?;
            version(request.contract_version)?;
            let resource = ResourceScope {
                project_id: request.project_id,
                mission_id: request.mission_id,
                agent_id: request.agent_id,
                visibility: request.visibility,
            };
            let scope = identity
                .authorize(token, Capability::MemoryRead, &resource)
                .map_err(ApiError::operation)?;
            let id = Uuid::parse_str(&id).map_err(|_| {
                ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "Expected a record UUID",
                )
            })?;
            let record = memory
                .read_memory_record(&scope.storage_namespace, id)
                .map_err(ApiError::operation)?
                .ok_or_else(|| {
                    ApiError::new(
                        StatusCode::NOT_FOUND,
                        "record_not_found",
                        "No current record exists in the authorized scope",
                    )
                })?;
            Ok(Json(
                json!({"contract_version":1,"scope":resource,"record":record}),
            ))
        })
        .await
}
async fn query(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<QueryRequest>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let memory = state.memory.clone();
    state
        .run(headers, move |identity, token, _| {
            let request = json_body(body)?;
            version(request.contract_version)?;
            let scope = identity
                .authorize(token, Capability::MemoryRead, &request.scope)
                .map_err(ApiError::operation)?;
            let page = memory
                .query_memory_records(&scope.storage_namespace, &request.filter)
                .map_err(ApiError::operation)?;
            Ok(Json(
                json!({"contract_version":1,"scope":request.scope,"page":page}),
            ))
        })
        .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmbeddingRequest {
    contract_version: u32,
    scope: ResourceScope,
    idempotency_key: String,
    record_id: Uuid,
    record_revision: u64,
    space: qilbee_memory::storage::platform::EmbeddingSpace,
    vector: Vec<f32>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SemanticRequest {
    contract_version: u32,
    scope: ResourceScope,
    query: qilbee_memory::storage::platform::SemanticQuery,
}
async fn embedding(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<EmbeddingRequest>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let memory = state.memory.clone();
    let limits = state.retrieval_limits.clone();
    state
        .run(headers, move |identity, token, _| {
            let request = json_body(body)?;
            version(request.contract_version)?;
            let scope = identity
                .authorize(token, Capability::MemoryWrite, &request.scope)
                .map_err(ApiError::operation)?;
            limits.dimensions(request.space.dimensions)?;
            let command = qilbee_memory::storage::platform::EmbeddingCommand {
                contract_version: request.contract_version,
                idempotency_key: request.idempotency_key,
                record_id: request.record_id,
                record_revision: request.record_revision,
                space: request.space,
                vector: request.vector,
            };
            let author = RecordAuthor {
                credential_id: scope.credential_id,
                subject_id: scope.subject_id,
            };
            let receipt = memory
                .apply_memory_embedding(&scope.storage_namespace, &author, &command)
                .map_err(ApiError::operation)?;
            Ok(Json(json!({"contract_version":1,"receipt":receipt})))
        })
        .await
}
async fn semantic_search(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<SemanticRequest>, JsonRejection>,
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
            limits.dimensions(request.query.space.dimensions)?;
            let _permit = limits.acquire()?;
            let retrieval_started = std::time::Instant::now();
            let page = memory
                .search_memory_semantic(&scope.storage_namespace, &request.query)
                .map_err(ApiError::operation)?;
            let retrieval_micros = retrieval_started
                .elapsed()
                .as_micros()
                .min(u64::MAX as u128) as u64;
            let mut response =
                Json(json!({"contract_version":1,"scope":request.scope,"page":page}))
                    .into_response();
            response.headers_mut().insert(
                "x-qilbee-ranking-version",
                HeaderValue::from_static("cosine_exact_v1"),
            );
            response.headers_mut().insert(
                "x-qilbee-retrieval-micros",
                HeaderValue::from_str(&retrieval_micros.to_string())
                    .map_err(|_| ApiError::internal())?,
            );
            Ok(response)
        })
        .await
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum RetrievalMode {
    Lexical,
    Hybrid,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RankedRequest<T> {
    contract_version: u32,
    scope: ResourceScope,
    mode: RetrievalMode,
    query: T,
}

async fn lexical_search(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<
        Json<RankedRequest<qilbee_memory::storage::platform::LexicalQuery>>,
        JsonRejection,
    >,
) -> ApiResult<Json<Value>> {
    let memory = state.memory.clone();
    let limits = state.retrieval_limits.clone();
    state.run(headers, move |identity, token, _| {
        let request = json_body(body)?;
        version(request.contract_version)?;
        if !matches!(request.mode, RetrievalMode::Lexical) {
            return Err(ApiError::new(StatusCode::BAD_REQUEST, "invalid_request", "Search mode does not match the endpoint"));
        }
        let scope = identity.authorize(token, Capability::MemoryRead, &request.scope).map_err(ApiError::operation)?;
        limits.scan_bytes(request.query.scan_bytes_limit)?;
        let _permit = limits.acquire()?;
        let retrieval_started = std::time::Instant::now();
        let page = memory.search_memory_lexical(&scope.storage_namespace, &request.query).map_err(ApiError::operation)?;
        let retrieval_micros = retrieval_started.elapsed().as_micros().min(u64::MAX as u128) as u64;
        Ok(Json(json!({"contract_version":1,"scope":request.scope,"mode":"lexical","ranking_version":"bm25_v1","page":page,"timing":{"retrieval_micros":retrieval_micros}})))
    }).await
}

async fn hybrid_search(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<RankedRequest<qilbee_memory::storage::platform::HybridQuery>>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let memory = state.memory.clone();
    let limits = state.retrieval_limits.clone();
    state.run(headers, move |identity, token, _| {
        let request = json_body(body)?;
        version(request.contract_version)?;
        if !matches!(request.mode, RetrievalMode::Hybrid) {
            return Err(ApiError::new(StatusCode::BAD_REQUEST, "invalid_request", "Search mode does not match the endpoint"));
        }
        let scope = identity.authorize(token, Capability::MemoryRead, &request.scope).map_err(ApiError::operation)?;
        limits.scan_bytes(request.query.scan_bytes_limit)?;
        limits.dimensions(request.query.space.dimensions)?;
        let _permit = limits.acquire()?;
        let retrieval_started = std::time::Instant::now();
        let page = memory.search_memory_hybrid(&scope.storage_namespace, &request.query).map_err(ApiError::operation)?;
        let retrieval_micros = retrieval_started.elapsed().as_micros().min(u64::MAX as u128) as u64;
        Ok(Json(json!({"contract_version":1,"scope":request.scope,"mode":"hybrid","ranking_version":page.ranking.version,"page":page,"timing":{"retrieval_micros":retrieval_micros}})))
    }).await
}
