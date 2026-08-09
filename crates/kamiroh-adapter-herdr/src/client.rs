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

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

use crate::pane::{PaneAgentState, REPORT_SOURCE};

/// Talks to one Herdr session socket, a connection at a time.
#[derive(Debug, Clone)]
pub struct Client {
    socket: PathBuf,
    next_id: u64,
}

impl Client {
    /// Prepares a client for the session socket at `path`. Connects nothing.
    pub fn new(socket: impl Into<PathBuf>) -> Self {
        Self {
            socket: socket.into(),
            next_id: 1,
        }
    }

    /// Reports `agent`'s state for `pane_id`.
    pub async fn report_agent(
        &mut self,
        pane_id: &str,
        agent: &str,
        state: PaneAgentState,
    ) -> Result<(), ClientError> {
        let id = self.next_id.to_string();
        self.next_id += 1;

        let request = serde_json::json!({
            "id": id,
            "method": "pane.report_agent",
            "params": {
                "pane_id": pane_id,
                "source": REPORT_SOURCE,
                "agent": agent,
                "state": state.as_str(),
            }
        });

        self.request(&request).await
    }

    /// Opens a connection, writes one request, reads the one response.
    async fn request(&self, request: &serde_json::Value) -> Result<(), ClientError> {
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
        Ok(())
    }

    /// The socket this client talks to.
    pub fn socket(&self) -> &Path {
        &self.socket
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tokio::net::UnixListener;
    use tokio::sync::Mutex;

    use super::*;

    /// A fake Herdr, behaving the way the real one does: one request per
    /// connection, then close. That is the behaviour these tests exist to pin —
    /// a fake that kept the connection open would have hidden the bug this
    /// module was written around.
    struct FakeHerdr {
        _dir: tempfile::TempDir,
        path: PathBuf,
        seen: Arc<Mutex<Vec<serde_json::Value>>>,
    }

    impl FakeHerdr {
        async fn replying(reply: &'static str) -> Self {
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
                    recorder
                        .lock()
                        .await
                        .push(serde_json::from_str(&line).unwrap());

                    let mut response = reply.to_owned();
                    response.push('\n');
                    let _ = stream.get_mut().write_all(response.as_bytes()).await;
                    // Closed by dropping `stream`, exactly as Herdr does.
                }
            });

            Self {
                _dir: dir,
                path,
                seen,
            }
        }

        async fn ok() -> Self {
            Self::replying(r#"{"id":"1","result":{}}"#).await
        }

        async fn requests(&self) -> Vec<serde_json::Value> {
            self.seen.lock().await.clone()
        }
    }

    #[tokio::test]
    async fn a_report_is_sent_in_the_shape_herdr_documents() {
        let herdr = FakeHerdr::ok().await;
        let mut client = Client::new(&herdr.path);

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
        let mut client = Client::new(&herdr.path);

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
        let mut client = Client::new(&herdr.path);

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
        let mut client = Client::new(&herdr.path);

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
        let mut client = Client::new(dir.path().join("absent.sock"));

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
        let mut client = Client::new(&herdr.path);

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
