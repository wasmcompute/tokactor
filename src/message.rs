use tokio::{sync::oneshot, task::JoinError};

use crate::{Actor, Ctx};

/// The message that an actor can handle.
pub trait Message: Send + 'static {}

// #[derive(Debug)]
// pub enum ShutdownStatus {
//     Complete,
// }

// impl ShutdownStatus {
//     /// Returns `true` if the shutdown status is [`Complete`].
//     ///
//     /// [`Complete`]: ShutdownStatus::Complete
//     #[must_use]
//     pub fn is_complete(&self) -> bool {
//         matches!(self, Self::Complete)
//     }
// }

pub type DeadActorResult<A> = Result<DeadActor<A>, ChildError>;

pub struct DeadActor<A: Actor> {
    pub actor: A,
    pub ctx: A::Context,
}

impl<A: Actor> DeadActor<A> {
    pub fn success(actor: A, ctx: A::Context) -> DeadActorResult<A> {
        Ok(Self { actor, ctx })
    }

    pub fn panic(err: JoinError) -> DeadActorResult<A> {
        Err(ChildError::Panic(err))
    }

    pub fn cancelled(err: JoinError) -> DeadActorResult<A> {
        Err(ChildError::Cancelled(err))
    }
}

impl<A: Actor> Message for DeadActorResult<A> {}

#[derive(Debug)]
pub enum ChildError {
    Panic(JoinError),
    Cancelled(JoinError),
}

impl<A: Actor<Context = Ctx<A>>> Message for IntoFutureShutdown<A> {}
pub struct IntoFutureShutdown<A: Actor<Context = Ctx<A>>> {
    pub tx: oneshot::Sender<A>,
}

impl<A: Actor<Context = Ctx<A>>> IntoFutureShutdown<A> {
    pub fn new(tx: oneshot::Sender<A>) -> Self {
        Self { tx }
    }
}
