use std::future::Future;

use crate::{context::Ctx, ActorRef, Message};

pub enum Scheduler {
    Blocking,
    NonBlocking,
}

/// User implemented actor. The user must inheriate this trait so that we can
/// control the execution of the actors struct.
pub trait Actor: Send + Sync + Sized + 'static {
    /// Return a debuggable name of the actor
    fn name() -> &'static str {
        std::any::type_name::<Self>()
    }

    /// Declare the maximum amount of anonymous actors that a parent actor can
    /// spawn. If the maximum amount of anonymous actors are executing, then pause
    /// the actor until there is space to spawn another.
    fn max_anonymous_actors() -> usize {
        usize::MAX >> 3
    }

    /// The max size of the actors mailbox. The actor will only be able to store
    /// a max of the number provided. After the mailbox is full, it will stop
    /// accepting new message or apply back pressure and the sender will need to
    /// wait for items to be removed from the mailbox.
    fn mailbox_size() -> usize {
        6
    }

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
    fn pre_run(&mut self, _: &mut Ctx<Self>) {}

    /// Called after the handler for any message has been called. Is called before
    /// actor state transitions from a [`ActorState::Running`] to a [`ActorState::Started`].
    /// Good usage is logging.
    fn post_run(&mut self, _: &mut Ctx<Self>) {}

    /// Called after transitioning to an [`ActorState::Stopping`] state. Mainly
    /// used to communicate with all child actors that the actor will be shutting
    /// down shortly and that they should also finish executing.
    fn on_stopping(&mut self, _: &mut Ctx<Self>) {}

    /// Called after transitioning to an [`ActorState::Stopped`] state. Even when
    /// this state is reached there could be messages left inside of the mailbox.
    /// Users should save an actors data during the [`Actor::on_end`] state transition.
    fn on_stopped(&mut self, _: &mut Ctx<Self>) {}

    /// Called after clearing out the actors mailbox and after all child actors
    /// have been de-initialized. Good time to clean up the actor and save some
    /// of it's state.
    fn on_end(&mut self, _: &mut Ctx<Self>) {}
}

/// An internal implementation for the handler that we can implement generic behavior
/// for all actors. This would include messages for shutting down the actors, timing
/// out the actors, ect.
pub trait InternalHandler<M: Message>: Actor {
    /// Handle the message recieved by the actor
    fn private_handler(&mut self, message: M, context: &mut Ctx<Self>);
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

    /// Explain the type of scheduler the actor should use
    fn scheduler() -> Scheduler {
        Scheduler::NonBlocking
    }
}

/// Ask an actor to handle a message but also return a response. Asking is an
/// optimization to be able to handle two operations at once.
pub trait Ask<M: Message>: Actor {
    type Result: Message;

    fn handle(&mut self, message: M, context: &mut Ctx<Self>) -> Self::Result;

    /// Explain the type of scheduler the actor should use
    fn scheduler() -> Scheduler {
        Scheduler::NonBlocking
    }
}

/// Ask an actor to recieve a message and respond back. The respond is expected to
/// be some type of async operation and thus is executed by a anonymous actor that
/// takes one messages and handles it.
pub trait AsyncAsk<M: Message>: Actor {
    type Output: Message;
    type Future<'a>: Future<Output = Self::Output> + Send + Sync + 'a;

    fn handle<'a>(&'a mut self, message: M, context: &mut Ctx<Self>) -> Self::Future<'a>;

    /// Explain the type of scheduler the actor should use
    fn scheduler() -> Scheduler {
        Scheduler::NonBlocking
    }
}
