//! The frames of the app-server's stdio protocol: JSON-RPC 2.0 shapes
//! without the `jsonrpc` member, one JSON object per line in each
//! direction. A request carries `id` and `method`, an answer carries the
//! same `id` with `result` or `error`, a notification carries `method`
//! alone. Both sides send requests: the app-server asks the client to run a
//! dynamic tool (`item/tool/call`) and waits for the answer, so a frame is
//! classified by which members it has, never by who is expected to speak.
//!
//! The shapes here are the ones codex [`CODEX_VERSION`](super::CODEX_VERSION)
//! writes; the fixture under `../fixtures` records them line by line.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// The id of a request this client sent. The app-server accepts numbers and
/// strings; this client numbers its own requests and echoes the app-server's
/// ids, whatever their type, when it answers.
pub type RequestId = u64;

/// The first request on a fresh connection.
pub const INITIALIZE: &str = "initialize";
/// The notification that follows a successful `initialize`.
pub const INITIALIZED: &str = "initialized";
/// Stop the running turn of a thread; the app-server answers `{}` and then
/// ends the turn with a `turn/completed` whose status is `interrupted`.
pub const TURN_INTERRUPT: &str = "turn/interrupt";

/// The error object the app-server answers a request with.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// One line the app-server wrote.
#[derive(Clone, Debug, PartialEq)]
pub enum Frame {
    /// The answer to a request this client sent.
    Response {
        id: RequestId,
        outcome: Result<Value, RpcError>,
    },
    /// A request the app-server sent; it waits for an answer with this id.
    Request {
        id: Value,
        method: String,
        params: Value,
    },
    /// A streamed event, such as a turn's items and deltas.
    Notification { method: String, params: Value },
}

/// Why a line is not a frame.
#[derive(Clone, Debug, PartialEq)]
pub enum FrameError {
    /// Not a JSON object at all: a log line, a prompt, half a message.
    NotJson(String),
    /// A JSON object without the members that make it a frame.
    Shape(String),
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotJson(line) => write!(f, "not a JSON object: {line:?}"),
            Self::Shape(why) => write!(f, "{why}"),
        }
    }
}

/// Read one line the app-server wrote. Unknown members (`emittedAtMs` on
/// notifications, whatever a later release adds) are ignored.
pub fn parse_frame(line: &str) -> Result<Frame, FrameError> {
    let not_json = || FrameError::NotJson(line.chars().take(120).collect());
    let Ok(Value::Object(mut frame)) = serde_json::from_str::<Value>(line) else {
        return Err(not_json());
    };
    let id = frame.remove("id");
    let method = match frame.remove("method") {
        Some(Value::String(method)) => Some(method),
        Some(other) => {
            return Err(FrameError::Shape(format!(
                "method is not a string: {other}"
            )));
        }
        None => None,
    };
    let params = frame
        .remove("params")
        .unwrap_or_else(|| Value::Object(Map::new()));
    match (method, id) {
        (Some(method), Some(id)) => Ok(Frame::Request { id, method, params }),
        (Some(method), None) => Ok(Frame::Notification { method, params }),
        (None, Some(id)) => {
            let id = id.as_u64().ok_or_else(|| {
                FrameError::Shape(format!("response id {id} is not one this client sends"))
            })?;
            let outcome = match (frame.remove("result"), frame.remove("error")) {
                (Some(result), _) => Ok(result),
                (None, Some(error)) => {
                    Err(serde_json::from_value::<RpcError>(error).map_err(|error| {
                        FrameError::Shape(format!("malformed error object: {error}"))
                    })?)
                }
                (None, None) => {
                    return Err(FrameError::Shape(format!(
                        "response {id} has neither result nor error"
                    )));
                }
            };
            Ok(Frame::Response { id, outcome })
        }
        (None, None) => Err(FrameError::Shape(
            "object has neither method nor id".to_owned(),
        )),
    }
}

/// The line that sends request `id` for `method`.
pub fn request_line(id: RequestId, method: &str, params: Value) -> String {
    let mut frame = Map::new();
    frame.insert("id".into(), Value::from(id));
    frame.insert("method".into(), Value::from(method));
    frame.insert("params".into(), params);
    line(frame)
}

/// The line that sends a notification.
pub fn notification_line(method: &str, params: Value) -> String {
    let mut frame = Map::new();
    frame.insert("method".into(), Value::from(method));
    frame.insert("params".into(), params);
    line(frame)
}

/// The line that answers the app-server's request `id`.
pub fn response_line(id: &Value, outcome: Result<Value, RpcError>) -> String {
    let mut frame = Map::new();
    match outcome {
        Ok(result) => {
            frame.insert("id".into(), id.clone());
            frame.insert("result".into(), result);
        }
        Err(error) => {
            // The live shape puts `error` before `id`.
            frame.insert(
                "error".into(),
                serde_json::to_value(error).expect("an error object serializes"),
            );
            frame.insert("id".into(), id.clone());
        }
    }
    line(frame)
}

