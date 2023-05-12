use am::{Actor, ActorRef, Ask, Ctx, Handler, Message};
use std::time::Duration;

#[derive(Default)]
struct PingReceiver;

impl Actor for PingReceiver {}

impl Ask<Ping> for PingReceiver {
    type Result = Str;

    fn handle(&mut self, _msg: Ping, _ctx: &mut Ctx<Self>) -> Self::Result {
        Str("Pong".to_string())
    }
}

struct PingSender {
    peer: ActorRef<PingReceiver>,
}

#[derive(Debug)]
struct Str(String);
impl Message for Str {}

#[derive(Debug)]
struct Loop;
impl Message for Loop {}

#[derive(Debug)]
struct Ping;
impl Message for Ping {}

impl Actor for PingSender {
    fn on_start(&mut self, ctx: &mut Ctx<Self>)
    where
        Self: Actor,
    {
        ctx.address().try_ask(Loop);
    }
}

impl Ask<Loop> for PingSender {
    type Result = ();

    fn handle(&mut self, _: Loop, ctx: &mut Ctx<Self>) -> Self::Result {
        let peer = self.peer.clone();
        let me = ctx.address();
        ctx.anonymous(async move {
            let reply_msg = peer.ask(Ping).await;
            println!("{reply_msg}");
            me.schedule(Duration::from_secs(1)).ask(Loop).await;
        });
    }
}

#[tokio::main]
async fn main() {
    let ping_rx = PingReceiver::default().start();
    let ping_tx = PingSender { peer: ping_rx }.start();

    ping_tx.await;
}
