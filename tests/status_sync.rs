//! Sync rounds against a stand-in for ccez-words (newest `at` wins; a
//! loser with another status sends the winner out again). From
//! ccez-tube's word_sync tests, on statuses kept here.
#![cfg(feature = "net")]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use ccez_lang_core::{Status, Statuses};

#[derive(Default)]
struct Server {
    /// key, word, status, at, seq
    rows: Vec<(String, String, String, i64, i64)>,
    seq: i64,
    /// Uploads answered 500.
    refuse: bool,
}

impl Server {
    fn put(&mut self, lang: &str, word: &str, status: &str, at: i64) {
        let key = format!("{lang}:{}", word.to_lowercase());
        self.seq += 1;
        let seq = self.seq;
        match self.rows.iter_mut().find(|r| r.0 == key) {
            Some(r) if r.3 >= at => {
                if r.2 != status {
                    r.4 = seq;
                }
            }
            Some(r) => *r = (key, word.into(), status.into(), at, seq),
            None => self.rows.push((key, word.into(), status.into(), at, seq)),
        }
    }

    fn status(&self, key: &str) -> Option<String> {
        self.rows.iter().find(|r| r.0 == key).map(|r| r.2.clone())
    }
}

fn serve(server: Arc<Mutex<Server>>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let mut len = 0;
            loop {
                let mut h = String::new();
                reader.read_line(&mut h).unwrap();
                if h.trim().is_empty() {
                    break;
                }
                if let Some(v) = h.to_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap();
                }
            }
            let mut body = vec![0; len];
            reader.read_exact(&mut body).unwrap();
            let mut s = server.lock().unwrap();
            let mut out = stream;
            if line.starts_with("POST") && s.refuse {
                let _ = write!(out, "HTTP/1.1 500 Internal Server Error\r\ncontent-length: 0\r\nconnection: close\r\n\r\n");
                continue;
            }
            let reply = if line.starts_with("POST") {
                let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
                for c in v.as_array().unwrap() {
                    s.put(c["lang"].as_str().unwrap(), c["word"].as_str().unwrap(), c["status"].as_str().unwrap(), c["at"].as_i64().unwrap());
                }
                String::new()
            } else {
                let since: i64 = line.split("since=").nth(1).and_then(|r| r.split_whitespace().next()).and_then(|n| n.parse().ok()).unwrap_or(0);
                let mut rows: Vec<_> = s.rows.iter().filter(|r| r.4 > since).collect();
                rows.sort_by_key(|r| r.4);
                let words: Vec<_> = rows.iter().map(|r| serde_json::json!({ "lang": r.0.split(':').next(), "word": r.1, "status": r.2, "at": r.3 })).collect();
                serde_json::json!({ "words": words, "seq": rows.last().map_or(since, |r| r.4) }).to_string()
            };
            let _ = write!(out, "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{reply}", reply.len());
        }
    });
    format!("http://{addr}")
}

fn store(name: &str) -> Statuses {
    let dir = std::env::temp_dir().join(format!("ccez-lang-sync-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    Statuses::new(dir)
}

const NOWHERE: &str = "http://127.0.0.1:9";

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64
}

#[tokio::test]
async fn an_offline_change_keeps_its_own_time() {
    let server = Arc::new(Mutex::new(Server::default()));
    let url = serve(server.clone());
    let s = store("offline");
    s.set("fr", "maison", Status::Learning);
    s.sync(&url, "t").await.unwrap();
    assert_eq!(server.lock().unwrap().status("fr:maison").as_deref(), Some("learning"));
    // Cleared here while offline: noted now, though it can't go up.
    s.set("fr", "maison", Status::New);
    assert!(s.sync(NOWHERE, "t").await.is_err());
    // Later, another app saves it again; later still, back online.
    std::thread::sleep(std::time::Duration::from_millis(20));
    server.lock().unwrap().put("fr", "maison", "learning", now());
    std::thread::sleep(std::time::Duration::from_millis(20));
    let came = s.sync(&url, "t").await.unwrap();
    assert_eq!(server.lock().unwrap().status("fr:maison").as_deref(), Some("learning"));
    assert!(came.iter().any(|w| w.word == "maison" && w.status == Status::Learning), "{came:?}");
    assert_eq!(s.status("fr", "maison"), Status::Learning);
}

#[tokio::test]
async fn a_later_change_here_beats_an_older_one_there() {
    let server = Arc::new(Mutex::new(Server::default()));
    let url = serve(server.clone());
    let s = store("later");
    s.set("fr", "chat", Status::Learning);
    s.sync(&url, "t").await.unwrap();
    server.lock().unwrap().put("fr", "chat", "new", 1_000);
    s.set("fr", "chat", Status::Known);
    let came = s.sync(&url, "t").await.unwrap();
    assert!(came.is_empty(), "{came:?}");
    assert_eq!(server.lock().unwrap().status("fr:chat").as_deref(), Some("known"));
}

#[tokio::test]
async fn a_fresh_install_never_wipes_what_others_changed() {
    let server = Arc::new(Mutex::new(Server::default()));
    let url = serve(server.clone());
    server.lock().unwrap().put("fr", "pain", "known", 5_000);
    let s = store("fresh");
    s.set("fr", "pain", Status::Learning);
    let came = s.sync(&url, "t").await.unwrap();
    assert_eq!(server.lock().unwrap().status("fr:pain").as_deref(), Some("known"));
    assert!(came.iter().any(|w| w.word == "pain" && w.status == Status::Known), "{came:?}");
    assert_eq!(s.status("fr", "pain"), Status::Known);
    // Its own words, unknown there, go up.
    let s2 = store("fresh2");
    s2.set("ja", "猫", Status::Learning);
    s2.sync(&url, "t").await.unwrap();
    assert_eq!(server.lock().unwrap().status("ja:猫").as_deref(), Some("learning"));
}

#[tokio::test]
async fn its_own_changes_coming_back_change_nothing() {
    let server = Arc::new(Mutex::new(Server::default()));
    let url = serve(server.clone());
    let s = store("echo");
    s.set("zh", "喜欢", Status::Learning);
    s.sync(&url, "t").await.unwrap();
    let came = s.sync(&url, "t").await.unwrap();
    assert!(came.is_empty(), "{came:?}");
    let seq = server.lock().unwrap().seq;
    s.sync(&url, "t").await.unwrap();
    assert_eq!(server.lock().unwrap().seq, seq, "nothing sent again");
}

#[tokio::test]
async fn what_came_down_is_kept_when_the_upload_fails() {
    let server = Arc::new(Mutex::new(Server::default()));
    let url = serve(server.clone());
    let s = store("refused");
    s.set("fr", "lune", Status::Learning);
    s.sync(&url, "t").await.unwrap();
    server.lock().unwrap().put("fr", "soleil", "learning", now());
    server.lock().unwrap().refuse = true;
    s.set("fr", "étoile", Status::Learning);
    let came = s.sync(&url, "t").await.unwrap();
    assert!(came.iter().any(|w| w.word == "soleil" && w.status == Status::Learning), "{came:?}");
    assert_eq!(s.status("fr", "soleil"), Status::Learning);
    server.lock().unwrap().refuse = false;
    s.sync(&url, "t").await.unwrap();
    assert_eq!(server.lock().unwrap().status("fr:soleil").as_deref(), Some("learning"));
    assert_eq!(server.lock().unwrap().status("fr:étoile").as_deref(), Some("learning"));
}
