use anyhow::bail;
use clap::{Parser, Subcommand};
use clio::*;
use comfy_table::{presets, Table};
use cron::Schedule;
use lmb::{
    check_syntax, render_fullmoon_result, render_script, render_solution, schedule_script,
    EvaluationBuilder, LmbError, LmbStore, LmbValue, PrintOptions, ScheduleOptions, StoreOptions,
    DEFAULT_TIMEOUT, EXAMPLES,
};
use mlua::prelude::*;
use serve::ServeOptions;
use std::io::Read;
use std::str::FromStr;
use std::{io, path::PathBuf, process::ExitCode, time::Duration};
use tracing::Level;
use tracing_subscriber::{fmt::format::FmtSpan, EnvFilter};

mod serve;

static VERSION: &str = env!("APP_VERSION");

/// lmb is a Lua function runner.
#[derive(Parser)]
#[command(about, author, version=VERSION)]
struct Cli {
    /// Checks the syntax of the function, disabled by default for performance reasons
    #[arg(long, env = "LMB_CHECK_SYNTAX")]
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

    /// Store path
    #[arg(long, env = "LMB_STORE_PATH")]
    store_path: Option<PathBuf>,

    /// Run migrations
    #[arg(long, env = "LMB_RUN_MIGRATIONS")]
    run_migrations: bool,

