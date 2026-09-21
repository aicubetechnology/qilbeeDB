//! Real HTTP graph reads must preserve scope, evidence revisions and the schema.
use super::administration::Client;
use super::*;
use uuid::Uuid;
const SCOPED: &str = "/api/v1/memory/graph";
const COMPANY: &str = "/api/v1/company/memory/graph";
mod qualification;
mod recovery;

struct Server {
    http: Client,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Server {
    async fn start(router: Router) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap();
        let api = client
            .get(format!("{base}/openapi.json"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        Self {
            http: Client { client, base, api },
            task,
        }
    }
}
fn scoped(ids: Value, visibility: &str) -> Value {
    json!({"contract_version":1,"scope":memory_scope(visibility),"query":{"root_record_ids":ids}})
}

#[tokio::test]
async fn scoped_memory_graph_real_http_reads_current_roots() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("company", "owner").unwrap();
    let key = memory_key(&identity, &admin.secret, "subject", true);
    let server = Server::start(router).await;
    let result = server
        .http
        .call(
            "POST",
            SCOPED,
            SCOPED,
            &key,
            scoped(json!([Uuid::new_v4()]), "shared"),
            200,
        )
        .await;
    assert_eq!(result["graph"]["roots"][0]["status"], "unavailable");
    assert_eq!(result["graph"]["nodes"], json!([]));
    assert_eq!(result["graph"]["coverage"]["complete"], true);
}

#[tokio::test]
async fn company_memory_graph_real_http_reads_a_canonical_workspace() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("company", "owner").unwrap();
    let key = memory_key(&identity, &admin.secret, "private-subject", true);
    let server = Server::start(router).await;
    let commands = "/api/v1/memory/commands";
    let receipt = server
        .http
        .call(
            "POST",
            commands,
            commands,
            &key,
            memory_create("root", "Private root", "private"),
            200,
        )
        .await;
    let route = "/api/v1/company/memory/workspaces";
    let workspaces = server
        .http
        .call(
            "GET",
            &format!("{route}?contract_version=1"),
            route,
            &admin.secret,
            Value::Null,
            200,
        )
        .await;
    let request = json!({"contract_version":1,"workspace_id":workspaces["page"]["workspaces"][0]["workspace_id"],"query":{"root_record_ids":[receipt["receipt"]["record_id"]]}});
    let result = server
        .http
        .call("POST", COMPANY, COMPANY, &admin.secret, request, 200)
        .await;
    assert_eq!(
        result["graph"]["nodes"][0]["record"]["payload"]["content"]["primary"],
        "Private root"
    );
}
