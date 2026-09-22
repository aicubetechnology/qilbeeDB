//! Human login is explicitly provisioned and never an anonymous signup operation.
use super::*;
use crate::security::identity::LoginAuthority;
use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

#[derive(Clone)]
pub(super) struct LoginLimits {
    work: Arc<Semaphore>,
    attempts: Arc<Mutex<Attempts>>,
}
struct Attempts {
    started: Instant,
    total: u32,
    names: HashMap<String, u32>,
}
impl Default for LoginLimits {
    fn default() -> Self {
        Self {
            work: Arc::new(Semaphore::new(2)),
            attempts: Arc::new(Mutex::new(Attempts {
                started: Instant::now(),
                total: 0,
                names: HashMap::new(),
            })),
        }
    }
}
impl LoginLimits {
    fn work(&self) -> ApiResult<OwnedSemaphorePermit> {
        self.work.clone().try_acquire_owned().map_err(|_| {
            ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "login_busy",
                "Password processing capacity is occupied; retry later",
            )
        })
    }
    fn attempt(&self, authority: &LoginAuthority, username: &str) -> ApiResult<()> {
        let key =
            serde_json::to_string(&(authority, username)).map_err(|_| ApiError::internal())?;
        let mut attempts = self.attempts.lock().map_err(|_| ApiError::internal())?;
        if attempts.started.elapsed() >= Duration::from_secs(60) {
            attempts.started = Instant::now();
            attempts.total = 0;
            attempts.names.clear();
        }
        if attempts.total >= 120 || attempts.names.get(&key).copied().unwrap_or(0) >= 8 {
            return Err(ApiError::new(
                StatusCode::TOO_MANY_REQUESTS,
                "login_rate_limited",
                "Login attempt limit reached; retry after one minute",
            ));
        }
        attempts.total += 1;
        *attempts.names.entry(key).or_default() += 1;
        Ok(())
    }
}

