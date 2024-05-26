use crate::StoreOptions;
use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, Method, StatusCode},
    response::IntoResponse,
    routing::any,
    Router,
};
use lam::*;
use std::{collections::HashMap, fmt::Display, io::Cursor, time::Duration};
use tokio::net::ToSocketAddrs;
use tower_http::trace::{self, TraceLayer};
use tracing::{error, info, warn, Level};

#[derive(Clone)]
struct AppState {
    json: bool,
    name: String,
    script: String,
    store: LamStore,
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
        .with_name(state.name)
        .with_timeout(state.timeout)
        .with_store(state.store.clone())
        .build();

    let mut headers_map = HashMap::new();
    for (name, value) in headers {
        if let Some(name) = name {
            let value = value.to_str().unwrap_or("");
            headers_map.insert(name.to_string(), value.into());
        }
    }

    let mut request_map = HashMap::new();
    request_map.insert("method", method.as_str().into());
    request_map.insert("path", path.as_ref().into());
    request_map.insert("headers", headers_map.into());

    let eval_state = LamState::new();
    eval_state.insert(LamStateKey::Request, request_map.into());

    let res = e.evaluate_with_state(eval_state);
    match res {
        Ok(res) => {
            if state.json {
                let Ok(serialized) = serde_json::to_string(&res.payload) else {
                    return (StatusCode::INTERNAL_SERVER_ERROR, "".to_string());
                };
                (StatusCode::OK, serialized)
            } else {
                (StatusCode::OK, res.payload.to_string())
            }
        }
        Err(err) => {
            error!(%err, "failed to run Lua script");
            (StatusCode::BAD_REQUEST, "".to_string())
        }
    }
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
        let store = LamStore::new(path.as_path())?;
        if opts.store_options.run_migrations {
            store.migrate(None)?;
        }
        info!(?path, "open store");
        store
    } else {
        let store = LamStore::default();
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

#[cfg(not(tarpaulin_include))]
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
    use serde_json::{json, Value};

    #[tokio::test]
    async fn serve() {
        let cli = Cli::parse_from(["lam", "--json", "serve", "--file", "a.lua"]);
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

    #[tokio::test]
    async fn echo_request() {
        let cli = Cli::parse_from(["lam", "--json", "serve", "--file", "a.lua"]);
        let script = r#"
        local m = require('@lam')
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
}
