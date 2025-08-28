
use serde::{Deserialize, Deserializer};
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
    #[serde(alias = "Node")]
    #[tabled(rename = "上課星期節次")]
    pub node: String,
    #[serde(alias = "ClassRoomNo", deserialize_with = "deserialize_null_default")]
    #[tabled(rename = "上課教室")]
    pub class_room_no: String,
    #[serde(alias = "CreditPoint")]
    #[tabled(rename = "學分")]
    pub course_times: String,
    #[serde(alias = "RequireOption")]
    #[tabled(rename = "必選修")]
    pub require_option: String,
    #[serde(alias = "AllYear")]
    #[tabled(rename = "全半")]
    pub all_year: String,
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

fn deserialize_null_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    T: Default + Deserialize<'de>,
    D: Deserializer<'de>,
{
    let opt = Option::deserialize(deserializer)?;
    Ok(opt.unwrap_or_default())
}