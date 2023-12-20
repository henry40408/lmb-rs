use axum::{
    body::Bytes, extract::State, http::StatusCode, response::IntoResponse, routing::post, Router,
};
use clap::{Parser, Subcommand};
use lam::{evaluate, EvaluationBuilder};
use std::{
    fs,
    io::{self, Cursor, Read},
    path,
    sync::Arc,
};
use tracing::error;

#[derive(Parser, Debug)]
#[command(author,version,about,long_about=None)]
struct Cli {
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
            let mut e = EvaluationBuilder::new(io::stdin(), script)
                .set_timeout(timeout)
                .build();
            let res = evaluate(&mut e)?;
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

struct AppState {
    script: String,
    timeout: u64,
}

async fn index_route(State(state): State<Arc<AppState>>, body: Bytes) -> impl IntoResponse {
    let mut e = EvaluationBuilder::new(Cursor::new(body), state.script.clone())
        .set_timeout(state.timeout)
        .build();
    let res = evaluate(&mut e);
    let (status_code, response_body) = match res {
        Ok(res) => (StatusCode::OK, res.result),
        Err(e) => {
            error!("{:?}", e);
            (StatusCode::BAD_REQUEST, "".to_string())
        }
    };
    (status_code, response_body)
}

async fn serve_file(file: &path::PathBuf, bind: &str, timeout: u64) -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let script = fs::read_to_string(file)?;
    let app_state = Arc::new(AppState { script, timeout });

    let app = Router::new()
        .route("/", post(index_route))
        .with_state(app_state);
    let listener = tokio::net::TcpListener::bind(bind).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
