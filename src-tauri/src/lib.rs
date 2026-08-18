use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter, State};
use tokio::process::Command;
use zip::write::SimpleFileOptions;
use regex::Regex;

// Windows: 阻止 svn.exe 弹出命令行窗口
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

fn new_svn_command() -> Command {
    let mut cmd = Command::new("svn");
    // 超时丢弃 future 时同步杀掉子进程，避免残留孤儿 svn.exe
    cmd.kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd.arg("--no-auth-cache");
    cmd
}

/// svn 命令统一超时时长：网络挂起/服务器无响应时避免无限等待
const SVN_TIMEOUT: Duration = Duration::from_secs(60);

/// 带超时执行 svn 命令，防止前端永久卡在加载状态
async fn run_svn_with_timeout(cmd: &mut Command) -> Result<std::process::Output, String> {
    match tokio::time::timeout(SVN_TIMEOUT, cmd.output()).await {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(e)) => Err(format!("执行 svn 命令失败: {}", e)),
        Err(_) => Err(format!("svn 命令执行超时（{} 秒），请检查网络或 SVN 服务器状态", SVN_TIMEOUT.as_secs())),
    }
}

/// 解码 svn 命令输出：优先 UTF-8；非法 UTF-8 时回退 GBK
/// （中文 Windows 下 svn 的报错信息通常为 GBK 编码，直接 lossy 会乱码）
fn decode_svn_bytes(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => encoding_rs::GBK.decode(bytes).0.into_owned(),
    }
}

/// 反转义 XML 预定义实体（&amp; 必须最后替换，避免二次反转义）
fn unescape_xml(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

static TRUST_CERT_SUPPORTED: OnceLock<bool> = OnceLock::new();
static SHOW_ITEM_SUPPORTED: OnceLock<bool> = OnceLock::new();

/// 直接通过 svn help 输出判断参数是否真的支持（而非猜版本号），
/// 因为有些 SVN 版本号正常却是特殊构建，不识别某些参数。
fn check_help_contains(subcommand: &str, needle: &str) -> bool {
    let mut cmd = std::process::Command::new("svn");
    cmd.arg("help").arg(subcommand);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    match cmd.output() {
        Ok(out) => {
            let s = decode_svn_bytes(&out.stdout);
            s.contains(needle)
        }
        Err(_) => false,
    }
}

/// 判断当前 svn 是否支持 --trust-server-cert-failures 参数。
fn trust_cert_supported() -> bool {
    *TRUST_CERT_SUPPORTED.get_or_init(|| check_help_contains("log", "--trust-server-cert-failures"))
}

/// 判断当前 svn 是否支持 --show-item 参数。
fn show_item_supported() -> bool {
    *SHOW_ITEM_SUPPORTED.get_or_init(|| check_help_contains("info", "--show-item"))
}

/// 仅当 svn help 确认支持时才添加 --trust-server-cert-failures 参数。
fn add_trust_cert_arg(cmd: &mut Command) {
    if trust_cert_supported() {
        cmd.arg("--trust-server-cert-failures=unknown-ca,cn-mismatch,expired,not-yet-valid,other");
    }
}

/// 仅当 svn help 确认支持时才添加 --show-item 参数。
/// 低版本时返回 false，调用方需要 fallback。
fn add_show_item_arg(cmd: &mut Command, item: &str) -> bool {
    if show_item_supported() {
        cmd.arg("--show-item").arg(item);
        true
    } else {
        false
    }
}

// ==================== Models ====================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SvnProject {
    pub id: String,
    pub name: String,
    pub svn_url: String,
    pub local_path: String,
    pub username: String,
    pub password: String,
    pub last_rev: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitRecord {
    pub revision: i64,
    pub author: String,
    pub date: String,
    pub message: String,
    pub changed_paths: Vec<String>,
    #[serde(default)]
    pub deleted_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub output_dir: String,
    pub excludes: Vec<String>,
    #[serde(default)]
    pub sensitive_files: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            output_dir: "output".to_string(),
            excludes: vec![
                "license.xml".to_string(),
                "web.xml".to_string(),
                "config.properties".to_string(),
                "db.properties".to_string(),
                "email.properties".to_string(),
                "logback.xml".to_string(),
                "redis.properties".to_string(),
                "wechat-config.properties".to_string(),
                "whitelist.xml".to_string(),
                "sn.txt".to_string(),
            ],
            sensitive_files: vec![
                "db.txt".to_string(),
                "1jw_DDL.sql".to_string(),
                "2view-scrpts.sql".to_string(),
                "5jw_DML.sql".to_string(),
                "6config.txt".to_string(),
            ],
        }
    }
}

// ==================== Config Manager ====================

pub struct ConfigManager {
    pub projects: Mutex<Vec<SvnProject>>,
    pub settings: Mutex<Settings>,
}

impl ConfigManager {
    pub fn new() -> Self {
        Self {
            projects: Mutex::new(Self::load_projects()),
            settings: Mutex::new(Self::load_settings()),
        }
    }

    fn config_dir() -> PathBuf {
        let app_data = std::env::var("APPDATA").unwrap_or_else(|_| {
            std::env::var("USERPROFILE").unwrap_or_else(|_| ".".to_string())
        });
        PathBuf::from(app_data).join("SVNPackager").join("config")
    }

    fn projects_file() -> PathBuf {
        Self::config_dir().join("projects.json")
    }

    fn settings_file() -> PathBuf {
        Self::config_dir().join("settings.json")
    }

    /// 损坏的配置文件隔离保留（追加时间戳后缀）而非静默丢弃，便于排查与恢复
    fn quarantine_corrupt(path: &Path) {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "config".to_string());
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let backup = path.with_file_name(format!("{}.corrupt.{}", name, stamp));
        let _ = fs::rename(path, backup);
    }

    /// 先写 .tmp 文件再 rename 原子替换，避免写入中途崩溃导致配置损坏
    fn write_atomic(path: &Path, content: &str) -> Result<(), String> {
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, content)
            .map_err(|e| format!("写入配置 {} 失败: {}", tmp.display(), e))?;
        fs::rename(&tmp, path)
            .map_err(|e| format!("替换配置 {} 失败: {}", path.display(), e))?;
        Ok(())
    }

    fn ensure_config_dir() -> Result<(), String> {
        let dir = Self::config_dir();
        fs::create_dir_all(&dir)
            .map_err(|e| format!("创建配置目录 {} 失败: {}", dir.display(), e))
    }

    fn load_projects() -> Vec<SvnProject> {
        let path = Self::projects_file();
        if !path.exists() {
            return vec![];
        }
        let content = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => {
                Self::quarantine_corrupt(&path);
                return vec![];
            }
        };
        match serde_json::from_str::<Vec<SvnProject>>(&content) {
            Ok(mut projects) => {
                Self::decrypt_project_passwords(&mut projects);
                projects
            }
            Err(_) => {
                Self::quarantine_corrupt(&path);
                vec![]
            }
        }
    }

    fn save_projects(projects: &[SvnProject]) -> Result<(), String> {
        Self::ensure_config_dir()?;
        let mut to_save = projects.to_vec();
        Self::encrypt_project_passwords(&mut to_save);
        let json = serde_json::to_string_pretty(&to_save)
            .map_err(|e| format!("序列化项目配置失败: {}", e))?;
        Self::write_atomic(&Self::projects_file(), &json)
    }

    fn load_settings() -> Settings {
        let path = Self::settings_file();
        if !path.exists() {
            return Settings::default();
        }
        let content = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => {
                Self::quarantine_corrupt(&path);
                return Settings::default();
            }
        };
        // 判断 sensitive_files 键是否存在：仅缺失（旧版配置）才回填默认值，
        // 用户显式清空时保留空列表
        let has_sensitive = serde_json::from_str::<serde_json::Value>(&content)
            .ok()
            .and_then(|v| v.get("sensitive_files").map(|_| ()))
            .is_some();
        let mut s: Settings = match serde_json::from_str(&content) {
            Ok(s) => s,
            Err(_) => {
                Self::quarantine_corrupt(&path);
                return Settings::default();
            }
        };
        if !has_sensitive {
            s.sensitive_files = Settings::default().sensitive_files;
        }
        s
    }

    fn save_settings(settings: &Settings) -> Result<(), String> {
        Self::ensure_config_dir()?;
        let json = serde_json::to_string_pretty(settings)
            .map_err(|e| format!("序列化设置失败: {}", e))?;
        Self::write_atomic(&Self::settings_file(), &json)
    }

    /// 保存前加密密码（已加密的跳过）；加密失败时保留明文，不影响保存
    fn encrypt_project_passwords(projects: &mut [SvnProject]) {
        for p in projects.iter_mut() {
            if p.password.is_empty() || p.password.starts_with("enc:") {
                continue;
            }
            if let Some(enc) = secret::protect(p.password.as_bytes()) {
                p.password = format!("enc:{}", to_hex(&enc));
            }
        }
    }

    /// 加载后解密密码；解密失败时置空（避免把密文当明文传给 svn）；
    /// 无前缀的旧版明文原样保留，下次保存时自动迁移为加密格式
    fn decrypt_project_passwords(projects: &mut [SvnProject]) {
        for p in projects.iter_mut() {
            if let Some(hex_str) = p.password.strip_prefix("enc:") {
                p.password = match from_hex(hex_str).and_then(|bytes| secret::unprotect(&bytes)) {
                    Some(plain) => String::from_utf8_lossy(&plain).into_owned(),
                    None => String::new(),
                };
            }
        }
    }
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn from_hex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

