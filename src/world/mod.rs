pub mod builder;
pub mod messages;

use std::{future::Future, pin::Pin};

use tokio::{
    net::{tcp::OwnedReadHalf, ToSocketAddrs},
    runtime::Runtime,
    sync::{broadcast, mpsc},
};

use crate::{
    io::{
        tcp::TcpListener, Component, ComponentFuture, ComponentReader, DataFrameReceiver, IoRead,
    },
    Actor, Ask, AsyncAsk, TcpRequest,
};

pub struct World {
    rt: Runtime,
    inputs: Vec<FnOnceLoop>,
}

impl From<Runtime> for World {
    fn from(value: Runtime) -> Self {
        Self {
            rt: value,
            inputs: Vec::new(),
        }
    }
}

pub type WorldResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;
type FnOnceLoopResult = Pin<Box<dyn Future<Output = WorldResult> + Send + Sync>>;
type FnOnceLoop = Box<dyn FnOnce(broadcast::Receiver<()>) -> FnOnceLoopResult + Send + Sync>;

impl World {
    pub fn new() -> std::io::Result<World> {
        let rt = Runtime::new()?;
        Ok(World { rt, inputs: vec![] })
    }

    pub fn with_state<Fut: Future>(&self, f: Fut) -> Fut::Output
where {
        self.rt.block_on(f)
    }

    pub fn tcp_component<A, O>(
        &self,
        address: impl ToSocketAddrs,
        actor: impl Actor + Ask<TcpRequest, Result = A>,
    ) -> std::io::Result<impl Component>
    where
        A: Actor + AsyncAsk<O::Request>,
        O: DataFrameReceiver,
        O::Frame: IoRead<OwnedReadHalf>,
    {
        self.rt
            .block_on(TcpListener::<_, A, O::Frame, O>::new(address, actor))
    }

    pub fn on_input<C: Component + 'static>(&mut self, mut component: C) {
        let event_loop: FnOnceLoop = Box::new(move |mut shutdown: broadcast::Receiver<()>| {
            Box::pin(async move {
                loop {
                    tokio::select! {
                        result = component.accept() => {
                            let (reader, actor) = result.unwrap();
                            let connection = actor.start();
                            tokio::spawn(async move {
                                loop {
                                    match reader.read().await {
                                        Ok(Some(payload)) => {
                                            let _ = connection.async_ask(payload).await;
                                        }
                                        Ok(None) => {
                                            // we can no longer read from the socket
                                            tracing::info!("Disconnected from tcp read pipe");
                                            break;
                                        }
                                        Err(err) => {
                                            tracing::error!(error = err.to_string(), "Disconnected from tcp read pipe");
                                            break;
                                        }
                                    }
                                }
                                if let Err(err) = connection.await {
                                    match err {
                                        crate::address::IntoFutureError::MailboxClosed => tracing::trace!(actor = <C as ComponentFuture>::Actor::name(), "already closed"),
                                        crate::address::IntoFutureError::Paniced => tracing::error!(actor = <C as ComponentFuture>::Actor::name(), "paniced during close"),
                                    }
                                }
                            });
                        }
                        _ = shutdown.recv() => {
                            break;
                        }
                    }
                }
                component.shutdown().await;
                Ok(())
            })
        });
        self.inputs.push(event_loop);
    }

    // pub fn with_terminal_input<Act: Actor + Handler<Stdin>>(mut actor: Act) -> Self {}

    // pub fn on_input<I, Input>(&self, machine: I, callback: F)
    // where
    //     I: FnOnce(),
    //     F: Fn(Input, State),
    // {
    //     let _state = self.state.clone();
    //     self.rt.spawn(async move {
    //         let state = _state;
    //         machine.handle(state);
    //         loop {

    //         }
    //     })
    // }

    /// Block the current runtime until one of the possible senarios complete:
    ///
    /// 1. All inputs are turned off; thus we completed executing the program
    /// 2. Ctrl+C is selected, which we will communicate with the actor system to start cleaning up and completing tasks
    /// 3. Ctrl+C (times 2): We will not wait for the actor system to clean up and will forcefully exit the system
    ///
    /// Upon exiting, the current inputs will be turned off (if they aren't already)
    /// and the runtime will be shutdown.
    pub fn block_until_completion(self) {
        self.rt.block_on(async move {
            let shutdown = tokio::signal::ctrl_c();
            let (notify_shutdown, _) = broadcast::channel::<()>(1);
            let (shutdown_complete_tx, mut shutdown_complete_rx) = mpsc::channel::<()>(1);
            let mut inputs = vec![];

            for input in self.inputs {
                let reciever = notify_shutdown.subscribe();
                inputs.push(tokio::spawn(async move {
                    if let Err(err) = input(reciever).await {
                        tracing::error!(error = err.to_string(), "Failed to accept input");
                    }
                }));
            }

            tokio::spawn(async move {
                let _ = shutdown.await;
                drop(notify_shutdown);
            });

            for input in inputs {
                let _ = input.await;
            }

            drop(shutdown_complete_tx);
            let _ = shutdown_complete_rx.recv().await;
        });
    }
}
