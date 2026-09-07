use crate::core;

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
fn http_client_supports_https_query_and_notification_json() {
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
    let payload = serde_json::json!({"topic": "test-only", "message": "vacancy"});
    let notification = client
        .post("https://example.com/")
        .json(&payload)
        .build()
        .unwrap();
    assert_eq!(notification.headers()["content-type"], "application/json");
    let body: serde_json::Value =
        serde_json::from_slice(notification.body().unwrap().as_bytes().unwrap()).unwrap();
    assert_eq!(body, payload);
}
