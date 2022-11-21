use crate::{
    context::Ctx,
    message::{AnonymousTaskCancelled, IntoFutureShutdown},
    ActorRef, Message,
};

/// User implemented actor. The user must inheriate this trait so that we can
/// control the execution of the actors struct.
pub trait Actor: Send + Sync + Sized + 'static {
    const KIND: &'static str;

    /// Start an actor using a context
    fn start(self) -> ActorRef<Self>
    where
        Self: Actor,
    {
        Ctx::new().run(self)
    }

    /// Called before transitioning to an [`ActorState::Started`] state. Good usage is to load data
    /// from a database or disk.
    fn on_start(&mut self, _: &mut Ctx<Self>)
    where
        Self: Actor,
    {
    }

    /// Called right before the handler for the message. Basically before the actors
    /// state transitions from a [`ActorState::Started`] to a [`ActorState::Running`].
    /// Good usage is to cache data that can not be lost!
    fn on_run(&mut self) {}

    /// Called after the handler for any message has been called. Is called before
    /// actor state transitions from a [`ActorState::Running`] to a [`ActorState::Started`].
    /// Good usage is logging.
    fn post_run(&mut self) {}

    /// Called after transitioning to an [`ActorState::Stopping`] state. Mainly
    /// used to communicate with all child actors that the actor will be shutting
    /// down shortly and that they should also finish executing.
    fn on_stopping(&mut self) {}

    /// Called after transitioning to an [`ActorState::Stopped`] state. Even when
    /// this state is reached there could be messages left inside of the mailbox.
    /// Users should save an actors data during the [`Actor::on_end`] state transition.
    fn on_stopped(&mut self) {}

    /// Called after clearing out the actors mailbox and after all child actors
    /// have been de-initialized. Good time to clean up the actor and save some
    /// of it's state.
    fn on_end(&mut self) {}
}

/// Handler is able to be programmed to handle certian messages. When executing
/// it can answer messages or it can forward them to new actors. Special care
/// must be taken when forwarding a message from an actor. When forwarding,
/// a user can return None.
///
/// However, when responding to a message, it is required to return a message
/// inside of the option. If no response is found, an error is triggered and
/// the request is complete with an error.
pub trait Handler<M: Message>: Actor {
    /// this method is called for every message received by the actor
    fn handle(&mut self, message: M, context: &mut Ctx<Self>);
}

/// Ask an actor to handle a message but also return a response. Asking is an
/// optimization to be able to handle two operations at once.
pub trait Ask<M: Message>: Actor {
    type Result: Message;

    fn handle(&mut self, message: M, context: &mut Ctx<Self>) -> Self::Result;
}

impl<A: Actor> Handler<IntoFutureShutdown<A>> for A {
    fn handle(&mut self, message: IntoFutureShutdown<A>, context: &mut Ctx<Self>) {
        context.halt(message.tx);
    }
}

impl<A: Actor> Handler<AnonymousTaskCancelled> for A {
    fn handle(&mut self, _: AnonymousTaskCancelled, _: &mut Ctx<Self>) {
        println!("Cancelled actor")
    }
}