    /// Theme. Checkout `list-themes` for available themes.
    #[arg(long, env = "LMB_THEME")]
    theme: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Check syntax of script
    Check {
        /// Script path
        #[arg(long, value_parser, default_value = "-")]
        file: Input,
    },
    /// Evaluate a script file
    #[command(alias = "eval")]
    Evaluate {
        /// Script path
        #[arg(long, value_parser, default_value = "-")]
        file: Input,
        /// Timeout in seconds
        #[arg(long, default_value_t = DEFAULT_TIMEOUT.as_secs())]
        timeout: u64,
    },
    /// Interact with examples
    #[command(subcommand)]
    Example(ExampleCommands),
    /// List available themes
    ListThemes,
    /// Schedule the script as a cron job
    Schedule {
        /// Cron
        #[arg(long)]
        cron: String,
        /// Run for the first time immediately
        #[arg(long)]
        initial_run: bool,
        /// Script path
        #[arg(long, value_parser, default_value = "-")]
        file: Input,
    },
    /// Run a HTTP server from a Lua script
    Serve {
        /// Bind
        #[arg(long, default_value = "127.0.0.1:3000")]
        bind: String,
        /// Script path
        #[arg(long, value_parser, default_value = "-")]
        file: Input,
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
    /// Delete a value
    Delete {
        /// Name
        #[arg(long)]
        name: String,
    },
    /// Get a value
    Get {
        /// Name
        #[arg(long)]
        name: String,
    },
    /// List values
    List,
    /// Run migrations on the store
    Migrate {
        /// Target version, 0 to revert ALL migrations
        #[arg(long)]
        version: Option<usize>,
    },
    /// Put a value
    Put {
        /// Name
        #[arg(long)]
        name: String,
        /// Plain. Consider value as plain string instead of JSON value.
        #[arg(long)]
        plain: bool,
        /// Value, the content should be a valid JSON value e.g. true or "string" or 1
        #[arg(long, value_parser, default_value = "-")]
        value: Input,
    },
    /// Show current version
    Version,
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

fn read_script(input: &mut Input) -> anyhow::Result<(String, String)> {
    let name = input.path().to_string();
    let mut script = String::new();
    input.read_to_string(&mut script)?;
    Ok((name, script))
}

fn prepare_store(options: &StoreOptions) -> anyhow::Result<LmbStore> {
    let store = if let Some(store_path) = &options.store_path {
        let store = LmbStore::new(store_path)?;
        if options.run_migrations {
            store.migrate(None)?;
        }
        store
    } else {
        LmbStore::default()
    };
    Ok(store)
}

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
    let span_events = env_filter.max_level_hint().map_or_else(
        || FmtSpan::CLOSE,
        |l| {
            if l >= Level::DEBUG {
                FmtSpan::CLOSE
            } else {
                FmtSpan::NONE
            }
        },
    );
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
    let store_options = StoreOptions {
        store_path: cli.store_path,
        run_migrations: cli.run_migrations,
    };
    match cli.command {
        Commands::Check { mut file } => {
            let (name, script) = read_script(&mut file)?;
            do_check_syntax(cli.no_color, &name, &script)
        }
        Commands::Evaluate { mut file, timeout } => {
            let (name, script) = read_script(&mut file)?;
            if cli.check_syntax {
                do_check_syntax(cli.no_color, &name, &script)?;
            }
            let store = prepare_store(&store_options)?;
            let e = EvaluationBuilder::new(&script, io::stdin())
                .with_name(&name)
                .with_store(store)
                .with_timeout(Some(Duration::from_secs(timeout)))
                .build();
            let res = e.evaluate();
            let mut buf = String::new();
            match render_solution(&mut buf, name, script, res, &print_options) {
                Ok(_) => {
                    print!("{buf}");
                    Ok(())
                }
                Err(e) => {
                    eprint!("{buf}");
                    Err(e.into())
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
            println!("{buf}");
            Ok(())
        }
        Commands::Example(ExampleCommands::Evaluate { name }) => {
            let Some(found) = EXAMPLES.iter().find(|e| e.name == name) else {
                bail!("example with {name} not found");
            };
            let script = found.script.trim();
            let store = prepare_store(&store_options)?;
            let e = EvaluationBuilder::new(script, io::stdin())
                .with_name(name.as_str())
                .with_store(store)
                .build();
            let res = e.evaluate();
            let mut buf = String::new();
            match render_solution(&mut buf, name, script.to_string(), res, &print_options) {
                Ok(_) => {
                    print!("{buf}");
                    Ok(())
                }
                Err(e) => {
                    eprint!("{buf}");
                    Err(e.into())
                }
            }
        }
        Commands::Example(ExampleCommands::List) => {
            let mut table = Table::new();
            table.load_preset(presets::NOTHING);
            table.set_header(vec!["name", "description"]);
            for e in EXAMPLES.iter() {
                table.add_row(vec![&e.name, &e.description]);
            }
            println!("{table}");
            Ok(())
        }
        Commands::Example(ExampleCommands::Serve {
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
        Commands::Schedule {
            cron,
            mut file,
            initial_run,
        } => {
            let (name, script) = read_script(&mut file)?;
            let schedule = Schedule::from_str(&cron)?;
            let store = prepare_store(&store_options)?;
            let options = ScheduleOptions {
                initial_run,
                name,
                schedule,
                script,
                store,
            };
            schedule_script(options);
            Ok(())
        }
        Commands::Serve {
            bind,
            mut file,
            timeout,
        } => {
            let (name, script) = read_script(&mut file)?;
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
        Commands::Store(c) => {
            let Some(store_path) = &store_options.store_path else {
                bail!("store_path is required");
            };
            let store = LmbStore::new(store_path)?;
            if store_options.run_migrations {
                store.migrate(None)?;
            }
            match c {
                StoreCommands::Delete { name } => {
                    let affected = store.delete(name)?;
                    print!("{affected}");
                    Ok(())
                }
                StoreCommands::Get { name } => {
                    let value = store.get(name)?;
                    let value = serde_json::to_string(&value)?;
                    print!("{value}");
                    Ok(())
                }
                StoreCommands::List => {
                    let metadata_rows = store.list()?;
                    let mut table = Table::new();
                    table.load_preset(presets::NOTHING);
                    table.set_header(vec!["name", "type", "size", "created at", "updated at"]);
                    for m in metadata_rows.iter() {
                        table.add_row(vec![
                            &m.name,
                            &m.type_hint,
                            &m.size.to_string(),
                            &m.created_at.to_rfc3339(),
                            &m.updated_at.to_rfc3339(),
                        ]);
                    }
                    println!("{table}");
                    Ok(())
                }
                StoreCommands::Migrate { version } => {
                    store.migrate(version)?;
                    Ok(())
                }
                StoreCommands::Put {
                    name,
                    plain,
                    mut value,
                } => {
                    let mut buf = String::new();
                    value.read_to_string(&mut buf)?;
                    let value = if plain {
                        LmbValue::String(buf)
                    } else {
                        serde_json::from_str(&buf)?
                    };
                    let affected = store.put(name, &value)?;
                    print!("{affected}");
                    Ok(())
                }
                StoreCommands::Version => {
                    let version = store.current_version()?;
                    println!("{version}");
                    Ok(())
                }
            }
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    if let Err(e) = try_main().await {
        match e.downcast_ref::<LmbError>() {
            // the following errors are handled, do nothing
            Some(&LmbError::Lua(LuaError::RuntimeError(_) | LuaError::SyntaxError { .. })) => {}
            _ => eprintln!("{e}"),
        }
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
