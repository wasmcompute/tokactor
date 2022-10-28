use crate::{Actor, Handler, Message};

pub struct Envelope<M: Message> {
    msg: Option<M>,
}

impl<M: Message> Envelope<M> {
    pub fn new(msg: M) -> Self {
        Self { msg: Some(msg) }
    }
}

pub trait SendMessage<A: Actor>: Send {
    fn send(&mut self, actor: &mut A, context: &mut A::Context);
}

/// Delivery of a message is handled by the envelope delivery service. It is
/// able to tell the type of message that is being sent in the envelope so that
/// the user only knows they are dealing with a user message. System messages
/// are handled through our own internal system.
impl<M: Message, A: Handler<M>> SendMessage<A> for Envelope<M> {
    fn send(&mut self, actor: &mut A, context: &mut A::Context) {
        actor.handle(self.msg.take().unwrap(), context);
    }
}
