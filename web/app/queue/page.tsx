"use client";

import { useState, useEffect, useCallback, useRef } from "react";
import {
  getQueue,
  addToQueue,
  cancelQueue,
  reorderQueue,
  getCategories,
  QueueEntry,
  Category,
} from "@/lib/api";
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
  const [expandedId, setExpandedId] = useState<number | null>(null);
  const [dragId, setDragId] = useState<number | null>(null);
  const [dragOverId, setDragOverId] = useState<number | null>(null);

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

      if (update.output) {
        setLogs((prev) => {
          const current = prev[update.queue_id] || [];
          const updated = [...current, update.output!].slice(-MAX_LOG_LINES);
          return { ...prev, [update.queue_id]: updated };
        });
      }

      if (update.status === "completed" || update.status === "failed") {
        setTimeout(refresh, 500);
      }
    });
  }, [refresh]);

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
      }
      setError(msg);
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

  // Drag and drop
  const handleDragStart = (id: number) => {
    setDragId(id);
  };

  const handleDragOver = (e: React.DragEvent, id: number) => {
    e.preventDefault();
    if (dragId !== null && dragId !== id) {
      setDragOverId(id);
    }
  };

  const handleDragLeave = () => {
    setDragOverId(null);
  };

  const handleDrop = async (e: React.DragEvent, targetId: number) => {
    e.preventDefault();
    setDragOverId(null);

    if (dragId === null || dragId === targetId) {
      setDragId(null);
      return;
    }

    const newEntries = [...entries];
    const dragIdx = newEntries.findIndex((e) => e.id === dragId);
    const targetIdx = newEntries.findIndex((e) => e.id === targetId);
    if (dragIdx === -1 || targetIdx === -1) return;

    const [moved] = newEntries.splice(dragIdx, 1);
    newEntries.splice(targetIdx, 0, moved);
    setEntries(newEntries);
    setDragId(null);

    const order = newEntries.map((e) => e.id);
    try {
      await reorderQueue(order);
    } catch {
      await refresh();
    }
  };

  const handleDragEnd = () => {
    setDragId(null);
    setDragOverId(null);
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
      <div className="space-y-1">
        {entries.length === 0 ? (
          <p className="text-muted-foreground text-sm py-8 text-center">
            Queue is empty. Paste a URL above to start a download.
          </p>
        ) : (
          entries.map((entry) => {
            const isExpanded = expandedId === entry.id;
            const isDragging = dragId === entry.id;
            const isDragOver = dragOverId === entry.id;
            const canDrag = entry.status === "pending";
            const logLines = logs[entry.id] || [];

            return (
              <div
                key={entry.id}
                draggable={canDrag}
                onDragStart={() => handleDragStart(entry.id)}
                onDragOver={(e) => handleDragOver(e, entry.id)}
                onDragLeave={handleDragLeave}
                onDrop={(e) => handleDrop(e, entry.id)}
                onDragEnd={handleDragEnd}
                className={`rounded-lg border bg-zinc-900 transition-all ${
                  isDragOver
                    ? "border-accent border-2"
                    : "border-border"
                } ${isDragging ? "opacity-40" : ""}`}
              >
                {/* Compact row */}
                <div
                  className="flex items-center gap-3 px-4 py-3 cursor-pointer"
                  onClick={() => setExpandedId(isExpanded ? null : entry.id)}
                >
                  {/* Drag handle */}
                  {canDrag ? (
                    <span
                      className="text-zinc-600 hover:text-zinc-400 cursor-grab active:cursor-grabbing select-none"
                      title="Drag to reorder"
                    >
                      &#x2807;
                    </span>
                  ) : (
                    <span className="w-3" />
                  )}

                  {/* Status badge */}
                  <span
                    className={`shrink-0 inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium ${STATUS_COLORS[entry.status] || ""}`}
                  >
                    {entry.status}
                  </span>

                  {/* Progress % */}
                  {entry.status === "downloading" && entry.progress && (
                    <span className="shrink-0 text-xs font-mono text-accent w-12 text-right">
                      {entry.progress}
                    </span>
                  )}

                  {/* URL */}
                  <span
                    className="flex-1 min-w-0 text-sm font-mono truncate text-zinc-300"
                    title={entry.url}
                  >
                    {entry.url}
                  </span>

                  {/* Expand indicator */}
                  <span className="text-xs text-zinc-600 shrink-0">
                    {isExpanded ? "\u25BE" : "\u25B8"}
                  </span>
                </div>

                {/* Progress bar (always visible when downloading) */}
                {entry.status === "downloading" && entry.progress && (
                  <div className="w-full bg-zinc-800 h-0.5">
                    <div
                      className="bg-accent h-0.5 transition-all duration-300"
                      style={{
                        width: entry.progress.replace("%", "") + "%",
                      }}
                    />
                  </div>
                )}

                {/* Expanded detail */}
                {isExpanded && (
                  <div className="px-4 pb-4 pt-2 space-y-3 border-t border-zinc-800">
                    <p className="text-xs font-mono text-zinc-400 break-all">
                      {entry.url}
                    </p>

                    <div className="flex items-center gap-4 text-xs text-muted-foreground">
                      <span>Queued: {entry.created_at}</span>
                      {entry.error && (
                        <span className="text-destructive">{entry.error}</span>
                      )}
                    </div>

                    <div className="flex gap-2">
                      {entry.status === "pending" && (
                        <button
                          onClick={(e) => {
                            e.stopPropagation();
                            handleCancel(entry.id);
                          }}
                          className="rounded-md border border-border px-3 py-1 text-xs hover:bg-zinc-800 transition-colors"
                        >
                          Cancel
                        </button>
                      )}
                      {entry.status === "failed" && (
                        <button
                          onClick={(e) => {
                            e.stopPropagation();
                            handleRetry(entry);
                          }}
                          className="rounded-md border border-border px-3 py-1 text-xs hover:bg-zinc-800 transition-colors"
                        >
                          Retry
                        </button>
                      )}
                    </div>

                    {logLines.length > 0 && <LogPanel lines={logLines} />}
                  </div>
                )}
              </div>
            );
          })
        )}
      </div>
    </div>
  );
}

function LogPanel({ lines }: { lines: string[] }) {
  const ref = useRef<HTMLPreElement>(null);

  useEffect(() => {
    if (ref.current) {
      ref.current.scrollTop = ref.current.scrollHeight;
    }
  }, [lines.length]);

  return (
    <pre
      ref={ref}
      className="max-h-48 overflow-y-auto rounded-md bg-zinc-950 border border-zinc-800 p-3 text-xs font-mono text-zinc-400 leading-relaxed"
    >
      {lines.map((line, i) => (
        <div key={i}>{line}</div>
      ))}
    </pre>
  );
}
