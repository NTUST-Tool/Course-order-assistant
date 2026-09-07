use clap::Parser;
use kdam::{BarExt, Spinner, tqdm};
use rand::Rng;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io;
use std::io::prelude::*;
use std::path::PathBuf;
use std::process::exit;
use std::sync::{Mutex, OnceLock, mpsc};
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
#[derive(Default)]
struct CourseVacancyState {
    had_vacancy: bool,                       // 上次是否有空缺
    notification_count: u32,                 // 已發送通知次數
    last_notification_time: Option<Instant>, // 上次發送通知的時間
}

/// 設定檔案結構
#[derive(Serialize, Deserialize, Default)]
struct AppConfig {
    last_courses: Option<String>,
    ntfy_topic: Option<String>,
}

fn get_config_path() -> anyhow::Result<PathBuf> {
    Ok(std::env::current_exe()?
        .parent()
        .ok_or_else(|| anyhow::anyhow!("找不到設定檔目錄"))?
        .join("course_assistant_config.json"))
}

fn load_config_from(path: &std::path::Path) -> anyhow::Result<AppConfig> {
    match std::fs::read_to_string(path) {
        Ok(content) => Ok(serde_json::from_str(&content)?),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(AppConfig::default()),
        Err(e) => Err(e.into()),
    }
}

fn save_config_to(path: &std::path::Path, config: &AppConfig) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("找不到設定檔目錄"))?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(serde_json::to_string_pretty(config)?.as_bytes())?;
    file.as_file().sync_all()?;
    file.persist(path)?;
    Ok(())
}

fn http_client() -> reqwest::Result<Client> {
    Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(20))
        .build()
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

// One reader owns stdin for the lifetime of the process, including between menus.
fn input_receiver() -> &'static Mutex<mpsc::Receiver<io::Result<String>>> {
    static INPUT: OnceLock<Mutex<mpsc::Receiver<io::Result<String>>>> = OnceLock::new();
    INPUT.get_or_init(|| {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            for line in io::stdin().lock().lines() {
                let failed = line.is_err();
                if tx.send(line).is_err() || failed {
                    break;
                }
            }
        });
        Mutex::new(rx)
    })
}

fn read_input(prompt: &str) -> String {
    print!("{}", prompt);
    io::stdout().flush().unwrap();
    match input_receiver().lock().unwrap().recv() {
        Ok(Ok(line)) => line.trim().to_string(),
        Ok(Err(e)) => {
            eprintln!("讀取輸入失敗: {e}");
            exit(1);
        }
        Err(_) => exit(0),
    }
}

async fn wait_for_quit(receiver: &Mutex<mpsc::Receiver<io::Result<String>>>) {
    loop {
        match receiver.lock().unwrap().try_recv() {
            Ok(Ok(line)) if line.trim().eq_ignore_ascii_case("q") => return,
            Ok(Err(e)) => {
                eprintln!("讀取輸入失敗: {e}");
                return;
            }
            Err(mpsc::TryRecvError::Disconnected) => return,
            _ => {}
        }
        sleep(Duration::from_millis(100)).await;
    }
}

fn ensure_ntfy_topic(config: &mut AppConfig) -> &str {
    config.ntfy_topic.get_or_insert_with(|| {
        let bytes: [u8; 32] = rand::rng().random();
        format!(
            "course-{}",
            bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
        )
    })
}

async fn send_vacancy_notification(
    client: &Client,
    base_url: &str,
    topic: &str,
    title: &str,
    message: &str,
    state: &mut CourseVacancyState,
) -> Result<(), Box<dyn std::error::Error>> {
    send_notification_to(client, base_url, topic, title, message, 4).await?;
    state.notification_count += 1;
    state.last_notification_time = Some(Instant::now());
    Ok(())
}

impl CourseVacancyState {
    fn should_notify(&self, now: Instant) -> bool {
        self.notification_count < 12
            && self
                .last_notification_time
                .is_none_or(|last| now.duration_since(last) >= Duration::from_secs(300))
    }
}

async fn until_quit(
    receiver: &Mutex<mpsc::Receiver<io::Result<String>>>,
    work: impl std::future::Future<Output = ()>,
) {
    tokio::select! {
        biased;
        _ = wait_for_quit(receiver) => {}
        _ = work => {}
    }
}

async fn send_notification_to(
    client: &Client,
    base_url: &str,
    topic: &str,
    title: &str,
    message: &str,
    priority: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    let url = format!("{}/{}", base_url.trim_end_matches('/'), topic);

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
    max_attempts: u32,
) -> Result<Course, String> {
    let mut retries = 0;

    loop {
        match get_course_info(client, semester, course_id.to_string(), None).await {
            Ok(course) => return Ok(course),
            Err(e) => {
                retries += 1;
                if retries >= max_attempts {
                    return Err(format!("查詢失敗（共嘗試 {} 次）: {}", max_attempts, e));
                }

                // 指數退避重試
                let wait_time = Duration::from_secs(2u64.pow(retries.min(5)));
                eprintln!(
                    "⚠️  查詢 {} 失敗，{}秒後重試 ({}/{})...",
                    course_id,
                    wait_time.as_secs(),
                    retries,
                    max_attempts
                );
                sleep(wait_time).await;
            }
        }
    }
}

