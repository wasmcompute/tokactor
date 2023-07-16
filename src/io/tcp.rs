use std::{future::Future, io, marker::PhantomData, pin::Pin, task::Poll};

use tokio::{
    net::{self, tcp::OwnedReadHalf, TcpStream, ToSocketAddrs},
    sync::{mpsc, oneshot},
};

use crate::{
    executor::{Executor, RawExecutor},
    Actor, Ask, AsyncAsk, Ctx, Message, TcpRequest,
};

use super::{
    create_reader_actor, create_writer_actor, Component, ComponentFuture, DataFrameReceiver,
    IoRead, Reader, Writer,
};

pub struct TcpAcceptFut<
    'a,
    RouterAct: Actor,
    ConnAct: Actor,
    Reader: IoRead<OwnedReadHalf> + Send,
    O: DataFrameReceiver<Frame = Reader>,
> {
    listener: &'a net::TcpListener,
    executor: &'a mut RawExecutor<RouterAct>,
    _actor: PhantomData<ConnAct>,
    _reader: PhantomData<Reader>,
    _payload: PhantomData<O>,
}

impl<'a, O, RouterAct, ConnAct, Reader> Unpin for TcpAcceptFut<'a, RouterAct, ConnAct, Reader, O>
where
    RouterAct: Actor + Ask<TcpRequest, Result = ConnAct>,
    ConnAct: Actor,
    Reader: IoRead<OwnedReadHalf> + Default + Send + 'static,
    O: DataFrameReceiver<Frame = Reader>,
{
}

impl<'a, O, RouterAct, ConnAct, Reader> Future for TcpAcceptFut<'a, RouterAct, ConnAct, Reader, O>
where
    RouterAct: Actor + Ask<TcpRequest, Result = ConnAct>,
    ConnAct: Actor,
    Reader: IoRead<OwnedReadHalf> + Default + Send + 'static,
    O: DataFrameReceiver<Frame = Reader>,
{
    type Output = io::Result<(crate::io::Reader<Reader, O>, ConnAct)>;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        if let Poll::Ready(result) = self.listener.poll_accept(cx) {
            let result = match result {
                Ok((stream, address)) => {
                    let this = self.get_mut();
                    let (read, write) = this
                        .executor
                        .with_ctx(move |ctx| tcp_actors::<RouterAct, Reader, O>(ctx, stream));
                    let request = TcpRequest(write, address);
                    let actor = this.executor.ask(request);
                    Ok((read, actor))
                }
                Err(err) => Err(err),
            };
            Poll::Ready(result)
        } else {
            Poll::Pending
        }
    }
}

pub struct TcpListener<
    P: Actor,
    A: Actor,
    R: IoRead<OwnedReadHalf>,
    O: DataFrameReceiver<Frame = R>,
> {
    executor: RawExecutor<P>,
    listener: net::TcpListener,
    _actor: PhantomData<A>,
    _reader: PhantomData<R>,
    _payload: PhantomData<O>,
}

impl<'a, P: Actor, A: Actor, R: IoRead<OwnedReadHalf>, O: DataFrameReceiver<Frame = R>>
    TcpListener<P, A, R, O>
{
    pub async fn new(
        address: impl ToSocketAddrs,
        parent: P,
    ) -> io::Result<TcpListener<P, A, R, O>> {
        let listener = net::TcpListener::bind(address).await?;
        let mut executor = Executor::new(parent, Ctx::<P>::new()).into_raw();
        executor.raw_start();
        Ok(Self {
            executor,
            listener,
            _actor: PhantomData,
            _reader: PhantomData,
            _payload: PhantomData,
        })
    }
}

impl<P, A, R, O> ComponentFuture for TcpListener<P, A, R, O>
where
    P: Actor + Ask<TcpRequest, Result = A>,
    A: Actor + AsyncAsk<O>,
    R: IoRead<OwnedReadHalf> + Default + Message + std::fmt::Debug + Send + Sync + 'static,
    O: DataFrameReceiver<Frame = R>,
{
    type Payload = O;
    type Reader = crate::io::Reader<R, O>;
    type Actor = A;
    type Error = std::io::Error;
    type Future<'a> = TcpAcceptFut<'a, P, A, R, O>;
}

impl<P, A, R, O> Component for TcpListener<P, A, R, O>
where
    P: Actor + Ask<TcpRequest, Result = A>,
    A: Actor + AsyncAsk<O>,
    R: IoRead<OwnedReadHalf> + Default + Message + std::fmt::Debug + Send + Sync + 'static,
    O: DataFrameReceiver<Frame = R>,
{
    type Shutdown = Pin<Box<dyn Future<Output = ()> + Send + Sync + 'static>>;

    #[allow(clippy::needless_lifetimes)]
    fn accept<'a>(&'a mut self) -> Self::Future<'a> {
        TcpAcceptFut {
            listener: &self.listener,
            executor: &mut self.executor,
            _actor: PhantomData,
            _reader: PhantomData,
            _payload: PhantomData,
        }
    }

    fn shutdown(self) -> Self::Shutdown {
        Box::pin(async move { self.executor.raw_shutdown().await })
    }
}

fn tcp_actors<
    A: Actor,
    R: IoRead<OwnedReadHalf> + Default + Send + 'static,
    Payload: DataFrameReceiver<Frame = R>,
>(
    ctx: &Ctx<A>,
    stream: TcpStream,
) -> (Reader<R, Payload>, Writer) {
    let (read, write) = stream.into_split();
    let (reader_tx, reader_rx) =
        mpsc::channel::<(R, oneshot::Sender<std::io::Result<Payload>>)>(10);
    let (writer_tx, writer_rx) =
        mpsc::channel::<(Vec<u8>, oneshot::Sender<std::io::Result<()>>)>(10);
    let (shutdown_tx, shutdown_rx) = oneshot::channel();

    let parent_rx = ctx.notifier.subscribe();
    tokio::spawn(create_reader_actor::<OwnedReadHalf, R, Payload>(
        read,
        reader_rx,
        parent_rx,
        shutdown_tx,
    ));

    let parent_rx = ctx.notifier.subscribe();
    tokio::spawn(create_writer_actor(
        write,
        writer_rx,
        parent_rx,
        shutdown_rx,
    ));

    let reader = Reader::<R, Payload>::new(reader_tx);
    let writer = Writer::new(writer_tx);

    (reader, writer)
}
