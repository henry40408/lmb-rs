use anyhow::bail;
use clap::{Parser, Subcommand};
use clap_stdin::{FileOrStdin, Source};
use comfy_table::{presets, Table};
use lam::*;
use mlua::prelude::*;
use serve::ServeOptions;
use std::{io, path::PathBuf, process::ExitCode, time::Duration};
use tracing::Level;
use tracing_subscriber::{fmt::format::FmtSpan, EnvFilter};

mod serve;

static VERSION: &str = env!("APP_VERSION");

#[derive(Parser)]
#[command(about, author, version=VERSION)]
/// lam is a Lua function runner.
struct Cli {
    /// Checks the syntax of the function, disabled by default for performance reasons
    #[arg(long, env = "LAM_CHECK_SYNTAX")]
    check_syntax: bool,

    /// Debug mode
    #[arg(long, short = 'd', env = "DEBUG")]
    debug: bool,

    /// Output as JSON
    #[arg(long)]
    json: bool,

    /// No color <https://no-color.org/>
    #[arg(long, env = "NO_COLOR")]
    no_color: bool,

    /// Theme. Checkout `list-themes` for available themes.
    #[arg(long, env = "LAM_THEME")]
    theme: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Default, Parser)]
struct StoreOptions {
    /// Run migrations
    #[arg(long, env = "LMA_RUN_MIGRATIONS")]
    run_migrations: bool,

    /// Store path
    #[arg(long, env = "LAM_STORE_PATH")]
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
    #[command(alias = "eval")]
    Evaluate {
        #[command(flatten)]
        store_options: StoreOptions,
        /// Script path
        #[arg(long)]
        file: FileOrStdin,
        /// Timeout in seconds
        #[arg(long)]
        timeout: Option<u64>,
    },
    /// Interact with examples
    #[command(subcommand)]
    Example(ExampleCommands),
    /// List available themes
    ListThemes,
    /// Run a HTTP server from a Lua script
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
    /// Commands on store
    #[command(subcommand)]
    Store(StoreCommands),
}

