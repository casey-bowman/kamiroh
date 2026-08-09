//! The controller actor: one per agent, owning that agent's lifecycle.
//!
//! Everything that mutates an agent's state goes through this actor's mailbox,
//! which is what makes the state machine safe without a lock. A prompt runs as
//! a separate task so the mailbox stays live while the agent works; that task
//! reports back *through the mailbox* rather than touching state directly, so a
//! completion and an interrupt racing each other are simply two messages in an
//! order the mailbox already decided.

use std::sync::Arc;

use kameo::Actor;
use kameo::actor::{ActorRef, WeakActorRef};
use kameo::error::Infallible;
use kameo::message::{Context, Message};
use kameo::reply::{DelegatedReply, ReplySender};
use kamiroh_domain::{ActorName, AgentStatus, ControlMessage, ControlReply, Payload};
use kamiroh_ports::{Agent, AgentError, AgentOutcome, ControllerError};
use tokio::task::AbortHandle;

/// What a controller actor answers with. The error half is the port's own error
/// type, so nothing has to be translated on the way back out to `ControlApi`.
pub(crate) type Answer = Result<ControlReply, ControllerError>;

/// A prompt currently being run by the agent.
struct Running {
    /// Aborts the task running [`Agent::run`].
    abort: AbortHandle,
    /// Where the prompt's reply goes when it finishes — or why it did not.
    ///
    /// `None` when the prompt arrived as a `tell`, which expects no reply.
    reply: Option<ReplySender<Answer>>,
}

/// One agent's controller.
pub(crate) struct AgentActor {
    name: ActorName,
    status: AgentStatus,
    agent: Arc<dyn Agent>,
    running: Option<Running>,
}

impl AgentActor {
    /// Builds a controller for `name`, idle and ready for work.
    pub(crate) fn new(name: ActorName, agent: Arc<dyn Agent>) -> Self {
        Self {
            name,
            status: AgentStatus::Idle,
            agent,
            running: None,
        }
    }

    /// Abandons any in-flight prompt, telling its caller why.
    ///
    /// The abort is the reason [`Agent::run`] must be cancel-safe: the task is
    /// dropped wherever it happened to be suspended.
    fn abandon(&mut self, reason: &str) {
        let Some(running) = self.running.take() else {
            return;
        };
        running.abort.abort();
        if let Some(reply) = running.reply {
            reply.send(Err(ControllerError::Rejected {
                actor: self.name.to_string(),
                reason: reason.to_owned(),
            }));
        }
    }

    /// Starts `prompt` on the agent, replying only once it finishes.
    fn start(
        &mut self,
        prompt: Payload,
        ctx: &mut Context<Self, DelegatedReply<Answer>>,
    ) -> DelegatedReply<Answer> {
        // One prompt at a time. The mailbox would happily queue a second, but
        // silently serialising them would make `Busy` a lie: the caller would
        // wait with no way to tell queued from running.
        if self.running.is_some() {
            return ctx.reply(Err(ControllerError::Rejected {
                actor: self.name.to_string(),
                reason: "the agent is already running a prompt".to_owned(),
            }));
        }

        let (delegated, reply) = ctx.reply_sender();
        let agent = Arc::clone(&self.agent);
        let actor = ctx.actor_ref();

        let task = tokio::spawn(async move {
            let outcome = agent.run(prompt).await; // Result; the actor decides what it means
            // Back through the mailbox, so this transition is ordered against
            // Interrupt and Shutdown instead of racing them. A send failure
            // means the actor is already gone, which is not ours to report.
            let _ = actor.tell(Finished(outcome)).await;
        });

        self.status = AgentStatus::Busy;
        self.running = Some(Running {
            abort: task.abort_handle(),
            reply,
        });
        delegated
    }
}

impl Actor for AgentActor {
    type Args = Self;
    type Error = Infallible;

    fn name() -> &'static str {
        "kamiroh agent controller"
    }

    async fn on_start(args: Self::Args, _actor_ref: ActorRef<Self>) -> Result<Self, Self::Error> {
        Ok(args)
    }

    /// A stopping actor still owes an answer to whoever is waiting on a prompt.
    async fn on_stop(
        &mut self,
        _actor_ref: WeakActorRef<Self>,
        _reason: kameo::error::ActorStopReason,
    ) -> Result<(), Self::Error> {
        self.abandon("the controller actor stopped");
        Ok(())
    }
}

