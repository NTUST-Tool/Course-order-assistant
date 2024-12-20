use clap::{CommandFactory, FromArgMatches, Parser};
use futures::stream::{FuturesUnordered, StreamExt};
use kdam::{tqdm, BarExt, Spinner};
use regex::Regex;
use reqwest::Client;
use serde::Deserialize;
use serde_json::{from_value, json, Value};
use std::error::Error;
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

trait ResultExt<T, E> {
    fn wrap_or_exit<F>(self, err_msg: F) -> T
    where
        F: Into<String>;
}

impl<T, E> ResultExt<T, E> for Result<T, E>
where
    E: std::fmt::Debug,
{
    fn wrap_or_exit<F>(self, err_msg: F) -> T
    where
        F: Into<String>,
    {
        self.unwrap_or_else(|err| {
            println!("錯誤: {}", err_msg.into());
            println!("詳細資料: {:?}", err);
            wait_exit_with_code(1);
            panic!("for type checking");
        })
    }
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

fn get_process_bar(count: usize) -> impl BarExt {
    tqdm!(
        total = count,
        force_refresh = true,
        bar_format = "{desc suffix=' '}|{animation}| {spinner} {count}/{total} [{percentage:.0}%] in {elapsed human=true} ({rate:.1}/s, eta: {remaining human=true})",
        spinner = Spinner::new(
            &["▁▂▃", "▂▃▄", "▃▄▅", "▄▅▆", "▅▆▇", "▆▇█", "▇█▇", "█▇▆", "▇▆▅", "▆▅▄", "▅▄▃", "▄▃▂", "▃▂▁"],
            30.0,
            1.0,
        )
    )
}

async fn get_course_info(
    client: &Client,
    semester: &str,
    course_id: &str,
) -> Result<Course, Box<dyn Error>> {
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
    let mut data = from_value::<Course>(json_object.clone())
        .wrap_or_exit("不可能，絕對不可能，怎麼可能沒有課程資料");

    let raw_choice_rate = data.student_count as f32
        / (data.student_limit)
            .parse::<f32>()
            .wrap_or_exit("人數上限轉換失敗");

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

async fn fetch_all_courses(
    course_ids: Vec<&str>,
    client: &Client,
    semester: &str,
) -> (Vec<Course>, Vec<Course>, Vec<String>) {
    let mut unsafe_courses: Vec<Course> = Vec::new();
    let mut safe_courses = Vec::new();
    let mut unknown_courses = Vec::new();

    let mut pb = get_process_bar(course_ids.len());

    let mut futures = FuturesUnordered::new();
    for course in course_ids.into_iter() {
        let client = client.clone();
        let semester = semester.to_string();
        futures.push(async move { get_course_info(&client, &semester, course).await });
    }

    while let Some(result) = futures.next().await {
        let _ = pb.update(1); // 當每個任務完成後即時更新
        if result.is_err() {
            unknown_courses.push(result.unwrap_err().to_string());
            continue;
        }
        let course_info = result.wrap_or_exit("不可能，絕對不可能，怎麼可能沒有課程資料");
        if course_info.sucess_rate == 100.0 {
            safe_courses.push(course_info);
        } else {
            unsafe_courses.push(course_info);
        }
    }

    (safe_courses, unsafe_courses, unknown_courses)
}

#[tokio::main]
async fn main() {
    let file_path = get_path();
    let file = std::fs::read_to_string(&file_path).wrap_or_exit("檔案開啟失敗");

    let re = Regex::new(r"[A-Z]{2}[G|1-9]{1}[AB|0-9]{3}[0|1|3|5|7]{1}[0-9]{2}")
        .wrap_or_exit("Regex 模板創建失敗");

    let course_ids: Vec<_> = re.find_iter(&file).map(|m| m.as_str()).collect();

    let client = Client::new();

    let semester = get_semester(&client).await.wrap_or_exit("無法取得學期資訊");

    let (mut safe_courses, mut unsafe_courses, unknown_courses) =
        fetch_all_courses(course_ids, &client, &semester).await;

    for course in unknown_courses {
        eprint!("\n警告: 查無課程資料，課程代碼: {}", course);
    }
    println!();

    unsafe_courses.sort_by(|a, b| b.choice_rate.partial_cmp(&a.choice_rate).unwrap());
    safe_courses.sort_by(|a, b| b.choice_rate.partial_cmp(&a.choice_rate).unwrap());

    let safe_part_table = Table::new(&safe_courses);
    let mut unsafe_part_table = Table::new(&unsafe_courses);

    if safe_courses.len() > 0 {
        let len = unsafe_courses.len() + 1;
        unsafe_part_table
            .with(Concat::vertical(safe_part_table))
            .with(Modify::new(Cell::new(len, 0)).with("以下課程皆會選上，無須考慮位置"))
            .modify((len, 0), Span::column(7));
    }

    unsafe_part_table
        .modify(Segment::all(), Alignment::center())
        .with(Style::ascii_rounded())
        .with(Panel::header(format!(
            "{}學年期 選課志願序分析結果如下",
            semester
        )));

    println!("{}", unsafe_part_table);

    wait_exit_with_code(0);
}
