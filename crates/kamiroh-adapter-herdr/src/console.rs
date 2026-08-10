//! The pane console: lines in, replies out.
//!
//! A Herdr pane is a terminal, so the channel is plain text and the console's
//! whole job is turning a typed line into a [`ControlMessage`] and a reply back
//! into something readable.
//!
//! **A bare line is a prompt.** That is the case that has to feel like nothing
//! at all — you type, the agent answers. Because a pane drives exactly one
//! agent, no line ever has to say *which* agent it means, which is what keeps
//! the common case free of syntax.
//!
//! The other [`ControlMessage`] verbs are slash commands. They exist because
//! the Iroh front carries all of them, and a local console that could only
//! prompt would be strictly weaker than the remote path for no reason.

use std::sync::Arc;

use kamiroh_domain::{AgentStatus, ControlMessage, ControlReply, Payload};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};

use crate::link::Link;

/// What the person at the pane typed.
#[derive(Debug, PartialEq, Eq)]
enum Input {
    /// Nothing worth sending.
    Blank,
    /// End the console.
    Quit,
    /// Print the command list.
    Help,
    /// An unrecognised slash command.
    Unknown(String),
    /// A command that used to work, and what replaced it.
    ///
    /// Separate from [`Self::Unknown`] because "unknown command" is a poor
    /// answer to a word that worked yesterday. **Not an alias** — it sends
    /// nothing. It says what the verb is called now and why it changed, which
    /// is the part worth knowing: the old name described something kamiroh
    /// never did.
    Retired(&'static str),
    /// A message for the agent.
    Message(ControlMessage),
}

/// Reads `input` until it ends, driving the agent behind `link`.
///
/// `prompt` is written before each line is awaited; pass `""` for none. It is
/// the caller's choice because only the caller knows whether a person is
/// there: a terminal echoes the newline when someone presses Enter, so the
/// prompt lands on a fresh line, while piped input echoes nothing and every
/// blank line would stack another prompt on the same one.
///
/// Returns when the input reaches end-of-file or `/quit` is typed. **Reaching
/// the end of input is not an error and does not stop the node** — a kamiroh
/// process serving agents for other peers has no one at its pane, and its stdin
/// may be closed from the moment it starts. The console simply has nothing more
/// to do.
pub async fn serve<R, W>(
    input: R,
    mut output: W,
    link: Arc<dyn Link>,
    prompt: &str,
) -> std::io::Result<()>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut lines = input.lines();

    output
        .write_all(format!("talking to {} — /help for commands\n", link.describe()).as_bytes())
        .await?;

    loop {
        output.write_all(prompt.as_bytes()).await?;
        output.flush().await?;

        let Some(line) = lines.next_line().await? else {
            break;
        };

        match parse(&line) {
            Input::Blank => continue,
            Input::Quit => break,
            Input::Help => output.write_all(HELP.as_bytes()).await?,
            Input::Unknown(command) => {
                let text = format!("unknown command {command:?} — /help for the list\n");
                output.write_all(text.as_bytes()).await?;
            }
            Input::Retired(note) => {
                output.write_all(format!("{note}\n").as_bytes()).await?;
            }
            Input::Message(message) => {
                let rendered = match link.send(message).await {
                    Ok(reply) => render(&reply),
                    // Reported, never fatal. A peer being unreachable is
                    // routine when the pane is a laptop that just moved
                    // networks, and closing the console over it would be
                    // exactly the wrong response.
                    Err(error) => format!("{error}"),
                };
                output.write_all(rendered.as_bytes()).await?;
                output.write_all(b"\n").await?;
            }
        }
        output.flush().await?;
    }

    output.write_all(b"\n").await?;
    output.flush().await
}

/// The command list, as shown by `/help`.
const HELP: &str = "\
  <anything>     send as a prompt
  /status        ask what the agent is doing
  /await         wait until it finishes or needs you, then say which
  /stop-waiting  give up on the current prompt; the agent carries on
  /detach        stop controlling the agent; it carries on, unwatched
  /quit          leave this console; the node keeps running
";

/// Turns a typed line into an [`Input`].
fn parse(line: &str) -> Input {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Input::Blank;
    }

    // Only a leading slash is a command. A prompt beginning with one is
    // vanishingly rare next to the cost of reserving more of the line.
    let Some(command) = trimmed.strip_prefix('/') else {
        // The untrimmed line is sent: leading whitespace can be meaningful to
        // an agent, and it is not the console's to decide otherwise.
        return Input::Message(ControlMessage::Prompt(Payload::text(line)));
    };

    match command {
        "status" => Input::Message(ControlMessage::Status),
        "await" | "watch" => Input::Message(ControlMessage::AwaitSettled),
        "stop-waiting" => Input::Message(ControlMessage::StopWaiting),
        "interrupt" => Input::Retired(
            "/interrupt is now /stop-waiting — it never reached the agent, only kamiroh's wait",
        ),
        "detach" => Input::Message(ControlMessage::Detach),
        "shutdown" => Input::Retired(
            "/shutdown is now /detach — it never stopped the agent, only kamiroh's control of it",
        ),
        "help" | "?" => Input::Help,
        "quit" | "exit" => Input::Quit,
        _ => Input::Unknown(trimmed.to_owned()),
    }
}

