use std::{collections::HashMap, future::Future, pin::Pin};

use tokactor::{
    util::{
        io::{DataFrameReceiver, Writer},
        read::Read,
    },
    Actor, ActorRef, Ask, AskResult, AsyncAsk, Ctx, DeadActorResult, Handler, TcpRequest, World,
};
use tracing::Level;

#[derive(Default)]
struct Broadcaster {
    map: HashMap<usize, Writer>,
}
impl Actor for Broadcaster {}

impl Handler<(usize, Writer)> for Broadcaster {
    fn handle(&mut self, message: (usize, Writer), _context: &mut Ctx<Self>) {
        self.map.insert(message.0, message.1);
    }
}

impl Handler<Vec<u8>> for Broadcaster {
    fn handle(&mut self, message: Vec<u8>, context: &mut Ctx<Self>) {
        let addresses = self.map.values().map(Clone::clone).collect::<Vec<_>>();

        context.anonymous_task(async move {
            for address in addresses {
                let _ = address.write(message.clone()).await;
            }
        });
    }
}

impl Handler<usize> for Broadcaster {
    fn handle(&mut self, message: usize, _context: &mut Ctx<Self>) {
        self.map.remove(&message);
    }
}

struct Connection {
    id: usize,
    broadcaster: ActorRef<Broadcaster>,
}
impl Actor for Connection {}

impl AsyncAsk<Data> for Connection {
    type Output = ();
    type Future<'a> = Pin<Box<dyn Future<Output = Self::Output> + Send + Sync + 'a>>;

    fn handle<'a>(&'a mut self, Data(message): Data, _: &mut Ctx<Self>) -> Self::Future<'a> {
        let broadcaster = self.broadcaster.clone();
        Box::pin(async move {
            broadcaster.send_async(message).await.unwrap();
        })
    }
}

#[derive(Default)]
struct Router {
    counter: usize,
    broadcaster: Option<ActorRef<Broadcaster>>,
}
impl Actor for Router {
    fn on_start(&mut self, ctx: &mut Ctx<Self>)
    where
        Self: Actor,
    {
        self.broadcaster = Some(ctx.spawn(Broadcaster::default()));
    }
}
impl Ask<TcpRequest> for Router {
    type Result = Connection;

    fn handle(&mut self, message: TcpRequest, context: &mut Ctx<Self>) -> AskResult<Self::Result> {
        let conn = Connection {
            id: self.counter,
            broadcaster: self.broadcaster.as_ref().unwrap().clone(),
        };

        let counter = self.counter;
        let broadcaster = self.broadcaster.as_ref().unwrap().clone();
        context.anonymous_task(async move {
            broadcaster.send_async((counter, message.0)).await.unwrap();
        });

        self.counter += 1;
        AskResult::Reply(conn)
    }
}

impl Handler<DeadActorResult<Broadcaster>> for Router {
    fn handle(&mut self, dead: DeadActorResult<Broadcaster>, _context: &mut Ctx<Self>) {
        match dead {
            Ok(_bcast) => {
                // how do we delete this?
            }
            Err(error) => {
                println!("Broadcast actor died unexpectingly with {:?}", error);
            }
        }
    }
}

impl Handler<DeadActorResult<Connection>> for Router {
    fn handle(&mut self, dead: DeadActorResult<Connection>, ctx: &mut Ctx<Self>) {
        match dead {
            Ok(conn) => {
                // how do we delete this?
                let broadcaster = self.broadcaster.as_ref().unwrap().clone();
                ctx.anonymous_task(async move {
                    broadcaster.send_async(conn.actor.id).await.unwrap();
                });
            }
            Err(error) => {
                println!("Connection actor died unexpectingly with {:?}", error);
            }
        }
    }
}

#[derive(Default, Debug)]
struct Data(Vec<u8>);

impl DataFrameReceiver for Data {
    type Request = Self;
    type Frame = Read<1024>;

    fn recv(&mut self, frame: &Self::Frame) -> Option<Self::Request> {
        Some(Data(frame.to_vec()))
    }
}

fn main() {
    tracing_subscriber::fmt()
        .pretty()
        // all spans/events with a level higher than TRACE (e.g, info, warn, etc.)
        // will be written to stdout.
        .with_max_level(Level::TRACE)
        .with_writer(std::io::stdout)
        // sets this to be the default, global collector for this application.
        .init();

    tracing::info!("Starting up...");

    let mut world = World::new().unwrap();

    let tcp_input = world
        .tcp_component::<Connection, Data>("127.0.0.1:8080", Router::default())
        .unwrap();

    world.on_input(tcp_input);

    world.block_until_completion();

    println!("Completed! Shutting down...");
}
