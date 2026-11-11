//! Windows 原生标准流和控制台取消，只在自有隔离进程中调用实际异步组件。
use super::*;
use crate::automation::native_group::{tests::ordinary, ManagedGroup};
use std::{fs, future::Future, path::Path, task::Poll};

pub(in crate::automation) fn run_fixture(root: &Path, mode: &str) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let shutdown = CancellationToken::new();
        let mut watching = Box::pin(monitor(shutdown.clone()));
        // 首次 poll 完成真实信号登记后才发布测试就绪，不依靠定时睡眠猜测处理器已安装。
        std::future::poll_fn(|context| {
            assert!(
                watching.as_mut().poll(context).is_pending(),
                "真实监督器未能启动"
            );
            Poll::Ready(())
        })
        .await;
        let monitor = tokio::spawn(watching);
        let mut group = ManagedGroup::spawn_command(ordinary("writer", root)).unwrap();
        group.close_input();
        let mut output = from_output(group.take_stdout().unwrap());
        {
            use tokio::io::AsyncReadExt;
            let mut first = [0u8; 1];
            tokio::time::timeout(Duration::from_secs(2), output.read_exact(&mut first))
                .await
                .unwrap()
                .unwrap();
        }
        let end = std::time::Instant::now() + Duration::from_secs(8);
        while !root
            .join("heartbeat")
            .metadata()
            .is_ok_and(|info| info.len() > 0)
        {
            assert!(std::time::Instant::now() < end);
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        fs::write(root.join("ready"), b"test").unwrap();
        match mode {
            "async-input" => {
                use tokio::io::AsyncReadExt;
                let mut reader = DisconnectReader {
                    input: tokio::io::stdin(),
                    shutdown: shutdown.clone(),
                };
                assert_eq!(
                    tokio::time::timeout(Duration::from_secs(5), reader.read(&mut [0u8; 1]))
                        .await
                        .unwrap()
                        .unwrap(),
                    0
                );
            }
            "async-output" => {
                use tokio::io::AsyncWriteExt;
                while !root.join("write-now").exists() {
                    assert!(std::time::Instant::now() < end);
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
                let mut writer = DeadlineWriter::new(tokio::io::stdout(), shutdown.clone());
                // Tokio 标准输出会先接收缓冲字节，只有刷新完成才代表底层交付完成。
                let result = match writer.write_all(b"finite probe\n").await {
                    Ok(()) => writer.flush().await,
                    Err(error) => Err(error),
                };
                assert!(result.is_err());
            }
            "async-signal" => {
                tokio::time::timeout(Duration::from_secs(5), shutdown.cancelled())
                    .await
                    .unwrap();
            }
            _ => panic!("未知自有异步夹具模式"),
        }
        assert!(shutdown.is_cancelled());
        group.stop(Duration::from_secs(2)).unwrap();
        tokio::time::timeout(Duration::from_secs(1), monitor)
            .await
            .unwrap()
            .unwrap();
        let cause = match mode {
            "async-input" => "stdin_closed",
            "async-output" => "output_closed",
            _ => "signal",
        };
        fs::write(root.join("cause"), cause).unwrap();
        fs::write(root.join("stopped"), b"test").unwrap();
    });
    runtime.shutdown_timeout(Duration::from_secs(1));
    if mode == "async-output" {
        std::process::exit(3);
    }
}
