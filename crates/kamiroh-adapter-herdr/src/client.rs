//! A minimal client for Herdr's local socket API.
//!
//! Newline-delimited JSON, one request per line, one response per line:
//!
//! ```text
//! -> {"id":"1","method":"pane.report_agent","params":{...}}
//! <- {"id":"1","result":{...}}
//! <- {"id":"1","error":{"code":"pane_not_found","message":"pane w9:p9 not found"}}
//! ```
//!
//! Codes observed from `herdr 0.8.0` itself, not from its documentation:
//! `pane_not_found` for a pane that does not exist, and `invalid_request` for
//! an unknown method or a missing required field. They are distinct, which is
//! what makes a `pane_not_found` reply evidence that the method name and the
//! parameter set were both accepted.
//!
//! # One request per connection
//!
//! Herdr answers a request and then **closes the connection**. Three `ping`s
//! written to one socket produce one response, not three. This is not in the
//! prose documentation, and getting it wrong is invisible until the second
//! request: the first report lands, and every one after it fails with a broken
//! pipe.
//!
//! So each report opens its own connection. That is not the waste it looks
//! like — a state change happens at human speed, and a connect on a Unix
//! socket costs microseconds. Long-lived connections do exist in this API, for
//! `events.subscribe`, which kamiroh does not use.
//!
//! Only `pane.report_agent` is implemented, because that is all kamiroh needs.
//! This is not a general Herdr client and should not grow into one by accident:
//! every method added here is a piece of someone else's protocol that kamiroh
//! then has to keep up with.
//!
//! # Why `serde_json` and not a hand-written encoder
//!
//! Slice F2 hand-wrote its wire codec rather than take a serde dependency, and
//! that reasoning does **not** transfer. There the point was keeping
//! `kamiroh-domain` free of dependencies for a protocol kamiroh defines. Here
//! the JSON is Herdr's, the response shape is theirs to change, and one field
//! — the pane id — arrives from the environment unvalidated, so it must be
//! escaped by something that knows the rules. Hand-rolling a parser for another
//! project's format to save one adapter-local crate is a bad trade.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

use crate::pane::{PaneAgentState, REPORT_SOURCE};

/// Talks to one Herdr session socket, a connection at a time.
///
/// Every method takes `&self`: [`Agent::run`](kamiroh_ports::Agent::run) does,
/// so anything reachable from an agent must too. The request counter is atomic
/// rather than the whole client being behind a lock, so two calls can be in
/// flight at once — which they are, since each has its own connection anyway.
#[derive(Debug)]
pub struct Client {
    socket: PathBuf,
    next_id: AtomicU64,
}

impl Client {
    /// Prepares a client for the session socket at `path`. Connects nothing.
    pub fn new(socket: impl Into<PathBuf>) -> Self {
        Self {
            socket: socket.into(),
            next_id: AtomicU64::new(1),
        }
    }

    /// Reports `agent`'s state for `pane_id`.
    pub async fn report_agent(
        &self,
        pane_id: &str,
        agent: &str,
        state: PaneAgentState,
    ) -> Result<(), ClientError> {
        self.request(
            "pane.report_agent",
            serde_json::json!({
                "pane_id": pane_id,
                "source": REPORT_SOURCE,
                "agent": agent,
                "state": state.as_str(),
            }),
        )
        .await?;
        Ok(())
    }

