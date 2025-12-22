use clap::{CommandFactory, FromArgMatches, Parser};
use kdam::{BarExt, Spinner, tqdm};
use reqwest::Client;
use std::io;
use std::io::prelude::*;
use std::process::exit;
use std::time::Duration;
use tokio::time::sleep;
use rand::Rng;
use tabled::{
    Table,
    settings::{
        Alignment, Concat, Modify, Panel, Remove, Span, Style,
        object::{Cell, Columns, Segment},
    },
};
pub mod core;
pub mod model;
use core::{extract_course_ids, extract_student_identity, fetch_all_courses, get_semester, get_course_info};
use model::Course;

#[derive(Parser, Debug)]
#[command(author, about = "台灣科技大學\n選課志願序小幫手", long_about)]
struct Args {
    #[arg(required = false)]
    file_path: Option<String>,
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
fn get_path() -> Option<String> {
    let matches = Args::command().try_get_matches();
    if let Err(err) = &matches {
        let _ = err.print();
        return None;
    }
    let args = Args::from_arg_matches(&matches.unwrap());
    if let Err(err) = &args {
        let _ = err.print();
        return None;
    }

    args.unwrap().file_path
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
            &[
                "▁▂▃",
                "▂▃▄",
                "▃▄▅",
                "▄▅▆",
                "▅▆▇",
                "▆▇█",
                "▇█▇",
                "█▇▆",
                "▇▆▅",
                "▆▅▄",
                "▅▄▃",
                "▄▃▂",
                "▃▂▁"
            ],
            30.0,
            1.0,
        )
    )
}

/// 播放系統嗶聲
fn play_beep() {
    // 跨平台的嗶聲提醒
    print!("\x07");
    io::stdout().flush().unwrap();
}

/// 顯示視覺提醒
fn show_visual_alert(course_code: &str) {
    println!("\n{}", "\\".repeat(50));
    println!("🔔 課程有空缺！ 🔔");
    println!("課程代碼: {}", course_code);
    println!("{}", "/".repeat(50));
}

/// 讀取使用者輸入
fn read_input(prompt: &str) -> String {
    print!("{}", prompt);
    io::stdout().flush().unwrap();
    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();
    input.trim().to_string()
}

/// 顯示主選單並取得使用者選擇
fn show_main_menu() -> u32 {
    loop {
        println!("\n========================================");
        println!("      台科大選課助理 - 主選單");
        println!("========================================");
        println!("1. 執行原始選課分析功能");
        println!("2. 執行課程監測功能");
        println!("0. 結束程式");
        println!("========================================");
        
        let input = read_input("請選擇功能 (0-2): ");
        
        match input.parse::<u32>() {
            Ok(n) if n <= 2 => return n,
            _ => println!("❌ 無效的選項，請輸入 0-2 之間的數字"),
        }
    }
}

/// 解析課程代碼輸入（支援逗號分隔，允許有空格）
fn parse_course_codes(input: &str) -> Vec<String> {
    input
        .split(',')
        .map(|s| s.trim().to_uppercase())
        .filter(|s| !s.is_empty())
        .collect()
}

/// 驗證課程代碼格式
fn validate_course_code(code: &str) -> bool {
    // 課程代碼格式: [A-Z]{2}[G|1-9]{1}[AB|0-9]{3}[0|1|3|5|7]{1}[0-9]{2}
    if code.len() != 9 {
        return false;
    }
    
    let chars: Vec<char> = code.chars().collect();
    
    // 前兩位是大寫字母
    if !chars[0].is_ascii_uppercase() || !chars[1].is_ascii_uppercase() {
        return false;
    }
    
    // 第三位是 G 或 1-9
    if chars[2] != 'G' && !('1'..='9').contains(&chars[2]) {
        return false;
    }
    
    // 第四到六位是 A、B 或 0-9
    for i in 3..6 {
        if chars[i] != 'A' && chars[i] != 'B' && !chars[i].is_ascii_digit() {
            return false;
        }
    }
    
    // 第七位是 0、1、3、5、7
    if !['0', '1', '3', '5', '7'].contains(&chars[6]) {
        return false;
    }
    
    // 最後兩位是數字
    if !chars[7].is_ascii_digit() || !chars[8].is_ascii_digit() {
        return false;
    }
    
    true
}

