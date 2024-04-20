use axum::{
    body::Bytes, extract::State, http::StatusCode, response::IntoResponse, routing::post, Router,
};
use lam::*;
use std::{fs, io::Cursor, path::PathBuf};
use tower_http::trace::{self, TraceLayer};
use tracing::{error, info, warn, Level};

#[derive(Clone)]
struct AppState {
    script: String,
    store: LamStore,
    timeout: u64,
}

async fn index_route(State(state): State<AppState>, body: Bytes) -> impl IntoResponse {
    let e = EvalBuilder::new(Cursor::new(body), state.script.clone())
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

pub fn init_route(
    file: &PathBuf,
    timeout: u64,
    store_path: Option<&PathBuf>,
    run_migrations: bool,
) -> anyhow::Result<Router> {
    let script = fs::read_to_string(file)?;
    let store = if let Some(path) = store_path {
        let store = LamStore::new(path)?;
        if run_migrations {
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
        script,
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

pub async fn serve_file(
    file: &PathBuf,
    bind: &str,
    timeout: u64,
    store_path: Option<&PathBuf>,
    run_migrations: bool,
) -> anyhow::Result<()> {
    let app = init_route(file, timeout, store_path, run_migrations)?;
    let listener = tokio::net::TcpListener::bind(bind).await?;
    info!(%bind, "serving lua script");
    axum::serve(listener, app).await?;
    Ok(())
}
