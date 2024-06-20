use anyhow::bail;
use clap::{Parser, Subcommand};
use clio::*;
use comfy_table::{presets, Table};
use cron::Schedule;
use lmb::{
    Error, EvaluationBuilder, LuaCheck, PrintOptions, ScheduleOptions, Store, StoreOptions,
    DEFAULT_TIMEOUT, EXAMPLES, GUIDES,
};
use mlua::prelude::*;
use serde_json::json;
use serve::ServeOptions;
use std::{
    fmt::Display,
    io::{self, Read},
    path::PathBuf,
    process::ExitCode,
    str::FromStr,
    time::Duration,
};
use termimad::MadSkin;
use tracing::Level;
use tracing_subscriber::{fmt::format::FmtSpan, EnvFilter};

mod serve;

static VERSION: &str = env!("APP_VERSION");

/// lmb is a Lua function runner.
#[derive(Parser)]
#[command(about, author, version=VERSION)]
struct Cli {
    /// Checks the syntax of the function before evaluation or serving,
    /// disabled by default for startup performance
    #[arg(long, env = "LMB_CHECK_SYNTAX")]
    check_syntax: bool,

    /// Debug mode
    #[arg(long, short = 'd', env = "DEBUG")]
    debug: bool,

    /// Enable JSON mode.
    /// When evaluating, output the solution in JSON format.
    /// When serving, always respond with the solution as a JSON value
    #[arg(long)]
    json: bool,

    /// No color <https://no-color.org/>
    #[arg(long, env = "NO_COLOR")]
    no_color: bool,

    /// Store path. By default, the store is in-memory,
    /// and changes will be lost when the program terminates.
    /// To persist values, a store path must be specified
    #[arg(long, env = "LMB_STORE_PATH")]
    store_path: Option<PathBuf>,

    /// Migrate the store before startup.
    /// If the store path is not specified and the store is in-memory,
    /// it will be automatically migrated
    #[arg(long, env = "LMB_RUN_MIGRATIONS")]
    run_migrations: bool,

