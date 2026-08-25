use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use futures_core::Stream;

use crate::{AsyncQuery, CancellationToken, Error, Result};

const ROW_CHANNEL_CAPACITY: usize = 8;
type ReceiveFuture<T> =
    Pin<Box<dyn Future<Output = std::result::Result<Result<T>, async_channel::RecvError>> + Send>>;

struct AsyncQueryState<T> {
    receiver: Arc<async_channel::Receiver<Result<T>>>,
    receive: ReceiveFuture<T>,
    cancellation: CancellationToken,
    operation_cancellation: CancellationToken,
    cancelled: Pin<Box<dyn Future<Output = ()> + Send>>,
    done: bool,
}

impl<T> AsyncQueryState<T> {
    fn stop(&mut self) {
        self.done = true;
        self.receiver.close();
        self.operation_cancellation.cancel();
    }
}

impl<T> Drop for AsyncQueryState<T> {
    fn drop(&mut self) {
        self.stop();
    }
}

impl<T> Stream for AsyncQueryState<T>
where
    T: Send + 'static,
{
    type Item = Result<T>;

    fn poll_next(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        if this.done {
            return Poll::Ready(None);
        }
        if this.cancellation.is_cancelled() || this.cancelled.as_mut().poll(context).is_ready() {
            this.stop();
            return Poll::Ready(Some(Err(Error::cancelled())));
        }
        match this.receive.as_mut().poll(context) {
            Poll::Ready(Ok(_)) if this.cancellation.is_cancelled() => {
                this.stop();
                Poll::Ready(Some(Err(Error::cancelled())))
            }
            Poll::Ready(Ok(row)) => {
                this.receive = receive(Arc::clone(&this.receiver));
                Poll::Ready(Some(row))
            }
            Poll::Ready(Err(_)) => {
                this.done = true;
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

pub(crate) fn spawn<T, I, F>(factory: F, cancellation: CancellationToken) -> Result<AsyncQuery<T>>
where
    T: Send + 'static,
    I: Iterator<Item = Result<T>> + Send + 'static,
    F: FnOnce() -> Result<I> + Send + 'static,
{
    if cancellation.is_cancelled() {
        return Err(Error::cancelled());
    }

    let operation_cancellation = CancellationToken::new();
    let worker_cancellation = cancellation.clone();
    let worker_operation_cancellation = operation_cancellation.clone();
    let (sender, receiver) = async_channel::bounded(ROW_CHANNEL_CAPACITY);
    std::thread::Builder::new().name("miniexcel-async-query".to_owned()).spawn(move || {
        let error_sender = sender.clone();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<()> {
            let rows = factory()?;
            for row in rows {
                if worker_cancellation.is_cancelled()
                    || worker_operation_cancellation.is_cancelled()
                    || sender.send_blocking(row).is_err()
                {
                    break;
                }
            }
            Ok(())
        }));
        match result {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                let _ = error_sender.send_blocking(Err(error));
            }
            Err(_) => {
                let _ =
                    error_sender.send_blocking(Err(Error::stream("async query worker panicked")));
            }
        }
    })?;

    let cancelled = {
        let cancellation = cancellation.clone();
        Box::pin(async move { cancellation.cancelled().await })
    };
    let receiver = Arc::new(receiver);
    let receive = receive(Arc::clone(&receiver));
    Ok(Box::pin(AsyncQueryState {
        receiver,
        receive,
        cancellation,
        operation_cancellation,
        cancelled,
        done: false,
    }))
}

fn receive<T>(receiver: Arc<async_channel::Receiver<Result<T>>>) -> ReceiveFuture<T>
where
    T: Send + 'static,
{
    Box::pin(async move { receiver.recv().await })
}

#[cfg(test)]
mod tests {
    use futures_executor::block_on;
    use futures_util::StreamExt;

    use super::spawn;
    use crate::{CancellationToken, Result};

    #[test]
    fn reports_worker_panics_as_stream_errors() {
        block_on(async {
            let rows =
                std::iter::from_fn(|| -> Option<Result<usize>> { panic!("query iterator panic") });
            let mut stream = spawn(|| Ok(rows), CancellationToken::new()).unwrap();
            let error = stream.next().await.unwrap().unwrap_err();
            assert!(error.to_string().contains("async query worker panicked"));
            assert!(stream.next().await.is_none());
        });
    }
}
