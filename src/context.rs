use std::{future::Future, sync::Arc};

use tokio::sync::{mpsc, oneshot, watch, OwnedSemaphorePermit, Semaphore, TryAcquireError};

use crate::{
    actor::InternalHandler,
    envelope::SendMessage,
    executor::Executor,
    message::{AnonymousTaskCancelled, AsyncHandle, DeadActor},
    single::{AskRx, AsyncAskRx, Noop},
    Actor, ActorRef, AnonymousRef, Ask, AsyncAsk, DeadActorResult, Handler, Message,
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

impl std::fmt::Display for ActorState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ActorState::Running => write!(f, "Running"),
            ActorState::Stopping => write!(f, "Stopping"),
            ActorState::Stopped => write!(f, "Stopped"),
        }
    }
}

/// Messages that an actors supervisor can send a running child actor. Messages
/// sent to children are handled by the framework and do general options that
/// effect the state the actor is in.
#[derive(Debug, Clone)]
pub(crate) enum SupervisorMessage {
    /// Ask for all child actors to shutdown
    Shutdown,
    // Ask all child actors to respond about their health
    // HealthCheck(),
}

pub struct AnonymousActor<T> {
    pub(crate) result: Option<T>,
    receiver: watch::Receiver<Option<SupervisorMessage>>,
}

impl<T> AnonymousActor<T> {
    pub(crate) fn new(receiver: watch::Receiver<Option<SupervisorMessage>>) -> Self {
        Self {
            result: None,
            receiver,
        }
    }

    pub(crate) async fn handle<F>(mut self, f: F) -> Self
    where
        F: Future<Output = T> + Send + 'static,
    {
        let is_reciever_updated = self.receiver.borrow_and_update().is_some();
        if is_reciever_updated {
            self
        } else {
            tokio::select! {
                // TODO(Alec): When a reciever gets a value, we don't want the
                //             future to fail. We only want it to fail if the
                //             recieved message is a "shut down right now"
                //             Read more: https://docs.rs/tokio/latest/tokio/macro.select.html
                _ = self.receiver.changed() => {},
                result = f => {
                    self.result = Some(result);
                },
            };
            self
        }
    }
}

/// Decided when the actor should respond to the awaiting address. If `Now`, then
/// it will [`ActorState::Stopped`] the executing actor and exit quietly. Otherwise,
/// the awaiter will wait until the actor has completed executing.
pub enum IntoFutureSender<A: Actor> {
    /// Complete the actors event loop and process all messages
    Now(oneshot::Sender<A>),
    /// Wait for the actor to exit normally, and then respond to the awaiter
    UponCompletion(oneshot::Sender<A>),
}

impl<A: Actor> IntoFutureSender<A> {
    pub fn into_inner(self) -> oneshot::Sender<A> {
        match self {
            IntoFutureSender::Now(inner) => inner,
            IntoFutureSender::UponCompletion(inner) => inner,
        }
    }
}

/// General context for actors written
pub struct Ctx<A: Actor> {
    /// The sending side of an actors mailbox.
    address: ActorRef<A>,
    /// The recving side of an actors mailbox.
    pub(crate) mailbox: mpsc::Receiver<Box<dyn SendMessage<A>>>,
    /// The send side of a watch pipe to send messages to child tasks to communicate
    /// with them general tasks that the framework handles internally.
    pub(crate) notifier: watch::Sender<Option<SupervisorMessage>>,
    /// Actor State keeps track of the current actors context state. State travels
    /// in one direction from `Running` -> `Stopping` -> `Stopped`.
    pub(crate) state: ActorState,
    /// The maximum amount of anonymous actors that the parent actor can spawn.
    /// Note that this does not effect the number of children spawned.
    pub(crate) max_anonymous_actors: Arc<Semaphore>,
    /// A counter of the maximum amount of anonymous actors not being tracked because
    /// the maximum number of actors allowed was exceeded. Capture the amount exceeded
    /// and wait until enough have tracked actors have completed before allowing for
    /// more anonymous actors to be created
    pub(crate) overflow_anonymous_actors: usize,
    /// An optional flag a user can set when they **await** an actor through their
    /// address.
    ///
    /// By awaiting on an actors address, you tell the actor to shutdown and
    /// wait until the actor has completed (All children exit and all messages
    /// processed).
    ///
    /// If not set, when the actor completes execution, the actors
    /// data will be dropped.
    pub(crate) into_future_sender: Option<IntoFutureSender<A>>,
}

