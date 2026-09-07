use crate::core;

#[tokio::test]
#[ignore = "Publishes one synthetic message to a random public ntfy.sh topic; requires explicit approval"]
async fn live_ntfy_subscription_roundtrip() {
    let mut config = crate::AppConfig::default();
    let topic = crate::ensure_ntfy_topic(&mut config).to_string();
    let client = crate::http_client().unwrap();
    let mut subscription = client
        .get(format!("https://ntfy.sh/{topic}/json"))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let mut pending = Vec::new();
    let open = next_ntfy_event(&mut subscription, &mut pending).await;
    assert_eq!(open["event"], "open");

    let title = "🎓 選課助理通知測試";
    let message = format!(
        "Smoke test only — 中文通知正常。 Test ID: {}",
        rand::random::<u64>()
    );
    let mut state = crate::CourseVacancyState::default();
    crate::send_vacancy_notification(
        &client,
        "https://ntfy.sh",
        &topic,
        title,
        &message,
        &mut state,
    )
    .await
    .unwrap();
    let event = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let event = next_ntfy_event(&mut subscription, &mut pending).await;
            if event["event"] == "message" {
                break event;
            }
        }
    })
    .await
    .expect("subscription did not receive notification");
    assert_eq!(event["title"], title);
    assert_eq!(event["message"], message);
    assert_eq!(event["priority"], 4);
    assert_eq!(event["tags"], serde_json::json!(["mortar_board", "bell"]));
    assert_eq!(state.notification_count, 1);
    println!(
        "PASS: ntfy subscription opened before publishing; received matching Chinese title/body, priority and tags; success count = 1. Topic withheld."
    );
}

async fn next_ntfy_event(
    response: &mut reqwest::Response,
    pending: &mut Vec<u8>,
) -> serde_json::Value {
    loop {
        if let Some(end) = pending.iter().position(|&b| b == b'\n') {
            let line: Vec<_> = pending.drain(..=end).collect();
            if line.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            return serde_json::from_slice(&line).expect("invalid subscription event");
        }
        let chunk = response
            .chunk()
            .await
            .unwrap()
            .expect("subscription closed unexpectedly");
        pending.extend_from_slice(&chunk);
    }
}

#[tokio::test]
#[ignore = "Requires access to the live NTUST course API"]
async fn live_semester_https_request() {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .unwrap();
    let semester = core::get_semester(&client).await.unwrap();
    assert!(!semester.is_empty(), "school API returned no semester");
    println!("School API semester: {semester}");
}

#[test]
fn course_parser_prefers_cart_table() {
    let html = "<p>CS2001301</p><table id='cartTable'><tr><td>CS1001301</td></tr></table>";
    assert_eq!(core::extract_course_ids(html), vec!["CS1001301"]);
}

#[test]
fn course_parser_supports_plain_html_and_empty_input() {
    assert_eq!(
        core::extract_course_ids("<p>CS1001301</p>"),
        vec!["CS1001301"]
    );
    assert!(core::extract_course_ids("").is_empty());
}

#[test]
fn http_client_supports_https_query() {
    let client = reqwest::Client::builder().build().unwrap();
    let request = client
        .get("https://example.com/courses")
        .query(&[("semester", "1151"), ("courseNo", "CS1001301")])
        .build()
        .unwrap();
    assert_eq!(
        request.url().query(),
        Some("semester=1151&courseNo=CS1001301")
    );
}

async fn notification_server(status: u16) -> (String, tokio::task::JoinHandle<String>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut data = Vec::new();
        loop {
            let mut chunk = [0u8; 1024];
            let n = socket.read(&mut chunk).await.unwrap();
            assert_ne!(n, 0, "request closed before body arrived");
            data.extend_from_slice(&chunk[..n]);
            if let Some(end) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&data[..end]).to_lowercase();
                let length: usize = headers
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length: "))
                    .unwrap()
                    .parse()
                    .unwrap();
                if data.len() >= end + 4 + length {
                    break;
                }
            }
        }
        socket
            .write_all(
                format!("HTTP/1.1 {status} Test\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                    .as_bytes(),
            )
            .await
            .unwrap();
        String::from_utf8(data).unwrap()
    });
    (url, task)
}

#[tokio::test]
async fn notification_uses_real_headers_and_counts_only_success() {
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let mut state = crate::CourseVacancyState::default();
    let (url, request) = notification_server(503).await;
    assert!(
        crate::send_vacancy_notification(
            &client,
            &url,
            "test-only",
            "🎓 有空缺",
            "課程通知",
            &mut state
        )
        .await
        .is_err()
    );
    request.await.unwrap();
    assert_eq!(state.notification_count, 0);
    assert!(state.last_notification_time.is_none());
    state.had_vacancy = true;
    assert!(state.should_notify(std::time::Instant::now()));

    let (url, request) = notification_server(200).await;
    crate::send_vacancy_notification(
        &client,
        &url,
        "test-only",
        "🎓 有空缺",
        "課程通知",
        &mut state,
    )
    .await
    .unwrap();
    let request = request.await.unwrap();
    assert!(request.starts_with("POST /test-only HTTP/1.1\r\n"));
    assert!(request.contains("title: 🎓 有空缺\r\n"));
    assert!(request.contains("priority: 4\r\n"));
    assert!(request.contains("tags: mortar_board,bell\r\n"));
    assert!(request.ends_with("\r\n\r\n課程通知"));
    assert_eq!(state.notification_count, 1);
    assert!(!state.should_notify(std::time::Instant::now()));
    assert!(state.should_notify(std::time::Instant::now() + std::time::Duration::from_secs(301)));
    state.notification_count = 12;
    assert!(!state.should_notify(std::time::Instant::now() + std::time::Duration::from_secs(301)));
}

