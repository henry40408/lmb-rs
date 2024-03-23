use axum::{
    body::Bytes, extract::State, http::StatusCode, response::IntoResponse, routing::post, Router,
};
use clap::{Parser, Subcommand};
use dashmap::DashMap;
use lam::{evaluate, EvalBuilder, LamKV};
use std::{
    fs,
    io::{self, Cursor, Read},
    path,
    sync::Arc,
};
use tower_http::trace::{self, TraceLayer};
use tracing::{error, info, Level};
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(author,version,about,long_about=None)]
struct Cli {
    /// Debug mode
    #[arg(long, short = 'd', env = "DEBUG")]
    debug: bool,

    /// No color https://no-color.org/
    #[arg(long, env = "NO_COLOR")]
    no_color: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Evaluate a script file
    Eval {
        /// Script path
        #[arg(long)]
        file: Option<path::PathBuf>,
        /// Timeout
        #[arg(long, default_value_t = 30)]
        timeout: u64,
    },
    /// Handle request with a script file
    Serve {
        /// Script path
        #[arg(long)]
        file: path::PathBuf,
        /// Timeout
        #[arg(long, default_value_t = 60)]
        timeout: u64,
        /// Bind
        #[arg(long, default_value = "127.0.0.1:3000")]
        bind: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let level = if cli.debug {
        Level::DEBUG.into()
    } else {
        Level::INFO.into()
    };
    let env_filter = EnvFilter::builder()
        .with_default_directive(level)
        .from_env_lossy();
    tracing_subscriber::fmt()
        .with_ansi(!cli.no_color)
        .with_env_filter(env_filter)
        .compact()
        .init();

    match cli.command {
        Commands::Eval { file, timeout } => {
            let script = if let Some(f) = file {
                fs::read_to_string(f)?
            } else {
                let mut buf = String::new();
                std::io::stdin()
                    .read_to_string(&mut buf)
                    .expect("either file or script via standard input should be provided");
                buf
            };
            let e = EvalBuilder::new(io::stdin(), script)
                .set_timeout(timeout)
                .build();
            let res = evaluate(&e)?;
            print!("{}", res.result);
        }
        Commands::Serve {
            bind,
            file,
            timeout,
        } => {
            serve_file(&file, &bind, timeout).await?;
        }
    }
    Ok(())
}

#[derive(Clone)]
struct AppState {
    script: String,
    store: LamKV,
    timeout: u64,
}

async fn index_route(State(state): State<AppState>, body: Bytes) -> impl IntoResponse {
    let e = EvalBuilder::new(Cursor::new(body), state.script.clone())
        .set_timeout(state.timeout)
        .set_store(state.store.clone())
        .build();
    let res = evaluate(&e);
    match res {
        Ok(res) => (StatusCode::OK, res.result),
        Err(err) => {
            error!(%err, "failed to run Lua script");
            (StatusCode::BAD_REQUEST, "".to_string())
        }
    }
}

async fn serve_file(file: &path::PathBuf, bind: &str, timeout: u64) -> anyhow::Result<()> {
    let script = fs::read_to_string(file)?;
    let store = Arc::new(DashMap::new());
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
    let listener = tokio::net::TcpListener::bind(bind).await?;
    info!(%bind, "serving lua script");
    axum::serve(listener, app).await?;
    Ok(())
}
