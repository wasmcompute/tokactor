use std::collections::HashMap;

use tokactor::{
    util::{
        io::{DataFrameReceiver, Writer},
        read::Read,
    },
    Actor, ActorRef, Ask, AsyncAsk, AsyncHandle, Ctx, DeadActorResult, Handler, TcpRequest, World,
};

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
    type Result = ();

    fn handle(
        &mut self,
        Data(message): Data,
        context: &mut tokactor::Ctx<Self>,
    ) -> AsyncHandle<()> {
        let broadcaster = self.broadcaster.clone();
        context.anonymous_handle(async move {
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

    fn handle(&mut self, message: TcpRequest, context: &mut Ctx<Self>) -> Self::Result {
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
        conn
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
    type Frame = Read<1024>;

    fn recv(&mut self, frame: &Self::Frame) -> Option<Self> {
        Some(Data(frame.to_vec()))
    }
}

fn main() {
    println!("Starting up...");
    let mut world = World::new().unwrap();

    let tcp_input = world
        .tcp_component::<Connection, Read<1024>, Data>("127.0.0.1:8080", Router::default())
        .unwrap();

    world.with_input(tcp_input);

    world.block_until_completion();

    println!("Completed! Shutting down...");
}
