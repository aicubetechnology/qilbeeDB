//! HTTP/REST API server implementation using Axum

use axum::{
    extract::{Path, Query as AxumQuery, State, FromRef},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::{delete, get, post, put},
    Router,
};
use qilbee_core::{EntityId, Label, NodeId, Property, PropertyValue};
use qilbee_graph::Database;
use qilbee_memory::{
    AgentMemory, Episode, EpisodeContent, EpisodeType,
    LLMConfig, LLMProviderType, LLMService,
};
use qilbee_protocol::http::HealthResponse;
use std::collections::HashMap as StdHashMap;
use std::sync::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tower_http::trace::TraceLayer;

use crate::security::{
    AuthService, UserService, TokenService, Credentials, AuthConfig,
    RateLimitService, AuthMiddleware, global_rate_limit, require_auth, RbacService, AuditService, AuditConfig,
    AuditEventType, AuditResult, TokenBlacklist, BlacklistConfig, RevocationReason,
    AccountLockoutService, LockoutConfig, security_headers_middleware, CorsConfig,
    https_redirect_middleware,
};

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub database: Arc<Database>,
    pub start_time: Instant,
    pub agent_memories: Arc<Mutex<StdHashMap<String, Arc<AgentMemory>>>>,
    pub auth_service: Arc<AuthService>,
    pub token_service: Arc<TokenService>,
    pub user_service: Arc<UserService>,
    pub rate_limit_service: Arc<RateLimitService>,
    pub audit_service: Arc<AuditService>,
    pub lockout_service: Arc<AccountLockoutService>,
    pub auth_middleware: AuthMiddleware,
    /// LLM service for memory consolidation (runtime configurable)
    pub llm_service: Arc<LLMService>,
}

/// Implement FromRef to allow extracting AuthMiddleware from AppState in middleware
impl FromRef<AppState> for AuthMiddleware {
    fn from_ref(state: &AppState) -> Self {
        state.auth_middleware.clone()
    }
}

/// Create HTTP server router
pub fn create_router(database: Arc<Database>) -> Router {
    // Initialize security services
    let user_service = Arc::new(UserService::new());
    let token_service = Arc::new(TokenService::new("qilbee_jwt_secret_change_in_production".to_string()));
    let rate_limit_service = Arc::new(RateLimitService::new());

    // Create bootstrap admin user for testing
    // TODO: Replace with proper bootstrap process
    let _ = user_service.create_default_admin("SecureAdmin@123!");

    // Create token blacklist (in-memory for now, can add persistence later)
    let token_blacklist = Arc::new(TokenBlacklist::new(BlacklistConfig::default()));

    let token_service_clone = token_service.clone();
    let auth_service = Arc::new(AuthService::new(
        user_service.clone(),
        token_service,
        token_blacklist,
        AuthConfig::default(),
    ));

    // Create RBAC and Audit services for AuthMiddleware
    let rbac_service = Arc::new(RbacService::new());
    let audit_service = Arc::new(AuditService::new(AuditConfig::default()));

    // Create account lockout service
    let lockout_service = Arc::new(AccountLockoutService::new(LockoutConfig::default()));

    // Create AuthMiddleware for rate limiting
    let auth_middleware = AuthMiddleware {
        auth_service: auth_service.clone(),
        rbac_service,
        audit_service: audit_service.clone(),
        rate_limit_service: rate_limit_service.clone(),
    };

    // Initialize LLM service (starts with mock, can be configured at runtime via API)
    // To start with OpenAI, set OPENAI_API_KEY environment variable
    let llm_service = match std::env::var("OPENAI_API_KEY") {
        Ok(api_key) if !api_key.is_empty() => {
            let config = LLMConfig::openai_mini(&api_key);
            Arc::new(LLMService::new(config).unwrap_or_else(|_| LLMService::mock()))
        }
        _ => Arc::new(LLMService::mock()),
    };

    let state = AppState {
        database,
        start_time: Instant::now(),
        agent_memories: Arc::new(Mutex::new(StdHashMap::new())),
        auth_service,
        token_service: token_service_clone,
        user_service: user_service.clone(),
        rate_limit_service,
        audit_service,
        lockout_service,
        auth_middleware: auth_middleware.clone(),
        llm_service,
    };

    // Build router with all routes and apply global rate limiting
    Router::new()
        // Health check (rate limiting skipped in global middleware)
        .route("/health", get(health_check))
        // Auth endpoints
        .route("/api/v1/auth/login", post(auth_login))
        .route("/api/v1/auth/logout", post(auth_logout))
        .route("/api/v1/auth/refresh", post(auth_refresh))
        .route("/api/v1/auth/revoke", post(auth_revoke))
        .route("/api/v1/auth/revoke-all", post(auth_revoke_all))
        // API key management
        .route("/api/v1/api-keys", post(api_key_create).get(api_key_list))
        .route("/api/v1/api-keys/:key_id", delete(api_key_revoke))
        .route("/api/v1/api-keys/rotate", post(api_key_rotate))
        // User management
        .route("/api/v1/users", post(user_create).get(user_list))
        .route("/api/v1/users/:user_id", get(user_get).put(user_update).delete(user_delete))
        .route("/api/v1/users/:user_id/roles", put(user_update_roles))
        // Rate limit policy management
        .route("/api/v1/rate-limits", post(rate_limit_create).get(rate_limit_list))
        .route("/api/v1/rate-limits/:policy_id", get(rate_limit_get).put(rate_limit_update).delete(rate_limit_delete))
        // Audit log query (Admin only)
        .route("/api/v1/audit-logs", get(audit_logs_query))
        // Account lockout management (Admin only)
        .route("/api/v1/lockouts", get(lockout_list))
        .route("/api/v1/lockouts/:username", get(lockout_status).delete(lockout_unlock))
        .route("/api/v1/lockouts/:username/lock", post(lockout_lock))
        // LLM configuration (Admin only)
        .route("/api/v1/llm/status", get(llm_status))
        .route("/api/v1/llm/config", put(llm_update_config))
        // Graph operations
        .route("/graphs/:name", post(create_graph).delete(delete_graph))
        .route("/graphs/:name/nodes", post(create_node).get(find_nodes))
        .route("/graphs/:name/nodes/:id", get(get_node).put(update_node).delete(delete_node))
        .route("/graphs/:name/relationships", post(create_relationship))
        .route("/graphs/:name/nodes/:id/relationships", get(get_relationships))
        .route("/graphs/:name/query", post(execute_query))
        // Memory operations (require authentication)
        .nest("/memory", memory_routes(auth_middleware.clone()))
        // Apply global rate limiting middleware (determines endpoint type from path)
        // Uses from_fn_with_state for proper state access in middleware
        .layer(axum::middleware::from_fn_with_state(auth_middleware, global_rate_limit))
        // CORS configuration: reads from environment variables or uses permissive defaults for development
        // Set CORS_ALLOWED_ORIGINS env var for production (comma-separated list of origins)
        .layer(CorsConfig::from_env().build_layer())
        .layer(TraceLayer::new_for_http())
        // Add security headers to all responses
        .layer(axum::middleware::from_fn(security_headers_middleware))
        // HTTPS redirect middleware (disabled by default, enable with HTTPS_ENFORCE=true)
        // When enabled, redirects HTTP requests to HTTPS (respects X-Forwarded-Proto for proxies)
        .layer(axum::middleware::from_fn(https_redirect_middleware))
        .with_state(state)
}

/// Create memory routes with authentication middleware applied
/// All memory operations require valid authentication (JWT token or API key)
fn memory_routes(auth_middleware: AuthMiddleware) -> Router<AppState> {
    Router::new()
        .route("/:agent_id/episodes", post(store_episode))
        .route("/:agent_id/episodes/:id", get(get_episode))
        .route("/:agent_id/episodes/:id/similar", get(find_similar_episodes))
        .route("/:agent_id/episodes/recent", get(get_recent_episodes))
        .route("/:agent_id/episodes/search", post(search_episodes))
        .route("/:agent_id/episodes/semantic-search", post(semantic_search))
        .route("/:agent_id/episodes/hybrid-search", post(hybrid_search))
        .route("/:agent_id/statistics", get(get_memory_statistics))
        .route("/:agent_id/semantic-search/status", get(get_semantic_search_status))
        .route("/:agent_id/consolidate", post(consolidate_memory))
        .route("/:agent_id/forget", post(forget_memory))
        .route("/:agent_id", delete(clear_memory))
        // Apply authentication middleware to all memory routes
        .layer(axum::middleware::from_fn_with_state(auth_middleware, require_auth))
}

// ==================== Health Check ====================

async fn health_check(State(state): State<AppState>) -> impl IntoResponse {
    let response = HealthResponse {
        status: "healthy".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        uptime_seconds: state.start_time.elapsed().as_secs(),
    };

    Json(response)
}

// ==================== Graph Management ====================

async fn create_graph(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    match state.database.create_graph(&name) {
        Ok(_) => (StatusCode::CREATED, Json(json!({"name": name}))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        ),
    }
}

async fn delete_graph(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    match state.database.delete_graph(&name) {
        Ok(_) => (StatusCode::OK, Json(json!({"deleted": true}))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        ),
    }
}

// ==================== Node Operations ====================

#[derive(Debug, Deserialize)]
struct CreateNodeRequest {
    labels: Vec<String>,
    properties: HashMap<String, Value>,
}

#[derive(Debug, Serialize)]
struct NodeResponse {
    id: u64,
    labels: Vec<String>,
    properties: HashMap<String, Value>,
}

async fn create_node(
    State(state): State<AppState>,
    Path(graph_name): Path<String>,
    Json(request): Json<CreateNodeRequest>,
) -> impl IntoResponse {
    let graph = match state.database.graph(&graph_name) {
        Ok(g) => g,
        Err(e) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error": e.to_string()})),
            );
        }
    };

    // Convert properties to qilbee_core::Property
    let props = json_map_to_property(&request.properties);

    // Convert labels to Label type
    let labels: Vec<Label> = request.labels.iter().map(|s| Label::new(s)).collect();

    match graph.create_node_with_properties(labels, props) {
        Ok(node) => {
            let response = NodeResponse {
                id: node.id.as_internal(),
                labels: request.labels,
                properties: request.properties,
            };
            (StatusCode::CREATED, Json(json!(response)))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        ),
    }
}

async fn get_node(
    State(state): State<AppState>,
    Path((graph_name, node_id)): Path<(String, String)>,
) -> impl IntoResponse {
    let graph = match state.database.graph(&graph_name) {
        Ok(g) => g,
        Err(e) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error": e.to_string()})),
            );
        }
    };

    let id_val: u64 = match node_id.parse() {
        Ok(i) => i,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid node ID"})),
            );
        }
    };

    let id = NodeId::from_internal(id_val);

    match graph.get_node(id) {
        Ok(Some(node)) => {
            let props = property_to_json_map(&node.properties);
            let labels: Vec<String> = node.labels.iter().map(|l| l.name().to_string()).collect();
            let response = NodeResponse {
                id: node.id.as_internal(),
                labels,
                properties: props,
            };
            (StatusCode::OK, Json(json!(response)))
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "Node not found"})),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        ),
    }
}

#[derive(Debug, Deserialize)]
struct UpdateNodeRequest {
    labels: Vec<String>,
    properties: HashMap<String, Value>,
}

async fn update_node(
    State(state): State<AppState>,
    Path((graph_name, node_id)): Path<(String, String)>,
    Json(request): Json<UpdateNodeRequest>,
) -> impl IntoResponse {
    let graph = match state.database.graph(&graph_name) {
        Ok(g) => g,
        Err(e) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error": e.to_string()})),
            );
        }
    };

    let id_val: u64 = match node_id.parse() {
        Ok(i) => i,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid node ID"})),
            );
        }
    };

    let id = NodeId::from_internal(id_val);

    // Get the existing node
    let mut node = match graph.get_node(id) {
        Ok(Some(n)) => n,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "Node not found"})),
            );
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            );
        }
    };

    // Update labels and properties
    node.labels = request.labels.iter().map(|s| Label::new(s)).collect();
    node.properties = json_map_to_property(&request.properties);

    match graph.update_node(&node) {
        Ok(_) => {
            let response = NodeResponse {
                id: id_val,
                labels: request.labels,
                properties: request.properties,
            };
            (StatusCode::OK, Json(json!(response)))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        ),
    }
}