impl<A: Actor> Ctx<A> {
    /// Create a new actor and pass in the system in which the actor is meant to
    /// be created on.
    pub(crate) fn new() -> Ctx<A> {
        let (tx, rx) = mpsc::channel(A::mailbox_size());
        let (notifier, _) = watch::channel(None);
        let max_anonymous_actors = Semaphore::new(A::max_anonymous_actors());
        Self {
            address: ActorRef::new(tx),
            mailbox: rx,
            state: ActorState::Running,
            into_future_sender: None,
            max_anonymous_actors: Arc::new(max_anonymous_actors),
            overflow_anonymous_actors: 0,
            notifier,
        }
    }

    /// Run an actor without a supervisor. If the actor fails, it does so silently.
    /// This should be only ran by top level actors. Panics caused by top level actors
    /// are not handled either, so it will cause your entire program to crash and
    /// fail.
    pub fn run(self, actor: A) -> ActorRef<A> {
        let address = self.address.clone();
        let executor = Executor::new(actor, self);
        tokio::spawn(executor.run_actor());
        address
    }

    /// Execute an actor with a pre-existin context built from the [`crate::util::builder::CtxBuilder`].
    /// This will allow a context that was built to be executed as a child of an
    /// existing actor.
    pub fn spawn_with<C>(&self, ctx: Ctx<C>, actor: C) -> ActorRef<C>
    where
        A: Handler<DeadActorResult<C>>,
        C: Actor,
    {
        // trace here
        tracing::info!(parent = A::name(), actor = C::name(), "spawning");

        if self.state != ActorState::Running {
            panic!("Can't start an actor when stopped or stopping");
        }

        let address = ctx.address.clone();
        let supervisor = self.address.clone();
        let executor = Executor::child(actor, ctx, self.notifier.subscribe());

        tokio::spawn(async move {
            match tokio::spawn(executor.run_supervised_actor()).await {
                // Execution of actor successfully completed. Message supervisor of success
                Ok(mut executor) => {
                    // We can unwrap here because if the task finishes without
                    // an errors.

                    // Also, we don't need to do both of these. If we are being awaited on
                    // it probably means we are exiting successfully. The dead actor
                    // handler is there for panics and errors that happen during execution
                    // not when we are expecting it complete executing.
                    if let Some(sender) = executor.context.into_future_sender.take() {
                        if let Err(actor) = sender.into_inner().send(executor.actor) {
                            // We failed to send the actor to the part of the
                            // code that was awaiting us to complete. We still
                            // exited correctly though. Log the error and move on.
                            tracing::error!(
                                parent = A::name(),
                                actor = C::name(),
                                "reciever dropped"
                            );
                            executor.actor = actor;
                        } else {
                            return;
                        }
                    }
                    // Keep the reciever alive because this is how we tell that
                    // the supervisor still has children alive and active.
                    let (_rx, dead_actor) = executor.into_dead_actor();
                    // We MUST send this message to supervisor so that it runs its
                    // update loop and registers that the child died. Once it's
                    // event loop runs, it can decide if the supervisor should
                    // die as well.
                    if (supervisor.send_async(Ok(dead_actor)).await).is_err() {
                        unreachable!("Tried to send dead actor {}, but supervisor {} failed to accept message", C::name(), A::name())
                    }
                }
                // The child actor have failed for some reason whether that was
                // them be cancelled on purpose or paniced while executing.
                Err(err) => {
                    if err.is_cancelled() {
                        let _ = supervisor.send_async(DeadActor::cancelled(err)).await;
                    } else if err.is_panic() {
                        let _ = supervisor.send_async(DeadActor::panic(err)).await;
                    } else {
                        unreachable!("Tokio tasked failed in unknown way. This shouldn't happen");
                    }
                }
            };
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
        self.spawn_with(Ctx::new(), actor)
    }

    pub(crate) fn spawn_anonymous_rx_handle<In>(&self, rx: mpsc::Receiver<In>)
    where
        A: Handler<In>,
        In: Message,
    {
        if self.state != ActorState::Running {
            panic!("Can't start an actor when stopped or stopping");
        }

        let ctx = Ctx::new();
        let address = self.address();
        let executor = Executor::child(Noop(), ctx, self.notifier.subscribe());
        tokio::spawn(executor.child_with_custom_handle_rx(address, rx));
    }

    pub(crate) fn spawn_anonymous_rx_ask<In>(&self, rx: AskRx<In, A>)
    where
        A: Actor + Ask<In>,
        In: Message,
    {
        if self.state != ActorState::Running {
            panic!("Can't start an actor when stopped or stopping");
        }

        let ctx = Ctx::new();
        let address = self.address();
        let executor = Executor::child(Noop(), ctx, self.notifier.subscribe());
        tokio::spawn(executor.child_with_custom_ask_rx(address, rx));
    }

    pub(crate) fn spawn_anonymous_rx_async<In>(&self, rx: AsyncAskRx<In, A>)
    where
        A: AsyncAsk<In>,
        In: Message,
    {
        if self.state != ActorState::Running {
            panic!("Can't start an actor when stopped or stopping");
        }

        let ctx = Ctx::new();
        let address = self.address();
        let executor = Executor::child(Noop(), ctx, self.notifier.subscribe());
        tokio::spawn(executor.child_with_custom_async_ask_rx(address, rx));
    }

    fn acquire_anonymous_permit(&mut self) -> Option<OwnedSemaphorePermit> {
        match self.max_anonymous_actors.clone().try_acquire_owned() {
            Ok(permit) => Some(permit),
            Err(TryAcquireError::NoPermits) => {
                self.overflow_anonymous_actors += 1;
                None
            }
            Err(TryAcquireError::Closed) => unreachable!(),
        }
    }

    /// Spawn an anonymous task that runs an asynchronous
    /// task. When the task completes, it returns the result back to the actor.
    ///
    /// If the task is cancelled (ex. Supervisor asks for task to be shutdown) or
    /// panics then no result is returned to the supervisor.
    pub fn anonymous<F>(&mut self, future: F) -> AnonymousRef
    where
        F: Future + Sync + Send + 'static,
        F::Output: Message + Send + 'static,
        A: Handler<F::Output> + InternalHandler<AnonymousTaskCancelled>,
    {
        let supervisor = self.address.clone();
        let actor = AnonymousActor::new(self.notifier.subscribe());
        let permit = self.acquire_anonymous_permit();

        let handle = tokio::spawn(async move {
            let _permit = permit;
            match tokio::spawn(actor.handle(future)).await {
                Ok(inner) => {
                    match inner.result {
                        Some(message) => {
                            let _ = supervisor.send_async(message).await;
                        }
                        None => {
                            // The task was cancelled by the supervisor so we are just
                            // going to drop the work that was being executed.
                            let _ = supervisor
                                .internal_send_async(AnonymousTaskCancelled::Cancel)
                                .await;
                        }
                    }
                }
                Err(_) => {
                    // The task ended by a user cancelling or the function panicing.
                    // Drop reciver to register function as complete.
                    tracing::error!(parent = A::name(), actor = "anonymous", "actor paniced");
                    let _ = supervisor
                        .internal_send_async(AnonymousTaskCancelled::Panic)
                        .await;
                }
            };
        });
        AnonymousRef::new(handle)
    }

    /// Spawn an anonymous task that supports running asynchrounsly but once complete,
    /// don't send a message back to the sender.
    pub fn anonymous_task<F>(&mut self, future: F) -> AnonymousRef
    where
        F: Future<Output = ()> + Sync + Send + 'static,
    {
        let supervisor = self.address.clone();
        let actor = AnonymousActor::new(self.notifier.subscribe());
        let permit = self.acquire_anonymous_permit();

        let handle = tokio::spawn(async move {
            let _permit = permit;
            let _ = tokio::spawn(actor.handle(future)).await;
            // No matter what, we send back that the task was cancelled.
            let _ = supervisor
                .internal_send_async(AnonymousTaskCancelled::Success)
                .await;
        });
        AnonymousRef::new(handle)
    }

    /// Create an anonymous actor that will at some point in the future, be resolved.
    /// Don't block the current actor.
    pub fn anonymous_handle<F>(&mut self, future: F) -> AsyncHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Message + Send + 'static,
    {
        let actor = AnonymousActor::<F::Output>::new(self.notifier.subscribe());
        let permit = self.acquire_anonymous_permit();

        let inner = tokio::spawn(async move {
            let _permit = permit;
            actor.handle(future).await
        });
        AsyncHandle(inner)
    }

    /// Clone the current actors address
    pub fn address(&self) -> ActorRef<A> {
        self.address.clone()
    }

    /// Subscribe to the completion of the currect executing actor context. Also
    /// tell the actor that it should stop soon by changing the actor state to
    /// [`ActorState::Stopping`]. A stopping state will stop the actor from
    /// recieving messages and will empty it's mailbox. Once the actor has
    /// finished executing, if a supervisor is waiting for a response,
    /// it sends a message to them.
    pub(crate) fn subscribe_and_stop(&mut self, tx: oneshot::Sender<A>) {
        self.into_future_sender = Some(IntoFutureSender::Now(tx));
        if matches!(self.state, ActorState::Running) {
            self.stop();
        }
    }

    pub(crate) fn subscribe_and_wait(&mut self, tx: oneshot::Sender<A>) {
        self.into_future_sender = Some(IntoFutureSender::UponCompletion(tx));
    }
}

pub trait ActorContext {
    fn stop(&mut self);
    fn abort(&mut self);
}

impl<A: Actor> ActorContext for Ctx<A> {
    /// Stop an actor while keeping it's mailbox open. Good for waiting for children
    /// to finish executing an messaging the parent
    fn stop(&mut self) {
        if self.state == ActorState::Running {
            self.state = ActorState::Stopping;
        }
    }

    /// Stop an actor but also close it's mailbox. This is a dangrous operation
    /// and results in children not being about to message their parent when they
    /// shutdown
    fn abort(&mut self) {
        if matches!(self.state, ActorState::Running | ActorState::Stopping) {
            self.mailbox.close();
            self.state = ActorState::Stopped;
        }
    }
}

#[cfg(test)]
impl<A: Actor> Drop for Ctx<A> {
    fn drop(&mut self) {
        if let Err(err) = self.mailbox.try_recv() {
            // Note that is can never be Disconnected because we probably always have
            // a sender avaliable.
            assert_eq!(
                err,
                mpsc::error::TryRecvError::Empty,
                "Actors mailbox should be empty"
            );
        } else {
            eprintln!(
                "MAILBOX FOR ACTOR SHOULD BE CLOSED. ACTOR {} PROBABLY PANICED",
                A::name()
            );
        }
        if self.state == ActorState::Running {
            eprintln!(
                "ACTOR {} IS BEING DESTORIED IN A RUNNING STATE. PROBABLY PANICED",
                A::name()
            );
        } else {
            assert_eq!(
                self.state,
                ActorState::Stopped,
                "Actor {} is not in correct state",
                A::name()
            );
        }
        assert_eq!(
            self.notifier.receiver_count(),
            0,
            "All Actor children should be dead"
        );
        assert!(
            self.into_future_sender.is_none(),
            "Actor should not be waiting for the future"
        );
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, time::Duration};

    use crate::{
        message::ChildError, Actor, ActorContext, ActorRef, Ctx, DeadActorResult, Handler,
        IntoFutureError,
    };

    #[derive(Debug, PartialEq, Eq)]
    enum ActorLifecycle {
        Start,
        PreRun,
        PostRun,
        Stopping,
        Stopped,
        End,
    }

    trait DebugActor: Send + Sync + 'static {
        const DEBUG_KIND: &'static str;

        fn start(self) -> ActorRef<DebuggableActor<Self>>
        where
            Self: Default + Send + Sync + 'static,
        {
            Ctx::new().run(DebuggableActor::default())
        }
    }

    #[derive(Default)]
    struct DebuggableActor<A: Send + Sync + 'static> {
        state: VecDeque<ActorLifecycle>,
        messages: VecDeque<TestMessage>,
        inner: A,
    }