/// Windows: 用 DPAPI (CryptProtectData) 以当前用户凭据加密密码。
/// 非 Windows: 无 DPAPI，退化为原样返回（仅开发环境涉及）。
#[cfg(windows)]
mod secret {
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    use windows::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    fn crypt(data: &[u8], protect: bool) -> Option<Vec<u8>> {
        unsafe {
            let mut input = data.to_vec();
            let in_blob = CRYPT_INTEGER_BLOB {
                cbData: input.len() as u32,
                pbData: input.as_mut_ptr(),
            };
            let mut out_blob = CRYPT_INTEGER_BLOB::default();
            let result = if protect {
                CryptProtectData(
                    &in_blob,
                    None,
                    None,
                    None,
                    None,
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut out_blob,
                )
            } else {
                CryptUnprotectData(
                    &in_blob,
                    None,
                    None,
                    None,
                    None,
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut out_blob,
                )
            };
            if result.is_err() {
                return None;
            }
            let out = std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize).to_vec();
            let _ = LocalFree(HLOCAL(out_blob.pbData as *mut std::ffi::c_void));
            Some(out)
        }
    }

    pub fn protect(plain: &[u8]) -> Option<Vec<u8>> {
        crypt(plain, true)
    }

    pub fn unprotect(cipher: &[u8]) -> Option<Vec<u8>> {
        crypt(cipher, false)
    }
}

#[cfg(not(windows))]
mod secret {
    pub fn protect(plain: &[u8]) -> Option<Vec<u8>> {
        Some(plain.to_vec())
    }

    pub fn unprotect(cipher: &[u8]) -> Option<Vec<u8>> {
        Some(cipher.to_vec())
    }
}

// ==================== SVN Service ====================

pub struct SvnService;

impl SvnService {
    pub async fn test_connection(
        url: &str,
        username: &str,
        password: &str,
    ) -> Result<bool, String> {
        let mut cmd = new_svn_command();
        cmd.arg("info")
            .arg(url)
            .arg("--non-interactive")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        add_trust_cert_arg(&mut cmd);

        if !username.is_empty() {
            cmd.arg("--username").arg(username);
        }
        if !password.is_empty() {
            cmd.arg("--password").arg(password);
        }

        match run_svn_with_timeout(&mut cmd).await {
            Ok(output) => {
                if output.status.success() {
                    Ok(true)
                } else {
                    let stderr = decode_svn_bytes(&output.stderr);
                    let stdout = decode_svn_bytes(&output.stdout);
                    let msg = if !stderr.trim().is_empty() {
                        stderr.trim().to_string()
                    } else if !stdout.trim().is_empty() {
                        stdout.trim().to_string()
                    } else {
                        "未知错误".to_string()
                    };
                    Err(msg)
                }
            }
            Err(e) => Err(format!("执行 svn 命令失败: {}", e)),
        }
    }

    pub async fn get_log(
        url: &str,
        username: &str,
        password: &str,
        start_date: &str,
        end_date: &str,
    ) -> Result<Vec<CommitRecord>, String> {
        let mut cmd = new_svn_command();
        cmd.arg("log")
            .arg(url)
            .arg("-v")
            .arg("--xml")
            .arg("--non-interactive")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        add_trust_cert_arg(&mut cmd);

        if !start_date.is_empty() || !end_date.is_empty() {
            // SVN {DATE} 表示特定时间点，需要扩展为全天范围才能搜到当天日志；
            // 缺失的一侧用半开区间（最早时间 / HEAD）
            let start_part = if start_date.is_empty() {
                "{1970-01-01T00:00:00}".to_string()
            } else {
                format!("{{{}}}", format!("{}T00:00:00", start_date))
            };
            let end_part = if end_date.is_empty() {
                "HEAD".to_string()
            } else {
                format!("{{{}}}", format!("{}T23:59:59", end_date))
            };
            cmd.arg("-r").arg(format!("{}:{}", start_part, end_part));
        }
        if !username.is_empty() {
            cmd.arg("--username").arg(username);
        }
        if !password.is_empty() {
            cmd.arg("--password").arg(password);
        }

        let output = run_svn_with_timeout(&mut cmd)
            .await
            .map_err(|e| format!("执行 svn log 失败: {}", e))?;

        if !output.status.success() {
            let stderr = decode_svn_bytes(&output.stderr);
            return Err(format!("svn log 错误: {}", stderr));
        }

        let xml = decode_svn_bytes(&output.stdout);
        Self::parse_log_xml(&xml)
    }

