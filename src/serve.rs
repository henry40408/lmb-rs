use crate::StoreOptions;
use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, Method, StatusCode},
    response::IntoResponse,
    routing::any,
    Router,
};
use http::{HeaderName, HeaderValue};
use lmb::{EvaluationBuilder, LmbState, LmbStateKey, LmbStore};
use serde_json::{Map, Value};
use std::{
    collections::HashMap, fmt::Display, io::Cursor, str::FromStr as _, sync::Arc, time::Duration,
};
use tokio::net::ToSocketAddrs;
use tower_http::trace::{self, TraceLayer};
use tracing::{error, info, warn, Level};

#[derive(Clone)]
struct AppState {
    json: bool,
    name: String,
    script: String,
    store: LmbStore,
    timeout: Option<Duration>,
}

#[derive(Default)]
pub struct ServeOptions<S, T>
where
    S: AsRef<str>,
    T: Display + ToSocketAddrs,
{
    pub json: bool,
    pub bind: T,
    pub name: S,
    pub script: S,
    pub timeout: Option<Duration>,
    pub store_options: StoreOptions,
}

fn do_handle_request<S>(
    state: AppState,
    method: Method,
    path: S,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse
where
    S: AsRef<str>,
{
    let e = EvaluationBuilder::new(state.script, Cursor::new(body))
        .name(state.name)
        .timeout(state.timeout)
        .store(state.store.clone())
        .build();

    let mut headers_map: Map<_, Value> = Map::new();
    for (name, value) in headers {
        if let Some(name) = name {
            let value = value.to_str().unwrap_or("");
            headers_map.insert(name.to_string(), value.into());
        }
    }

    let mut request_map: Map<_, Value> = Map::new();
    request_map.insert("method".into(), method.as_str().into());
    request_map.insert("path".into(), path.as_ref().into());
    request_map.insert("headers".into(), headers_map.into());

    let eval_state = Arc::new(LmbState::new());
    eval_state.insert(LmbStateKey::Request, request_map.into());

    let res = e.evaluate_with_state(eval_state.clone());
    match res {
        Ok(res) => match build_response(state.json, eval_state, res.payload) {
            Ok(t) => t,
            Err(err) => {
                error!(?err, "failed to build response");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    HeaderMap::new(),
                    String::new(),
                )
            }
        },
        Err(err) => {
            error!(%err, "failed to run Lua script");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                HeaderMap::new(),
                String::new(),
            )
        }
    }
}

fn build_response(
    json: bool,
    state: Arc<LmbState>,
    value: Value,
) -> anyhow::Result<(StatusCode, HeaderMap, String)> {
    let (status_code, headers) = state
        .view(&LmbStateKey::Response, |_k, res| {
            let status_code = res
                .get("status_code")
                .and_then(|s| s.as_u64())
                .unwrap_or(200u64);
            let mut m = HashMap::new();
            if let Some(h) = res.get("headers").and_then(|h| h.as_object()) {
                for (name, value) in h.iter() {
                    m.insert(
                        name.to_string(),
                        match value {
                            Value::String(s) => s.to_string(),
                            _ => value.to_string(),
                        },
                    );
                }
            }
            (status_code, m)
        })
        .unwrap_or_else(|| (200u64, HashMap::new()));

    let status_code = StatusCode::from_u16(u16::try_from(status_code)?)?;
    let mut header_map = HeaderMap::new();
    for (name, value) in headers.iter() {
        header_map.insert(HeaderName::from_str(name)?, HeaderValue::from_str(value)?);
    }
    let body = if json {
        serde_json::to_string(&value)?
    } else {
        match value {
            Value::String(s) => s,
            _ => value.to_string(),
        }
    };
    Ok((status_code, header_map, body))
}

async fn index_route(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    do_handle_request(state, method, "/", headers, body)
}

async fn match_all_route(
    State(state): State<AppState>,
    method: Method,
    Path(path): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let path = format!("/{path}");
    do_handle_request(state, method, path, headers, body)
}

pub fn init_route<S, T>(opts: &ServeOptions<S, T>) -> anyhow::Result<Router>
where
    S: AsRef<str>,
    T: Display + ToSocketAddrs,
{
    let store = if let Some(path) = &opts.store_options.store_path {
        let store = LmbStore::new(path.as_path())?;
        if opts.store_options.run_migrations {
            store.migrate(None)?;
        }
        info!(?path, "open store");
        store
    } else {
        let store = LmbStore::default();
        warn!("no store path is specified, an in-memory store will be used and values will be lost when process ends");
        store
    };
    let app_state = AppState {
        json: opts.json,
        name: opts.name.as_ref().to_string(),
        script: opts.script.as_ref().to_string(),
        store,
        timeout: opts.timeout,
    };
    let app = Router::new()
        .route("/", any(index_route))
        .route("/*path", any(match_all_route))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(trace::DefaultMakeSpan::new().level(Level::INFO))
                .on_response(trace::DefaultOnResponse::new().level(Level::INFO)),
        )
        .with_state(app_state);
    Ok(app)
}