    impl<A: Send + Sync + 'static> DebuggableActor<A> {
        fn push_state(&mut self, state: ActorLifecycle) {
            self.state.push_back(state);
        }

        fn push_message(&mut self, message: TestMessage) {
            self.messages.push_back(message)
        }

        fn shift_state(&mut self) -> Option<ActorLifecycle> {
            self.state.pop_front()
        }

        fn shift_message(&mut self) -> Option<TestMessage> {
            self.messages.pop_front()
        }

        fn expect_message(&mut self, msg: TestMessage) {
            assert_eq!(self.shift_state(), Some(ActorLifecycle::PreRun));
            assert_eq!(self.shift_message(), Some(msg));
            assert_eq!(self.shift_state(), Some(ActorLifecycle::PostRun));
        }

        fn expect_system_message(&mut self) {
            assert_eq!(self.shift_state(), Some(ActorLifecycle::PreRun));
            assert_eq!(self.shift_state(), Some(ActorLifecycle::PostRun));
        }

        fn expect_start(&mut self) {
            assert_eq!(self.shift_state(), Some(ActorLifecycle::Start));
        }

        fn expect_stopping(&mut self) {
            assert_eq!(self.shift_state(), Some(ActorLifecycle::Stopping));
        }

        fn expect_stopped(&mut self) {
            assert_eq!(self.shift_state(), Some(ActorLifecycle::Stopped));
        }

