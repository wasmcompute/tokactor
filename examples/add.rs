use tokactor::{Actor, Ask, AsyncAsk, AsyncHandle, Ctx, Handler};
use tracing::Level;

#[derive(Debug)]
struct Add(u32);

#[derive(Debug)]
struct Sum;

#[derive(Debug)]
struct Counter {
    inner: u32,
}

impl Actor for Counter {}

impl Handler<Add> for Counter {
    fn handle(&mut self, message: Add, _: &mut Ctx<Self>) {
        self.inner += message.0;
    }
}

impl Ask<Add> for Counter {
    type Result = ();
    fn handle(&mut self, message: Add, _: &mut Ctx<Self>) -> Self::Result {
        self.inner += message.0;
    }
}

impl AsyncAsk<Add> for Counter {
    type Result = ();

    fn handle(&mut self, message: Add, context: &mut Ctx<Self>) -> AsyncHandle<Self::Result> {
        self.inner += message.0;
        context.anonymous_handle(async {})
    }
}

impl Ask<Sum> for Counter {
    type Result = Counter;

    fn handle(&mut self, _: Sum, _: &mut Ctx<Self>) -> Self::Result {
        Counter { inner: self.inner }
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .pretty()
        // all spans/events with a level higher than TRACE (e.g, info, warn, etc.)
        // will be written to stdout.
        .with_max_level(Level::TRACE)
        .with_writer(std::io::stdout)
        // sets this to be the default, global collector for this application.
        .init();

    tracing::info!("Starting up...");

    let addr = Counter { inner: 0 }.start();
    addr.send_async(Add(10)).await.unwrap();
    addr.ask(Add(10)).await.unwrap();
    addr.async_ask(Add(10)).await.unwrap();
    let counter = addr.ask(Sum).await.unwrap();
    println!("Total count should be 30 = {:?}", counter);
}
