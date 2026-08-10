//! The controller actor: one per agent, owning that agent's lifecycle.
//!
//! Everything that mutates an agent's state goes through this actor's mailbox,
//! which is what makes the state machine safe without a lock. A prompt runs as
//! a separate task so the mailbox stays live while the agent works; that task
//! reports back *through the mailbox* rather than touching state directly, so a
//! completion and a `StopWaiting` racing each other are simply two messages in an
//! order the mailbox already decided.

use std::sync::Arc;
use std::time::Duration;

use kameo::Actor;
use kameo::actor::{ActorRef, WeakActorRef};
use kameo::error::Infallible;
use kameo::message::{Context, Message};
use kameo::reply::{DelegatedReply, ReplySender};
use kamiroh_domain::{ActorName, AgentStatus, ControlMessage, ControlReply, Payload};
use kamiroh_ports::{Agent, AgentError, AgentOutcome, ControllerError};
use tokio::task::AbortHandle;

/// How long the actor will wait for an agent to describe itself.
///
/// Generous for what it bounds — asking a local runtime what it is doing — and
/// short enough that a hung one cannot make an agent unstoppable. This is the
/// only place the actor awaits anything inline; everything slow is spawned.
const STATUS_TIMEOUT: Duration = Duration::from_secs(2);

/// How long an [`AwaitSettled`](ControlMessage::AwaitSettled) waits before
/// answering "still working, ask again".
///
/// **The node's number, not the caller's** — the verb carries no timeout, so a
/// peer cannot ask this actor to hold a waiter open for an hour.
///
/// It must stay under the Iroh front's 30s request timeout and the transport's
/// 30s reply timeout, or a remote caller is answered by a timeout instead of by
/// this node; `patience_leaves_room_under_the_front_timeout` pins that. The same
/// ceiling `HerdrAgent::DEFAULT_PATIENCE` already lives under, for the same
/// reason.
pub const SETTLE_PATIENCE: Duration = Duration::from_secs(20);

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

/// A caller waiting to be told the agent has settled.
struct Awaiting {
    /// Aborts the task waiting on [`Agent::await_settled`].
    abort: AbortHandle,
    /// Where the answer goes once the agent settles — or why it will not.
    reply: Option<ReplySender<Answer>>,
}

/// One agent's controller.
pub(crate) struct AgentActor {
    name: ActorName,
    status: AgentStatus,
    agent: Arc<dyn Agent>,
    running: Option<Running>,
    awaiting: Option<Awaiting>,
}

impl AgentActor {
    /// Builds a controller for `name`, idle and ready for work.
    pub(crate) fn new(name: ActorName, agent: Arc<dyn Agent>) -> Self {
        Self {
            name,
            status: AgentStatus::Idle,
            agent,
            running: None,
            awaiting: None,
        }
    }

