use std::time::Duration;

use tokio::sync::{mpsc, watch};

use crate::{
    context::{ActorState, SupervisorMessage},
    envelope::SendMessage,
    message::DeadActor,
    single::{AskRx, AsyncAskRx},
    Actor, ActorRef, Ask, AskResult, AsyncAsk, Ctx, Handler, Message, Scheduler, SendError,
};

pub(crate) enum ExecutorLoop {
    Continue,
    Break,
}

type SupervisorReciever = watch::Receiver<Option<SupervisorMessage>>;

pub(crate) struct RawExecutor<A: Actor>(Option<Executor<A>>);

impl<A: Actor> RawExecutor<A> {
    pub fn raw_start(&mut self) {
        if let Some(executor) = self.0.as_mut() {
            executor.actor.on_start(&mut executor.context);
        } else {
            unreachable!()
        }
    }

    // pub async fn recv(&mut self) -> Option<Box<dyn SendMessage<A>>> {
    //     if let Some(executor) = self.0.as_mut() {
    //         executor.context.mailbox.recv().await
    //     } else {
    //         unreachable!()
    //     }
    // }

    // pub async fn process(&mut self, message: Option<Box<dyn SendMessage<A>>>) -> ExecutorLoop {
    //     if let Some(message) = message {
    //         let executor: Executor<A> = self.0.take().unwrap();
    //         // this process_message is not safe
    //         let (executor, event) = executor.process_message(message).await;
    //         self.0 = Some(executor);
    //         event
    //     } else {
    //         ExecutorLoop::Break
    //     }
    // }

    // pub async fn receive_messages(&mut self) -> ExecutorLoop {
    //     let mut executor: Executor<A> = self.0.take().unwrap();

    //     executor.check_anonymous_actors().await;

    //     while let Ok(message) = executor.context.mailbox.try_recv() {
    //         let (this, event) = executor.process_message(message).await;
    //         executor = this;
    //         if matches!(event, ExecutorLoop::Break) {
    //             self.0 = Some(executor);
    //             return ExecutorLoop::Break;
    //         }
    //     }

    //     self.0 = Some(executor);
    //     ExecutorLoop::Continue
    // }

    pub async fn raw_shutdown(mut self) {
        let executor = self.0.take().unwrap();

        let mut this = executor.shutdown().await;
        if let Some(tx) = this.context.into_future_sender.take() {
            let _ = tx.into_inner().send(this.actor);
        }
    }

    // TODO(Alec): Remove
    // pub fn handle<M>(&mut self, message: M)
    // where
    //     M: Message,
    //     A: Handler<M>,
    // {
    //     if let Some(executor) = self.0.as_mut() {
    //         executor.actor.handle(message, &mut executor.context)
    //     } else {
    //         unreachable!()
    //     }
    // }

    pub fn ask<M>(&mut self, message: M) -> <A as Ask<M>>::Result
    where
        M: Message,
        A: Ask<M>,
    {
        if let Some(executor) = self.0.as_mut() {
            match executor.actor.handle(message, &mut executor.context) {
                AskResult::Reply(reply) => reply,
                AskResult::Task(_) => unreachable!(), // TODO(Alec): need to fix this
            }
        } else {
            unreachable!()
        }
    }

    // TODO(Alec): Remove
    // pub fn spawn<Child>(&mut self, child: Child) -> ActorRef<Child>
    // where
    //     A: Handler<DeadActorResult<Child>>,
    //     Child: Actor,
    // {
    //     if let Some(executor) = self.0.as_mut() {
    //         executor.context.spawn(child)
    //     } else {
    //         unreachable!()
    //     }
    // }

    pub fn with_ctx<Out, F: FnOnce(&Ctx<A>) -> Out>(&self, f: F) -> Out {
        if let Some(executor) = self.0.as_ref() {
            f(&executor.context)
        } else {
            unreachable!()
        }
    }
}

pub(crate) struct Executor<A: Actor> {
    pub actor: A,
    pub context: Ctx<A>,
    pub receiver: Option<SupervisorReciever>,
}

impl<A: Actor> Executor<A> {
    pub fn into_raw(self) -> RawExecutor<A> {
        RawExecutor(Some(self))
    }

    pub fn new(actor: A, context: Ctx<A>) -> Self {
        Self {
            actor,
            context,
            receiver: None,
        }
    }

    pub fn child(actor: A, context: Ctx<A>, receiver: SupervisorReciever) -> Self {
        Self {
            actor,
            context,
            receiver: Some(receiver),
        }
    }

