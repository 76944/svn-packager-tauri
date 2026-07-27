import { useState, useEffect, useRef, useMemo, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  Plus,
  Settings,
  RefreshCw,
  FolderGit,
  Package,
  Trash2,
  Edit3,
  ChevronRight,
  GitCommit,
  Calendar,
  Search,
  CheckCircle2,
  X,
  Copy,
  ArrowLeft,
  FileCode,
  FileText,
  Loader2,
  Terminal,
} from "lucide-react";
import type { SvnProject, CommitRecord, Settings as AppSettings } from "./types";

function generateId() {
  return "p" + Date.now().toString(36) + Math.random().toString(36).substr(2, 5);
}

// ==================== Diff parsing ====================

interface DiffLine {
  type: "add" | "remove" | "context";
  oldNum: number | null;
  newNum: number | null;
  content: string;
}

interface DiffHunk {
  lines: DiffLine[];
}

function parseDiff(diffText: string): DiffHunk[] {
  const lines = diffText.split("\n");
  const hunks: DiffHunk[] = [];
  let currentHunk: DiffHunk | null = null;
  let oldLine = 0;
  let newLine = 0;

  for (const line of lines) {
    const hunkMatch = line.match(/^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/);
    if (hunkMatch) {
      oldLine = parseInt(hunkMatch[1]);
      newLine = parseInt(hunkMatch[2]);
      currentHunk = { lines: [] };
      hunks.push(currentHunk);
      continue;
    }
    if (!currentHunk) continue;
    if (line.startsWith("---") || line.startsWith("+++") || line.startsWith("Index:") || line.startsWith("===")) {
      continue;
    }
    if (line.startsWith("-")) {
      currentHunk.lines.push({ type: "remove", oldNum: oldLine++, newNum: null, content: line.slice(1) });
    } else if (line.startsWith("+")) {
      currentHunk.lines.push({ type: "add", oldNum: null, newNum: newLine++, content: line.slice(1) });
    } else if (line.startsWith(" ")) {
      currentHunk.lines.push({ type: "context", oldNum: oldLine++, newNum: newLine++, content: line.slice(1) });
    } else if (line === "") {
      currentHunk.lines.push({ type: "context", oldNum: oldLine++, newNum: newLine++, content: "" });
    } else if (line.startsWith("\\")) {
      // Skip meta lines like "\ No newline at end of file"
      continue;
    }
  }
  return hunks;
}

function getFileIcon(path: string) {
  if (path.endsWith(".java")) return <FileCode size={14} className="text-orange-500 shrink-0" />;
  if (path.endsWith(".xml")) return <FileCode size={14} className="text-blue-500 shrink-0" />;
  if (path.endsWith(".js") || path.endsWith(".ts") || path.endsWith(".tsx")) return <FileCode size={14} className="text-yellow-500 shrink-0" />;
  if (path.endsWith(".css") || path.endsWith(".scss")) return <FileCode size={14} className="text-pink-500 shrink-0" />;
  if (path.endsWith(".html") || path.endsWith(".htm")) return <FileCode size={14} className="text-red-500 shrink-0" />;
  if (path.endsWith(".sql")) return <FileCode size={14} className="text-purple-500 shrink-0" />;
  if (path.endsWith(".properties") || path.endsWith(".yml") || path.endsWith(".yaml") || path.endsWith(".json")) return <FileText size={14} className="text-green-500 shrink-0" />;
  return <FileText size={14} className="text-slate-400 shrink-0" />;
}

// ==================== App ====================

type View = "commits" | "package";

interface DiffState {
  filePath: string;
  loading: boolean;
  diff: string | null;
  error: string | null;
}

interface PackageFile {
  path: string;
  revisions: number[];
}