    /// Abandons any in-flight prompt **and any pending await**, telling their
    /// callers why.
    ///
    /// The abort is the reason [`Agent::run`] must be cancel-safe: the task is
    /// dropped wherever it happened to be suspended.
    ///
    /// **Awaits are included deliberately.** §6d's rule is that nobody waiting
    /// is left hanging, and an await is the longest wait this actor holds. After
    /// a `Detach` the actor is gone, so anything left here would never be
    /// answered by anyone — it would sit until its caller's own timeout, with no
    /// explanation. It also keeps `StopWaiting` honest: a verb named for giving
    /// up a wait that left one running would be its own small lie.
    fn abandon(&mut self, reason: &str) {
        if let Some(awaiting) = self.awaiting.take() {
            awaiting.abort.abort();
            if let Some(reply) = awaiting.reply {
                reply.send(Err(ControllerError::Rejected {
                    actor: self.name.to_string(),
                    reason: reason.to_owned(),
                }));
            }
        }

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

    /// Answers when the agent settles, or when patience runs out.
    ///
    /// **Spawned, not awaited inline.** §6d forbids an unbounded inline await
    /// and this one is bounded only by [`SETTLE_PATIENCE`] — twenty seconds
    /// during which nothing else in the mailbox would move, leaving `Status`,
    /// `StopWaiting` and `Detach` all unanswerable. That is the hazard slice G
    /// wrote the rule for and M1 then walked into. So this uses the same
    /// machinery a prompt does: spawn, and report back through the mailbox.
    ///
    /// Independent of `running` on purpose: awaiting while a prompt is in
    /// flight is the normal case. A prompt came back `Partial{Busy}`, and this
    /// is how a caller waits for the rest without typing at the agent again.
    fn settle(
        &mut self,
        ctx: &mut Context<Self, DelegatedReply<Answer>>,
    ) -> DelegatedReply<Answer> {
        // One waiter at a time. Two would both be answered by one `Settled`,
        // and the second would hang; refusing says so instead.
        if self.awaiting.is_some() {
            return ctx.reply(Err(ControllerError::Rejected {
                actor: self.name.to_string(),
                reason: "another caller is already waiting for this agent to settle".to_owned(),
            }));
        }

        let (delegated, reply) = ctx.reply_sender();
        let agent = Arc::clone(&self.agent);
        let actor = ctx.actor_ref();

        let task = tokio::spawn(async move {
            // `tokio::time::Instant`, not `std::time`: the same clock this sleeps
            // on, so a test with paused time measures what it thinks it does.
            let started = tokio::time::Instant::now();
            let result = agent.await_settled(SETTLE_PATIENCE).await;

            // **An agent with no opinion answers instantly**, and a caller
            // long-polling one would then spin at full speed — `EchoAgent` is
            // the production stand-in for a node with no agent runtime, not
            // just a test double. The bound is honoured here rather than by the
            // port, because the port has no clock and the actor must not sleep
            // in a handler.
            if matches!(result, Ok(None)) {
                let elapsed = started.elapsed();
                if elapsed < SETTLE_PATIENCE {
                    tokio::time::sleep(SETTLE_PATIENCE - elapsed).await;
                }
            }

            let _ = actor.tell(Settled(result)).await;
        });

        self.awaiting = Some(Awaiting {
            abort: task.abort_handle(),
            reply,
        });
        delegated
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
            // StopWaiting and Detach instead of racing them. A send failure
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

            ControlMessage::AwaitSettled => self.settle(ctx),

            ControlMessage::Status => {
                // Ask the agent, because this actor's view is only as fresh as
                // the last run it finished. An agent can block without kamiroh
                // doing anything — a startup permission dialog is the case
                // that caught this — and answering `Idle` then is a guess
                // presented as a fact.
                //
                // Awaited inline, which holds the mailbox for one local round
                // trip. Worth it: the alternative is a delegated reply and a
                // second state machine for a call that should be microseconds.
                // Skipped entirely while a prompt is in flight, since then this
                // actor already knows the answer.
                //
                // **Bounded, and that bound is load-bearing.** An inline await
                // is exactly what this actor must not do without a limit: while
                // it runs, nothing else in the mailbox moves, so an agent
                // runtime that accepts a connection and never answers would
                // make `StopWaiting` and `Detach` unreachable — the agent could
                // not even be stopped. `run` is spawned and may take minutes;
                // this is asked and answered, or it is abandoned.
                //
                // `Ok(None)`, an error, and a timeout all leave the cached
                // value alone: "no better answer than yours", "could not ask"
                // and "asking took too long" are the same instruction, and none
                // of them is itself a state.
                if self.running.is_none() {
                    let agent = Arc::clone(&self.agent);
                    if let Ok(Ok(Some(status))) =
                        tokio::time::timeout(STATUS_TIMEOUT, agent.status()).await
                    {
                        self.status = status;
                    }
                }
                ctx.reply(Ok(ControlReply::Status(self.status)))
            }

            ControlMessage::StopWaiting => {
                self.abandon("kamiroh stopped waiting on the prompt");
                // **The status is deliberately not touched.** Giving up the wait
                // establishes one thing — kamiroh is no longer waiting — and
                // nothing at all about the agent, which for a real runtime
                // carries on working; only the wait was abandoned. Setting
                // `Idle` here claimed the agent was ready for work, which is
                // the direction §6d and §6e both call dangerous.
                //
                // Leaving it means the cached value stays `Busy` when a run was
                // abandoned, and unchanged when there was nothing to abandon —
                // both of which are what kamiroh actually knows. `Status` then
                // corrects it from the agent whenever the agent has an opinion.
                //
                // Safe to be conservative here because the status is a
                // *report*, never a gate: `start` refuses a second prompt on
                // `running.is_some()`, so a stale `Busy` never blocks one.
                ctx.reply(Ok(ControlReply::Accepted))
            }

            ControlMessage::Detach => {
                self.abandon("kamiroh detached from the agent");
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

/// An await finished: the agent settled, or patience ran out.
struct Settled(Result<Option<AgentStatus>, AgentError>);

impl Message<Settled> for AgentActor {
    type Reply = ();

    async fn handle(&mut self, Settled(result): Settled, _ctx: &mut Context<Self, Self::Reply>) {
        // Absent when a `StopWaiting` or `Detach` got here first and already
        // answered the caller. The abort races the agent's last step, so this
        // is normal rather than a fault.
        let Some(awaiting) = self.awaiting.take() else {
            return;
        };

        let answer = match result {
            Ok(Some(status)) => {
                tracing::debug!(agent = %self.name, ?status, "agent settled");
                // The agent is the authority on where it ended up, exactly as
                // it is for a finished run.
                self.status = status;
                Ok(ControlReply::Status(status))
            }
            // No opinion of its own, so the cached view is the best answer
            // there is. The bound was already honoured in the task.
            Ok(None) => Ok(ControlReply::Status(self.status)),
            Err(error) => {
                tracing::warn!(agent = %self.name, %error, "waiting on the agent failed");
                Err(ControllerError::Backend(Box::new(error)))
            }
        };

        if let Some(reply) = awaiting.reply {
            reply.send(answer);
        }
    }
}

impl Message<Finished> for AgentActor {
    type Reply = ();

    async fn handle(&mut self, Finished(result): Finished, _ctx: &mut Context<Self, Self::Reply>) {
        // Absent when a `StopWaiting` got here first and already answered the
        // caller. The abort races the agent's last step, so this is normal.
        let Some(running) = self.running.take() else {
            return;
        };

        let answer = match result {
            Ok(outcome) => {
                tracing::debug!(
                    agent = %self.name,
                    status = ?outcome.status,
                    output_len = outcome.output.bytes().len(),
                    "agent run finished"
                );
                // The agent is the authority on where it ended up. Assuming
                // `Idle` is what made a blocked agent indistinguishable from a
                // finished one.
                self.status = outcome.status;
                Ok(reply_for(outcome))
            }
            Err(error) => {
                tracing::warn!(agent = %self.name, %error, "agent run failed");
                // **The status is deliberately not touched**, and the comment
                // that used to sit here said why while the code did the
                // opposite: the runtime failed, so the agent's state is not
                // something we know, and claiming `Idle` invites another prompt
                // into the same failure.
                //
                // A failed run says nothing about the *agent*. The one that
                // matters is a read refused mid-task — the agent is working
                // perfectly well and only kamiroh's question failed. The cached
                // `Busy` stands, and `Status` corrects it from the agent
                // whenever the agent has an opinion.
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
