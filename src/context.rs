use std::future::Future;

use tokio::sync::{mpsc, oneshot, watch};

use crate::{
    envelope::SendMessage, message::DeadActor, Actor, ActorRef, AnonymousRef, DeadActorResult,
    Handler, Message,
};

/// The Actor State records what the life cycle state that the actor currently
/// implements. This is used mostly for communication with the system.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum ActorState {
    /// A running state means that the actor is currently processing requests.
    Running,

    /// A stopping state means that the actor is shutting down.
    Stopping,

    /// A stopped state means the actor has been shut down.
    Stopped,
}

/// Messages that an actors supervisor can send a running child actor. Messages
/// sent to children are handled by the framework and do general options that
/// effect the state the actor is in.
#[derive(Debug, Clone)]
enum SupervisorMessage {
    Shutdown,
}

/// General context for actors written
pub struct Ctx<A: Actor> {
    /// The sending side of an actors mailbox.
    address: ActorRef<A>,
    /// The recving side of an actors mailbox.
    mailbox: mpsc::Receiver<Box<dyn SendMessage<A>>>,
    /// The send side of a watch pipe to send messages to child tasks to communicate
    /// with them general tasks that the framework handles internally.
    notifier: watch::Sender<Option<SupervisorMessage>>,
    /// Actor State keeps track of the current actors context state. State travels
    /// in one direction from `Running` -> `Stopping` -> `Stopped`.
    state: ActorState,
    /// An optional flag a user can set when awaiting for an actor to execute until
    /// compleition. If not set, when the actor completes execution, the actors
    /// data will be dropped.
    into_future_sender: Option<oneshot::Sender<A>>,
}

impl<A: Actor> Ctx<A> {
    /// Create a new actor and pass in the system in which the actor is meant to
    /// be created on.
    pub(crate) fn new() -> Ctx<A> {
        let (tx, rx) = mpsc::channel(20);
        let (notifier, _) = watch::channel(None);
        Self {
            address: ActorRef::new(tx),
            mailbox: rx,
            state: ActorState::Running,
            into_future_sender: None,
            notifier,
        }
    }

    /// Run an actor without a supervisor. If the actor fails, it does so silently.
    /// This should be only ran by top level actors. Panics caused by top level actors
    /// are not handled either, so it will cause your entire program to crash and
    /// fail.
    pub fn run(self, actor: A) -> ActorRef<A> {
        let address = self.address.clone();
        tokio::spawn(async move {
            let mut actor = actor;
            let mut ctx = self;
            run_actor(&mut actor, &mut ctx).await;
            if let Some(sender) = ctx.into_future_sender.take() {
                if sender.send(actor).is_err() {
                    unreachable!(
                        "Actor {} awaited for completion but dropped reciever",
                        A::KIND
                    )
                }
            }
        });
        address
    }

    /// Run as actor that is supervised by another actor. The supervisor watches
    /// the child and they can communicate with each other.
    pub fn spawn<C>(&self, actor: C) -> ActorRef<C>
    where
        A: Handler<DeadActorResult<C>>,
        C: Actor,
    {
        if self.state != ActorState::Running {
            panic!("Can't start an actor when stopped or stopping");
        }

        let child: Ctx<C> = Ctx::new();
        let address = child.address.clone();
        let supervisor = self.address.clone();
        let receiver = self.notifier.subscribe();

        tokio::spawn(async move {
            let result = tokio::spawn(async move {
                let mut actor = actor;
                let mut ctx = child;
                run_supervised_actor(&mut actor, &mut ctx, receiver).await;
                DeadActor::success(actor, ctx)
            })
            .await;

            match result {
                // Execution of actor successfully completed. Message supervisor of success
                Ok(actor) => {
                    let _ = supervisor.send_async(actor).await;
                }
                // The child actor have failed for some reason whether that was
                // them be cancelled on purpose or paniced while executing.
                Err(err) if err.is_cancelled() => {
                    let _ = supervisor.send_async(DeadActor::cancelled(err)).await;
                }
                Err(err) if err.is_panic() => {
                    let _ = supervisor.send_async(DeadActor::panic(err)).await;
                }
                _ => unreachable!("Tokio tasked failed in unknown way. This shouldn't happen"),
            };
        });
        address
    }