impl Message<ControlMessage> for AgentActor {
    type Reply = DelegatedReply<Answer>;

    async fn handle(
        &mut self,
        message: ControlMessage,
        ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        // A shut-down agent answers the same way whether it is still draining
        // its mailbox or already gone — `KameoController` maps a send to a dead
        // actor onto this same error. Without the explicit check the answer
        // would depend on whether the actor had finished stopping yet.
        if self.status == AgentStatus::Stopped {
            return ctx.reply(Err(ControllerError::Stopped {
                actor: self.name.to_string(),
            }));
        }

        match message {
            ControlMessage::Prompt(prompt) => self.start(prompt, ctx),

            ControlMessage::Status => {
                // Ask the agent, because this actor's view is only as fresh as
                // the last run it finished. An agent can block without kamiroh
                // doing anything — a startup permission dialog is the case
                // that caught this — and answering `Idle` then is a guess
                // presented as a fact.
                //
                // Awaited inline, which holds the mailbox for one local
                // round trip. Worth it: the alternative is a delegated reply
                // and a second state machine for a call that is microseconds.
                // Skipped entirely while a prompt is in flight, since then
                // this actor already knows the answer.
                //
                // `Ok(None)` and any error both leave the cached value alone:
                // "no better answer than yours" and "could not ask" are the
                // same instruction, and a failure to ask is not itself a state.
                if self.running.is_none() {
                    let agent = Arc::clone(&self.agent);
                    if let Ok(Some(status)) = agent.status().await {
                        self.status = status;
                    }
                }
                ctx.reply(Ok(ControlReply::Status(self.status)))
            }

            ControlMessage::Interrupt => {
                self.abandon("the prompt was interrupted");
                self.status = AgentStatus::Idle;
                ctx.reply(Ok(ControlReply::Accepted))
            }

            ControlMessage::Shutdown => {
                self.abandon("the agent was shut down");
                self.status = AgentStatus::Stopped;

                // Stopping is asked for from another task on purpose: the
                // mailbox is bounded, and an actor awaiting a send into its own
                // mailbox from inside a handler cannot drain it to make room.
                let actor = ctx.actor_ref();
                tokio::spawn(async move {
                    let _ = actor.stop_gracefully().await;
                });

                ctx.reply(Ok(ControlReply::Accepted))
            }
        }
    }
}

/// A run came back. Internal: sent by the task, never from outside.
///
/// "Finished" names the *run*, not the agent — a run can end with the agent
/// blocked or still working, which is the whole point of [`AgentOutcome`].
struct Finished(Result<AgentOutcome, AgentError>);

impl Message<Finished> for AgentActor {
    type Reply = ();

    async fn handle(&mut self, Finished(result): Finished, _ctx: &mut Context<Self, Self::Reply>) {
        // Absent when an interrupt got here first and already answered the
        // caller. The abort races the agent's last step, so this is normal.
        let Some(running) = self.running.take() else {
            return;
        };

        let answer = match result {
            Ok(outcome) => {
                // The agent is the authority on where it ended up. Assuming
                // `Idle` is what made a blocked agent indistinguishable from a
                // finished one.
                self.status = outcome.status;
                Ok(reply_for(outcome))
            }
            Err(error) => {
                // The runtime failed, so the agent's state is not something we
                // know — and claiming `Idle` would invite another prompt into
                // the same failure.
                self.status = AgentStatus::Idle;
                Err(ControllerError::Backend(Box::new(error)))
            }
        };

        if let Some(reply) = running.reply {
            reply.send(answer);
        }
    }
}

/// Turns an outcome into the narrowest reply that is still true.
///
/// `Output` claims the agent is done, so it is only used when it is.
fn reply_for(outcome: AgentOutcome) -> ControlReply {
    if outcome.is_finished() {
        ControlReply::Output(outcome.output)
    } else {
        ControlReply::Partial {
            output: outcome.output,
            status: outcome.status,
        }
    }
}
