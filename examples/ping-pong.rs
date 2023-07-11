use tokactor::{Actor, Ask, Ctx};

/// [PingPong] is a basic actor that will print
/// ping..pong.. repeatedly until some exit
/// condition is met (a counter hits 10). Then
/// it will exit
pub struct PingPong {
    counter: u8,
}

/// This is the types of message [PingPong] supports
#[derive(Debug, Clone)]
pub enum Msg {
    Ping,
    Pong,
}
impl Msg {
    // retrieve the next message in the sequence
    fn next(&self) -> Self {
        match self {
            Self::Ping => Self::Pong,
            Self::Pong => Self::Ping,
        }
    }
    // print out this message
    fn print(&self) {
        match self {
            Self::Ping => print!("ping.."),
            Self::Pong => print!("pong.."),
        }
    }
}

// the implementation of our actor's "logic"
impl Actor for PingPong {}

impl Ask<Msg> for PingPong {
    type Result = Msg;

    // This is our main message handler
    fn handle(&mut self, message: Msg, _: &mut Ctx<Self>) -> Self::Result {
        message.print();
        self.counter += 1;
        message.next()
    }
}

#[tokio::main]
async fn main() {
    let handle = PingPong { counter: 0 }.start();
    let mut message = Msg::Ping;
    for _ in 0..10 {
        message = handle.ask(message).await.unwrap();
    }
    let actor = handle
        .await
        .expect("Ping-pong actor failed to exit properly");
    assert_eq!(actor.counter, 10);
    println!("\nProcessed {} messages", actor.counter);
}
