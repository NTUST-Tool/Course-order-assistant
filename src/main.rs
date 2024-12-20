use clap::{CommandFactory, FromArgMatches, Parser};
use regex::Regex;
use reqwest::Client;
use serde::Deserialize;
use serde_json::{from_value, json, Value};
use std::io;
use std::io::prelude::*;
use std::process::exit;
use tabled::{
    settings::{
        object::{Cell, Segment},
        Alignment, Concat, Modify, Panel, Span, Style,
    },
    Table, Tabled,
};

#[derive(Parser, Debug)]
#[command(author, about = "台灣科技大學\n選課志願序小幫手", long_about)]
struct Args {
    file_path: String,
}

#[derive(Debug, Deserialize, Tabled)]
struct Course {
    #[serde(alias = "CourseNo")]
    #[tabled(rename = "課程代碼")]
    course_id: String,
    #[serde(alias = "AllStudent")]
    #[tabled(rename = "選課人數")]
    student_count: i32,
    #[serde(alias = "Restrict2")]
    #[tabled(rename = "人數上限")]
    student_limit: String,
    #[serde(alias = "CourseTeacher")]
    #[tabled(rename = "授課老師")]
    course_teacher: String,
    #[serde(alias = "CourseName")]
    #[tabled(rename = "課程名稱")]
    course_name: String,
    #[serde(default)]
    #[tabled(rename = "選上機率(%)")]
    sucess_rate: f32,
    #[serde(default)]
    #[tabled(rename = "選課比例")]
    choice_rate: f32,
}

fn round_digits(num: f32, digits: i32) -> f32 {
    let base = 10.0_f32.powi(digits);
    return (num * base).round() / base;
}

fn get_path() -> String {
    let matches = Args::command().try_get_matches();
    if matches.is_err() {
        let _ = matches.as_ref().unwrap_err().print();
        wait_exit_with_code(1);
    }
    let args = Args::from_arg_matches(&matches.unwrap());
    if args.is_err() {
        let _ = args.as_ref().unwrap_err().print();
        wait_exit_with_code(1);
    }
    let path = args.unwrap().file_path;
    return path;
}

fn wait_exit_with_code(code: i32) {
    let mut stdin = io::stdin();
    let mut stdout = io::stdout();

    write!(stdout, "\n按下 Enter 鍵結束執行...\n").unwrap();
    stdout.flush().unwrap();

    let _ = stdin.read(&mut [0u8]).unwrap();
    exit(code);
}

async fn get_course_info(
    client: &Client,
    semester: &str,
    course_id: &str,
) -> Result<Course, reqwest::Error> {
    let url = "https://querycourse.ntust.edu.tw/querycourse/api/courses";
    let body = json!({
        "Semester": semester,
        "CourseNo": course_id,
        "Language": "zh"
    });
    let res = client.post(url).json(&body).send().await?;
    let json_array = res.json::<Value>().await?;
    if json_array.as_array().unwrap().is_empty() {
        return Err(course_id)?;
    }
    let json_object = &json_array[0];
    let mut data = from_value::<Course>(json_object.clone()).unwrap();

    let raw_choice_rate = data.student_count as f32 / (data.student_limit).parse::<f32>().unwrap();

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

async fn get_semester(client: &Client) -> Result<String, reqwest::Error> {
    let url = "https://querycourse.ntust.edu.tw/querycourse/api/semestersinfo";
    let data = client.get(url).send().await?.json::<Value>().await?;
    let body = data[0]["Semester"].as_str().unwrap_or_default().to_string();
    Ok(body)
}
#[tokio::main]
async fn main() {
    let file_path = get_path();
    if !std::path::Path::new(&file_path).exists() {
        println!("路徑錯誤或檔案不存在：{}", file_path);
        wait_exit_with_code(1);
    }
    let file = std::fs::read_to_string(&file_path).unwrap();
    let re = Regex::new(r"[A-Z]{2}[G|1-9]{1}[AB|0-9]{3}[0|1|3|5|7]{1}[0-9]{2}").unwrap();
    let course_ids: Vec<_> = re.find_iter(&file).map(|m| m.as_str()).collect();

    let client = Client::new();

    let semester = get_semester(&client).await.unwrap();

    let mut unsafe_courses = Vec::new();
    let mut safe_courses = Vec::new();
    for course in course_ids.iter() {
        let course_info = get_course_info(&client, &semester, course).await.unwrap();
        if course_info.sucess_rate == 100.0 {
            safe_courses.push(course_info);
        } else {
            unsafe_courses.push(course_info);
        }
    }
    unsafe_courses.sort_by(|a, b| b.choice_rate.partial_cmp(&a.choice_rate).unwrap());

    let safe_table = Table::new(&safe_courses);
    let mut table = Table::new(&unsafe_courses);

    if safe_courses.len() > 0 {
        let len = unsafe_courses.len() + 1;
        table
            .with(Concat::vertical(safe_table))
            .with(Modify::new(Cell::new(len, 0)).with("以下課程皆會選上，無須考慮位置"))
            .modify((len, 0), Span::column(7));
    }

    table
        .modify(Segment::all(), Alignment::center())
        .with(Style::ascii_rounded())
        .with(Panel::header(format!(
            "{}學年期 選課志願序分析結果如下",
            semester
        )));

    println!("{}", table);

    wait_exit_with_code(0);
}
