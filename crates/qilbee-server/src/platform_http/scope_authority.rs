use super::*;
use crate::security::identity::ScopeAuthority;

pub(super) fn routes() -> Router<PlatformState> {
    Router::new().route("/api/v1/credentials/:id/scope-authority", post(update))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateRequest {
    contract_version: u32,
    expected_revision: u64,
    authority: ScopeAuthority,
}

async fn update(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Result<Json<UpdateRequest>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    state
        .run(headers, move |identity, token, principal| {
            require_admin(&principal)?;
            let request = json_body(body)?;
            version(request.contract_version)?;
            if request.expected_revision == 0 {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "Expected revision must be positive",
                ));
            }
            let credential = identity
                .set_scope_authority(
                    token,
                    identifier(&id)?,
                    request.expected_revision,
                    request.authority,
                )
                .map_err(ApiError::operation)?;
            Ok(Json(json!({"contract_version":1,"credential":credential})))
        })
        .await
}
