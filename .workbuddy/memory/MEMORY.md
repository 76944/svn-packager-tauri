# SVN Packager Tauri - 项目记忆

## 项目概述
- Tauri 2.0 + React 19 + Rust 的 SVN 增量打包工具
- 核心逻辑在 `src-tauri/src/lib.rs`，`PackagerService` 负责路径映射和打包
- 前端在 `src/`，TypeScript + Tailwind CSS

## 架构要点
- `map_source_path_to_war_output()`: 将 SVN 变更路径映射到 war 编译产物中的实际文件
- 路径映射规则: `src/main/java/*.java` → `WEB-INF/classes/*.class`，`src/` → `WEB-INF/classes/`，`src/main/resources/` → `WEB-INF/classes/`，`src/main/webapp/` 或 `WebRoot/` → war 根
- `find_war_exploded_dir_anywhere()`: 在 target/out/build 目录下递归查找含 WEB-INF 的 war 展开目录
- `extract_class_names()`: 从 .java 源文件提取所有顶层类名（用于多类单文件场景）

## 2026-07-22 修复: 多顶层类 Java 文件的增量打包
- 问题: `JxrlAuditPolicyService.java` 含 34 个顶层类（责任链模式），编译出 34 个独立 .class，旧代码只打包主类
- 根因1: `fs::read_to_string` 对 GBK 编码文件失败 → 改为 `fs::read` + `String::from_utf8_lossy`
- 根因2: `java_file` 路径硬编码 `project_root.join("src/main/java")`，有前缀目录时找不到文件 → 改为 `project_root.join(&local_path)`
- 根因3: 正则 `(?:class|interface|enum|@interface)` 缺 `\b` → 改为 `\b(?:class|interface|enum)\b`
- 新增 `collect_inner_classes()` 辅助函数，统一处理主类和兄弟类的内部类 `$` 匹配
- 回退分支也增加了两遍扫描: 先找主类定位目录，再提取兄弟类名