pub(super) fn routes() -> Router<PlatformState> {
    Router::new()
        .route("/api/v1/login", post(tenant_login))
        .route("/api/v1/admin/login", post(global_login))
        .route("/api/v1/logout", post(logout))
        .route("/api/v1/login-accounts", post(create_tenant))
        .route("/api/v1/admin/login-accounts", post(create_global))
        .route("/api/v1/login-accounts/:id", get(inspect_tenant))
        .route("/api/v1/admin/login-accounts/:id", get(inspect_global))
        .route("/api/v1/login-accounts/:id/password", post(password_tenant))
        .route(
            "/api/v1/admin/login-accounts/:id/password",
            post(password_global),
        )
        .route("/api/v1/login-accounts/:id/disable", post(disable_tenant))
        .route(
            "/api/v1/admin/login-accounts/:id/disable",
            post(disable_global),
        )
        .layer(DefaultBodyLimit::max(8 * 1024))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TenantLogin {
    contract_version: u32,
    tenant_id: String,
    username: String,
    password: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GlobalLogin {
    contract_version: u32,
    username: String,
    password: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateAccount {
    contract_version: u32,
    username: String,
    password: String,
    credential_id: Uuid,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PasswordChange {
    contract_version: u32,
    expected_revision: u64,
    password: String,
}

async fn login(
    state: PlatformState,
    authority: LoginAuthority,
    username: String,
    password: String,
) -> ApiResult<Json<Value>> {
    if username.len() > 256
        || password.len() > 1024
        || matches!(&authority, LoginAuthority::Tenant { tenant_id } if tenant_id.len() > 256)
    {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Login fields exceed their size limits",
        ));
    }
    state.login_limits.attempt(&authority, &username)?;
    let permit = state.login_limits.work()?;
    crate::http_work::spawn_blocking(move || {
        let _permit = permit;
        let session = state
            .identity
            .login(authority, &username, &password)
            .map_err(|error| match error {
                Error::Unauthorized(_) => ApiError::new(
                    StatusCode::UNAUTHORIZED,
                    "invalid_login",
                    "The login is invalid or unavailable",
                ),
                error => ApiError::operation(error),
            })?;
        Ok(Json(json!({"contract_version":1,"session":session})))
    })
    .await
    .map_err(|_| ApiError::internal())?
}
async fn tenant_login(
    State(state): State<PlatformState>,
    body: Result<Json<TenantLogin>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let body = json_body(body)?;
    version(body.contract_version)?;
    login(
        state,
        LoginAuthority::Tenant {
            tenant_id: body.tenant_id,
        },
        body.username,
        body.password,
    )
    .await
}
async fn global_login(
    State(state): State<PlatformState>,
    body: Result<Json<GlobalLogin>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let body = json_body(body)?;
    version(body.contract_version)?;
    login(state, LoginAuthority::Global, body.username, body.password).await
}
async fn logout(State(state): State<PlatformState>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    let token = bearer(&headers)?;
    crate::http_work::spawn_blocking(move || {
        state
            .identity
            .logout(&token)
            .map_err(ApiError::authentication)?;
        Ok(Json(json!({"contract_version":1,"logged_out":true})))
    })
    .await
    .map_err(|_| ApiError::internal())?
}

async fn account_run<T, F>(
    state: PlatformState,
    headers: HeaderMap,
    global: bool,
    operation: F,
) -> ApiResult<T>
where
    T: Send + 'static,
    F: FnOnce(&IdentityStore, &str) -> ApiResult<T> + Send + 'static,
{
    if global {
        administration::run(state, headers, operation).await
    } else {
        state
            .run(headers, move |identity, token, _| {
                operation(identity, token)
            })
            .await
    }
}
async fn create(
    state: PlatformState,
    headers: HeaderMap,
    body: Result<Json<CreateAccount>, JsonRejection>,
    global: bool,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let permit = state.login_limits.work()?;
    account_run(state, headers, global, move |identity, token| {
        let _permit = permit;
        let body = json_body(body)?;
        version(body.contract_version)?;
        let account = identity
            .create_login_account(
                token,
                global,
                &body.username,
                &body.password,
                body.credential_id,
            )
            .map_err(ApiError::operation)?;
        Ok((
            StatusCode::CREATED,
            Json(json!({"contract_version":1,"account":account})),
        ))
    })
    .await
}
async fn inspect(
    state: PlatformState,
    headers: HeaderMap,
    id: String,
    global: bool,
) -> ApiResult<Json<Value>> {
    account_run(state, headers, global, move |identity, token| {
        let account = identity
            .inspect_login_account(token, global, identifier(&id)?)
            .map_err(ApiError::operation)?;
        Ok(Json(json!({"contract_version":1,"account":account})))
    })
    .await
}
async fn password(
    state: PlatformState,
    headers: HeaderMap,
    id: String,
    body: Result<Json<PasswordChange>, JsonRejection>,
    global: bool,
) -> ApiResult<Json<Value>> {
    let permit = state.login_limits.work()?;
    account_run(state, headers, global, move |identity, token| {
        let _permit = permit;
        let body = json_body(body)?;
        version(body.contract_version)?;
        let account = identity
            .change_login_account(
                token,
                global,
                identifier(&id)?,
                body.expected_revision,
                Some(&body.password),
            )
            .map_err(ApiError::operation)?;
        Ok(Json(json!({"contract_version":1,"account":account})))
    })
    .await
}
async fn disable(
    state: PlatformState,
    headers: HeaderMap,
    id: String,
    body: Result<Json<ChangeRequest>, JsonRejection>,
    global: bool,
) -> ApiResult<Json<Value>> {
    account_run(state, headers, global, move |identity, token| {
        let body = json_body(body)?;
        version(body.contract_version)?;
        let account = identity
            .change_login_account(
                token,
                global,
                identifier(&id)?,
                body.expected_revision,
                None,
            )
            .map_err(ApiError::operation)?;
        Ok(Json(json!({"contract_version":1,"account":account})))
    })
    .await
}

macro_rules! account_handlers {
    ($create:ident, $inspect:ident, $password:ident, $disable:ident, $global:expr) => {
        async fn $create(
            State(state): State<PlatformState>,
            headers: HeaderMap,
            body: Result<Json<CreateAccount>, JsonRejection>,
        ) -> ApiResult<(StatusCode, Json<Value>)> {
            create(state, headers, body, $global).await
        }
        async fn $inspect(
            State(state): State<PlatformState>,
            headers: HeaderMap,
            Path(id): Path<String>,
        ) -> ApiResult<Json<Value>> {
            inspect(state, headers, id, $global).await
        }
        async fn $password(
            State(state): State<PlatformState>,
            headers: HeaderMap,
            Path(id): Path<String>,
            body: Result<Json<PasswordChange>, JsonRejection>,
        ) -> ApiResult<Json<Value>> {
            password(state, headers, id, body, $global).await
        }
        async fn $disable(
            State(state): State<PlatformState>,
            headers: HeaderMap,
            Path(id): Path<String>,
            body: Result<Json<ChangeRequest>, JsonRejection>,
        ) -> ApiResult<Json<Value>> {
            disable(state, headers, id, body, $global).await
        }
    };
}
account_handlers!(
    create_tenant,
    inspect_tenant,
    password_tenant,
    disable_tenant,
    false
);
account_handlers!(
    create_global,
    inspect_global,
    password_global,
    disable_global,
    true
);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn login_admission_bounds_names_work_and_fixed_windows() {
        let limits = LoginLimits::default();
        for _ in 0..8 {
            limits.attempt(&LoginAuthority::Global, "same").unwrap();
        }
        assert_eq!(
            limits
                .attempt(&LoginAuthority::Global, "same")
                .unwrap_err()
                .status,
            StatusCode::TOO_MANY_REQUESTS
        );
        for i in 0..112 {
            limits
                .attempt(&LoginAuthority::Global, &i.to_string())
                .unwrap();
        }
        assert!(limits.attempt(&LoginAuthority::Global, "another").is_err());
        assert!(limits.attempts.lock().unwrap().names.len() <= 120);
        limits.attempts.lock().unwrap().started = Instant::now() - Duration::from_secs(61);
        limits.attempt(&LoginAuthority::Global, "same").unwrap();
        let first = limits.work().unwrap();
        let _second = limits.work().unwrap();
        assert_eq!(
            limits.work().unwrap_err().status,
            StatusCode::SERVICE_UNAVAILABLE
        );
        drop(first);
        assert!(limits.work().is_ok());
    }
}
