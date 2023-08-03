use std::{
    future::{ready, Ready},
    time::Duration,
};

use tokactor::{Actor, Ask, AskResult, AsyncAsk, Ctx, Handler};
use tracing::Level;

#[derive(Debug)]
struct Add(u32);

#[derive(Debug)]
struct Sleep(u64);

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
    fn handle(&mut self, message: Add, _: &mut Ctx<Self>) -> AskResult<Self::Result> {
        self.inner += message.0;
        AskResult::Reply(())
    }
}

impl Ask<Sleep> for Counter {
    type Result = ();
    fn handle(&mut self, Sleep(time): Sleep, context: &mut Ctx<Self>) -> AskResult<Self::Result> {
        let duration = Duration::from_secs(time);
        let handle = context.anonymous_handle(async move {
            tokio::time::sleep(duration).await;
        });
        AskResult::Task(handle)
    }
}

impl AsyncAsk<Add> for Counter {
    type Output = ();
    type Future<'a> = Ready<Self::Output>;

    fn handle<'a>(&'a mut self, message: Add, _: &mut Ctx<Self>) -> Self::Future<'a> {
        self.inner += message.0;
        ready(())
    }
}

impl Ask<Sum> for Counter {
    type Result = Counter;

    fn handle(&mut self, _: Sum, _: &mut Ctx<Self>) -> AskResult<Self::Result> {
        AskResult::Reply(Counter { inner: self.inner })
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
    println!("Sleeping for 3 seconds");
    addr.ask(Sleep(3)).await.unwrap();
    println!("Complete");
}