export default function App() {
  const [projects, setProjects] = useState<SvnProject[]>([]);
  const [currentProject, setCurrentProject] = useState<SvnProject | null>(null);
  const [commitRecords, setCommitRecords] = useState<CommitRecord[]>([]);
  const [selectedRevs, setSelectedRevs] = useState<Set<number>>(new Set());
  const [settings, setSettings] = useState<AppSettings>({
    output_dir: "output",
    excludes: [],
  });

  const [isPackaging, setIsPackaging] = useState(false);
  const [progressMsg, setProgressMsg] = useState("");
  const [notification, setNotification] = useState<{ type: "success" | "error"; message: string } | null>(null);
  const [consoleLogs, setConsoleLogs] = useState<string[]>([]);
  const consoleRef = useRef<HTMLDivElement>(null);
  const [view, setView] = useState<View>("commits");
  const [diffState, setDiffState] = useState<DiffState | null>(null);

  const [showProjectModal, setShowProjectModal] = useState(false);
  const [showSettingsModal, setShowSettingsModal] = useState(false);
  const [editingProject, setEditingProject] = useState<SvnProject | null>(null);

  const [dateStart, setDateStart] = useState("");
  const [dateEnd, setDateEnd] = useState("");
  const [searchQuery, setSearchQuery] = useState("");
  const [packageSearchQuery, setPackageSearchQuery] = useState("");
  const [isLoading, setIsLoading] = useState(false);

  // Listen for packaging progress
  useEffect(() => {
    const unlisten = listen<string>("package_progress", (event) => {
      setProgressMsg(event.payload);
      setConsoleLogs((prev) => [...prev, event.payload]);
    });
    return () => { unlisten.then((f) => f()); };
  }, []);

  // Auto-scroll console
  useEffect(() => {
    if (consoleRef.current) {
      consoleRef.current.scrollTop = consoleRef.current.scrollHeight;
    }
  }, [consoleLogs]);

  // Auto-clear notification
  useEffect(() => {
    if (notification) {
      const timer = setTimeout(() => setNotification(null), 4000);
      return () => clearTimeout(timer);
    }
  }, [notification]);

  useEffect(() => {
    const now = new Date();
    const weekAgo = new Date(now.getTime() - 7 * 24 * 60 * 60 * 1000);
    setDateEnd(now.toISOString().split("T")[0]);
    setDateStart(weekAgo.toISOString().split("T")[0]);
    loadProjects();
    loadSettings();
  }, []);

  async function loadProjects() {
    try {
      const list = await invoke<SvnProject[]>("get_projects");
      setProjects(list);
      if (list.length > 0 && !currentProject) {
        setCurrentProject(list[0]);
      }
    } catch (e) {
      setNotification({ type: "error", message: `加载项目失败: ${e}` });
    }
  }

  async function loadSettings() {
    try {
      const s = await invoke<AppSettings>("get_settings");
      setSettings(s);
    } catch (e) {
      console.error("加载设置失败:", e);
    }
  }

  async function handleFetchLogs() {
    if (!currentProject) return;
    setIsLoading(true);
    try {
      const records = await invoke<CommitRecord[]>("get_svn_log", {
        url: currentProject.svn_url,
        username: currentProject.username,
        password: currentProject.password,
        startDate: dateStart,
        endDate: dateEnd,
      });
      setCommitRecords(records);
      setSelectedRevs(new Set());
    } catch (e) {
      setNotification({ type: "error", message: `获取日志失败: ${e}` });
    }
    setIsLoading(false);
  }

  async function handleSaveProject(project: SvnProject) {
    try {
      if (projects.find((p) => p.id === project.id)) {
        await invoke("update_project", { project });
        setNotification({ type: "success", message: "项目已更新" });
      } else {
        await invoke("add_project", { project });
        setNotification({ type: "success", message: "项目已创建" });
      }
      await loadProjects();
      setCurrentProject(project);
      setShowProjectModal(false);
      setEditingProject(null);
    } catch (e) {
      setNotification({ type: "error", message: `保存项目失败: ${e}` });
    }
  }

  async function handleDeleteProject(id: string) {
    if (!confirm("确定要删除这个项目吗？")) return;
    try {
      await invoke("remove_project", { id });
      setNotification({ type: "success", message: "项目已删除" });
      await loadProjects();
      if (currentProject?.id === id) {
        setCurrentProject(null);
        setCommitRecords([]);
      }
    } catch (e) {
      setNotification({ type: "error", message: `删除项目失败: ${e}` });
    }
  }

  async function handleSaveSettings(newSettings: AppSettings) {
    try {
      await invoke("save_settings", { settings: newSettings });
      setSettings(newSettings);
      setNotification({ type: "success", message: "设置已保存" });
      setShowSettingsModal(false);
    } catch (e) {
      setNotification({ type: "error", message: `保存设置失败: ${e}` });
    }
  }

  async function handlePackage() {
    if (!currentProject) return;

    // 检查默认输出目录
    if (!settings.output_dir || !settings.output_dir.trim()) {
      setNotification({ type: "error", message: "请先设置默认输出目录" });
      setShowSettingsModal(true);
      return;
    }
    try {
      const exists = await invoke<boolean>("check_dir_exists", { path: settings.output_dir });
      if (!exists) {
        setNotification({ type: "error", message: `默认输出目录不存在: ${settings.output_dir}` });
        setShowSettingsModal(true);
        return;
      }
    } catch {
      setNotification({ type: "error", message: "检查输出目录失败，请重新设置" });
      setShowSettingsModal(true);
      return;
    }

    setIsPackaging(true);
    setProgressMsg("开始增量打包...");
    setConsoleLogs((prev) => [...prev, "开始增量打包..."]);
    try {
      const changedFiles = commitRecords
        .filter((r) => selectedRevs.has(r.revision))
        .flatMap((r) => r.changed_paths);
      const result = await invoke<string>("package_incremental", {
        projectPath: currentProject.local_path,
        outputDir: settings.output_dir,
        appName: currentProject.name,
        changedFiles,
      });
      setConsoleLogs((prev) => [...prev, `✓ 打包完成: ${result}`]);
    } catch (e) {
      setConsoleLogs((prev) => [...prev, `✗ 打包失败: ${e}`]);
    }
    setIsPackaging(false);
    setProgressMsg("");
  }

  async function handleFileDoubleClick(filePath: string, targetRev?: number) {
    if (!currentProject) return;
    const fileData = packageFiles.find((f) => f.path === filePath);
    if (!fileData || fileData.revisions.length === 0) return;
    let fromRev: number;
    let toRev: number;
    if (typeof targetRev === "number") {
      fromRev = targetRev - 1;
      toRev = targetRev;
    } else {
      fromRev = fileData.revisions[0] - 1;
      toRev = fileData.revisions[fileData.revisions.length - 1];
    }
    setDiffState({ filePath, loading: true, diff: null, error: null });
    try {
      const diff = await invoke<string>("get_svn_diff", {
        url: currentProject.svn_url,
        username: currentProject.username,
        password: currentProject.password,
        filePath,
        fromRev,
        toRev,
      });
      setDiffState({ filePath, loading: false, diff, error: null });
    } catch (e) {
      setDiffState({ filePath, loading: false, diff: null, error: String(e) });
    }
  }

  // Deduplicated file list from selected commits
  const packageFiles = useMemo<PackageFile[]>(() => {
    const fileMap = new Map<string, number[]>();
    for (const record of commitRecords) {
      if (selectedRevs.has(record.revision)) {
        for (const path of record.changed_paths) {
          if (path.endsWith("/")) continue;
          if (!fileMap.has(path)) fileMap.set(path, []);
          fileMap.get(path)!.push(record.revision);
        }
      }
    }
    return Array.from(fileMap.entries())
      .map(([path, revs]) => ({ path, revisions: revs.sort((a, b) => a - b) }))
      .sort((a, b) => a.path.localeCompare(b.path));
  }, [commitRecords, selectedRevs]);

  const filteredPackageFiles = useMemo(() => {
    const q = packageSearchQuery.trim().toLowerCase();
    if (!q) return packageFiles;
    return packageFiles.filter((f) => {
      const fileName = f.path.split("/").pop() || f.path;
      return fileName.toLowerCase().includes(q);
    });
  }, [packageFiles, packageSearchQuery]);

  const filteredRecords = useMemo(() => {
    return [...commitRecords]
      .sort((a, b) => b.revision - a.revision)
      .filter(
        (r) =>
          !searchQuery ||
          r.author.toLowerCase().includes(searchQuery.toLowerCase()) ||
          r.message.toLowerCase().includes(searchQuery.toLowerCase())
      );
  }, [commitRecords, searchQuery]);

  const toggleRev = (rev: number) => {
    const next = new Set(selectedRevs);
    if (next.has(rev)) next.delete(rev);
    else next.add(rev);
    setSelectedRevs(next);
  };

  const toggleAll = () => {
    if (selectedRevs.size === commitRecords.length) {
      setSelectedRevs(new Set());
    } else {
      setSelectedRevs(new Set(commitRecords.map((r) => r.revision)));
    }
  };

  // Enter key handler
  const handlePackageRef = useRef(handlePackage);
  handlePackageRef.current = handlePackage;

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key !== "Enter") return;
      const target = e.target as HTMLElement;
      if (["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName)) return;
      if (showProjectModal || showSettingsModal || diffState) return;
      if (view === "commits" && selectedRevs.size > 0 && !isPackaging) {
        e.preventDefault();
        setConsoleLogs([]);
        setView("package");
      } else if (view === "package" && !isPackaging) {
        e.preventDefault();
        handlePackageRef.current();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [view, selectedRevs.size, isPackaging, showProjectModal, showSettingsModal, diffState]);

  return (
    <div className="flex h-screen bg-slate-50 text-slate-800">
      {/* Sidebar */}
      <aside className="w-64 bg-white border-r border-slate-200 flex flex-col shrink-0">
        <div className="p-5 border-b border-slate-100">
          <div className="flex items-center gap-3">
            <div className="w-9 h-9 rounded-lg bg-brand-600 flex items-center justify-center text-white font-bold text-sm">
              SP
            </div>
            <div>
              <h1 className="font-semibold text-sm text-slate-900">SVN Packager</h1>
              <p className="text-[10px] text-slate-400 font-mono tracking-wider">v2.1.0</p>
            </div>
          </div>
        </div>

        <div className="flex-1 overflow-y-auto p-3">
          <div className="text-[10px] font-medium text-slate-400 uppercase tracking-wider mb-2 px-2">
            项目列表
          </div>
          {projects.length === 0 && (
            <div className="px-2 py-6 text-center text-xs text-slate-400">
              暂无项目，点击新增
            </div>
          )}
          <div className="space-y-1">
            {projects.map((p) => {
              const active = currentProject?.id === p.id;
              return (
                <div
                  key={p.id}
                  onClick={() => {
                    setCurrentProject(p);
                    setCommitRecords([]);
                    setSelectedRevs(new Set());
                    setView("commits");
                    setDiffState(null);
                  }}
                  className={`group flex items-center gap-3 px-3 py-2.5 rounded-lg cursor-pointer transition-all ${
                    active
                      ? "bg-brand-50 border-l-4 border-brand-500"
                      : "hover:bg-slate-50 border-l-4 border-transparent"
                  }`}
                >
                  <div
                    className={`w-7 h-7 rounded-md flex items-center justify-center text-[10px] font-bold shrink-0 ${
                      active ? "bg-brand-600 text-white" : "bg-slate-100 text-slate-500"
                    }`}
                  >
                    {p.name.substring(0, 2).toUpperCase()}
                  </div>
                  <div className="flex-1 min-w-0">
                    <div className={`text-xs font-medium truncate ${active ? "text-brand-700" : "text-slate-700"}`}>
                      {p.name}
                    </div>
                    <div className="text-[10px] text-slate-400 font-mono truncate">
                      {p.last_rev ? `r${p.last_rev}` : "未连接"}
                    </div>
                  </div>
                  <button
                    onClick={(e) => { e.stopPropagation(); setEditingProject(p); setShowProjectModal(true); }}
                    className="opacity-0 group-hover:opacity-100 p-1 rounded hover:bg-slate-200 text-slate-400 hover:text-slate-600 transition-opacity"
                  >
                    <Edit3 size={12} />
                  </button>
                  <button
                    onClick={(e) => { e.stopPropagation(); handleDeleteProject(p.id); }}
                    className="opacity-0 group-hover:opacity-100 p-1 rounded hover:bg-red-50 text-slate-400 hover:text-red-500 transition-opacity"
                  >
                    <Trash2 size={12} />
                  </button>
                </div>
              );
            })}
          </div>
        </div>

        <div className="p-3 border-t border-slate-100 flex gap-2">
          <button
            onClick={() => { setEditingProject(null); setShowProjectModal(true); }}
            className="flex-1 flex items-center justify-center gap-1.5 px-3 py-2 rounded-lg bg-slate-50 hover:bg-slate-100 border border-slate-200 text-xs font-medium text-slate-600 transition-colors"
          >
            <Plus size={14} /> 新增项目
          </button>
          <button
            onClick={() => setShowSettingsModal(true)}
            className="px-3 py-2 rounded-lg bg-slate-50 hover:bg-slate-100 border border-slate-200 text-slate-500 transition-colors"
          >
            <Settings size={14} />
          </button>
        </div>
      </aside>

      {/* Main */}
      <main className="flex-1 flex flex-col min-w-0">
        {/* Header */}
        <header className="h-14 bg-white border-b border-slate-200 flex items-center justify-between px-6 shrink-0">
          <div className="flex items-center gap-2 text-xs text-slate-400">
            <span className="font-mono uppercase tracking-wider">SVN Packager</span>
            <ChevronRight size={12} />
            <span className="text-brand-600 font-medium">{currentProject?.name || "项目控制台"}</span>
            {view === "package" && (
              <>
                <ChevronRight size={12} />
                <span className="text-slate-600 font-medium">打包预览</span>
              </>
            )}
          </div>
          <div className="flex items-center gap-3">
            {currentProject && (
              <div className="flex items-center gap-2 px-2.5 py-1 rounded-md bg-slate-50 border border-slate-200 text-[10px] font-mono text-slate-500">
                <FolderGit size={12} className="text-brand-500" />
                {currentProject.svn_url}
              </div>
            )}
            {isPackaging && (
              <div className="flex items-center gap-2 px-2.5 py-1 rounded-md bg-amber-50 border border-amber-200 text-[10px] font-mono text-amber-600 max-w-xs">
                <Loader2 size={12} className="animate-spin shrink-0" />
                <span className="truncate">{progressMsg || "打包中..."}</span>
              </div>
            )}
          </div>
        </header>

        {/* Content */}
        <div className="flex-1 overflow-hidden flex flex-col">
          {!currentProject ? (
            <div className="flex-1 flex flex-col items-center justify-center text-slate-400">
              <Package size={48} className="mb-4 text-slate-300" />
              <h2 className="text-sm font-medium text-slate-600 mb-1">欢迎使用 SVN Packager</h2>
              <p className="text-xs">点击左侧"新增项目"按钮开始配置</p>
            </div>
          ) : view === "commits" ? (
            /* ===== Commits View ===== */
            <div className="flex-1 overflow-y-auto p-6 scrollbar-thin">
              <div className="bg-white rounded-xl border border-slate-200 shadow-sm overflow-hidden">
                <div className="flex items-center justify-between px-5 py-3.5 border-b border-slate-100">
                  <div className="flex items-center gap-2">
                    <div className="w-1 h-4 bg-brand-500 rounded-full" />
                    <span className="text-xs font-semibold text-slate-700 uppercase tracking-wider">
                      SVN Commit Log
                    </span>
                  </div>
                  <span className="text-[10px] text-slate-400 font-mono">
                    {commitRecords.length > 0 ? `已获取 ${commitRecords.length} 条提交记录` : ""}
                  </span>
                </div>

                <div className="flex items-center gap-3 px-5 py-3 border-b border-slate-100 bg-slate-50/50">
                  <div className="flex items-center gap-2">
                    <Calendar size={14} className="text-slate-400" />
                    <input
                      type="date"
                      value={dateStart}
                      onChange={(e) => setDateStart(e.target.value)}
                      className="px-2.5 py-1.5 rounded-md border border-slate-200 bg-white text-xs text-slate-700 focus:outline-none focus:ring-2 focus:ring-brand-500/20 focus:border-brand-500"
                    />
                    <span className="text-slate-400 text-xs">~</span>
                    <input
                      type="date"
                      value={dateEnd}
                      onChange={(e) => setDateEnd(e.target.value)}
                      className="px-2.5 py-1.5 rounded-md border border-slate-200 bg-white text-xs text-slate-700 focus:outline-none focus:ring-2 focus:ring-brand-500/20 focus:border-brand-500"
                    />
                  </div>
                  <button
                    onClick={handleFetchLogs}
                    disabled={isLoading}
                    className="flex items-center gap-1.5 px-3 py-1.5 rounded-md bg-brand-50 hover:bg-brand-100 text-brand-700 text-xs font-medium transition-colors disabled:opacity-50"
                  >
                    <RefreshCw size={12} className={isLoading ? "animate-spin" : ""} />
                    获取日志
                  </button>
                  <div className="flex-1" />
                  <div className="relative">
                    <Search size={14} className="absolute left-2.5 top-1/2 -translate-y-1/2 text-slate-400" />
                    <input
                      type="text"
                      placeholder="搜索作者或提交说明..."
                      value={searchQuery}
                      onChange={(e) => setSearchQuery(e.target.value)}
                      className="pl-8 pr-3 py-1.5 w-56 rounded-md border border-slate-200 bg-white text-xs text-slate-700 focus:outline-none focus:ring-2 focus:ring-brand-500/20 focus:border-brand-500"
                    />
                  </div>
                  <button
                    onClick={() => {
                      setConsoleLogs([]);
                      setPackageSearchQuery("");
                      setView("package");
                    }}
                    disabled={selectedRevs.size === 0 || isPackaging}
                    className="flex items-center gap-1.5 px-3 py-1.5 rounded-md bg-brand-600 hover:bg-brand-700 text-white text-xs font-medium transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
                    title="按 Enter 快速进入"
                  >
                    <Package size={12} />
                    进入打包 ({selectedRevs.size})
                  </button>
                </div>

                <div className="overflow-auto max-h-[calc(100vh-220px)]">
                  <table className="w-full">
                    <thead className="sticky top-0 z-10">
                      <tr className="bg-slate-50 border-b border-slate-100">
                        <th className="px-4 py-2.5 text-left w-10">
                          <button
                            onClick={toggleAll}
                            className={`w-4 h-4 rounded border flex items-center justify-center text-[10px] transition-colors ${
                              selectedRevs.size === commitRecords.length && commitRecords.length > 0
                                ? "bg-brand-600 border-brand-600 text-white"
                                : "border-slate-300 hover:border-brand-400"
                            }`}
                          >
                            {selectedRevs.size === commitRecords.length && commitRecords.length > 0 && <CheckCircle2 size={10} />}
                          </button>
                        </th>
                        <th className="px-4 py-2.5 text-left text-[10px] font-semibold text-slate-400 uppercase tracking-wider">版本</th>
                        <th className="px-4 py-2.5 text-left text-[10px] font-semibold text-slate-400 uppercase tracking-wider">作者</th>
                        <th className="px-4 py-2.5 text-left text-[10px] font-semibold text-slate-400 uppercase tracking-wider">提交说明</th>
                        <th className="px-4 py-2.5 text-left text-[10px] font-semibold text-slate-400 uppercase tracking-wider">日期</th>
                        <th className="px-4 py-2.5 text-left text-[10px] font-semibold text-slate-400 uppercase tracking-wider">文件</th>
                      </tr>
                    </thead>
                    <tbody>
                      {filteredRecords.map((record) => {
                        const isSelected = selectedRevs.has(record.revision);
                        const dateObj = new Date(record.date);
                        const dateStr = `${(dateObj.getMonth() + 1).toString().padStart(2, "0")}-${dateObj.getDate().toString().padStart(2, "0")} ${dateObj.getHours().toString().padStart(2, "0")}:${dateObj.getMinutes().toString().padStart(2, "0")}`;
                        return (
                          <tr
                            key={record.revision}
                            onClick={() => toggleRev(record.revision)}
                            className={`border-b border-slate-50 cursor-pointer transition-colors ${isSelected ? "bg-brand-50/60" : "hover:bg-slate-50"}`}
                          >
                            <td className="px-4 py-3">
                              <div className={`w-4 h-4 rounded border flex items-center justify-center text-[10px] transition-colors ${isSelected ? "bg-brand-600 border-brand-600 text-white" : "border-slate-300"}`}>
                                {isSelected && <CheckCircle2 size={10} />}
                              </div>
                            </td>
                            <td className="px-4 py-3">
                              <span className="inline-flex items-center px-2 py-0.5 rounded bg-brand-50 text-brand-700 text-[11px] font-mono font-bold">
                                r{record.revision}
                              </span>
                            </td>
                            <td className="px-4 py-3">
                              <div className="flex items-center gap-2">
                                <div className="w-6 h-6 rounded-full bg-gradient-to-br from-brand-400 to-brand-600 flex items-center justify-center text-[9px] font-bold text-white">
                                  {record.author.substring(0, 2).toUpperCase()}
                                </div>
                                <span className="text-xs text-slate-700">{record.author}</span>
                              </div>
                            </td>
                            <td className="px-4 py-3">
                              <span className="text-xs text-slate-600 truncate max-w-[240px] block" title={record.message}>
                                {record.message}
                              </span>
                            </td>
                            <td className="px-4 py-3 text-[11px] font-mono text-slate-400">{dateStr}</td>
                            <td className="px-4 py-3">
                              <span className="inline-flex items-center px-2 py-0.5 rounded bg-slate-100 text-slate-600 text-[10px] font-mono">
                                {record.changed_paths.length} files
                              </span>
                            </td>
                          </tr>
                        );
                      })}
                    </tbody>
                  </table>
                  {filteredRecords.length === 0 && commitRecords.length > 0 && (
                    <div className="flex flex-col items-center justify-center py-8 text-slate-400">
                      <p className="text-xs">没有匹配的提交记录</p>
                    </div>
                  )}
                  {commitRecords.length === 0 && (
                    <div className="flex flex-col items-center justify-center py-16 text-slate-400">
                      <GitCommit size={32} className="mb-3 text-slate-300" />
                      <p className="text-sm text-slate-500 mb-1">暂无提交记录</p>
                      <p className="text-xs">选择日期范围后点击"获取日志"</p>
                    </div>
                  )}
                </div>
              </div>
            </div>
          ) : (
            /* ===== Package Preview View ===== */
            <>
              <div className="flex-1 overflow-hidden p-6 min-h-0">
                <div className="bg-white rounded-xl border border-slate-200 shadow-sm overflow-hidden h-full flex flex-col">
                  {/* Toolbar */}
                  <div className="flex items-center gap-3 px-5 py-3 border-b border-slate-100 shrink-0">
                    <button
                      onClick={() => setView("commits")}
                      className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-md bg-slate-50 hover:bg-slate-100 border border-slate-200 text-xs font-medium text-slate-600 transition-colors"
                    >
                      <ArrowLeft size={12} /> 返回
                    </button>
                    <div className="w-1 h-4 bg-brand-500 rounded-full" />
                    <span className="text-xs font-semibold text-slate-700 uppercase tracking-wider">打包预览</span>
                    <span className="text-[10px] text-slate-400 font-mono">{filteredPackageFiles.length} / {packageFiles.length} 个文件</span>
                    <div className="flex-1" />
                    <div className="relative">
                      <Search size={14} className="absolute left-2.5 top-1/2 -translate-y-1/2 text-slate-400" />
                      <input
                        type="text"
                        placeholder="搜索文件名..."
                        value={packageSearchQuery}
                        onChange={(e) => setPackageSearchQuery(e.target.value)}
                        className="pl-8 pr-3 py-1.5 w-48 rounded-md border border-slate-200 bg-white text-xs text-slate-700 focus:outline-none focus:ring-2 focus:ring-brand-500/20 focus:border-brand-500"
                      />
                    </div>
                    <span className="text-[10px] text-slate-400">双击文件查看变更详情</span>
                    <button
                      onClick={handlePackage}
                      disabled={isPackaging}
                      className="flex items-center gap-1.5 px-3 py-1.5 rounded-md bg-brand-600 hover:bg-brand-700 text-white text-xs font-medium transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
                      title="按 Enter 快速打包"
                    >
                      {isPackaging ? <Loader2 size={12} className="animate-spin" /> : <Package size={12} />}
                      打包
                    </button>
                  </div>

                  {/* File list */}
                  <div className="flex-1 overflow-auto min-h-0">
                    <table className="w-full">
                      <thead className="sticky top-0 z-10">
                        <tr className="bg-slate-50 border-b border-slate-100">
                          <th className="px-4 py-2.5 text-left text-[10px] font-semibold text-slate-400 uppercase tracking-wider w-10">#</th>
                          <th className="px-4 py-2.5 text-left text-[10px] font-semibold text-slate-400 uppercase tracking-wider">文件路径</th>
                          <th className="px-4 py-2.5 text-left text-[10px] font-semibold text-slate-400 uppercase tracking-wider w-40">版本</th>
                        </tr>
                      </thead>
                      <tbody>
                        {filteredPackageFiles.map((file, idx) => {
                          const fileName = file.path.split("/").pop() || file.path;
                          const dirPath = file.path.substring(0, file.path.length - fileName.length);
                          return (
                            <tr
                              key={file.path}
                              onDoubleClick={() => handleFileDoubleClick(file.path)}
                              className="border-b border-slate-50 cursor-pointer transition-colors hover:bg-brand-50/40"
                              title="双击查看变更详情"
                            >
                              <td className="px-4 py-2.5 text-[11px] font-mono text-slate-400">{idx + 1}</td>
                              <td className="px-4 py-2.5">
                                <div className="flex items-center gap-2 min-w-0">
                                  {getFileIcon(file.path)}
                                  <div className="min-w-0">
                                    <span className="text-xs font-medium text-slate-700">{fileName}</span>
                                    <span className="text-[10px] text-slate-400 font-mono ml-1 truncate">{dirPath}</span>
                                  </div>
                                </div>
                              </td>
                              <td className="px-4 py-2.5">
                                <div className="flex items-center gap-1 flex-wrap">
                                  {file.revisions.map((rev) => (
                                    <button
                                      key={rev}
                                      onClick={(e) => {
                                        e.stopPropagation();
                                        handleFileDoubleClick(file.path, rev);
                                      }}
                                      className="inline-flex items-center px-1.5 py-0.5 rounded bg-brand-50 hover:bg-brand-600 text-brand-700 hover:text-white text-[10px] font-mono font-bold transition-colors cursor-pointer"
                                      title={`点击查看 r${rev} 的变更`}
                                    >
                                      r{rev}
                                    </button>
                                  ))}
                                </div>
                              </td>
                            </tr>
                          );
                        })}
                      </tbody>
                    </table>
                    {packageFiles.length === 0 && (
                      <div className="flex flex-col items-center justify-center py-16 text-slate-400">
                        <Package size={32} className="mb-3 text-slate-300" />
                        <p className="text-sm text-slate-500 mb-1">没有待打包文件</p>
                        <p className="text-xs">请返回选择提交记录</p>
                      </div>
                    )}
                    {packageFiles.length > 0 && filteredPackageFiles.length === 0 && (
                      <div className="flex flex-col items-center justify-center py-16 text-slate-400">
                        <Search size={32} className="mb-3 text-slate-300" />
                        <p className="text-sm text-slate-500 mb-1">没有匹配的文件</p>
                        <p className="text-xs">请调整搜索关键词</p>
                      </div>
                    )}
                  </div>
                </div>
              </div>

              {/* Console — fixed at bottom of interface */}
              <div className="shrink-0 border-t border-slate-200 bg-slate-50">
                <div className="flex items-center justify-between px-5 py-2 border-b border-slate-200 bg-white">
                  <div className="flex items-center gap-2">
                    <div className="w-1 h-3.5 bg-brand-500 rounded-full" />
                    <Terminal size={12} className="text-slate-500" />
                    <span className="text-[10px] font-semibold text-slate-600 uppercase tracking-wider">系统控制台</span>
                  </div>
                  <div className="flex items-center gap-3">
                    <button
                      onClick={() => setConsoleLogs([])}
                      className="text-[10px] text-slate-400 hover:text-slate-600 transition-colors"
                    >
                      清空
                    </button>
                    <button
                      onClick={() => navigator.clipboard.writeText(consoleLogs.join("\n"))}
                      className="text-[10px] text-slate-400 hover:text-slate-600 transition-colors"
                    >
                      复制
                    </button>
                  </div>
                </div>
                <div
                  ref={consoleRef}
                  className="h-36 overflow-auto px-5 py-2.5 font-mono text-[11px] leading-relaxed space-y-0.5"
                >
                  {consoleLogs.length === 0 ? (
                    <span className="text-slate-400 italic">等待打包...</span>
                  ) : (
                    consoleLogs.map((log, idx) => (
                      <div
                        key={idx}
                        className={`break-all ${
                          log.startsWith("✓")
                            ? "text-green-600"
                            : log.startsWith("✗")
                            ? "text-red-600"
                            : log.includes("开始")
                            ? "text-brand-600"
                            : "text-slate-600"
                        }`}
                      >
                        {log}
                      </div>
                    ))
                  )}
                </div>
              </div>
            </>
          )}
        </div>
      </main>

      {/* Diff Viewer Modal */}
      {diffState && (
        <DiffViewerModal
          filePath={diffState.filePath}
          diffText={diffState.diff}
          loading={diffState.loading}
          error={diffState.error}
          onClose={() => setDiffState(null)}
        />
      )}

      {/* Project Modal */}
      {showProjectModal && (
        <ProjectModal
          project={editingProject}
          onSave={handleSaveProject}
          onClose={() => { setShowProjectModal(false); setEditingProject(null); }}
        />
      )}

      {/* Settings Modal */}
      {showSettingsModal && (
        <SettingsModal
          settings={settings}
          onSave={handleSaveSettings}
          onClose={() => setShowSettingsModal(false)}
        />
      )}

      {/* Floating Notification Toast */}
      {notification && (
        <div className="fixed top-4 right-4 z-[60] flex items-center gap-2 px-3 py-2 rounded-lg shadow-lg border text-xs font-medium bg-white/95 backdrop-blur-sm">
          <span
            className={`w-1.5 h-1.5 rounded-full ${
              notification.type === "success" ? "bg-green-500" : "bg-red-500"
            }`}
          />
          <span className={notification.type === "success" ? "text-green-700" : "text-red-700"}>
            {notification.message}
          </span>
          <button
            onClick={() => setNotification(null)}
            className="ml-1 p-0.5 rounded hover:bg-slate-100 text-slate-400 hover:text-slate-600 transition-colors"
          >
            <X size={12} />
          </button>
        </div>
      )}
    </div>
  );
}

// ==================== Diff Viewer Modal ====================

function DiffViewerModal({
  filePath,
  diffText,
  loading,
  error,
  onClose,
}: {
  filePath: string;
  diffText: string | null;
  loading: boolean;
  error: string | null;
  onClose: () => void;
}) {
  const hunks = useMemo(() => {
    if (!diffText) return [];
    return parseDiff(diffText);
  }, [diffText]);

  const scrollRef = useRef<HTMLDivElement>(null);
  const firstChangeRef = useRef<HTMLDivElement>(null);
  const [scrollInfo, setScrollInfo] = useState({ scrollTop: 0, scrollHeight: 1, clientHeight: 1 });
  const [isDraggingMarker, setIsDraggingMarker] = useState(false);

  // Flatten all lines for marker calculation
  const allLines = useMemo(() => hunks.flatMap((h) => h.lines), [hunks]);
  const totalLines = allLines.length;

  // Group consecutive change lines into regions for cleaner markers
  const changeRegions = useMemo(() => {
    type Region = { start: number; end: number; hasAdd: boolean; hasRemove: boolean };
    const regions: Region[] = [];
    let current: Region | null = null;

    allLines.forEach((line, idx) => {
      if (line.type === "add" || line.type === "remove") {
        if (!current) {
          current = { start: idx, end: idx, hasAdd: line.type === "add", hasRemove: line.type === "remove" };
        } else if (idx - current.end <= 2) {
          current.end = idx;
          if (line.type === "add") current.hasAdd = true;
          if (line.type === "remove") current.hasRemove = true;
        } else {
          regions.push(current);
          current = { start: idx, end: idx, hasAdd: line.type === "add", hasRemove: line.type === "remove" };
        }
      }
    });
    if (current) regions.push(current);
    return regions;
  }, [allLines]);

  const handleScroll = useCallback(() => {
    if (scrollRef.current) {
      const { scrollTop, scrollHeight, clientHeight } = scrollRef.current;
      setScrollInfo({ scrollTop, scrollHeight: Math.max(scrollHeight, 1), clientHeight: Math.max(clientHeight, 1) });
    }
  }, []);

  // Update scroll info after content loads
  useEffect(() => {
    handleScroll();
  }, [diffText, handleScroll]);

  // Auto-scroll to first change region when diff loads
  useEffect(() => {
    if (loading || error || changeRegions.length === 0) return;
    const raf = requestAnimationFrame(() => {
      if (firstChangeRef.current && scrollRef.current) {
        const container = scrollRef.current;
        const target = firstChangeRef.current;
        // Scroll so the first change line sits ~120px from the top (some context above)
        const offset = target.offsetTop - 120;
        container.scrollTo({ top: Math.max(0, offset), behavior: "auto" });
        handleScroll();
      }
    });
    return () => cancelAnimationFrame(raf);
  }, [loading, error, changeRegions.length, handleScroll]);

  // Viewport indicator position
  const viewportTop = (scrollInfo.scrollTop / scrollInfo.scrollHeight) * 100;
  const viewportHeight = (scrollInfo.clientHeight / scrollInfo.scrollHeight) * 100;

  // Click-to-jump on marker bar
  const handleMarkerClick = (e: React.MouseEvent<HTMLDivElement>) => {
    if (!scrollRef.current || totalLines === 0) return;
    const rect = e.currentTarget.getBoundingClientRect();
    const clickRatio = (e.clientY - rect.top) / rect.height;
    const targetScroll = clickRatio * scrollInfo.scrollHeight - scrollInfo.clientHeight / 2;
    scrollRef.current.scrollTo({ top: Math.max(0, targetScroll), behavior: "smooth" });
  };

  // Drag marker bar to scroll
  useEffect(() => {
    if (!isDraggingMarker || !scrollRef.current) return;

    const handleMove = (e: MouseEvent) => {
      if (!scrollRef.current) return;
      const markerBar = document.getElementById("diff-marker-bar");
      if (!markerBar) return;
      const rect = markerBar.getBoundingClientRect();
      const ratio = Math.min(Math.max((e.clientY - rect.top) / rect.height, 0), 1);
      scrollRef.current.scrollTop = ratio * (scrollInfo.scrollHeight - scrollInfo.clientHeight);
    };

    const handleUp = () => setIsDraggingMarker(false);

    window.addEventListener("mousemove", handleMove);
    window.addEventListener("mouseup", handleUp);
    return () => {
      window.removeEventListener("mousemove", handleMove);
      window.removeEventListener("mouseup", handleUp);
    };
  }, [isDraggingMarker, scrollInfo.scrollHeight, scrollInfo.clientHeight]);

  const fileName = filePath.split("/").pop() || filePath;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 backdrop-blur-sm">
      <div className="bg-white rounded-xl border border-slate-200 shadow-2xl w-[90vw] max-w-[1200px] h-[85vh] flex flex-col overflow-hidden">
        {/* Header */}
        <div className="flex items-center justify-between px-5 py-3.5 border-b border-slate-100 shrink-0">
          <div className="flex items-center gap-2 min-w-0">
            <div className="w-1 h-5 bg-brand-500 rounded-full shrink-0" />
            <div className="min-w-0">
              <h3 className="text-sm font-semibold text-slate-800 truncate">{fileName}</h3>
              <p className="text-[10px] text-slate-400 font-mono truncate" title={filePath}>{filePath}</p>
            </div>
          </div>
          <div className="flex items-center gap-2 shrink-0">
            {diffText && (
              <button
                onClick={() => navigator.clipboard.writeText(diffText)}
                className="flex items-center gap-1 px-2.5 py-1.5 rounded-md bg-slate-50 hover:bg-slate-100 border border-slate-200 text-xs text-slate-600 transition-colors"
              >
                <Copy size={12} /> 复制全部
              </button>
            )}
            <button onClick={onClose} className="p-1 rounded hover:bg-slate-100 text-slate-400 hover:text-slate-600">
              <X size={16} />
            </button>
          </div>
        </div>

        {/* Content area with scrollbar markers */}
        <div className="flex-1 relative min-h-0">
          {/* Scrollable content */}
          <div
            ref={scrollRef}
            onScroll={handleScroll}
            className="absolute inset-0 overflow-auto bg-slate-50 [&::-webkit-scrollbar]:hidden"
            style={{ scrollbarWidth: "none", msOverflowStyle: "none" }}
          >
            {loading && (
              <div className="flex items-center justify-center h-full">
                <Loader2 size={24} className="animate-spin text-brand-500" />
                <span className="ml-2 text-sm text-slate-500">加载变更详情...</span>
              </div>
            )}
            {error && (
              <div className="flex items-center justify-center h-full">
                <div className="text-center">
                  <X size={32} className="mx-auto mb-2 text-red-400" />
                  <p className="text-sm text-red-600">{error}</p>
                </div>
              </div>
            )}
            {diffText && hunks.length === 0 && !loading && !error && (
              <div className="flex items-center justify-center h-full">
                <p className="text-sm text-slate-400">没有文本变更（可能是二进制文件）</p>
              </div>
            )}
            {hunks.length > 0 && (
              <div className="py-2 pr-7">
                {(() => {
                  let globalIdx = 0;
                  const firstChangeIdx = changeRegions.length > 0 ? changeRegions[0].start : -1;
                  return hunks.map((hunk, hi) => (
                    <div key={hi}>
                      {hunk.lines.map((line, li) => {
                        const idx = globalIdx++;
                        const isFirstChange = idx === firstChangeIdx;
                        return (
                          <div
                            key={li}
                            ref={isFirstChange ? firstChangeRef : undefined}
                            className={`flex text-xs font-mono ${
                              line.type === "add" ? "bg-green-50" : line.type === "remove" ? "bg-red-50" : ""
                            }`}
                          >
                        {/* Old line number */}
                        <div className="shrink-0 w-12 text-right pr-2 py-0.5 text-slate-300 select-none border-r border-slate-100">
                          {line.oldNum ?? ""}
                        </div>
                        {/* New line number */}
                        <div className="shrink-0 w-12 text-right pr-2 py-0.5 text-slate-300 select-none border-r border-slate-100">
                          {line.newNum ?? ""}
                        </div>
                        {/* Symbol */}
                        <div
                          className={`shrink-0 w-5 text-center py-0.5 select-none ${
                            line.type === "add" ? "text-green-600" : line.type === "remove" ? "text-red-600" : "text-slate-300"
                          }`}
                        >
                          {line.type === "add" ? "+" : line.type === "remove" ? "-" : " "}
                        </div>
                        {/* Content */}
                        <div className="flex-1 min-w-0 pl-2 py-0.5 whitespace-pre-wrap break-all">
                          <span
                            className={
                              line.type === "add"
                                ? "text-green-800"
                                : line.type === "remove"
                                ? "text-red-800"
                                : "text-slate-600"
                            }
                          >
                            {line.content || " "}
                          </span>
                        </div>
                          </div>
                        );
                      })}
                    </div>
                  ));
                })()}
              </div>
            )}
          </div>

          {/* Scrollbar change markers overlay */}
          {totalLines > 0 && !loading && !error && (
            <div
              id="diff-marker-bar"
              className="absolute right-0 top-0 bottom-0 w-5 z-20 cursor-pointer hover:w-6 transition-all"
              onClick={handleMarkerClick}
              onMouseDown={(e) => {
                setIsDraggingMarker(true);
                handleMarkerClick(e);
              }}
              title="拖拽或点击跳转"
            >
              {/* Background track */}
              <div className="absolute inset-0 bg-slate-100/80 border-l border-slate-200" />

              {/* Viewport indicator (draggable handle) */}
              <div
                className="absolute left-0.5 right-0.5 bg-slate-300/70 rounded-sm border border-slate-400/30 hover:bg-slate-400/70 transition-colors"
                style={{
                  top: `${viewportTop}%`,
                  height: `${Math.max(viewportHeight, 3)}%`,
                  minHeight: "16px",
                }}
                onMouseDown={(e) => {
                  e.stopPropagation();
                  setIsDraggingMarker(true);
                }}
              />

              {/* Change region markers */}
              {changeRegions.map((region, i) => {
                const top = (region.start / totalLines) * 100;
                const height = Math.max(((region.end - region.start + 1) / totalLines) * 100, 0.8);
                const bgColor = region.hasAdd && region.hasRemove
                  ? "bg-amber-500"
                  : region.hasAdd
                  ? "bg-green-500"
                  : "bg-red-500";
                return (
                  <div
                    key={i}
                    className={`absolute left-1 right-1 rounded-sm ${bgColor} pointer-events-none`}
                    style={{
                      top: `${top}%`,
                      height: `${height}%`,
                      minHeight: "2px",
                    }}
                  />
                );
              })}
            </div>
          )}
        </div>

        {/* Footer legend */}
        {totalLines > 0 && !loading && !error && (
          <div className="shrink-0 flex items-center justify-end gap-4 px-5 py-2 border-t border-slate-100 bg-white text-[10px] text-slate-400">
            <span className="flex items-center gap-1.5">
              <span className="w-2 h-2 rounded-sm bg-green-500" /> 新增
            </span>
            <span className="flex items-center gap-1.5">
              <span className="w-2 h-2 rounded-sm bg-red-500" /> 删除
            </span>
            <span className="flex items-center gap-1.5">
              <span className="w-2 h-2 rounded-sm bg-amber-500" /> 混合
            </span>
            <span className="text-slate-300">|</span>
            <span>{changeRegions.length} 处变更</span>
          </div>
        )}
      </div>
    </div>
  );
}

