"use client";

import { useState, useEffect, useCallback, useRef } from "react";
import { getQueue, addToQueue, cancelQueue, getCategories, QueueEntry, Category } from "@/lib/api";
import { subscribeProgress, ProgressUpdate } from "@/lib/ws";

const STATUS_COLORS: Record<string, string> = {
  pending: "bg-yellow-500/20 text-yellow-400",
  downloading: "bg-blue-500/20 text-blue-400",
  completed: "bg-green-500/20 text-green-400",
  failed: "bg-red-500/20 text-red-400",
};

const MAX_LOG_LINES = 200;

export default function QueuePage() {
  const [entries, setEntries] = useState<QueueEntry[]>([]);
  const [url, setUrl] = useState("");
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  const [category, setCategory] = useState("");
  const [categories, setCategories] = useState<Category[]>([]);
  const [duplicateUrl, setDuplicateUrl] = useState("");
  const [logs, setLogs] = useState<Record<number, string[]>>({});
  const [expandedLogs, setExpandedLogs] = useState<Set<number>>(new Set());

  const refresh = useCallback(async () => {
    try {
      const data = await getQueue();
      setEntries(data);
    } catch {
      // silently fail on refresh
    }
  }, []);

  useEffect(() => {
    refresh();
    getCategories().then(setCategories).catch(() => {});
    const interval = setInterval(refresh, 5000);
    return () => clearInterval(interval);
  }, [refresh]);

  // WebSocket for real-time progress + output
  useEffect(() => {
    return subscribeProgress((update: ProgressUpdate) => {
      setEntries((prev) =>
        prev.map((e) =>
          e.id === update.queue_id
            ? {
                ...e,
                progress: update.progress || e.progress,
                status: update.status as QueueEntry["status"],
              }
            : e
        )
      );

      // Append output line to log
      if (update.output) {
        setLogs((prev) => {
          const current = prev[update.queue_id] || [];
          const updated = [...current, update.output!].slice(-MAX_LOG_LINES);
          return { ...prev, [update.queue_id]: updated };
        });
        // Auto-expand log when download starts
        setExpandedLogs((prev) => {
          if (update.status === "downloading" && !prev.has(update.queue_id)) {
            const next = new Set(prev);
            next.add(update.queue_id);
            return next;
          }
          return prev;
        });
      }

      // Refresh full list when a download completes or fails
      if (update.status === "completed" || update.status === "failed") {
        setTimeout(refresh, 500);
      }
    });
  }, [refresh]);

  const toggleLog = (id: number) => {
    setExpandedLogs((prev) => {
      const next = new Set(prev);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  };

  const handleSubmit = async (e: React.FormEvent, force: boolean = false) => {
    e.preventDefault();
    if (!url.trim()) return;
    setError("");
    setDuplicateUrl("");
    setLoading(true);
    try {
      await addToQueue(url.trim(), force, category || undefined);
      setUrl("");
      await refresh();
    } catch (err) {
      const msg = err instanceof Error ? err.message : "Failed to queue URL";
      if (msg.includes("already queued")) {
        setDuplicateUrl(url.trim());
        setError(msg);
      } else {
        setError(msg);
      }
    } finally {
      setLoading(false);
    }
  };

  const handleForceDownload = async (e: React.FormEvent) => {
    await handleSubmit(e, true);
    setDuplicateUrl("");
  };

  const handleCancel = async (id: number) => {
    try {
      await cancelQueue(id);
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to cancel");
    }
  };

  const handleRetry = async (entry: QueueEntry) => {
    try {
      await addToQueue(entry.url);
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to retry");
    }
  };

  return (
    <div className="space-y-6">
      <h1 className="text-2xl font-bold">Download Queue</h1>

      {/* URL Input */}
      <form onSubmit={handleSubmit} className="flex gap-3">
        <input
          type="url"
          value={url}
          onChange={(e) => setUrl(e.target.value)}
          placeholder="Paste a URL to download..."
          className="flex-1 min-w-0 rounded-lg bg-zinc-900 border border-border px-4 py-2.5 text-sm placeholder:text-muted-foreground focus:outline-none focus:ring-2 focus:ring-accent"
        />
        <select
          value={category}
          onChange={(e) => setCategory(e.target.value)}
          className="rounded-lg bg-zinc-900 border border-border px-3 py-2.5 text-sm focus:outline-none focus:ring-2 focus:ring-accent"
        >
          <option value="">Uncategorized</option>
          {categories.map((cat) => (
            <option key={cat.id} value={cat.name}>
              {cat.name}
            </option>
          ))}
        </select>
        <button
          type="submit"
          disabled={loading || !url.trim()}
          className="rounded-lg bg-accent px-6 py-2.5 text-sm font-medium text-white hover:bg-blue-600 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
        >
          {loading ? "Queuing..." : "Queue"}
        </button>
      </form>

      {error && (
        <div className="text-sm text-destructive bg-red-500/10 rounded-lg px-4 py-2 flex items-center justify-between gap-3">
          <span>{error}</span>
          {duplicateUrl && (
            <button
              onClick={handleForceDownload}
              className="shrink-0 rounded-md bg-zinc-800 border border-border px-3 py-1 text-xs text-zinc-200 hover:bg-zinc-700 transition-colors"
            >
              Force re-download
            </button>
          )}
        </div>
      )}

      {/* Queue List */}
      <div className="space-y-3">
        {entries.length === 0 ? (
          <p className="text-muted-foreground text-sm py-8 text-center">
            Queue is empty. Paste a URL above to start a download.
          </p>
        ) : (
          entries.map((entry) => (
            <div
              key={entry.id}
              className="rounded-lg border border-border bg-zinc-900 p-4 space-y-2"
            >
              <div className="flex items-start justify-between gap-4">
                <div className="min-w-0 flex-1">
                  <p className="text-sm font-mono truncate" title={entry.url}>
                    {entry.url}
                  </p>
                  <div className="flex items-center gap-2 mt-1.5">
                    <span
                      className={`inline-flex items-center rounded-full px-2.5 py-0.5 text-xs font-medium ${STATUS_COLORS[entry.status] || ""}`}
                    >
                      {entry.status}
                    </span>
                    {entry.progress && entry.status === "downloading" && (
                      <span className="text-xs text-muted-foreground">
                        {entry.progress}
                      </span>
                    )}
                    {entry.error && (
                      <span className="text-xs text-destructive">{entry.error}</span>
                    )}
                    {/* Log toggle */}
                    {(logs[entry.id]?.length ?? 0) > 0 && (
                      <button
                        onClick={() => toggleLog(entry.id)}
                        className="text-xs text-muted-foreground hover:text-white transition-colors ml-1"
                      >
                        {expandedLogs.has(entry.id) ? "Hide" : "Show"} log ({logs[entry.id].length})
                      </button>
                    )}
                  </div>
                </div>
                <div className="flex gap-2 shrink-0">
                  {entry.status === "pending" && (
                    <button
                      onClick={() => handleCancel(entry.id)}
                      className="rounded-md border border-border px-3 py-1 text-xs hover:bg-zinc-800 transition-colors"
                    >
                      Cancel
                    </button>
                  )}
                  {entry.status === "failed" && (
                    <button
                      onClick={() => handleRetry(entry)}
                      className="rounded-md border border-border px-3 py-1 text-xs hover:bg-zinc-800 transition-colors"
                    >
                      Retry
                    </button>
                  )}
                </div>
              </div>

              {/* Progress bar */}
              {entry.status === "downloading" && entry.progress && (
                <div className="w-full bg-zinc-800 rounded-full h-1.5">
                  <div
                    className="bg-accent h-1.5 rounded-full transition-all duration-300"
                    style={{
                      width: entry.progress.replace("%", "") + "%",
                    }}
                  />
                </div>
              )}

              {/* Console output log */}
              {expandedLogs.has(entry.id) && logs[entry.id]?.length > 0 && (
                <LogPanel lines={logs[entry.id]} />
              )}
            </div>
          ))
        )}
      </div>
    </div>
  );
}

function LogPanel({ lines }: { lines: string[] }) {
  const ref = useRef<HTMLPreElement>(null);

  useEffect(() => {
    // Auto-scroll to bottom on new lines
    if (ref.current) {
      ref.current.scrollTop = ref.current.scrollHeight;
    }
  }, [lines.length]);

  return (
    <pre
      ref={ref}
      className="mt-2 max-h-48 overflow-y-auto rounded-md bg-zinc-950 border border-zinc-800 p-3 text-xs font-mono text-zinc-400 leading-relaxed"
    >
      {lines.map((line, i) => (
        <div key={i}>{line}</div>
      ))}
    </pre>
  );
}
