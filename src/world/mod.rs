pub mod builder;
pub mod messages;
pub mod tcp;

use std::{future::Future, pin::Pin};

use tokio::{
    net::{TcpListener, ToSocketAddrs},
    runtime::Runtime,
    sync::{broadcast, mpsc},
};

use crate::{
    executor::{Executor, ExecutorLoop},
    Actor, ActorRef, Ask, Ctx, DeadActorResult, Handler, IntoFutureError, Message,
};

use self::{
    messages::{read::TcpReadable, TcpRequest},
    tcp::{ReadResult, TcpReader},
};

use tcp::tcp_raw_actor;

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
        ConnAct:
            Actor + Ask<Msg, Result = Result<Option<Payload>, std::io::Error>> + Handler<Payload>,
        Msg: messages::read::TcpReadable + std::fmt::Debug + Send + Sync,
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
                            let (read, write) = executor.with_ctx(|ctx| tcp_raw_actor::<RouterAct, Msg>(ctx, stream));
                            let handler = executor.ask(TcpRequest(write, addr))?;
                            let address = handler.start();
                            // This task is a diangling task. It represents the
                            // lifetime of the connection
                            tokio::spawn(async move {
                                match tcp_connection(read, address).await {
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
                println!("Shut down complete");
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
    }
}

async fn tcp_connection<Msg, Act, Payload>(
    read: TcpReader<Msg>,
    address: ActorRef<Act>,
) -> Result<Option<Act>, Box<dyn std::error::Error>>
where
    Payload: Message + std::fmt::Debug,
    Msg: TcpReadable + std::fmt::Debug + Send + Sync,
    Act: Actor + Ask<Msg, Result = Result<Option<Payload>, std::io::Error>> + Handler<Payload>,
{
    loop {
        let result = loop {
            let message = match read.read().await {
                ReadResult::Data(reader) => reader,
                ReadResult::Closed | ReadResult::NoResponse => match address.await {
                    Ok(actor) => return Ok(Some(actor)),
                    Err(IntoFutureError::MailboxClosed) => {
                        // we are shutting down from the supervisor down
                        return Err(Box::new(IntoFutureError::MailboxClosed));
                    }
                    Err(IntoFutureError::Paniced) => {
                        return Err(Box::new(IntoFutureError::Paniced))
                    }
                },
            };
            if message.is_closed() {
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

        if result.as_ref().is_err() {
            address.await?;
            return Err(Box::new(result.unwrap_err()));
        }

        match address.send_async(result.unwrap()).await {
            Ok(()) => {}
            Err(crate::SendError::Closed(_)) => {
                // The actors mailbox is closed. Actor stopped accepting messages. Drop the stream
                return Ok(None);
            }
            Err(_) => {
                // because we are using the async version, this method is unreacable
                unreachable!()
            }
        };
    }
}
