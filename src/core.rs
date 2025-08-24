use anyhow::{anyhow, Result};
use encoding_rs::Encoding;
use futures::{stream::FuturesUnordered, StreamExt};
use mailparse::{parse_mail, MailHeaderMap, ParsedMail};
use regex::Regex;
use reqwest::Client;
use scraper::{Html, Selector};
use serde::Deserialize;
use serde_json::{from_value, json, Value};
use tabled::Tabled;

#[derive(Debug, Deserialize, Tabled)]
pub struct Course {
    #[serde(alias = "CourseNo")]
    #[tabled(rename = "課程代碼")]
    pub course_id: String,
    #[serde(alias = "AllStudent")]
    #[tabled(rename = "選課人數")]
    pub student_count: i32,
    #[serde(alias = "Restrict2")]
    #[tabled(rename = "人數上限")]
    pub student_limit: String,
    #[serde(alias = "CourseTeacher")]
    #[tabled(rename = "授課老師")]
    pub course_teacher: String,
    #[serde(alias = "CourseName")]
    #[tabled(rename = "課程名稱")]
    pub course_name: String,
    #[serde(default)]
    #[tabled(rename = "選上機率(%)")]
    pub success_rate: f32,
    #[serde(default)]
    #[tabled(rename = "選課比例")]
    pub choice_rate: f32,
}

#[derive(Debug, Clone)]
pub struct StudentIdentity {
    pub program_type: String, // 四技、二專等
    pub department: String,   // 系所
    pub grade: String,        // 年級
}

#[derive(Debug, Deserialize)]
pub struct CourseDetailResponse {
    #[serde(alias = "Display")]
    pub display: String,
    #[serde(alias = "Result")]
    pub result: Vec<CourseDetail>,
}

#[derive(Debug, Deserialize)]
pub struct CourseDetail {
    #[serde(alias = "EducationCode")]
    pub education_code: String,
    #[serde(alias = "DepartmentAliase")]
    pub department_aliase: String,
    #[serde(alias = "Restrict")]
    pub restrict: String,
    #[serde(alias = "Persons")]
    pub persons: i32,
}

pub fn round_digits(num: f32, digits: i32) -> f32 {
    let base = 10.0_f32.powi(digits);
    return (num * base).round() / base;
}

/// Build a department search string based on student identity,
/// For example, StudentIdentity { program_type: "四技", department: "資訊工程系", grade: "二年級", class: "甲班" }
/// Will generate "四技資訊工程系二"
pub fn build_department_search_string(identity: &StudentIdentity) -> String {
    let grade_number = identity
        .grade
        .chars()
        .find(|c| matches!(c, '一' | '二' | '三' | '四'))
        .expect("年級資訊不完整");

    format!(
        "{}{}{}",
        identity.program_type, identity.department, grade_number
    )
}

pub async fn get_course_info(client: &Client, semester: &str, course_id: String) -> Result<Course> {
    let url = "https://querycourse.ntust.edu.tw/querycourse/api/courses";
    let body = json!({
        "Semester": semester,
        "CourseNo": course_id,
        "Language": "zh"
    });
    let res = client.post(url).json(&body).send().await?;
    let json_array = res.json::<Value>().await?;
    if json_array.as_array().unwrap().is_empty() {
        return Err(anyhow!(course_id.to_string()));
    }
    let json_object = &json_array[0];
    let mut data = from_value::<Course>(json_object.clone())?;
    //    .wrap_or_exit("不可能，絕對不可能，怎麼可能沒有課程資料");

    let raw_choice_rate = (data.student_count as f32) / (data.student_limit).parse::<f32>()?;
    //      .wrap_or_exit("人數上限轉換失敗");

    data.choice_rate = round_digits(raw_choice_rate, 2);
    data.success_rate = 100.0;
    if data.choice_rate > 0.0 {
        data.success_rate = 100.0 / data.choice_rate;
        if data.success_rate > 100.0 {
            data.success_rate = 100.0;
        }
        data.success_rate = round_digits(data.success_rate, 2);
    }
    Ok(data)
}

/// Use a new probability calculation method for physical education courses
/// Find corresponding enrollment limits based on the user's department information
pub async fn get_pe_course_info_with_identity(
    client: &Client,
    semester: &str,
    course_id: String,
    student_identity: &StudentIdentity,
) -> Result<Course> {
    let mut course = get_course_info(client, semester, course_id.clone()).await?;

    if course_id.contains("PE") {
        match get_course_limit_detail(client, semester, &course_id).await {
            Ok(limit_response) => {
                let search_string = build_department_search_string(student_identity);

                if let Some(dept_limit) = limit_response
                    .result
                    .iter()
                    .find(|d| d.department_aliase == search_string)
                {
                    let restrict_num = dept_limit.restrict.parse().unwrap_or(1.0);
                    let persons = dept_limit.persons as f32;

                    if restrict_num <= 0.0 {
                        return Ok(course);
                    }

                    course.choice_rate = round_digits(persons / restrict_num, 2);
                    course.success_rate = if course.choice_rate > 0.0 {
                        round_digits(100.0_f32.min(100.0 / course.choice_rate), 2)
                    } else {
                        100.0
                    };
                }
            }
            Err(_) => return Ok(course),
        }
    }

    Ok(course)
}

