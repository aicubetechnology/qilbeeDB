//! HTTP/REST API server implementation using Axum

use axum::{
    extract::{Path, Query as AxumQuery, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::{delete, get, post, put},
    Router,
};
use qilbee_core::{EntityId, Label, NodeId, Property, PropertyValue};
use qilbee_graph::Database;
use qilbee_memory::{AgentMemory, Episode, EpisodeContent, EpisodeType};
use qilbee_protocol::http::HealthResponse;
use std::collections::HashMap as StdHashMap;
use std::sync::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub database: Arc<Database>,
    pub start_time: Instant,
    pub agent_memories: Arc<Mutex<StdHashMap<String, Arc<AgentMemory>>>>,
}

/// Create HTTP server router
pub fn create_router(database: Arc<Database>) -> Router {
    let state = AppState {
        database,
        start_time: Instant::now(),
        agent_memories: Arc::new(Mutex::new(StdHashMap::new())),
    };

    Router::new()
        // Health check
        .route("/health", get(health_check))
        // Graph management
        .route("/graphs/:name", post(create_graph))
        .route("/graphs/:name", delete(delete_graph))
        // Node operations
        .route("/graphs/:name/nodes", post(create_node))
        .route("/graphs/:name/nodes", get(find_nodes))
        .route("/graphs/:name/nodes/:id", get(get_node))
        .route("/graphs/:name/nodes/:id", put(update_node))
        .route("/graphs/:name/nodes/:id", delete(delete_node))
        // Relationship operations
        .route("/graphs/:name/relationships", post(create_relationship))
        .route(
            "/graphs/:name/nodes/:id/relationships",
            get(get_relationships),
        )
        // Query execution
        .route("/graphs/:name/query", post(execute_query))
        // Memory operations
        .route("/memory/:agent_id/episodes", post(store_episode))
        .route("/memory/:agent_id/episodes/:id", get(get_episode))
        .route("/memory/:agent_id/episodes/recent", get(get_recent_episodes))
        .route("/memory/:agent_id/episodes/search", post(search_episodes))
        .route("/memory/:agent_id/statistics", get(get_memory_statistics))
        .route("/memory/:agent_id/consolidate", post(consolidate_memory))
        .route("/memory/:agent_id/forget", post(forget_memory))
        .route("/memory/:agent_id", delete(clear_memory))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
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
    let primary_content = request
        .content
        .get("primary")
        .or(request.content.get("input"))
        .or(request.content.get("message"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let mut content = EpisodeContent::new(primary_content);

    if let Some(secondary) = request.content.get("secondary").or(request.content.get("output")).or(request.content.get("response")) {
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

async fn get_episode(
    State(state): State<AppState>,
    Path((agent_id, episode_id)): Path<(String, String)>,
) -> impl IntoResponse {
    // Get agent memory
    let memory = {
        let memories = state.agent_memories.lock().unwrap();
        match memories.get(&agent_id) {
            Some(m) => m.clone(),
            None => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(json!({"error": "Agent memory not found"})),
                );
            }
        }
    };

    // Parse episode ID - just compare as string for now
    // TODO: Implement proper UUID parsing for EpisodeId
    // For now, get recent episodes and find by string comparison
    match memory.get_recent_episodes(100) {
        Ok(episodes) => {
            let found = episodes.iter().find(|ep| ep.id.to_string() == episode_id);
            match found {
                Some(episode) => {
                    let mut content_map = HashMap::new();
                    content_map.insert("primary".to_string(), json!(episode.content.primary));
                    if let Some(secondary) = &episode.content.secondary {
                        content_map.insert("secondary".to_string(), json!(secondary));
                    }

                    let response = json!({
                        "agentId": episode.agent_id,
                        "episodeType": format!("{:?}", episode.episode_type),
                        "content": content_map,
                        "eventTime": episode.event_time.as_millis()
                    });
                    (StatusCode::OK, Json(response))
                }
                None => (
                    StatusCode::NOT_FOUND,
                    Json(json!({"error": "Episode not found"})),
                ),
            }
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        ),
    }
}

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
                        "episodeType": format!("{:?}", ep.episode_type),
                        "content": ep.content,
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

async fn search_episodes(
    State(_state): State<AppState>,
    Path(_agent_id): Path<String>,
    Json(_request): Json<Value>,
) -> impl IntoResponse {
    // TODO: Implement episode search
    (StatusCode::OK, Json(json!({"episodes": []})))
}

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

async fn consolidate_memory(
    State(_state): State<AppState>,
    Path(_agent_id): Path<String>,
    Json(_request): Json<Value>,
) -> impl IntoResponse {
    // TODO: Implement consolidation
    (StatusCode::OK, Json(json!({"consolidated": 0})))
}

async fn forget_memory(
    State(_state): State<AppState>,
    Path(_agent_id): Path<String>,
    Json(_request): Json<Value>,
) -> impl IntoResponse {
    // TODO: Implement forgetting
    (StatusCode::OK, Json(json!({"forgotten": 0})))
}

async fn clear_memory(
    State(_state): State<AppState>,
    Path(_agent_id): Path<String>,
) -> impl IntoResponse {
    // TODO: Implement memory clearing
    (StatusCode::OK, Json(json!({"cleared": true})))
}

// ==================== Helper Functions ====================

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