/// Configure monitoring before polling; configuration failures stop the operation.
async fn monitor_courses() -> anyhow::Result<()> {
    let client = http_client()?;
    let semester = get_semester(&client).await?;
    println!("\n當前學期: {semester}");
    let path = get_config_path()?;
    let mut config = load_config_from(&path)?;
    let migrated = config.ntfy_topic.is_none();
    let topic = ensure_ntfy_topic(&mut config).to_string();
    save_config_to(&path, &config)?;
    if migrated {
        println!("通知頻道已升級，請在 ntfy app 重新訂閱下方頻道。");
    }
    println!("ntfy topic: {topic}\n請勿公開此頻道；它不是加密或身分驗證。");

    loop {
        let input = if let Some(previous) = config.last_courses.as_ref() {
            println!("上次查詢的課程清單: {previous}");
            if confirms_default(&read_input("是否沿用上次的課程清單？(Y/n): ")) {
                previous.clone()
            } else {
                read_input("請輸入課程代碼（逗號分隔）: ")
            }
        } else {
            read_input("請輸入課程代碼（逗號分隔）: ")
        };
        let codes = parse_course_codes(&input);
        let invalid: Vec<_> = codes
            .iter()
            .filter(|code| !validate_course_code(code))
            .collect();
        if codes.is_empty() || !invalid.is_empty() {
            println!("請輸入有效課程代碼，例如 CS1001301。無效代碼: {invalid:?}");
            config.last_courses = None;
            if confirms_default(&read_input("是否重新輸入？(Y/n): ")) {
                continue;
            }
            return Ok(());
        }
        config.last_courses = Some(codes.join(","));
        save_config_to(&path, &config)?;
        start_monitoring(&client, &semester, codes, &topic).await;
        return Ok(());
    }
}

fn confirms_default(input: &str) -> bool {
    input.is_empty() || input.eq_ignore_ascii_case("y") || input.eq_ignore_ascii_case("yes")
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
                total_seconds = total_seconds.checked_add(hours.checked_mul(3600)?)?;
                current_number.clear();
            } else {
                return None;
            }
        } else if ch == 'm' {
            if let Ok(minutes) = current_number.parse::<u64>() {
                total_seconds = total_seconds.checked_add(minutes.checked_mul(60)?)?;
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
            total_seconds = total_seconds.checked_add(seconds)?;
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
) {
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
    println!("提示: 隨時輸入 Q 然後按 Enter 可結束監測");
    println!("========================================\n");

    // 初始化課程狀態追蹤
    let mut course_states: HashMap<String, CourseVacancyState> = HashMap::new();
    for code in &course_codes {
        course_states.insert(code.clone(), CourseVacancyState::default());
    }

    until_quit(
        input_receiver(),
        poll_courses(
            client,
            semester,
            &course_codes,
            ntfy_topic,
            interval,
            &mut course_states,
        ),
    )
    .await;
    println!("\n✓ 已停止監測，返回主選單...");
}

async fn poll_courses(
    client: &Client,
    semester: &str,
    course_codes: &[String],
    ntfy_topic: &str,
    interval: u64,
    course_states: &mut HashMap<String, CourseVacancyState>,
) {
    let mut iteration = 0;

    loop {
        iteration += 1;
        println!(
            "\n[第 {} 次查詢] {}",
            iteration,
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
        );
        println!("{}", "-".repeat(80));

        for course_code in course_codes {
            match get_course_info_with_retry(client, semester, course_code, 3).await {
                Ok(course) => {
                    let current_students: i32 = course.student_count;
                    let Ok(limit) = course.student_limit.parse::<i32>() else {
                        eprintln!("{course_code} | 人數上限格式錯誤，略過本次查詢");
                        continue;
                    };
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
                        if state.should_notify(now) {
                            let title = format!("🎓 {} 有空缺！", course_code);
                            let message = format!(
                                "課程名稱: {}\n上課時間: {}\n選課人數: {}/{}\n\n通知次數: {}/12",
                                course.course_name,
                                course.node,
                                current_students,
                                limit,
                                state.notification_count + 1
                            );

                            match send_vacancy_notification(
                                client,
                                "https://ntfy.sh",
                                ntfy_topic,
                                &title,
                                &message,
                                state,
                            )
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

                            match send_notification_to(
                                client,
                                "https://ntfy.sh",
                                ntfy_topic,
                                &title,
                                &message,
                                3,
                            )
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
        let jitter: f64 = rng.random_range(-2.0..=2.0);
        let wait_time = (interval as f64 + jitter).max(1.0);
        let wait_duration = Duration::from_secs_f64(wait_time);

        println!("\n等待 {:.1} 秒後進行下次查詢...", wait_time);

        wait_interval(wait_duration).await;
    }
}

async fn wait_interval(duration: Duration) {
    let start = Instant::now();
    while let Some(remaining) = duration.checked_sub(start.elapsed()) {
        print!(
            "\r倒數計時: {:.1} 秒 | 輸入 Q 然後按 Enter 結束監測",
            remaining.as_secs_f64()
        );
        io::stdout().flush().unwrap();
        sleep(remaining.min(Duration::from_millis(100))).await;
    }
    println!();
}

/// 原始選課分析功能
async fn run_original_analysis(file_path: String) -> anyhow::Result<()> {
    let file_content = std::fs::read_to_string(&file_path)?;
    let course_ids = extract_course_ids(&file_content);
    let client = http_client()?;
    let semester = get_semester(&client).await?;

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
    Ok(())
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
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
                let file_path = match args.file_path.clone() {
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

                if let Err(e) = run_original_analysis(file_path).await {
                    eprintln!("分析失敗: {e}");
                }
                // 執行完畢後會自動返回主選單
            }
            2 => {
                // 課程監測功能
                if let Err(e) = monitor_courses().await {
                    eprintln!("監測失敗: {e}");
                }
                // 執行完畢後會自動返回主選單
            }
            _ => {
                println!("❌ 無效的選項");
                // 繼續循環，不退出
            }
        }
    }
}
