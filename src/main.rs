use clap::{Parser, Subcommand, ValueEnum};
use lam::{EvalBuilder, LamStore};
use std::{
    fs,
    io::{self, Read},
    path::{self, PathBuf},
};
use tracing::Level;
use tracing_subscriber::{fmt::format::FmtSpan, EnvFilter};

mod serve;

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

    let default_directive = if cli.debug {
        Level::DEBUG.into()
    } else {
        Level::INFO.into()
    };
    let env_filter = EnvFilter::builder()
        .with_default_directive(default_directive)
        .from_env_lossy();
    let span_events = env_filter.max_level_hint().map_or(FmtSpan::CLOSE, |l| {
        if l >= Level::DEBUG {
            FmtSpan::CLOSE
        } else {
            FmtSpan::NONE
        }
    });
    tracing_subscriber::fmt()
        .with_ansi(!cli.no_color)
        .with_env_filter(env_filter)
        .with_span_events(span_events)
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
            let res = e.evaluate()?;
            let output = match output_format {
                OutputFormat::Text => res.result.to_string(),
                OutputFormat::Json => serde_json::to_string(&res.result)?,
            };
            print!("{}", output);
        }
        Commands::Serve {
            bind,
            file,
            store_options,
            timeout,
        } => {
            let run_migrations = store_options.run_migrations;
            let store_path = store_options.store_path.as_ref();
            serve::serve_file(&file, &bind, timeout, store_path, run_migrations).await?;
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
