use axum::{
    body::Bytes, extract::State, http::StatusCode, response::IntoResponse, routing::post, Router,
};
use clap::{Parser, Subcommand, ValueEnum};
use lam::{lam_evaluate, EvalBuilder, LamStore};
use std::{
    fs,
    io::{self, Cursor, Read},
    path::{self, PathBuf},
};
use tower_http::trace::{self, TraceLayer};
use tracing::{error, info, warn, Level};
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(about, author, long_about=None, version)]
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

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
enum OutputFormat {
    /// Plain text
    Text,
    /// JSON
    Json,
}

#[derive(Debug, Parser)]
struct StoreOptions {
    /// Run migrations
    #[arg(long, env = "RUN_MIGRATIONS")]
    run_migrations: bool,

    /// Store path
    #[arg(long, env = "STORE_PATH")]
    store_path: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Evaluate a script file
    Eval {
        #[command(flatten)]
        store_options: StoreOptions,
        /// Script path
        #[arg(long)]
        file: Option<path::PathBuf>,
        /// Timeout
        #[arg(long, default_value_t = 30)]
        timeout: u64,
        /// Output format
        #[arg(long, value_enum, default_value = "text")]
        output_format: OutputFormat,
    },
    /// Handle request with a script file
    Serve {
        #[command(flatten)]
        store_options: StoreOptions,
        /// Bind
        #[arg(long, default_value = "127.0.0.1:3000")]
        bind: String,
        /// Script path
        #[arg(long)]
        file: path::PathBuf,
        /// Timeout
        #[arg(long, default_value_t = 60)]
        timeout: u64,
    },
    /// Store commands
    #[command(subcommand)]
    Store(StoreCommands),
}

#[derive(Debug, Parser)]
enum StoreCommands {
    /// Run migrations on the store
    Migrate {
        /// Store path
        #[arg(long)]
        store_path: PathBuf,
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
        Commands::Eval {
            file,
            output_format,
            timeout,
            ..
        } => {
            let name = if let Some(ref f) = file {
                f.to_string_lossy().to_string()
            } else {
                "(stdin)".to_string()
            };
            let script = if let Some(ref f) = file {
                fs::read_to_string(f)?
            } else {
                let mut buf = String::new();
                std::io::stdin()
                    .read_to_string(&mut buf)
                    .expect("either file or script via standard input should be provided");
                buf
            };
            let e = EvalBuilder::new(io::stdin(), script)
                .set_name(name)
                .set_timeout(timeout)
                .build();
            let res = lam_evaluate(&e)?;
            let res = match output_format {
                OutputFormat::Text => res.result.to_string(),
                OutputFormat::Json => serde_json::to_string(&res.result)?,
            };
            print!("{}", res);
        }
        Commands::Serve {
            bind,
            file,
            store_options,
            timeout,
        } => {
            let run_migrations = store_options.run_migrations;
            let store_path = store_options.store_path.as_ref();
            serve_file(&file, &bind, timeout, store_path, run_migrations).await?;
        }
        Commands::Store(c) => match c {
            StoreCommands::Migrate { store_path } => {
                let store = LamStore::new(&store_path)?;
                store.migrate()?;
            }
        },
    }
    Ok(())
}

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
    let res = lam_evaluate(&e);
    match res {
        Ok(res) => (StatusCode::OK, res.result.to_string()),
        Err(err) => {
            error!(%err, "failed to run Lua script");
            (StatusCode::BAD_REQUEST, "".to_string())
        }
    }
}

async fn serve_file(
    file: &path::PathBuf,
    bind: &str,
    timeout: u64,
    store_path: Option<&PathBuf>,
    run_migrations: bool,
) -> anyhow::Result<()> {
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
        store.migrate()?;
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
    let listener = tokio::net::TcpListener::bind(bind).await?;
    info!(%bind, "serving lua script");
    axum::serve(listener, app).await?;
    Ok(())
}
