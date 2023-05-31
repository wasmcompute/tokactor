use crate::{Actor, Ask, AskError, AsyncAsk};

mod address;
mod context;
pub mod tuple;

pub use address::{ActorAskRef, ActorAsyncAskRef, ActorSendRef};
pub use context::CtxBuilder;
use tokio::sync::{mpsc, oneshot};

pub struct Noop();
impl Actor for Noop {}

pub type AskRx<In, A> = mpsc::Receiver<(
    In,
    oneshot::Sender<Result<<A as Ask<In>>::Result, AskError<In>>>,
)>;

pub type AsyncAskRx<In, A> = mpsc::Receiver<(
    In,
    oneshot::Sender<Result<<A as AsyncAsk<In>>::Result, AskError<In>>>,
)>;

#[cfg(test)]
mod tests {
    use crate::{Actor, Ask, AsyncAsk, AsyncHandle, Ctx, Handler, Message};

    use super::context::CtxBuilder;

    trait SafeMsg: Send + Sync + Sized + std::fmt::Debug + 'static {}

    #[derive(Debug)]
    struct MsgA<A: SafeMsg>(A);
    impl<A: SafeMsg> Message for MsgA<A> {}

    #[derive(Debug)]
    struct MsgB<B: SafeMsg>(B);
    impl<B: SafeMsg> Message for MsgB<B> {}

    #[derive(Debug)]
    struct MsgC<C: SafeMsg>(C);
    impl<C: SafeMsg> Message for MsgC<C> {}

    #[derive(Debug)]
    struct Test<A: SafeMsg, B: SafeMsg, C: SafeMsg> {
        _a: A,
        _b: B,
        _c: C,
    }

    impl<A: SafeMsg, B: SafeMsg, C: SafeMsg> Actor for Test<A, B, C> {}

    impl<A: SafeMsg, B: SafeMsg, C: SafeMsg> Handler<MsgA<A>> for Test<A, B, C> {
        fn handle(&mut self, _: MsgA<A>, _: &mut Ctx<Self>) {}
    }

    impl<A: SafeMsg, B: SafeMsg, C: SafeMsg> Ask<MsgB<B>> for Test<A, B, C> {
        type Result = ();
        fn handle(&mut self, _: MsgB<B>, _: &mut Ctx<Self>) -> Self::Result {}
    }

    impl<A: SafeMsg, B: SafeMsg, C: SafeMsg> AsyncAsk<MsgC<C>> for Test<A, B, C> {
        type Result = ();

        fn handle(&mut self, _: MsgC<C>, ctx: &mut Ctx<Self>) -> AsyncHandle<Self::Result> {
            ctx.anonymous_handle(async move {})
        }
    }

    impl SafeMsg for u8 {}
    impl SafeMsg for u16 {}
    impl SafeMsg for u32 {}

    #[tokio::test]
    async fn hide_actor_underlying_implementation() {
        let test = Test {
            _a: 0_u8,
            _b: 0_u16,
            _c: 0_u32,
        };
        let ctx = CtxBuilder::new(test);
        let ctx = ctx.sender::<MsgA<u8>>();
        let ctx = ctx.asker::<MsgB<u16>>();
        let ctx = ctx.ask_asyncer::<MsgC<u32>>();
        let (a1, a2, a3) = ctx.run();
        a1.send(MsgA(1_u8)).await.unwrap();
        a2.ask(MsgB(1_u16)).await.unwrap();
        a3.ask_async(MsgC(1_u32)).await.unwrap();
    }
}
