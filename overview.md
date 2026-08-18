# SVN Packager 改版总览

## 最新改动（2026-08-18）

### 健壮性修复（前后端多处）
- 后端：
  - 修复 pom.xml 依赖分析旧版本双减 Bug（前端已传首个选中版本的前一版，后端不再额外减一）
  - 配置文件改为 .tmp + rename 原子写入，写失败会向前端报错；损坏配置隔离为 *.corrupt.{时间戳} 保留，不再静默丢失
  - 敏感文件列表支持清空（仅旧配置缺失字段时才回填默认值）
  - 所有 svn 命令增加 60 秒超时（超时后杀掉子进程），防止网络挂起时永久卡住
  - svn 输出解码兼容 GBK（中文 Windows 报错不再乱码）；svn log XML 中的实体（&amp; 等）正确反转义
  - get_svn_log 日期范围：仅填开始或仅填结束也能正常查询（半开区间）
  - 打包失败时自动删除半成品 zip；修复 war 产物目录搜索中单个异常条目会终止整个搜索的问题
  - package_incremental 命令改为异步（spawn_blocking），重 IO/压缩不再阻塞主线程，进度日志实时到达
  - 项目密码落盘前用 Windows DPAPI 加密（enc: 前缀 + hex），旧明文配置下次保存自动迁移
- 前端：
  - 修复设置弹窗排除/敏感文件粘贴 CRLF 文本残留 \r 导致规则静默失效的 Bug
  - 敏感文件匹配改为文件名精确匹配；控制台日志限制 2000 条上限
  - 无待打包文件时禁用打包按钮与 Enter 快捷键；获取日志增加请求序号防竞态与日期范围校验
  - 新增全局 ErrorBoundary，渲染异常时显示可重新加载的错误页而非白屏

### 夜间主题改版（纯净中性灰 · 珊瑚红）
- tailwind.config.js：graphite 色阶由暖灰改为纯净中性灰（页面底 #0c0c0d、卡片 #171718、输入 #1f1f21、边框 #2c2c2f），并新增 accent 珊瑚红强调色阶（#f0655a 等）
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
