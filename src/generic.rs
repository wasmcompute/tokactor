use crate::{
    actor::InternalHandler,
    message::{AnonymousTaskCancelled, IntoFutureShutdown},
    Actor, Ask, AsyncAsk, AsyncHandle, Ctx, Handler, Message,
};

impl<A: Actor> InternalHandler<IntoFutureShutdown<A>> for A {
    fn private_handler(&mut self, message: IntoFutureShutdown<A>, context: &mut Ctx<Self>) {
        context.halt(message.tx);
    }
}

impl<A: Actor> InternalHandler<AnonymousTaskCancelled> for A {
    fn private_handler(&mut self, message: AnonymousTaskCancelled, _: &mut Ctx<Self>) {
        // TODO(Alec): Add tracing here
        use AnonymousTaskCancelled::*;
        match message {
            Success => {}
            Cancel => println!("{} was cancelled", A::name()),
            Panic => println!("{} paniced", A::name()),
        }
    }
}

// impl<M: Message, A: Actor + Handler<M>> Ask<M> for A {
//     type Result = ();

//     fn handle(&mut self, message: M, context: &mut Ctx<Self>) {
//         self.handle(message, context);
//     }
// }

// impl<M: Message, A: Actor + Ask<M>> AsyncAsk<M> for A {
//     type Result = <A as Ask<M>>::Result;

//     fn handle(&mut self, message: M, context: &mut Ctx<Self>) -> AsyncHandle<Self::Result> {
//         let reply = self.handle(message, context);
//         context.anonymous_handle(async move { reply })
//     }
// }
