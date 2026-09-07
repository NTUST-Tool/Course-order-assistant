use clap::{CommandFactory, FromArgMatches, Parser};
use kdam::{BarExt, Spinner, tqdm};
use rand::Rng;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io;
use std::io::prelude::*;
use std::path::PathBuf;
use std::process::exit;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;
use std::time::Instant;
use tabled::{
    Table,
    settings::{
        Alignment, Concat, Modify, Panel, Remove, Span, Style,
        object::{Cell, Columns, Segment},
    },
};
use tokio::time::sleep;
pub mod core;
#[cfg(test)]
mod dependency_tests;
pub mod model;
use core::{
    extract_course_ids, extract_student_identity, fetch_all_courses, get_course_info, get_semester,
};
use model::Course;

#[derive(Parser, Debug)]
#[command(author, about = "台灣科技大學\n選課志願序小幫手", long_about)]
struct Args {
    #[arg(required = false)]
    file_path: Option<String>,
}

/// 課程空缺狀態追蹤
#[derive(Clone)]
struct CourseVacancyState {
    had_vacancy: bool,                       // 上次是否有空缺
    notification_count: u32,                 // 已發送通知次數
    last_notification_time: Option<Instant>, // 上次發送通知的時間
}

/// 設定檔案結構
#[derive(Serialize, Deserialize, Default)]
struct AppConfig {
    student_id: Option<String>,
    last_courses: Option<String>,
}

/// 取得設定檔路徑（位於可執行檔所在目錄）
fn get_config_path() -> PathBuf {
    let exe_path = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("."));
    let exe_dir = exe_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    exe_dir.join("course_assistant_config.json")
}

/// 讀取設定檔
fn load_config() -> AppConfig {
    let config_path = get_config_path();
    if config_path.exists()
        && let Ok(content) = std::fs::read_to_string(&config_path)
        && let Ok(config) = serde_json::from_str::<AppConfig>(&content)
    {
        return config;
    }
    AppConfig::default()
}

