use tokactor::{
    util::{read::Read, tcp::TcpWriter},
    Actor, Ask, Ctx, DeadActorResult, Handler, TcpRequest, World,
};

struct Connection {
    writer: TcpWriter,
}
impl Actor for Connection {}

impl Ask<Read<1024>> for Connection {
    type Result = Result<Option<Vec<u8>>, std::io::Error>;

    fn handle(&mut self, message: Read<1024>, _: &mut Ctx<Self>) -> Self::Result {
        Ok(Some(message.to_vec()))
    }
}

impl Handler<Vec<u8>> for Connection {
    fn handle(&mut self, message: Vec<u8>, context: &mut Ctx<Self>) {
        println!("{}", String::from_utf8(message.clone()).unwrap());
        let writer = self.writer.clone();
        context.anonymous_task(async move {
            let _ = writer.write(message).await;
        });
    }
}

#[derive(Default)]
struct Router {}
impl Actor for Router {}
impl Ask<TcpRequest> for Router {
    type Result = Result<Connection, std::io::Error>;

    fn handle(&mut self, req: TcpRequest, _: &mut Ctx<Self>) -> Self::Result {
        Ok(Connection { writer: req.0 })
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