/// 帶重試機制的課程資訊查詢
async fn get_course_info_with_retry(
    client: &Client,
    semester: &str,
    course_id: &str,
    max_retries: u32,
) -> Result<Course, String> {
    let mut retries = 0;
    
    loop {
        match get_course_info(client, semester, course_id.to_string(), None).await {
            Ok(course) => return Ok(course),
            Err(e) => {
                retries += 1;
                if retries >= max_retries {
                    return Err(format!("查詢失敗（已重試 {} 次）: {}", max_retries, e));
                }
                
                // 指數退避重試
                let wait_time = Duration::from_secs(2u64.pow(retries.min(5)));
                eprintln!("⚠️  查詢 {} 失敗，{}秒後重試 ({}/{})...", 
                         course_id, wait_time.as_secs(), retries, max_retries);
                sleep(wait_time).await;
            }
        }
    }
}

/// 課程監測功能
async fn monitor_courses() {
    let client = Client::new();
    
    // 取得學期資訊
    let semester = match get_semester(&client).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("❌ 無法取得學期資訊: {}", e);
            wait_exit_with_code(1);
            return;
        }
    };
    
    println!("\n當前學期: {}", semester);
    
    // 讀取上次的課程清單（如果存在）
    let last_courses_file = "last_courses.txt";
    if std::path::Path::new(last_courses_file).exists() {
        if let Ok(content) = std::fs::read_to_string(last_courses_file) {
            let last_courses = content.trim();
            if !last_courses.is_empty() {
                println!("\n上次查詢的課程清單: {}", last_courses);
                let use_last = read_input("是否沿用上次的課程清單？(Y/n): ");
                if use_last.is_empty() || use_last.to_lowercase() == "y" || use_last.to_lowercase() == "yes" {
                    let course_codes = parse_course_codes(last_courses);
                    start_monitoring(&client, &semester, course_codes).await;
                    return;
                }
            }
        }
    }
    
    // 輸入課程代碼
    loop {
        let input = read_input("\n請輸入要監測的課程代碼（多個代碼以逗號分隔）: ");
        
        if input.is_empty() {
            println!("❌ 請至少輸入一個課程代碼");
            continue;
        }
        
        let course_codes = parse_course_codes(&input);
        
        // 驗證所有課程代碼
        let mut all_valid = true;
        let mut invalid_codes = Vec::new();
        
        for code in &course_codes {
            if !validate_course_code(code) {
                all_valid = false;
                invalid_codes.push(code.clone());
            }
        }
        
        if !all_valid {
            println!("❌ 以下課程代碼格式無效: {:?}", invalid_codes);
            println!("💡 課程代碼應為 9 位字元，例如: CS1001301");
            let retry = read_input("是否重新輸入？(Y/n): ");
            if retry.is_empty() || retry.to_lowercase() == "y" || retry.to_lowercase() == "yes" {
                continue;
            } else {
                wait_exit_with_code(0);
                return;
            }
        }
        
        // 儲存課程清單
        let _ = std::fs::write(last_courses_file, input.clone());
        
        start_monitoring(&client, &semester, course_codes).await;
        break;
    }
}

/// 開始監測課程
async fn start_monitoring(client: &Client, semester: &str, course_codes: Vec<String>) {
    // 詢問查詢間隔
    let interval = loop {
        let input = read_input("\n請輸入查詢間隔（秒數，預設 5 秒，按 Enter 使用預設值）: ");
        
        if input.is_empty() {
            break 5;
        }
        
        match input.parse::<u64>() {
            Ok(n) if n > 0 && n <= 3600 => break n,
            _ => println!("❌ 請輸入 1-3600 之間的數字"),
        }
    };
    
    println!("\n========================================");
    println!("開始監測以下課程:");
    for code in &course_codes {
        println!("  - {}", code);
    }
    println!("查詢間隔: {} 秒（含隨機延遲 ±2 秒）", interval);
    println!("按 Ctrl+C 結束監測");
    println!("========================================\n");
    
    let mut iteration = 0;
    
    loop {
        iteration += 1;
        println!("\n[第 {} 次查詢] {}", iteration, chrono::Local::now().format("%Y-%m-%d %H:%M:%S"));
        println!("{}", "-".repeat(80));
        
        for course_code in &course_codes {
            match get_course_info_with_retry(client, semester, course_code, 3).await {
                Ok(course) => {
                    let current_students: i32 = course.student_count;
                    let limit: i32 = course.student_limit.parse().unwrap_or(0);
                    let has_vacancy = current_students < limit;
                    let vacancy_symbol = if has_vacancy { "✅ 有空缺" } else { "❌ 已滿" };
                    
                    println!(
                        "{} | {} | 選課人數: {}/{} | {}",
                        course_code,
                        course.course_name,
                        current_students,
                        limit,
                        vacancy_symbol
                    );
                    
                    // 如果有空缺，發出提醒
                    if has_vacancy {
                        play_beep();
                        show_visual_alert(course_code);
                        play_beep();
                        play_beep();
                    }
                }
                Err(e) => {
                    println!("{} | ❌ 代碼無效或查詢失敗: {}", course_code, e);
                }
            }
        }
        
        // 隨機延遲（模擬真人行為）
        let mut rng = rand::thread_rng();
        let jitter: f64 = rng.gen_range(-2.0..3.0);
        let wait_time = interval as f64 + jitter;
        let wait_duration = Duration::from_secs_f64(wait_time.max(1.0));
        
        println!("\n等待 {:.1} 秒後進行下次查詢...", wait_time);
        sleep(wait_duration).await;
    }
}