pub async fn get_semester(client: &Client) -> Result<String> {
    let url = "https://querycourse.ntust.edu.tw/querycourse/api/semestersinfo";
    let data = client.get(url).send().await?.json::<Value>().await?;
    let body = data[0]["Semester"].as_str().unwrap_or_default().to_string();
    Ok(body)
}
/// Get enrollment limit information for courses
/// Used for probability and ratio calculation of physical education courses
pub async fn get_course_limit_detail(
    client: &Client,
    semester: &str,
    course_no: &str,
) -> Result<CourseDetailResponse> {
    let url = "https://querycourse.ntust.edu.tw/querycourse/api/LimitOnTheNumber";
    let params = [
        ("semester", semester),
        ("courseNo", course_no),
        ("mylanguage", "zh"),
    ];

    let res = client.get(url).query(&params).send().await?;
    let response = res.json::<CourseDetailResponse>().await?;
    Ok(response)
}

pub async fn fetch_all_courses(
    course_ids: Vec<String>,
    client: &Client,
    semester: &str,
    callback: impl FnMut(),
) -> (Vec<Course>, Vec<Course>, Vec<String>) {
    fetch_all_courses_with_identity(course_ids, client, semester, None, callback).await
}

pub async fn fetch_all_courses_with_identity(
    course_ids: Vec<String>,
    client: &Client,
    semester: &str,
    student_identity: Option<&StudentIdentity>,
    callback: impl FnMut(),
) -> (Vec<Course>, Vec<Course>, Vec<String>) {
    let mut unsafe_courses: Vec<Course> = Vec::new();
    let mut safe_courses = Vec::new();
    let mut unknown_courses = Vec::new();

    let mut futures = FuturesUnordered::new();
    for course in course_ids.into_iter() {
        let client = client.clone();
        let semester = semester.to_string();
        let identity = student_identity.cloned();

        futures.push(async move {
            if course.contains("PE") {
                if let Some(id) = identity.as_ref() {
                    return get_pe_course_info_with_identity(&client, &semester, course, id).await;
                }
            }

            get_course_info(&client, &semester, course).await
        });
    }
    let mut what = callback;

    while let Some(result) = futures.next().await {
        what();
        if result.is_err() {
            unknown_courses.push(result.unwrap_err().to_string());
            continue;
        }
        match result {
            Ok(course_info) => {
                if course_info.success_rate == 100.0 {
                    safe_courses.push(course_info);
                } else {
                    unsafe_courses.push(course_info);
                }
            }
            Err(err) => {
                unknown_courses.push(err.to_string());
            }
        }
    }

    (safe_courses, unsafe_courses, unknown_courses)
}

/// Detect if the content is MHTML format by checking for multipart boundaries
pub fn is_mhtml_format(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    let first_tag = lower.find('<').unwrap_or(usize::MAX);
    let sniff = &lower[..first_tag.min(4096).min(lower.len())];

    let has_mime = sniff.contains("mime-version:");
    let has_multipart = sniff.contains("content-type: multipart/");
    let has_boundary = sniff.contains("boundary=");

    (has_mime || has_multipart) && has_boundary
}

/// Extract and decode HTML content from MHTML format
/// Walk through MIME tree to collect candidate HTML parts with metadata
fn decode_part_to_utf8(
    part: &ParsedMail<'_>,
) -> Result<(String, usize, Option<String>, Option<String>)> {
    let raw = part.get_body_raw()?;
    let raw_len = raw.len();

    let charset = part
        .ctype
        .params
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("charset"))
        .map(|(_, v)| v.as_str())
        .unwrap_or("utf-8");

    let enc = Encoding::for_label(charset.as_bytes()).unwrap_or(encoding_rs::UTF_8);
    let (cow, _, _) = enc.decode(&raw);
    let html = cow.into_owned();

    let content_location = part.get_headers().get_first_value("Content-Location");
    let content_id = part
        .get_headers()
        .get_first_value("Content-ID")
        .map(|mut s| {
            s.retain(|ch| ch != '<' && ch != '>');
            s
        });

    Ok((html, raw_len, content_location, content_id))
}

fn collect_html_parts<'a>(
    part: &'a ParsedMail<'a>,
    acc: &mut Vec<(String, usize, Option<String>, Option<String>)>,
) -> Result<()> {
    if part.ctype.mimetype.eq_ignore_ascii_case("text/html") {
        acc.push(decode_part_to_utf8(part)?);
    }
    for sp in &part.subparts {
        collect_html_parts(sp, acc)?;
    }
    Ok(())
}

