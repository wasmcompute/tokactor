use std::future::{pending, Future};

use tokio::{
    io::AsyncWriteExt,
    net::TcpStream,
    sync::{mpsc, oneshot, watch},
};

use crate::{context::SupervisorMessage, util::read::TcpReadable, Actor, Ctx};

#[derive(Clone, Debug)]
pub struct TcpWriter {
    inner: mpsc::Sender<(Vec<u8>, oneshot::Sender<std::io::Result<()>>)>,
}

pub enum WriteResult {
    Ok(std::io::Result<()>),
    Closed,
    NoResponse,
}

impl TcpWriter {
    pub async fn write(&self, data: Vec<u8>) -> WriteResult {
        let (tx, rx) = oneshot::channel();
        if (self.inner.send((data, tx)).await).is_err() {
            WriteResult::Closed
        } else {
            match rx.await {
                Ok(result) => WriteResult::Ok(result),
                Err(_) => WriteResult::NoResponse,
            }
        }
    }
}

pub struct TcpReader<R: TcpReadable> {
    inner: mpsc::Sender<(R, oneshot::Sender<R>)>,
}

pub enum ReadResult<R: TcpReadable> {
    Data(R),
    Closed,
    NoResponse,
}

impl<R: TcpReadable> TcpReader<R> {
    fn new(inner: mpsc::Sender<(R, oneshot::Sender<R>)>) -> Self {
        Self { inner }
    }

    pub async fn read(&self) -> ReadResult<R>
    where
        R: std::fmt::Debug,
    {
        let (tx, rx) = oneshot::channel();
        if (self.inner.send((R::default(), tx)).await).is_err() {
            // read tcp actor is dead (probably). Return an error
            ReadResult::Closed
        } else {
            match rx.await {
                Ok(response) => ReadResult::Data(response),
                Err(_) => ReadResult::NoResponse,
            }
        }
    }
}

enum Supervise<Output> {
    Success(Output),
    Continue,
    Shutdown,
    Dead,
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

pub fn tcp_raw_actor<A, R>(ctx: &Ctx<A>, stream: TcpStream) -> (TcpReader<R>, TcpWriter)
where
    A: Actor,
    R: TcpReadable,
{
    let (read, mut write) = stream.into_split();
    let (read_tx, mut read_rx) = mpsc::channel::<(R, oneshot::Sender<R>)>(10);
    let (write_tx, mut write_rx) =
        mpsc::channel::<(Vec<u8>, oneshot::Sender<std::io::Result<()>>)>(10);
    let (shutdown_tx, shutdown_rx) = oneshot::channel();

    let reciever = ctx.notifier.subscribe();
    // Create the reader side of the TCP pipe
    tokio::spawn(async move {
        let mut read_stream = read;
        let mut rx = reciever;
        loop {
            let (mut reader, sender) =
                match supervisor(&mut rx, pending::<()>(), read_rx.recv()).await {
                    Supervise::Success(Some(output)) => output,
                    Supervise::Continue => continue,
                    Supervise::Success(None) | Supervise::Dead | Supervise::Shutdown => break,
                };

            match supervisor(&mut rx, pending::<()>(), reader.read(&mut read_stream)).await {
                Supervise::Success(Ok(())) => {
                    let _ = sender.send(reader);
                }
                Supervise::Continue => continue, // TODO(Alec): Won't this drop some messages?
                Supervise::Success(Err(_)) | Supervise::Dead | Supervise::Shutdown => break,
            };
        }
        let _ = shutdown_tx.send(());
    });

    let reciever = ctx.notifier.subscribe();
    // Create the writer side of the TCP pipe
    tokio::spawn(async move {
        let mut rx = reciever;
        let mut shutdown_rx = shutdown_rx;
        loop {
            let (data, sender) = match supervisor(&mut rx, &mut shutdown_rx, write_rx.recv()).await
            {
                Supervise::Success(Some(output)) => output,
                Supervise::Continue => continue,
                Supervise::Success(None) | Supervise::Dead | Supervise::Shutdown => break,
            };

            match supervisor(&mut rx, &mut shutdown_rx, write.write_all(&data)).await {
                Supervise::Success(result) => {
                    let _ = sender.send(result);
                }
                Supervise::Continue => continue, // TODO(Alec): Won't this drop some messages?
                Supervise::Dead | Supervise::Shutdown => break,
            };
        }
    });

    let reader = TcpReader::<R>::new(read_tx);
    let writer = TcpWriter { inner: write_tx };

    (reader, writer)
}