async fn delete_node(
    State(state): State<AppState>,
    Path((graph_name, node_id)): Path<(String, String)>,
) -> impl IntoResponse {
    let graph = match state.database.graph(&graph_name) {
        Ok(g) => g,
        Err(e) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error": e.to_string()})),
            );
        }
    };

    let id_val: u64 = match node_id.parse() {
        Ok(i) => i,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid node ID"})),
            );
        }
    };

    let id = NodeId::from_internal(id_val);

    // Try detach delete first (deletes relationships too)
    match graph.detach_delete_node(id) {
        Ok(_) => (StatusCode::OK, Json(json!({"deleted": true}))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        ),
    }
}

#[derive(Debug, Deserialize)]
struct FindNodesQuery {
    label: Option<String>,
    limit: Option<usize>,
}

async fn find_nodes(
    State(state): State<AppState>,
    Path(graph_name): Path<String>,
    AxumQuery(query): AxumQuery<FindNodesQuery>,
) -> impl IntoResponse {
    let graph = match state.database.graph(&graph_name) {
        Ok(g) => g,
        Err(e) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error": e.to_string()})),
            );
        }
    };

    // Get nodes by label if specified, otherwise get all nodes
    let core_nodes = if let Some(label) = &query.label {
        match graph.find_nodes_by_label(label) {
            Ok(nodes) => nodes,
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": e.to_string()})),
                );
            }
        }
    } else {
        match graph.get_all_nodes() {
            Ok(nodes) => nodes,
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": e.to_string()})),
                );
            }
        }
    };

    // Apply limit
    let limit = query.limit.unwrap_or(100);
    let limited_nodes: Vec<_> = core_nodes.into_iter().take(limit).collect();

    // Convert to response format
    let nodes: Vec<NodeResponse> = limited_nodes
        .iter()
        .map(|node| {
            let props = property_to_json_map(&node.properties);
            let labels: Vec<String> = node.labels.iter().map(|l| l.name().to_string()).collect();
            NodeResponse {
                id: node.id.as_internal(),
                labels,
                properties: props,
            }
        })
        .collect();

    (
        StatusCode::OK,
        Json(json!({"nodes": nodes, "count": nodes.len()})),
    )
}

// ==================== Relationship Operations ====================

#[derive(Debug, Deserialize)]
struct CreateRelationshipRequest {
    #[serde(rename = "startNode")]
    start_node: u64,
    #[serde(rename = "type")]
    rel_type: String,
    #[serde(rename = "endNode")]
    end_node: u64,
    properties: HashMap<String, Value>,
}

#[derive(Debug, Serialize)]
struct RelationshipResponse {
    id: u64,
    #[serde(rename = "type")]
    rel_type: String,
    #[serde(rename = "startNode")]
    start_node: u64,
    #[serde(rename = "endNode")]
    end_node: u64,
    properties: HashMap<String, Value>,
}

async fn create_relationship(
    State(state): State<AppState>,
    Path(graph_name): Path<String>,
    Json(request): Json<CreateRelationshipRequest>,
) -> impl IntoResponse {
    let graph = match state.database.graph(&graph_name) {
        Ok(g) => g,
        Err(e) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error": e.to_string()})),
            );
        }
    };

    // Convert properties
    let props = json_map_to_property(&request.properties);
    let source = NodeId::from_internal(request.start_node);
    let target = NodeId::from_internal(request.end_node);
    let rel_type = Label::new(&request.rel_type);

    match graph.create_relationship_with_properties(source, rel_type, target, props) {
        Ok(rel) => {
            let response = RelationshipResponse {
                id: rel.id.as_internal(),
                rel_type: request.rel_type,
                start_node: request.start_node,
                end_node: request.end_node,
                properties: request.properties,
            };
            (StatusCode::CREATED, Json(json!(response)))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        ),
    }
}

async fn get_relationships(
    State(_state): State<AppState>,
    Path((_graph_name, _node_id)): Path<(String, String)>,
) -> impl IntoResponse {
    // TODO: Implement relationship retrieval
    let relationships: Vec<RelationshipResponse> = vec![];
    (StatusCode::OK, Json(json!({"relationships": relationships})))
}

// ==================== Query Operations ====================

#[derive(Debug, Deserialize)]
struct QueryRequestJson {
    cypher: String,
    parameters: Option<HashMap<String, Value>>,
}

async fn execute_query(
    State(state): State<AppState>,
    Path(graph_name): Path<String>,
    Json(request): Json<QueryRequestJson>,
) -> impl IntoResponse {
    use qilbee_query::{parse_simple, QueryPlanner, QueryExecutor};
    use std::sync::Arc;

    let graph = match state.database.graph(&graph_name) {
        Ok(g) => g,
        Err(e) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error": e.to_string()})),
            );
        }
    };

    // Parse the query
    let parsed_query = match parse_simple(&request.cypher) {
        Ok(q) => q,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": format!("Parse error: {}", e)})),
            );
        }
    };

    // Create execution plan
    let planner = QueryPlanner::new();
    let plan = match planner.plan(&parsed_query) {
        Ok(p) => p,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": format!("Planning error: {}", e)})),
            );
        }
    };

    // Execute the plan
    let executor = QueryExecutor::new(Arc::new(graph));

    // Convert parameters from JSON Value to PropertyValue
    let mut params = std::collections::HashMap::new();
    if let Some(req_params) = &request.parameters {
        for (key, value) in req_params {
            let prop_value = match value {
                Value::Number(n) if n.is_i64() => PropertyValue::Integer(n.as_i64().unwrap()),
                Value::Number(n) if n.is_f64() => PropertyValue::Float(n.as_f64().unwrap()),
                Value::String(s) => PropertyValue::String(s.clone()),
                Value::Bool(b) => PropertyValue::Boolean(*b),
                Value::Null => PropertyValue::Null,
                _ => PropertyValue::Null,
            };
            params.insert(key.clone(), prop_value);
        }
    }

    let result = match executor.execute(&plan, &params) {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("Execution error: {}", e)})),
            );
        }
    };

    // Convert result to JSON format expected by SDK
    let mut results = Vec::new();
    for row in result.rows {
        let mut row_map = HashMap::new();
        for (i, col_name) in result.columns.iter().enumerate() {
            row_map.insert(col_name.clone(), property_value_to_json(&row[i]));
        }
        results.push(row_map);
    }

    let response = json!({
        "results": results,
        "stats": {
            "nodesCreated": result.stats.nodes_created,
            "nodesDeleted": result.stats.nodes_deleted,
            "relationshipsCreated": result.stats.relationships_created,
            "relationshipsDeleted": result.stats.relationships_deleted,
            "executionTimeMs": result.stats.execution_time_ms
        }
    });

    (StatusCode::OK, Json(response))
}

// ==================== Memory Operations ====================

/// Error types for memory operations with proper HTTP status code mapping
#[derive(Debug)]
enum MemoryOperationError {
    /// Agent memory not found (404)
    AgentNotFound { agent_id: String },
    /// Episode not found (404)
    EpisodeNotFound { agent_id: String, episode_id: String },
    /// Request validation failed (400)
    ValidationError { field: String, message: String },
    /// Memory operation failed (500)
    OperationFailed { operation: String, message: String },
    /// Storage error (500)
    StorageError { message: String },
}

impl MemoryOperationError {
    fn error_code(&self) -> &'static str {
        match self {
            MemoryOperationError::AgentNotFound { .. } => "AGENT_NOT_FOUND",
            MemoryOperationError::EpisodeNotFound { .. } => "EPISODE_NOT_FOUND",
            MemoryOperationError::ValidationError { .. } => "VALIDATION_ERROR",
            MemoryOperationError::OperationFailed { .. } => "OPERATION_FAILED",
            MemoryOperationError::StorageError { .. } => "STORAGE_ERROR",
        }
    }
}

impl std::fmt::Display for MemoryOperationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MemoryOperationError::AgentNotFound { agent_id } => {
                write!(f, "Agent memory not found for agent_id: {}", agent_id)
            }
            MemoryOperationError::EpisodeNotFound { agent_id, episode_id } => {
                write!(f, "Episode {} not found for agent {}", episode_id, agent_id)
            }
            MemoryOperationError::ValidationError { field, message } => {
                write!(f, "Validation error for '{}': {}", field, message)
            }
            MemoryOperationError::OperationFailed { operation, message } => {
                write!(f, "{} operation failed: {}", operation, message)
            }
            MemoryOperationError::StorageError { message } => {
                write!(f, "Storage error: {}", message)
            }
        }
    }
}

impl IntoResponse for MemoryOperationError {
    fn into_response(self) -> axum::response::Response {
        let (status, error_response) = match &self {
            MemoryOperationError::AgentNotFound { agent_id } => (
                StatusCode::NOT_FOUND,
                json!({
                    "error": self.to_string(),
                    "error_code": self.error_code(),
                    "agent_id": agent_id
                }),
            ),
            MemoryOperationError::EpisodeNotFound { agent_id, episode_id } => (
                StatusCode::NOT_FOUND,
                json!({
                    "error": self.to_string(),
                    "error_code": self.error_code(),
                    "agent_id": agent_id,
                    "episode_id": episode_id
                }),
            ),
            MemoryOperationError::ValidationError { field, message } => (
                StatusCode::BAD_REQUEST,
                json!({
                    "error": "Validation failed",
                    "error_code": self.error_code(),
                    "field": field,
                    "details": message
                }),
            ),
            MemoryOperationError::OperationFailed { operation, message } => (
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({
                    "error": self.to_string(),
                    "error_code": self.error_code(),
                    "operation": operation,
                    "details": message
                }),
            ),
            MemoryOperationError::StorageError { message } => (
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({
                    "error": self.to_string(),
                    "error_code": self.error_code(),
                    "details": message
                }),
            ),
        };
        (status, Json(error_response)).into_response()
    }
}

#[derive(Debug, Deserialize)]
struct StoreEpisodeRequest {
    #[serde(rename = "agentId")]
    agent_id: String,
    #[serde(rename = "episodeType")]
    episode_type: String,
    content: HashMap<String, Value>,
    #[serde(rename = "eventTime")]
    event_time: Option<i64>,
    metadata: Option<HashMap<String, Value>>,
}

/// Request body for consolidating agent memory
#[derive(Debug, Deserialize)]
struct ConsolidateMemoryRequest {
    /// Minimum relevance threshold (0.0 to 1.0) - episodes below this will be decayed
    #[serde(default = "default_min_relevance")]
    min_relevance: f64,
    /// Decay factor to apply (0.0 to 1.0) - lower means more aggressive decay
    #[serde(default = "default_decay_factor")]
    decay_factor: f64,
}

fn default_min_relevance() -> f64 {
    0.3
}

fn default_decay_factor() -> f64 {
    0.9
}

impl ConsolidateMemoryRequest {
    fn validate(&self) -> Result<(), String> {
        if self.min_relevance < 0.0 || self.min_relevance > 1.0 {
            return Err("min_relevance must be between 0.0 and 1.0".to_string());
        }
        if self.decay_factor < 0.0 || self.decay_factor > 1.0 {
            return Err("decay_factor must be between 0.0 and 1.0".to_string());
        }
        Ok(())
    }
}

/// Request body for forgetting agent memory
#[derive(Debug, Deserialize)]
struct ForgetMemoryRequest {
    /// Minimum relevance threshold (0.0 to 1.0) - episodes below this will be forgotten
    #[serde(default = "default_forget_min_relevance")]
    min_relevance: f64,
    /// Maximum age in seconds - episodes older than this may be forgotten
    #[serde(default)]
    max_age_seconds: Option<u64>,
    /// Episode types to target for forgetting (empty means all types)
    #[serde(default)]
    episode_types: Vec<String>,
}

fn default_forget_min_relevance() -> f64 {
    0.1
}

impl ForgetMemoryRequest {
    fn validate(&self) -> Result<(), String> {
        if self.min_relevance < 0.0 || self.min_relevance > 1.0 {
            return Err("min_relevance must be between 0.0 and 1.0".to_string());
        }
        Ok(())
    }
}

