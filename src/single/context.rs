use tokio::sync::mpsc;

use crate::{Actor, Ask, AsyncAsk, Ctx, DeadActorResult, Handler, Message};

use super::{
    address::{ActorAskRef, ActorAsyncAskRef, ActorSendRef},
    tuple::Tuple,
};

pub struct CtxBuilder<A: Actor, B: Tuple> {
    actor: A,
    context: Ctx<A>,
    senders: B,
}

impl<A: Actor> CtxBuilder<A, ()> {
    pub fn new(actor: A) -> Self {
        let context = Ctx::new();
        Self {
            actor,
            context,
            senders: (),
        }
    }
}

#[allow(clippy::type_complexity)]
impl<A: Actor, B: Tuple> CtxBuilder<A, B> {
    pub fn sender<In: Message>(self) -> CtxBuilder<A, (ActorSendRef<In>, B)>
    where
        A: Handler<In>,
        (ActorSendRef<In>, B): Tuple,
    {
        let (tx, rx) = mpsc::channel::<In>(A::mailbox_size());
        let sender = ActorSendRef::new(tx);
        self.context.spawn_anonymous_rx_handle(rx);
        CtxBuilder {
            actor: self.actor,
            context: self.context,
            senders: (sender, self.senders),
        }
    }

    pub fn asker<In: Message>(self) -> CtxBuilder<A, (ActorAskRef<In, <A as Ask<In>>::Result>, B)>
    where
        A: Ask<In>,
        (ActorAskRef<In, <A as Ask<In>>::Result>, B): Tuple,
    {
        let (tx, rx) = mpsc::channel(A::mailbox_size());
        let sender = ActorAskRef::new(tx);
        self.context.spawn_anonymous_rx_ask(rx);
        CtxBuilder {
            actor: self.actor,
            context: self.context,
            senders: (sender, self.senders),
        }
    }

    pub fn ask_asyncer<In: Message>(
        self,
    ) -> CtxBuilder<A, (ActorAsyncAskRef<In, <A as AsyncAsk<In>>::Result>, B)>
    where
        A: AsyncAsk<In>,
        (ActorAsyncAskRef<In, <A as AsyncAsk<In>>::Result>, B): Tuple,
    {
        let (tx, rx) = mpsc::channel(A::mailbox_size());
        let sender = ActorAsyncAskRef::new(tx);
        self.context.spawn_anonymous_rx_async(rx);
        CtxBuilder {
            actor: self.actor,
            context: self.context,
            senders: (sender, self.senders),
        }
    }

    pub fn spawn<P>(self, ctx: &Ctx<P>) -> B::Output
    where
        P: Actor + Handler<DeadActorResult<A>>,
    {
        ctx.spawn_with(self.context, self.actor);
        self.senders.flatten()
    }

    pub fn run(self) -> B::Output {
        self.context.run(self.actor);
        self.senders.flatten()
    }
}
