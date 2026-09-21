//! Installation administration requires a distinct, locally bootstrapped master.
use super::*;
use crate::security::identity::GlobalCredentialSpec;

pub(super) fn routes() -> Router<PlatformState> {
    Router::new()
        .route("/api/v1/admin/identity", get(identity))
        .route("/api/v1/admin/credentials", post(issue_global))
        .route("/api/v1/admin/credentials/:id", get(inspect_global))
        .route("/api/v1/admin/credentials/:id/rotate", post(rotate))
        .route("/api/v1/admin/credentials/:id/revoke", post(revoke))
        .route("/api/v1/admin/tenants", post(register))
        .route("/api/v1/admin/tenants/:tenant", get(inspect))
        .route(
            "/api/v1/admin/tenants/:tenant/admin-credentials",
            post(issue_admin),
        )
}

pub(super) async fn run<T, F>(
    state: PlatformState,
    headers: HeaderMap,
    operation: F,
) -> ApiResult<T>
where
    T: Send + 'static,
    F: FnOnce(&IdentityStore, &str) -> ApiResult<T> + Send + 'static,
{
    let token = bearer(&headers)?;
    tokio::task::spawn_blocking(move || {
        if token.starts_with("qdb1_") || token.starts_with("qdbst1_") {
            state
                .identity
                .authenticate(&token)
                .map_err(ApiError::authentication)?;
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                "forbidden",
                "Global administration authority is required",
            ));
        }
        state
            .identity
            .authenticate_global(&token)
            .map_err(ApiError::authentication)?;
        operation(&state.identity, &token)
    })
    .await
    .map_err(|_| ApiError::internal())?
}

fn operation_error(error: Error) -> ApiError {
    match error {
        Error::KeyNotFound(_) => ApiError::new(
            StatusCode::NOT_FOUND,
            "tenant_not_found",
            "The tenant does not exist",
        ),
        error => ApiError::operation(error),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RegisterRequest {
    contract_version: u32,
    tenant_id: String,
    display_name: Option<String>,
    subject_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AdminRequest {
    contract_version: u32,
    subject_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GlobalIssueRequest {
    contract_version: u32,
    spec: GlobalCredentialSpec,
}

async fn identity(
    State(state): State<PlatformState>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    run(state, headers, |identity, token| {
        let credential = identity
            .authenticate_global(token)
            .map_err(ApiError::authentication)?;
        Ok(Json(json!({"contract_version":1,"credential":credential})))
    })
    .await
}

async fn register(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<RegisterRequest>, JsonRejection>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    run(state, headers, move |identity, token| {
        let request = json_body(body)?;
        version(request.contract_version)?;
        let (tenant, issued) = identity.register_tenant_named(token, &request.tenant_id, &request.subject_id, request.display_name.as_deref()).map_err(operation_error)?;
        Ok((StatusCode::CREATED, Json(json!({"contract_version":1,"tenant":tenant,"credential":issued.credential,"secret":issued.secret}))))
    }).await
}

async fn inspect(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    Path(tenant): Path<String>,
) -> ApiResult<Json<Value>> {
    run(state, headers, move |identity, token| {
        let registration = identity
            .inspect_tenant(token, &tenant)
            .map_err(operation_error)?;
        Ok(Json(
            json!({"contract_version":1,"tenant_id":tenant,"registration":registration}),
        ))
    })
    .await
}

async fn issue_admin(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    Path(tenant): Path<String>,
    body: Result<Json<AdminRequest>, JsonRejection>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    run(state, headers, move |identity, token| {
        let request = json_body(body)?;
        version(request.contract_version)?;
        let issued = identity
            .issue_tenant_admin(token, &tenant, &request.subject_id)
            .map_err(operation_error)?;
        Ok((
            StatusCode::CREATED,
            Json(
                json!({"contract_version":1,"credential":issued.credential,"secret":issued.secret}),
            ),
        ))
    })
    .await
}

async fn issue_global(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<GlobalIssueRequest>, JsonRejection>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    run(state, headers, move |identity, token| {
        let request = json_body(body)?;
        version(request.contract_version)?;
        let issued = identity
            .issue_global(token, request.spec)
            .map_err(operation_error)?;
        Ok((
            StatusCode::CREATED,
            Json(
                json!({"contract_version":1,"credential":issued.credential,"secret":issued.secret}),
            ),
        ))
    })
    .await
}

async fn inspect_global(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    run(state, headers, move |identity, token| {
        let credential = identity
            .inspect_global(token, identifier(&id)?)
            .map_err(operation_error)?;
        Ok(Json(json!({"contract_version":1,"credential":credential})))
    })
    .await
}

async fn rotate(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Result<Json<ChangeRequest>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    run(state, headers, move |identity, token| {
        let request = json_body(body)?;
        version(request.contract_version)?;
        let issued = identity
            .rotate_global(token, identifier(&id)?, request.expected_revision)
            .map_err(operation_error)?;
        Ok(Json(
            json!({"contract_version":1,"credential":issued.credential,"secret":issued.secret}),
        ))
    })
    .await
}

async fn revoke(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Result<Json<ChangeRequest>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    run(state, headers, move |identity, token| {
        let request = json_body(body)?;
        version(request.contract_version)?;
        let credential = identity
            .revoke_global(token, identifier(&id)?, request.expected_revision)
            .map_err(operation_error)?;
        Ok(Json(json!({"contract_version":1,"credential":credential})))
    })
    .await
}
