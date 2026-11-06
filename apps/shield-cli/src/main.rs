use anyhow::Result;
use clap::Parser;

mod args;
mod automation;
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
    match execute(cli) {
        Ok(code) if code != 0 => std::process::exit(code),
        Ok(_) => {}
        Err(error) => {
            if machine_output {
                println!("{}", error_event_json(format!("{error:#}")));
            } else {
                eprintln!("{error:#}");
            }
            std::process::exit(1);
        }
    }
}

fn execute(cli: Cli) -> Result<i32> {
    let load_config = || CliConfig::load(cli.config.as_deref());
    match cli.command {
        #[cfg(all(feature = "agent-executor", target_os = "macos"))]
        Commands::Agent(args) => {
            return Ok(automation::execution::run_agent(
                args,
                cli.config.as_deref(),
            ))
        }
        #[cfg(all(feature = "agent-executor", target_os = "macos"))]
        Commands::AgentProbe(args) => {
            return Ok(automation::execution::run_probe(args, cli.config.is_some()))
        }
        #[cfg(all(feature = "agent-executor", target_os = "macos"))]
        Commands::AgentWorker(args) => {
            return Ok(automation::execution::run_worker(
                args,
                cli.config.is_some(),
            ))
        }
        Commands::Doctor(args) => return Ok(automation::run_doctor(args, cli.config.is_some())),
        Commands::Preflight(args) => {
            return Ok(automation::run_preflight(args, cli.config.is_some()))
        }
        Commands::PlanProtect(args) => {
            return Ok(automation::run_plan_protect(args, cli.config.as_deref()))
        }
        Commands::Protect(args) => run_protect(load_config()?.merge_protect(args)?)?,
        Commands::Sign(args) => run_sign(load_config()?.merge_sign(args)?)?,
        Commands::CheckApk { path } => {
            load_config()?;
            let result = run_check_apk(path)?;
            println!("{result}");
        }
        Commands::CheckKeystore { ks, alias, ks_pass } => {
            load_config()?;
            let result = run_check_keystore(ks, alias, ks_pass)?;
            println!("{result}");
        }
    }

    Ok(0)
}
