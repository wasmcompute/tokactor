use std::{future::Future, pin::Pin};

use http::Request;
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

impl AsyncAsk<Request<()>> for Connection {
    type Output = ();
    type Future<'a> = Pin<Box<dyn Future<Output = Self::Output> + Send + Sync + 'a>>;

    fn handle<'a>(&'a mut self, req: Request<()>, _: &mut Ctx<Self>) -> Self::Future<'a> {
        println!("{:?}", req);
        let writer = &self.writer;
        Box::pin(async move {
            let str = format!(
                "HTTP/1.1 200 OK\r\nContent-Type:text/html\r\n\r\n<h1>{:?}</h1>",
                req.headers().get("Host")
            );
            let _ = writer.write(str.into_bytes()).await;
        })
    }
}

#[derive(Debug, Default)]
struct Data {
    data: Vec<u8>,
}

impl DataFrameReceiver for Data {
    type Request = Request<()>;
    type Frame = Read<1024>;

    fn recv(&mut self, frame: &Self::Frame) -> Option<Self::Request> {
        self.data.extend_from_slice(frame.as_slice());
        println!("{:?}", self.data);
        let mut headers = [httparse::EMPTY_HEADER; 64];
        let mut request = httparse::Request::new(&mut headers);
        match request.parse(&self.data).unwrap() {
            httparse::Status::Complete(_) => {
                let mut builder = Request::builder()
                    .method(request.method.unwrap())
                    .uri(request.path.unwrap());
                for header in request.headers {
                    builder = builder.header(header.name, header.value);
                }
                Some(builder.body(()).unwrap())
            }
            httparse::Status::Partial => None,
        }
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