// ==================== Project Modal ====================

function ProjectModal({
  project,
  onSave,
  onClose,
}: {
  project: SvnProject | null;
  onSave: (p: SvnProject) => void;
  onClose: () => void;
}) {
  const [form, setForm] = useState<SvnProject>(
    project || { id: generateId(), name: "", svn_url: "", local_path: "", username: "", password: "" }
  );
  const [testStatus, setTestStatus] = useState<"idle" | "loading" | "success" | "fail" | "error">("idle");
  const [testError, setTestError] = useState("");

  async function testConnection() {
    if (!form.svn_url) return;
    setTestStatus("loading");
    try {
      const ok = await invoke<boolean>("test_svn_connection", {
        url: form.svn_url,
        username: form.username,
        password: form.password,
      });
      setTestStatus(ok ? "success" : "fail");
    } catch (e) {
      setTestStatus("error");
      setTestError(String(e));
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 backdrop-blur-sm">
      <div className="bg-white rounded-xl border border-slate-200 shadow-2xl w-[480px] max-w-[90vw] overflow-hidden">
        <div className="flex items-center justify-between px-5 py-4 border-b border-slate-100">
          <div className="flex items-center gap-2">
            <div className="w-1 h-5 bg-brand-500 rounded-full" />
            <h3 className="text-sm font-semibold text-slate-800">{project ? "编辑项目" : "新增项目"}</h3>
          </div>
          <button onClick={onClose} className="p-1 rounded hover:bg-slate-100 text-slate-400 hover:text-slate-600">
            <X size={16} />
          </button>
        </div>
        <div className="p-5 space-y-4">
          <FormField label="项目名称">
            <input
              value={form.name}
              onChange={(e) => setForm({ ...form, name: e.target.value })}
              placeholder="例如：myapp-backend"
              className="w-full px-3 py-2 rounded-lg border border-slate-200 bg-white text-xs text-slate-700 focus:outline-none focus:ring-2 focus:ring-brand-500/20 focus:border-brand-500"
            />
          </FormField>
          <FormField label="SVN 仓库地址">
            <input
              value={form.svn_url}
              onChange={(e) => setForm({ ...form, svn_url: e.target.value })}
              placeholder="https://svn.company.com/repo/project"
              className="w-full px-3 py-2 rounded-lg border border-slate-200 bg-white text-xs text-slate-700 focus:outline-none focus:ring-2 focus:ring-brand-500/20 focus:border-brand-500"
            />
          </FormField>
          <FormField label="本地项目路径">
            <input
              value={form.local_path}
              onChange={(e) => setForm({ ...form, local_path: e.target.value })}
              placeholder="D:\\\\workspace\\\\myapp"
              className="w-full px-3 py-2 rounded-lg border border-slate-200 bg-white text-xs text-slate-700 focus:outline-none focus:ring-2 focus:ring-brand-500/20 focus:border-brand-500"
            />
          </FormField>
          <div className="grid grid-cols-2 gap-3">
            <FormField label="SVN 用户名">
              <input
                value={form.username}
                onChange={(e) => setForm({ ...form, username: e.target.value })}
                placeholder="可选"
                className="w-full px-3 py-2 rounded-lg border border-slate-200 bg-white text-xs text-slate-700 focus:outline-none focus:ring-2 focus:ring-brand-500/20 focus:border-brand-500"
              />
            </FormField>
            <FormField label="SVN 密码">
              <input
                type="password"
                value={form.password}
                onChange={(e) => setForm({ ...form, password: e.target.value })}
                placeholder="可选"
                className="w-full px-3 py-2 rounded-lg border border-slate-200 bg-white text-xs text-slate-700 focus:outline-none focus:ring-2 focus:ring-brand-500/20 focus:border-brand-500"
              />
            </FormField>
          </div>
        </div>
        <div className="flex items-center justify-between gap-2 px-5 py-4 border-t border-slate-100 bg-slate-50/50">
          <div className="flex items-center gap-2">
            {testStatus === "loading" && (
              <span className="flex items-center gap-1 text-xs text-brand-600">
                <Loader2 size={12} className="animate-spin" /> 正在测试...
              </span>
            )}
            {testStatus === "success" && (
              <span className="flex items-center gap-1 text-xs text-green-600 font-medium">
                <CheckCircle2 size={12} /> 连接成功
              </span>
            )}
            {testStatus === "fail" && (
              <span className="flex items-center gap-1 text-xs text-red-600 font-medium">
                <X size={12} /> 连接失败
              </span>
            )}
            {testStatus === "error" && (
              <span className="flex items-center gap-1 text-xs text-red-600 font-medium" title={testError}>
                <X size={12} /> 连接异常
              </span>
            )}
          </div>
          <div className="flex items-center gap-2">
            <button
              onClick={onClose}
              className="px-4 py-2 rounded-lg border border-slate-200 bg-white text-xs font-medium text-slate-600 hover:bg-slate-50 transition-colors"
            >
              取消
            </button>
            <button
              onClick={testConnection}
              disabled={testStatus === "loading"}
              className="px-4 py-2 rounded-lg border border-brand-200 bg-brand-50 text-xs font-medium text-brand-700 hover:bg-brand-100 transition-colors disabled:opacity-50"
            >
              测试连接
            </button>
            <button
              onClick={() => onSave(form)}
              className="px-4 py-2 rounded-lg bg-brand-600 text-white text-xs font-medium hover:bg-brand-700 transition-colors"
            >
              保存项目
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

// ==================== Settings Modal ====================

function SettingsModal({
  settings,
  onSave,
  onClose,
}: {
  settings: AppSettings;
  onSave: (s: AppSettings) => void;
  onClose: () => void;
}) {
  const [form, setForm] = useState(settings);
  const [outputDirError, setOutputDirError] = useState("");

  function handleSave() {
    if (!form.output_dir.trim()) {
      setOutputDirError("默认输出目录不能为空");
      return;
    }
    onSave(form);
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 backdrop-blur-sm">
      <div className="bg-white rounded-xl border border-slate-200 shadow-2xl w-[440px] max-w-[90vw] overflow-hidden">
        <div className="flex items-center justify-between px-5 py-4 border-b border-slate-100">
          <div className="flex items-center gap-2">
            <div className="w-1 h-5 bg-brand-500 rounded-full" />
            <h3 className="text-sm font-semibold text-slate-800">系统设置</h3>
          </div>
          <button onClick={onClose} className="p-1 rounded hover:bg-slate-100 text-slate-400 hover:text-slate-600">
            <X size={16} />
          </button>
        </div>
        <div className="p-5 space-y-4">
          <FormField label="默认输出目录" required>
            <input
              value={form.output_dir}
              onChange={(e) => {
                setForm({ ...form, output_dir: e.target.value });
                setOutputDirError("");
              }}
              placeholder="例如：D:\\output"
              className={`w-full px-3 py-2 rounded-lg border bg-white text-xs text-slate-700 focus:outline-none focus:ring-2 focus:ring-brand-500/20 focus:border-brand-500 ${
                outputDirError ? "border-red-400" : "border-slate-200"
              }`}
            />
            {outputDirError && (
              <p className="mt-1 text-[10px] text-red-500">{outputDirError}</p>
            )}
          </FormField>
          <FormField label="自动排除文件（每行一个）">
            <textarea
              rows={5}
              value={form.excludes.join("\n")}
              onChange={(e) => setForm({ ...form, excludes: e.target.value.split("\n").filter((s) => s.trim()) })}
              className="w-full px-3 py-2 rounded-lg border border-slate-200 bg-white text-xs text-slate-700 font-mono focus:outline-none focus:ring-2 focus:ring-brand-500/20 focus:border-brand-500"
            />
          </FormField>
        </div>
        <div className="flex items-center justify-end gap-2 px-5 py-4 border-t border-slate-100 bg-slate-50/50">
          <button
            onClick={onClose}
            className="px-4 py-2 rounded-lg border border-slate-200 bg-white text-xs font-medium text-slate-600 hover:bg-slate-50 transition-colors"
          >
            取消
          </button>
          <button
            onClick={handleSave}
            className="px-4 py-2 rounded-lg bg-brand-600 text-white text-xs font-medium hover:bg-brand-700 transition-colors"
          >
            保存设置
          </button>
        </div>
      </div>
    </div>
  );
}

// ==================== Form Field ====================

function FormField({ label, required, children }: { label: string; required?: boolean; children: React.ReactNode }) {
  return (
    <div>
      <label className="block text-[11px] font-semibold text-slate-500 uppercase tracking-wider mb-1.5">
        {label}{required && <span className="text-red-500 ml-0.5">*</span>}
      </label>
      {children}
    </div>
  );
}