/// 原始選課分析功能
async fn run_original_analysis(file_path: String) {
    let file_content = std::fs::read_to_string(&file_path)
        .unwrap_or_else(|_| {
            eprintln!("❌ 檔案開啟失敗: {}", file_path);
            wait_exit_with_code(1);
            String::new()
        });

    let course_ids: Vec<_> = extract_course_ids(&file_content);

    let client = Client::new();

    let semester = get_semester(&client).await.unwrap_or_else(|_| {
        eprintln!("❌ 無法取得學期資訊");
        wait_exit_with_code(1);
        String::new()
    });

    let need_identity_extraction = course_ids.iter().any(|id| id.starts_with("PE"));
    // Try to extract student identity from the HTML file for enhanced PE course calculations
    let student_identity = if need_identity_extraction {
        match extract_student_identity(&file_content) {
            Ok(identity) => {
                println!(
                    "✓ 成功提取學生身份資訊: {} {} {}",
                    identity.program_type, identity.department, identity.grade
                );
                Some(identity)
            }
            Err(err) => {
                println!("⚠ 無法提取學生身份資訊，將使用一般計算方式計算體育課人數");
                eprintln!("錯誤詳細: {}", err);

                None
            }
        }
    } else {
        None
    };

    let mut pb = get_process_bar(course_ids.len());
    let callback = || {
        let _ = pb.update(1);
    };

    // Use enhanced course fetching with student identity if available
    let (mut safe_courses, mut unsafe_courses, unknown_courses) =
        fetch_all_courses(course_ids, &client, &semester, student_identity, callback).await;

    for course in unknown_courses {
        eprint!("\n警告: 查無課程資料，課程代碼: {}", course);
    }
    println!();

    unsafe_courses.sort_by(|a, b| b.choice_rate.partial_cmp(&a.choice_rate).unwrap());
    safe_courses.sort_by(|a, b| b.choice_rate.partial_cmp(&a.choice_rate).unwrap());

    let mut safe_part_table = Table::new(&safe_courses);
    let mut unsafe_part_table = Table::new(&unsafe_courses);

    if unsafe_courses.iter().all(|c| c.class_room_no.is_empty())
        && safe_courses.iter().all(|c| c.class_room_no.is_empty())
    {
        safe_part_table.with(Remove::column(Columns::one(6)));
        unsafe_part_table.with(Remove::column(Columns::one(6)));
    }

    if !safe_courses.is_empty() {
        let len = unsafe_courses.len() + 1;
        unsafe_part_table
            .with(Concat::vertical(safe_part_table))
            .with(Modify::new(Cell::new(len, 0)).with("以下課程皆會選上，無須考慮位置"))
            .modify((len, 0), Span::column(99));
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

#[tokio::main]
async fn main() {
    // 顯示主選單
    let choice = show_main_menu();
    
    match choice {
        0 => {
            println!("👋 再見！");
            exit(0);
        }
        1 => {
            // 原始選課分析功能
            let file_path = match get_path() {
                Some(path) => path,
                None => {
                    let path = read_input("\n請輸入 HTML 檔案路徑: ");
                    if path.is_empty() {
                        eprintln!("❌ 未提供檔案路徑");
                        wait_exit_with_code(1);
                        return;
                    }
                    path
                }
            };
            
            run_original_analysis(file_path).await;
        }
        2 => {
            // 課程監測功能
            monitor_courses().await;
        }
        _ => {
            println!("❌ 無效的選項");
            wait_exit_with_code(1);
        }
    }
}
