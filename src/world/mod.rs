pub mod builder;
pub mod messages;
mod shutdown;

use std::{future::Future, pin::Pin};

use tokio::{
    net::{TcpListener, ToSocketAddrs},
    runtime::Runtime,
    sync::{broadcast, mpsc},
};

use crate::{
    executor::{Executor, ExecutorLoop},
    Actor, ActorRef, Ask, Ctx, Handler, Message,
};

use self::messages::TcpRequest;

pub struct World<State: Send + Sync + 'static> {
    rt: Runtime,
    state: State,
    inputs: Vec<FnOnceLoop>,
}

impl From<Runtime> for World<()> {
    fn from(value: Runtime) -> Self {
        Self {
            rt: value,
            state: (),
            inputs: Vec::new(),
        }
    }
}

impl Message for WorldResult {}
pub type WorldResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;
type FnOnceLoopResult = Pin<Box<dyn Future<Output = WorldResult> + Send + Sync>>;
type FnOnceLoop = Box<dyn FnOnce() -> FnOnceLoopResult + Send + Sync>;

impl<State: Send + Sync + 'static> World<State> {
    pub fn new() -> std::io::Result<World<()>> {
        let rt = Runtime::new()?;
        Ok(World {
            rt,
            state: (),
            inputs: vec![],
        })
    }

    pub fn with_state<Output, F, Fut>(self, f: F) -> World<Output>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Output>,
        Output: Send + Sync + 'static,
    {
        let output = self.rt.block_on(async move { f().await });
        World {
            rt: self.rt,
            state: output,
            inputs: vec![],
        }
    }

    pub fn with_tcp_input<A, Act>(mut self, address: A, mut actor: Act) -> Self
    where
        A: ToSocketAddrs + Send + Sync + 'static,
        A::Future: Send + Sync,
        Act: Actor + Send + Sync + Ask<TcpRequest, Result = WorldResult>,
    {
        let event_loop: FnOnceLoop = Box::new(move || {
            Box::pin(async move {
                let mut executor = Executor::new(actor, Ctx::<Act>::new()).into_raw();

                executor.raw_start();

                let listener = TcpListener::bind(address).await?;
                loop {
                    tokio::select! {
                        socket = listener.accept() => {
                            let (stream, addr) = socket?;
                            executor.ask(TcpRequest(stream, addr))?;
                        }
                        message = executor.recv() => {
                            let event = executor.process(message).await;
                            if matches!(event, ExecutorLoop::Break) {
                                break
                            }
                        }
                    };

                    match executor.receive_messages().await {
                        ExecutorLoop::Continue => {}
                        ExecutorLoop::Break => {
                            break;
                        }
                    }
                }

                println!("Shutting down");
                executor.raw_shutdown().await;
                Ok(())
            })
        });
        self.inputs.push(event_loop);
        self
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
                let mut reciever = notify_shutdown.subscribe();
                inputs.push(tokio::spawn(async move {
                    let future = input();
                    tokio::select! {
                        result = future => {
                            if let Err(e) = result {
                                println!("Failed to accept input {:?}", e);
                            }
                        }
                        _ = reciever.recv() => {
                            println!("Shutting down");
                        }
                    };
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
        self.rt.shutdown_background();
    }
}

pub struct Listener {}
