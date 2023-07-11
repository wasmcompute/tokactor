use crate::{Actor, ActorContext, ActorRef, AskError, AsyncAsk, DeadActorResult, Handler, Message};

pub enum RouterStrategyBuilder {
    RoundRobin,
}

pub struct RouterBuilder {
    max_retry: usize,
    max_actors: usize,
    strategy: RouterStrategyBuilder,
}

impl RouterBuilder {
    pub fn new(max_actors: usize) -> Self {
        Self {
            max_retry: 0,
            max_actors,
            strategy: RouterStrategyBuilder::RoundRobin,
        }
    }

    pub fn max_retry(mut self, max: usize) -> Self {
        self.max_retry = max;
        self
    }

    pub fn strategy(mut self, strategy: RouterStrategyBuilder) -> Self {
        self.strategy = strategy;
        self
    }
}

enum RouterStrategy {
    RoundRobin { index: usize },
}

struct ProxyFail {
    actor_index: usize,
}

pub struct Router<A: Actor + Default> {
    max_retry: usize,
    max_actors: usize,
    actors: Vec<ActorRef<A>>,
    strategy: RouterStrategy,
}

impl<A: Actor + Default> Router<A> {
    pub fn new(builder: RouterBuilder) -> Self {
        Self {
            max_retry: builder.max_retry,
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
    fn handle(&mut self, message: DeadActorResult<A>, _context: &mut crate::Ctx<Self>) {
        match message {
            Ok(_actor) => {}
            Err(_error) => todo!(),
        }
    }
}

impl<M, A> AsyncAsk<M> for Router<A>
where
    M: Message,
    A: Actor + Default + AsyncAsk<M>,
{
    type Result = A::Result;

    fn handle(
        &mut self,
        message: M,
        context: &mut crate::Ctx<Self>,
    ) -> crate::AsyncHandle<Self::Result> {
        match &mut self.strategy {
            RouterStrategy::RoundRobin { index } => {
                let max_retry = self.max_retry;
                let actor_index = *index;

                let address = self.actors[actor_index].clone();
                *index = (*index + 1) % self.max_actors;
                context.anonymous_handle(async move {
                    match address.async_ask(message).await {
                        Ok(result) => result,
                        Err(AskError::Closed(msg)) if max_retry > 0 => {
                            // Message failed to send to this actor because their
                            // pipe was closed. Resend the message and replace
                            // the failed actor with a new one.
                            let mut retry_message = msg;
                            for _ in 0..max_retry {
                                match address.async_ask(retry_message).await {
                                    Ok(result) => return result,
                                    Err(AskError::Closed(msg)) => retry_message = msg,
                                    Err(AskError::Dropped) => {
                                        break;
                                    }
                                }
                            }
                            todo!()
                            // This actor in-particular wasn't able to recieve
                            // our message. Tell Our seleves about the error.
                        }
                        Err(AskError::Dropped) | Err(AskError::Closed(_)) => {
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

impl<A> Handler<ProxyFail> for Router<A>
where
    A: Actor + Default,
{
    fn handle(&mut self, message: ProxyFail, context: &mut crate::Ctx<Self>) {
        println!(
            "Stopped processing because actor {} failed",
            message.actor_index
        );
        context.stop();
    }
}

#[cfg(test)]
mod tests {
    use crate::{Actor, AsyncAsk};

    use super::{Router, RouterBuilder};

    static mut VAL: usize = 0;

    #[derive(Debug)]
    struct Id(());

    struct ChoosenActor {
        number: usize,
    }

    impl Actor for ChoosenActor {}

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
