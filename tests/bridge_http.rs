use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use timetoll::bridge;

#[test]
fn authenticated_loopback_bridge_rejects_web_origins_and_delivers_actions() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let token = "a".repeat(32);
    let running = Arc::new(AtomicBool::new(true));
    let state = bridge::start(
        port,
        token.clone(),
        vec!["chrome.exe".into()],
        running.clone(),
    )
    .unwrap();
    fn send(port: u16, auth: &str, origin: &str, host: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let body = r#"{"app":"chrome.exe","url":"https://example.com/learn"}"#;
        write!(stream, "POST /v1/activity HTTP/1.1\r\nHost: {host}\r\nAuthorization: Bearer {auth}\r\nOrigin: {origin}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response
    }
    let host = format!("127.0.0.1:{port}");
    let denied = send(port, "wrong", "chrome-extension://test", &host);
    assert!(denied.starts_with("HTTP/1.1 403"));
    assert!(state.lock().unwrap().url("chrome.exe").is_none());
    assert!(send(port, &token, "https://example.com", &host).starts_with("HTTP/1.1 403"));
    assert!(
        send(port, &token, "chrome-extension://test", "evil.example").starts_with("HTTP/1.1 403")
    );
    state.lock().unwrap().request_new_tab("chrome.exe".into());
    let accepted = send(port, &token, "chrome-extension://test", &host);
    assert!(accepted.starts_with("HTTP/1.1 200"));
    assert!(accepted.contains("\"new_tab\":true"));
    assert_eq!(
        state.lock().unwrap().url("chrome.exe"),
        Some("https://example.com/learn")
    );
    let next = send(port, &token, "chrome-extension://test", &host);
    assert!(next.contains("\"new_tab\":false"));
    running.store(false, Ordering::Relaxed);
}