#[test]
fn time_intervals_reject_overflow_and_out_of_range() {
    for input in [
        "5124095576030432h",
        "18446744073709551615m",
        "1h18446744073709551615",
        "0",
        "24h1",
        "",
        "abc",
    ] {
        assert_eq!(crate::parse_time_interval(input), None, "{input}");
    }
    assert_eq!(crate::parse_time_interval("1h30m50"), Some(5450));
    assert_eq!(crate::parse_time_interval("24h"), Some(86400));
}

#[test]
fn random_topics_persist_and_config_errors_are_reported() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(format!("config-test-{}", rand::random::<u64>()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.json");
    let mut config = crate::load_config_from(&path).unwrap();
    let topic = crate::ensure_ntfy_topic(&mut config).to_string();
    assert_eq!(topic.len(), 64);
    assert!(topic.bytes().all(|b| b.is_ascii_hexdigit()));
    let mut oversized = crate::AppConfig {
        ntfy_topic: Some(format!("course-{topic}")),
        ..Default::default()
    };
    assert_eq!(crate::ensure_ntfy_topic(&mut oversized), topic);
    assert_eq!(crate::ensure_ntfy_topic(&mut config), topic);
    let mut other = crate::AppConfig::default();
    assert_ne!(crate::ensure_ntfy_topic(&mut other), topic);
    crate::save_config_to(&path, &config).unwrap();
    let mut restored = crate::load_config_from(&path).unwrap();
    assert_eq!(crate::ensure_ntfy_topic(&mut restored), topic);
    let mut legacy: crate::AppConfig =
        serde_json::from_str(r#"{"student_id":"TEST-ONLY","last_courses":"CS1001301"}"#).unwrap();
    assert!(legacy.ntfy_topic.is_none());
    crate::ensure_ntfy_topic(&mut legacy);
    assert_eq!(legacy.last_courses.as_deref(), Some("CS1001301"));
    assert!(
        !serde_json::to_string(&legacy)
            .unwrap()
            .contains("student_id")
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    assert!(crate::save_config_to(&dir, &config).is_err());
    std::fs::write(&path, "invalid json").unwrap();
    assert!(crate::load_config_from(&path).is_err());
    std::fs::remove_file(&path).unwrap();
    std::fs::remove_dir(&dir).unwrap();
}

#[tokio::test]
async fn quit_cancels_in_flight_work_and_preserves_next_menu_input() {
    let (tx, rx) = std::sync::mpsc::channel();
    let receiver = std::sync::Mutex::new(rx);
    let sender = tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        tx.send(Ok("Q".to_string())).unwrap();
        tx.send(Ok("0".to_string())).unwrap();
    });
    tokio::time::timeout(
        std::time::Duration::from_secs(1),
        crate::until_quit(&receiver, std::future::pending()),
    )
    .await
    .unwrap();
    sender.await.unwrap();
    assert_eq!(receiver.lock().unwrap().recv().unwrap().unwrap(), "0");
}

#[tokio::test]
async fn eof_stops_monitoring() {
    let (tx, rx) = std::sync::mpsc::channel();
    drop(tx);
    tokio::time::timeout(
        std::time::Duration::from_secs(1),
        crate::until_quit(&std::sync::Mutex::new(rx), std::future::pending()),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn quit_cancels_a_stalled_http_request() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (tx, rx) = std::sync::mpsc::channel();
    let server = tokio::spawn(async move {
        let (_socket, _) = listener.accept().await.unwrap();
        tx.send(Ok("q".into())).unwrap();
        std::future::pending::<()>().await;
    });
    let receiver = std::sync::Mutex::new(rx);
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        crate::until_quit(&receiver, async {
            let _ = client.get(url).send().await;
            panic!("request should have been cancelled before completing");
        }),
    )
    .await;
    server.abort();
    result.unwrap();
}

#[tokio::test]
async fn production_client_times_out_stalled_requests() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (_socket, _) = listener.accept().await.unwrap();
        std::future::pending::<()>().await;
    });
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(25),
        crate::http_client().unwrap().get(url).send(),
    )
    .await;
    server.abort();
    assert!(
        result
            .expect("production client did not enforce timeout")
            .unwrap_err()
            .is_timeout()
    );
}