    pub fn into_dead_actor(mut self) -> (Option<SupervisorReciever>, DeadActor<A>) {
        (
            self.receiver.take(),
            DeadActor {
                actor: self.actor,
                ctx: self.context,
            },
        )
    }

    /// Check to see if the we are executing more anonymous actors then initally
    /// allowed to run. If we are, then wait for some anonymous tasks to complete
    /// before continuing to execute the parent actor
    async fn check_anonymous_actors(&mut self) {
        let avaliable_permits = self.context.max_anonymous_actors.available_permits();
        let overflow = self.context.overflow_anonymous_actors;
        if avaliable_permits == 0 && overflow > 0 {
            if overflow < A::max_anonymous_actors() {
                let _ = self
                    .context
                    .max_anonymous_actors
                    .acquire_many(overflow as u32)
                    .await;
                self.context.overflow_anonymous_actors = 0;
            } else {
                // TODO(Alec): The user has spawned more anonymous actors then we
                //             can relistically track...
                let _ = self
                    .context
                    .max_anonymous_actors
                    .acquire_many(A::max_anonymous_actors() as u32)
                    .await;
                self.context.overflow_anonymous_actors -= A::max_anonymous_actors();
            }
        } else if overflow > 0 {
            let running_tasks = A::max_anonymous_actors() - avaliable_permits;
            if overflow < running_tasks {
                let _ = self
                    .context
                    .max_anonymous_actors
                    .acquire_many(overflow as u32)
                    .await;
                self.context.overflow_anonymous_actors = 0;
            } else {
                // TODO(Alec): The amount of overflow tasks is more then the avaliable
                //             running tasks...
                let _ = self
                    .context
                    .max_anonymous_actors
                    .acquire_many(running_tasks as u32)
                    .await;
                self.context.overflow_anonymous_actors -= running_tasks;
            }
        }
    }

    /// Run an actor that accepts messages from it's supervisor as well as from
    /// it's mailbox. Continue processing messages until told other wise.
    pub async fn run_supervised_actor(mut self) -> Self {
        tracing::trace!(
            callback = "on_start",
            actor = A::name(),
            lifecycle = self.context.state.to_string()
        );
        self.actor.on_start(&mut self.context);
        loop {
            self.check_anonymous_actors().await;
            let (this, event) = self.handle_supervised_message().await;
            self = this;
            match event {
                ExecutorLoop::Continue => {}
                ExecutorLoop::Break => break,
            }
        }
        self.shutdown().await
    }

    /// Run an actor but only accept messages from a mailbox. This actor has no
    /// supervisor so it can not recieve messages from one. Continue to accept
    /// messges until the mailbox is closed.
    pub async fn run_actor(mut self) {
        self.actor.on_start(&mut self.context);
        while let Some(msg) = self.context.mailbox.recv().await {
            self.check_anonymous_actors().await;
            let (this, event) = self.process_message(msg).await;
            self = this;
            match event {
                ExecutorLoop::Continue => {}
                ExecutorLoop::Break => break,
            }
        }
        let mut this = self.shutdown().await;
        if let Some(tx) = this.context.into_future_sender.take() {
            let _ = tx.into_inner().send(this.actor);
        }
    }

    /// Wait for one of the following events
    ///
    /// 1. A item is recieved in our mailbox
    /// 2. We recieve a priority event from our supervisor
    ///
    /// Process the event that is recieved first. The message from the supervisor
    /// takes president if both recieve a message at the same time.
    ///
    /// Decide whether if the executor loop should continue to execute.
    async fn handle_supervised_message(mut self) -> (Self, ExecutorLoop) {
        assert!(self.receiver.is_some());
        let reciever = self.receiver.as_mut().unwrap();
        let result = tokio::select! {
            // Or recieve a message from our supervisor
            result = reciever.changed() => match result {
                Ok(_) => match *reciever.borrow() {
                    Some(SupervisorMessage::Shutdown) => {
                        tracing::trace!(
                            actor = A::name(),
                            lifecycle = self.context.state.to_string(),
                            "Recieved shutdown message"
                        );
                        ExecutorLoop::Break
                    },
                    None => ExecutorLoop::Continue,
                },
                Err(err) => {
                    panic!("Supervisor died before child. This shouldn't happen: {:?}", err)
                }
            },
            // attempt to run actor to completion
            option = self.context.mailbox.recv() => {
                if let Some(message) = option {
                    return self.process_message(message).await
                } else {
                    ExecutorLoop::Break
                }
            }
        };
        (self, result)
    }

