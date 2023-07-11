use std::time::Duration;

use tokactor::{Actor, Ctx, Handler};

#[derive(Debug, Clone)]
struct Spawn(usize);

struct MaxActors;

impl Actor for MaxActors {
    fn max_anonymous_actors() -> usize {
        2
    }
}

impl Handler<Spawn> for MaxActors {
    fn handle(&mut self, msg: Spawn, c: &mut Ctx<Self>) {
        for _ in 0..msg.0 {
            c.anonymous_task(async move {
                tokio::time::sleep(Duration::from_secs(1)).await;
            });
        }
    }
}

#[tokio::test]
async fn send_low_amount_of_messages_to_actor() {
    let addr = MaxActors.start();
    addr.send_async(Spawn(1)).await.unwrap();
    let _ = addr.await.unwrap();
}

// #[tokio::test]
// async fn send_high_number_of_messages_to_actor() {
//     let addr = MaxActors.start();
//     addr.send_async(Spawn(3)).await.unwrap();
//     let _ = addr.await.unwrap();
// }

// #[tokio::test]
// async fn send_really_high_number_of_messages_to_actor() {
//     let addr = MaxActors.start();
//     addr.send_async(Spawn(300)).await.unwrap();
//     let _ = addr.await.unwrap();
// }