    /// Prompts a Herdr-managed agent and waits for it to settle.
    ///
    /// Returns the state it settled in. `patience` bounds the wait; hitting it
    /// is not an error, it means the agent is still working.
    pub async fn prompt_agent(
        &self,
        target: &str,
        text: &str,
        patience: Duration,
        until: &[PaneAgentState],
    ) -> Result<PaneAgentState, ClientError> {
        let until: Vec<&str> = until.iter().map(|state| state.as_str()).collect();
        let result = self
            .request(
                "agent.prompt",
                serde_json::json!({
                    "target": target,
                    "text": text,
                    "wait": {
                        "until": until,
                        "timeout_ms": patience.as_millis() as u64,
                    },
                }),
            )
            .await;

        match result {
            // `agent.prompt` answers with the agent's info, so the state after
            // waiting comes back in the same round trip.
            Ok(result) => Ok(PaneAgentState::from_wire(
                result
                    .get("agent")
                    .and_then(|agent| agent.get("agent_status"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("unknown"),
            )),
            // Herdr reports an expired wait as an **error**, not as a state.
            // It is not a failure: it means the agent had not settled inside
            // the time allowed, which is precisely `working`. Treating it as an
            // error made a slow agent indistinguishable from a broken socket.
            Err(ClientError::Refused { ref code, .. }) if code == "timeout" => {
                Ok(PaneAgentState::Working)
            }
            Err(other) => Err(other),
        }
    }

    /// Asks what a Herdr-managed agent is doing right now.
    pub async fn agent_state(&self, target: &str) -> Result<PaneAgentState, ClientError> {
        let result = self
            .request("agent.get", serde_json::json!({ "target": target }))
            .await?;

        Ok(PaneAgentState::from_wire(
            result
                .get("agent")
                .and_then(|agent| agent.get("agent_status"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown"),
        ))
    }

    /// Reads the last `lines` of what a Herdr-managed agent has produced.
    pub async fn read_agent(
        &self,
        target: &str,
        source: ReadSource,
        lines: u32,
    ) -> Result<String, ClientError> {
        let result = self
            .request(
                "agent.read",
                serde_json::json!({
                    "target": target,
                    "source": source.as_str(),
                    // A maximum, not a request: with `visible` Herdr returns the
                    // screen and this only caps it. Checked against herdr 0.8.0,
                    // which answered a 200-line ask with the 57 lines on screen.
                    "lines": lines,
                    "strip_ansi": true,
                }),
            )
            .await?;

        Ok(result
            .get("read")
            .and_then(|read| read.get("text"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned())
    }

    /// Opens a connection, writes one request, reads the one response.
    ///
    /// Returns the `result` object, which every caller here reads a field out
    /// of.
    async fn request(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, ClientError> {
        let request = serde_json::json!({
            "id": self.next_id.fetch_add(1, Ordering::Relaxed).to_string(),
            "method": method,
            "params": params,
        });
        self.round_trip(&request).await
    }

    /// One connection, one request, one response.
    async fn round_trip(
        &self,
        request: &serde_json::Value,
    ) -> Result<serde_json::Value, ClientError> {
        let stream =
            UnixStream::connect(&self.socket)
                .await
                .map_err(|source| ClientError::Connect {
                    path: self.socket.display().to_string(),
                    source,
                })?;
        let mut stream = BufReader::new(stream);

        let mut line = serde_json::to_vec(request).map_err(ClientError::Encode)?;
        line.push(b'\n');
        stream.get_mut().write_all(&line).await?;
        stream.get_mut().flush().await?;

        // The response is read rather than ignored: a rejected report would
        // otherwise look exactly like a delivered one.
        let mut response = String::new();
        if stream.read_line(&mut response).await? == 0 {
            return Err(ClientError::Closed);
        }

        let response: serde_json::Value =
            serde_json::from_str(&response).map_err(ClientError::Decode)?;

        if let Some(error) = response.get("error") {
            return Err(ClientError::Refused {
                code: field(error, "code"),
                message: field(error, "message"),
            });
        }

        Ok(response
            .get("result")
            .cloned()
            .unwrap_or(serde_json::Value::Null))
    }

    /// The socket this client talks to.
    pub fn socket(&self) -> &Path {
        &self.socket
    }
}

/// Which terminal snapshot to ask Herdr for.
///
/// **Not a preference — the two are not interchangeable, and the difference is
/// what a live run found.** `Recent` includes what has scrolled off the screen,
/// which is what you want from an agent that has finished. But a coding agent
/// draws on the alternate screen, and Herdr can only capture that history by
/// scrolling it *while the agent is idle*, so it refuses a `Recent` read of a
/// working agent with `agent_not_idle`. `Visible` is always available and is the
/// only thing that can be read mid-task.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadSource {
    /// What the agent produced, including lines that have scrolled away.
    ///
    /// Refused while the agent is working.
    Recent,
    /// What is on the screen right now. Always available.
    Visible,
}

impl ReadSource {
    /// The spelling Herdr's API uses.
    fn as_str(self) -> &'static str {
        match self {
            Self::Recent => "recent",
            Self::Visible => "visible",
        }
    }
}

/// Reads a string field, tolerating anything.
///
/// This is an error path: a client that panics while explaining a failure is
/// worse than one that says "unknown".
fn field(value: &serde_json::Value, key: &str) -> String {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown")
        .to_owned()
}

/// Why a Herdr request did not succeed.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    /// The session socket could not be reached.
    #[error("could not connect to the Herdr socket at {path}: {source}")]
    Connect {
        /// The socket path that was tried.
        path: String,
        /// What the OS reported.
        #[source]
        source: std::io::Error,
    },

    /// The socket closed before replying.
    #[error("the Herdr socket closed without replying")]
    Closed,

    /// Herdr rejected the request.
    #[error("Herdr refused the request: {code}: {message}")]
    Refused {
        /// Herdr's error code.
        code: String,
        /// Herdr's message.
        message: String,
    },

    /// The request could not be encoded.
    #[error("could not encode a Herdr request: {0}")]
    Encode(#[source] serde_json::Error),

    /// The response was not the JSON we expect.
    #[error("could not decode Herdr's response: {0}")]
    Decode(#[source] serde_json::Error),

    /// The socket failed while reading or writing.
    #[error("Herdr socket I/O failed: {0}")]
    Io(#[from] std::io::Error),
}

impl ClientError {
    /// Herdr's error code, when this is a refusal rather than a transport
    /// failure.
    ///
    /// Callers match on the **code**, never on the message: the codes are a
    /// stable part of the API and are more specific than its documentation
    /// suggests, while the prose is Herdr's to reword. `pane_not_found` and
    /// `agent_not_ready` were both read this way before this.
    pub fn refusal_code(&self) -> Option<&str> {
        match self {
            Self::Refused { code, .. } => Some(code),
            _ => None,
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::VecDeque;
    use std::sync::Arc;

    use tokio::net::UnixListener;
    use tokio::sync::Mutex;

    use super::*;

    /// A fake Herdr, behaving the way the real one does: one request per
    /// connection, then close. That is the behaviour these tests exist to pin —
    /// a fake that kept the connection open would have hidden the bug this
    /// module was written around.
    pub(crate) struct FakeHerdr {
        _dir: tempfile::TempDir,
        pub(crate) path: PathBuf,
        seen: Arc<Mutex<Vec<serde_json::Value>>>,
    }

    impl FakeHerdr {
        /// Answers each connection with the next scripted reply, in order.
        ///
        /// A vector rather than one canned answer because a single
        /// `HerdrAgent::run` is two round trips — `agent.prompt` then
        /// `agent.read` — and they must be able to differ.
        pub(crate) async fn scripted(replies: Vec<String>) -> Self {
            let queue = Arc::new(Mutex::new(replies.into_iter().collect::<VecDeque<_>>()));
            Self::answering(move |_request| {
                let queue = Arc::clone(&queue);
                Box::pin(async move { queue.lock().await.pop_front() })
            })
            .await
        }

        /// Answers each request by looking at it.
        ///
        /// **This is the constructor the read bug needed and did not have.**
        /// `scripted` hands back canned replies positionally and never reads the
        /// request, so nine tests passed against an `agent.read` the real daemon
        /// rejects — it could not express "refuse *this* source and accept that
        /// one". A fake that cannot disagree with a request cannot catch a
        /// request being wrong.
        pub(crate) async fn answering<F>(answer: F) -> Self
        where
            F: Fn(
                    &serde_json::Value,
                )
                    -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<String>> + Send>>
                + Send
                + Sync
                + 'static,
        {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("herdr.sock");
            let listener = UnixListener::bind(&path).unwrap();
            let seen = Arc::new(Mutex::new(Vec::new()));
            let recorder = Arc::clone(&seen);

            tokio::spawn(async move {
                loop {
                    let Ok((stream, _)) = listener.accept().await else {
                        break;
                    };
                    let mut stream = BufReader::new(stream);
                    let mut line = String::new();
                    if stream.read_line(&mut line).await.unwrap_or(0) == 0 {
                        continue;
                    }
                    let request: serde_json::Value = serde_json::from_str(&line).unwrap();
                    recorder.lock().await.push(request.clone());

                    if let Some(mut response) = answer(&request).await {
                        response.push('\n');
                        let _ = stream.get_mut().write_all(response.as_bytes()).await;
                    }
                    // Closed by dropping `stream`, exactly as Herdr does.
                }
            });

            Self {
                _dir: dir,
                path,
                seen,
            }
        }

        /// Answers every connection with the same reply.
        async fn replying(reply: &'static str) -> Self {
            // Enough for any test here; the loop simply stops answering after.
            Self::scripted(vec![reply.to_owned(); 8]).await
        }

        async fn ok() -> Self {
            Self::replying(r#"{"id":"1","result":{}}"#).await
        }

        pub(crate) fn path(&self) -> &Path {
            &self.path
        }

        pub(crate) async fn requests(&self) -> Vec<serde_json::Value> {
            self.seen.lock().await.clone()
        }
    }

    #[tokio::test]
    async fn a_report_is_sent_in_the_shape_herdr_documents() {
        let herdr = FakeHerdr::ok().await;
        let client = Client::new(&herdr.path);

        client
            .report_agent("w1:p1", "agent", PaneAgentState::Working)
            .await
            .unwrap();

        let sent = herdr.requests().await;
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0]["method"], "pane.report_agent");
        assert_eq!(sent[0]["params"]["pane_id"], "w1:p1");
        assert_eq!(sent[0]["params"]["source"], "custom:kamiroh");
        assert_eq!(sent[0]["params"]["agent"], "agent");
        assert_eq!(sent[0]["params"]["state"], "working");
        assert!(sent[0]["id"].is_string(), "id must be present");
    }

    /// The regression this module was rewritten for. Herdr closes after each
    /// response, so a client holding one connection succeeds once and then
    /// fails with a broken pipe forever.
    #[tokio::test]
    async fn many_reports_succeed_although_herdr_closes_after_each() {
        let herdr = FakeHerdr::ok().await;
        let client = Client::new(&herdr.path);

        for state in [
            PaneAgentState::Idle,
            PaneAgentState::Working,
            PaneAgentState::Idle,
            PaneAgentState::Done,
        ] {
            client.report_agent("w1:p1", "agent", state).await.unwrap();
        }

        let sent = herdr.requests().await;
        assert_eq!(sent.len(), 4, "every report must reach Herdr");
        assert_eq!(sent[1]["params"]["state"], "working");
        assert_eq!(sent[3]["params"]["state"], "done");
    }

    #[tokio::test]
    async fn ids_are_distinct_across_requests() {
        let herdr = FakeHerdr::ok().await;
        let client = Client::new(&herdr.path);

        for _ in 0..3 {
            client
                .report_agent("w1:p1", "agent", PaneAgentState::Idle)
                .await
                .unwrap();
        }

        let ids: std::collections::HashSet<String> = herdr
            .requests()
            .await
            .iter()
            .map(|request| request["id"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(ids.len(), 3, "ids were {ids:?}");
    }

    #[tokio::test]
    async fn an_error_reply_is_surfaced_rather_than_swallowed() {
        // The real code and message from `herdr 0.8.0`.
        let herdr = FakeHerdr::replying(
            r#"{"id":"1","error":{"code":"pane_not_found","message":"pane w9:p9 not found"}}"#,
        )
        .await;
        let client = Client::new(&herdr.path);

        let error = client
            .report_agent("w9:p9", "agent", PaneAgentState::Idle)
            .await
            .unwrap_err();

        let ClientError::Refused { code, message } = &error else {
            panic!("expected Refused, got {error:?}");
        };
        assert_eq!(code, "pane_not_found");
        assert_eq!(message, "pane w9:p9 not found");
    }

    #[tokio::test]
    async fn a_missing_socket_is_a_connect_error_not_a_panic() {
        let dir = tempfile::tempdir().unwrap();
        let client = Client::new(dir.path().join("absent.sock"));

        let error = client
            .report_agent("w1:p1", "agent", PaneAgentState::Idle)
            .await
            .unwrap_err();
        assert!(matches!(error, ClientError::Connect { .. }), "{error:?}");
    }

    /// A pane id comes from the environment, so it is not ours to trust.
    #[tokio::test]
    async fn a_pane_id_with_json_metacharacters_is_escaped() {
        let herdr = FakeHerdr::ok().await;
        let client = Client::new(&herdr.path);

        let hostile = r#"w1:p1","x":"injected"#;
        client
            .report_agent(hostile, "agent", PaneAgentState::Idle)
            .await
            .unwrap();

        let sent = herdr.requests().await;
        assert_eq!(sent[0]["params"]["pane_id"], hostile);
        assert!(sent[0]["params"].get("x").is_none(), "field was injected");
    }
}