        fn expect_end(&mut self) {
            assert_eq!(self.shift_state(), Some(ActorLifecycle::End));
        }

        fn expect_stopped_event_in_state(&mut self) {
            let index = self
                .state
                .iter()
                .position(|p| matches!(p, ActorLifecycle::Stopped));
            self.state.remove(index.unwrap());
        }

        fn is_empty(&mut self) {
            assert_eq!(self.shift_message(), None);
            assert_eq!(self.shift_state(), None);
        }

        fn inner(self) -> A {
            self.inner
        }
    }

    impl<A: Send + Sync + 'static> Actor for DebuggableActor<A> {
        fn pre_run(&mut self, _: &mut Ctx<Self>) {
            self.push_state(ActorLifecycle::PreRun)
        }

        fn post_run(&mut self, _: &mut Ctx<Self>) {
            self.push_state(ActorLifecycle::PostRun)
        }

        fn on_stopping(&mut self, _: &mut Ctx<Self>) {
            self.push_state(ActorLifecycle::Stopping)
        }

        fn on_stopped(&mut self, _: &mut Ctx<Self>) {
            self.push_state(ActorLifecycle::Stopped)
        }

        fn on_end(&mut self, _: &mut Ctx<Self>) {
            self.push_state(ActorLifecycle::End)
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
            self.push_state(ActorLifecycle::Start)
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
    enum TestResult {
        Ok(usize),
        Panic,
        Cancel,
    }

    #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
    enum TestMessage {
        Normal,
        Spawn(usize, Option<Box<TestMessage>>),
        SpawnAnonymous(u64, usize),
        SpawnAnonymousTask(u64),
        DespawnAnonymous(usize),
        Despawn(TestResult),
        Stop,
        Abort,
        Panic,
    }

    impl<A: Send + Sync + 'static> Handler<TestMessage> for DebuggableActor<A> {
        fn handle(&mut self, message: TestMessage, context: &mut Ctx<Self>) {
            self.push_message(message.clone());
            match message {
                TestMessage::Normal => {}
                TestMessage::Spawn(num, mut opt) => {
                    let inner = ChildActor { inner: num };
                    let addr = context.spawn(DebuggableActor {
                        state: Default::default(),
                        messages: Default::default(),
                        inner,
                    });
                    if let Some(msg) = opt.take() {
                        addr.try_send(*msg);
                    }
                }
                TestMessage::SpawnAnonymous(time, num) => {
                    context.anonymous(async move {
                        tokio::time::sleep(Duration::from_millis(time)).await;
                        TestMessage::DespawnAnonymous(num)
                    });
                }
                TestMessage::SpawnAnonymousTask(time) => {
                    context.anonymous_task(tokio::time::sleep(Duration::from_millis(time)));
                }
                TestMessage::Stop => context.stop(),
                TestMessage::Abort => context.abort(),
                TestMessage::Panic => panic!("AAAHHH"),
                _ => {
                    // We are just pushing the message onto the stack. We aren't doing anything
                }
            }
        }
    }

    type DeadChildActor = DeadActorResult<DebuggableActor<ChildActor>>;
    impl<A: Send + Sync + 'static> Handler<DeadChildActor> for DebuggableActor<A> {
        fn handle(&mut self, message: DeadChildActor, _context: &mut Ctx<Self>) {
            let msg = match message {
                Ok(actor) => TestMessage::Despawn(TestResult::Ok(actor.actor.inner().inner)),
                Err(ChildError::Panic(_)) => TestMessage::Despawn(TestResult::Panic),
                Err(ChildError::Cancelled(_)) => TestMessage::Despawn(TestResult::Cancel),
            };
            self.push_message(msg);
        }
    }

    /***************************************************************************
     * Definitions for actors
     **************************************************************************/

    #[derive(Default)]
    struct ParentActor {}

    impl DebugActor for ParentActor {
        const DEBUG_KIND: &'static str = "ParentActor";
    }

    struct ChildActor {
        inner: usize,
    }

    impl DebugActor for ChildActor {
        const DEBUG_KIND: &'static str = "ChildActor";
    }

    #[test]
    fn size_of_context() {
        assert_eq!(64, std::mem::size_of::<Ctx<DebuggableActor<ParentActor>>>())
    }

    /***************************************************************************
     * Testing `Ctx::run` method for running an actor
     **************************************************************************/

    async fn start_message_and_stop_test_actor<Inner: DebugActor + Default>(
        messages: &[TestMessage],
    ) -> DebuggableActor<Inner> {
        let addr = Inner::default().start();
        for msg in messages {
            addr.send(msg.clone()).unwrap();
        }
        addr.await.unwrap()
    }

    #[tokio::test]
    async fn run_actor_to_complition() {
        let addr = Ctx::new().run(DebuggableActor::<ParentActor>::default());
        let addr2 = addr.clone();
        let mut actor = addr.await.unwrap();

        actor.expect_start();
        actor.expect_system_message(); // System message is the actor being awaited
        actor.expect_stopping();
        actor.expect_stopped();
        actor.expect_end();
        actor.is_empty();

        assert_eq!(addr2.await.err(), Some(IntoFutureError::MailboxClosed));
    }

    #[tokio::test]
    async fn stop_running_actor() {
        use TestMessage::*;
        let messages = vec![Normal, Stop, Normal, Normal];
        let mut actor: DebuggableActor<ParentActor> =
            start_message_and_stop_test_actor::<ParentActor>(&messages).await;

        actor.expect_start();
        actor.expect_message(Normal);
        actor.expect_message(Stop);
        actor.expect_stopping();
        actor.expect_stopped();
        actor.expect_message(Normal);
        actor.expect_message(Normal);
        actor.expect_system_message(); // This is us awaiting an address
        actor.expect_end();
        actor.is_empty();
    }

    #[tokio::test]
    async fn abort_running_actor() {
        use TestMessage::*;
        let messages = vec![Normal, Abort, Normal, Normal];
        let mut actor = start_message_and_stop_test_actor::<ParentActor>(&messages).await;

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
        let _ = start_message_and_stop_test_actor::<ParentActor>(&messages).await;
    }

    /***************************************************************************
     * Testing `Ctx::spawn` method for running child actors
     **************************************************************************/

    #[tokio::test]
    async fn run_parent_and_child_to_complition() {
        let addr = DebuggableActor::<ParentActor>::default().start();
        addr.try_send(TestMessage::Spawn(1, None));
        let mut debuggable = addr.await.unwrap();

        debuggable.expect_start();
        debuggable.expect_message(TestMessage::Spawn(1, None));
        debuggable.expect_system_message(); // System message is the actor being awaited
        debuggable.expect_stopping();
        debuggable.expect_message(TestMessage::Despawn(TestResult::Ok(1)));
        debuggable.expect_stopped();
        debuggable.expect_end();
        debuggable.is_empty();
    }

    #[tokio::test]
    async fn run_parent_and_many_child_to_complition() {
        let range = 1..=10;
        let addr = DebuggableActor::<ParentActor>::default().start();
        for i in range.clone() {
            addr.send_async(TestMessage::Spawn(i, None)).await.unwrap();
        }
        let mut debuggable = addr.await.unwrap();

        debuggable.expect_start();
        for i in range.clone() {
            debuggable.expect_message(TestMessage::Spawn(i, None));
        }
        debuggable.expect_system_message(); // System message is the actor being awaited
        debuggable.expect_stopping();

        // Dead actor messages come back in an unordered list
        let mut list = vec![];
        while let Some(msg) = debuggable.shift_message() {
            list.push(msg);
        }
        list.sort();

        // The stopped event may come out of order as all the children may "die"
        // before the parent actor can process all the dead children. Expect
        // that it shows up in the list at some point instead.
        debuggable.expect_stopped_event_in_state();

        for i in range.rev() {
            // Now expect all the child dying events
            assert_eq!(list.pop(), Some(TestMessage::Despawn(TestResult::Ok(i))));
            debuggable.expect_system_message(); // Handler<DeadActorResult<ChildActor>>
        }

        // finish validating all the dead actor messages
        debuggable.expect_end();
        debuggable.is_empty();
    }

    #[tokio::test]
    async fn child_actor_stops_by_itself() {
        let addr = DebuggableActor::<ParentActor>::default().start();
        addr.try_send(TestMessage::Spawn(1, Some(Box::new(TestMessage::Stop))));
        let mut debuggable = addr.await.unwrap();

        debuggable.expect_start();
        debuggable.expect_message(TestMessage::Spawn(1, Some(Box::new(TestMessage::Stop))));
        debuggable.expect_system_message(); // System message is the actor being awaited
        debuggable.expect_stopping();
        debuggable.expect_message(TestMessage::Despawn(TestResult::Ok(1)));
        debuggable.expect_stopped();
        debuggable.expect_end();
        debuggable.is_empty();
    }

    #[tokio::test]
    async fn child_actor_panics() {
        let addr = DebuggableActor::<ParentActor>::default().start();
        addr.try_send(TestMessage::Spawn(1, Some(Box::new(TestMessage::Panic))));
        let mut debuggable = addr.await.unwrap();

        debuggable.expect_start();
        debuggable.expect_message(TestMessage::Spawn(1, Some(Box::new(TestMessage::Panic))));
        debuggable.expect_system_message(); // System message is the actor being awaited
        debuggable.expect_stopping();
        debuggable.expect_message(TestMessage::Despawn(TestResult::Panic));
        debuggable.expect_stopped();
        debuggable.expect_end();
        debuggable.is_empty();
    }

    /***************************************************************************
     * Testing `Ctx::anonymous` method for running anonymous actors
     **************************************************************************/

    #[tokio::test]
    async fn run_parent_and_anonymous_actor_to_complition() {
        let addr = DebuggableActor::<ParentActor>::default().start();
        addr.try_send(TestMessage::SpawnAnonymous(500, 1));
        tokio::time::sleep(Duration::from_millis(600)).await;
        let mut debuggable = addr.await.unwrap();

        debuggable.expect_start();
        debuggable.expect_message(TestMessage::SpawnAnonymous(500, 1));
        debuggable.expect_system_message(); // System message is the actor being awaited
        debuggable.expect_message(TestMessage::DespawnAnonymous(1));
        debuggable.expect_stopping();
        debuggable.expect_stopped();
        debuggable.expect_end();
        debuggable.is_empty();
    }

    #[tokio::test]
    async fn run_parent_and_cancel_anonymous_actor() {
        let addr = DebuggableActor::<ParentActor>::default().start();
        addr.try_send(TestMessage::SpawnAnonymous(1000, 1));
        let mut debuggable = addr.await.unwrap();

        debuggable.expect_start();
        debuggable.expect_message(TestMessage::SpawnAnonymous(1000, 1));
        debuggable.expect_system_message(); // System message is the actor being awaited
                                            // We don't recieve a Despawn event for Anonymous
                                            // because we didn't wait for sleep to finish.
                                            // So the task was cancelled and cancelled tasks
                                            // don't send messages back to the supervisor.
        debuggable.expect_stopping();
        debuggable.expect_system_message(); // System message from anonymous actor being cancelled
        debuggable.expect_stopped();
        debuggable.expect_end();
        debuggable.is_empty();
    }

    /***************************************************************************
     * Testing `Ctx::anonymous_task` method for running anonymous actors
     **************************************************************************/

    #[tokio::test]
    async fn run_parent_and_anonymous_task_to_complition() {
        let addr = DebuggableActor::<ParentActor>::default().start();
        addr.try_send(TestMessage::SpawnAnonymousTask(100));
        tokio::time::sleep(Duration::from_millis(50)).await;
        let mut debuggable = addr.await.unwrap();

        debuggable.expect_start();
        debuggable.expect_message(TestMessage::SpawnAnonymousTask(100));
        debuggable.expect_system_message(); // System message is the actor being awaited
        debuggable.expect_stopping();
        debuggable.expect_system_message(); // System message with message of task completing
        debuggable.expect_stopped();
        debuggable.expect_end();
        debuggable.is_empty();
    }

    #[tokio::test]
    async fn run_parent_and_cancel_anonymous_task() {
        let addr = DebuggableActor::<ParentActor>::default().start();
        addr.try_send(TestMessage::SpawnAnonymousTask(1000));
        let mut debuggable = addr.await.unwrap();

        debuggable.expect_start();
        debuggable.expect_message(TestMessage::SpawnAnonymousTask(1000));
        debuggable.expect_system_message(); // System message is the actor being awaited
                                            // We don't recieve a Despawn event for Anonymous
                                            // because we didn't wait for sleep to finish.
                                            // So the task was cancelled and cancelled tasks
                                            // don't send messages back to the supervisor.
        debuggable.expect_stopping();
        debuggable.expect_system_message(); // System message from anonymous actor being cancelled
        debuggable.expect_stopped();
        debuggable.expect_end();
        debuggable.is_empty();
    }
}