    /// Theme. Checkout `list-themes` for available themes
    #[arg(long, env = "LMB_THEME")]
    theme: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Check syntax of script
    Check {
        /// Script path. Specify "-" or omit to load the script from standard input
        #[arg(long, value_parser, default_value = "-")]
        file: Input,
    },
    /// Evaluate a script file
    #[command(alias = "eval")]
    Evaluate {
        /// Script path. Specify "-" or omit to load the script from standard input
        #[arg(long, value_parser, default_value = "-")]
        file: Input,
        /// Timeout in seconds
        #[arg(long, default_value_t = DEFAULT_TIMEOUT.as_secs())]
        timeout: u64,
    },
    /// Check out examples and evaluate or serve them
    #[command(subcommand)]
    Example(ExampleCommands),
    /// Guide commands
    #[command(subcommand)]
    Guide(GuideCommands),
    /// List available themes
    ListThemes,
    /// Schedule the script as a cron job
    Schedule {
        /// Exit immediately upon N number of errors. 0 to disable.
        #[arg(long, default_value_t = 1)]
        bail: usize,
        /// Cron
        #[arg(long)]
        cron: String,
        /// Run the script at startup even if the next execution is not due
        #[arg(long)]
        initial_run: bool,
        /// Script path. Specify "-" or omit to load the script from standard input
        #[arg(long, value_parser, default_value = "-")]
        file: Input,
    },
    /// Handle HTTP requests with the script
    Serve {
        /// Bind the server to a specific host and port
        #[arg(long, default_value = "127.0.0.1:3000")]
        bind: String,
        /// Script path. Specify "-" or omit to load the script from standard input
        #[arg(long, value_parser, default_value = "-")]
        file: Input,
        /// Timeout in seconds
        #[arg(long)]
        timeout: Option<u64>,
    },
    /// Store commands
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
        /// Bind the server to a specific host and port
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
enum GuideCommands {
    /// Read a guide
    Cat {
        /// Name
        #[arg(long)]
        name: String,
    },
    /// List available guides
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
    /// Migrate the store
    Migrate {
        /// Target version. Specify 0 to revert ALL migrations. Omit to migrate to the latest
        #[arg(long)]
        version: Option<usize>,
    },
    /// Insert or update a value
    Put {
        /// Name
        #[arg(long)]
        name: String,
        /// Consider value as plain string instead of JSON value
        #[arg(long)]
        plain: bool,
        /// Value, the content should be a valid JSON value e.g. true or "string" or 1
        #[arg(long, value_parser, default_value = "-")]
        value: Input,
    },
    /// Show current version
    Version,
}

fn do_check_syntax<S>(no_color: bool, name: S, script: S) -> anyhow::Result<()>
where
    S: Display,
{
    let check = LuaCheck::new(name, script);
    if let Err(err) = check.check() {
        let mut buf = Vec::new();
        check.write_error(&mut buf, err, no_color)?;
        bail!(String::from_utf8_lossy(&buf).trim().to_string());
    }
    Ok(())
}

fn read_script(input: &mut Input) -> anyhow::Result<(String, String)> {
    let name = input.path().to_string_lossy().to_string();
    let mut script = String::new();
    input.read_to_string(&mut script)?;
    Ok((name, script))
}

fn prepare_store(options: &StoreOptions) -> anyhow::Result<Store> {
    let store = if let Some(store_path) = options.store_path() {
        let store = Store::new(store_path)?;
        if options.run_migrations() {
            store.migrate(None)?;
        }
        store
    } else {
        Store::default()
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

    let mut print_options = PrintOptions::default();
    print_options.set_no_color(cli.no_color);
    print_options.set_theme(cli.theme);

    let store_options = StoreOptions::new(cli.store_path, cli.run_migrations);
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
                .name(&name)
                .store(store)
                .timeout(Some(Duration::from_secs(timeout)))
                .build();
            let mut buf = String::new();
            match e.evaluate() {
                Ok(s) => {
                    s.write(&mut buf, cli.json)?;
                    print!("{buf}");
                    Ok(())
                }
                Err(err) => {
                    err.write_lua_error(&mut buf, &e, cli.no_color)?;
                    eprint!("{buf}");
                    Err(err.into())
                }
            }
        }
        Commands::Example(ExampleCommands::Cat { name }) => {
            let Some(found) = EXAMPLES.iter().find(|e| e.name() == name) else {
                bail!("example with {name} not found");
            };
            let script = found.script().trim();
            let mut buf = String::new();
            let e = EvaluationBuilder::new(script, io::stdin()).build();
            e.write_script(&mut buf, &print_options)?;
            println!("{buf}");
            Ok(())
        }
        Commands::Example(ExampleCommands::Evaluate { name }) => {
            let Some(found) = EXAMPLES.iter().find(|e| e.name() == name) else {
                bail!("example with {name} not found");
            };
            let script = found.script().trim();
            let store = prepare_store(&store_options)?;
            let e = EvaluationBuilder::new(script, io::stdin())
                .name(name.as_str())
                .store(store)
                .build();
            let mut buf = String::new();
            match e.evaluate() {
                Ok(s) => {
                    s.write(&mut buf, cli.json)?;
                    print!("{buf}");
                    Ok(())
                }
                Err(err) => {
                    err.write_lua_error(&mut buf, &e, cli.no_color)?;
                    eprint!("{buf}");
                    Err(err.into())
                }
            }
        }
        Commands::Example(ExampleCommands::List) => {
            let mut table = Table::new();
            table.load_preset(presets::NOTHING);
            table.set_header(["name", "description"]);
            for e in EXAMPLES.iter() {
                table.add_row([e.name(), e.description()]);
            }
            println!("{table}");
            Ok(())
        }
        Commands::Example(ExampleCommands::Serve {
            bind,
            name,
            timeout,
        }) => {
            let Some(found) = EXAMPLES.iter().find(|e| e.name() == name) else {
                bail!("example with {name} not found");
            };
            if cli.check_syntax {
                do_check_syntax(cli.no_color, name.as_str(), found.script())?;
            }
            let timeout = timeout.map(Duration::from_secs);
            let mut options = ServeOptions::new(name.as_str(), found.script(), bind, store_options);
            options.set_json(cli.json);
            options.set_timeout(timeout);
            serve::serve_file(&options).await?;
            Ok(())
        }
        Commands::Guide(GuideCommands::List) => {
            let mut table = Table::new();
            table.load_preset(presets::NOTHING);
            table.set_header(["name", "title"]);
            for guide in GUIDES.iter() {
                table.add_row([guide.name(), guide.title()]);
            }
            print!("{table}");
            Ok(())
        }
        Commands::Guide(GuideCommands::Cat { name }) => {
            let Some(guide) = GUIDES.iter().find(|g| name == g.name()) else {
                bail!("guide with {name} not found");
            };
            let skin = MadSkin::default();
            println!("{}", skin.term_text(guide.content()));
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
            bail,
            cron,
            mut file,
            initial_run,
        } => {
            let (name, script) = read_script(&mut file)?;
            let schedule = Schedule::from_str(&cron)?;
            let store = prepare_store(&store_options)?;

            let mut options = ScheduleOptions::new(schedule);
            options.set_bail(bail);
            options.set_initial_run(initial_run);

            let e = EvaluationBuilder::new(script, io::stdin())
                .name(name)
                .store(store)
                .build();
            e.schedule(&options);
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
            let mut options = ServeOptions::new(name, script, bind, store_options);
            options.set_timeout(timeout);
            serve::serve_file(&options).await?;
            Ok(())
        }
        Commands::Store(c) => {
            let Some(store_path) = store_options.store_path() else {
                bail!("store_path is required");
            };
            let store = Store::new(store_path)?;
            if store_options.run_migrations() {
                store.migrate(None)?;
            }
            match c {
                StoreCommands::Delete { name } => {
                    let affected = store.delete(name)?;
                    print!("{affected}");
                    Ok(())
                }
                StoreCommands::Get { name } => {
                    let values = store.get([name])?;
                    if let Some(value) = values.as_array().and_then(|a| a.first()) {
                        let value = serde_json::to_string(&value)?;
                        print!("{value}");
                    } else {
                        print!("{}", serde_json::Value::Null);
                    }
                    Ok(())
                }
                StoreCommands::List => {
                    let metadata_rows = store.list()?;
                    let mut table = Table::new();
                    table.load_preset(presets::NOTHING);
                    table.set_header(["name", "type", "size", "created at", "updated at"]);
                    for m in metadata_rows.iter() {
                        table.add_row([
                            m.name(),
                            m.type_hint(),
                            &m.size().to_string(),
                            &m.created_at().to_rfc3339(),
                            &m.updated_at().to_rfc3339(),
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
                        json!(buf)
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
        match e.downcast_ref::<Error>() {
            // the following errors are handled, do nothing
            Some(&Error::Lua(LuaError::RuntimeError(_) | LuaError::SyntaxError { .. })) => {}
            _ => eprintln!("{e}"),
        }
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