pub async fn serve_file<'a, S, T>(opts: &ServeOptions<S, T>) -> anyhow::Result<()>
where
    S: AsRef<str>,
    T: Display + ToSocketAddrs,
{
    let bind = &opts.bind;
    let app = init_route(opts)?;
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    info!(%bind, "serving lua script");
    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::init_route;
    use crate::{serve::ServeOptions, Cli, StoreOptions};
    use axum_test::TestServer;
    use clap::Parser;
    use http::HeaderValue;
    use serde_json::{json, Value};

    #[tokio::test]
    async fn echo_request() {
        let cli = Cli::parse_from(["lmb", "--json", "serve", "--file", "-"]);
        let script = r#"
        local m = require('@lmb')
        return { request = m.request, body = io.read('*a') }
        "#;
        let store_options = StoreOptions {
            store_path: None,
            run_migrations: true,
        };
        let opts = ServeOptions {
            bind: "",
            json: cli.json,
            script,
            store_options,
            ..Default::default()
        };
        let router = init_route(&opts).unwrap();
        let server = TestServer::new(router.into_make_service()).unwrap();
        let res = server.post("/foo/bar/baz").json(&json!({"a":1})).await;
        assert_eq!(200, res.status_code());

        let value: Value = serde_json::from_str(&res.text()).unwrap();
        let expected = json!({
            "body": r#"{"a":1}"#,
            "request": {
                "headers": {
                    "content-type": "application/json",
                },
                "method": "POST",
                "path": "/foo/bar/baz",
            },
        });
        assert_eq!(expected, value);
    }

    #[tokio::test]
    async fn headers_status_code() {
        let cli = Cli::parse_from(["lmb", "serve", "--file", "-"]);
        let script = r#"
        local m = require('@lmb')
        print(m.response)
        local res = {}
        res.status_code = 418 -- I'm a teapot
        res.headers = { quantity = 1, whoami = "a teapot" }
        m.response = res
        print(m.response)
        return "I'm a teapot."
        "#;
        let store_options = StoreOptions {
            store_path: None,
            run_migrations: true,
        };
        let opts = ServeOptions {
            bind: "",
            json: cli.json,
            script,
            store_options,
            ..Default::default()
        };
        let router = init_route(&opts).unwrap();
        let server = TestServer::new(router.into_make_service()).unwrap();
        let res = server.post("/").await;
        assert_eq!(418, res.status_code());
        assert_eq!(
            HeaderValue::from_static("a teapot"),
            res.headers().get("whoami").unwrap()
        );
        assert_eq!("I'm a teapot.", res.text());
    }

    #[tokio::test]
    async fn headers_status_code_bad_script() {
        let cli = Cli::parse_from(["lmb", "serve", "--file", "-"]);
        let script = "ret 'hello'";
        let store_options = StoreOptions {
            store_path: None,
            run_migrations: true,
        };
        let opts = ServeOptions {
            bind: "",
            json: cli.json,
            script,
            store_options,
            ..Default::default()
        };
        let router = init_route(&opts).unwrap();
        let server = TestServer::new(router.into_make_service()).unwrap();
        let res = server.post("/").await;
        assert_eq!(500, res.status_code());
        assert_eq!("", res.text());
    }

    #[tokio::test]
    async fn headers_status_code_invalid_status_code() {
        let cli = Cli::parse_from(["lmb", "serve", "--file", "-"]);
        let script = r#"
        local m = require('@lmb')
        local res = {}
        res.status_code = 10000
        m.response = res
        return "hello"
        "#;
        let store_options = StoreOptions {
            store_path: None,
            run_migrations: true,
        };
        let opts = ServeOptions {
            bind: "",
            json: cli.json,
            script,
            store_options,
            ..Default::default()
        };
        let router = init_route(&opts).unwrap();
        let server = TestServer::new(router.into_make_service()).unwrap();
        let res = server.post("/").await;
        assert_eq!(500, res.status_code());
        assert_eq!("", res.text());
    }

    #[tokio::test]
    async fn json_string() {
        let cli = Cli::parse_from(["lmb", "--json", "serve", "--file", "-"]);
        let script = "return 'hello'";
        let store_options = StoreOptions {
            store_path: None,
            run_migrations: true,
        };
        let opts = ServeOptions {
            bind: "",
            json: cli.json,
            script,
            store_options,
            ..Default::default()
        };
        let router = init_route(&opts).unwrap();
        let server = TestServer::new(router.into_make_service()).unwrap();
        let res = server.post("/").await;
        assert_eq!(200, res.status_code());
        assert_eq!(r#""hello""#, res.text());
    }

    #[tokio::test]
    async fn number() {
        let cli = Cli::parse_from(["lmb", "serve", "--file", "-"]);
        let script = r#"return 1"#;
        let store_options = StoreOptions {
            store_path: None,
            run_migrations: true,
        };
        let opts = ServeOptions {
            bind: "",
            json: cli.json,
            script,
            store_options,
            ..Default::default()
        };
        let router = init_route(&opts).unwrap();
        let server = TestServer::new(router.into_make_service()).unwrap();
        let res = server.post("/").await;
        assert_eq!(200, res.status_code());
        assert_eq!("1", res.text());
    }

    #[tokio::test]
    async fn raw_string() {
        let cli = Cli::parse_from(["lmb", "serve", "--file", "-"]);
        let script = "return 'hello'";
        let store_options = StoreOptions {
            store_path: None,
            run_migrations: true,
        };
        let opts = ServeOptions {
            bind: "",
            json: cli.json,
            script,
            store_options,
            ..Default::default()
        };
        let router = init_route(&opts).unwrap();
        let server = TestServer::new(router.into_make_service()).unwrap();
        let res = server.post("/").await;
        assert_eq!(200, res.status_code());
        assert_eq!("hello", res.text());
    }

    #[tokio::test]
    async fn serve() {
        let cli = Cli::parse_from(["lmb", "--json", "serve", "--file", "-"]);
        let script = "return 1";
        let store_options = StoreOptions {
            store_path: None,
            run_migrations: true,
        };
        let opts = ServeOptions {
            bind: "",
            json: cli.json,
            script,
            store_options,
            ..Default::default()
        };
        let router = init_route(&opts).unwrap();
        let server = TestServer::new(router.into_make_service()).unwrap();
        let res = server.post("/").await;
        assert_eq!(200, res.status_code());
        assert_eq!("1", res.text());
    }
}