    /// Spawn an anonymous task that runs an actor that supports running an asyncrous
    /// task. When the task completes, it returns the result back to the actor.
    pub fn anonymous<F>(&self, future: F) -> AnonymousRef
    where
        F: Future + Send + 'static,
        F::Output: Message + Send + 'static,
        A: Handler<F::Output>,
    {
        let supervisor = self.address.clone();
        let receiver = self.notifier.subscribe();

        let handle = tokio::spawn(async move {
            let result = tokio::spawn(async move {
                tokio::select! {
                    result = future => Some(result),
                    // TODO(Alec): When a reciever gets a value, we don't want the
                    //             future to fail. We only want it to fail if the
                    //             recieved message is a "shut down right f*** now"
                    //             Read more: https://docs.rs/tokio/latest/tokio/macro.select.html
                    // _ = receiver.changed() => None,
                }
            })
            .await;

            match result {
                Ok(Some(output)) => {
                    let _ = supervisor.send_async(output).await;
                }
                Ok(None) => todo!("Parent asked child to shutdown before completion"),
                Err(err) if !err.is_cancelled() => {
                    todo!("Please handle error ctx.anonymous() -> {}", err)
                }
                _ => {
                    println!("Anonymous task was cancelled successfully")
                }
            };
            drop(receiver)
        });
        AnonymousRef::new(handle)
    }

    /// Spawn an anonymous task that supports running asynchrounsly but once complete,
    /// don't send a message back to the sender.
    pub fn anonymous_task<F>(&self, future: F) -> AnonymousRef
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let receiver = self.notifier.subscribe();
        let handle = tokio::spawn(async move {
            let result = tokio::spawn(async move {
                tokio::select! {
                    result = future => Some(result),
                    // TODO(Alec): When a reciever gets a value, we don't want the
                    //             future to fail. We only want it to fail if the
                    //             recieved message is a "shut down right f*** now"
                    //             Read more: https://docs.rs/tokio/latest/tokio/macro.select.html
                    // _ = receiver.changed() => None,
                }
            })
            .await;

            match result {
                Ok(Some(_)) => {}
                Ok(None) => todo!("Parent asked child to shutdown before completion"),
                Err(err) if !err.is_cancelled() => {
                    todo!("Please handle error ctx.anonymous() -> {}", err)
                }
                _ => {
                    println!("Anonymous task was cancelled successfully")
                }
            }
            drop(receiver)
        });
        AnonymousRef::new(handle)
    }

    /// Clone the current actors address
    pub fn address(&self) -> ActorRef<A> {
        self.address.clone()
    }

    /// Halt the execution of the currect actors context. By halting an actor
    /// it transitions into a [`ActorState::Stopping`]. A stopping state will
    /// stop the actor from recieving messages and will empty it's mailbox. Once
    /// the actor has finished executing, if a supervisor is waiting for a response,
    /// it sends a message to them.
    pub(crate) fn halt(&mut self, tx: oneshot::Sender<A>) {
        self.into_future_sender = Some(tx);
        self.stop();
    }
}

async fn run_actor<A: Actor>(actor: &mut A, ctx: &mut Ctx<A>) {
    actor.on_start(ctx);
    // The actor in the running state
    running(actor, ctx).await;
    // The actor transitioning into the stopping state
    actor.on_stopping();
    // The actor in the stopping or stopped state
    stopping(actor, ctx).await;
    // The actor transitioned to a stopped state
    ctx.state = ActorState::Stopped;
    actor.on_stopped();
    // The actor has stopped
    stopped(actor, ctx).await;
    // the actor has completed execution
    actor.on_end();
}

