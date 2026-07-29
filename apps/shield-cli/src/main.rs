use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

mod cli_json;
mod commands;

use commands::{run_check_apk, run_check_keystore, run_protect};

#[derive(Clone, Copy, Default, ValueEnum)]
enum EnvironmentPolicyArg {
    #[default]
    Compatible,
    Strict,
}

#[derive(Parser)]
#[command(
    name = "shield",
    version,
    author,
    about = "Android APK 加固工具",
    arg_required_else_help = true
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Protect {
        #[arg(short, long, value_name = "APK")]
        input: PathBuf,
        #[arg(short, long, value_name = "APK")]
        output: PathBuf,
        /// 内部回归测试使用的运行时资源包。
        #[arg(long, value_name = "ZIP", hide = true)]
        resources: Option<PathBuf>,
        /// 运行时环境策略：兼容模式仅执行反调试，严格模式额外拒绝高置信 Root 环境。
        #[arg(long, value_enum, default_value_t = EnvironmentPolicyArg::Compatible)]
        environment_policy: EnvironmentPolicyArg,
        #[arg(long)]
        json_progress: bool,
        #[arg(short, long)]
        verbose: bool,
    },
    CheckApk {
        path: PathBuf,
    },
    CheckKeystore {
        #[arg(long)]
        ks: PathBuf,
        #[arg(long)]
        alias: String,
        #[arg(long)]
        ks_pass: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Protect {
            input,
            output,
            resources,
            environment_policy,
            json_progress,
            verbose,
        } => run_protect(
            input,
            output,
            resources,
            environment_policy,
            json_progress,
            verbose,
        )?,

        Commands::CheckApk { path } => {
            let result = run_check_apk(path);
            println!("{result}");
        }

        Commands::CheckKeystore { ks, alias, ks_pass } => {
            let result = run_check_keystore(ks, alias, ks_pass);
            println!("{result}");
        }
    }

    Ok(())
}
