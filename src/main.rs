use clap::{Parser, Subcommand};
use lam::{evaluate, Evaluation, InMemory};
use std::{fs, io, path};

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
        /// Inline script
        script: Option<String>,
    },
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::Eval {
            file,
            timeout,
            script,
        } => {
            let script = if let Some(f) = file {
                fs::read_to_string(f).expect("failed to read script")
            } else {
                script.expect("inline script is expected when filename is absent")
            };
            let state_manager = Some(InMemory::default());
            let mut e = Evaluation::new(lam::EvalConfig {
                input: io::stdin(),
                script,
                state_manager,
                timeout: Some(timeout),
            });
            let res = evaluate(&mut e).expect("failed to evaluate the script");
            print!("{}", res.result);
        }
    }
}
