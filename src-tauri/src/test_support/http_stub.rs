//! 本机回环上的桩 HTTP 服务器：按顺序回答，并记下收到的请求。联网相关的测试共用。

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use push_protocol::{ApiError, ApiErrorCode};

#[derive(Debug, Clone)]
pub struct Captured {
    pub request_line: String,
    pub headers: String,
    pub body: String,
}

pub struct Stub {
    pub base_url: String,
    pub captured: Arc<Mutex<Vec<Captured>>>,
}

pub struct Reply {
    pub status: u16,
    pub body: String,
    pub location: Option<&'static str>,
}

pub fn json_reply(status: u16, body: impl serde::Serialize) -> Reply {
    Reply {
        status,
        body: serde_json::to_string(&body).unwrap(),
        location: None,
    }
}

pub fn api_error(status: u16, code: ApiErrorCode) -> Reply {
    json_reply(
        status,
        ApiError {
            code,
            message: "服务端原话".to_string(),
        },
    )
}

/// 按顺序回答：第 n 个连接拿第 n 个回复，之后线程退出。
pub fn serve(replies: Vec<Reply>) -> Stub {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let captured = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&captured);
    std::thread::spawn(move || {
        for reply in replies {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut raw = Vec::new();
            let mut chunk = [0u8; 4096];
            let (head, mut body) = loop {
                let read = stream.read(&mut chunk).unwrap_or(0);
                raw.extend_from_slice(&chunk[..read]);
                if let Some(at) = raw.windows(4).position(|window| window == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&raw[..at]).into_owned();
                    break (head, raw[at + 4..].to_vec());
                }
                if read == 0 {
                    return;
                }
            };
            let length = head
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .and_then(|value| value.trim().parse::<usize>().ok())
                })
                .unwrap_or(0);
            while body.len() < length {
                let read = stream.read(&mut chunk).unwrap_or(0);
                if read == 0 {
                    break;
                }
                body.extend_from_slice(&chunk[..read]);
            }
            let (request_line, headers) = head.split_once("\r\n").unwrap_or((&head, ""));
            sink.lock().unwrap().push(Captured {
                request_line: request_line.to_string(),
                headers: headers.to_string(),
                body: String::from_utf8_lossy(&body).into_owned(),
            });
            let location = reply
                .location
                .map(|target| format!("Location: {target}\r\n"))
                .unwrap_or_default();
            let response = format!(
                "HTTP/1.1 {} X\r\nContent-Type: application/json\r\n{location}Content-Length: {}\r\nConnection: close\r\n\r\n{}",
                reply.status,
                reply.body.len(),
                reply.body
            );
            let _ = stream.write_all(response.as_bytes());
        }
    });
    Stub { base_url, captured }
}
