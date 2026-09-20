//! The channel to one Cua Driver process.
//!
//! [`DriverTransport`] is the only thing the driver client needs: send a tool
//! call, get the structured payload back. The production implementation
//! speaks MCP over stdio to a `cua-driver mcp` child; tests use an in-memory
//! fake so no driver binary is ever required.

use std::{future::Future, path::Path, pin::Pin};

use cua_protocol::driver_mcp::DriverCallFailure;
use rmcp::model::CallToolResult;
use serde_json::{Map, Value};

/// The structured payload of one tool call, or why there is none.
pub type CallOutcome = Result<Value, DriverCallFailure>;

pub type TransportFuture<'a> = Pin<Box<dyn Future<Output = CallOutcome> + Send + 'a>>;

/// One driver process the client can call tools on.
pub trait DriverTransport: Send + Sync {
    /// Call one MCP tool by its driver name and return its structured payload.
    fn call_tool(&self, name: &str, arguments: Map<String, Value>) -> TransportFuture<'_>;
}

/// The structured payload of an MCP `tools/call` result.
///
/// The driver puts the typed result in `structuredContent` and a human
/// rendering in the text block, so the text is only consulted when it is
/// itself JSON. An `isError` result carries the driver's structured `code`
/// beside its text.
pub(crate) fn payload_from_call_result(result: CallToolResult) -> CallOutcome {
    let _ = result;
    todo!("slice 2: CallToolResult -> payload")
}

/// MCP over stdio to one persistent `cua-driver mcp` child process.
pub struct StdioDriverTransport {
    _private: (),
}

impl StdioDriverTransport {
    /// Spawn `<driver> mcp` and complete the MCP handshake.
    pub async fn spawn(driver: &Path) -> anyhow::Result<Self> {
        let _ = driver;
        todo!("slice 2: spawn cua-driver mcp over rmcp stdio")
    }

    /// Stop the driver child; the process is killed when the connection drops.
    pub fn shutdown(self) {
        todo!("slice 2: shut the child down")
    }
}

impl DriverTransport for StdioDriverTransport {
    fn call_tool(&self, name: &str, arguments: Map<String, Value>) -> TransportFuture<'_> {
        let _ = (name, arguments);
        todo!("slice 2: rmcp call_tool")
    }
}

#[cfg(test)]
pub(crate) mod fake {
    //! An in-memory driver for tests: records every call and answers from a
    //! queue of canned outcomes.

    use super::*;
    use std::{collections::VecDeque, sync::Mutex};

    #[derive(Default)]
    pub struct FakeTransport {
        calls: Mutex<Vec<(String, Map<String, Value>)>>,
        outcomes: Mutex<VecDeque<CallOutcome>>,
    }

    impl FakeTransport {
        pub fn answering(outcomes: impl IntoIterator<Item = CallOutcome>) -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                outcomes: Mutex::new(outcomes.into_iter().collect()),
            }
        }

        /// Every `(tool, arguments)` pair in the order it was called.
        pub fn calls(&self) -> Vec<(String, Map<String, Value>)> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl DriverTransport for FakeTransport {
        fn call_tool(&self, name: &str, arguments: Map<String, Value>) -> TransportFuture<'_> {
            self.calls
                .lock()
                .unwrap()
                .push((name.to_owned(), arguments));
            let outcome = self
                .outcomes
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| {
                    Err(DriverCallFailure::Transport(format!(
                        "fake driver has no answer for {name}"
                    )))
                });
            Box::pin(async move { outcome })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::Content;
    use serde_json::json;

    #[test]
    fn structured_content_is_the_payload() {
        let payload = json!({"apps": []});
        assert_eq!(
            payload_from_call_result(CallToolResult::structured(payload.clone())),
            Ok(payload)
        );
    }

    #[test]
    fn text_only_json_results_are_parsed() {
        let result = CallToolResult::success(vec![Content::text(r#"{"apps": []}"#)]);
        assert_eq!(payload_from_call_result(result), Ok(json!({"apps": []})));
    }

    #[test]
    fn human_text_without_structured_content_is_malformed() {
        let result = CallToolResult::success(vec![Content::text("✅ cua-driver 0.28.2 — ok")]);
        assert!(matches!(
            payload_from_call_result(result),
            Err(DriverCallFailure::Malformed(_))
        ));
        let empty = CallToolResult::success(vec![]);
        assert!(matches!(
            payload_from_call_result(empty),
            Err(DriverCallFailure::Malformed(_))
        ));
    }

    #[test]
    fn error_results_carry_the_structured_code_and_the_text() {
        // The shape the live 0.28.2 driver returns for a closed window.
        let mut result = CallToolResult::error(vec![Content::text(
            "window_id 1 is not a live window (closed, or the id is stale).",
        )]);
        result.structured_content = Some(json!({
            "code": "window_id_not_found", "pid": 1, "window_id": 1,
            "suggestion": "call list_windows for current window_ids"
        }));
        assert_eq!(
            payload_from_call_result(result),
            Err(DriverCallFailure::Tool {
                code: Some("window_id_not_found".into()),
                message: "window_id 1 is not a live window (closed, or the id is stale).".into(),
            })
        );

        let bare = CallToolResult::error(vec![Content::text("boom")]);
        assert_eq!(
            payload_from_call_result(bare),
            Err(DriverCallFailure::Tool {
                code: None,
                message: "boom".into(),
            })
        );
    }

    #[test]
    fn spawning_a_missing_driver_fails_instead_of_hanging() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let missing = std::env::temp_dir().join("nolune-no-such-cua-driver");
        let error = runtime
            .block_on(StdioDriverTransport::spawn(&missing))
            .err()
            .expect("a missing binary cannot be spawned");
        assert!(!error.to_string().is_empty());
    }
}