#[tracing::instrument(
    name = "memory.store_episode",
    skip(state, request),
    fields(agent_id = %agent_id)
)]
async fn store_episode(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
    Json(request): Json<StoreEpisodeRequest>,
) -> impl IntoResponse {
    // Get or create agent memory
    let memory = {
        let mut memories = state.agent_memories.lock().unwrap();
        memories
            .entry(agent_id.clone())
            .or_insert_with(|| Arc::new(AgentMemory::for_agent(&agent_id)))
            .clone()
    };

    // Parse episode type
    let episode_type = match request.episode_type.as_str() {
        "conversation" | "Conversation" => EpisodeType::Conversation,
        "observation" | "Observation" => EpisodeType::Observation,
        "action" => EpisodeType::Custom("Action".to_string()),
        _ => EpisodeType::Conversation,
    };

    // Create episode content from HashMap
    // Support multiple field names for primary content:
    // - primary, input, message, user_input, text, observation, action
    let primary_content = request
        .content
        .get("primary")
        .or(request.content.get("input"))
        .or(request.content.get("message"))
        .or(request.content.get("user_input"))
        .or(request.content.get("text"))
        .or(request.content.get("observation"))
        .or(request.content.get("action"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let mut content = EpisodeContent::new(primary_content);

    // Support multiple field names for secondary content:
    // - secondary, output, response, agent_response, result
    if let Some(secondary) = request.content.get("secondary")
        .or(request.content.get("output"))
        .or(request.content.get("response"))
        .or(request.content.get("agent_response"))
        .or(request.content.get("result"))
    {
        if let Some(s) = secondary.as_str() {
            content = content.with_secondary(s);
        }
    }

    // Create episode
    let episode = Episode::new(&agent_id, episode_type, content);
    let episode_id = episode.id.to_string();

    // Store episode
    match memory.store_episode(episode) {
        Ok(_) => (StatusCode::CREATED, Json(json!({"episodeId": episode_id}))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        ),
    }
}

#[tracing::instrument(
    name = "memory.get_episode",
    skip(state),
    fields(agent_id = %agent_id, episode_id = %episode_id)
)]
async fn get_episode(
    State(state): State<AppState>,
    Path((agent_id, episode_id)): Path<(String, String)>,
) -> Result<impl IntoResponse, MemoryOperationError> {
    // Get agent memory
    let memory = {
        let memories = state.agent_memories.lock().unwrap();
        memories.get(&agent_id).cloned().ok_or_else(|| {
            MemoryOperationError::AgentNotFound {
                agent_id: agent_id.clone(),
            }
        })?
    };

    // Parse episode ID - just compare as string for now
    // TODO: Implement proper UUID parsing for EpisodeId
    // For now, get recent episodes and find by string comparison
    let episodes = memory.get_recent_episodes(100).map_err(|e| {
        MemoryOperationError::StorageError {
            message: e.to_string(),
        }
    })?;

    let episode = episodes
        .iter()
        .find(|ep| ep.id.to_string() == episode_id)
        .ok_or_else(|| MemoryOperationError::EpisodeNotFound {
            agent_id: agent_id.clone(),
            episode_id: episode_id.clone(),
        })?;

    // Format content based on episode type for intuitive field names
    let content_map = format_episode_content(&episode);

    let response = json!({
        "agentId": episode.agent_id,
        "episodeId": episode.id.to_string(),
        "episodeType": format!("{:?}", episode.episode_type),
        "content": content_map,
        "eventTime": episode.event_time.as_millis()
    });
    Ok((StatusCode::OK, Json(response)))
}

#[tracing::instrument(
    name = "memory.get_recent_episodes",
    skip(state),
    fields(agent_id = %agent_id)
)]
async fn get_recent_episodes(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
) -> impl IntoResponse {
    // Get agent memory
    let memory = {
        let memories = state.agent_memories.lock().unwrap();
        match memories.get(&agent_id) {
            Some(m) => m.clone(),
            None => {
                // Return empty list if agent doesn't exist yet
                return (StatusCode::OK, Json(json!({"episodes": []})));
            }
        }
    };

    // Get recent episodes
    match memory.get_recent_episodes(10) {
        Ok(episodes) => {
            let episode_list: Vec<_> = episodes
                .iter()
                .map(|ep| {
                    json!({
                        "agentId": ep.agent_id,
                        "episodeId": ep.id.to_string(),
                        "episodeType": format!("{:?}", ep.episode_type),
                        "content": format_episode_content(ep),
                        "eventTime": ep.event_time.as_millis()
                    })
                })
                .collect();
            (StatusCode::OK, Json(json!({"episodes": episode_list})))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        ),
    }
}

#[derive(Debug, Deserialize)]
struct SearchEpisodesRequest {
    query: String,
    #[serde(default = "default_search_limit")]
    limit: usize,
}

fn default_search_limit() -> usize {
    10
}

#[tracing::instrument(
    name = "memory.search_episodes",
    skip(state, request),
    fields(agent_id = %agent_id)
)]
async fn search_episodes(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
    Json(request): Json<SearchEpisodesRequest>,
) -> impl IntoResponse {
    // Get agent memory
    let memory = {
        let memories = state.agent_memories.lock().unwrap();
        match memories.get(&agent_id) {
            Some(m) => m.clone(),
            None => {
                return (StatusCode::OK, Json(json!({"episodes": []})));
            }
        }
    };

    // Search episodes using keyword search
    match memory.search_episodes(&request.query) {
        Ok(episodes) => {
            let episode_list: Vec<_> = episodes
                .iter()
                .take(request.limit)
                .map(|ep| {
                    json!({
                        "episodeId": ep.id.to_string(),
                        "agentId": ep.agent_id,
                        "episodeType": format!("{:?}", ep.episode_type),
                        "content": {
                            "primary": ep.content.primary,
                            "secondary": ep.content.secondary
                        },
                        "eventTime": ep.event_time.as_millis(),
                        "metadata": ep.metadata
                    })
                })
                .collect();
            (StatusCode::OK, Json(json!({"episodes": episode_list})))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        ),
    }
}

// ==================== Semantic Search Request/Response Types ====================

#[derive(Debug, Deserialize)]
struct SemanticSearchRequest {
    query: String,
    #[serde(default = "default_search_limit")]
    limit: usize,
    #[serde(rename = "minScore")]
    min_score: Option<f32>,
}

#[derive(Debug, Deserialize)]
struct HybridSearchRequest {
    query: String,
    #[serde(default = "default_search_limit")]
    limit: usize,
    #[serde(rename = "semanticWeight", default = "default_semantic_weight")]
    semantic_weight: f32,
}

fn default_semantic_weight() -> f32 {
    0.5
}

#[derive(Debug, Deserialize)]
struct FindSimilarQuery {
    #[serde(default = "default_search_limit")]
    limit: usize,
}

// ==================== Semantic Search Handlers ====================

#[tracing::instrument(
    name = "memory.semantic_search",
    skip(state, request),
    fields(agent_id = %agent_id, query = %request.query)
)]
async fn semantic_search(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
    Json(request): Json<SemanticSearchRequest>,
) -> impl IntoResponse {
    // Get agent memory
    let memory = {
        let memories = state.agent_memories.lock().unwrap();
        match memories.get(&agent_id) {
            Some(m) => m.clone(),
            None => {
                return (
                    StatusCode::NOT_IMPLEMENTED,
                    Json(json!({
                        "error": "Semantic search is not enabled for this agent",
                        "error_code": "SEMANTIC_SEARCH_NOT_ENABLED"
                    })),
                );
            }
        }
    };

    // For now, semantic search falls back to keyword search since we're using AgentMemory (not PersistentAgentMemory)
    // In production, PersistentAgentMemory with semantic search enabled would be used
    // Return a message indicating semantic search is not available for in-memory agents
    match memory.search_episodes(&request.query) {
        Ok(episodes) => {
            let results: Vec<_> = episodes
                .iter()
                .take(request.limit)
                .enumerate()
                .filter(|(_, ep)| {
                    // Filter by min_score if specified (using rank-based score for now)
                    if let Some(min) = request.min_score {
                        // Simulate a decreasing score based on rank
                        let score = 1.0 - (0.1 * (episodes.iter().position(|e| e.id == ep.id).unwrap_or(0) as f32));
                        score >= min
                    } else {
                        true
                    }
                })
                .map(|(i, ep)| {
                    // Calculate a simulated score based on rank
                    let score = 1.0 - (0.05 * i as f32);
                    json!({
                        "episode": {
                            "episodeId": ep.id.to_string(),
                            "agentId": ep.agent_id,
                            "episodeType": format!("{:?}", ep.episode_type),
                            "content": {
                                "primary": ep.content.primary,
                                "secondary": ep.content.secondary
                            },
                            "eventTime": ep.event_time.as_millis(),
                            "metadata": ep.metadata
                        },
                        "score": score
                    })
                })
                .collect();
            (StatusCode::OK, Json(json!({"results": results})))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": e.to_string(),
                "error_code": "SEARCH_ERROR"
            })),
        ),
    }
}

#[tracing::instrument(
    name = "memory.hybrid_search",
    skip(state, request),
    fields(agent_id = %agent_id, query = %request.query, semantic_weight = %request.semantic_weight)
)]
async fn hybrid_search(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
    Json(request): Json<HybridSearchRequest>,
) -> impl IntoResponse {
    // Validate semantic_weight
    if request.semantic_weight < 0.0 || request.semantic_weight > 1.0 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "semanticWeight must be between 0.0 and 1.0",
                "error_code": "VALIDATION_ERROR"
            })),
        );
    }

    // Get agent memory
    let memory = {
        let memories = state.agent_memories.lock().unwrap();
        match memories.get(&agent_id) {
            Some(m) => m.clone(),
            None => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(json!({
                        "error": "Agent memory not found",
                        "error_code": "AGENT_NOT_FOUND"
                    })),
                );
            }
        }
    };

    // For in-memory agents, hybrid search falls back to keyword search
    // The keyword_weight and semantic_weight are recorded but not used
    let keyword_weight = 1.0 - request.semantic_weight;

    match memory.search_episodes(&request.query) {
        Ok(episodes) => {
            let results: Vec<_> = episodes
                .iter()
                .take(request.limit)
                .enumerate()
                .map(|(i, ep)| {
                    // Calculate simulated scores
                    let keyword_score = 1.0 - (0.05 * i as f32);
                    // Since no semantic search is available, semantic_score is null
                    let combined_score = keyword_score * keyword_weight;

                    json!({
                        "episode": {
                            "episodeId": ep.id.to_string(),
                            "agentId": ep.agent_id,
                            "episodeType": format!("{:?}", ep.episode_type),
                            "content": {
                                "primary": ep.content.primary,
                                "secondary": ep.content.secondary
                            },
                            "eventTime": ep.event_time.as_millis(),
                            "metadata": ep.metadata
                        },
                        "score": combined_score,
                        "keywordScore": keyword_score,
                        "semanticScore": null
                    })
                })
                .collect();
            (StatusCode::OK, Json(json!({"results": results})))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": e.to_string(),
                "error_code": "SEARCH_ERROR"
            })),
        ),
    }
}

#[tracing::instrument(
    name = "memory.find_similar_episodes",
    skip(state),
    fields(agent_id = %agent_id, episode_id = %episode_id)
)]
async fn find_similar_episodes(
    State(state): State<AppState>,
    Path((agent_id, episode_id)): Path<(String, String)>,
    AxumQuery(query): AxumQuery<FindSimilarQuery>,
) -> impl IntoResponse {
    // Get agent memory
    let memory = {
        let memories = state.agent_memories.lock().unwrap();
        match memories.get(&agent_id) {
            Some(m) => m.clone(),
            None => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(json!({
                        "error": "Agent memory not found",
                        "error_code": "AGENT_NOT_FOUND"
                    })),
                );
            }
        }
    };

    // Get the source episode first
    let episodes = match memory.get_recent_episodes(100) {
        Ok(eps) => eps,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": e.to_string(),
                    "error_code": "STORAGE_ERROR"
                })),
            );
        }
    };

    let source_episode = match episodes.iter().find(|ep| ep.id.to_string() == episode_id) {
        Some(ep) => ep,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "error": format!("Episode {} not found", episode_id),
                    "error_code": "EPISODE_NOT_FOUND"
                })),
            );
        }
    };

    // For in-memory agents without vector index, find similar episodes by keyword overlap
    // Search using the source episode's content
    let search_query = format!(
        "{} {}",
        source_episode.content.primary,
        source_episode.content.secondary.as_deref().unwrap_or("")
    );

    match memory.search_episodes(&search_query) {
        Ok(similar_episodes) => {
            let results: Vec<_> = similar_episodes
                .iter()
                .filter(|ep| ep.id.to_string() != episode_id) // Exclude source episode
                .take(query.limit)
                .enumerate()
                .map(|(i, ep)| {
                    let score = 1.0 - (0.1 * i as f32);
                    json!({
                        "episode": {
                            "episodeId": ep.id.to_string(),
                            "agentId": ep.agent_id,
                            "episodeType": format!("{:?}", ep.episode_type),
                            "content": {
                                "primary": ep.content.primary,
                                "secondary": ep.content.secondary
                            },
                            "eventTime": ep.event_time.as_millis(),
                            "metadata": ep.metadata
                        },
                        "score": score
                    })
                })
                .collect();
            (StatusCode::OK, Json(json!({"results": results})))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": e.to_string(),
                "error_code": "SEARCH_ERROR"
            })),
        ),
    }
}

