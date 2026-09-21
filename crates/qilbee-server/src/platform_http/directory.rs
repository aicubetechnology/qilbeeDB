//! Administrative enumeration derives each directory prefix from server authority.
use super::*;
use axum::extract::{Query, rejection::QueryRejection};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PageQuery {
    contract_version: u32,
    #[serde(default = "default_limit")]
    limit: usize,
    cursor: Option<String>,
}
fn default_limit() -> usize {
    25
}

pub(super) fn routes() -> Router<PlatformState> {
    Router::new()
        .route("/api/v1/admin/tenants", get(tenants))
        .route("/api/v1/admin/credentials", get(global_credentials))
        .route("/api/v1/credentials", get(tenant_credentials))
        .route("/api/v1/admin/login-accounts", get(global_accounts))
        .route("/api/v1/login-accounts", get(tenant_accounts))
}

fn query(body: Result<Query<PageQuery>, QueryRejection>) -> ApiResult<PageQuery> {
    let Query(query) = body.map_err(|_| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Invalid directory page parameters",
        )
    })?;
    version(query.contract_version)?;
    if !(1..=100).contains(&query.limit) || query.cursor.as_ref().is_some_and(|c| c.len() > 8192) {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Directory limit must be 1..100 and cursor at most 8192 bytes",
        ));
    }
    Ok(query)
}
async fn list(
    state: PlatformState,
    headers: HeaderMap,
    body: Result<Query<PageQuery>, QueryRejection>,
    kind: &'static str,
) -> ApiResult<Json<Value>> {
    let operation = move |identity: &IdentityStore, token: &str| {
        let q = query(body)?;
        let cursor = q.cursor.as_deref();
        let page = match kind {
            "tenants" => json!(
                identity
                    .list_tenants(token, cursor, q.limit)
                    .map_err(ApiError::operation)?
            ),
            "global_credentials" => json!(
                identity
                    .list_global_credentials(token, cursor, q.limit)
                    .map_err(ApiError::operation)?
            ),
            "tenant_credentials" => json!(
                identity
                    .list_tenant_credentials(token, cursor, q.limit)
                    .map_err(ApiError::operation)?
            ),
            "global_accounts" => json!(
                identity
                    .list_login_accounts(token, true, cursor, q.limit)
                    .map_err(ApiError::operation)?
            ),
            _ => json!(
                identity
                    .list_login_accounts(token, false, cursor, q.limit)
                    .map_err(ApiError::operation)?
            ),
        };
        Ok(Json(json!({"contract_version":1,"page":page})))
    };
    if kind.starts_with("tenant_") {
        state
            .run(headers, move |identity, token, _| {
                operation(identity, token)
            })
            .await
    } else {
        administration::run(state, headers, operation).await
    }
}
macro_rules! handlers {
    ($($name:ident => $kind:literal),*) => { $(
        async fn $name(State(state): State<PlatformState>, headers: HeaderMap, query: Result<Query<PageQuery>, QueryRejection>) -> ApiResult<Json<Value>> {
            list(state, headers, query, $kind).await
        }
    )* };
}
handlers!(tenants => "tenants", global_credentials => "global_credentials", tenant_credentials => "tenant_credentials", global_accounts => "global_accounts", tenant_accounts => "tenant_accounts");

pub(super) fn cors(
    origin: Option<&str>,
) -> qilbee_core::Result<Option<tower_http::cors::CorsLayer>> {
    let Some(origin) = origin else {
        return Ok(None);
    };
    let uri: axum::http::Uri = origin.parse().map_err(|_| {
        Error::Configuration("Administrative origin must be an exact HTTPS origin".into())
    })?;
    if uri.scheme_str() != Some("https")
        || uri.authority().is_none()
        || uri.authority().is_some_and(|a| a.as_str().contains('@'))
        || origin
            != format!(
                "https://{}",
                uri.authority().map(|a| a.as_str()).unwrap_or("")
            )
    {
        return Err(Error::Configuration("Administrative origin must be an exact HTTPS origin without path, query or credentials".into()));
    }
    let value = HeaderValue::from_str(origin)
        .map_err(|_| Error::Configuration("Invalid administrative origin".into()))?;
    Ok(Some(
        tower_http::cors::CorsLayer::new()
            .allow_origin([value])
            .allow_methods([axum::http::Method::GET, axum::http::Method::POST])
            .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
            .max_age(std::time::Duration::from_secs(300)),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use tower::Service;

    #[tokio::test]
    async fn cors_allows_only_the_exact_https_origin_and_never_cookie_credentials() {
        for bad in [
            "*",
            "http://admin.example.test",
            "https://admin.example.test/",
            "https://admin.example.test?q=1",
            "https://user@admin.example.test",
        ] {
            assert!(cors(Some(bad)).is_err());
        }
        assert!(cors(None).unwrap().is_none());
        let router = Router::new()
            .route("/api/v1/login", post(|| async { "login" }))
            .layer(cors(Some("https://admin.example.test")).unwrap().unwrap());
        for (origin, allowed) in [
            ("https://admin.example.test", true),
            ("https://attacker.example.test", false),
        ] {
            let response = router
                .clone()
                .call(
                    Request::builder()
                        .method("OPTIONS")
                        .uri("/api/v1/login")
                        .header("origin", origin)
                        .header("access-control-request-method", "POST")
                        .header(
                            "access-control-request-headers",
                            "authorization,content-type",
                        )
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                response
                    .headers()
                    .get("access-control-allow-origin")
                    .is_some(),
                allowed
            );
            assert!(
                response
                    .headers()
                    .get("access-control-allow-credentials")
                    .is_none()
            );
        }
    }
}
