use std::env;
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let mut args = env::args_os().skip(1);
    let input_dir = args
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("缺少原始 DEX 目录"))?;
    let output = args
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("缺少 DEXB 输出路径"))?;
    let signature = args
        .next()
        .and_then(|value| value.into_string().ok())
        .ok_or_else(|| anyhow::anyhow!("缺少签名证书 SHA-256"))?;
    if args.next().is_some() {
        anyhow::bail!("参数过多");
    }

    // 实验仅验证真实格式与加载链路；正式 packer 仍会为每次载荷生成随机 nonce。
    let ikm = [0x42u8; 32];
    shield_core::aab_experiment::pack_dex_files(&input_dir, &output, &ikm, &signature)?;
    println!("DEXB v5 已生成：{}", output.display());
    Ok(())
}