/// One JSON object per line: the app-server reads up to the newline, so a
/// frame must never contain one. `serde_json` escapes newlines inside
/// strings and emits none of its own, which this keeps visible.
fn line(frame: Map<String, Value>) -> String {
    let mut text = Value::Object(frame).to_string();
    debug_assert!(!text.contains('\n'));
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_result_answers_the_request_with_that_id() {
        // The live shape of the `initialize` answer from codex-cli 0.156.1.
        let line = r#"{"id":1,"result":{"userAgent":"nolune/0.156.1 (Mac OS 26.5.2; arm64)","codexHome":"/home/u/.codex","platformFamily":"unix","platformOs":"macos"}}"#;
        assert_eq!(
            parse_frame(line),
            Ok(Frame::Response {
                id: 1,
                outcome: Ok(json!({
                    "userAgent": "nolune/0.156.1 (Mac OS 26.5.2; arm64)",
                    "codexHome": "/home/u/.codex",
                    "platformFamily": "unix",
                    "platformOs": "macos"
                })),
            })
        );
    }

    #[test]
    fn an_error_answers_the_request_even_when_the_id_comes_last() {
        // The live shape of a request sent before `initialize`.
        let line = r#"{"error":{"code":-32600,"message":"Not initialized"},"id":7}"#;
        assert_eq!(
            parse_frame(line),
            Ok(Frame::Response {
                id: 7,
                outcome: Err(RpcError {
                    code: -32600,
                    message: "Not initialized".into(),
                    data: None,
                }),
            })
        );
        let with_data = r#"{"id":8,"error":{"code":-32000,"message":"thread not found","data":{"threadId":"t1"}}}"#;
        assert_eq!(
            parse_frame(with_data),
            Ok(Frame::Response {
                id: 8,
                outcome: Err(RpcError {
                    code: -32000,
                    message: "thread not found".into(),
                    data: Some(json!({"threadId": "t1"})),
                }),
            })
        );
    }

    #[test]
    fn a_method_without_an_id_is_a_notification_whatever_else_it_carries() {
        let line = r#"{"method":"item/agentMessage/delta","params":{"threadId":"t1","turnId":"u1","itemId":"i1","delta":"Hel"},"emittedAtMs":1789917827501}"#;
        assert_eq!(
            parse_frame(line),
            Ok(Frame::Notification {
                method: "item/agentMessage/delta".into(),
                params: json!({"threadId": "t1", "turnId": "u1", "itemId": "i1", "delta": "Hel"}),
            })
        );
        // No params at all is an empty object, not a failure.
        assert_eq!(
            parse_frame(r#"{"method":"initialized"}"#),
            Ok(Frame::Notification {
                method: "initialized".into(),
                params: Value::Object(Map::new()),
            })
        );
    }

    #[test]
    fn a_method_with_an_id_is_a_request_from_the_app_server() {
        // The app-server numbers its own requests; string ids are kept too.
        let line = r#"{"id":0,"method":"item/tool/call","params":{"threadId":"t1","turnId":"u1","callId":"c1","tool":"read_memory","arguments":{"query":"x"}}}"#;
        assert_eq!(
            parse_frame(line),
            Ok(Frame::Request {
                id: json!(0),
                method: "item/tool/call".into(),
                params: json!({"threadId": "t1", "turnId": "u1", "callId": "c1", "tool": "read_memory", "arguments": {"query": "x"}}),
            })
        );
        assert!(matches!(
            parse_frame(r#"{"id":"call-9","method":"item/tool/call","params":{}}"#),
            Ok(Frame::Request { id, .. }) if id == json!("call-9")
        ));
    }

    #[test]
    fn lines_that_are_not_frames_say_why() {
        assert!(matches!(
            parse_frame("running 1 test"),
            Err(FrameError::NotJson(_))
        ));
        assert!(matches!(parse_frame("[1,2]"), Err(FrameError::NotJson(_))));
        assert!(matches!(parse_frame(""), Err(FrameError::NotJson(_))));
        // An object that is neither a response nor a message.
        assert!(matches!(
            parse_frame(r#"{"result":{}}"#),
            Err(FrameError::Shape(_))
        ));
        assert!(matches!(
            parse_frame(r#"{"id":1}"#),
            Err(FrameError::Shape(_))
        ));
        // A response to an id this client never uses.
        assert!(matches!(
            parse_frame(r#"{"id":"abc","result":{}}"#),
            Err(FrameError::Shape(_))
        ));
        assert!(matches!(
            parse_frame(r#"{"id":-1,"result":{}}"#),
            Err(FrameError::Shape(_))
        ));
        // A malformed error object.
        assert!(matches!(
            parse_frame(r#"{"id":1,"error":"boom"}"#),
            Err(FrameError::Shape(_))
        ));
    }

    #[test]
    fn outgoing_lines_have_no_jsonrpc_member_and_end_with_one_newline() {
        let request = request_line(3, "thread/start", json!({"model": "gpt-5.5"}));
        assert_eq!(
            request,
            "{\"id\":3,\"method\":\"thread/start\",\"params\":{\"model\":\"gpt-5.5\"}}\n"
        );
        assert!(!request.contains("jsonrpc"));
        assert_eq!(
            notification_line(INITIALIZED, Value::Object(Map::new())),
            "{\"method\":\"initialized\",\"params\":{}}\n"
        );
        assert_eq!(
            response_line(&json!(0), Ok(json!({"success": true}))),
            "{\"id\":0,\"result\":{\"success\":true}}\n"
        );
        assert_eq!(
            response_line(
                &json!("call-9"),
                Err(RpcError {
                    code: -32601,
                    message: "no such tool".into(),
                    data: None,
                })
            ),
            "{\"error\":{\"code\":-32601,\"message\":\"no such tool\"},\"id\":\"call-9\"}\n"
        );
        // Exactly one newline, and only at the end: the reader splits on it.
        let text = request_line(
            4,
            "turn/start",
            json!({"input": [{"type": "text", "text": "a\nb"}]}),
        );
        assert_eq!(text.matches('\n').count(), 1);
        assert!(text.ends_with('\n'));
    }

    #[test]
    fn a_frame_round_trips_through_its_own_encoding() {
        let sent = request_line(11, "model/list", json!({}));
        assert!(matches!(
            parse_frame(sent.trim_end()),
            Ok(Frame::Request { id, method, params })
                if id == json!(11) && method == "model/list" && params == json!({})
        ));
    }
}
