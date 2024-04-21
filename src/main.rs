use annotate_snippets::Renderer;
use clap::{Parser, Subcommand, ValueEnum};
use lam::{check_syntax, render_error, EvalBuilder, LamStore};
use std::{
    fs,
    io::{self, Read},
    path::PathBuf,
    time::Duration,
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
    /// Check syntax of script
    Check {
        /// Script path
        #[arg(long)]
        file: Option<PathBuf>,
    },
    /// Evaluate a script file
    Eval {
        #[command(flatten)]
        store_options: StoreOptions,
        /// Script path
        #[arg(long)]
        file: Option<PathBuf>,
        /// Timeout in seconds
        #[arg(long)]
        timeout: Option<u64>,
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
        file: Option<PathBuf>,
        /// Timeout in seconds
        #[arg(long)]
        timeout: Option<u64>,
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

fn file_or_stdin(file: Option<PathBuf>) -> anyhow::Result<(String, String)> {
    let name = if let Some(f) = &file {
        f.to_string_lossy().to_string()
    } else {
        "(stdin)".to_string()
    };
    let script = if let Some(f) = &file {
        fs::read_to_string(f)?
    } else {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .expect("either file or script via standard input should be provided");
        buf
    };
    Ok((name, script))
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
        Commands::Check { file } => {
            let (name, script) = file_or_stdin(file)?;
            if let Err(err) = check_syntax(&script) {
                if let Some(message) = render_error(&name, &script, &err) {
                    let renderer = if cli.no_color {
                        Renderer::plain()
                    } else {
                        Renderer::styled()
                    };
                    let rendered = renderer.render(message);
                    eprintln!("{rendered}");
                }
            }
        }
        Commands::Eval {
            file,
            output_format,
            timeout,
            ..
        } => {
            let (name, script) = file_or_stdin(file)?;
            let timeout = timeout.map(Duration::from_secs);
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
            let (name, script) = file_or_stdin(file)?;
            let timeout = timeout.map(Duration::from_secs);
            serve::serve_file(&bind, &name, &script, timeout, &store_options).await?;
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
