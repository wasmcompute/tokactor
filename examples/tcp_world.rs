use tokactor::{
    util::{
        io::{DataFrameReceiver, Writer},
        read::Read,
    },
    Actor, Ask, AsyncAsk, AsyncHandle, Ctx, TcpRequest, World,
};

struct Connection {
    writer: Writer,
}
impl Actor for Connection {}

impl AsyncAsk<Data> for Connection {
    type Result = ();
    fn handle(&mut self, Data(msg): Data, context: &mut Ctx<Self>) -> AsyncHandle<Self::Result> {
        println!("{}", String::from_utf8(msg.clone()).unwrap());
        let writer = self.writer.clone();
        context.anonymous_handle(async move {
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
    println!("Starting up...");
    let mut world = World::new().unwrap();

    let tcp_input = world
        .tcp_component::<Connection, Data>("127.0.0.1:8080", Router {})
        .unwrap();

    world.on_input(tcp_input);

    world.block_until_completion();

    println!("Completed! Shutting down...");
}
