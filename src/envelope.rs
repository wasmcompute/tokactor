use std::any::Any;

use crate::{Actor, Ctx, Handler, Message};

/// Contain a message so that it can be accessed by the framework.
pub struct Envelope<M: Message> {
    msg: Option<M>,
}

impl<M: Message> Envelope<M> {
    pub fn new(msg: M) -> Self {
        Self { msg: Some(msg) }
    }

    pub fn unwrap(&mut self) -> M {
        self.msg.take().unwrap()
    }
}

pub trait SendMessage<A: Actor>: Send + Sync {
    fn send(&mut self, actor: &mut A, context: &mut Ctx<A>);

    fn as_any(&mut self) -> &mut dyn Any;
}

/// Delivery of a message is handled by the envelope delivery service. It is
/// able to tell the type of message that is being sent in the envelope so that
/// the user only knows they are dealing with a user message. System messages
/// are handled through our own internal system.
impl<M: Message, A: Handler<M>> SendMessage<A> for Envelope<M> {
    fn send(&mut self, actor: &mut A, context: &mut Ctx<A>) {
        actor.handle(self.msg.take().unwrap(), context);
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

// impl<A: Actor> &(dyn SendMessage<A> + 'static) {
//     pub fn as_any(self) -> &mut (dyn Any + 'static) {
//         self
//     }
// }

trait SendBackMessage<A: Actor, M: Message>
where
    Self: Any + Sized + 'static,
{
    fn restore(self) -> M;
}

impl<M: Message, A: Actor> SendBackMessage<A, M> for Box<dyn Any + 'static> {
    fn restore(self) -> M {
        self.downcast::<Envelope<M>>().unwrap().unwrap()
    }
}
