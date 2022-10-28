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

#[derive(Debug, Clone)]
enum SupervisorMessage {
    Shutdown,
}

pub struct Ctx<A: Actor> {
    address: ActorRef<A>,
    mailbox: mpsc::Receiver<Box<dyn SendMessage<A>>>,
    notifier: watch::Sender<Option<SupervisorMessage>>,
    state: ActorState,
    into_future_sender: Option<oneshot::Sender<A>>,
}

impl<A: Actor<Context = Ctx<A>>> Ctx<A> {
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
    /// This should be only ran by supervisor actors that implement default.
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

    // Run as actor that is supervised by another actor. The supervisor watches
    // the child and they can communicate with each other.
    pub fn spawn<C>(&self, actor: C) -> ActorRef<C>
    where
        A: Handler<DeadActorResult<C>>,
        C: Actor<Context = Ctx<C>>,
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
                    let _ = supervisor.async_send(actor).await;
                }
                // The child actor have failed for some reason whether that was
                // them be cancelled on purpose or paniced while executing.
                Err(err) if err.is_cancelled() => {
                    let _ = supervisor.async_send(DeadActor::cancelled(err)).await;
                }
                Err(err) if err.is_panic() => {
                    let _ = supervisor.async_send(DeadActor::panic(err)).await;
                }
                _ => unreachable!("Tokio tasked failed in unknown way. This shouldn't happen"),
            };
        });
        address
    }

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
                    let _ = supervisor.async_send(output).await;
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

    pub fn address(&self) -> ActorRef<A> {
        self.address.clone()
    }

    pub fn halt(&mut self, tx: oneshot::Sender<A>) {
        self.into_future_sender = Some(tx);
        self.stop();
    }
}

async fn run_actor<A>(actor: &mut A, ctx: &mut A::Context)
where
    A: Actor<Context = Ctx<A>>,
{
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

async fn run_supervised_actor<A>(
    actor: &mut A,
    ctx: &mut A::Context,
    mut receiver: watch::Receiver<Option<SupervisorMessage>>,
) where
    A: Actor<Context = Ctx<A>>,
{
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
    fn stop(&mut self) {
        self.state = ActorState::Stopping;
    }

    fn abort(&mut self) {
        self.mailbox.close();
        self.state = ActorState::Stopped;
    }
}

async fn running<A: Actor<Context = Ctx<A>>>(actor: &mut A, ctx: &mut A::Context) {
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

async fn stopping<A: Actor<Context = Ctx<A>>>(actor: &mut A, ctx: &mut A::Context) {
    if !ctx.notifier.is_closed() {
        ctx.notifier
            .send(Some(SupervisorMessage::Shutdown))
            .unwrap();
    } else {
        return;
    }

    while let Some(mut msg) = ctx.mailbox.recv().await {
        actor.on_run();
        msg.send(actor, ctx);
        actor.post_run();

        if ctx.notifier.is_closed() {
            ctx.mailbox.close();
        }
    }
}

async fn stopped<A: Actor<Context = Ctx<A>>>(actor: &mut A, ctx: &mut A::Context) {
    assert!(ctx.notifier.is_closed());
    while let Ok(mut msg) = ctx.mailbox.try_recv() {
        actor.on_run();
        msg.send(actor, ctx);
        actor.post_run();
    }
}
