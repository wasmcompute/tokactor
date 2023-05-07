use am::{Actor, Ask, Handler, Message};

#[derive(Debug)]
struct Add(u32);
impl Message for Add {}

#[derive(Debug)]
struct Sum;
impl Message for Sum {}

#[derive(Debug)]
struct Counter {
    inner: u32,
}

impl Actor for Counter {}
impl Message for Counter {}

impl Handler<Add> for Counter {
    fn handle(&mut self, message: Add, _: &mut am::Ctx<Self>) {
        self.inner += message.0;
    }
}

impl Ask<Add> for Counter {
    type Result = ();
    fn handle(&mut self, message: Add, _: &mut am::Ctx<Self>) -> Self::Result {
        self.inner += message.0;
    }
}

impl Ask<Sum> for Counter {
    type Result = Counter;

    fn handle(&mut self, _: Sum, _: &mut am::Ctx<Self>) -> Self::Result {
        Counter { inner: self.inner }
    }
}

#[tokio::main]
async fn main() {
    let addr = Counter { inner: 0 }.start();
    addr.send_async(Add(10)).await.unwrap();
    addr.ask(Add(10)).await.unwrap();
    addr.async_ask(Add(10)).await.unwrap();
    let counter = addr.async_ask(Sum).await.unwrap();
    println!("Total count should be 30 = {:?}", counter);
}
