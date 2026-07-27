use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::process::Command;
use zip::write::SimpleFileOptions;
use regex::Regex;

// Windows: 阻止 svn.exe 弹出命令行窗口
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

fn new_svn_command() -> Command {
    let mut cmd = Command::new("svn");
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub output_dir: String,
    pub excludes: Vec<String>,
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

    fn load_projects() -> Vec<SvnProject> {
        let path = Self::projects_file();
        if !path.exists() {
            return vec![];
        }
        fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn save_projects(projects: &[SvnProject]) {
        let dir = Self::config_dir();
        let _ = fs::create_dir_all(&dir);
        let path = Self::projects_file();
        if let Ok(json) = serde_json::to_string_pretty(projects) {
            let _ = fs::write(path, json);
        }
    }

    fn load_settings() -> Settings {
        let path = Self::settings_file();
        if !path.exists() {
            return Settings::default();
        }
        fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn save_settings(settings: &Settings) {
        let dir = Self::config_dir();
        let _ = fs::create_dir_all(&dir);
        let path = Self::settings_file();
        if let Ok(json) = serde_json::to_string_pretty(settings) {
            let _ = fs::write(path, json);
        }
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
            .arg("--trust-server-cert-failures=unknown-ca,cn-mismatch,expired,not-yet-valid,other")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if !username.is_empty() {
            cmd.arg("--username").arg(username);
        }
        if !password.is_empty() {
            cmd.arg("--password").arg(password);
        }

        match cmd.output().await {
            Ok(output) => Ok(output.status.success()),
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
            .arg("--trust-server-cert-failures=unknown-ca,cn-mismatch,expired,not-yet-valid,other")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if !start_date.is_empty() {
            // SVN {DATE} 表示特定时间点，需要扩展为全天范围才能搜到当天日志
            let start = format!("{}T00:00:00", start_date);
            let end = format!("{}T23:59:59", end_date);
            cmd.arg("-r").arg(format!("{{{}}}:{{{}}}", start, end));
        }
        if !username.is_empty() {
            cmd.arg("--username").arg(username);
        }
        if !password.is_empty() {
            cmd.arg("--password").arg(password);
        }

        let output = cmd
            .output()
            .await
            .map_err(|e| format!("执行 svn log 失败: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("svn log 错误: {}", stderr));
        }

        let xml = String::from_utf8_lossy(&output.stdout);
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
            let path_re = Regex::new(r#"<path(?:\s[^>]*)?>([\s\S]*?)</path>"#).unwrap();

            let author = author_re.captures(body).map(|c| c[1].trim().to_string()).unwrap_or_default();
            let date = date_re.captures(body).map(|c| c[1].trim().to_string()).unwrap_or_default();
            let msg = msg_re.captures(body).map(|c| c[1].trim().to_string()).unwrap_or_default();
            let paths: Vec<String> = path_re.captures_iter(body).map(|c| c[1].trim().to_string()).collect();

            records.push(CommitRecord {
                revision: rev,
                author,
                date,
                message: msg,
                changed_paths: paths,
            });
        }

        Ok(records)
    }

    /// Get the repository root URL for a given SVN URL.
    async fn get_repo_root(url: &str, username: &str, password: &str) -> Result<String, String> {
        let mut cmd = new_svn_command();
        cmd.arg("info")
            .arg(url)
            .arg("--show-item")
            .arg("repos-root-url")
            .arg("--non-interactive")
            .arg("--trust-server-cert-failures=unknown-ca,cn-mismatch,expired,not-yet-valid,other")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if !username.is_empty() {
            cmd.arg("--username").arg(username);
        }
        if !password.is_empty() {
            cmd.arg("--password").arg(password);
        }

        let output = cmd.output().await.map_err(|e| format!("执行 svn info 失败: {}", e))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("svn info 错误: {}", stderr));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
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
            .arg("--trust-server-cert-failures=unknown-ca,cn-mismatch,expired,not-yet-valid,other")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if !username.is_empty() {
            cmd.arg("--username").arg(username);
        }
        if !password.is_empty() {
            cmd.arg("--password").arg(password);
        }

        let output = cmd.output().await.map_err(|e| format!("执行 svn diff 失败: {}", e))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("svn diff 错误: {}", stderr));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

// ==================== Packager Service ====================

pub struct PackagerService;

impl PackagerService {
    pub fn package_incremental(
        project_path: &str,
        output_dir: &str,
        app_name: &str,
        changed_files: &[String],
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
            let resolved =
                Self::map_source_path_to_war_output(changed_file, &project_root, &war_dir, &web_inf_classes);
            progress_callback(format!("  映射到 {} 个候选路径", resolved.len()));
            for p in &resolved {
                progress_callback(format!("    候选: {} (存在: {})", p.to_string_lossy(), p.exists()));
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

        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
        let zip_file_name = format!("{}_incremental_{}.zip", app_name, timestamp);
        let zip_path = PathBuf::from(output_dir).join(&zip_file_name);

        fs::create_dir_all(output_dir).map_err(|e| e.to_string())?;

        let file = File::create(&zip_path).map_err(|e| e.to_string())?;
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

        zip.finish().map_err(|e| e.to_string())?;

        progress_callback(format!(
            "增量打包完成: {}",
            zip_path.to_string_lossy()
        ));
        Ok(zip_path.to_string_lossy().to_string())
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
                    let name = path.file_name()?.to_string_lossy().to_string();
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
                            eprintln!("[SVN Packager] Found Java source: {}", java_file.display());
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
                            eprintln!("[SVN Packager] WARNING: Java source not found for class_base='{}' under project_root='{}'", class_base, project_root.display());
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
                            eprintln!("[SVN Packager] Found Java source: {}", java_file.display());
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
                            eprintln!("[SVN Packager] WARNING: Java source not found for class_base='{}' under project_root='{}'", class_base, project_root.display());
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
                            eprintln!("[SVN Packager] Found Java source (fallback): {}", java_file.display());
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
                            eprintln!("[SVN Packager] WARNING: Java source not found (fallback) for class_base='{}' under project_root='{}'", cb, project_root.display());
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
fn add_project(state: State<ConfigManager>, project: SvnProject) {
    let mut projects = state.projects.lock().unwrap();
    projects.push(project);
    ConfigManager::save_projects(&projects);
}

#[tauri::command]
fn update_project(state: State<ConfigManager>, project: SvnProject) {
    let mut projects = state.projects.lock().unwrap();
    if let Some(idx) = projects.iter().position(|p| p.id == project.id) {
        projects[idx] = project;
        ConfigManager::save_projects(&projects);
    }
}

#[tauri::command]
fn remove_project(state: State<ConfigManager>, id: String) {
    let mut projects = state.projects.lock().unwrap();
    projects.retain(|p| p.id != id);
    ConfigManager::save_projects(&projects);
}

#[tauri::command]
fn get_settings(state: State<ConfigManager>) -> Settings {
    state.settings.lock().unwrap().clone()
}

#[tauri::command]
fn save_settings(state: State<ConfigManager>, settings: Settings) {
    let mut s = state.settings.lock().unwrap();
    *s = settings;
    ConfigManager::save_settings(&s);
}

#[tauri::command]
fn package_incremental(
    app: AppHandle,
    state: State<ConfigManager>,
    project_path: String,
    output_dir: String,
    app_name: String,
    changed_files: Vec<String>,
) -> Result<String, String> {
    let settings = state.settings.lock().unwrap().clone();
    let app_handle = app.clone();

    PackagerService::package_incremental(
        &project_path,
        &output_dir,
        &app_name,
        &changed_files,
        &settings,
        &|msg: String| {
            let _ = app_handle.emit("package_progress", msg);
        },
    )
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
            package_incremental,
            check_dir_exists,
            open_directory,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
