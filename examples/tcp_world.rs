use tokactor::{Actor, Ask, Ctx, Handler, Message, TcpRequest, World, WorldResult};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Debug)]
struct Complete;
impl Message for Complete {}

#[derive(Default)]
struct Echo {
    connections: usize,
}

impl Actor for Echo {}

impl Ask<TcpRequest> for Echo {
    type Result = WorldResult;

    fn handle(
        &mut self,
        TcpRequest(mut stream, _): TcpRequest,
        ctx: &mut Ctx<Self>,
    ) -> Self::Result {
        println!("Initializing a new connection");
        self.connections += 1;
        let address = ctx.address();
        ctx.anonymous_task(async move {
            let mut buf = vec![0; 1024];

            loop {
                let n = stream
                    .read(&mut buf)
                    .await
                    .expect("failed to read data from socket");

                if n == 0 {
                    println!("Breaking");
                    break;
                } else {
                    println!("Recieved '{}'", String::from_utf8_lossy(&buf[..n]).trim())
                }

                stream
                    .write_all(&buf[0..n])
                    .await
                    .expect("failed to write data to socket");
            }
            println!("Sending");
            address.send_async(Complete).await.unwrap();
            println!("Sent!");
        });
        Ok(())
    }
}

impl Handler<Complete> for Echo {
    fn handle(&mut self, _: Complete, _: &mut Ctx<Self>) {
        self.connections -= 1;
        println!("Active Connections Remaining: {}", self.connections);
    }
}

fn main() {
    println!("Starting up...");
    World::<()>::new()
        .unwrap()
        .with_tcp_input("127.0.0.1:8080", Echo::default())
        .block_until_completion();
    println!("Completed! Shutting down...");
}