#[tracing::instrument(
    name = "memory.get_semantic_search_status",
    skip(state),
    fields(agent_id = %agent_id)
)]
async fn get_semantic_search_status(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
) -> impl IntoResponse {
    // Check if agent memory exists
    let memory_exists = {
        let memories = state.agent_memories.lock().unwrap();
        memories.contains_key(&agent_id)
    };

    // For in-memory agents (AgentMemory), semantic search is not available
    // Only PersistentAgentMemory supports semantic search
    (
        StatusCode::OK,
        Json(json!({
            "enabled": false,
            "model": null,
            "dimensions": null,
            "indexedEpisodes": 0,
            "agentExists": memory_exists,
            "message": "Semantic search requires PersistentAgentMemory with vector embeddings enabled"
        })),
    )
}

#[tracing::instrument(
    name = "memory.get_statistics",
    skip(state),
    fields(agent_id = %agent_id)
)]
async fn get_memory_statistics(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
) -> impl IntoResponse {
    // Get agent memory
    let memory = {
        let memories = state.agent_memories.lock().unwrap();
        match memories.get(&agent_id) {
            Some(m) => m.clone(),
            None => {
                // Return empty stats if agent doesn't exist yet
                return (
                    StatusCode::OK,
                    Json(json!({
                        "totalEpisodes": 0,
                        "episodesByType": {},
                        "oldestEpisode": null,
                        "newestEpisode": null,
                        "avgRelevance": 0.0
                    })),
                );
            }
        }
    };

    // Get statistics
    match memory.get_statistics() {
        Ok(stats) => (
            StatusCode::OK,
            Json(json!({
                "totalEpisodes": stats.total_episodes,
                "episodesByType": {},
                "oldestEpisode": stats.oldest_episode,
                "newestEpisode": stats.newest_episode,
                "avgRelevance": stats.avg_relevance
            })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        ),
    }
}

#[tracing::instrument(
    name = "memory.consolidate",
    skip(state, request),
    fields(agent_id = %agent_id, min_relevance = %request.min_relevance, decay_factor = %request.decay_factor)
)]
async fn consolidate_memory(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
    Json(request): Json<ConsolidateMemoryRequest>,
) -> Result<impl IntoResponse, MemoryOperationError> {
    // Validate request parameters
    request.validate().map_err(|e| MemoryOperationError::ValidationError {
        field: "request".to_string(),
        message: e,
    })?;

    // Get agent memory
    let memory = {
        let memories = state.agent_memories.lock().unwrap();
        memories.get(&agent_id).cloned().ok_or_else(|| {
            MemoryOperationError::AgentNotFound {
                agent_id: agent_id.clone(),
            }
        })?
    };

    // Get episode count before consolidation
    let episodes_before = memory.episode_count().unwrap_or(0);

    // Apply decay (consolidation operation)
    // Note: Currently using default apply_decay; future enhancement could use request.min_relevance and request.decay_factor
    memory.apply_decay().map_err(|e| {
        // Log audit event for failure
        state.audit_service.log_memory_event(
            AuditEventType::MemoryConsolidated,
            None,
            None,
            &agent_id,
            AuditResult::Error,
            None,
            serde_json::json!({"error": e.to_string()}),
        );
        MemoryOperationError::OperationFailed {
            operation: "consolidate".to_string(),
            message: e.to_string(),
        }
    })?;

    // Get episode count after consolidation
    let episodes_after = memory.episode_count().unwrap_or(0);

    // Log audit event
    state.audit_service.log_memory_event(
        AuditEventType::MemoryConsolidated,
        None,
        None,
        &agent_id,
        AuditResult::Success,
        None,
        serde_json::json!({
            "episodes_before": episodes_before,
            "episodes_after": episodes_after,
            "min_relevance": request.min_relevance,
            "decay_factor": request.decay_factor
        }),
    );

    Ok((StatusCode::OK, Json(json!({
        "consolidated": episodes_after,
        "episodes_before": episodes_before,
        "episodes_after": episodes_after,
        "min_relevance": request.min_relevance,
        "decay_factor": request.decay_factor
    }))))
}

#[tracing::instrument(
    name = "memory.forget",
    skip(state, request),
    fields(agent_id = %agent_id, min_relevance = %request.min_relevance)
)]
async fn forget_memory(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
    Json(request): Json<ForgetMemoryRequest>,
) -> Result<impl IntoResponse, MemoryOperationError> {
    // Validate request parameters
    request.validate().map_err(|e| MemoryOperationError::ValidationError {
        field: "request".to_string(),
        message: e,
    })?;

    // Get agent memory
    let memory = {
        let memories = state.agent_memories.lock().unwrap();
        memories.get(&agent_id).cloned().ok_or_else(|| {
            MemoryOperationError::AgentNotFound {
                agent_id: agent_id.clone(),
            }
        })?
    };

    // Get episode count before forget
    let episodes_before = memory.episode_count().unwrap_or(0);

    // Forget low-relevance episodes
    // Note: Currently using default forget; future enhancement could use request.min_relevance and request.max_age_seconds
    let count = memory.forget().map_err(|e| {
        // Log audit event for failure
        state.audit_service.log_memory_event(
            AuditEventType::MemoryForgotten,
            None,
            None,
            &agent_id,
            AuditResult::Error,
            None,
            serde_json::json!({"error": e.to_string()}),
        );
        MemoryOperationError::OperationFailed {
            operation: "forget".to_string(),
            message: e.to_string(),
        }
    })?;

    // Get episode count after forget
    let episodes_after = memory.episode_count().unwrap_or(0);

    // Log audit event
    state.audit_service.log_memory_event(
        AuditEventType::MemoryForgotten,
        None,
        None,
        &agent_id,
        AuditResult::Success,
        None,
        serde_json::json!({
            "episodes_before": episodes_before,
            "episodes_after": episodes_after,
            "episodes_forgotten": count,
            "min_relevance": request.min_relevance,
            "max_age_seconds": request.max_age_seconds,
            "episode_types": request.episode_types
        }),
    );

    Ok((StatusCode::OK, Json(json!({
        "forgotten": count,
        "episodes_before": episodes_before,
        "episodes_after": episodes_after,
        "min_relevance": request.min_relevance,
        "max_age_seconds": request.max_age_seconds,
        "episode_types": request.episode_types
    }))))
}

#[tracing::instrument(
    name = "memory.clear",
    skip(state),
    fields(agent_id = %agent_id)
)]
async fn clear_memory(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
) -> Result<impl IntoResponse, MemoryOperationError> {
    // Get agent memory
    let memory = {
        let memories = state.agent_memories.lock().unwrap();
        memories.get(&agent_id).cloned().ok_or_else(|| {
            MemoryOperationError::AgentNotFound { agent_id: agent_id.clone() }
        })?
    };

    // Get episode count before clear
    let episodes_before = memory.episode_count().unwrap_or(0);

    // Clear all episodes
    memory.clear().map_err(|e| {
        // Log audit event for failure
        state.audit_service.log_memory_event(
            AuditEventType::MemoryCleared,
            None,
            None,
            &agent_id,
            AuditResult::Error,
            None,
            serde_json::json!({"error": e.to_string()}),
        );
        MemoryOperationError::OperationFailed {
            operation: "clear".to_string(),
            message: e.to_string(),
        }
    })?;

    // Log audit event for success
    state.audit_service.log_memory_event(
        AuditEventType::MemoryCleared,
        None,
        None,
        &agent_id,
        AuditResult::Success,
        None,
        serde_json::json!({
            "episodes_cleared": episodes_before
        }),
    );

    Ok((StatusCode::OK, Json(json!({"cleared": true, "episodes_cleared": episodes_before}))))
}

// ==================== Authentication Operations ====================

#[derive(Debug, Deserialize)]
struct LoginRequest {
    username: String,
    password: String,
}

#[derive(Debug, Serialize)]
struct LoginResponse {
    access_token: String,
    token_type: String,
    expires_in: u64,
    refresh_token: Option<String>,
    username: String,
    user_id: String,
}

async fn auth_login(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(request): Json<LoginRequest>,
) -> impl IntoResponse {
    // Extract client IP from X-Forwarded-For header or X-Real-IP
    let client_ip = headers
        .get("X-Forwarded-For")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(',').next().unwrap_or(s).trim().to_string())
        .or_else(|| {
            headers
                .get("X-Real-IP")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string())
        });

    // Check if account is locked before attempting login
    if let Err(lockout_status) = state.lockout_service.check_login_allowed(
        &request.username,
        client_ip.as_deref(),
    ) {
        // Log account lockout event
        state.audit_service.log_auth_event(
            AuditEventType::AccountLockoutTriggered,
            &request.username,
            AuditResult::Failure,
            client_ip.clone(),
            Some(format!(
                "Account locked. Remaining: {:?}s, Reason: {:?}",
                lockout_status.lockout_remaining_seconds,
                lockout_status.lockout_reason
            )),
        );

        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({
                "error": "Account locked due to too many failed login attempts",
                "locked": true,
                "lockout_expires": lockout_status.lockout_expires,
                "lockout_remaining_seconds": lockout_status.lockout_remaining_seconds,
                "lockout_reason": lockout_status.lockout_reason,
            })),
        );
    }

    let credentials = Credentials {
        username: request.username.clone(),
        password: request.password,
    };

    match state.auth_service.login(credentials) {
        Ok(token) => {
            // Record successful login (resets failed attempt counter)
            state.lockout_service.record_successful_login(
                &request.username,
                client_ip.as_deref(),
            );

            // Log successful login
            state.audit_service.log_auth_event(
                AuditEventType::Login,
                &request.username,
                AuditResult::Success,
                client_ip,
                None,
            );

            // Get user_id from token claims (we should validate token to get claims)
            // For now, we'll just return the token info
            let response = LoginResponse {
                access_token: token.access_token.clone(),
                token_type: token.token_type,
                expires_in: token.expires_in,
                refresh_token: token.refresh_token,
                username: request.username,
                user_id: "user_id_placeholder".to_string(), // TODO: Extract from token
            };
            (StatusCode::OK, Json(json!(response)))
        }
        Err(e) => {
            // Record failed login attempt
            let lockout_status = state.lockout_service.record_failed_attempt(
                &request.username,
                client_ip.as_deref(),
            );

            // Log failed login with lockout status
            let details = if lockout_status.locked {
                Some(format!(
                    "Account locked after {} failed attempts",
                    lockout_status.failed_attempts
                ))
            } else {
                Some(format!(
                    "Failed attempt {}/{}, {} remaining",
                    lockout_status.failed_attempts,
                    lockout_status.failed_attempts + lockout_status.remaining_attempts,
                    lockout_status.remaining_attempts
                ))
            };

            state.audit_service.log_auth_event(
                AuditEventType::LoginFailed,
                &request.username,
                AuditResult::Failure,
                client_ip,
                details,
            );

            // If account got locked due to this attempt, log the lockout event
            if lockout_status.locked {
                state.audit_service.log_auth_event(
                    AuditEventType::AccountLocked,
                    &request.username,
                    AuditResult::Success,
                    None,
                    Some(format!(
                        "Account locked after {} failed attempts. Lockout expires: {:?}",
                        lockout_status.failed_attempts,
                        lockout_status.lockout_expires
                    )),
                );
            }

            // Return error with lockout information
            let mut response = json!({
                "error": format!("Invalid username or password: {}", e),
                "failed_attempts": lockout_status.failed_attempts,
                "remaining_attempts": lockout_status.remaining_attempts,
            });

            if lockout_status.locked {
                response["locked"] = json!(true);
                response["lockout_expires"] = json!(lockout_status.lockout_expires);
                response["lockout_remaining_seconds"] = json!(lockout_status.lockout_remaining_seconds);
            }

            (StatusCode::UNAUTHORIZED, Json(response))
        }
    }
}

#[derive(Debug, Deserialize)]
struct LogoutRequest {
    user_id: String,
}

async fn auth_logout(
    State(state): State<AppState>,
    Json(request): Json<LogoutRequest>,
) -> impl IntoResponse {
    match state.auth_service.logout(&request.user_id) {
        Ok(_) => (StatusCode::OK, Json(json!({"success": true}))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        ),
    }
}

#[derive(Debug, Deserialize)]
struct RefreshRequest {
    refresh_token: String,
}

