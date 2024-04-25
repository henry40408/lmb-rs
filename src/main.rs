use clap::{Parser, Subcommand};
use clap_stdin::{FileOrStdin, Source};
use lam::{check_syntax, render_error, EvalBuilder, LamStore};
use std::{io, path::PathBuf, time::Duration};
use tracing::Level;
use tracing_subscriber::{fmt::format::FmtSpan, EnvFilter};

mod serve;

#[derive(Parser)]
#[command(about, author, long_about=None, version)]
struct Cli {
    /// Checks the syntax of the function, disabled by default for performance reasons
    #[arg(long, env = "CHECK_SYNTAX")]
    check_syntax: bool,

    /// Debug mode
    #[arg(long, short = 'd', env = "DEBUG")]
    debug: bool,

    /// Output as JSON
    #[arg(long)]
    json: bool,

    /// No color https://no-color.org/
    #[arg(long, env = "NO_COLOR")]
    no_color: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Parser)]
struct StoreOptions {
    /// Run migrations
    #[arg(long, env = "RUN_MIGRATIONS")]
    run_migrations: bool,

    /// Store path
    #[arg(long, env = "STORE_PATH")]
    store_path: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Commands {
    /// Check syntax of script
    Check {
        /// Script path
        #[arg(long)]
        file: FileOrStdin,
    },
    /// Evaluate a script file
    Eval {
        #[command(flatten)]
        store_options: StoreOptions,
        /// Script path
        #[arg(long)]
        file: FileOrStdin,
        /// Timeout in seconds
        #[arg(long)]
        timeout: Option<u64>,
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
        file: FileOrStdin,
        /// Timeout in seconds
        #[arg(long)]
        timeout: Option<u64>,
    },
    /// Store commands
    #[command(subcommand)]
    Store(StoreCommands),
}

#[derive(Parser)]
enum StoreCommands {
    /// Run migrations on the store
    Migrate {
        /// Store path
        #[arg(long)]
        store_path: PathBuf,
    },
}

fn do_check_syntax<S: AsRef<str>>(no_color: bool, name: S, script: S) -> bool {
    let res = check_syntax(script.as_ref());
    if let Some(message) = render_error(no_color, name, script, res) {
        eprint!("{message}");
        false
    } else {
        true
    }
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
            let name = if let Source::Arg(path) = &file.source {
                path.to_string()
            } else {
                "(stdin)".to_string()
            };
            let script = file.contents()?;
            do_check_syntax(cli.no_color, name, script);
        }
        Commands::Eval { file, timeout, .. } => {
            let name = if let Source::Arg(path) = &file.source {
                path.to_string()
            } else {
                "(stdin)".to_string()
            };
            let script = file.contents()?;
            if cli.check_syntax && !do_check_syntax(cli.no_color, &name, &script) {
                return Ok(());
            }
            let timeout = timeout.map(Duration::from_secs);
            let e = EvalBuilder::new(script.into(), io::stdin())
                .with_name(name.into())
                .with_timeout(timeout)
                .build();
            let res = e.evaluate()?;
            let output = if cli.json {
                serde_json::to_string(&res.result)?
            } else {
                res.result.to_string()
            };
            print!("{}", output);
        }
        Commands::Serve {
            bind,
            file,
            store_options,
            timeout,
        } => {
            let name = if let Source::Arg(path) = &file.source {
                path.to_string()
            } else {
                "(stdin)".to_string()
            };
            let script = file.contents()?;
            if cli.check_syntax && !do_check_syntax(cli.no_color, &name, &script) {
                return Ok(());
            }
            let timeout = timeout.map(Duration::from_secs);
            serve::serve_file(&bind, name.into(), script.into(), timeout, &store_options).await?;
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
