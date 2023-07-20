use std::{future::Future, pin::Pin};

use tokactor::{
    util::{
        io::{DataFrameReceiver, Writer},
        read::Read,
    },
    Actor, Ask, AsyncAsk, Ctx, TcpRequest, World,
};
use tracing::Level;

struct Connection {
    writer: Writer,
}
impl Actor for Connection {}

impl AsyncAsk<Data> for Connection {
    type Output = ();
    type Future = Pin<Box<dyn Future<Output = Self::Output> + Send + Sync>>;

    fn handle(&mut self, Data(msg): Data, _: &mut Ctx<Self>) -> Self::Future {
        println!("{}", String::from_utf8(msg.clone()).unwrap());
        let writer = self.writer.clone();
        Box::pin(async move {
            let _ = writer.write(msg).await;
        })
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

struct Router {}
impl Actor for Router {}
impl Ask<TcpRequest> for Router {
    type Result = Connection;

    fn handle(&mut self, message: TcpRequest, _: &mut Ctx<Self>) -> Self::Result {
        Connection { writer: message.0 }
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
        .tcp_component::<Connection, Data>("127.0.0.1:8080", Router {})
        .unwrap();

    world.on_input(tcp_input);

    world.block_until_completion();

    println!("Completed! Shutting down...");
}
