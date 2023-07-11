use tokactor::{
    util::read::Read, Actor, Ask, AsyncAsk, Ctx, DeadActorResult, Handler, TcpRequest, World,
};

#[derive(Debug)]
struct Payload(Vec<u8>);

struct Connection {}
impl Actor for Connection {}

impl Ask<Read<1024>> for Connection {
    type Result = Result<Option<Payload>, std::io::Error>;

    fn handle(&mut self, message: Read<1024>, _: &mut Ctx<Self>) -> Self::Result {
        Ok(Some(Payload(message.to_vec())))
    }
}

impl AsyncAsk<Payload> for Connection {
    type Result = Vec<u8>;

    fn handle(
        &mut self,
        message: Payload,
        context: &mut Ctx<Self>,
    ) -> tokactor::AsyncHandle<Self::Result> {
        println!("{}", String::from_utf8(message.0.clone()).unwrap());
        context.anonymous_handle(async move { message.0 })
    }
}

#[derive(Default)]
struct Router {}
impl Actor for Router {}
impl Ask<TcpRequest> for Router {
    type Result = Result<Connection, std::io::Error>;

    fn handle(&mut self, _: TcpRequest, _: &mut Ctx<Self>) -> Self::Result {
        Ok(Connection {})
    }
}

impl Handler<DeadActorResult<Connection>> for Router {
    fn handle(&mut self, dead: DeadActorResult<Connection>, _: &mut Ctx<Self>) {
        if let Err(error) = dead {
            println!("Connection actor died unexpectingly with {:?}", error);
        }
    }
}

fn main() {
    println!("Starting up...");
    World::<()>::new()
        .unwrap()
        .with_tcp_input("127.0.0.1:8080", Router::default())
        .block_until_completion();
    println!("Completed! Shutting down...");
}
