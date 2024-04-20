use crate::StoreOptions;
use axum::{
    body::Bytes, extract::State, http::StatusCode, response::IntoResponse, routing::post, Router,
};
use lam::*;
use std::{io::Cursor, time::Duration};
use tower_http::trace::{self, TraceLayer};
use tracing::{error, info, warn, Level};

#[derive(Clone)]
struct AppState {
    name: String,
    script: String,
    store: LamStore,
    timeout: Option<Duration>,
}

async fn index_route(State(state): State<AppState>, body: Bytes) -> impl IntoResponse {
    let e = EvalBuilder::new(Cursor::new(body), &state.script)
        .set_name(state.name)
        .set_timeout(state.timeout)
        .set_store(state.store.clone())
        .build();
    let res = e.evaluate();
    match res {
        Ok(res) => (StatusCode::OK, res.result.to_string()),
        Err(err) => {
            error!(%err, "failed to run Lua script");
            (StatusCode::BAD_REQUEST, "".to_string())
        }
    }
}

pub fn init_route<S>(
    name: S,
    script: S,
    timeout: Option<Duration>,
    store_options: &StoreOptions,
) -> anyhow::Result<Router>
where
    S: AsRef<str>,
{
    let store = if let Some(path) = &store_options.store_path {
        let store = LamStore::new(path.as_path())?;
        if store_options.run_migrations {
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
        name: name.as_ref().to_string(),
        script: script.as_ref().to_string(),
        store,
        timeout,
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

pub async fn serve_file<S, T>(
    bind: T,
    name: S,
    script: S,
    timeout: Option<Duration>,
    store_options: &StoreOptions,
) -> anyhow::Result<()>
where
    S: AsRef<str>,
    T: std::fmt::Display + tokio::net::ToSocketAddrs,
{
    let app = init_route(name, script, timeout, store_options)?;
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    info!(%bind, "serving lua script");
    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::StoreOptions;

    use super::init_route;
    use axum_test::TestServer;
    use std::time::Duration;

    #[tokio::test]
    async fn serve() {
        let name = "";
        let script = "return 1";
        let timeout = Duration::from_secs(1);
        let store_options = StoreOptions {
            store_path: None,
            run_migrations: true,
        };
        let router = init_route(name, script, Some(timeout), &store_options).unwrap();
        let server = TestServer::new(router.into_make_service()).unwrap();
        let res = server.post("/").await;
        assert_eq!(200, res.status_code());
        assert_eq!("1", res.text());
    }
}
