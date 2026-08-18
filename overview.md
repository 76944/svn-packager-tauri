# SVN Packager 改版总览

## 最新改动（2026-08-18）

### 夜间主题改版（冷蓝灰 · 珊瑚红）
- tailwind.config.js：graphite 色阶由暖灰改为冷蓝灰（页面底 #101014、卡片 #18181d、输入 #1f1f26、边框 #2b2b35），并新增 accent 珊瑚红强调色阶（#f0655a 等）
- src/index.css：同步更新暗色下 body 底色、滚动条、resizer 等硬编码值
- src/App.tsx：品牌强调红（logo、项目选中态、面包屑、卡片标题竖条、主按钮、焦点环、加载动画、头像渐变等）在暗色下切换为 accent 珊瑚红；错误/危险语义红（错误项目、删除按钮、diff 删除行、错误通知等）保持不变；控制台日志各级颜色补充暗色变体提升可读性
- 亮色主题完全不变；配色预览见 theme-preview.html

## 最新改动（2026-07-23）

### 1. 系统控制台
- 底部 console 面板保留，显示打包进度日志（无上限自动滚动）
- 顶部 notification 横幅作为成功/错误提示（4 秒自动消失）
- 打包进度同时通过 header 中的实时状态条显示
- 敏感文件提醒：打包提交包含以下文件时控制台输出警告：`db.txt`、`1jw_DDL.sql`、`2view-scrpts.sql`、`5jw_DML.sql`、`6config.txt`

### 2. 打包流程改造
- **Commits 视图**：按钮改为「进入打包」，按 Enter 快速进入打包预览
- **打包预览视图**（新增）：
  - 展示去重后的文件列表（按路径排序）
  - 每个文件显示：图标（按扩展名着色）、文件路径、关联版本号
  - 「打包」按钮（按 Enter 快速打包）+「返回」按钮
  - 打包完成后自动返回 commits 视图并清空选择

### 3. Diff 变更详情预览
- 双击文件列表中的文件，弹出 Diff 预览窗口
- 后端 `get_svn_diff` 命令：先获取 repo root URL，再执行 `svn diff -r {from}:{to}` 获取变更
- 前端解析 unified diff 格式，显示：
  - 旧/新行号两列（固定宽度 `shrink-0`，文本换行不侵占行号列）
  - `+`/`-` 符号列
  - 内容列（`whitespace-pre-wrap break-all`，可自由选择复制）
  - 新增行绿色背景，删除行红色背景
- 提供「复制全部」按钮

### 4. 之前的修复
- 多顶层类 Java 文件增量打包（`find_java_source` 智能路径查找 + `extract_class_names` 编码兼容）
- 移除全量打包功能
- 移除顶部统计卡片

## 技术栈
- Frontend: React + TypeScript + Tailwind CSS + Vite
- Backend: Rust + Tauri 2.x
- SVN: 通过 `svn` CLI 命令操作

## 构建
```bash
npx tsc --noEmit        # TypeScript 检查
npx vite build          # 前端构建
cargo build --release   # Rust 构建
```
二进制: `src-tauri/target/release/svn-packager.exe`
