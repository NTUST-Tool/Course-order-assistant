use anyhow::{anyhow, Result};
use futures::{stream::FuturesUnordered, StreamExt};
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
    pub sucess_rate: f32,
    #[serde(default)]
    #[tabled(rename = "選課比例")]
    pub choice_rate: f32,
}

pub fn round_digits(num: f32, digits: i32) -> f32 {
    let base = 10.0_f32.powi(digits);
    return (num * base).round() / base;
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
    data.sucess_rate = 100.0;
    if data.choice_rate > 0.0 {
        data.sucess_rate = 100.0 / data.choice_rate;
        if data.sucess_rate > 100.0 {
            data.sucess_rate = 100.0;
        }
        data.sucess_rate = round_digits(data.sucess_rate, 2);
    }
    Ok(data)
}

pub async fn get_semester(client: &Client) -> Result<String> {
    let url = "https://querycourse.ntust.edu.tw/querycourse/api/semestersinfo";
    let data = client.get(url).send().await?.json::<Value>().await?;
    let body = data[0]["Semester"].as_str().unwrap_or_default().to_string();
    Ok(body)
}

pub async fn fetch_all_courses(
    course_ids: Vec<String>,
    client: &Client,
    semester: &str,
    callback: impl FnMut(),
) -> (Vec<Course>, Vec<Course>, Vec<String>) {
    let mut unsafe_courses: Vec<Course> = Vec::new();
    let mut safe_courses = Vec::new();
    let mut unknown_courses = Vec::new();

    let mut futures = FuturesUnordered::new();
    for course in course_ids.into_iter() {
        let client = client.clone();
        let semester = semester.to_string();
        futures.push(async move { get_course_info(&client, &semester, course).await });
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
                if course_info.sucess_rate == 100.0 {
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

pub fn extract_course_ids(file_content: &str) -> Vec<String> {
    let re = Regex::new(r"[A-Z]{2}[G|1-9]{1}[AB|0-9]{3}[0|1|3|5|7]{1}[0-9]{2}")
        .expect("Regex 模板創建失敗");

    let document = Html::parse_document(file_content);
    let selector = Selector::parse("#cartTable").expect("無法解析選擇器");

    if let Some(table_element) = document.select(&selector).next() {
        let table_html = table_element.inner_html();
        re.find_iter(&table_html)
            .map(|m| m.as_str().to_string())
            .collect()
    } else {
        re.find_iter(file_content)
            .map(|m| m.as_str().to_string())
            .collect()
    }
}
