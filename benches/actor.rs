#[macro_use]
extern crate criterion;

use am::{Actor, Handler, Message};
use criterion::{BatchSize, Criterion};

struct BenchActor;

#[derive(Debug)]
struct BenchActorMessage;
impl Message for BenchActorMessage {}
impl Actor for BenchActor {}
impl Handler<BenchActorMessage> for BenchActor {
    fn handle(&mut self, _: BenchActorMessage, _: &mut am::Ctx<Self>) {}
}

fn create_actors(c: &mut Criterion) {
    let small = 100;
    let large = 10000;

    let id = format!("Creation of {small} actors");
    let runtime = tokio::runtime::Builder::new_multi_thread().build().unwrap();
    c.bench_function(&id, move |b| {
        b.iter_batched(
            || {},
            |()| {
                runtime.block_on(async move {
                    let mut handles = vec![];
                    for _ in 0..small {
                        let handler = BenchActor {}.start();
                        handles.push(handler);
                    }
                    handles
                })
            },
            BatchSize::PerIteration,
        );
    });

    let id = format!("Creation of {large} actors");
    let runtime = tokio::runtime::Builder::new_multi_thread().build().unwrap();
    c.bench_function(&id, move |b| {
        b.iter_batched(
            || {},
            |()| {
                runtime.block_on(async move {
                    let mut handles = vec![];
                    for _ in 0..large {
                        let handler = BenchActor {}.start();
                        handles.push(handler);
                    }
                    handles
                })
            },
            BatchSize::PerIteration,
        );
    });
}

fn schedule_work(c: &mut Criterion) {
    let small = 100;
    let large = 1000;

    let id = format!("Waiting on {small} actors to process first message");
    let runtime = tokio::runtime::Builder::new_multi_thread().build().unwrap();
    c.bench_function(&id, move |b| {
        b.iter_batched(
            || {
                runtime.block_on(async move {
                    let mut join_set = tokio::task::JoinSet::new();

                    for _ in 0..small {
                        let handler = BenchActor {}.start();
                        join_set.spawn(async move { handler.await });
                    }
                    join_set
                })
            },
            |mut handles| {
                runtime.block_on(async move { while handles.join_next().await.is_some() {} })
            },
            BatchSize::PerIteration,
        );
    });

    let id = format!("Waiting on {large} actors to process first message");
    let runtime = tokio::runtime::Builder::new_multi_thread().build().unwrap();
    c.bench_function(&id, move |b| {
        b.iter_batched(
            || {
                runtime.block_on(async move {
                    let mut join_set = tokio::task::JoinSet::new();
                    for _ in 0..large {
                        let handler = BenchActor {}.start();
                        join_set.spawn(async move { handler.await });
                    }
                    join_set
                })
            },
            |mut handles| {
                runtime.block_on(async move { while handles.join_next().await.is_some() {} })
            },
            BatchSize::PerIteration,
        );
    });
}

#[allow(clippy::async_yields_async)]
fn process_messages(c: &mut Criterion) {
    const NUM_MSGS: u64 = 100000;

    struct MessagingActor;

    impl Actor for MessagingActor {}
    impl Handler<BenchActorMessage> for MessagingActor {
        fn handle(&mut self, _: BenchActorMessage, _: &mut am::Ctx<Self>) {}
    }

    let id = format!("Waiting on {NUM_MSGS} messages to be processed");
    let runtime = tokio::runtime::Builder::new_multi_thread().build().unwrap();
    c.bench_function(&id, move |b| {
        b.iter_batched(
            || runtime.block_on(async move { MessagingActor {}.start() }),
            |handle| {
                runtime.block_on(async move {
                    for _ in 0..NUM_MSGS {
                        handle.send_async(BenchActorMessage {}).await.unwrap();
                    }
                    let _ = handle.await;
                })
            },
            BatchSize::PerIteration,
        );
    });
}

criterion_group!(actors, create_actors, schedule_work, process_messages);
criterion_main!(actors);
