use tokio::{
    sync::oneshot,
    task::{JoinError, JoinHandle},
};

use crate::{context::AnonymousActor, Actor, Ctx};

pub struct AsyncHandle<T: Message>(pub(crate) JoinHandle<AnonymousActor<T>>);

pub enum AskResult<T: Message> {
    Reply(T),
    Task(AsyncHandle<T>),
}

/// The message that an actor can handle.
pub trait Message: Send + Sync + 'static {}
impl<A: Send + Sync + 'static> Message for A {}

/// Possible errors that can be caught by tokio while executing an asyncrouns task.
/// In this case, no data about the actor can be returned.
#[derive(Debug)]
pub enum ChildError {
    Panic(JoinError),
    Cancelled(JoinError),
}

pub enum AnonymousTaskCancelled {
    Success,
    Cancel,
    Panic,
}

/// A dead actor must be handled by a supervisor. The death of an actor could be
/// because it completed executing or it paniced/cancelled during execution.
///
/// A supervisor must make the desion how to restart the actor.
pub type DeadActorResult<A> = Result<DeadActor<A>, ChildError>;

/// Contains the data for the actor as well as the actors context.
pub struct DeadActor<A: Actor> {
    pub actor: A,
    pub ctx: Ctx<A>,
}

impl<A: Actor> DeadActor<A> {
    pub(crate) fn panic(err: JoinError) -> DeadActorResult<A> {
        Err(ChildError::Panic(err))
    }

    pub(crate) fn cancelled(err: JoinError) -> DeadActorResult<A> {
        Err(ChildError::Cancelled(err))
    }
}

pub struct IntoFutureShutdown<A: Actor> {
    pub stop_now: bool,
    pub tx: oneshot::Sender<A>,
}

impl<A: Actor> IntoFutureShutdown<A> {
    pub fn new(stop_now: bool, tx: oneshot::Sender<A>) -> Self {
        Self { stop_now, tx }
    }
}
