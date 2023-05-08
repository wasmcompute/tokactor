use crate::{
    message::DeadActor, Actor, ActorRef, Ask, AskError, AsyncAsk, DeadActorResult, Handler, Message,
};

enum RouterStrategyBuilder {
    RoundRobin,
}

pub struct RouterBuilder {
    max_actors: usize,
    strategy: RouterStrategyBuilder,
}

impl RouterBuilder {
    pub fn new(max_actors: usize) -> Self {
        Self {
            max_actors,
            strategy: RouterStrategyBuilder::RoundRobin,
        }
    }
}

enum RouterStrategy {
    RoundRobin { index: usize },
}

pub struct Router<A: Actor + Default> {
    max_actors: usize,
    actors: Vec<ActorRef<A>>,
    strategy: RouterStrategy,
}

impl<A: Actor + Default> Router<A> {
    pub fn new(builder: RouterBuilder) -> Self {
        Self {
            max_actors: builder.max_actors,
            actors: vec![],
            strategy: RouterStrategy::RoundRobin { index: 0 },
        }
    }
}

impl<A: Actor + Default> Actor for Router<A> {
    fn on_start(&mut self, ctx: &mut crate::Ctx<Self>)
    where
        Self: Actor,
    {
        for _ in 0..self.max_actors {
            let actor = A::default();
            self.actors.push(ctx.spawn(actor));
        }
    }
}

impl<A: Actor + Default> Handler<DeadActorResult<A>> for Router<A> {
    fn handle(&mut self, message: DeadActorResult<A>, context: &mut crate::Ctx<Self>) {
        match message {
            Ok(actor) => {
                // Actor probably existed
            }
            Err(error) => todo!(),
        }
    }
}

// impl<M: Message, A: Actor + Default + Handler<M>> Handler<M> for Router<A> {
//     fn handle(&mut self, message: M, context: &mut crate::Ctx<Self>) {
//         todo!()
//     }
// }

impl<M: Message, A: Actor + Default + AsyncAsk<M>> AsyncAsk<M> for Router<A> {
    type Result = A::Result;

    fn handle(
        &mut self,
        message: M,
        context: &mut crate::Ctx<Self>,
    ) -> crate::AsyncHandle<Self::Result> {
        match &mut self.strategy {
            RouterStrategy::RoundRobin { index } => {
                let address = self.actors[*index].clone();
                *index = (*index + 1) % self.max_actors;
                context.anonymous_handle(async move {
                    match address.async_ask(message).await {
                        Ok(result) => result,
                        Err(AskError::Closed(_msg)) => {
                            // Message failed to send to this actor because their
                            // pipe was closed. Resend the message and replace
                            // the failed actor with a new one.
                            todo!("Implement re-create and message for router")
                        }
                        Err(AskError::Dropped) => {
                            // One of the actors pipes were destroied. For now
                            // stop executing and record unexpected error state.
                            todo!("Implement unexpected exit")
                        }
                    }
                })
            }
        }
    }
}

mod tests {
    use crate::{Actor, AsyncAsk, Message};

    use super::{Router, RouterBuilder};

    static mut VAL: usize = 0;

    #[derive(Debug)]
    struct Id(());
    impl Message for Id {}

    struct ChoosenActor {
        number: usize,
    }

    impl Actor for ChoosenActor {}
    impl Message for ChoosenActor {}

    impl Default for ChoosenActor {
        fn default() -> Self {
            unsafe { VAL += 1 };
            Self {
                number: unsafe { VAL },
            }
        }
    }

    impl AsyncAsk<Id> for ChoosenActor {
        type Result = ChoosenActor;

        fn handle(&mut self, _: Id, c: &mut crate::Ctx<Self>) -> crate::AsyncHandle<Self::Result> {
            let number = self.number;
            c.anonymous_handle(async move { ChoosenActor { number } })
        }
    }

    #[tokio::test]
    async fn round_robin_router() {
        let builder = RouterBuilder::new(5);
        let router = Router::<ChoosenActor>::new(builder);
        let address = router.start();
        for _ in 0..5 {
            for i in 0..5 {
                let actor = address.async_ask(Id(())).await.unwrap();
                assert_eq!(actor.number, i + 1);
            }
        }
        let _ = address.await;
    }
}