/// 儲存設定檔
fn save_config(config: &AppConfig) {
    let config_path = get_config_path();
    if let Ok(json) = serde_json::to_string_pretty(config) {
        let _ = std::fs::write(&config_path, json);
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
    println!("{}", "\\".repeat(50));
    println!("{}", "\\".repeat(50));
    println!("🔔 課程有空缺！ 🔔");
    println!("課程代碼: {}", course_code);
    println!("{}", "/".repeat(50));
    println!("{}", "/".repeat(50));
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

/// 根據學號生成 ntfy topic (學號_SHA256後6位)
fn generate_ntfy_topic(student_id: &str) -> String {
    // 獲取機器唯一識別碼作為 salt（不需要 UAC 或 root 權限）
    // 使用 HWIDComponent 添加硬體資訊
    use machineid_rs::{Encryption, HWIDComponent, IdBuilder};

    let machine_id = IdBuilder::new(Encryption::SHA256)
        .add_component(HWIDComponent::SystemID)
        .build("ntfy-topic")
        .unwrap_or_else(|_| "default-machine-id".to_string());

    // 使用學號 + 機器 ID 作為 salt 進行 SHA-256 加密
    let combined = format!("{}{}", student_id, machine_id);
    let mut hasher = Sha256::new();
    hasher.update(combined.as_bytes());
    let result = hasher.finalize();
    let hash = format!("{:x}", result);
    let last_6 = &hash[hash.len() - 6..];

    format!("{}_{}", student_id, last_6)
}

/// 獲取學號輸入並保存
fn get_student_id_input() -> String {
    loop {
        let input = read_input("\n請輸入您的學號（用於接收課程空缺通知）: ");
        if input.is_empty() {
            println!("❌ 學號不能為空");
            continue;
        }
        if input.chars().all(|c| c.is_alphanumeric()) {
            let topic = generate_ntfy_topic(&input);
            println!("✓ 學號已設定: {}", input);
            println!("📱 請在手機 ntfy app 中訂閱以下 topic:");
            println!("   {}", topic);
            println!(
                "   (這個 topic 是由您的學號 + 本機硬體資訊加密生成，確保只有您在這台電腦上能產生相同 topic)"
            );

            // 保存學號到設定檔
            let mut config = load_config();
            config.student_id = Some(input.clone());
            save_config(&config);

            return input;
        } else {
            println!("❌ 學號只能包含字母和數字");
        }
    }
}

/// 發送 ntfy.sh 通知
async fn send_ntfy_notification(
    client: &Client,
    student_id: &str,
    title: &str,
    message: &str,
    priority: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    let url = format!("https://ntfy.sh/{}", student_id);

    let response = client
        .post(&url)
        .header("Title", title)
        .header("Priority", priority.to_string())
        .header("Tags", "mortar_board,bell")
        .body(message.to_string())
        .send()
        .await?;

    if response.status().is_success() {
        Ok(())
    } else {
        Err(format!("通知發送失敗: {}", response.status()).into())
    }
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
    for ch in chars.iter().take(6).skip(3) {
        if *ch != 'A' && *ch != 'B' && !ch.is_ascii_digit() {
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
                eprintln!(
                    "⚠️  查詢 {} 失敗，{}秒後重試 ({}/{})...",
                    course_id,
                    wait_time.as_secs(),
                    retries,
                    max_retries
                );
                sleep(wait_time).await;
            }
        }
    }
}

/// 帶倒數計時的等待函數，檢查共享的退出信號
async fn countdown_with_quit_option(duration: Duration, quit_signal: &Arc<AtomicBool>) -> bool {
    let total_millis = duration.as_millis() as u64;
    let steps = (total_millis / 100).max(1); // 每0.1秒更新一次

    // 倒數循環
    for i in 0..steps {
        if quit_signal.load(Ordering::Relaxed) {
            println!(); // 換行
            return false; // 用戶按了Q
        }

        let remaining = (steps - i) as f64 * 0.1;
        print!(
            "\r倒數計時: {:.1} 秒 | 輸入 Q 然後按 Enter 結束監測",
            remaining
        );
        io::stdout().flush().unwrap();

        sleep(Duration::from_millis(100)).await;
    }

    println!(); // 換行
    true // 正常完成倒數
}

/// 課程監測功能
async fn monitor_courses() -> bool {
    let client = Client::new();

    // 取得學期資訊
    let semester = match get_semester(&client).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("❌ 無法取得學期資訊: {}", e);
            return false;
        }
    };

    println!("\n當前學期: {}", semester);

    // 讀取設定檔
    let config = load_config();

    // 讀取上次的學號（如果存在）
    let student_id = if let Some(last_id) = config.student_id.as_ref() {
        if !last_id.is_empty() {
            let topic = generate_ntfy_topic(last_id);
            println!("\n上次使用的學號: {}", last_id);
            println!("對應的 ntfy topic: {}", topic);
            let use_last = read_input("是否沿用上次的學號？(Y/n): ");
            if use_last.is_empty()
                || use_last.to_lowercase() == "y"
                || use_last.to_lowercase() == "yes"
            {
                last_id.to_string()
            } else {
                get_student_id_input()
            }
        } else {
            get_student_id_input()
        }
    } else {
        get_student_id_input()
    };

    // 生成 ntfy topic
    let ntfy_topic = generate_ntfy_topic(&student_id);

    // 讀取上次的課程清單（如果存在）
    if let Some(last_courses) = config.last_courses.as_ref()
        && !last_courses.is_empty()
    {
        println!("\n上次查詢的課程清單: {}", last_courses);
        let use_last = read_input("是否沿用上次的課程清單？(Y/n): ");
        if use_last.is_empty() || use_last.to_lowercase() == "y" || use_last.to_lowercase() == "yes"
        {
            let course_codes = parse_course_codes(last_courses);
            return start_monitoring(&client, &semester, course_codes, &ntfy_topic).await;
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
                return false; // 返回主選單
            }
        }

        // 儲存課程清單到設定檔
        let mut config = load_config();
        config.last_courses = Some(input.clone());
        save_config(&config);

        return start_monitoring(&client, &semester, course_codes, &ntfy_topic).await;
    }
}

