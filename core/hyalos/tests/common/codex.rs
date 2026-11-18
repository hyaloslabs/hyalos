//! A scripted OpenAI Responses model for Codex in the sandbox image: answer its first call with one
//! `exec_command`, and the next with "Done.", streamed or not as Codex asked.

use serde_json::{Value, json};

pub fn response(status: &str, output: Value) -> Value {
    json!({
        "id": "resp_scripted", "object": "response", "created_at": 1_700_000_000, "model": "scripted",
        "status": status, "output": output,
        "usage": {
            "input_tokens": 10, "output_tokens": 2, "total_tokens": 12,
            "input_tokens_details": { "cached_tokens": 0 },
            "output_tokens_details": { "reasoning_tokens": 0 }
        }
    })
}

pub fn sse(events: &[Value]) -> String {
    events
        .iter()
        .map(|e| format!("event: {}\ndata: {e}\n\n", e["type"].as_str().unwrap_or("")))
        .collect()
}

pub fn run_command(script: &str, stream: bool) -> String {
    let arguments = json!({ "cmd": script }).to_string();
    let call = |status: &str, arguments: &str| {
        json!({
            "type": "function_call", "id": "fc_scripted", "call_id": "call_scripted",
            "name": "exec_command", "arguments": arguments, "status": status
        })
    };
    let finished = call("completed", &arguments);
    if !stream {
        return response("completed", json!([finished])).to_string();
    }
    sse(&[
        json!({ "type": "response.created", "response": response("in_progress", json!([])) }),
        json!({ "type": "response.output_item.added", "output_index": 0, "item": call("in_progress", "") }),
        json!({ "type": "response.function_call_arguments.delta", "output_index": 0, "item_id": "fc_scripted", "delta": arguments }),
        json!({ "type": "response.function_call_arguments.done", "output_index": 0, "item_id": "fc_scripted", "arguments": arguments }),
        json!({ "type": "response.output_item.done", "output_index": 0, "item": finished }),
        json!({ "type": "response.completed", "response": response("completed", json!([finished])) }),
    ])
}

pub fn done(stream: bool) -> String {
    let message = json!({
        "type": "message", "id": "msg_scripted", "role": "assistant", "status": "completed",
        "content": [{ "type": "output_text", "text": "Done.", "annotations": [] }]
    });
    if !stream {
        return response("completed", json!([message])).to_string();
    }
    sse(&[
        json!({ "type": "response.created", "response": response("in_progress", json!([])) }),
        json!({ "type": "response.output_item.added", "output_index": 0, "item": { "type": "message", "id": "msg_scripted", "role": "assistant", "status": "in_progress", "content": [] } }),
        json!({ "type": "response.output_text.delta", "output_index": 0, "content_index": 0, "item_id": "msg_scripted", "delta": "Done." }),
        json!({ "type": "response.output_item.done", "output_index": 0, "item": message }),
        json!({ "type": "response.completed", "response": response("completed", json!([message])) }),
    ])
}
