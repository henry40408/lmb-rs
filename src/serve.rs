use crate::StoreOptions;
use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
    Router,
};
use lam::*;
use std::{collections::HashMap, io::Cursor, time::Duration};
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
    T: std::fmt::Display + tokio::net::ToSocketAddrs,
{
    pub json: bool,
    pub bind: T,
    pub name: S,
    pub script: S,
    pub timeout: Option<Duration>,
    pub store_options: StoreOptions,
}

async fn index_route(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let e = EvalBuilder::new(state.script.into(), Cursor::new(body))
        .with_name(state.name.into())
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
    request_map.insert("method".into(), "POST".into());
    request_map.insert("headers".into(), headers_map.into());

    let eval_state = LamState::new();
    eval_state.insert(LamStateKey::Request, request_map.into());

    let res = e.evaluate_with_state(eval_state);
    match res {
        Ok(res) => {
            if state.json {
                let Ok(serialized) = serde_json::to_string(&res.result) else {
                    return (StatusCode::INTERNAL_SERVER_ERROR, "".to_string());
                };
                (StatusCode::OK, serialized)
            } else {
                (StatusCode::OK, res.result.to_string())
            }
        }
        Err(err) => {
            error!(%err, "failed to run Lua script");
            (StatusCode::BAD_REQUEST, "".to_string())
        }
    }
}

pub fn init_route<S, T>(opts: &ServeOptions<S, T>) -> anyhow::Result<Router>
where
    S: AsRef<str>,
    T: std::fmt::Display + tokio::net::ToSocketAddrs,
{
    let store = if let Some(path) = &opts.store_options.store_path {
        let store = LamStore::new(path.as_path())?;
        if opts.store_options.run_migrations {
            store.migrate()?;
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
        .route("/", post(index_route))
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
    T: std::fmt::Display + tokio::net::ToSocketAddrs,
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
    use crate::{serve::ServeOptions, Cli, StoreOptions};

    use super::init_route;
    use axum_test::TestServer;
    use clap::Parser;

    #[tokio::test]
    async fn serve() {
        let cli = Cli::parse_from(["--json", "serve", "--file", "a.lua"]);
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