/// 解析時間間隔輸入（支持h=小時, m=分鐘）
fn parse_time_interval(input: &str) -> Option<u64> {
    let input = input.trim().to_lowercase();

    // 解析格式如: 3h, 10m, 1h30m, 2h30m50
    let mut total_seconds: u64 = 0;
    let mut current_number = String::new();

    for ch in input.chars() {
        if ch.is_ascii_digit() {
            current_number.push(ch);
        } else if ch == 'h' {
            if let Ok(hours) = current_number.parse::<u64>() {
                total_seconds += hours * 3600;
                current_number.clear();
            } else {
                return None;
            }
        } else if ch == 'm' {
            if let Ok(minutes) = current_number.parse::<u64>() {
                total_seconds += minutes * 60;
                current_number.clear();
            } else {
                return None;
            }
        } else if !ch.is_whitespace() {
            return None; // 無效字符
        }
    }

    // 處理剩餘的數字（視為秒數）
    if !current_number.is_empty() {
        if let Ok(seconds) = current_number.parse::<u64>() {
            total_seconds += seconds;
        } else {
            return None;
        }
    }

    if total_seconds > 0 && total_seconds <= 86400 {
        Some(total_seconds)
    } else {
        None
    }
}

/// 開始監測課程
async fn start_monitoring(
    client: &Client,
    semester: &str,
    course_codes: Vec<String>,
    ntfy_topic: &str,
) -> bool {
    // 詢問查詢間隔
    let interval = loop {
        let input = read_input(
            "\n請輸入查詢間隔（預設 5 秒，按 Enter 使用預設值）\n支援格式: 數字(秒), 3h(小時), 10m(分鐘), 1h30m(組合): ",
        );

        if input.is_empty() {
            break 5;
        }

        match parse_time_interval(&input) {
            Some(n) => {
                println!("✓ 已設定查詢間隔: {} 秒", n);
                break n;
            }
            None => println!("❌ 無效的時間格式，請重新輸入"),
        }
    };

    println!("\n========================================");
    println!("開始監測以下課程:");
    for code in &course_codes {
        println!("  - {}", code);
    }
    println!("查詢間隔: {} 秒（含隨機延遲 ±2 秒）", interval);
    println!("通知設定: 發現空缺後每 5 分鐘通知一次，最多 12 次");
    println!("通知 Topic: {}", ntfy_topic);
    println!("提示: 等待期間輸入 Q 然後按 Enter 可結束監測");
    println!("========================================\n");

    // 初始化課程狀態追蹤
    let mut course_states: HashMap<String, CourseVacancyState> = HashMap::new();
    for code in &course_codes {
        course_states.insert(
            code.clone(),
            CourseVacancyState {
                had_vacancy: false,
                notification_count: 0,
                last_notification_time: None,
            },
        );
    }

    // 創建共享的退出信號
    let quit_signal = Arc::new(AtomicBool::new(false));
    let quit_signal_clone = quit_signal.clone();

    // 啟動單一的鍵盤監聽執行緒，在整個監測期間持續運行
    let input_thread = thread::spawn(move || {
        let stdin = io::stdin();
        loop {
            let mut buffer = String::new();
            if stdin.read_line(&mut buffer).is_ok() {
                let input = buffer.trim().to_lowercase();
                if input == "q" {
                    quit_signal_clone.store(true, Ordering::Relaxed);
                    break; // 退出輸入監聽迴圈
                }
            }
        }
    });

    let mut iteration = 0;

    loop {
        // 檢查是否在查詢前就收到退出信號
        if quit_signal.load(Ordering::Relaxed) {
            break;
        }

        iteration += 1;
        println!(
            "\n[第 {} 次查詢] {}",
            iteration,
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
        );
        println!("{}", "-".repeat(80));

        for course_code in &course_codes {
            match get_course_info_with_retry(client, semester, course_code, 3).await {
                Ok(course) => {
                    let current_students: i32 = course.student_count;
                    let limit: i32 = course.student_limit.parse().unwrap_or(0);
                    let has_vacancy = current_students < limit;
                    let vacancy_symbol = if has_vacancy {
                        "✅ 有空缺"
                    } else {
                        "❌ 已滿"
                    };

                    println!(
                        "{} | {} | 選課人數: {}/{} | {}",
                        course_code, course.course_name, current_students, limit, vacancy_symbol
                    );

                    // 獲取課程狀態
                    let state = course_states.get_mut(course_code).unwrap();
                    let now = Instant::now();

                    // 檢查是否需要發送通知
                    if has_vacancy {
                        // 如果有空缺
                        play_beep();
                        show_visual_alert(course_code);
                        play_beep();
                        play_beep();

                        // 檢查是否需要發送 ntfy 通知
                        let should_notify = if !state.had_vacancy {
                            // 剛發現空缺，立即通知
                            true
                        } else if state.notification_count < 12 {
                            // 已經有空缺，檢查是否過了 5 分鐘
                            if let Some(last_time) = state.last_notification_time {
                                now.duration_since(last_time) >= Duration::from_secs(300) // 5 分鐘
                            } else {
                                true
                            }
                        } else {
                            false
                        };

                        if should_notify {
                            state.notification_count += 1;
                            state.last_notification_time = Some(now);

                            let title = format!("🎓 {} 有空缺！", course_code);
                            let message = format!(
                                "課程名稱: {}\n上課時間: {}\n選課人數: {}/{}\n\n通知次數: {}/12",
                                course.course_name,
                                course.node,
                                current_students,
                                limit,
                                state.notification_count
                            );

                            match send_ntfy_notification(client, ntfy_topic, &title, &message, 4)
                                .await
                            {
                                Ok(_) => {
                                    println!("  📱 已發送通知 ({}/12)", state.notification_count)
                                }
                                Err(e) => eprintln!("  ⚠️  通知發送失敗: {}", e),
                            }
                        }

                        state.had_vacancy = true;
                    } else {
                        // 如果沒有空缺
                        if state.had_vacancy {
                            // 之前有空缺，現在沒有了，發送通知
                            let title = format!("❌ {} 已滿額", course_code);
                            let message = format!(
                                "課程名稱: {}\n上課時間: {}\n選課人數: {}/{}\n\n該課程已被選滿，請繼續關注其他時段",
                                course.course_name, course.node, current_students, limit
                            );

                            match send_ntfy_notification(client, ntfy_topic, &title, &message, 3)
                                .await
                            {
                                Ok(_) => println!("  📱 已發送『課程已滿』通知"),
                                Err(e) => eprintln!("  ⚠️  通知發送失敗: {}", e),
                            }

                            // 重置狀態
                            state.had_vacancy = false;
                            state.notification_count = 0;
                            state.last_notification_time = None;
                        }
                    }
                }
                Err(e) => {
                    println!("{} | ❌ 代碼無效或查詢失敗: {}", course_code, e);
                }
            }
        }

        // 隨機延遲（模擬真人行為）
        let mut rng = rand::rng();
        let jitter: f64 = rng.random_range(-2.0..3.0);
        let wait_time = interval as f64 + jitter;
        let wait_duration = Duration::from_secs_f64(wait_time.max(1.0));

        println!("\n等待 {:.1} 秒後進行下次查詢...", wait_time);

        // 帶倒數計時的等待，檢查退出信號
        if !countdown_with_quit_option(wait_duration, &quit_signal).await {
            break;
        }
    }

    // 等待輸入執行緒結束（最多等待1秒）
    // 由於執行緒可能還在 read_line 阻塞中，我們不強制等待它完成
    drop(input_thread);

    println!("\n✓ 已停止監測，返回主選單...");
    false // 返回false表示用戶主動退出或結束
}

/// 原始選課分析功能
async fn run_original_analysis(file_path: String) -> bool {
    let file_content = std::fs::read_to_string(&file_path).unwrap_or_else(|_| {
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

    // 等待用戶按Enter後返回主選單
    let _ = read_input("\n按 Enter 鍵返回主選單...");
    true // 返回主選單
}

#[tokio::main]
async fn main() {
    // 主循環：不斷顯示選單直到用戶選擇退出
    loop {
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
                            continue; // 返回主選單
                        }
                        path
                    }
                };

                run_original_analysis(file_path).await;
                // 執行完畢後會自動返回主選單
            }
            2 => {
                // 課程監測功能
                monitor_courses().await;
                // 執行完畢後會自動返回主選單
            }
            _ => {
                println!("❌ 無效的選項");
                // 繼續循環，不退出
            }
        }
    }
}
