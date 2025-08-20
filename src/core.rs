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
    #[serde(alias = "CourseName")]
    #[tabled(rename = "課程名稱")]
    pub course_name: String,
    #[serde(alias = "CourseTeacher")]
    #[tabled(rename = "授課老師")]
    pub course_teacher: String,
    #[serde(alias = "Credit", alias = "Credits", alias = "CreditHours", alias = "CourseCredit")]
    #[serde(default)]
    #[tabled(rename = "學分")]
    pub credit: String,
    #[serde(alias = "RequiredElective", alias = "CourseType", alias = "Required", alias = "Compulsory", alias = "Restrict1")]
    #[serde(default)]
    #[tabled(rename = "選必")]
    pub required_elective: String,
    #[serde(alias = "Duration", alias = "SemesterType", alias = "Period", alias = "FullHalf", alias = "SemesterPart")]
    #[serde(default)]
    #[tabled(rename = "全半")]
    pub full_half: String,
    #[serde(alias = "CourseTime", alias = "ClassTime", alias = "Schedule", alias = "TimeSlot", alias = "Time")]
    #[serde(default)]
    #[tabled(rename = "上課時間")]
    pub class_time: String,
    #[serde(alias = "ClassRoom", alias = "Room", alias = "Location", alias = "Classroom", alias = "Place")]
    #[serde(default)]
    #[tabled(rename = "教室")]
    pub classroom: String,
    #[serde(alias = "AllStudent")]
    #[tabled(rename = "選課人數")]
    pub student_count: i32,
    #[serde(alias = "Restrict2")]
    #[tabled(rename = "人數上限")]
    pub student_limit: String,
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

#[cfg(test)]
mod tests {
    use super::*;
    use tabled::Table;

    #[test]
    fn test_course_struct_and_table() {
        // Create a sample course with all new fields
        let course = Course {
            course_id: "CS100101".to_string(),
            course_name: "計算機概論".to_string(),
            course_teacher: "王教授".to_string(),
            credit: "3".to_string(),
            required_elective: "必修".to_string(),
            full_half: "全".to_string(),
            class_time: "一234".to_string(),
            classroom: "TR-101".to_string(),
            student_count: 50,
            student_limit: "60".to_string(),
            sucess_rate: 83.33,
            choice_rate: 1.2,
        };

        // Test that we can create a table with the new structure
        let courses = vec![course];
        let table = Table::new(&courses);
        let table_string = table.to_string();
        
        // Verify that all the new fields appear in the table
        assert!(table_string.contains("課程代碼"));
        assert!(table_string.contains("課程名稱"));
        assert!(table_string.contains("授課老師"));
        assert!(table_string.contains("學分"));
        assert!(table_string.contains("選必"));
        assert!(table_string.contains("全半"));
        assert!(table_string.contains("上課時間"));
        assert!(table_string.contains("教室"));
        assert!(table_string.contains("選課人數"));
        assert!(table_string.contains("人數上限"));
        assert!(table_string.contains("選上機率"));
        assert!(table_string.contains("選課比例"));
        
        // Verify that the data appears in the table
        assert!(table_string.contains("CS100101"));
        assert!(table_string.contains("計算機概論"));
        assert!(table_string.contains("王教授"));
        assert!(table_string.contains("必修"));
        assert!(table_string.contains("一234"));
        assert!(table_string.contains("TR-101"));

        println!("Generated table:\n{}", table_string);
    }

    #[test]
    fn test_course_deserialization_with_missing_fields() {
        use serde_json::{json, from_value};
        
        // Test that the Course struct can be deserialized even when new fields are missing
        let json_data = json!({
            "CourseNo": "CS100101",
            "CourseName": "計算機概論",
            "CourseTeacher": "王教授",
            "AllStudent": 50,
            "Restrict2": "60"
            // Note: new fields are missing, should use defaults
        });

        let course: Result<Course, _> = from_value(json_data);
        assert!(course.is_ok());
        
        let course = course.unwrap();
        assert_eq!(course.course_id, "CS100101");
        assert_eq!(course.course_name, "計算機概論");
        assert_eq!(course.course_teacher, "王教授");
        assert_eq!(course.student_count, 50);
        assert_eq!(course.student_limit, "60");
        
        // New fields should be empty strings (default values)
        assert_eq!(course.credit, "");
        assert_eq!(course.required_elective, "");
        assert_eq!(course.full_half, "");
        assert_eq!(course.class_time, "");
        assert_eq!(course.classroom, "");
    }
}
