use std::collections::HashMap;

use tokactor::{
    util::{read::Read, tcp::TcpWriter},
    Actor, ActorRef, Ask, Ctx, DeadActorResult, Handler, TcpRequest, World,
};

#[derive(Default)]
struct Broadcaster {
    map: HashMap<usize, TcpWriter>,
}
impl Actor for Broadcaster {}

impl Handler<(usize, TcpWriter)> for Broadcaster {
    fn handle(&mut self, message: (usize, TcpWriter), _context: &mut Ctx<Self>) {
        self.map.insert(message.0, message.1);
    }
}

impl Handler<Vec<u8>> for Broadcaster {
    fn handle(&mut self, message: Vec<u8>, context: &mut Ctx<Self>) {
        let addresses = self.map.values().map(Clone::clone).collect::<Vec<_>>();

        context.anonymous_task(async move {
            for address in addresses {
                address.write(message.clone()).await;
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

impl Ask<Read<1024>> for Connection {
    type Result = Result<Option<Vec<u8>>, std::io::Error>;

    fn handle(&mut self, message: Read<1024>, _: &mut Ctx<Self>) -> Self::Result {
        Ok(Some(message.to_vec()))
    }
}

impl Handler<Vec<u8>> for Connection {
    fn handle(&mut self, message: Vec<u8>, context: &mut tokactor::Ctx<Self>) {
        let broadcaster = self.broadcaster.clone();
        context.anonymous_handle(async move {
            broadcaster.send_async(message).await.unwrap();
        });
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
    type Result = Result<Connection, std::io::Error>;

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
        Ok(conn)
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

fn main() {
    World::<()>::new()
        .unwrap()
        .with_tcp_input("127.0.0.1:8080", Router::default())
        .block_until_completion();
}
