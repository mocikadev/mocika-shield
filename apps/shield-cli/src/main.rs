use anyhow::Result;
use clap::Parser;

mod args;
mod automation;
mod cli_json;
mod commands;
mod config;
mod management;
#[cfg(all(feature = "mcp-server", target_os = "macos"))]
mod mcp;

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
    #[cfg(all(feature = "agent-executor", target_os = "macos"))]
    let shared_agent = matches!(&cli.command, Commands::Agent(_));
    #[cfg(not(all(feature = "agent-executor", target_os = "macos")))]
    let shared_agent = false;
    #[cfg(all(feature = "mcp-server", target_os = "macos"))]
    let shared_mcp = matches!(&cli.command, Commands::Mcp(_));
    #[cfg(not(all(feature = "mcp-server", target_os = "macos")))]
    let shared_mcp = false;
    let shared_sign = matches!(&cli.command, Commands::Sign(args) if args.certificate_id.is_some() || args.use_default_certificate);
    if cli.app_home.is_some()
        && !matches!(
            &cli.command,
            Commands::Cert(_) | Commands::Config(_) | Commands::Protect(_)
        )
        && !shared_sign
        && !shared_agent
        && !shared_mcp
    {
        return Ok(management::unsupported_home());
    }
    let load_config = || CliConfig::load(cli.config.as_deref());
    match cli.command {
        Commands::Cert(args) => {
            return Ok(management::run_certificates(
                args,
                cli.app_home.as_deref(),
                cli.config.is_some(),
            ))
        }
        Commands::Config(args) => {
            return Ok(management::run_config(
                args,
                cli.app_home.as_deref(),
                cli.config.is_some(),
            ))
        }
        #[cfg(all(feature = "mcp-server", target_os = "macos"))]
        Commands::Mcp(args) => {
            return Ok(mcp::run(
                args,
                cli.config.as_deref(),
                cli.app_home.as_deref(),
            ))
        }
        #[cfg(all(feature = "agent-executor", target_os = "macos"))]
        Commands::Agent(args) => {
            return Ok(automation::execution::run_agent(
                args,
                cli.config.as_deref(),
                cli.app_home.as_deref(),
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
        Commands::Protect(args) => {
            let config = load_config().map_err(|_| {
                anyhow::anyhow!("无法读取或解析加固项目配置；请在本机检查，不会输出文件原文")
            })?;
            run_protect(config.resolve_protect(args, cli.app_home.as_deref())?)?;
        }
        Commands::Sign(args) if shared_sign => {
            let config = load_config().map_err(|_| {
                anyhow::anyhow!("共享签名无法读取或解析项目配置；不会输出文件原文，请在本机检查")
            })?;
            run_sign(management::signing::resolve(
                &config,
                args,
                cli.app_home.as_deref(),
            )?)?;
        }
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