async fn auth_refresh(
    State(state): State<AppState>,
    Json(request): Json<RefreshRequest>,
) -> impl IntoResponse {
    match state.auth_service.refresh_token(&request.refresh_token) {
        Ok(token) => {
            let response = json!({
                "access_token": token.access_token,
                "token_type": token.token_type,
                "expires_in": token.expires_in,
            });
            (StatusCode::OK, Json(response))
        }
        Err(e) => (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": format!("Token refresh failed: {}", e)})),
        ),
    }
}

// ==================== Token Revocation ====================

/// Request to revoke a specific token
#[derive(Debug, Deserialize)]
struct RevokeTokenRequest {
    /// The token to revoke (JWT access token)
    token: String,
}

/// Revoke a specific token (logout)
///
/// POST /api/v1/auth/revoke
///
/// This endpoint allows a user to revoke their own token (logout).
/// The token to be revoked is provided in the request body.
/// No authentication header is required - the token being revoked serves as proof of ownership.
async fn auth_revoke(
    State(state): State<AppState>,
    Json(request): Json<RevokeTokenRequest>,
) -> impl IntoResponse {
    // Get the token to revoke
    let token_to_revoke = &request.token;

    // Validate the token and extract claims
    let claims = match state.auth_service.validate_token_claims(token_to_revoke) {
        Ok(claims) => claims,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": format!("Invalid token: {}", e)})),
            );
        }
    };

    // Get the expiration time from the token
    let expires_at = chrono::DateTime::from_timestamp(claims.exp as i64, 0)
        .unwrap_or_else(|| chrono::Utc::now() + chrono::Duration::hours(1));

    // Revoke the token
    match state.auth_service.revoke_token(
        claims.jti.clone(),
        claims.sub.clone(),
        claims.username.clone(),
        expires_at,
        RevocationReason::Logout,
    ) {
        Ok(_) => {
            // Log the revocation as TokenRevoked (with logout context)
            state.audit_service.log_event(
                AuditEventType::TokenRevoked,
                Some(claims.sub.clone()),
                Some(claims.username.clone()),
                "revoke_token".to_string(),
                format!("jti:{}", claims.jti),
                AuditResult::Success,
                None, // IP address can be extracted from request if needed
                None, // User agent
                serde_json::json!({
                    "reason": "logout",
                    "jti": claims.jti
                }),
            );

            (
                StatusCode::OK,
                Json(json!({
                    "success": true,
                    "message": "Token revoked successfully",
                    "jti": claims.jti
                })),
            )
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("Failed to revoke token: {}", e)})),
        ),
    }
}

/// Request to revoke all tokens for a user
#[derive(Debug, Deserialize)]
struct RevokeAllTokensRequest {
    /// User ID whose tokens should be revoked
    user_id: String,
    /// Optional reason for revocation
    reason: Option<String>,
}

/// Revoke all tokens for a user (Admin only)
///
/// POST /api/v1/auth/revoke-all
/// Requires Admin role
async fn auth_revoke_all(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(request): Json<RevokeAllTokensRequest>,
) -> impl IntoResponse {
    // Require admin privileges
    let admin_id = match extract_admin_from_token(&headers, &state) {
        Ok(uid) => uid,
        Err(status) => {
            return (
                status,
                Json(json!({"error": "Admin privileges required to revoke all tokens for a user"})),
            );
        }
    };

    // Get the user to validate they exist
    let user_id = match uuid::Uuid::parse_str(&request.user_id) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid user ID format"})),
            );
        }
    };

    let user = match state.user_service.get_user(&crate::security::UserId(user_id)) {
        Some(user) => user,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "User not found"})),
            );
        }
    };

    // Determine the revocation reason
    let reason = match request.reason.as_deref() {
        Some("security_incident") => RevocationReason::SecurityIncident,
        Some("password_changed") => RevocationReason::PasswordChanged,
        _ => RevocationReason::AdminRevoke,
    };

    // Revoke all tokens for the user
    match state.auth_service.revoke_all_user_tokens(
        &request.user_id,
        &user.username,
        reason.clone(),
    ) {
        Ok(count) => {
            // Log the revocation using AllTokensRevoked event type
            state.audit_service.log_event(
                AuditEventType::AllTokensRevoked,
                Some(request.user_id.clone()),
                Some(user.username.clone()),
                "revoke_all_tokens".to_string(),
                format!("user:{}", request.user_id),
                AuditResult::Success,
                None,
                None,
                json!({
                    "reason": reason.to_string(),
                    "previously_revoked_count": count,
                    "revoked_by_admin_id": format!("{:?}", admin_id)
                }),
            );

            (
                StatusCode::OK,
                Json(json!({
                    "success": true,
                    "message": "All tokens revoked for user",
                    "user_id": request.user_id,
                    "previously_revoked_count": count
                })),
            )
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("Failed to revoke tokens: {}", e)})),
        ),
    }
}

// ==================== API Key Management ====================

#[derive(Debug, Deserialize)]
struct CreateApiKeyRequest {
    name: String,
    expires_in_days: Option<u32>,
}

#[derive(Debug, Serialize)]
struct CreateApiKeyResponse {
    key: String,
    id: String,
    prefix: String,
    name: String,
    created_at: String,
    expires_at: Option<String>,
}

#[derive(Debug, Serialize)]
struct ListApiKeysResponse {
    keys: Vec<ApiKeyInfo>,
}

#[derive(Debug, Serialize)]
struct ApiKeyInfo {
    id: String,
    prefix: String,
    name: String,
    created_at: String,
    expires_at: Option<String>,
    last_used: Option<String>,
    is_active: bool,
}

async fn api_key_create(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(request): Json<CreateApiKeyRequest>,
) -> impl IntoResponse {
    // Extract user_id from JWT token in Authorization header
    let user_id = match extract_user_from_auth(&headers, &state) {
        Ok(uid) => uid,
        Err(status) => {
            return (
                status,
                Json(json!({"error": "Unauthorized: invalid or missing JWT token"})),
            )
        }
    };

    // Get username for audit logging
    let username = state.user_service.get_user(&user_id)
        .map(|u| u.username)
        .unwrap_or_else(|| "unknown".to_string());

    // Generate API key with optional expiration
    match state
        .token_service
        .generate_api_key(user_id, request.name.clone(), request.expires_in_days)
    {
        Ok((key, api_key)) => {
            // Log successful API key creation
            state.audit_service.log_api_key_event(
                AuditEventType::ApiKeyCreated,
                &user_id.0.to_string(),
                &username,
                &api_key.id,
                AuditResult::Success,
                None,
            );

            let response = CreateApiKeyResponse {
                key,
                id: api_key.id,
                prefix: api_key.prefix,
                name: api_key.name,
                created_at: api_key.created_at.to_rfc3339(),
                expires_at: api_key.expires_at.map(|dt| dt.to_rfc3339()),
            };
            (StatusCode::CREATED, Json(json!(response)))
        }
        Err(e) => {
            // Log failed API key creation
            state.audit_service.log_api_key_event(
                AuditEventType::ApiKeyCreated,
                &user_id.0.to_string(),
                &username,
                "unknown",
                AuditResult::Failure,
                None,
            );

            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("Failed to create API key: {}", e)})),
            )
        }
    }
}

async fn api_key_list(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    // Extract user_id from JWT token
    let user_id = match extract_user_from_auth(&headers, &state) {
        Ok(uid) => uid,
        Err(status) => {
            return (
                status,
                Json(json!({"error": "Unauthorized: invalid or missing JWT token"})),
            )
        }
    };

    // List API keys for user
    let keys = state.token_service.list_api_keys(&user_id);
    let key_infos: Vec<ApiKeyInfo> = keys
        .into_iter()
        .map(|k| ApiKeyInfo {
            id: k.id,
            prefix: k.prefix,
            name: k.name,
            created_at: k.created_at.to_rfc3339(),
            expires_at: k.expires_at.map(|dt| dt.to_rfc3339()),
            last_used: k.last_used.map(|dt| dt.to_rfc3339()),
            is_active: k.is_active,
        })
        .collect();

    (
        StatusCode::OK,
        Json(json!(ListApiKeysResponse { keys: key_infos })),
    )
}

