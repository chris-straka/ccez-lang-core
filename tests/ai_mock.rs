//! The AI client against a stand-in server on this machine: what it
//! sends, and how it reads a streamed answer. A fake key; never the real
//! API.
#![cfg(feature = "net")]

use ccez_lang_core::ai::{self, Ai, Event, Mode};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Serves one request per answer in `answers` (status, body), handing
/// back each request as text.
async fn stand_in(answers: Vec<(u16, String)>) -> (String, tokio::task::JoinHandle<Vec<String>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let mut seen = Vec::new();
        for (status, body) in answers {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut req = Vec::new();
            let mut buf = [0u8; 8192];
            loop {
                let n = sock.read(&mut buf).await.unwrap();
                req.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&req).to_string();
                if let Some(head_end) = text.find("\r\n\r\n") {
                    let len = text[..head_end]
                        .lines()
                        .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap()))
                        .unwrap_or(0);
                    if req.len() >= head_end + 4 + len {
                        break;
                    }
                }
                if n == 0 {
                    break;
                }
            }
            seen.push(String::from_utf8_lossy(&req).to_string());
            let kind = if status == 200 { "text/event-stream" } else { "application/json" };
            let out = format!("HTTP/1.1 {status} X\r\ncontent-type: {kind}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len());
            sock.write_all(out.as_bytes()).await.unwrap();
            sock.shutdown().await.ok();
        }
        seen
    });
    (url, task)
}

fn sse(chunks: &[&str]) -> String {
    chunks.iter().map(|c| format!("data: {c}\n\n")).collect::<String>() + "data: [DONE]\n\n"
}

#[tokio::test]
async fn ask_streams_with_the_key_light_thinking_and_web_tools() {
    let body = sse(&[
        r#"{"choices":[{"index":0,"delta":{},"server_tool_event":{"type":"call_started","step":0,"call":{"action":{"type":"search","queries":["verlan"]}}}}]}"#,
        r#"{"choices":[{"index":0,"delta":{"content":"Back"}}]}"#,
        r#"{"choices":[{"index":0,"delta":{"content":"slang.","server_tool_calls":[{"index":0,"calls":[{"action":{"type":"search","queries":["verlan"]},"sources":[{"url":"https://a.example/v","title":"Verlan"}]}]}]},"finish_reason":"stop"}]}"#,
    ]);
    let (url, server) = stand_in(vec![(200, body)]).await;
    let ai = Ai::new(&url, "", "sk-test-fake");
    let mut events = Vec::new();
    let reply = ai::chat(&ai, &[ai::Message { role: "user".into(), content: "verlan?".into(), carry: None }], Mode::Ask, |e| events.push(e)).await.unwrap();
    assert_eq!(reply.text, "Backslang.");
    assert_eq!(reply.searches, ["verlan"]);
    assert_eq!(reply.sources[0].url, "https://a.example/v");
    assert!(reply.carry.unwrap().contains("server_tool_calls"));
    assert_eq!(events[0], Event::Looking("Searching: verlan".into()));
    let req = &server.await.unwrap()[0];
    assert!(req.starts_with("POST /chat/completions "));
    assert!(req.to_ascii_lowercase().contains("authorization: bearer sk-test-fake"));
    let sent: serde_json::Value = serde_json::from_str(&req[req.find("\r\n\r\n").unwrap() + 4..]).unwrap();
    assert_eq!(sent["model"], "deepseek-flash");
    assert_eq!(sent["thinking"]["type"], "enabled");
    assert_eq!(sent["reasoning_effort"], "low");
    assert_eq!(sent["tools"][0]["type"], "deepseek_web_search");
}

#[tokio::test]
async fn a_hub_gets_no_key_and_quick_work_no_thinking() {
    let (url, server) = stand_in(vec![(200, sse(&[r#"{"choices":[{"delta":{"content":"Yes."}}]}"#]))]).await;
    let ai = Ai::new(&format!("{url}/ai/deepseek"), "", "");
    let reply = ai::chat(&ai, &[], Mode::Quick, |_| {}).await.unwrap();
    assert_eq!(reply.text, "Yes.");
    assert_eq!(reply.carry, None);
    let req = &server.await.unwrap()[0];
    assert!(req.starts_with("POST /ai/deepseek/chat/completions "));
    assert!(!req.to_ascii_lowercase().contains("authorization"));
    assert!(req.contains(r#""thinking":{"type":"disabled"}"#));
}

#[tokio::test]
async fn a_refused_tool_request_is_asked_again_without_tools() {
    let (url, server) = stand_in(vec![
        (400, r#"{"error":{"message":"tools: not supported"}}"#.into()),
        (200, sse(&[r#"{"choices":[{"delta":{"content":"ok"}}]}"#])),
    ])
    .await;
    let reply = ai::chat(&Ai::new(&url, "", "sk-test-fake"), &[], Mode::Ask, |_| {}).await.unwrap();
    assert_eq!(reply.text, "ok");
    let reqs = server.await.unwrap();
    assert!(reqs[0].contains("deepseek_web_search") && !reqs[1].contains("deepseek_web_search"));
}

#[tokio::test]
async fn a_used_up_balance_says_so() {
    let (url, _server) = stand_in(vec![(402, r#"{"error":{"message":"Insufficient Balance"}}"#.into())]).await;
    let e = ai::chat(&Ai::new(&url, "", "sk-test-fake"), &[], Mode::Quick, |_| {}).await.unwrap_err();
    assert!(e.to_string().contains("balance is used up"), "{e}");
}