async fn run_supervised_actor<A: Actor>(
    actor: &mut A,
    ctx: &mut Ctx<A>,
    mut receiver: watch::Receiver<Option<SupervisorMessage>>,
) {
    actor.on_start(ctx);
    // The actor in the running state
    loop {
        tokio::select! {
            // We attempt to run an actor to completion
            option = ctx.mailbox.recv() => {
                if option.is_none() {
                    break;
                }
                actor.on_run();
                option.unwrap().send(actor, ctx);
                actor.post_run();

                match ctx.state {
                    ActorState::Stopped | ActorState::Stopping => break,
                    _ => continue,
                }
            },
            // Or we recieve a message from our supervisor
            result = receiver.changed() => match result {
                Ok(_) => match &*receiver.borrow() {
                    Some(SupervisorMessage::Shutdown) => break,
                    None => continue,
                },
                Err(err) => {
                    panic!("Supervisor died before child. This shouldn't happen: {:?}", err)
                }
            }
        };
    }
    // The actor transitioning into the stopping state
    actor.on_stopping();
    // The actor in the stopping or stopped state
    stopping(actor, ctx).await;
    // The actor transitioned to a stopped state
    ctx.state = ActorState::Stopped;
    actor.on_stopped();
    // The actor has stopped
    stopped(actor, ctx).await;
    // the actor has completed execution
    actor.on_end();
}

pub trait ActorContext {
    fn stop(&mut self);
    fn abort(&mut self);
}

impl<A: Actor> ActorContext for Ctx<A> {
    /// Stop an actor while keeping it's mailbox open. Good for waiting for children
    /// to finish executing an messaging the parent
    fn stop(&mut self) {
        self.state = ActorState::Stopping;
    }

    /// Stop an actor but also close it's mailbox. This is a dangrous operation
    /// and results in children not being about to message their parent when they
    /// shutdown
    fn abort(&mut self) {
        self.mailbox.close();
        self.state = ActorState::Stopped;
    }
}

async fn running<A: Actor>(actor: &mut A, ctx: &mut Ctx<A>) {
    while let Some(mut msg) = ctx.mailbox.recv().await {
        actor.on_run();
        msg.send(actor, ctx);
        actor.post_run();

        match ctx.state {
            ActorState::Stopped | ActorState::Stopping => return,
            _ => continue,
        }
    }
}

async fn stopping<A: Actor>(actor: &mut A, ctx: &mut Ctx<A>) {
    // We have no children, so we can just move to the stopping state. If we had
    // children, then we want to continue running and recieving messages until
    // all of our children have died.
    if !ctx.notifier.is_closed() {
        ctx.notifier
            .send(Some(SupervisorMessage::Shutdown))
            .unwrap();
    } else {
        // We have no children. Go to ending state.
        ctx.mailbox.close();
        return;
    }

    while let Some(mut msg) = ctx.mailbox.recv().await {
        actor.on_run();
        msg.send(actor, ctx);
        actor.post_run();

        // If all of our children have died
        if ctx.notifier.is_closed() {
            ctx.mailbox.close();
        }
    }
}