async fn api_key_revoke(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    axum::extract::Path(key_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    // Extract user_id from JWT token to verify ownership
    let user_id = match extract_user_from_auth(&headers, &state) {
        Ok(uid) => uid,
        Err(status) => {
            return (
                status,
                Json(json!({"error": "Unauthorized: invalid or missing JWT token"})),
            )
        }
    };

    // Get username for audit logging
    let username = state.user_service.get_user(&user_id)
        .map(|u| u.username)
        .unwrap_or_else(|| "unknown".to_string());

    // Revoke API key by ID
    match state.token_service.revoke_api_key_by_id(&key_id) {
        Ok(_) => {
            state.audit_service.log_api_key_event(
                AuditEventType::ApiKeyRevoked,
                &user_id.0.to_string(),
                &username,
                &key_id,
                AuditResult::Success,
                None,
            );

            (
                StatusCode::OK,
                Json(json!({"success": true, "message": "API key revoked"})),
            )
        }
        Err(e) => {
            state.audit_service.log_api_key_event(
                AuditEventType::ApiKeyRevoked,
                &user_id.0.to_string(),
                &username,
                &key_id,
                AuditResult::Failure,
                None,
            );

            (
                StatusCode::NOT_FOUND,
                Json(json!({"error": format!("Failed to revoke API key: {}", e)})),
            )
        }
    }
}

// ==================== API Key Rotation ====================

#[derive(Debug, Deserialize)]
struct RotateApiKeyRequest {
    current_key: String,
    new_name: Option<String>,
    expires_in_days: Option<u32>,
}

async fn api_key_rotate(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(request): Json<RotateApiKeyRequest>,
) -> impl IntoResponse {
    // Extract user_id from JWT token or API key
    let user_id = match extract_user_from_auth(&headers, &state) {
        Ok(uid) => uid,
        Err(status) => {
            return (
                status,
                Json(json!({"error": "Unauthorized: invalid or missing authentication"})),
            )
        }
    };

    // Get username for audit logging
    let username = state.user_service.get_user(&user_id)
        .map(|u| u.username)
        .unwrap_or_else(|| "unknown".to_string());

    // Rotate API key
    match state
        .token_service
        .rotate_api_key(&request.current_key, request.new_name.clone(), request.expires_in_days)
    {
        Ok((new_key, api_key)) => {
            // Log successful API key rotation
            state.audit_service.log_api_key_event(
                AuditEventType::ApiKeyCreated,
                &user_id.0.to_string(),
                &username,
                &api_key.id,
                AuditResult::Success,
                Some("rotated from previous key".to_string()),
            );

            let response = CreateApiKeyResponse {
                key: new_key,
                id: api_key.id,
                prefix: api_key.prefix,
                name: api_key.name,
                created_at: api_key.created_at.to_rfc3339(),
                expires_at: api_key.expires_at.map(|dt| dt.to_rfc3339()),
            };
            (StatusCode::CREATED, Json(json!(response)))
        }
        Err(e) => {
            state.audit_service.log_api_key_event(
                AuditEventType::ApiKeyCreated,
                &user_id.0.to_string(),
                &username,
                "unknown",
                AuditResult::Failure,
                Some(format!("rotation failed: {}", e)),
            );

            (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": format!("Failed to rotate API key: {}", e)})),
            )
        }
    }
}

/// Helper function to extract user_id from either JWT token or API key
/// Supports both Bearer token (Authorization: Bearer <token>) and API key (X-API-Key: <key>)
fn extract_user_from_auth(
    headers: &axum::http::HeaderMap,
    state: &AppState,
) -> Result<crate::security::UserId, StatusCode> {
    use crate::security::UserId;
    use uuid::Uuid;

    // Try X-API-Key header first
    if let Some(api_key_header) = headers.get("X-API-Key").and_then(|v| v.to_str().ok()) {
        // Validate API key and get associated user_id
        match state.token_service.validate_api_key(api_key_header) {
            Ok(user_id) => return Ok(user_id),
            Err(_) => return Err(StatusCode::UNAUTHORIZED),
        }
    }

    // Fall back to JWT Bearer token
    if let Some(auth_header) = headers.get("Authorization").and_then(|v| v.to_str().ok()) {
        if let Some(token) = auth_header.strip_prefix("Bearer ") {
            match state.token_service.validate_jwt(token) {
                Ok(claims) => {
                    let uuid = Uuid::parse_str(&claims.sub).map_err(|_| StatusCode::UNAUTHORIZED)?;
                    return Ok(UserId(uuid));
                }
                Err(_) => return Err(StatusCode::UNAUTHORIZED),
            }
        }
    }

    // No valid authentication found
    Err(StatusCode::UNAUTHORIZED)
}

// ==================== User Management Endpoints ====================

/// Request/response DTOs for user management
#[derive(Debug, Deserialize)]
struct CreateUserRequest {
    username: String,
    email: String,
    password: String,
    roles: Option<Vec<super::security::rbac::Role>>,
}

#[derive(Debug, Deserialize)]
struct UpdateUserRequest {
    email: Option<String>,
    password: Option<String>,
    is_active: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct UpdateRolesRequest {
    roles: Vec<super::security::rbac::Role>,
}

#[derive(Debug, Serialize)]
struct UserResponse {
    id: String,
    username: String,
    email: String,
    roles: Vec<super::security::rbac::Role>,
    is_active: bool,
    created_at: String,
    updated_at: String,
    last_login: Option<String>,
}

#[derive(Debug, Serialize)]
struct ListUsersResponse {
    users: Vec<UserResponse>,
}

/// Helper to check if user has Admin role
fn require_admin(claims: &super::security::token::Claims) -> Result<(), StatusCode> {
    use super::security::rbac::Role;

    if claims.roles.contains(&Role::Admin) {
        Ok(())
    } else {
        Err(StatusCode::FORBIDDEN)
    }
}

/// Helper to extract and validate admin user from either JWT token or API key
fn extract_admin_from_token(
    headers: &axum::http::HeaderMap,
    state: &AppState,
) -> Result<crate::security::UserId, StatusCode> {
    use crate::security::UserId;
    use uuid::Uuid;

    // Try X-API-Key header first
    if let Some(api_key_header) = headers.get("X-API-Key").and_then(|v| v.to_str().ok()) {
        // Validate API key and get associated user_id
        let user_id = state.token_service.validate_api_key(api_key_header)
            .map_err(|_| StatusCode::UNAUTHORIZED)?;

        // Get user to check admin role
        let user = state.user_service.get_user(&user_id)
            .ok_or(StatusCode::UNAUTHORIZED)?;

        // Check if user has Admin role
        if !user.roles.contains(&super::security::rbac::Role::Admin) {
            return Err(StatusCode::FORBIDDEN);
        }

        return Ok(user_id);
    }

    // Fall back to JWT Bearer token
    let auth_header = headers
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let token = auth_header
        .strip_prefix("Bearer ")
        .ok_or(StatusCode::UNAUTHORIZED)?;

    // Use auth_service.validate_token which checks the blacklist
    match state.auth_service.validate_token(token) {
        Ok(user) => {
            // Check if user has Admin role
            if !user.roles.contains(&super::security::rbac::Role::Admin) {
                return Err(StatusCode::FORBIDDEN);
            }

            Ok(user.id)
        }
        Err(_) => Err(StatusCode::UNAUTHORIZED),
    }
}

/// Create a new user (Admin only)
async fn user_create(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(request): Json<CreateUserRequest>,
) -> impl IntoResponse {
    // Require admin privileges
    let admin_id = match extract_admin_from_token(&headers, &state) {
        Ok(uid) => uid,
        Err(status) => {
            return (
                status,
                Json(json!({"error": "Unauthorized: Admin access required"})),
            )
        }
    };

    // Get admin username for audit logging
    let admin_username = state.user_service.get_user(&admin_id)
        .map(|u| u.username)
        .unwrap_or_else(|| "unknown".to_string());

    // Create user with provided roles or default to Read
    let roles = request.roles.unwrap_or_else(|| vec![super::security::rbac::Role::Read]);

    match state.user_service.create_user(
        request.username.clone(),
        request.email.clone(),
        &request.password,
    ) {
        Ok(mut user) => {
            // Update roles if provided
            user.roles = roles.clone();

            // Log successful user creation
            state.audit_service.log_user_event(
                AuditEventType::UserCreated,
                &admin_id.0.to_string(),
                &admin_username,
                &user.id.0.to_string(),
                AuditResult::Success,
                None,
                serde_json::json!({
                    "new_username": user.username,
                    "new_email": user.email,
                    "roles": roles
                }),
            );

            let response = UserResponse {
                id: user.id.0.to_string(),
                username: user.username,
                email: user.email,
                roles: user.roles,
                is_active: user.is_active,
                created_at: user.created_at.to_rfc3339(),
                updated_at: user.updated_at.to_rfc3339(),
                last_login: user.last_login.map(|dt| dt.to_rfc3339()),
            };

            (StatusCode::CREATED, Json(json!(response)))
        }
        Err(e) => {
            // Log failed user creation
            state.audit_service.log_user_event(
                AuditEventType::UserCreated,
                &admin_id.0.to_string(),
                &admin_username,
                "unknown",
                AuditResult::Failure,
                None,
                serde_json::json!({
                    "attempted_username": request.username,
                    "error": e.to_string()
                }),
            );

            (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": format!("Failed to create user: {}", e)})),
            )
        }
    }
}

/// List all users (Admin only)
async fn user_list(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    // Require admin privileges
    let _admin_id = match extract_admin_from_token(&headers, &state) {
        Ok(uid) => uid,
        Err(status) => {
            return (
                status,
                Json(json!({"error": "Unauthorized: Admin access required"})),
            )
        }
    };

    let users = state.user_service.list_users();
    let user_responses: Vec<UserResponse> = users
        .into_iter()
        .map(|u| UserResponse {
            id: u.id.0.to_string(),
            username: u.username,
            email: u.email,
            roles: u.roles,
            is_active: u.is_active,
            created_at: u.created_at.to_rfc3339(),
            updated_at: u.updated_at.to_rfc3339(),
            last_login: u.last_login.map(|dt| dt.to_rfc3339()),
        })
        .collect();

    (
        StatusCode::OK,
        Json(json!(ListUsersResponse { users: user_responses })),
    )
}

/// Get user details (own user or Admin)
async fn user_get(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    axum::extract::Path(user_id_str): axum::extract::Path<String>,
) -> impl IntoResponse {
    use uuid::Uuid;
    use crate::security::UserId;

    // Extract requesting user
    let requester_id = match extract_user_from_auth(&headers, &state) {
        Ok(uid) => uid,
        Err(status) => {
            return (
                status,
                Json(json!({"error": "Unauthorized: invalid or missing JWT token"})),
            )
        }
    };

    // Parse target user ID
    let target_uuid = match Uuid::parse_str(&user_id_str) {
        Ok(uuid) => uuid,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid user ID format"})),
            )
        }
    };
    let target_id = UserId(target_uuid);

    // Check if requester is admin or requesting own user
    let auth_header = headers.get("Authorization").and_then(|v| v.to_str().ok()).unwrap();
    let token = auth_header.strip_prefix("Bearer ").unwrap();
    let is_admin = if let Ok(claims) = state.token_service.validate_jwt(token) {
        require_admin(&claims).is_ok()
    } else {
        false
    };

    if requester_id != target_id && !is_admin {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error": "Access denied: can only view own user or admin required"})),
        );
    }

    match state.user_service.get_user(&target_id) {
        Some(user) => {
            let response = UserResponse {
                id: user.id.0.to_string(),
                username: user.username,
                email: user.email,
                roles: user.roles,
                is_active: user.is_active,
                created_at: user.created_at.to_rfc3339(),
                updated_at: user.updated_at.to_rfc3339(),
                last_login: user.last_login.map(|dt| dt.to_rfc3339()),
            };
            (StatusCode::OK, Json(json!(response)))
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "User not found"})),
        ),
    }
}

/// Update user (own user or Admin)
async fn user_update(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    axum::extract::Path(user_id_str): axum::extract::Path<String>,
    Json(request): Json<UpdateUserRequest>,
) -> impl IntoResponse {
    use uuid::Uuid;
    use crate::security::UserId;

    // Extract requesting user
    let requester_id = match extract_user_from_auth(&headers, &state) {
        Ok(uid) => uid,
        Err(status) => {
            return (
                status,
                Json(json!({"error": "Unauthorized: invalid or missing JWT token"})),
            )
        }
    };

    // Get requester username for audit logging
    let requester_username = state.user_service.get_user(&requester_id)
        .map(|u| u.username)
        .unwrap_or_else(|| "unknown".to_string());

    // Parse target user ID
    let target_uuid = match Uuid::parse_str(&user_id_str) {
        Ok(uuid) => uuid,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid user ID format"})),
            )
        }
    };
    let target_id = UserId(target_uuid);

    // Check if requester is admin or updating own user
    let auth_header = headers.get("Authorization").and_then(|v| v.to_str().ok()).unwrap();
    let token = auth_header.strip_prefix("Bearer ").unwrap();
    let is_admin = if let Ok(claims) = state.token_service.validate_jwt(token) {
        require_admin(&claims).is_ok()
    } else {
        false
    };

    if requester_id != target_id && !is_admin {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error": "Access denied: can only update own user or admin required"})),
        );
    }

    // Get existing user
    let mut user = match state.user_service.get_user(&target_id) {
        Some(u) => u,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "User not found"})),
            )
        }
    };

    // Track what was updated for audit logging
    let mut changes = serde_json::Map::new();
    let password_changed = request.password.is_some();

    // Apply updates
    if let Some(ref email) = request.email {
        changes.insert("email_changed".to_string(), serde_json::json!(true));
        user.email = email.clone();
    }
    if let Some(ref password) = request.password {
        // Re-hash password using user method
        if let Err(e) = user.update_password(password) {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("Failed to hash password: {}", e)})),
            )
        }
        changes.insert("password_changed".to_string(), serde_json::json!(true));
    }
    if let Some(is_active) = request.is_active {
        // Only admin can change is_active status
        if !is_admin {
            return (
                StatusCode::FORBIDDEN,
                Json(json!({"error": "Only admin can change user active status"})),
            );
        }
        changes.insert("is_active".to_string(), serde_json::json!(is_active));
        user.is_active = is_active;
    }

    match state.user_service.update_user(user.clone()) {
        Ok(_) => {
            // Log the appropriate event type
            let event_type = if password_changed {
                AuditEventType::UserPasswordChanged
            } else {
                AuditEventType::UserUpdated
            };

            state.audit_service.log_user_event(
                event_type,
                &requester_id.0.to_string(),
                &requester_username,
                &target_id.0.to_string(),
                AuditResult::Success,
                None,
                serde_json::Value::Object(changes),
            );

            let response = UserResponse {
                id: user.id.0.to_string(),
                username: user.username,
                email: user.email,
                roles: user.roles,
                is_active: user.is_active,
                created_at: user.created_at.to_rfc3339(),
                updated_at: user.updated_at.to_rfc3339(),
                last_login: user.last_login.map(|dt| dt.to_rfc3339()),
            };
            (StatusCode::OK, Json(json!(response)))
        }
        Err(e) => {
            state.audit_service.log_user_event(
                AuditEventType::UserUpdated,
                &requester_id.0.to_string(),
                &requester_username,
                &target_id.0.to_string(),
                AuditResult::Failure,
                None,
                serde_json::json!({"error": e.to_string()}),
            );

            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("Failed to update user: {}", e)})),
            )
        }
    }
}

/// Delete user (Admin only)
async fn user_delete(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    axum::extract::Path(user_id_str): axum::extract::Path<String>,
) -> impl IntoResponse {
    use uuid::Uuid;
    use crate::security::UserId;

    // Require admin privileges
    let admin_id = match extract_admin_from_token(&headers, &state) {
        Ok(uid) => uid,
        Err(status) => {
            return (
                status,
                Json(json!({"error": "Unauthorized: Admin access required"})),
            )
        }
    };

    // Get admin username for audit logging
    let admin_username = state.user_service.get_user(&admin_id)
        .map(|u| u.username)
        .unwrap_or_else(|| "unknown".to_string());

    // Parse target user ID
    let target_uuid = match Uuid::parse_str(&user_id_str) {
        Ok(uuid) => uuid,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid user ID format"})),
            )
        }
    };
    let target_id = UserId(target_uuid);

    // Get the target user info before deletion for audit log
    let target_username = state.user_service.get_user(&target_id)
        .map(|u| u.username)
        .unwrap_or_else(|| "unknown".to_string());

    match state.user_service.delete_user(&target_id) {
        Ok(_) => {
            state.audit_service.log_user_event(
                AuditEventType::UserDeleted,
                &admin_id.0.to_string(),
                &admin_username,
                &target_id.0.to_string(),
                AuditResult::Success,
                None,
                serde_json::json!({"deleted_username": target_username}),
            );

            (
                StatusCode::OK,
                Json(json!({"success": true, "message": "User deleted"})),
            )
        }
        Err(e) => {
            state.audit_service.log_user_event(
                AuditEventType::UserDeleted,
                &admin_id.0.to_string(),
                &admin_username,
                &target_id.0.to_string(),
                AuditResult::Failure,
                None,
                serde_json::json!({"error": e.to_string()}),
            );

            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("Failed to delete user: {}", e)})),
            )
        }
    }
}

