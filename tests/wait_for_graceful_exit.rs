use tokactor::{Actor, ActorContext, Ctx, Handler, IntoFutureError};

#[derive(Debug, Clone)]
struct Count(());

#[derive(Debug, Clone)]
struct SetCount(usize);

#[derive(Debug, Clone)]
struct Stop(());

#[derive(Debug, Default)]
struct CountActor {
    counter: usize,
}
impl Actor for CountActor {}

impl Handler<SetCount> for CountActor {
    fn handle(&mut self, SetCount(count): SetCount, c: &mut Ctx<Self>) {
        let address = c.address();
        c.anonymous_task(async move {
            for _ in 0..count {
                let _ = address.send_async(Count(())).await;
            }
        });
    }
}

impl Handler<Count> for CountActor {
    fn handle(&mut self, _: Count, _: &mut Ctx<Self>) {
        self.counter += 1;
    }
}

impl Handler<Stop> for CountActor {
    fn handle(&mut self, _: Stop, context: &mut Ctx<Self>) {
        context.stop();
    }
}

#[tokio::test]
async fn wait_for_actor_to_finish_executing_with_correct_state() {
    let addr = CountActor::default().start();

    let address = addr.clone();
    tokio::spawn(async move {
        let actor = address.wait_for_completion().await.unwrap();
        assert_eq!(actor.counter, 10);
    });

    addr.send_async(SetCount(10)).await.unwrap();
    addr.send_async(Stop(())).await.unwrap();
}

#[tokio::test]
async fn multiple_wait_for_actor_to_finish_executing() {
    let addr = CountActor::default().start();
    let addr1 = addr.clone();
    let addr2 = addr1.clone();
    let addr3 = addr1.clone();

    tokio::spawn(async move {
        let r1 = addr1.wait_for_completion().await;
        let r2 = addr2.wait_for_completion().await;
        let r3 = addr3.wait_for_completion().await;
        assert_eq!(r1.err().unwrap(), IntoFutureError::Paniced);
        assert_eq!(r2.err().unwrap(), IntoFutureError::Paniced);
        assert_eq!(r3.ok().unwrap().counter, 1000);
    });

    addr.send_async(SetCount(1000)).await.unwrap();
    addr.send_async(Stop(())).await.unwrap();
}