async fn stopped<A: Actor>(actor: &mut A, ctx: &mut Ctx<A>) {
    assert!(ctx.notifier.is_closed());
    while let Ok(mut msg) = ctx.mailbox.try_recv() {
        actor.on_run();
        msg.send(actor, ctx);
        actor.post_run();
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use crate::{Actor, ActorContext, Ctx, Handler, IntoFutureError, Message};

    #[derive(Debug, PartialEq, Eq)]
    enum ActorLifecycle {
        Start,
        PreRun,
        PostRun,
        Stopping,
        Stopped,
        End,
    }

    #[derive(Default)]
    struct ParentActor {
        state: VecDeque<ActorLifecycle>,
        messages: VecDeque<TestMessage>,
    }

    impl ParentActor {
        pub fn expect_message(&mut self, msg: TestMessage) {
            assert_eq!(self.state.pop_front(), Some(ActorLifecycle::PreRun));
            assert_eq!(self.messages.pop_front(), Some(msg));
            assert_eq!(self.state.pop_front(), Some(ActorLifecycle::PostRun));
        }

        pub fn expect_system_message(&mut self) {
            assert_eq!(self.state.pop_front(), Some(ActorLifecycle::PreRun));
            assert_eq!(self.state.pop_front(), Some(ActorLifecycle::PostRun));
        }

        pub fn expect_start(&mut self) {
            assert_eq!(self.state.pop_front(), Some(ActorLifecycle::Start));
        }

        pub fn expect_stopping(&mut self) {
            assert_eq!(self.state.pop_front(), Some(ActorLifecycle::Stopping));
        }

        pub fn expect_stopped(&mut self) {
            assert_eq!(self.state.pop_front(), Some(ActorLifecycle::Stopped));
        }

        pub fn expect_end(&mut self) {
            assert_eq!(self.state.pop_front(), Some(ActorLifecycle::End));
        }

        pub fn is_empty(&mut self) {
            assert_eq!(self.messages.pop_front(), None);
            assert_eq!(self.state.pop_front(), None);
        }
    }

    impl Actor for ParentActor {
        const KIND: &'static str = "ParentActor";

        fn on_run(&mut self) {
            self.state.push_back(ActorLifecycle::PreRun)
        }

        fn post_run(&mut self) {
            self.state.push_back(ActorLifecycle::PostRun)
        }

        fn on_stopping(&mut self) {
            self.state.push_back(ActorLifecycle::Stopping)
        }

        fn on_stopped(&mut self) {
            self.state.push_back(ActorLifecycle::Stopped)
        }

        fn on_end(&mut self) {
            self.state.push_back(ActorLifecycle::End)
        }

        fn start(self) -> crate::ActorRef<Self>
        where
            Self: Actor,
        {
            Ctx::new().run(self)
        }

        fn on_start(&mut self, _: &mut Ctx<Self>)
        where
            Self: Actor,
        {
            self.state.push_back(ActorLifecycle::Start)
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum TestMessage {
        Normal,
        Stop,
        Abort,
        Panic,
    }

    impl Message for TestMessage {}

    impl Handler<TestMessage> for ParentActor {
        fn handle(&mut self, message: TestMessage, context: &mut Ctx<Self>) {
            self.messages.push_back(message);
            match message {
                TestMessage::Normal => {}
                TestMessage::Stop => context.stop(),
                TestMessage::Abort => context.abort(),
                TestMessage::Panic => panic!("AAAHHH"),
            }
        }
    }

    #[test]
    fn size_of_context() {
        assert_eq!(48, std::mem::size_of::<Ctx<ParentActor>>())
    }

    /***************************************************************************
     * Testing `Ctx::run` method for running an actor
     **************************************************************************/

    async fn start_message_and_stop_test_actor(messages: &[TestMessage]) -> ParentActor {
        let addr = ParentActor::default().start();
        for msg in messages {
            addr.send(*msg).unwrap();
        }
        addr.await.unwrap()
    }

    #[tokio::test]
    async fn run_actor_to_complition() {
        let addr = Ctx::new().run(ParentActor::default());
        let addr2 = addr.clone();
        let mut actor = addr.await.unwrap();

        actor.expect_start();
        actor.expect_stopping();
        actor.expect_stopped();
        actor.expect_system_message(); // System message is the actor being awaited
        actor.expect_end();
        actor.is_empty();

        assert_eq!(addr2.await.err(), Some(IntoFutureError::MailboxClosed));
    }

    #[tokio::test]
    async fn stop_running_actor() {
        use TestMessage::*;
        let messages = vec![Normal, Stop, Normal, Normal];
        let mut actor = start_message_and_stop_test_actor(&messages).await;

        actor.expect_start();
        actor.expect_message(Normal);
        actor.expect_message(Stop);
        actor.expect_stopping();
        actor.expect_stopped();
        actor.expect_message(Normal);
        actor.expect_message(Normal);
        actor.expect_system_message();
        actor.expect_end();
        actor.is_empty();
    }

    #[tokio::test]
    async fn abort_running_actor() {
        use TestMessage::*;
        let messages = vec![Normal, Abort, Normal, Normal];
        let mut actor = start_message_and_stop_test_actor(&messages).await;

        actor.expect_start();
        actor.expect_message(Normal);
        actor.expect_message(Abort);
        actor.expect_stopping();
        actor.expect_stopped();
        actor.expect_message(Normal);
        actor.expect_message(Normal);
        actor.expect_system_message();
        actor.expect_end();
        actor.is_empty();
    }

    #[tokio::test]
    #[should_panic]
    async fn panic_during_actor_running() {
        use TestMessage::*;
        let messages = vec![Normal, Panic, Normal, Normal];
        let _ = start_message_and_stop_test_actor(&messages).await;
    }
}