/// Update user roles (Admin only)
async fn user_update_roles(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    axum::extract::Path(user_id_str): axum::extract::Path<String>,
    Json(request): Json<UpdateRolesRequest>,
) -> impl IntoResponse {
    use uuid::Uuid;
    use crate::security::UserId;

    // Require admin privileges
    let admin_id = match extract_admin_from_token(&headers, &state) {
        Ok(uid) => uid,
        Err(status) => {
            return (
                status,
                Json(json!({"error": "Unauthorized: Admin access required"})),
            )
        }
    };

    // Get admin username for audit logging
    let admin_username = state.user_service.get_user(&admin_id)
        .map(|u| u.username)
        .unwrap_or_else(|| "unknown".to_string());

    // Parse target user ID
    let target_uuid = match Uuid::parse_str(&user_id_str) {
        Ok(uuid) => uuid,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid user ID format"})),
            )
        }
    };
    let target_id = UserId(target_uuid);

    // Get existing user
    let mut user = match state.user_service.get_user(&target_id) {
        Some(u) => u,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "User not found"})),
            )
        }
    };

    // Track old roles for audit log
    let old_roles = user.roles.clone();
    let new_roles = request.roles.clone();

    // Update roles
    user.roles = request.roles;

    match state.user_service.update_user(user.clone()) {
        Ok(_) => {
            // Log role change as RoleAssigned event (covers both assignment and removal)
            state.audit_service.log_user_event(
                AuditEventType::RoleAssigned,
                &admin_id.0.to_string(),
                &admin_username,
                &target_id.0.to_string(),
                AuditResult::Success,
                None,
                serde_json::json!({
                    "target_username": user.username,
                    "old_roles": old_roles,
                    "new_roles": new_roles
                }),
            );

            let response = UserResponse {
                id: user.id.0.to_string(),
                username: user.username,
                email: user.email,
                roles: user.roles,
                is_active: user.is_active,
                created_at: user.created_at.to_rfc3339(),
                updated_at: user.updated_at.to_rfc3339(),
                last_login: user.last_login.map(|dt| dt.to_rfc3339()),
            };
            (StatusCode::OK, Json(json!(response)))
        }
        Err(e) => {
            state.audit_service.log_user_event(
                AuditEventType::RoleAssigned,
                &admin_id.0.to_string(),
                &admin_username,
                &target_id.0.to_string(),
                AuditResult::Failure,
                None,
                serde_json::json!({"error": e.to_string()}),
            );

            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("Failed to update user roles: {}", e)})),
            )
        }
    }
}

// ==================== Rate Limit Policy Management Endpoints ====================

use crate::security::{RateLimitPolicy, PolicyId, EndpointType};

/// Request/response DTOs for rate limit policy management
#[derive(Debug, Deserialize)]
struct CreateRateLimitPolicyRequest {
    name: String,
    endpoint_type: EndpointType,
    max_requests: u32,
    window_secs: u64,
    enabled: bool,
}

#[derive(Debug, Deserialize)]
struct UpdateRateLimitPolicyRequest {
    name: Option<String>,
    max_requests: Option<u32>,
    window_secs: Option<u64>,
    enabled: Option<bool>,
}

#[derive(Debug, Serialize)]
struct RateLimitPolicyResponse {
    id: String,
    name: String,
    endpoint_type: EndpointType,
    max_requests: u32,
    window_secs: u64,
    enabled: bool,
    created_at: String,
    updated_at: String,
    created_by: String,
}

#[derive(Debug, Serialize)]
struct ListRateLimitPoliciesResponse {
    policies: Vec<RateLimitPolicyResponse>,
}

impl From<RateLimitPolicy> for RateLimitPolicyResponse {
    fn from(policy: RateLimitPolicy) -> Self {
        Self {
            id: policy.id.0.to_string(),
            name: policy.name,
            endpoint_type: policy.endpoint_type,
            max_requests: policy.max_requests,
            window_secs: policy.window_secs,
            enabled: policy.enabled,
            created_at: policy.created_at.to_rfc3339(),
            updated_at: policy.updated_at.to_rfc3339(),
            created_by: policy.created_by,
        }
    }
}

/// Create a new rate limit policy (Admin only)
async fn rate_limit_create(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(request): Json<CreateRateLimitPolicyRequest>,
) -> impl IntoResponse {
    // Require admin privileges
    let admin_id = match extract_admin_from_token(&headers, &state) {
        Ok(uid) => uid,
        Err(status) => {
            return (
                status,
                Json(json!({"error": "Unauthorized: Admin access required"})),
            )
        }
    };

    // Create policy
    let policy = RateLimitPolicy {
        id: PolicyId::new(),
        name: request.name,
        endpoint_type: request.endpoint_type,
        max_requests: request.max_requests,
        window_secs: request.window_secs,
        enabled: request.enabled,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        created_by: admin_id.0.to_string(),
    };

    let policy_id = state.rate_limit_service.create_policy(policy.clone());

    // Get the stored policy to return the correct ID
    let stored_policy = state.rate_limit_service.get_policy(policy_id).unwrap();
    let response = RateLimitPolicyResponse::from(stored_policy);

    (StatusCode::CREATED, Json(json!(response)))
}

/// List all rate limit policies (Admin only)
async fn rate_limit_list(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    // Require admin privileges
    let _admin_id = match extract_admin_from_token(&headers, &state) {
        Ok(uid) => uid,
        Err(status) => {
            return (
                status,
                Json(json!({"error": "Unauthorized: Admin access required"})),
            )
        }
    };

    let policies = state.rate_limit_service.list_policies();
    let policy_responses: Vec<RateLimitPolicyResponse> = policies
        .into_iter()
        .map(RateLimitPolicyResponse::from)
        .collect();

    (
        StatusCode::OK,
        Json(json!(ListRateLimitPoliciesResponse { policies: policy_responses })),
    )
}

/// Get a specific rate limit policy (Admin only)
async fn rate_limit_get(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    axum::extract::Path(policy_id_str): axum::extract::Path<String>,
) -> impl IntoResponse {
    use uuid::Uuid;

    // Require admin privileges
    let _admin_id = match extract_admin_from_token(&headers, &state) {
        Ok(uid) => uid,
        Err(status) => {
            return (
                status,
                Json(json!({"error": "Unauthorized: Admin access required"})),
            )
        }
    };

    // Parse policy ID
    let policy_uuid = match Uuid::parse_str(&policy_id_str) {
        Ok(uuid) => uuid,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid policy ID format"})),
            )
        }
    };
    let policy_id = PolicyId(policy_uuid);

    match state.rate_limit_service.get_policy(policy_id) {
        Some(policy) => {
            let response = RateLimitPolicyResponse::from(policy);
            (StatusCode::OK, Json(json!(response)))
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "Rate limit policy not found"})),
        ),
    }
}

/// Update a rate limit policy (Admin only)
async fn rate_limit_update(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    axum::extract::Path(policy_id_str): axum::extract::Path<String>,
    Json(request): Json<UpdateRateLimitPolicyRequest>,
) -> impl IntoResponse {
    use uuid::Uuid;

    // Require admin privileges
    let _admin_id = match extract_admin_from_token(&headers, &state) {
        Ok(uid) => uid,
        Err(status) => {
            return (
                status,
                Json(json!({"error": "Unauthorized: Admin access required"})),
            )
        }
    };

    // Parse policy ID
    let policy_uuid = match Uuid::parse_str(&policy_id_str) {
        Ok(uuid) => uuid,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid policy ID format"})),
            )
        }
    };
    let policy_id = PolicyId(policy_uuid);

    // Get existing policy
    let mut policy = match state.rate_limit_service.get_policy(policy_id) {
        Some(p) => p,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "Rate limit policy not found"})),
            )
        }
    };

    // Update fields
    if let Some(name) = request.name {
        policy.name = name;
    }
    if let Some(max_requests) = request.max_requests {
        policy.max_requests = max_requests;
    }
    if let Some(window_secs) = request.window_secs {
        policy.window_secs = window_secs;
    }
    if let Some(enabled) = request.enabled {
        policy.enabled = enabled;
    }

    match state.rate_limit_service.update_policy(policy_id, policy.clone()) {
        Some(_) => {
            let response = RateLimitPolicyResponse::from(policy);
            (StatusCode::OK, Json(json!(response)))
        }
        None => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Failed to update rate limit policy"})),
        ),
    }
}

/// Delete a rate limit policy (Admin only)
async fn rate_limit_delete(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    axum::extract::Path(policy_id_str): axum::extract::Path<String>,
) -> impl IntoResponse {
    use uuid::Uuid;

    // Require admin privileges
    let _admin_id = match extract_admin_from_token(&headers, &state) {
        Ok(uid) => uid,
        Err(status) => {
            return (
                status,
                Json(json!({"error": "Unauthorized: Admin access required"})),
            )
        }
    };

    // Parse policy ID
    let policy_uuid = match Uuid::parse_str(&policy_id_str) {
        Ok(uuid) => uuid,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid policy ID format"})),
            )
        }
    };
    let policy_id = PolicyId(policy_uuid);

    match state.rate_limit_service.delete_policy(policy_id) {
        Some(_) => (
            StatusCode::OK,
            Json(json!({"success": true, "message": "Rate limit policy deleted"})),
        ),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "Rate limit policy not found"})),
        ),
    }
}

// ==================== Audit Log Query Endpoint ====================

use crate::security::AuditFilter;

/// Query parameters for audit log filtering
#[derive(Debug, Deserialize)]
struct AuditLogsQueryParams {
    user_id: Option<String>,
    username: Option<String>,
    event_type: Option<String>,
    action: Option<String>,
    result: Option<String>,
    ip_address: Option<String>,
    start_time: Option<String>,
    end_time: Option<String>,
    limit: Option<usize>,
}

/// Response format for audit events
#[derive(Debug, Serialize)]
struct AuditEventResponse {
    event_id: String,
    event_type: String,
    timestamp: String,
    user_id: Option<String>,
    username: Option<String>,
    action: String,
    resource: String,
    result: String,
    ip_address: Option<String>,
    user_agent: Option<String>,
    metadata: serde_json::Value,
    transaction_time: String,
}

