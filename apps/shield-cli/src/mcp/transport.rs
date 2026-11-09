//! 标准流的断开和阻塞边界；不解析业务消息，也不判断任务成功。
use std::{
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::time::Sleep;
use tokio_util::sync::CancellationToken;

pub(super) struct DisconnectReader<R> {
    pub input: R,
    pub shutdown: CancellationToken,
}
impl<R: AsyncRead + Unpin> AsyncRead for DisconnectReader<R> {
    fn poll_read(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        let before = buffer.filled().len();
        let available = buffer.remaining();
        let result = Pin::new(&mut this.input).poll_read(context, buffer);
        if matches!(&result, Poll::Ready(Err(_)))
            || matches!(&result, Poll::Ready(Ok(())))
                && available > 0
                && buffer.filled().len() == before
        {
            this.shutdown.cancel();
        }
        result
    }
}
pub(super) struct DeadlineWriter<W> {
    output: W,
    shutdown: CancellationToken,
    deadline: Option<Pin<Box<Sleep>>>,
    allowance: Duration,
}
impl<W> DeadlineWriter<W> {
    pub fn new(output: W, shutdown: CancellationToken) -> Self {
        Self {
            output,
            shutdown,
            deadline: None,
            allowance: Duration::from_secs(2),
        }
    }
    fn ready(&mut self, context: &mut Context<'_>) -> std::io::Result<()> {
        use std::future::Future;
        let deadline = self
            .deadline
            .get_or_insert_with(|| Box::pin(tokio::time::sleep(self.allowance)));
        if self.shutdown.is_cancelled() || deadline.as_mut().poll(context).is_ready() {
            self.shutdown.cancel();
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "协议输出未能按期完成",
            ));
        }
        Ok(())
    }
    fn observe<T>(&self, result: &Poll<std::io::Result<T>>) {
        if matches!(result, Poll::Ready(Err(_))) {
            self.shutdown.cancel();
        }
    }
}
impl<W: AsyncWrite + Unpin> AsyncWrite for DeadlineWriter<W> {
    fn poll_write(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let this = self.get_mut();
        this.ready(context)?;
        let result = Pin::new(&mut this.output).poll_write(context, bytes);
        this.observe(&result);
        result
    }
    fn poll_flush(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        this.ready(context)?;
        let result = Pin::new(&mut this.output).poll_flush(context);
        this.observe(&result);
        if matches!(&result, Poll::Ready(Ok(()))) {
            this.deadline = None;
        }
        result
    }
    fn poll_shutdown(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        this.ready(context)?;
        let result = Pin::new(&mut this.output).poll_shutdown(context);
        this.observe(&result);
        result
    }
}

pub(super) async fn monitor(shutdown: CancellationToken) {
    use tokio::signal::unix::{signal, SignalKind};
    let (Ok(mut term), Ok(mut interrupt)) = (
        signal(SignalKind::terminate()),
        signal(SignalKind::interrupt()),
    ) else {
        shutdown.cancel();
        return;
    };
    loop {
        tokio::select! {
            _=shutdown.cancelled()=>return,
            _=term.recv()=>{shutdown.cancel();return;}
            _=interrupt.recv()=>{shutdown.cancel();return;}
            _=tokio::time::sleep(Duration::from_millis(20))=>{
                let mut descriptor=libc::pollfd {fd:libc::STDOUT_FILENO,events:0,revents:0};
                if unsafe {libc::poll(&mut descriptor,1,0)}>0 && descriptor.revents&(libc::POLLHUP|libc::POLLERR|libc::POLLNVAL)!=0 {
                    shutdown.cancel();return;
                }
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn 输出一直不被读取时按期取消而非阻塞清场() {
        use tokio::io::AsyncWriteExt;
        let (output, _unread) = tokio::io::duplex(1);
        let shutdown = CancellationToken::new();
        let mut writer = DeadlineWriter::new(output, shutdown.clone());
        writer.allowance = Duration::from_millis(30);
        assert!(
            tokio::time::timeout(Duration::from_secs(1), writer.write_all(b"fixture"))
                .await
                .unwrap()
                .is_err()
        );
        assert!(shutdown.is_cancelled());
    }
}