#[derive(Parser)]
enum ExampleCommands {
    /// Print script of example
    Cat {
        /// Example name
        #[arg(long)]
        name: String,
    },
    /// Evaluate the example
    #[command(alias = "eval")]
    Evaluate {
        /// Example name
        #[arg(long)]
        name: String,
    },
    /// Handle HTTP requests with the example
    Serve {
        #[command(flatten)]
        store_options: StoreOptions,
        /// Bind
        #[arg(long, default_value = "127.0.0.1:3000")]
        bind: String,
        /// Example name
        #[arg(long)]
        name: String,
        /// Timeout in seconds
        #[arg(long)]
        timeout: Option<u64>,
    },
    /// List examples
    #[command(alias = "ls")]
    List,
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

fn do_check_syntax<S: AsRef<str>>(no_color: bool, name: S, script: S) -> anyhow::Result<()> {
    let res = check_syntax(script.as_ref());
    if let Err(e) = res {
        if let Some(message) = render_fullmoon_result(no_color, name, script, &e) {
            bail!(message);
        }
        bail!(e);
    }
    Ok(())
}

#[cfg(not(tarpaulin_include))]
async fn try_main() -> anyhow::Result<()> {
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

    let print_options = PrintOptions {
        json: cli.json,
        no_color: cli.no_color,
        theme: cli.theme,
        ..Default::default()
    };
    match cli.command {
        Commands::Check { file } => {
            let name = if let Source::Arg(path) = &file.source {
                path.to_string()
            } else {
                "(stdin)".to_string()
            };
            let script = file.contents()?;
            do_check_syntax(cli.no_color, name, script)
        }
        Commands::Evaluate { file, timeout, .. } => {
            let name = if let Source::Arg(path) = &file.source {
                path.to_string()
            } else {
                "(stdin)".to_string()
            };
            let script = file.contents()?;
            if cli.check_syntax {
                do_check_syntax(cli.no_color, &name, &script)?;
            }
            let timeout = timeout.map(Duration::from_secs);
            let e = EvaluationBuilder::new(&script, io::stdin())
                .with_name(&name)
                .with_timeout(timeout)
                .build();
            let res = e.evaluate();
            let mut buf = String::new();
            match render_evaluation_result(&mut buf, script, res, &print_options) {
                Ok(_) => {
                    print!("{buf}");
                    Ok(())
                }
                Err(e) => {
                    eprint!("{buf}");
                    Err(e)
                }
            }
        }
        Commands::Example(ExampleCommands::Cat { name }) => {
            let Some(found) = EXAMPLES.iter().find(|e| e.name == name) else {
                bail!("example with {name} not found");
            };
            let script = &found.script.trim();
            let mut buf = String::new();
            render_script(&mut buf, script, &print_options)?;
            print!("{buf}");
            Ok(())
        }
        Commands::Example(ExampleCommands::Evaluate { name }) => {
            let Some(found) = EXAMPLES.iter().find(|e| e.name == name) else {
                bail!("example with {name} not found");
            };
            let script = found.script.trim();
            let e = EvaluationBuilder::new(script, io::stdin())
                .with_name(name.as_str())
                .build();
            let res = e.evaluate();
            let mut buf = String::new();
            match render_evaluation_result(&mut buf, script, res, &print_options) {
                Ok(_) => {
                    print!("{buf}");
                    Ok(())
                }
                Err(e) => {
                    eprint!("{buf}");
                    Err(e)
                }
            }
        }
        Commands::Example(ExampleCommands::List) => {
            let mut table = Table::new();
            table.load_preset(presets::NOTHING);
            table.set_header(vec!["name", "description"]);
            for e in EXAMPLES.iter() {
                let name = &e.name;
                let description = &e.description;
                table.add_row(vec![name, description]);
            }
            print!("{table}");
            Ok(())
        }
        Commands::Example(ExampleCommands::Serve {
            store_options,
            bind,
            name,
            timeout,
        }) => {
            let Some(found) = EXAMPLES.iter().find(|e| e.name == name) else {
                bail!("example with {name} not found");
            };
            let script = &found.script;
            if cli.check_syntax {
                do_check_syntax(cli.no_color, &name, script)?;
            }
            let timeout = timeout.map(Duration::from_secs);
            serve::serve_file(&ServeOptions {
                json: cli.json,
                bind,
                name,
                script: script.to_string(),
                timeout,
                store_options,
            })
            .await?;
            Ok(())
        }
        Commands::ListThemes => {
            let p = bat::PrettyPrinter::new();
            for t in p.themes() {
                println!("{t}");
            }
            Ok(())
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
            if cli.check_syntax {
                do_check_syntax(cli.no_color, &name, &script)?;
            }
            let timeout = timeout.map(Duration::from_secs);
            serve::serve_file(&ServeOptions {
                json: cli.json,
                bind,
                name,
                script,
                timeout,
                store_options,
            })
            .await?;
            Ok(())
        }
        Commands::Store(c) => match c {
            StoreCommands::Migrate { store_path } => {
                let store = LamStore::new(&store_path)?;
                store.migrate()?;
                Ok(())
            }
        },
    }
}

#[cfg(not(tarpaulin_include))]
#[tokio::main]
async fn main() -> ExitCode {
    if let Err(e) = try_main().await {
        match e.downcast_ref::<LamError>() {
            // the following errors are handled, do nothing
            Some(&LamError::Lua(LuaError::RuntimeError(_) | LuaError::SyntaxError { .. })) => {}
            _ => eprint!("{e:?}"),
        }
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use crate::do_check_syntax;

    #[test]
    fn syntax_check() {
        let no_color = true;
        let name = "test";
        do_check_syntax(no_color, name, "return true").unwrap();
        assert!(do_check_syntax(no_color, name, "ret true").is_err());
    }
}
