use tokio::{sync::oneshot, task::JoinError};

use crate::{Actor, Ctx};

/// The message that an actor can handle.
pub trait Message: Send + Sync + 'static {}

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
impl Message for AnonymousTaskCancelled {}

/// A dead actor must be handled by a supervisor. The death of an actor could be
/// because it completed executing or it paniced/cancelled during execution.
///
/// A supervisor must make the desion how to restart the actor.
pub type DeadActorResult<A> = Result<DeadActor<A>, ChildError>;

impl<A: Actor> Message for DeadActorResult<A> {}

/// Contains the data for the actor as well as the actors context.
pub struct DeadActor<A: Actor> {
    pub actor: A,
    pub ctx: Ctx<A>,
}

impl<A: Actor> DeadActor<A> {
    pub(crate) fn success(actor: A, ctx: Ctx<A>) -> DeadActorResult<A> {
        Ok(Self { actor, ctx })
    }

    pub(crate) fn panic(err: JoinError) -> DeadActorResult<A> {
        Err(ChildError::Panic(err))
    }

    pub(crate) fn cancelled(err: JoinError) -> DeadActorResult<A> {
        Err(ChildError::Cancelled(err))
    }
}

impl<A: Actor> Message for IntoFutureShutdown<A> {}
pub struct IntoFutureShutdown<A: Actor> {
    pub tx: oneshot::Sender<A>,
}

impl<A: Actor> IntoFutureShutdown<A> {
    pub fn new(tx: oneshot::Sender<A>) -> Self {
        Self { tx }
    }
}