/// Renders a reply for a person rather than for a log.
fn render(reply: &ControlReply) -> String {
    match reply {
        ControlReply::Accepted => "ok".to_owned(),
        ControlReply::Status(status) => format!("{status:?}").to_lowercase(),
        ControlReply::Output(payload) => render_payload(payload),
        // The state is said out loud rather than shown as text, because the
        // whole point of a partial reply is that the output alone would read
        // as a finished answer.
        ControlReply::Partial { output, status } => {
            let note = match status {
                AgentStatus::Blocked => "[waiting for you]",
                AgentStatus::Busy => "[still working]",
                other => return format!("{}\n[{other}]", render_payload(output)),
            };
            format!("{}\n{note}", render_payload(output))
        }
    }
}

/// Renders a payload as text where it is text, and as a summary where it is not.
fn render_payload(payload: &Payload) -> String {
    match payload.as_text() {
        Some(text) => text.to_owned(),
        // Agents are agent-agnostic: a payload need not be text, and spraying
        // arbitrary bytes at a terminal is how you lose a session.
        None => format!(
            "<{} bytes of {}>",
            payload.bytes().len(),
            payload.content_type()
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use async_trait::async_trait;
    use kamiroh_domain::AgentStatus;
    use kamiroh_ports::{ControlApiError, ControllerError};

    use super::*;
    use crate::link::LinkError;

    /// How a [`FakeLink`] answers.
    type Answer = Box<dyn Fn(&ControlMessage) -> Result<ControlReply, LinkError> + Send + Sync>;

    /// A link that records what it was sent and replies as told.
    struct FakeLink {
        sent: Mutex<Vec<ControlMessage>>,
        reply: Answer,
    }

    impl FakeLink {
        fn echoing() -> Arc<Self> {
            Arc::new(Self {
                sent: Mutex::new(Vec::new()),
                reply: Box::new(|message| match message {
                    ControlMessage::Prompt(payload) => Ok(ControlReply::Output(payload.clone())),
                    ControlMessage::Status => Ok(ControlReply::Status(AgentStatus::Idle)),
                    _ => Ok(ControlReply::Accepted),
                }),
            })
        }

        fn failing() -> Arc<Self> {
            Arc::new(Self {
                sent: Mutex::new(Vec::new()),
                reply: Box::new(|_| {
                    Err(LinkError::Local(ControlApiError::Controller(
                        ControllerError::NoSuchActor {
                            actor: "agent".to_owned(),
                        },
                    )))
                }),
            })
        }

        fn sent(&self) -> Vec<ControlMessage> {
            self.sent.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl Link for FakeLink {
        async fn send(&self, message: ControlMessage) -> Result<ControlReply, LinkError> {
            self.sent.lock().unwrap().push(message.clone());
            (self.reply)(&message)
        }

        fn describe(&self) -> String {
            "agent on this node".to_owned()
        }
    }

    /// Runs the console over `input`, returning everything it wrote.
    ///
    /// No prompt: these assert on what the console *says*, and a prompt before
    /// every line is noise in that.
    async fn run(input: &str, link: Arc<FakeLink>) -> String {
        let mut output = Vec::new();
        serve(input.as_bytes(), &mut output, link, "")
            .await
            .unwrap();
        String::from_utf8(output).unwrap()
    }

    #[tokio::test]
    async fn a_prompt_is_written_before_each_line_when_one_is_asked_for() {
        let mut output = Vec::new();
        serve(
            "one\ntwo\n".as_bytes(),
            &mut output,
            FakeLink::echoing(),
            "> ",
        )
        .await
        .unwrap();
        let output = String::from_utf8(output).unwrap();

        // Two lines read, plus the prompt sitting at the end awaiting a third.
        assert_eq!(output.matches("> ").count(), 3, "output was {output:?}");
    }

    #[tokio::test]
    async fn blank_lines_do_not_stack_prompts_when_there_is_no_prompt() {
        let output = run("\n\n\n", FakeLink::echoing()).await;
        assert!(!output.contains(">"), "output was {output:?}");
    }

    #[tokio::test]
    async fn a_bare_line_is_sent_as_a_prompt_and_the_output_shown() {
        let link = FakeLink::echoing();
        let output = run("hello there\n", Arc::clone(&link)).await;

        assert_eq!(
            link.sent(),
            vec![ControlMessage::Prompt(Payload::text("hello there"))]
        );
        assert!(output.contains("hello there"), "output was {output:?}");
    }

    #[tokio::test]
    async fn the_pane_says_which_agent_it_is_bound_to() {
        let output = run("", FakeLink::echoing()).await;
        assert!(output.starts_with("talking to agent on this node"));
    }

    #[tokio::test]
    async fn blank_lines_send_nothing() {
        let link = FakeLink::echoing();
        run("\n   \n\t\n", Arc::clone(&link)).await;
        assert!(link.sent().is_empty());
    }

    #[tokio::test]
    async fn slash_commands_map_to_the_other_control_verbs() {
        let link = FakeLink::echoing();
        run("/status\n/stop-waiting\n/detach\n", Arc::clone(&link)).await;

        assert_eq!(
            link.sent(),
            vec![
                ControlMessage::Status,
                ControlMessage::StopWaiting,
                ControlMessage::Detach,
            ]
        );
    }

    #[tokio::test]
    async fn a_status_reply_is_rendered_for_a_person() {
        let output = run("/status\n", FakeLink::echoing()).await;
        assert!(output.contains("idle"), "output was {output:?}");
    }

    #[tokio::test]
    async fn help_lists_the_commands_without_sending_anything() {
        let link = FakeLink::echoing();
        let output = run("/help\n", Arc::clone(&link)).await;

        assert!(link.sent().is_empty());
        assert!(output.contains("/status"));
        assert!(output.contains("/quit"));
    }

    #[tokio::test]
    async fn an_unknown_command_is_reported_and_not_sent_as_a_prompt() {
        let link = FakeLink::echoing();
        let output = run("/nonsense\n", Arc::clone(&link)).await;

        assert!(link.sent().is_empty(), "must not become a prompt");
        assert!(output.contains("unknown command"), "output was {output:?}");
    }

    #[tokio::test]
    async fn quit_ends_the_console_and_ignores_the_rest() {
        let link = FakeLink::echoing();
        run("first\n/quit\nsecond\n", Arc::clone(&link)).await;

        assert_eq!(
            link.sent(),
            vec![ControlMessage::Prompt(Payload::text("first"))],
            "nothing after /quit should be sent"
        );
    }

    /// The pane must survive a peer that is unreachable, refusing, or missing
    /// the actor — all routine when the near end is a laptop.
    #[tokio::test]
    async fn an_error_is_shown_and_the_console_keeps_going() {
        let link = FakeLink::failing();
        let output = run("one\ntwo\n", Arc::clone(&link)).await;

        assert_eq!(link.sent().len(), 2, "the console must not stop on error");
        assert!(
            output.contains("no controller actor"),
            "output was {output:?}"
        );
    }

    #[tokio::test]
    async fn a_non_text_payload_is_summarised_rather_than_dumped() {
        let link = Arc::new(FakeLink {
            sent: Mutex::new(Vec::new()),
            reply: Box::new(|_| {
                Ok(ControlReply::Output(Payload::new(
                    "application/octet-stream",
                    vec![0xff, 0xfe, 0x00, 0x01],
                )))
            }),
        });

        let output = run("give me bytes\n", link).await;
        assert!(
            output.contains("<4 bytes of application/octet-stream>"),
            "output was {output:?}"
        );
        assert!(
            !output.contains('\u{fffd}'),
            "raw bytes reached the terminal"
        );
    }

    #[tokio::test]
    async fn a_prompt_keeps_its_leading_whitespace() {
        let link = FakeLink::echoing();
        run("    indented\n", Arc::clone(&link)).await;

        assert_eq!(
            link.sent(),
            vec![ControlMessage::Prompt(Payload::text("    indented"))]
        );
    }

    #[test]
    fn parsing_is_total() {
        assert_eq!(parse(""), Input::Blank);
        assert_eq!(parse("   "), Input::Blank);
        assert_eq!(parse("/help"), Input::Help);
        assert_eq!(parse("/?"), Input::Help);
        assert_eq!(parse("/quit"), Input::Quit);
        assert_eq!(parse("/exit"), Input::Quit);
        assert_eq!(parse("/nope"), Input::Unknown("/nope".to_owned()));
        assert_eq!(
            parse("/await"),
            Input::Message(ControlMessage::AwaitSettled)
        );
        assert_eq!(
            parse("plain"),
            Input::Message(ControlMessage::Prompt(Payload::text("plain")))
        );
    }

    /// `/shutdown` explains itself rather than shrugging — and, importantly,
    /// **sends nothing**. An alias would keep the misleading word working.
    #[test]
    fn the_old_shutdown_command_says_what_replaced_it_and_sends_nothing() {
        let Input::Retired(note) = parse("/shutdown") else {
            panic!("expected a retired command, got {:?}", parse("/shutdown"));
        };
        assert!(note.contains("/detach"), "{note}");
        assert!(note.contains("never stopped the agent"), "{note}");
    }

    /// Same treatment for the other verb that was renamed for the same reason.
    #[test]
    fn the_old_interrupt_command_says_what_replaced_it_and_sends_nothing() {
        let Input::Retired(note) = parse("/interrupt") else {
            panic!("expected a retired command, got {:?}", parse("/interrupt"));
        };
        assert!(note.contains("/stop-waiting"), "{note}");
        assert!(note.contains("never reached the agent"), "{note}");
    }
}
