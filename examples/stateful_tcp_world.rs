use std::{future::Future, pin::Pin, sync::Arc};

use tokactor::{
    util::{
        io::{DataFrameReceiver, Writer},
        read::Read,
    },
    Actor, ActorContext, Ask, AsyncAsk, Ctx, DeadActorResult, Handler, TcpRequest, World,
};
use tracing::Level;

struct Connection {
    _state: State,
    write: Writer,
}
impl Actor for Connection {}

impl AsyncAsk<Data> for Connection {
    type Output = ();
    type Future<'a> = Pin<Box<dyn Future<Output = Self::Output> + Send + Sync + 'a>>;

    fn handle<'a>(&'a mut self, Data(message): Data, _: &mut Ctx<Self>) -> Self::Future<'a> {
        let writer = self.write.clone();
        Box::pin(async move {
            let _ = writer.write(message).await;
        })
    }
}

struct Router {
    state: State,
}

impl Router {
    fn new(state: State) -> Self {
        Self { state }
    }
}
impl Actor for Router {}
impl Ask<TcpRequest> for Router {
    type Result = Connection;

    fn handle(&mut self, message: TcpRequest, _: &mut Ctx<Self>) -> Self::Result {
        Connection {
            write: message.0,
            _state: self.state.clone(),
        }
    }
}

impl Handler<DeadActorResult<Connection>> for Router {
    fn handle(&mut self, _: DeadActorResult<Connection>, _: &mut Ctx<Self>) {}
}

#[derive(Clone)]
struct State(Arc<()>);
impl Actor for State {}
impl Handler<()> for State {
    fn handle(&mut self, _: (), context: &mut Ctx<Self>) {
        context.stop();
    }
}

async fn compute_state() -> State {
    let address = State(Arc::new(())).start();
    let _ = address.send(());
    let output = address.wait_for_completion().await;
    output.unwrap()
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

    let state = world.with_state(compute_state());

    let r1 = Router::new(state.clone());
    let r2 = Router::new(state);

    let tcp1 = world
        .tcp_component::<Connection, Data>("127.0.0.1:8080", r1)
        .unwrap();
    let tcp2 = world
        .tcp_component::<Connection, Data>("127.0.0.1:8081", r2)
        .unwrap();

    world.on_input(tcp1);
    world.on_input(tcp2);

    world.block_until_completion();
}