/// Query audit logs (Admin only)
async fn audit_logs_query(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    AxumQuery(params): AxumQuery<AuditLogsQueryParams>,
) -> impl IntoResponse {
    // Require admin privileges
    let _admin_id = match extract_admin_from_token(&headers, &state) {
        Ok(uid) => uid,
        Err(status) => {
            return (
                status,
                Json(json!({"error": "Unauthorized: Admin access required"})),
            )
        }
    };

    // Build filter from query params
    let mut filter = AuditFilter::new();

    if let Some(user_id) = params.user_id {
        filter = filter.user_id(user_id);
    }
    if let Some(username) = params.username {
        filter = filter.username(username);
    }
    if let Some(event_type_str) = params.event_type {
        // Parse event type from string
        let event_type = match event_type_str.as_str() {
            "login" => Some(AuditEventType::Login),
            "login_failed" => Some(AuditEventType::LoginFailed),
            "logout" => Some(AuditEventType::Logout),
            "token_refresh" => Some(AuditEventType::TokenRefresh),
            "token_refresh_failed" => Some(AuditEventType::TokenRefreshFailed),
            "api_key_created" => Some(AuditEventType::ApiKeyCreated),
            "api_key_revoked" => Some(AuditEventType::ApiKeyRevoked),
            "api_key_used" => Some(AuditEventType::ApiKeyUsed),
            "api_key_validation_failed" => Some(AuditEventType::ApiKeyValidationFailed),
            "user_created" => Some(AuditEventType::UserCreated),
            "user_updated" => Some(AuditEventType::UserUpdated),
            "user_deleted" => Some(AuditEventType::UserDeleted),
            "user_password_changed" => Some(AuditEventType::UserPasswordChanged),
            "role_assigned" => Some(AuditEventType::RoleAssigned),
            "role_removed" => Some(AuditEventType::RoleRemoved),
            "permission_denied" => Some(AuditEventType::PermissionDenied),
            "access_granted" => Some(AuditEventType::AccessGranted),
            "rate_limit_exceeded" => Some(AuditEventType::RateLimitExceeded),
            "system_startup" => Some(AuditEventType::SystemStartup),
            "system_shutdown" => Some(AuditEventType::SystemShutdown),
            "configuration_changed" => Some(AuditEventType::ConfigurationChanged),
            _ => None,
        };
        if let Some(et) = event_type {
            filter = filter.event_type(et);
        }
    }
    if let Some(action) = params.action {
        filter = filter.action(action);
    }
    if let Some(result_str) = params.result {
        let result = match result_str.as_str() {
            "success" => Some(AuditResult::Success),
            "failure" => Some(AuditResult::Failure),
            "unauthorized" => Some(AuditResult::Unauthorized),
            "forbidden" => Some(AuditResult::Forbidden),
            "error" => Some(AuditResult::Error),
            _ => None,
        };
        if let Some(r) = result {
            filter = filter.result(r);
        }
    }
    if let Some(ip) = params.ip_address {
        filter = filter.ip_address(ip);
    }
    if let (Some(start_str), Some(end_str)) = (&params.start_time, &params.end_time) {
        if let (Ok(start), Ok(end)) = (
            chrono::DateTime::parse_from_rfc3339(start_str),
            chrono::DateTime::parse_from_rfc3339(end_str),
        ) {
            filter = filter.time_range(start.with_timezone(&chrono::Utc), end.with_timezone(&chrono::Utc));
        }
    }

    let limit = params.limit.unwrap_or(100).min(1000); // Max 1000 events per query

    // Query events
    let events = state.audit_service.query_events(filter, limit);

    // Convert to response format
    let event_responses: Vec<AuditEventResponse> = events
        .into_iter()
        .map(|e| AuditEventResponse {
            event_id: e.event_id,
            event_type: e.event_type.to_string(),
            timestamp: e.timestamp.to_rfc3339(),
            user_id: e.user_id,
            username: e.username,
            action: e.action,
            resource: e.resource,
            result: e.result.to_string(),
            ip_address: e.ip_address,
            user_agent: e.user_agent,
            metadata: e.metadata,
            transaction_time: e.transaction_time.to_rfc3339(),
        })
        .collect();

    (
        StatusCode::OK,
        Json(json!({
            "events": event_responses,
            "count": event_responses.len(),
            "limit": limit
        })),
    )
}

// ==================== Account Lockout Handlers ====================

/// List all locked accounts (Admin only)
async fn lockout_list(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    // Require admin privileges
    let _admin_id = match extract_admin_from_token(&headers, &state) {
        Ok(uid) => uid,
        Err(status) => {
            return (
                status,
                Json(json!({"error": "Unauthorized: Admin access required"})),
            )
        }
    };

    let locked_users = state.lockout_service.get_locked_users();

    (
        StatusCode::OK,
        Json(json!({
            "locked_users": locked_users,
            "count": locked_users.len()
        })),
    )
}

/// Get lockout status for a specific user (Admin only)
async fn lockout_status(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(username): Path<String>,
) -> impl IntoResponse {
    // Require admin privileges
    let _admin_id = match extract_admin_from_token(&headers, &state) {
        Ok(uid) => uid,
        Err(status) => {
            return (
                status,
                Json(json!({"error": "Unauthorized: Admin access required"})),
            )
        }
    };

    let status = state.lockout_service.get_user_status(&username);

    (
        StatusCode::OK,
        Json(json!({
            "username": username,
            "status": status
        })),
    )
}

#[derive(Debug, Deserialize)]
struct LockUserRequest {
    reason: Option<String>,
}

/// Manually lock a user account (Admin only)
async fn lockout_lock(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(username): Path<String>,
    Json(request): Json<LockUserRequest>,
) -> impl IntoResponse {
    // Require admin privileges
    let admin_id = match extract_admin_from_token(&headers, &state) {
        Ok(uid) => uid,
        Err(status) => {
            return (
                status,
                Json(json!({"error": "Unauthorized: Admin access required"})),
            )
        }
    };

    state.lockout_service.lock_user(&username, request.reason.clone());

    // Log the manual lock event
    state.audit_service.log_auth_event(
        AuditEventType::AccountLocked,
        &username,
        AuditResult::Success,
        None,
        Some(format!(
            "Manually locked by admin {:?}. Reason: {:?}",
            admin_id,
            request.reason
        )),
    );

    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "message": format!("Account '{}' has been locked", username)
        })),
    )
}

/// Manually unlock a user account (Admin only)
async fn lockout_unlock(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(username): Path<String>,
) -> impl IntoResponse {
    // Require admin privileges
    let admin_id = match extract_admin_from_token(&headers, &state) {
        Ok(uid) => uid,
        Err(status) => {
            return (
                status,
                Json(json!({"error": "Unauthorized: Admin access required"})),
            )
        }
    };

    let was_locked = state.lockout_service.unlock_user(&username);

    if was_locked {
        // Log the unlock event
        state.audit_service.log_auth_event(
            AuditEventType::AccountUnlocked,
            &username,
            AuditResult::Success,
            None,
            Some(format!("Unlocked by admin {:?}", admin_id)),
        );

        (
            StatusCode::OK,
            Json(json!({
                "success": true,
                "message": format!("Account '{}' has been unlocked", username)
            })),
        )
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(json!({
                "success": false,
                "error": format!("No lockout record found for user '{}'", username)
            })),
        )
    }
}

// ==================== LLM Configuration ====================

/// Request body for updating LLM configuration
#[derive(Debug, Deserialize)]
struct LLMConfigUpdateRequest {
    /// Provider type: "openai" or "mock"
    provider: String,
    /// API key (required for OpenAI)
    api_key: Option<String>,
    /// Model name (optional, defaults based on provider)
    model: Option<String>,
    /// Temperature (optional, 0.0-2.0)
    temperature: Option<f32>,
    /// Max tokens (optional)
    max_tokens: Option<u32>,
}

/// Response for LLM status
#[derive(Debug, Serialize)]
struct LLMStatusResponse {
    configured: bool,
    provider: String,
    model: String,
}

/// Get current LLM configuration status
async fn llm_status(State(state): State<AppState>) -> impl IntoResponse {
    let is_configured = state.llm_service.is_configured().await;
    let model_name = state.llm_service.model_name().await;
    let config = state.llm_service.get_config().await;

    let provider_name = match config.provider {
        LLMProviderType::Mock => "mock",
        LLMProviderType::OpenAI => "openai",
    };

    (
        StatusCode::OK,
        Json(json!({
            "configured": is_configured,
            "provider": provider_name,
            "model": model_name,
            "temperature": config.temperature,
            "max_tokens": config.max_tokens
        })),
    )
}

/// Update LLM configuration at runtime
async fn llm_update_config(
    State(state): State<AppState>,
    Json(request): Json<LLMConfigUpdateRequest>,
) -> impl IntoResponse {
    // Build new configuration based on provider type
    let new_config = match request.provider.to_lowercase().as_str() {
        "openai" => {
            let api_key = match request.api_key {
                Some(key) if !key.is_empty() => key,
                _ => {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(json!({
                            "error": "API key is required for OpenAI provider"
                        })),
                    )
                }
            };

            // Use openai_mini as base and customize
            let mut config = LLMConfig::openai_mini(&api_key);

            // Override model if specified
            if let Some(ref model) = request.model {
                config.model = model.clone();
            }

            if let Some(temp) = request.temperature {
                config.temperature = temp;
            }
            if let Some(max_tokens) = request.max_tokens {
                config.max_tokens = max_tokens;
            }

            config
        }
        "mock" => LLMConfig::mock(),
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": format!("Unknown provider type: '{}'. Supported: 'openai', 'mock'", request.provider)
                })),
            )
        }
    };

    // Update the LLM service configuration
    match state.llm_service.update_config(new_config).await {
        Ok(()) => {
            let model_name = state.llm_service.model_name().await;
            let is_configured = state.llm_service.is_configured().await;

            (
                StatusCode::OK,
                Json(json!({
                    "success": true,
                    "message": "LLM configuration updated successfully",
                    "configured": is_configured,
                    "provider": request.provider.to_lowercase(),
                    "model": model_name
                })),
            )
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": format!("Failed to update LLM configuration: {}", e)
            })),
        ),
    }
}

// ==================== Helper Functions ====================

/// Format episode content with intuitive field names based on episode type.
/// For conversations: user_input, agent_response
/// For observations: observation
/// For task execution: action, result
fn format_episode_content(episode: &Episode) -> Value {
    let mut content_map = serde_json::Map::new();

    match &episode.episode_type {
        EpisodeType::Conversation => {
            content_map.insert("user_input".to_string(), json!(episode.content.primary));
            if let Some(ref secondary) = episode.content.secondary {
                content_map.insert("agent_response".to_string(), json!(secondary));
            }
        }
        EpisodeType::Observation => {
            content_map.insert("observation".to_string(), json!(episode.content.primary));
            if let Some(ref secondary) = episode.content.secondary {
                content_map.insert("details".to_string(), json!(secondary));
            }
        }
        EpisodeType::TaskExecution => {
            content_map.insert("action".to_string(), json!(episode.content.primary));
            if let Some(ref secondary) = episode.content.secondary {
                content_map.insert("result".to_string(), json!(secondary));
            }
        }
        _ => {
            // For custom/other types, use generic primary/secondary
            content_map.insert("primary".to_string(), json!(episode.content.primary));
            if let Some(ref secondary) = episode.content.secondary {
                content_map.insert("secondary".to_string(), json!(secondary));
            }
        }
    }

    // Include context if present
    if let Some(ref context) = episode.content.context {
        content_map.insert("context".to_string(), json!(context));
    }

    Value::Object(content_map)
}

fn json_to_property_value(value: &Value) -> Option<PropertyValue> {
    match value {
        Value::Null => Some(PropertyValue::Null),
        Value::Bool(b) => Some(PropertyValue::Boolean(*b)),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(PropertyValue::Integer(i))
            } else if let Some(f) = n.as_f64() {
                Some(PropertyValue::Float(f))
            } else {
                None
            }
        }
        Value::String(s) => Some(PropertyValue::String(s.clone())),
        Value::Array(arr) => {
            let values: Vec<_> = arr.iter().filter_map(json_to_property_value).collect();
            Some(PropertyValue::Array(values))
        }
        Value::Object(obj) => {
            let mut map = HashMap::new();
            for (k, v) in obj {
                if let Some(prop_val) = json_to_property_value(v) {
                    map.insert(k.clone(), prop_val);
                }
            }
            Some(PropertyValue::Map(map))
        }
    }
}

fn property_value_to_json(value: &PropertyValue) -> Value {
    match value {
        PropertyValue::Null => Value::Null,
        PropertyValue::Boolean(b) => Value::Bool(*b),
        PropertyValue::Integer(i) => json!(*i),
        PropertyValue::Float(f) => json!(*f),
        PropertyValue::String(s) => Value::String(s.clone()),
        PropertyValue::Array(list) => Value::Array(list.iter().map(property_value_to_json).collect()),
        PropertyValue::Map(map) => {
            let mut obj = serde_json::Map::new();
            for (k, v) in map.iter() {
                obj.insert(k.clone(), property_value_to_json(v));
            }
            Value::Object(obj)
        }
        // Handle temporal types - convert to JSON strings/numbers for now
        PropertyValue::Date(d) => json!(*d),
        PropertyValue::Time(t) => json!(*t),
        PropertyValue::DateTime(dt) => json!(*dt),
        PropertyValue::Duration(dur) => json!(*dur),
        PropertyValue::Bytes(b) => {
            // Encode bytes as base64 string
            Value::String(base64::Engine::encode(&base64::engine::general_purpose::STANDARD, b))
        }
        PropertyValue::Point2D { x, y, srid } => {
            json!({"x": x, "y": y, "srid": srid})
        }
        PropertyValue::Point3D { x, y, z, srid } => {
            json!({"x": x, "y": y, "z": z, "srid": srid})
        }
    }
}

fn json_map_to_property(map: &HashMap<String, Value>) -> Property {
    let mut props = Property::new();
    for (k, v) in map {
        if let Some(prop_val) = json_to_property_value(v) {
            props.set(k.clone(), prop_val);
        }
    }
    props
}

fn property_to_json_map(props: &Property) -> HashMap<String, Value> {
    let mut result = HashMap::new();
    for (k, v) in props.iter() {
        result.insert(k.clone(), property_value_to_json(v));
    }
    result
}
