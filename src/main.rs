use clap::{CommandFactory, FromArgMatches, Parser};
use kdam::{tqdm, BarExt, Spinner};
use reqwest::Client;
use std::io;
use std::io::prelude::*;
use std::process::exit;
use tabled::{
    settings::{
        object::{Cell, Segment},
        Alignment, Concat, Modify, Panel, Span, Style,
    },
    Table,
};
pub mod core;
use core::{extract_course_ids, fetch_all_courses, get_semester};

use serde_json::{json, Value};

pub async fn test_api_response() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::new();
    let url = "https://querycourse.ntust.edu.tw/querycourse/api/courses";
    
    // Get current semester first
    let semester_url = "https://querycourse.ntust.edu.tw/querycourse/api/semestersinfo";
    let semester_response = client.get(semester_url).send().await?;
    let semester_data = semester_response.json::<Value>().await?;
    let semester = semester_data[0]["Semester"].as_str().unwrap_or("113-1");
    
    println!("Using semester: {}", semester);
    
    // Use a common course code for testing (trying a few different patterns)
    let test_courses = vec!["CS1001101", "EE1001101", "MA1001101"];
    
    for course_id in test_courses {
        let body = json!({
            "Semester": semester,
            "CourseNo": course_id,
            "Language": "zh"
        });
        
        match client.post(url).json(&body).send().await {
            Ok(response) => {
                match response.json::<Value>().await {
                    Ok(json_response) => {
                        if let Some(array) = json_response.as_array() {
                            if !array.is_empty() {
                                println!("Course: {} - API Response:", course_id);
                                println!("{}", serde_json::to_string_pretty(&json_response)?);
                                return Ok(());
                            } else {
                                println!("Course: {} - No data found", course_id);
                            }
                        }
                    }
                    Err(e) => println!("Failed to parse JSON for {}: {:?}", course_id, e),
                }
            }
            Err(e) => println!("Failed to fetch {}: {:?}", course_id, e),
        }
    }
    
    Ok(())
}

#[derive(Parser, Debug)]
#[command(author, about = "台灣科技大學\n選課志願序小幫手", long_about)]
struct Args {
    file_path: String,
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

#[tokio::main]
async fn main() {
    // Test API response to understand available fields
    if std::env::args().any(|arg| arg == "--test-api") {
        test_api_response().await.unwrap_or_else(|e| println!("API test failed: {:?}", e));
        return;
    }

    let file_path = get_path();
    let file_content = std::fs::read_to_string(&file_path).wrap_or_exit("檔案開啟失敗");

    let course_ids: Vec<_> = extract_course_ids(&file_content);

    let client = Client::new();

    let semester = get_semester(&client).await.wrap_or_exit("無法取得學期資訊");

    let mut pb = get_process_bar(course_ids.len());
    let callback = || {
        let _ = pb.update(1);
    };

    // let callback = || {
    // };

    let (mut safe_courses, mut unsafe_courses, unknown_courses) =
        fetch_all_courses(course_ids, &client, &semester, callback).await;
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
