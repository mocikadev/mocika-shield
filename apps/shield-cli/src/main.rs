use anyhow::Result;
use clap::Parser;

mod args;
mod cli_json;
mod commands;
mod config;

use args::{Cli, Commands};
use cli_json::error_event_json;
use commands::{run_check_apk, run_check_keystore, run_protect, run_sign};
use config::CliConfig;

fn main() {
    let cli = Cli::parse();
    let machine_output = cli.command.machine_output();
    if let Err(error) = execute(cli) {
        if machine_output {
            println!("{}", error_event_json(format!("{error:#}")));
        } else {
            eprintln!("{error:#}");
        }
        std::process::exit(1);
    }
}

fn execute(cli: Cli) -> Result<()> {
    let config = CliConfig::load(cli.config.as_deref())?;

    match cli.command {
        Commands::Protect(args) => run_protect(config.merge_protect(args)?)?,
        Commands::Sign(args) => run_sign(config.merge_sign(args)?)?,
        Commands::CheckApk { path } => {
            let result = run_check_apk(path)?;
            println!("{result}");
        }
        Commands::CheckKeystore { ks, alias, ks_pass } => {
            let result = run_check_keystore(ks, alias, ks_pass)?;
            println!("{result}");
        }
    }

    Ok(())
}
