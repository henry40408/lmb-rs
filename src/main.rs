use anyhow::bail;
use clap::{Parser, Subcommand};
use clap_stdin::{FileOrStdin, Source};
use comfy_table::{presets, Table};
use fancy_regex::Regex;
use lam::*;
use mlua::prelude::*;
use once_cell::sync::Lazy;
use serve::ServeOptions;
use std::{
    io::{self, Read},
    path::PathBuf,
    process::ExitCode,
    time::Duration,
};
use tracing::Level;
use tracing_subscriber::{fmt::format::FmtSpan, EnvFilter};

mod serve;

static LUA_ERROR_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\[[^\]]+\]:(\d+):.+")
        .expect("failed to compile regular expression for Lua error message")
});
static VERSION: &str = env!("APP_VERSION");

#[derive(Parser)]
#[command(about, author, version=VERSION)]
/// lam is a Lua function runner.
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

    /// No color <https://no-color.org/>
    #[arg(long, env = "NO_COLOR")]
    no_color: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Default, Parser)]
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

fn print_result<R, S>(
    json: bool,
    no_color: bool,
    result: LamResult<Solution<R>>,
    script: S,
) -> anyhow::Result<()>
where
    for<'lua> R: 'lua + Read,
    S: AsRef<str>,
{
    match result {
        Ok(eval_result) => {
            let output = if json {
                serde_json::to_string(&eval_result.payload)?
            } else {
                eval_result.payload.to_string()
            };
            print!("{output}");
            Ok(())
        }
        Err(e) => match &e {
            LamError::Lua(LuaError::RuntimeError(message)) => {
                let first_line = message.lines().next().unwrap_or_default();
                eprintln!("{first_line}");
                if let Ok(Some(c)) = LUA_ERROR_REGEX.captures(first_line) {
                    if let Some(n) = c.get(1).and_then(|n| n.as_str().parse::<usize>().ok()) {
                        print_script(
                            script,
                            &PrintOptions {
                                highlighted: Some(n),
                                no_color: Some(no_color),
                            },
                        )?;
                    }
                }
                Err(e.into())
            }
            _ => Err(e.into()),
        },
    }
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
                .with_name(name)
                .with_timeout(timeout)
                .build();
            let res = e.evaluate();
            print_result(cli.json, cli.no_color, res, &script)
        }
        Commands::Example(ExampleCommands::Cat { name }) => {
            let Some(found) = EXAMPLES.iter().find(|e| e.name == name) else {
                bail!("example with {name} not found");
            };
            let script = &found.script.trim();
            print_script(
                script,
                &PrintOptions {
                    highlighted: None,
                    no_color: Some(cli.no_color),
                },
            )
        }
        Commands::Example(ExampleCommands::Evaluate { name }) => {
            let Some(found) = EXAMPLES.iter().find(|e| e.name == name) else {
                bail!("example with {name} not found");
            };
            let script = found.script.trim();
            let e = EvaluationBuilder::new(script, io::stdin())
                .with_name(name)
                .build();
            let res = e.evaluate();
            print_result(cli.json, cli.no_color, res, script)
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
            println!("{table}");
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
            // handled, do nothing
            Some(&LamError::Lua(LuaError::RuntimeError(_))) => {}
            _ => eprint!("{e:?}"),
        }
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use std::io::empty;

    use lam::EvaluationBuilder;

    use crate::{do_check_syntax, print_result};

    #[test]
    fn print_evaluation_result() {
        let script = "return true";
        let e = EvaluationBuilder::new(script, empty()).build();

        let res = e.evaluate();
        let no_color = true;
        print_result(false, no_color, res, script).unwrap();

        let res = e.evaluate();
        print_result(true, no_color, res, script).unwrap();
    }

    #[test]
    fn syntax_check() {
        let no_color = true;
        let name = "test";
        do_check_syntax(no_color, name, "return true").unwrap();
        assert!(do_check_syntax(no_color, name, "ret true").is_err());
    }
}
