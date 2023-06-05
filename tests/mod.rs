use std::time::Duration;

use tokactor::{Actor, Ctx, DeadActorResult, Handler, IntoFutureError, Message};

pub struct StringMessage(pub String);
impl Message for StringMessage {}

/**
 * Test that IntoFuture integration works with just one actor. Actors will represent
 * a unit of work
 */
#[derive(Debug, Default)]
struct StateActor {
    state: Option<String>,
}

impl Actor for StateActor {}

impl Handler<StringMessage> for StateActor {
    fn handle(&mut self, message: StringMessage, _context: &mut Ctx<Self>) {
        self.state = Some(message.0)
    }
}

#[tokio::test]
async fn create_actor_process_message_and_complete() {
    let actor = StateActor { state: None }.start();
    actor.try_send(StringMessage("test".to_string()));
    let a = actor.await.unwrap();
    assert_eq!(a.state, Some("test".to_string()));
}

#[tokio::test]
async fn panic_when_awaiting_on_async_actor() {
    let a1 = StateActor { state: None }.start();
    let a2 = a1.clone();
    a1.try_send(StringMessage("test".to_string()));
    assert_eq!(a1.await.unwrap().state, Some("test".to_string()));
    assert_eq!(a2.await.unwrap_err(), IntoFutureError::MailboxClosed);
}

/**
 * Test that IntoFuture works when a parent actor spawns many child actors. All
 * Child actors should finish executing at some point at which point, the parent
 * actor completes
 */
struct Children {
    count: usize,
}
impl Message for Children {}

struct ParentActor {
    children: usize,
    total: usize,
    dead: usize,
}

impl Actor for ParentActor {}

impl Handler<Children> for ParentActor {
    fn handle(&mut self, message: Children, context: &mut Ctx<Self>) {
        self.children = message.count;
        for index in 1..=message.count {
            context.spawn(ChildActor { index });
        }
    }
}

impl Handler<DeadActorResult<ChildActor>> for ParentActor {
    fn handle(&mut self, message: DeadActorResult<ChildActor>, _context: &mut Ctx<Self>) {
        let dead = message.unwrap();
        self.total += dead.actor.index;
        self.dead += 1;
    }
}

struct ChildActor {
    index: usize,
}

impl Actor for ChildActor {}

#[tokio::test]
async fn supervisor_actor_fan_out_and_fan_in_complete() {
    let addr = ParentActor {
        children: 0,
        total: 0,
        dead: 0,
    }
    .start();
    addr.try_send(Children { count: 5 });
    let parent = addr.await.unwrap();
    assert_eq!(parent.children, 5);
    assert_eq!(parent.total, 15);
    assert_eq!(parent.dead, 5);
}

#[tokio::test]
async fn supervisor_actor_fan_out_and_fan_in_complete_2() {
    let addr = ParentActor {
        children: 0,
        total: 0,
        dead: 0,
    }
    .start();
    addr.try_send(Children { count: 40 });
    let parent = addr.await.unwrap();
    assert_eq!(parent.children, 40);
    // assert_eq!(parent.total, 15);
    assert_eq!(parent.dead, 40);
}

/**
 * Test that IntoFuture works when a parent actor spawns many async child actors. All
 * Child actors should finish executing at some point at which point, the parent
 * actor completes.
 */
impl Message for AsyncChildren {}
struct AsyncChildren {
    count: usize,
}

impl Message for AsyncChildrenResult {}
struct AsyncChildrenResult {
    index: usize,
}

impl Handler<AsyncChildren> for ParentActor {
    fn handle(&mut self, message: AsyncChildren, context: &mut Ctx<Self>) {
        self.children = message.count;
        for index in 1..=message.count {
            context.anonymous(async move {
                tokio::time::sleep(Duration::from_secs(1)).await;
                AsyncChildrenResult { index }
            });
        }
    }
}

impl Handler<AsyncChildrenResult> for ParentActor {
    fn handle(&mut self, message: AsyncChildrenResult, _context: &mut Ctx<Self>) {
        self.total += message.index;
        self.dead += 1;
    }
}

#[tokio::test]
async fn supervisor_actor_async_fan_out_and_fan_in_complete() {
    let addr = ParentActor {
        children: 0,
        total: 0,
        dead: 0,
    }
    .start();
    addr.try_send(AsyncChildren { count: 5 });
    let parent = addr.await.unwrap();
    assert_eq!(parent.children, 5);
    assert_eq!(parent.total, 0);
    assert_eq!(parent.dead, 0);
}
