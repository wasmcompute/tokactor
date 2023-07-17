use std::{
    error::Error,
    future::{pending, Future},
    pin::Pin,
};

use tokio::{
    io::{AsyncRead, AsyncWrite, AsyncWriteExt},
    sync::{mpsc, oneshot, watch},
};

use crate::{context::SupervisorMessage, Actor, AsyncAsk, Message};

mod fs;
pub mod read;
pub mod tcp;

pub trait DataFrameReceiver: Sized + Default + Message + Send + Sync + 'static {
    type Request: Message;
    type Frame: DataFrame;

    fn recv(&mut self, frame: &Self::Frame) -> Option<Self::Request>;
}

pub trait DataFrame: Sized {
    fn frame(&self) -> &[u8];
}

pub trait ComponentReader<DFR: DataFrameReceiver>: Send + Sync + 'static {
    type Frame: Default + Message + std::fmt::Debug;
    type Future: Future<Output = std::io::Result<Option<DFR::Request>>> + Send;

    fn read(&self) -> Self::Future;
}

pub trait ComponentFuture {
    type Payload: DataFrameReceiver;
    type Reader: ComponentReader<Self::Payload>;
    type Actor: Actor + AsyncAsk<<Self::Payload as DataFrameReceiver>::Request>;
    type Error: Error + Send + Sync;
    type Future<'a>: Future<Output = Result<(Self::Reader, Self::Actor), Self::Error>>
        + Send
        + Sync
        + 'a;
}

pub trait Component: ComponentFuture + Send + Sync {
    type Shutdown: Future<Output = ()> + Send + Sync;

    #[allow(clippy::needless_lifetimes)]
    fn accept<'a>(&'a mut self) -> Self::Future<'a>;

    fn shutdown(self) -> Self::Shutdown;
}

pub trait IoReadFuture {
    type Future<'a>: Future<Output = Result<Option<()>, std::io::Error>> + Send + 'a;
}

pub trait IoRead<Stream: AsyncRead>:
    IoReadFuture + Default + std::fmt::Debug + Send + Sync + 'static
{
    fn read<'a>(&'a mut self, stream: &'a mut Stream) -> Self::Future<'a>;
}

#[derive(Debug)]
pub struct Reader<R: Default, O> {
    inner: mpsc::Sender<(R, oneshot::Sender<std::io::Result<O>>)>,
}

impl<R: Default, O> Clone for Reader<R, O> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<R: Default, O> Reader<R, O> {
    fn new(inner: mpsc::Sender<(R, oneshot::Sender<std::io::Result<O>>)>) -> Self {
        Self { inner }
    }

    pub async fn read(&self) -> std::io::Result<Option<O>> {
        let (tx, rx) = oneshot::channel();
        if (self.inner.send((R::default(), tx)).await).is_err() {
            // read tcp actor is dead (probably). Return an error
            Ok(None)
        } else {
            match rx.await {
                Ok(response) => response.map(Some),
                Err(_) => Ok(None),
            }
        }
    }
}

impl<Pay, R> ComponentReader<Pay> for Reader<R, Pay::Request>
where
    Pay: DataFrameReceiver<Frame = R>,
    R: Default + Message + std::fmt::Debug + Send + Sync + 'static,
{
    type Frame = R;
    type Future = Pin<Box<dyn Future<Output = std::io::Result<Option<Pay::Request>>> + Send>>;

    fn read(&self) -> Self::Future {
        let this = (*self).clone();
        Box::pin(async move { this.read().await })
    }
}

#[derive(Clone, Debug)]
pub struct Writer {
    inner: mpsc::Sender<(Vec<u8>, oneshot::Sender<std::io::Result<()>>)>,
}

impl Writer {
    fn new(inner: mpsc::Sender<(Vec<u8>, oneshot::Sender<std::io::Result<()>>)>) -> Self {
        Self { inner }
    }

    pub async fn write(&self, data: Vec<u8>) -> std::io::Result<()> {
        let (tx, rx) = oneshot::channel();
        if let Err(err) = self.inner.send((data, tx)).await {
            Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, err))
        } else {
            match rx.await {
                Ok(result) => result,
                Err(err) => Err(std::io::Error::new(std::io::ErrorKind::Other, err)),
            }
        }
    }
}

async fn supervisor<F: Future>(
    rx: &mut watch::Receiver<Option<SupervisorMessage>>,
    shutdown: impl Future,
    fut: F,
) -> Supervise<F::Output> {
    let output = tokio::select! {
        result = rx.changed() => {
            if result.is_err() {
                // supervisor died. time to shutdown
                return Supervise::Dead
            }
            match *rx.borrow() {
                Some(SupervisorMessage::Shutdown) => Supervise::Shutdown,
                None => Supervise::Continue
            }
        },
        _ = shutdown => Supervise::Shutdown,
        option = fut => Supervise::Success(option)
    };
    output
}

enum Supervise<Output> {
    Success(Output),
    Continue,
    Shutdown,
    Dead,
}

async fn create_reader_actor<
    Pipe: AsyncRead + Unpin,
    R: IoRead<Pipe> + Send,
    Payload: DataFrameReceiver<Frame = R>,
>(
    mut source: Pipe,
    mut reader_rx: mpsc::Receiver<(R, oneshot::Sender<std::io::Result<Payload::Request>>)>,
    mut parent_rx: watch::Receiver<Option<SupervisorMessage>>,
    shutdown_tx: oneshot::Sender<()>,
) {
    loop {
        // Wait until we receive a `Read` Event with our object that is readable
        let (mut frame, sender) =
            match supervisor(&mut parent_rx, pending::<()>(), reader_rx.recv()).await {
                Supervise::Success(Some(output)) => output,
                Supervise::Continue => continue,
                Supervise::Success(None) | Supervise::Dead | Supervise::Shutdown => break,
            };

        let mut payload = Payload::default();
        loop {
            // Then, read the object from the stream we have access to
            match supervisor(&mut parent_rx, pending::<()>(), frame.read(&mut source)).await {
                Supervise::Success(Ok(Some(_))) => {
                    if let Some(object) = payload.recv(&frame) {
                        let _ = sender.send(Ok(object));
                        break;
                    }
                }
                Supervise::Continue => continue, // TODO(Alec): Won't this drop some messages?
                Supervise::Success(Err(_))
                | Supervise::Success(Ok(None))
                | Supervise::Dead
                | Supervise::Shutdown => break,
            };
        }
    }
    let _ = shutdown_tx.send(());
}

async fn create_writer_actor(
    mut source: impl AsyncWrite + Unpin,
    mut writer_rx: mpsc::Receiver<(Vec<u8>, oneshot::Sender<std::io::Result<()>>)>,
    mut parent_rx: watch::Receiver<Option<SupervisorMessage>>,
    mut shutdown_rx: oneshot::Receiver<()>,
) {
    loop {
        let (data, sender) =
            match supervisor(&mut parent_rx, &mut shutdown_rx, writer_rx.recv()).await {
                Supervise::Success(Some(output)) => output,
                Supervise::Continue => continue,
                Supervise::Success(None) | Supervise::Dead | Supervise::Shutdown => break,
            };

        match supervisor(&mut parent_rx, &mut shutdown_rx, source.write_all(&data)).await {
            Supervise::Success(result) => {
                let _ = sender.send(result);
            }
            Supervise::Continue => continue, // TODO(Alec): Won't this drop some messages?
            Supervise::Dead | Supervise::Shutdown => break,
        };
    }
}