fn pick_best_html(
    mut candidates: Vec<(String, usize, Option<String>, Option<String>)>,
    root_cid: Option<&str>,
) -> Option<String> {
    if candidates.is_empty() {
        return None;
    }

    if let Some(cid) = root_cid {
        if let Some((html, _, _, _)) = candidates
            .iter()
            .find(|(_, _, _, c)| {
                if let Some(id) = c {
                    id.eq_ignore_ascii_case(cid)
                } else {
                    false
                }
            })
            .cloned()
        {
            return Some(html);
        }
    }

    candidates.sort_by(|a, b| {
        let score = |loc: &Option<String>, len: usize, html: &str| -> (i32, i64) {
            let mut s = 0;
            if let Some(l) = loc {
                let ll = l.to_ascii_lowercase();
                if ll.ends_with(".html") || ll.ends_with(".htm") {
                    s += 2;
                }
                let p = ll.split(&['?', '#'][..]).next().unwrap_or(&ll);
                let depth = p.matches('/').count();
                if depth <= 3 {
                    s += 1;
                }
            }
            let h = html.to_ascii_lowercase();
            if h.contains("<html") {
                s += 1;
            }
            if h.contains("<body") {
                s += 1;
            }
            (s, len as i64)
        };
        let sa = score(&a.2, a.1, &a.0);
        let sb = score(&b.2, b.1, &b.0);
        sa.cmp(&sb)
    });

    candidates.pop().map(|t| t.0)
}

pub fn extract_html_from_mhtml(content: &str) -> anyhow::Result<String> {
    let mail = parse_mail(content.as_bytes())?;

    let root_cid = mail
        .ctype
        .params
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("start"))
        .map(|(_, v)| v.trim_matches(['<', '>'].as_ref()).to_string());

    let mut candidates = Vec::new();
    collect_html_parts(&mail, &mut candidates)?;
    pick_best_html(candidates, root_cid.as_deref())
        .ok_or_else(|| anyhow::anyhow!("在 MHTML 中找不到 text/html part"))
}

/// If it looks like MHTML, parse and return decoded UTF-8 HTML.
/// Otherwise, return the original content.
pub fn preprocess_file_content(file_content: &str) -> String {
    if is_mhtml_format(file_content) {
        if let Ok(html) = extract_html_from_mhtml(file_content) {
            return html;
        }
    }

    file_content.to_string()
}

pub fn extract_course_ids(file_content: &str) -> Vec<String> {
    let processed = preprocess_file_content(file_content);

    let re = Regex::new(r"[A-Z]{2}[G|1-9]{1}[AB|0-9]{3}[0|1|3|5|7]{1}[0-9]{2}")
        .expect("Regex 模板創建失敗");

    let document = Html::parse_document(&processed);
    let selector = Selector::parse("#cartTable").expect("無法解析選擇器");

    if let Some(node) = document.select(&selector).next() {
        let sub_html = node.html();
        re.find_iter(&sub_html)
            .map(|m| m.as_str().to_string())
            .collect()
    } else {
        re.find_iter(&processed)
            .map(|m| m.as_str().to_string())
            .collect()
    }
}

/// Extract student identity information from HTML content
/// Looks for student identity in the format: "四技 資訊工程系 二年級 甲班"
pub fn extract_student_identity(html_content: &str) -> Result<StudentIdentity> {
    let processed_content = preprocess_file_content(html_content);
    let document = Html::parse_document(&processed_content);
    let selector = Selector::parse("span").expect("無法解析選擇器");

    // Find the span element containing grade information
    for element in document.select(&selector) {
        let text_content = element.inner_html();
        let text = text_content.trim();

        // Skip elements that don't contain grade info
        if !text.contains("年級") {
            continue;
        }

        // Found grade info - attempt to parse it using string splitting
        // Format: [學制] [系所] [年級] [班級], separated by spaces, fixed order
        let parts: Vec<&str> = text.split_whitespace().collect();

        // Check if we have enough parts (at least 4)
        if parts.len() < 3 {
            continue;
        }

        // Find the grade position
        let grade_pos = match parts.iter().position(|&part| part.contains("年級")) {
            Some(pos) => pos,
            None => continue,
        };

        // Validate grade position constraints
        if grade_pos < 2 || grade_pos >= parts.len() {
            continue;
        }

        // Extract components - all validations passed
        let program_type = parts[grade_pos - 2].to_string();
        let department = parts[grade_pos - 1].to_string();
        let grade = parts[grade_pos].to_string();

        return Ok(StudentIdentity {
            program_type,
            department,
            grade,
        });
    }

    // No grade information found in any span element
    Err(anyhow!(
        "未找到包含年級資訊的文字內容。請確認 HTML 內容是否正確，或將此錯誤訊息回報給開發者。"
    ))
}
