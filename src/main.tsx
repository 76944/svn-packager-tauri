import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./index.css";

// 全局错误边界：渲染异常时显示可恢复的错误页，避免直接白屏
class ErrorBoundary extends React.Component<
  { children: React.ReactNode },
  { error: Error | null }
> {
  state = { error: null as Error | null };

  static getDerivedStateFromError(error: Error) {
    return { error };
  }

  render() {
    if (this.state.error) {
      return (
        <div className="flex h-screen flex-col items-center justify-center gap-3 bg-slate-50 text-slate-700 dark:bg-graphite-800 dark:text-slate-200">
          <h1 className="text-sm font-semibold">应用发生渲染异常</h1>
          <p className="max-w-[480px] break-all px-6 text-center text-xs text-slate-400">
            {String(this.state.error)}
          </p>
          <button
            onClick={() => window.location.reload()}
            className="rounded-lg bg-red-500 px-4 py-2 text-xs font-medium text-white hover:bg-red-700 transition-colors"
          >
            重新加载
          </button>
        </div>
      );
    }
    return this.props.children;
  }
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <ErrorBoundary>
      <App />
    </ErrorBoundary>
  </React.StrictMode>
);
