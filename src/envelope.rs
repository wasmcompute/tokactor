use std::any::Any;

use tokio::sync::oneshot;

use crate::{actor::Ask, Actor, Ctx, Handler, Message};

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

pub struct Response<M: Message, R: Message> {
    msg: Envelope<M>,
    tx: Option<oneshot::Sender<R>>,
}

impl<M: Message, R: Message> Response<M, R> {
    pub fn new(msg: Envelope<M>, tx: oneshot::Sender<R>) -> Self {
        Self { msg, tx: Some(tx) }
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

// impl<A: Ask<M>, M: Message> Handler<M> for A {
//     fn handle(&mut self, message: M, context: &mut Ctx<Self>) {
//         let _ = self.handle(message, context);
//     }
// }

/// Delivery of a message that is asking for a response of some kind.
impl<M: Message, A: Ask<M>> SendMessage<A> for Response<M, A::Result> {
    fn send(&mut self, actor: &mut A, context: &mut Ctx<A>) {
        let message = self.msg.unwrap();
        let response = actor.handle(message, context);
        let _ = self.tx.take().unwrap().send(response);
        // TODO(Alec): Add tracing
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}
