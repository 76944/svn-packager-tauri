# SVN Packager v2.0

基于 Tauri 2.0 + React + Rust 重构的 SVN 增量打包工具。

## 技术栈

- **前端**：React 19 + TypeScript + Tailwind CSS 3 + Lucide Icons
- **后端**：Rust（Tauri 2.0）
- **打包框架**：Tauri 2.0（替代原 Java/Swing）
- **SVN 接入**：系统 svn 命令行（替代原 SVNKit）

## 与原 Java 版本对比

| 维度 | Java v1.0 | Tauri v2.0 |
|------|-----------|------------|
| 运行时依赖 | 需安装 JRE | 无需 JRE，原生 exe |
| 打包体积 | ~50MB+ | ~10MB |
| UI 框架 | Swing + FlatLaf | WebView + React + Tailwind |
| SVN 接入 | SVNKit（纯 Java） | svn 命令行 |
| 跨平台 | Java 跨平台 | Windows/macOS/Linux |
| 内存占用 | 较高 | 低 |

## 项目结构

```
svn-packager-tauri/
├── src/                        # 前端代码
│   ├── App.tsx                 # 主应用组件
│   ├── main.tsx                # React 入口
│   ├── index.css               # Tailwind 导入
│   └── types.ts                # TypeScript 类型定义
├── src-tauri/                  # Rust 后端
│   ├── src/
│   │   ├── main.rs             # 入口
│   │   └── lib.rs              # 核心逻辑（配置/SVN/打包）
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   └── build.rs
├── index.html
├── vite.config.ts
├── tailwind.config.js
└── package.json
```

## 环境要求

- **Node.js** >= 18
- **Rust** >= 1.77.2（通过 [rustup](https://rustup.rs/) 安装）
- **系统已安装 svn 命令行工具**（如 TortoiseSVN 自带）

## 构建步骤

```bash
# 1. 安装前端依赖
npm install

# 2. 开发模式运行
npm run tauri:dev

# 3. 生产构建（生成 Windows 安装包）
npm run tauri:build
```

构建完成后，安装包位于 `src-tauri/target/release/bundle/msi/`。

## 核心功能

### 前端
- 白色现代风格 UI，侧边栏项目列表
- SVN 提交日志获取与筛选（日期范围、关键词搜索）
- 版本复选打包（增量/全量）
- 实时控制台日志输出
- 项目增删改查 + 系统设置

### 后端（Rust）
- `SvnService`：调用 `svn log --xml`、`svn info` 获取提交记录
- `PackagerService`：完整移植原 Java 的路径映射逻辑
  - `src/main/java/*.java` → `WEB-INF/classes/*.class`（含内部类 `$*.class`）
  - `src/main/resources/*` → `WEB-INF/classes/*`
  - `src/main/webapp/*` / `WebRoot/*` → war 根目录
  - 自动提取 Java 文件中的 class/interface/enum 名称
  - 自动剥离 trunk/branches/tags 前缀
- `ConfigManager`：JSON 配置持久化（存储在 `%APPDATA%/SVNPackager/config/`）
- 排除文件列表支持

## 路径映射规则（移植自 Java 版本）

原 Java `PackagerService.mapSourcePathToWarOutput()` 中的全部规则已逐条移植到 Rust：

1. Java 源文件 → class 文件（含内部类 `$` 匹配）
2. 资源文件 → WEB-INF/classes
3. webapp/WebRoot 文件 → war 根目录
4. 自动从 Java 源码提取 class/interface/enum/@interface 名称
5. 分支路径剥离（trunk/branches/*/tags/*）
6. 多级回退查找机制

## 注意事项

- 使用系统 `svn` 命令行工具，需确保 `svn.exe` 在系统 PATH 中（安装 TortoiseSVN 时勾选命令行工具即可）
- 项目本地路径需指向 Maven/Gradle 编译后的 out/target 目录上层，程序会自动搜索 war exploded 目录
- 配置数据存储在 `%APPDATA%\SVNPackager\config\` 下