    /// Process a single message from an actors mailbox. Depending on the state of
    /// the actor, return whether the actor should continue running.
    async fn process_message(
        mut self,
        mut message: Box<dyn SendMessage<A>>,
    ) -> (Self, ExecutorLoop) {
        tracing::trace!(
            callback = "pre_run",
            actor = A::name(),
            lifecycle = self.context.state.to_string()
        );
        self.actor.pre_run(&mut self.context);
        match message.scheduler() {
            Scheduler::Blocking => {
                // TODO(Alec): Should we panic here? I think we should as it would
                // propagate the panic up the stack. It is advaised that you should
                // not ever panic in an actor if it's in your control.
                self = tokio::task::spawn_blocking(move || {
                    tracing::debug!(
                        callback = "recv",
                        actor = A::name(),
                        message = std::any::type_name::<dyn SendMessage<A>>(),
                        lifecycle = self.context.state.to_string(),
                        schedule = "blocking",
                        "recieved blocking message"
                    );
                    message.send(&mut self.actor, &mut self.context);
                    self
                })
                .await
                .unwrap();
            }
            Scheduler::NonBlocking => {
                tracing::debug!(
                    callback = "recv",
                    actor = A::name(),
                    message = std::any::type_name::<dyn SendMessage<A>>(),
                    lifecycle = self.context.state.to_string(),
                    schedule = "non-blocking",
                    "recieved message"
                );
                message.send(&mut self.actor, &mut self.context).await;
            }
        }
        tracing::trace!(
            callback = "post_run",
            actor = A::name(),
            lifecycle = self.context.state.to_string()
        );
        self.actor.post_run(&mut self.context);

        if matches!(self.context.state, ActorState::Running) {
            (self, ExecutorLoop::Continue)
        } else {
            (self, ExecutorLoop::Break)
        }
    }

    /// Shutdown the actor by sending a message to all children to kill themselves
    /// and then wait until we have no more children left and all of our messages
    /// have been processed.
    async fn shutdown(mut self) -> Self {
        self.context.state = ActorState::Stopping;
        tracing::trace!(
            callback = "on_stopping",
            actor = A::name(),
            lifecycle = self.context.state.to_string()
        );
        self.actor.on_stopping(&mut self.context);
        self = self.stopping().await;
        self.context.state = ActorState::Stopped;
        tracing::trace!(
            callback = "on_stopped",
            actor = A::name(),
            lifecycle = self.context.state.to_string()
        );
        self.actor.on_stopped(&mut self.context);
        self = self.stop().await;
        tracing::trace!(
            callback = "on_end",
            actor = A::name(),
            lifecycle = self.context.state.to_string()
        );
        self.actor.on_end(&mut self.context);
        self
    }

    /// Called when the actors mailbox should be closed. Transition the actor
    /// into a stopping state.
    async fn stopping(mut self) -> Self {
        // We have no children, so we can just move to the stopping state. If we had
        // children, then we want to continue running and recieving messages until
        // all of our children have died.
        if self.context.notifier.is_closed() {
            // TODO(Alec): Should this be configurable. A lot of examples of other
            //             actor libraries allow for an actor to continue sending
            //             messages to itself. We could support this if we could
            //             close the mailbox only when all messages have been recieved.
            //             This would mean an actor could continue sending messages
            //             to itself until it's completed some type of test.
            //             Example of what I'm talking about: https://github.com/slawlor/ractor/blob/main/ractor/benches/actor.rs

            // We have no children. Go to ending state.
            self.context.mailbox.close();
            return self;
        }

        let _ = self
            .context
            .notifier
            .send(Some(SupervisorMessage::Shutdown));

        let mut timeout_counter = 0;
        loop {
            tokio::select! {
                option = self.context.mailbox.recv() => {
                    if let Some(msg) = option {
                        let (this, _) = self.process_message(msg).await;
                        self = this;
                    }
                }
                _ = tokio::time::sleep(Duration::from_secs(1)) => {
                    tracing::warn!(
                        actor = A::name(),
                        lifecycle = self.context.state.to_string(),
                        counter = timeout_counter,
                        time = "1sec",
                        "Pausing to allow for actors to exit"
                    );
                    if timeout_counter == 10 {
                        panic!("Timeout counter reached 10. Not all actors are exiting when they should be");
                    }
                    timeout_counter += 1;
                }
            }

            // If all of our children have died
            if self.context.notifier.is_closed() {
                self.context.mailbox.close();
                return self;
            }
        }
    }

