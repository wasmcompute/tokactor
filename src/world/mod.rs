pub mod builder;
pub mod messages;
mod shutdown;

use std::{future::Future, pin::Pin};

use tokio::{
    io::AsyncWriteExt,
    net::{TcpListener, TcpStream, ToSocketAddrs},
    runtime::Runtime,
    sync::{broadcast, mpsc},
};

use crate::{
    executor::{Executor, ExecutorLoop},
    Actor, ActorRef, Ask, AsyncAsk, Ctx, DeadActorResult, Handler, Message,
};

use self::messages::{read::TcpReadable, TcpRequest};

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

pub type WorldResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;
type FnOnceLoopResult = Pin<Box<dyn Future<Output = WorldResult> + Send + Sync>>;
type FnOnceLoop = Box<dyn FnOnce(broadcast::Receiver<()>) -> FnOnceLoopResult + Send + Sync>;

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

    pub fn with_tcp_input<Address, RouterAct, ConnAct, Msg, Payload>(
        mut self,
        address: Address,
        actor: RouterAct,
    ) -> Self
    where
        Address: ToSocketAddrs + Send + Sync + 'static,
        Address::Future: Send + Sync,
        RouterAct: Actor
            + Send
            + Sync
            + Ask<TcpRequest, Result = Result<ConnAct, std::io::Error>>
            + Handler<DeadActorResult<ConnAct>>,
        ConnAct: Actor
            + Ask<Msg, Result = Result<Option<Payload>, std::io::Error>>
            + AsyncAsk<Payload, Result = Vec<u8>>,
        Msg: messages::read::TcpReadable + Send + Sync,
        Payload: Message + std::fmt::Debug,
    {
        // Create the event loop:
        // 1. The actor passed in is a router, they create actors that will handle the request
        // 2. Depending on the Message that is implemented, read the socket until all the contents are read
        let event_loop: FnOnceLoop = Box::new(move |mut shutdown: broadcast::Receiver<()>| {
            Box::pin(async move {
                let mut executor = Executor::new(actor, Ctx::<RouterAct>::new()).into_raw();

                executor.raw_start();

                let listener = TcpListener::bind(address).await?;
                loop {
                    tokio::select! {
                        socket = listener.accept() => {
                            let (stream, addr) = socket?;
                            let handler = executor.ask(TcpRequest(addr))?;
                            let address = executor.spawn(handler);
                            // This task is a diangling task. It represents the
                            // lifetime of the connection
                            tokio::spawn(async move {
                                match tcp_connection(stream, address).await {
                                    Ok(Some(_)) => {
                                        // connection completed successfully
                                        println!("Exited Cleanly");
                                    },
                                    Ok(None) => {
                                        println!("Connection actor did not exit cleanly");
                                    }
                                    Err(err) => {
                                        println!("Connection actor ran into an error when executing control loop {}", err)
                                    }
                                };
                            });
                        }
                        message = executor.recv() => {
                            let event = executor.process(message).await;
                            if matches!(event, ExecutorLoop::Break) {
                                break
                            }
                        }
                        _ = shutdown.recv() => {
                            break;
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
                let reciever = notify_shutdown.subscribe();
                inputs.push(tokio::spawn(async move {
                    if let Err(err) = input(reciever).await {
                        println!("Failed to accept input {:?}", err);
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
        self.rt.shutdown_background();
    }
}

async fn tcp_connection<Msg, Act, Payload>(
    mut stream: TcpStream,
    address: ActorRef<Act>,
) -> Result<Option<Act>, Box<dyn std::error::Error>>
where
    Payload: Message + std::fmt::Debug,
    Msg: TcpReadable + Send + Sync,
    Act: Actor
        + Ask<Msg, Result = Result<Option<Payload>, std::io::Error>>
        + AsyncAsk<Payload, Result = Vec<u8>>,
{
    loop {
        let result = loop {
            let mut message = Msg::default();
            if let Err(error) = message.read(&mut stream).await {
                // we failed to read from the stream. Kill the actor and terminate
                // the actor.
                break Err(error);
            }
            if message.is_closed() {
                // destory the actor
                return Ok(Some(address.await?));
            }
            match address.ask(message).await {
                Ok(result) => match result {
                    Ok(Some(obj)) => break Ok(obj), // value was successfully parsed
                    Ok(None) => {} // value is still not valid enough to complete being parsed
                    Err(error) => break Err(error), // failed to parse object
                },
                Err(crate::AskError::Closed(_)) => {
                    // The actors mailbox is closed. Actor stopped accepting messages. Drop the stream
                    return Ok(None);
                }
                Err(crate::AskError::Dropped) => {
                    // Actor recieved message, but we don't have the object. Stop processing.
                    return Ok(Some(address.await?));
                }
            }
        };

        if let Err(error) = result.as_ref() {
            address.await?;
            return Err(Box::new(result.unwrap_err()));
        }

        match address.async_ask(result.unwrap()).await {
            Ok(data) => {
                stream.write_all(&data).await.unwrap();
                println!("{}", String::from_utf8(data).unwrap());
            }
            Err(crate::AskError::Closed(_)) => {
                // The actors mailbox is closed. Actor stopped accepting messages. Drop the stream
                return Ok(None);
            }
            Err(crate::AskError::Dropped) => {
                // Actor recieved message, but we don't have the object. Stop processing.
                return Ok(Some(address.await?));
            }
        };
    }
}
