use tokio::sync::watch;

use crate::{
    context::{ActorState, SupervisorMessage},
    envelope::SendMessage,
    message::DeadActor,
    Actor, Ctx, Scheduler,
};

enum ExecutorLoop {
    Continue,
    Break,
}

type SupervisorReciever = watch::Receiver<Option<SupervisorMessage>>;

pub(crate) struct Executor<A: Actor> {
    pub actor: A,
    pub context: Ctx<A>,
    pub receiver: Option<SupervisorReciever>,
}

impl<A: Actor> Executor<A> {
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

    /// Run an actor that accepts messages from it's supervisor as well as from
    /// it's mailbox. Continue processing messages until told other wise.
    pub async fn run_supervised_actor(mut self) -> Self {
        self.actor.on_start(&mut self.context);
        loop {
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
            let (this, event) = self.process_message(msg).await;
            self = this;
            match event {
                ExecutorLoop::Continue => {}
                ExecutorLoop::Break => break,
            }
        }
        let this = self.shutdown().await;
        if let Some(tx) = this.context.into_future_sender {
            let _ = tx.send(this.actor);
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
                    Some(SupervisorMessage::Shutdown) => ExecutorLoop::Break,
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
        self.actor.on_run();
        match A::scheduler() {
            Scheduler::Blocking => {
                // TODO(Alec): Should we panic here? I think we should as it would
                // propagate the panic up the stack. It is advaised that you should
                // not ever panic in an actor if it's in your control.
                self = tokio::task::spawn_blocking(move || {
                    message.send(&mut self.actor, &mut self.context);
                    self
                })
                .await
                .unwrap();
            }
            Scheduler::NonBlocking => message.send(&mut self.actor, &mut self.context),
        }
        self.actor.post_run();

        if matches!(
            self.context.state,
            ActorState::Stopping | ActorState::Stopped
        ) {
            (self, ExecutorLoop::Break)
        } else {
            (self, ExecutorLoop::Continue)
        }
    }

    /// Shutdown the actor by sending a message to all children to kill themselves
    /// and then wait until we have no more children left and all of our messages
    /// have been processed.
    async fn shutdown(mut self) -> Self {
        self.actor.on_stopping();
        self.stopping().await;
        self.actor.on_stopped();
        self.stop().await;
        self.actor.on_end();
        self
    }

    /// Called when the actors mailbox should be closed. Transition the actor
    /// into a stopping state.
    async fn stopping(&mut self) {
        // We have no children, so we can just move to the stopping state. If we had
        // children, then we want to continue running and recieving messages until
        // all of our children have died.
        if self.context.notifier.is_closed() {
            // We have no children. Go to ending state.
            self.context.mailbox.close();
            return;
        }

        let _ = self
            .context
            .notifier
            .send(Some(SupervisorMessage::Shutdown));

        while let Some(mut msg) = self.context.mailbox.recv().await {
            self.actor.on_run();
            msg.send(&mut self.actor, &mut self.context);
            self.actor.post_run();

            // If all of our children have died
            if self.context.notifier.is_closed() {
                self.context.mailbox.close();
            }
        }
    }

    /// Call only when all children are dead and the actor is no longer supervising
    /// any more children. Completely empty the remaining items in the mailbox.
    async fn stop(&mut self) {
        assert!(self.context.notifier.is_closed());
        self.context.state = ActorState::Stopped;
        while let Ok(mut msg) = self.context.mailbox.try_recv() {
            self.actor.on_run();
            msg.send(&mut self.actor, &mut self.context);
            self.actor.post_run();
        }
    }
}