    fn parse_log_xml(xml: &str) -> Result<Vec<CommitRecord>, String> {
        let mut records = Vec::new();
        // 使用 [\s\S]*? 匹配跨行内容（Rust regex 中 . 不匹配换行）
        let entry_re = Regex::new(r#"<logentry[^>]*revision="(\d+)"[^>]*>([\s\S]*?)</logentry>"#).unwrap();

        for cap in entry_re.captures_iter(xml) {
            let rev: i64 = cap[1].parse().unwrap_or(0);
            let body = &cap[2];

            let author_re = Regex::new(r#"<author>([\s\S]*?)</author>"#).unwrap();
            let date_re = Regex::new(r#"<date>([\s\S]*?)</date>"#).unwrap();
            let msg_re = Regex::new(r#"<msg>([\s\S]*?)</msg>"#).unwrap();
            // 注意: <path[^>]*> 会错误匹配 <paths> 包装标签 (因为 <path 是 <paths> 的前缀)
            // 改为 <path(?:\s[^>]*)?> 确保 <path 后面是空格(有属性)或直接 > (无属性),不会匹配 <paths>
            // 同时提取 action 属性 (A=新增, M=修改, D=删除, R=替换)
            let path_re = Regex::new(r#"<path(?:\s[^>]*)?>([\s\S]*?)</path>"#).unwrap();
            let action_re = Regex::new(r#"\baction="([A-Z])""#).unwrap();

            // XML 输出中的实体需要反转义，否则作者/说明/路径中的 & < > 等字符会失真
            let author = author_re.captures(body).map(|c| unescape_xml(c[1].trim())).unwrap_or_default();
            let date = date_re.captures(body).map(|c| c[1].trim().to_string()).unwrap_or_default();
            let msg = msg_re.captures(body).map(|c| unescape_xml(c[1].trim())).unwrap_or_default();

            let mut paths: Vec<String> = Vec::new();
            let mut deleted_paths: Vec<String> = Vec::new();
            for cap in path_re.captures_iter(body) {
                let p = unescape_xml(cap[1].trim());
                if p.is_empty() {
                    continue;
                }
                // 从整个 <path ...> 标签中提取 action 属性
                let full_tag = cap.get(0).map(|m| m.as_str()).unwrap_or("");
                let action = action_re
                    .captures(full_tag)
                    .map(|c| c[1].to_string())
                    .unwrap_or_default();
                if action == "D" {
                    deleted_paths.push(p.clone());
                }
                paths.push(p);
            }

            records.push(CommitRecord {
                revision: rev,
                author,
                date,
                message: msg,
                changed_paths: paths,
                deleted_paths,
            });
        }

        Ok(records)
    }

    /// Get the repository root URL for a given SVN URL.
    async fn get_repo_root(url: &str, username: &str, password: &str) -> Result<String, String> {
        let mut cmd = new_svn_command();
        cmd.arg("info")
            .arg(url)
            .arg("--non-interactive")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        add_trust_cert_arg(&mut cmd);

        // SVN >= 1.6 支持 --show-item repos-root-url
        let use_show_item = add_show_item_arg(&mut cmd, "repos-root-url");

        if !username.is_empty() {
            cmd.arg("--username").arg(username);
        }
        if !password.is_empty() {
            cmd.arg("--password").arg(password);
        }

        let output = run_svn_with_timeout(&mut cmd)
            .await
            .map_err(|e| format!("执行 svn info 失败: {}", e))?;
        if !output.status.success() {
            let stderr = decode_svn_bytes(&output.stderr);
            return Err(format!("svn info 错误: {}", stderr));
        }

        let stdout = decode_svn_bytes(&output.stdout);
        if use_show_item {
            Ok(stdout.trim().to_string())
        } else {
            // Fallback: SVN < 1.6 时从 svn info 输出中解析 Repository Root 行
            for line in stdout.lines() {
                if let Some(rest) = line.strip_prefix("Repository Root:") {
                    return Ok(rest.trim().to_string());
                }
            }
            Err("无法从 svn info 输出中解析 Repository Root".to_string())
        }
    }

    /// Get the diff text for a file between two revisions.
    /// `file_path` is the repository-root-relative path (e.g. `/trunk/entss/src/...`).
    pub async fn get_diff(
        url: &str,
        username: &str,
        password: &str,
        file_path: &str,
        from_rev: i64,
        to_rev: i64,
    ) -> Result<String, String> {
        // Get repo root to construct the full file URL
        let repo_root = Self::get_repo_root(url, username, password).await?;
        let full_url = format!("{}{}", repo_root, file_path);

        let context_arg = if file_path.to_lowercase().ends_with(".sql") || file_path.ends_with("db.txt") {
            "-U3" // SQL and db.txt: only show changes with minimal context
        } else {
            "-U100000" // Other files: show entire file
        };

        let mut cmd = new_svn_command();
        cmd.arg("diff")
            .arg(&full_url)
            .arg("-r")
            .arg(format!("{}:{}", from_rev, to_rev))
            .arg("-x")
            .arg(context_arg)
            .arg("--non-interactive")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        add_trust_cert_arg(&mut cmd);

        if !username.is_empty() {
            cmd.arg("--username").arg(username);
        }
        if !password.is_empty() {
            cmd.arg("--password").arg(password);
        }

        let output = run_svn_with_timeout(&mut cmd)
            .await
            .map_err(|e| format!("执行 svn diff 失败: {}", e))?;
        if !output.status.success() {
            let stderr = decode_svn_bytes(&output.stderr);
            return Err(format!("svn diff 错误: {}", stderr));
        }

        Ok(decode_svn_bytes(&output.stdout))
    }

    /// 获取仓库中指定文件在某个版本的内容（svn cat -r rev）。
    /// `file_path` 为仓库根相对路径（例如 `/trunk/xxx/pom.xml`）。
    pub async fn get_file_at_rev(
        url: &str,
        username: &str,
        password: &str,
        file_path: &str,
        rev: i64,
    ) -> Result<String, String> {
        let repo_root = Self::get_repo_root(url, username, password).await?;
        let full_url = format!("{}{}", repo_root, file_path);

        let mut cmd = new_svn_command();
        cmd.arg("cat")
            .arg(&full_url)
            .arg("-r")
            .arg(rev.to_string())
            .arg("--non-interactive")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        add_trust_cert_arg(&mut cmd);

        if !username.is_empty() {
            cmd.arg("--username").arg(username);
        }
        if !password.is_empty() {
            cmd.arg("--password").arg(password);
        }

        let output = run_svn_with_timeout(&mut cmd)
            .await
            .map_err(|e| format!("执行 svn cat 失败: {}", e))?;
        if !output.status.success() {
            let stderr = decode_svn_bytes(&output.stderr);
            return Err(format!("svn cat 错误: {}", stderr));
        }

        Ok(decode_svn_bytes(&output.stdout))
    }
}

// ==================== POM Dependency Models ====================

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct PomDependency {
    group_id: String,
    artifact_id: String,
    version: String,
}

impl PomDependency {
    fn key(&self) -> String {
        format!("{}:{}", self.group_id, self.artifact_id)
    }
}

// ==================== Packager Service ====================

pub struct PackagerService;

impl PackagerService {
    /// 解析 pom.xml 内容中的所有 <dependency> 块，提取 groupId/artifactId/version。
    /// 不区分 dependencies 与 dependencyManagement，统一提取；无法解析的变量版本 (${...}) 原样保留。
    fn parse_pom_dependencies(pom_xml: &str) -> Vec<PomDependency> {
        let mut deps = Vec::new();
        let dep_re = regex::Regex::new(r#"<dependency>([\s\S]*?)</dependency>"#).unwrap();
        let gid_re = regex::Regex::new(r#"<groupId>\s*([^<]+?)\s*</groupId>"#).unwrap();
        let aid_re = regex::Regex::new(r#"<artifactId>\s*([^<]+?)\s*</artifactId>"#).unwrap();
        let ver_re = regex::Regex::new(r#"<version>\s*([^<]+?)\s*</version>"#).unwrap();

        for cap in dep_re.captures_iter(pom_xml) {
            let body = &cap[1];
            let group_id = gid_re
                .captures(body)
                .map(|c| c[1].trim().to_string())
                .unwrap_or_default();
            let artifact_id = aid_re
                .captures(body)
                .map(|c| c[1].trim().to_string())
                .unwrap_or_default();
            let version = ver_re
                .captures(body)
                .map(|c| c[1].trim().to_string())
                .unwrap_or_default();
            if !artifact_id.is_empty() {
                deps.push(PomDependency {
                    group_id,
                    artifact_id,
                    version,
                });
            }
        }
        deps
    }

    /// 在 WEB-INF/lib 目录中查找匹配依赖的 jar 文件。
    /// 优先精确匹配 `{artifactId}-{version}.jar`；version 是变量或精确匹配失败时，
    /// 按 `{artifactId}-` 前缀模糊匹配，多候选时返回修改时间最新的一个。
    fn find_jar_in_lib(lib_dir: &Path, dep: &PomDependency) -> Option<PathBuf> {
        if !lib_dir.is_dir() {
            return None;
        }

        // 精确匹配
        if !dep.version.is_empty() && !dep.version.starts_with("${") {
            let exact_name = format!("{}-{}.jar", dep.artifact_id, dep.version);
            if let Ok(entries) = fs::read_dir(lib_dir) {
                for entry in entries.filter_map(|e| e.ok()) {
                    let path = entry.path();
                    if path.file_name().and_then(|n| n.to_str()) == Some(&exact_name) {
                        return Some(path);
                    }
                }
            }
        }

        // 模糊匹配：{artifactId}-*.jar
        let prefix = format!("{}-", dep.artifact_id);
        let mut candidates: Vec<PathBuf> = Vec::new();
        if let Ok(entries) = fs::read_dir(lib_dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                let path = entry.path();
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.starts_with(&prefix) && name.ends_with(".jar") {
                        candidates.push(path);
                    }
                }
            }
        }

        // 多候选：按修改时间倒序，取最新
        candidates.sort_by(|a, b| {
            let ta = a
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            let tb = b
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            tb.cmp(&ta)
        });
        candidates.into_iter().next()
    }

    /// 分析 pom.xml 在 [from_rev, to_rev] 版本范围内的依赖变更，
    /// 返回需要打包的 jar 文件路径（来自 war 产物 WEB-INF/lib）和需要执行的旧 jar 清理命令。
    pub async fn analyze_pom_jar_changes(
        svn_url: &str,
        username: &str,
        password: &str,
        pom_paths: &[String],
        from_rev: i64,
        to_rev: i64,
        project_root: &Path,
        app_name: &str,
    ) -> Result<(Vec<PathBuf>, Vec<String>, Vec<String>), String> {
        let war_dir = Self::find_war_exploded_dir_anywhere(project_root, app_name)
            .ok_or("未找到编译产物（war展开目录），请先编译项目")?;
        let lib_dir = war_dir.join("WEB-INF").join("lib");

        let mut jars: Vec<PathBuf> = Vec::new();
        let mut cleanup_commands: Vec<String> = Vec::new();
        let mut warnings: Vec<String> = Vec::new();

        for pom_path in pom_paths {
            // 获取新旧版本 pom.xml 内容（前端传入的 from_rev 已是首个选中版本的前一版，
            // 即变更前状态，此处不可再减一，否则会多算一次提交的变更）
            let old_pom = match SvnService::get_file_at_rev(
                svn_url,
                username,
                password,
                pom_path,
                from_rev,
            )
            .await
            {
                Ok(content) => content,
                Err(e) => {
                    warnings.push(format!("获取旧版本 {} 失败: {}", pom_path, e));
                    String::new()
                }
            };
            let new_pom = match SvnService::get_file_at_rev(
                svn_url,
                username,
                password,
                pom_path,
                to_rev,
            )
            .await
            {
                Ok(content) => content,
                Err(e) => {
                    warnings.push(format!("获取新版本 {} 失败: {}", pom_path, e));
                    String::new()
                }
            };

            let old_deps = Self::parse_pom_dependencies(&old_pom);
            let new_deps = Self::parse_pom_dependencies(&new_pom);

            let old_map: HashMap<String, PomDependency> = old_deps
                .iter()
                .map(|d| (d.key(), d.clone()))
                .collect();
            let new_map: HashMap<String, PomDependency> = new_deps
                .iter()
                .map(|d| (d.key(), d.clone()))
                .collect();

            // 新增的依赖：打包 jar
            for (key, new_dep) in &new_map {
                if !old_map.contains_key(key) {
                    match Self::find_jar_in_lib(&lib_dir, new_dep) {
                        Some(jar_path) => {
                            if !jars.contains(&jar_path) {
                                jars.push(jar_path);
                            }
                        }
                        None => {
                            warnings.push(format!(
                                "新增依赖 {}:{}:{} 在 WEB-INF/lib 中未找到 jar",
                                new_dep.group_id, new_dep.artifact_id, new_dep.version
                            ));
                        }
                    }
                }
            }

            // 版本变更的依赖：打包新版本 jar + 生成删除旧版本命令
            for (key, new_dep) in &new_map {
                if let Some(old_dep) = old_map.get(key) {
                    if old_dep.version != new_dep.version {
                        match Self::find_jar_in_lib(&lib_dir, new_dep) {
                            Some(jar_path) => {
                                if !jars.contains(&jar_path) {
                                    jars.push(jar_path);
                                }
                            }
                            None => {
                                warnings.push(format!(
                                    "版本变更依赖 {}:{}:{} 在 WEB-INF/lib 中未找到 jar",
                                    new_dep.group_id, new_dep.artifact_id, new_dep.version
                                ));
                            }
                        }
                        // 生成删除旧版本 jar 的命令
                        if !old_dep.version.is_empty()
                            && !old_dep.version.starts_with("${")
                        {
                            cleanup_commands.push(format!(
                                "rm -rf WEB-INF/lib/{}-{}.jar",
                                old_dep.artifact_id, old_dep.version
                            ));
                        }
                    }
                }
            }

            // 删除的依赖：生成清理命令
            for (key, old_dep) in &old_map {
                if !new_map.contains_key(key) {
                    if !old_dep.version.is_empty() && !old_dep.version.starts_with("${") {
                        cleanup_commands.push(format!(
                            "rm -rf WEB-INF/lib/{}-{}.jar",
                            old_dep.artifact_id, old_dep.version
                        ));
                    }
                }
            }
        }

        Ok((jars, cleanup_commands, warnings))
    }

    pub fn package_incremental(
        project_path: &str,
        output_dir: &str,
        app_name: &str,
        changed_files: &[String],
        extra_jar_files: &[PathBuf],
        cleanup_commands: &[String],
        file_rev_dates: &HashMap<String, String>,
        settings: &Settings,
        progress_callback: &dyn Fn(String),
    ) -> Result<String, String> {
        progress_callback("增量打包模式...".to_string());

        let project_root = PathBuf::from(project_path);
        progress_callback(format!("项目路径: {}", project_path));
        progress_callback(format!("应用名称: {}", app_name));
        progress_callback(format!("变更文件数: {}", changed_files.len()));

        let war_dir = Self::find_war_exploded_dir_anywhere(&project_root, app_name)
            .ok_or("未找到编译产物（war展开目录），请先编译项目")?;

        progress_callback(format!("编译产物目录: {}", war_dir.to_string_lossy()));

        let web_inf_classes = war_dir.join("WEB-INF").join("classes");
        progress_callback(format!("WEB-INF/classes: {} (存在: {})", web_inf_classes.to_string_lossy(), web_inf_classes.exists()));

        progress_callback("筛选变更文件...".to_string());
        let excluded_files: HashSet<String> = settings.excludes.iter().cloned().collect();
        let mut files_to_package: Vec<PathBuf> = Vec::new();
        // 收集产物比 SVN 提交旧（未重新编译）的文件，用于打包前中止
        let mut stale_files: Vec<String> = Vec::new();

        for changed_file in changed_files {
            progress_callback(format!("处理变更文件: {}", changed_file));
            let file_name = Path::new(changed_file)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");
            if excluded_files.contains(file_name) {
                progress_callback(format!("  跳过排除文件: {}", file_name));
                continue;
            }
            let resolved = Self::map_source_path_to_war_output(
                changed_file,
                &project_root,
                &war_dir,
                &web_inf_classes,
                progress_callback,
            );
            progress_callback(format!("  映射到 {} 个候选路径", resolved.len()));
            // 缺失文件兜底校验：.java 源文件映射不到任何产物时给出显式警告
            // （D 删除的文件由前端从 changed_files 中过滤，不会走到这里误报）
            if resolved.is_empty() && changed_file.ends_with(".java") {
                progress_callback(format!(
                    "⚠ 未找到 {} 对应的 .class 产物，可能未编译或路径映射失败",
                    changed_file
                ));
            }
            for p in &resolved {
                progress_callback(format!("    候选: {} (存在: {})", p.to_string_lossy(), p.exists()));
                // 产物与版本一致性校验：只对 .class 产物比对本地编译时间 vs SVN 提交时间，
                // 资源文件/文本跳过，避免误报
                if p.exists()
                    && p.extension().and_then(|e| e.to_str()) == Some("class")
                    && !excluded_files.contains(
                        p.file_name().and_then(|n| n.to_str()).unwrap_or(""),
                    )
                {
                    if let Some(date_str) = file_rev_dates.get(changed_file) {
                        Self::check_artifact_freshness(p, date_str, changed_file, &mut stale_files, progress_callback);
                    }
                }
            }
            for p in resolved {
                if p.exists()
                    && !files_to_package.contains(&p)
                    && !excluded_files.contains(
                        p.file_name().and_then(|n| n.to_str()).unwrap_or(""),
                    )
                {
                    files_to_package.push(p);
                }
            }
        }

        if files_to_package.is_empty() {
            progress_callback("警告：未在编译产物中找到任何变更文件，尝试使用源文件相对路径匹配...".to_string());
            for changed_file in changed_files {
                let mut cf = changed_file.clone();
                if cf.starts_with('/') {
                    cf = cf[1..].to_string();
                }
                let file_path = war_dir.join(&cf);
                if file_path.exists()
                    && !files_to_package.contains(&file_path)
                    && !excluded_files.contains(
                        file_path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or(""),
                    )
                {
                    files_to_package.push(file_path);
                }
            }
        }

        if files_to_package.is_empty() {
            progress_callback("没有找到需要打包的变更文件".to_string());
            return Err("没有找到需要打包的变更文件".to_string());
        }

        // 产物与版本一致性中止：存在比 SVN 提交旧的产物时停止打包，提醒先编译
        if !stale_files.is_empty() {
            progress_callback(format!(
                "⛔ 检测到 {} 个文件未在本地编译（产物比 SVN 提交旧），停止打包",
                stale_files.len()
            ));
            for f in &stale_files {
                progress_callback(format!("  - {}", f));
            }
            progress_callback("⛔ 请先编译项目，再重新打包".to_string());
            return Err(format!(
                "检测到 {} 个文件未在本地编译，已停止打包。请先编译项目后再打包。\n{}",
                stale_files.len(),
                stale_files.join("\n")
            ));
        }

        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
        let zip_file_name = format!("{}_incremental_{}.zip", app_name, timestamp);
        let zip_path = PathBuf::from(output_dir).join(&zip_file_name);

        fs::create_dir_all(output_dir).map_err(|e| e.to_string())?;

        let file = File::create(&zip_path).map_err(|e| e.to_string())?;

        // 清理命令先去重（闭包外声明，供打包完成后输出）
        let pending_cleanup_commands: Vec<String> = {
            let mut seen: HashSet<String> = HashSet::new();
            let mut unique_cmds: Vec<String> = Vec::new();
            for cmd in cleanup_commands {
                if seen.insert(cmd.clone()) {
                    unique_cmds.push(cmd.clone());
                }
            }
            unique_cmds
        };

        // zip 写入过程包在闭包内：任一步失败时删除半成品 zip，避免残留损坏文件
        let write_result = (|| -> Result<(), String> {
        let mut zip = zip::ZipWriter::new(file);
        let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

        let mut added_entries: HashSet<String> = HashSet::new();
        for file in &files_to_package {
            let entry_name = war_dir
                .join(file.strip_prefix(&war_dir).unwrap_or(file))
                .to_string_lossy()
                .replace('\\', "/");
            progress_callback(format!("添加: {}", entry_name));

            if file.is_dir() {
                for entry in walkdir::WalkDir::new(file).into_iter().filter_map(|e| e.ok()) {
                    let path = entry.path();
                    if path.is_file() {
                        let relative = path
                            .strip_prefix(&war_dir)
                            .unwrap_or(path)
                            .to_string_lossy()
                            .replace('\\', "/");
                        if added_entries.insert(relative.clone()) {
                            zip.start_file(&relative, options)
                                .map_err(|e| e.to_string())?;
                            let mut f = File::open(path).map_err(|e| e.to_string())?;
                            io::copy(&mut f, &mut zip).map_err(|e| e.to_string())?;
                        }
                    }
                }
            } else if file.is_file() {
                let relative = file
                    .strip_prefix(&war_dir)
                    .unwrap_or(file)
                    .to_string_lossy()
                    .replace('\\', "/");
                if added_entries.insert(relative.clone()) {
                    zip.start_file(&relative, options)
                        .map_err(|e| e.to_string())?;
                    let mut f = File::open(file).map_err(|e| e.to_string())?;
                    io::copy(&mut f, &mut zip).map_err(|e| e.to_string())?;
                }
            }
        }

        // 处理 pom.xml 变更引入的额外 jar 包（来自 WEB-INF/lib）
        if !extra_jar_files.is_empty() {
            progress_callback(format!("检测到 pom.xml 变更，追加 {} 个依赖 jar", extra_jar_files.len()));
            for jar_path in extra_jar_files {
                if !jar_path.is_file() {
                    progress_callback(format!("  跳过不存在的 jar: {}", jar_path.display()));
                    continue;
                }
                // 优先用相对 war_dir 的路径（WEB-INF/lib/xxx.jar），否则用文件名兜底
                let entry_name = jar_path
                    .strip_prefix(&war_dir)
                    .ok()
                    .map(|p| p.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_else(|| {
                        format!(
                            "WEB-INF/lib/{}",
                            jar_path
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("unknown.jar")
                        )
                    });
                if added_entries.insert(entry_name.clone()) {
                    progress_callback(format!("  添加依赖 jar: {}", entry_name));
                    zip.start_file(&entry_name, options)
                        .map_err(|e| e.to_string())?;
                    let mut f = File::open(jar_path).map_err(|e| e.to_string())?;
                    io::copy(&mut f, &mut zip).map_err(|e| e.to_string())?;
                }
            }
        }

        zip.finish().map_err(|e| e.to_string())?;
        Ok(())
        })();

        if let Err(e) = write_result {
            let _ = fs::remove_file(&zip_path);
            return Err(e);
        }

        progress_callback(format!(
            "增量打包完成: {}",
            zip_path.to_string_lossy()
        ));

        // 在打包完成后输出清理命令（用 ⚠ 前缀让前端渲染为注意色）
        if !pending_cleanup_commands.is_empty() {
            progress_callback(format!(
                "⚠ 检测到依赖变更，需要在服务器执行以下 {} 条清理命令（删除旧 jar）：",
                pending_cleanup_commands.len()
            ));
            progress_callback("⚠ # Windows 环境可改用: del /F /Q WEB-INF\\lib\\xxx.jar".to_string());
            for cmd in &pending_cleanup_commands {
                progress_callback(format!("⚠   {}", cmd));
            }
        }

        Ok(zip_path.to_string_lossy().to_string())
    }

    /// 产物与版本一致性校验：比对本地 .class 产物的修改时间与 SVN 提交时间。
    /// 产物 mtime 早于 SVN 提交时间 → 说明本地未编译最新代码，加入 stale_files 列表
    /// 并由调用方决定是否中止打包（避免把旧代码打进增量包）。
    fn check_artifact_freshness(
        artifact: &Path,
        svn_date_str: &str,
        svn_path: &str,
        stale_files: &mut Vec<String>,
        progress_callback: &dyn Fn(String),
    ) {
        // SVN 日期为 ISO 8601（如 2026-08-17T03:22:44.123456Z），转本地时间后再比
        let svn_time = match chrono::DateTime::parse_from_rfc3339(svn_date_str) {
            Ok(dt) => dt.with_timezone(&chrono::Local).naive_local(),
            Err(_) => return, // 日期解析失败则跳过，避免误报
        };
        let mtime = match fs::metadata(artifact).and_then(|m| m.modified()) {
            Ok(t) => {
                let dt: chrono::DateTime<chrono::Local> = t.into();
                dt.naive_local()
            }
            Err(_) => return,
        };
        if mtime < svn_time {
            let detail = format!(
                "{} (产物编译时间 {} 早于 SVN 提交时间 {})",
                svn_path,
                mtime.format("%Y-%m-%d %H:%M:%S"),
                svn_time.format("%Y-%m-%d %H:%M:%S")
            );
            if !stale_files.contains(&detail) {
                stale_files.push(detail.clone());
            }
            progress_callback(format!(
                "⚠ 产物较旧: {} 的本地编译时间 {} 早于 SVN 提交时间 {}, 可能打包到旧代码",
                svn_path,
                mtime.format("%Y-%m-%d %H:%M:%S"),
                svn_time.format("%Y-%m-%d %H:%M:%S")
            ));
        }
    }

    fn find_war_exploded_dir_anywhere(project_root: &Path, app_name: &str) -> Option<PathBuf> {
        let candidates = [
            project_root.join("target"),
            project_root.join("out").join("artifacts"),
            project_root.join("out"),
            project_root.join("build"),
        ];

        for dir in &candidates {
            if dir.exists() {
                if let Some(found) = Self::find_war_exploded_dir(dir, app_name) {
                    return Some(found);
                }
            }
        }

        let out_dir = project_root.join("out");
        if out_dir.exists() {
            if let Some(found) = Self::find_war_exploded_dir_recursive(&out_dir, 3) {
                return Some(found);
            }
        }

        None
    }

    fn find_war_exploded_dir(search_dir: &Path, app_name: &str) -> Option<PathBuf> {
        let candidates = [
            search_dir.join(app_name),
            search_dir.join(format!("{}-exploded", app_name)),
            search_dir.join(format!("{}_war_exploded", app_name)),
            search_dir.join("exploded-war"),
        ];

        for candidate in &candidates {
            if candidate.is_dir() && candidate.join("WEB-INF").is_dir() {
                return Some(candidate.clone());
            }
        }

        let normalized = app_name.replace('.', "_").replace('-', "_");
        if let Ok(entries) = fs::read_dir(search_dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                let path = entry.path();
                if path.is_dir() {
                    // 单个异常 entry 取不到文件名时跳过即可，不能终止整个搜索
                    let Some(name_os) = path.file_name() else { continue; };
                    let name = name_os.to_string_lossy().to_string();
                    if name.contains(app_name) || name.contains(&normalized) {
                        if path.join("WEB-INF").is_dir() {
                            return Some(path);
                        }
                    }
                }
            }
        }

        if let Ok(entries) = fs::read_dir(search_dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                let path = entry.path();
                if path.is_dir() && path.join("WEB-INF").is_dir() {
                    return Some(path);
                }
            }
        }

        None
    }

    fn find_war_exploded_dir_recursive(dir: &Path, max_depth: i32) -> Option<PathBuf> {
        if max_depth <= 0 {
            return None;
        }
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                let path = entry.path();
                if path.is_dir() && path.join("WEB-INF").is_dir() {
                    return Some(path);
                }
            }
        }
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                let path = entry.path();
                if path.is_dir() {
                    if let Some(found) = Self::find_war_exploded_dir_recursive(&path, max_depth - 1) {
                        return Some(found);
                    }
                }
            }
        }
        None
    }

    fn map_source_path_to_war_output(
        svn_path: &str,
        project_root: &Path,
        war_dir: &Path,
        web_inf_classes: &Path,
        progress_callback: &dyn Fn(String),
    ) -> Vec<PathBuf> {
        let mut results = Vec::new();
        let local_path = Self::to_local_path(svn_path);

        if local_path.contains("src/main/java/") || local_path.contains("src\\main\\java\\") {
            let relative_to_src = if local_path.contains("src/main/java/") {
                local_path.split("src/main/java/").nth(1).unwrap_or("").to_string()
            } else {
                local_path
                    .split("src\\main\\java\\")
                    .nth(1)
                    .unwrap_or("")
                    .to_string()
            };
            if relative_to_src.ends_with(".java") {
                let class_base = &relative_to_src[..relative_to_src.len() - 5];
                let class_file = web_inf_classes.join(format!("{}.class", class_base));
                if class_file.exists() {
                    results.push(class_file.clone());
                }
                if let Some(parent_dir) = class_file.parent() {
                    if parent_dir.exists() {
                        let simple_name = if class_base.contains('/') {
                            class_base.rsplit('/').next().unwrap_or(class_base)
                        } else {
                            class_base
                        };
                        // Inner/nested classes of the main class (MainClass$*.class)
                        Self::collect_inner_classes(parent_dir, simple_name, &mut results);
                        // Sibling top-level classes declared in the same .java file.
                        // Use find_java_source to handle various project layouts and SVN path prefixes.
                        // The SVN path may include a project prefix (e.g. entss/src/main/java/...)
                        // but the local checkout root may or may not include that prefix.
                        if let Some(java_file) = Self::find_java_source(project_root, class_base) {
                            progress_callback(format!("  找到 Java 源文件: {}", java_file.display()));
                            for cn in Self::extract_class_names(&java_file) {
                                if cn == simple_name {
                                    continue; // Main class already added above
                                }
                                let cls_file = parent_dir.join(format!("{}.class", cn));
                                if cls_file.exists() && !results.contains(&cls_file) {
                                    results.push(cls_file);
                                }
                                // Also collect inner classes of siblings (SiblingClass$*.class)
                                Self::collect_inner_classes(parent_dir, &cn, &mut results);
                            }
                        } else {
                            progress_callback(format!("⚠ 未找到 Java 源文件: class_base='{}' under project_root='{}'", class_base, project_root.display()));
                        }
                    }
                }
            }
        } else if local_path.contains("src/main/resources/")
            || local_path.contains("src\\main\\resources\\")
        {
            let relative_to_resources = if local_path.contains("src/main/resources/") {
                local_path.split("src/main/resources/").nth(1).unwrap_or("").to_string()
            } else {
                local_path
                    .split("src\\main\\resources\\")
                    .nth(1)
                    .unwrap_or("")
                    .to_string()
            };
            let resource_file = web_inf_classes.join(&relative_to_resources);
            if resource_file.exists() {
                results.push(resource_file);
            }
        } else if local_path.contains("src/main/webapp/")
            || local_path.contains("src\\main\\webapp\\")
        {
            let relative_to_webapp = if local_path.contains("src/main/webapp/") {
                local_path.split("src/main/webapp/").nth(1).unwrap_or("").to_string()
            } else {
                local_path
                    .split("src\\main\\webapp\\")
                    .nth(1)
                    .unwrap_or("")
                    .to_string()
            };
            let webapp_file = war_dir.join(&relative_to_webapp);
            if webapp_file.exists() {
                results.push(webapp_file);
            }
        } else if local_path.contains("WebRoot/") || local_path.contains("WebRoot\\") {
            let relative_to_webroot = if local_path.contains("WebRoot/") {
                local_path.split("WebRoot/").nth(1).unwrap_or("").to_string()
            } else {
                local_path
                    .split("WebRoot\\")
                    .nth(1)
                    .unwrap_or("")
                    .to_string()
            };
            let webapp_file = war_dir.join(&relative_to_webroot);
            if webapp_file.exists() {
                results.push(webapp_file);
            }
        } else if local_path.starts_with("src/") || local_path.starts_with("src\\") {
            let mut relative_to_src = if local_path.starts_with("src/") {
                local_path[4..].to_string()
            } else {
                local_path[4..].to_string()
            };
            if relative_to_src.starts_with('/') || relative_to_src.starts_with('\\') {
                relative_to_src = relative_to_src[1..].to_string();
            }
            if relative_to_src.ends_with(".java") {
                let class_base = &relative_to_src[..relative_to_src.len() - 5];
                let class_file = web_inf_classes.join(format!("{}.class", class_base));
                if class_file.exists() {
                    results.push(class_file.clone());
                }
                if let Some(parent_dir) = class_file.parent() {
                    if parent_dir.exists() {
                        let simple_name = if class_base.contains('/') {
                            class_base.rsplit('/').next().unwrap_or(class_base)
                        } else {
                            class_base
                        };
                        // Inner/nested classes of the main class (MainClass$*.class)
                        Self::collect_inner_classes(parent_dir, simple_name, &mut results);
                        // Sibling top-level classes declared in the same .java file.
                        // Use find_java_source to handle various project layouts and SVN path prefixes.
                        if let Some(java_file) = Self::find_java_source(project_root, class_base) {
                            progress_callback(format!("  找到 Java 源文件: {}", java_file.display()));
                            for cn in Self::extract_class_names(&java_file) {
                                if cn == simple_name {
                                    continue; // Main class already added above
                                }
                                let cls_file = parent_dir.join(format!("{}.class", cn));
                                if cls_file.exists() && !results.contains(&cls_file) {
                                    results.push(cls_file);
                                }
                                // Also collect inner classes of siblings (SiblingClass$*.class)
                                Self::collect_inner_classes(parent_dir, &cn, &mut results);
                            }
                        } else {
                            progress_callback(format!("⚠ 未找到 Java 源文件: class_base='{}' under project_root='{}'", class_base, project_root.display()));
                        }
                    }
                }
            } else {
                let resource_file = web_inf_classes.join(&relative_to_src);
                if resource_file.exists() {
                    results.push(resource_file);
                }
            }
        }

        if results.is_empty() {
            let file_name = Path::new(&local_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");
            if file_name.contains('.') && web_inf_classes.exists() {
                let base_name = &file_name[..file_name.rfind('.').unwrap_or(0)];
                // First pass: find main class file + its inner classes by scanning WEB-INF/classes
                let mut found_parent: Option<PathBuf> = None;
                for entry in walkdir::WalkDir::new(web_inf_classes).into_iter().filter_map(|e| e.ok()) {
                    let path = entry.path();
                    if path.is_file() {
                        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                        if name == file_name
                            || name == format!("{}.class", base_name)
                            || name.starts_with(&format!("{}$", base_name))
                        {
                            let pb = path.to_path_buf();
                            if !results.contains(&pb) {
                                results.push(pb);
                            }
                            if name == format!("{}.class", base_name) {
                                found_parent = path.parent().map(|p| p.to_path_buf());
                            }
                        }
                    }
                }
                // Second pass: find sibling top-level classes from Java source
                if let Some(parent_dir) = found_parent {
                    // Derive class_base from the found class file path relative to WEB-INF/classes
                    let main_class_path = parent_dir.join(format!("{}.class", base_name));
                    let class_base = main_class_path
                        .strip_prefix(web_inf_classes)
                        .ok()
                        .map(|p| p.to_string_lossy().replace('\\', "/"))
                        .and_then(|s| {
                            if s.ends_with(".class") {
                                Some(s[..s.len() - 6].to_string())
                            } else {
                                None
                            }
                        });
                    if let Some(cb) = class_base {
                        if let Some(java_file) = Self::find_java_source(project_root, &cb) {
                            progress_callback(format!("  找到 Java 源文件 (fallback): {}", java_file.display()));
                            for cn in Self::extract_class_names(&java_file) {
                                if cn == base_name {
                                    continue;
                                }
                                let cls_file = parent_dir.join(format!("{}.class", cn));
                                if cls_file.exists() && !results.contains(&cls_file) {
                                    results.push(cls_file);
                                }
                                Self::collect_inner_classes(&parent_dir, &cn, &mut results);
                            }
                        } else {
                            progress_callback(format!("⚠ 未找到 Java 源文件 (fallback): class_base='{}' under project_root='{}'", cb, project_root.display()));
                        }
                    }
                }
            }
        }

        if results.is_empty() {
            let clean_path = if local_path.starts_with('/') {
                &local_path[1..]
            } else {
                &local_path
            };
            let direct_in_war = war_dir.join(clean_path);
            if direct_in_war.exists() {
                results.push(direct_in_war);
            }
        }

        if results.is_empty() {
            let clean_path = if local_path.starts_with('/') {
                &local_path[1..]
            } else {
                &local_path
            };
            let parts: Vec<&str> = clean_path.split('/').collect();
            if parts.len() >= 2 && parts[0] != "src" && parts[0] != "WEB-INF" && parts[0] != "WebRoot" {
                let stripped = parts[1..].join("/");
                let retry_in_war = war_dir.join(&stripped);
                if retry_in_war.exists() {
                    results.push(retry_in_war);
                }
            }
        }

        results
    }

    fn to_local_path(svn_path: &str) -> String {
        let mut path = svn_path.to_string();
        if path.starts_with('/') {
            path = path[1..].to_string();
        }
        if path.starts_with("trunk/") {
            path = path[6..].to_string();
        } else if path.starts_with("branches/") {
            path = path[9..].to_string();
            if let Some(slash_idx) = path.find('/') {
                path = path[slash_idx + 1..].to_string();
            }
        } else if path.starts_with("tags/") {
            path = path[5..].to_string();
            if let Some(slash_idx) = path.find('/') {
                path = path[slash_idx + 1..].to_string();
            }
        }
        path
    }

    fn extract_class_names(java_file: &Path) -> Vec<String> {
        let mut class_names = Vec::new();
        if !java_file.exists() {
            return class_names;
        }
        // Read as bytes to handle non-UTF-8 encodings (GBK/gb18030 common in Chinese projects).
        // from_utf8_lossy preserves all ASCII content; non-ASCII bytes become U+FFFD.
        // Since the regex only matches ASCII keywords, this works for both UTF-8 and GBK files.
        let bytes = match fs::read(java_file) {
            Ok(b) => b,
            Err(_) => return class_names,
        };
        let content = String::from_utf8_lossy(&bytes);
        // Word boundaries (\b) prevent matching "class" inside words like "subclass".
        // "interface" also matches "@interface" because @ is a non-word char (boundary before 'i').
        let re = regex::Regex::new(r"\b(?:class|interface|enum)\b\s+(\w+)").unwrap();
        for cap in re.captures_iter(&content) {
            if let Some(m) = cap.get(1) {
                let name = m.as_str().to_string();
                if !class_names.contains(&name) {
                    class_names.push(name);
                }
            }
        }
        class_names
    }

    /// Try to locate the Java source file for a given class base path (e.g. "com/dckj/kkgl/service/JxrlAuditPolicyService").
    ///
    /// The SVN log path may include a project prefix (e.g. `entss/src/main/java/...`) but the local
    /// checkout root may or may not include that prefix.  Instead of guessing, we try multiple
    /// candidate locations derived from `class_base` (the package path), which is the reliable part.
    fn find_java_source(project_root: &Path, class_base: &str) -> Option<PathBuf> {
        let java_rel = format!("{}.java", class_base);

        // 1. Standard Maven layout:  project_root/src/main/java/<package>/Foo.java
        let c = project_root.join("src").join("main").join("java").join(&java_rel);
        if c.exists() {
            return Some(c);
        }

        // 2. Simple src layout:  project_root/src/<package>/Foo.java
        let c = project_root.join("src").join(&java_rel);
        if c.exists() {
            return Some(c);
        }

        // 3. One level deep — handles SVN paths with a project prefix when the local
        //    checkout root is the *parent* of the project directory.
        //    e.g. project_root/entss/src/main/java/<package>/Foo.java
        if let Ok(entries) = fs::read_dir(project_root) {
            for entry in entries.filter_map(|e| e.ok()) {
                let dir = entry.path();
                if !dir.is_dir() {
                    continue;
                }
                // Skip common non-source directories
                let name = entry.file_name().to_string_lossy().to_string();
                if matches!(name.as_str(), "target" | "node_modules" | ".git" | ".svn" | "WEB-INF") {
                    continue;
                }
                let c = dir.join("src").join("main").join("java").join(&java_rel);
                if c.exists() {
                    return Some(c);
                }
                let c = dir.join("src").join(&java_rel);
                if c.exists() {
                    return Some(c);
                }
            }
        }

        // 4. Two levels deep (rare but possible: repo/trunk/entss/src/main/java/...)
        if let Ok(entries) = fs::read_dir(project_root) {
            for entry in entries.filter_map(|e| e.ok()) {
                let dir1 = entry.path();
                if !dir1.is_dir() {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().to_string();
                if matches!(name.as_str(), "target" | "node_modules" | ".git" | ".svn" | "WEB-INF") {
                    continue;
                }
                if let Ok(sub_entries) = fs::read_dir(&dir1) {
                    for sub_entry in sub_entries.filter_map(|e| e.ok()) {
                        let dir2 = sub_entry.path();
                        if !dir2.is_dir() {
                            continue;
                        }
                        let c = dir2.join("src").join("main").join("java").join(&java_rel);
                        if c.exists() {
                            return Some(c);
                        }
                        let c = dir2.join("src").join(&java_rel);
                        if c.exists() {
                            return Some(c);
                        }
                    }
                }
            }
        }

        None
    }

    /// Scan parent_dir for inner/nested classes of the given simple class name.
    /// Matches: SimpleName$*.class (e.g., Foo$1.class, Foo$Bar.class, Foo$1$Inner.class)
    fn collect_inner_classes(parent_dir: &Path, simple_name: &str, results: &mut Vec<PathBuf>) {
        let prefix = format!("{}$", simple_name);
        if let Ok(entries) = fs::read_dir(parent_dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with(&prefix) && name.ends_with(".class") {
                    let p = entry.path();
                    if !results.contains(&p) {
                        results.push(p);
                    }
                }
            }
        }
    }
}

// ==================== Tauri Commands ====================

#[tauri::command]
async fn test_svn_connection(
    url: String,
    username: String,
    password: String,
) -> Result<bool, String> {
    SvnService::test_connection(&url, &username, &password).await
}

#[tauri::command]
async fn get_svn_log(
    url: String,
    username: String,
    password: String,
    start_date: String,
    end_date: String,
) -> Result<Vec<CommitRecord>, String> {
    SvnService::get_log(&url, &username, &password, &start_date, &end_date).await
}

#[tauri::command]
async fn get_svn_diff(
    url: String,
    username: String,
    password: String,
    file_path: String,
    from_rev: i64,
    to_rev: i64,
) -> Result<String, String> {
    SvnService::get_diff(&url, &username, &password, &file_path, from_rev, to_rev).await
}

#[tauri::command]
fn get_projects(state: State<ConfigManager>) -> Vec<SvnProject> {
    state.projects.lock().unwrap().clone()
}

#[tauri::command]
fn add_project(state: State<ConfigManager>, project: SvnProject) -> Result<(), String> {
    let mut projects = state.projects.lock().unwrap();
    projects.push(project);
    ConfigManager::save_projects(&projects)
}

#[tauri::command]
fn update_project(state: State<ConfigManager>, project: SvnProject) -> Result<(), String> {
    let mut projects = state.projects.lock().unwrap();
    let Some(idx) = projects.iter().position(|p| p.id == project.id) else {
        return Err("要更新的项目不存在".to_string());
    };
    projects[idx] = project;
    ConfigManager::save_projects(&projects)
}

#[tauri::command]
fn remove_project(state: State<ConfigManager>, id: String) -> Result<(), String> {
    let mut projects = state.projects.lock().unwrap();
    projects.retain(|p| p.id != id);
    ConfigManager::save_projects(&projects)
}

#[tauri::command]
fn get_settings(state: State<ConfigManager>) -> Settings {
    state.settings.lock().unwrap().clone()
}

#[tauri::command]
fn save_settings(state: State<ConfigManager>, settings: Settings) -> Result<(), String> {
    let mut s = state.settings.lock().unwrap();
    *s = settings;
    ConfigManager::save_settings(&s)
}

// ==================== POM Jar Change Result ====================

#[derive(Debug, Clone, Serialize)]
struct PomJarChangeResult {
    /// 需要打包的 jar 文件绝对路径（来自 war 产物 WEB-INF/lib）
    jars: Vec<String>,
    /// 需要在服务器上执行的旧 jar 删除命令
    cleanup_commands: Vec<String>,
    /// 解析过程中的警告信息（变量版本、未找到 jar 等）
    warnings: Vec<String>,
}

#[tauri::command]
async fn analyze_pom_jar_changes(
    svn_url: String,
    username: String,
    password: String,
    pom_paths: Vec<String>,
    from_rev: i64,
    to_rev: i64,
    project_path: String,
    app_name: String,
) -> Result<PomJarChangeResult, String> {
    let project_root = PathBuf::from(&project_path);
    let (jars, cleanup_commands, warnings) = PackagerService::analyze_pom_jar_changes(
        &svn_url,
        &username,
        &password,
        &pom_paths,
        from_rev,
        to_rev,
        &project_root,
        &app_name,
    )
    .await?;

    Ok(PomJarChangeResult {
        jars: jars.into_iter().map(|p| p.to_string_lossy().to_string()).collect(),
        cleanup_commands,
        warnings,
    })
}

#[tauri::command]
async fn package_incremental(
    app: AppHandle,
    state: State<'_, ConfigManager>,
    project_path: String,
    output_dir: String,
    app_name: String,
    changed_files: Vec<String>,
    extra_jar_files: Option<Vec<String>>,
    cleanup_commands: Option<Vec<String>>,
    file_rev_dates: Option<HashMap<String, String>>,
) -> Result<String, String> {
    let settings = state.settings.lock().unwrap().clone();

    // 将前端传入的 jar 绝对路径字符串转为 PathBuf
    let extra_jars: Vec<PathBuf> = extra_jar_files
        .unwrap_or_default()
        .into_iter()
        .filter_map(|s| {
            if s.is_empty() {
                None
            } else {
                Some(PathBuf::from(s))
            }
        })
        .collect();
    let cleanups = cleanup_commands.unwrap_or_default();
    let rev_dates = file_rev_dates.unwrap_or_default();

    // 放入阻塞线程池执行：重 IO 遍历与 zip 压缩不占用主线程，
    // 避免 UI 卡顿、进度事件延迟到打包结束才批量到达
    tauri::async_runtime::spawn_blocking(move || {
        PackagerService::package_incremental(
            &project_path,
            &output_dir,
            &app_name,
            &changed_files,
            &extra_jars,
            &cleanups,
            &rev_dates,
            &settings,
            &|msg: String| {
                let _ = app.emit("package_progress", msg);
            },
        )
    })
    .await
    .map_err(|e| format!("打包任务被中断: {}", e))?
}

#[tauri::command]
fn check_dir_exists(path: String) -> bool {
    Path::new(&path).is_dir()
}

#[tauri::command]
fn open_directory(path: String) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ==================== Run ====================

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(ConfigManager::new())
        .invoke_handler(tauri::generate_handler![
            test_svn_connection,
            get_svn_log,
            get_svn_diff,
            get_projects,
            add_project,
            update_project,
            remove_project,
            get_settings,
            save_settings,
            analyze_pom_jar_changes,
            package_incremental,
            check_dir_exists,
            open_directory,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