    /// Call only when all children are dead and the actor is no longer supervising
    /// any more children. Completely empty the remaining items in the mailbox.
    async fn stop(mut self) -> Self {
        assert!(self.context.notifier.is_closed());
        while let Ok(msg) = self.context.mailbox.try_recv() {
            let (this, _) = self.process_message(msg).await;
            self = this;
        }
        self
    }
}

impl<A: Actor> Executor<A> {
    pub(crate) async fn child_with_custom_handle_rx<Parent, In>(
        mut self,
        parent: ActorRef<Parent>,
        mut rx: mpsc::Receiver<In>,
    ) where
        Parent: Actor + Handler<In>,
        In: Message,
    {
        self.actor.on_start(&mut self.context);

        loop {
            let reciever = self.receiver.as_mut().unwrap();

            let event = tokio::select! {
                // Or recieve a message from our supervisor
                result = reciever.changed() => match result {
                    Ok(_) => match *reciever.borrow() {
                        Some(SupervisorMessage::Shutdown) => ExecutorLoop::Break,
                        None => ExecutorLoop::Continue,
                    },
                    Err(err) => {
                        panic!("Supervisor died before child. This shouldn't happen: {:?}", err)
                    }
                },
                // attempt to run actor to completion
                option = rx.recv() => {
                    if let Some(message) = option {
                        if let Err(err) = parent.send_async(message).await {
                            match err {
                                SendError::Closed(_) => ExecutorLoop::Break,
                                SendError::Full(_) => ExecutorLoop::Continue,
                                SendError::Lost => ExecutorLoop::Continue,
                            }
                        } else {
                            ExecutorLoop::Continue
                        }
                    } else {
                        ExecutorLoop::Break
                    }
                }
            };

            match event {
                ExecutorLoop::Continue => {}
                ExecutorLoop::Break => break,
            }
        }

        self.shutdown().await;
    }

    pub(crate) async fn child_with_custom_ask_rx<Parent, In>(
        mut self,
        parent: ActorRef<Parent>,
        mut rx: AskRx<In, Parent>,
    ) where
        Parent: Actor + Ask<In>,
        In: Message,
    {
        self.actor.on_start(&mut self.context);

        loop {
            let reciever = self.receiver.as_mut().unwrap();

            let event = tokio::select! {
                // Or recieve a message from our supervisor
                result = reciever.changed() => match result {
                    Ok(_) => match *reciever.borrow() {
                        Some(SupervisorMessage::Shutdown) => ExecutorLoop::Break,
                        None => ExecutorLoop::Continue,
                    },
                    Err(err) => {
                        panic!("Supervisor died before child. This shouldn't happen: {:?}", err)
                    }
                },
                // attempt to run actor to completion
                option = rx.recv() => {
                    if let Some((message, rx)) = option {
                        let _ = rx.send(parent.ask(message).await);
                        ExecutorLoop::Continue
                    } else {
                        ExecutorLoop::Break
                    }
                }
            };

            match event {
                ExecutorLoop::Continue => {}
                ExecutorLoop::Break => break,
            }
        }

        self.shutdown().await;
    }

    pub(crate) async fn child_with_custom_async_ask_rx<Parent, In>(
        mut self,
        parent: ActorRef<Parent>,
        mut rx: AsyncAskRx<In, Parent>,
    ) where
        Parent: Actor + AsyncAsk<In>,
        In: Message,
    {
        self.actor.on_start(&mut self.context);

        loop {
            let reciever = self.receiver.as_mut().unwrap();

            let event = tokio::select! {
                // Or recieve a message from our supervisor
                result = reciever.changed() => match result {
                    Ok(_) => match *reciever.borrow() {
                        Some(SupervisorMessage::Shutdown) => ExecutorLoop::Break,
                        None => ExecutorLoop::Continue,
                    },
                    Err(err) => {
                        panic!("Supervisor died before child. This shouldn't happen: {:?}", err)
                    }
                },
                // attempt to run actor to completion
                option = rx.recv() => {
                    if let Some((message, rx)) = option {
                        let _ = rx.send(parent.async_ask(message).await);
                        ExecutorLoop::Continue
                    } else {
                        ExecutorLoop::Break
                    }
                }
            };

            match event {
                ExecutorLoop::Continue => {}
                ExecutorLoop::Break => break,
            }
        }

        self.shutdown().await;
    }
}
