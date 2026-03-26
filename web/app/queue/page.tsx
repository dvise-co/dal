"use client";

import { useState, useEffect, useCallback, useRef } from "react";
import {
  getQueue,
  addToQueue,
  cancelQueue,
  reorderQueue,
  archiveQueue,
  unarchiveQueue,
  archiveAllQueue,
  getCategories,
  QueueEntry,
  Category,
} from "@/lib/api";
import { subscribeProgress, ProgressUpdate } from "@/lib/ws";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Checkbox } from "@/components/ui/checkbox";

type StatusFilter = "all" | "pending" | "downloading" | "completed" | "failed" | "archived";

const STATUS_VARIANTS: Record<string, "default" | "secondary" | "destructive" | "outline"> = {
  pending: "outline",
  downloading: "default",
  completed: "secondary",
  failed: "destructive",
};

export default function QueuePage() {
  const [allEntries, setAllEntries] = useState<QueueEntry[]>([]);
  const [url, setUrl] = useState("");
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  const [category, setCategory] = useState("");
  const [categories, setCategories] = useState<Category[]>([]);
  const [duplicateUrl, setDuplicateUrl] = useState("");
  const [expandedId, setExpandedId] = useState<number | null>(null);
  const [wsLogs, setWsLogs] = useState<Record<number, string[]>>({});
  const [statusFilter, setStatusFilter] = useState<StatusFilter>("all");
  const [selectedCategories, setSelectedCategories] = useState<Set<number | null>>(new Set());
  const [categoriesInitialized, setCategoriesInitialized] = useState(false);
  const [dragId, setDragId] = useState<number | null>(null);
  const [dragOverId, setDragOverId] = useState<number | null>(null);

  const refresh = useCallback(async () => {
    try {
      const data = await getQueue(
        statusFilter === "archived"
          ? { archived: true }
          : { include_archived: false }
      );
      setAllEntries(data);
    } catch {
      // silently fail
    }
  }, [statusFilter]);

  useEffect(() => {
    refresh();
    getCategories().then((cats) => {
      setCategories(cats);
      if (!categoriesInitialized) {
        const allIds = new Set<number | null>([null, ...cats.map((c) => c.id)]);
        setSelectedCategories(allIds);
        setCategoriesInitialized(true);
      }
    }).catch(() => {});
    const interval = setInterval(refresh, 10000);
    return () => clearInterval(interval);
  }, [refresh, categoriesInitialized]);

  // WebSocket for real-time updates
  useEffect(() => {
    return subscribeProgress((update: ProgressUpdate) => {
      setAllEntries((prev) =>
        prev.map((e) =>
          e.id === update.queue_id
            ? { ...e, progress: update.progress || e.progress, status: update.status as QueueEntry["status"] }
            : e
        )
      );
      if (update.output) {
        setWsLogs((prev) => {
          const current = prev[update.queue_id] || [];
          return { ...prev, [update.queue_id]: [...current, update.output!].slice(-200) };
        });
      }
      if (update.status === "completed" || update.status === "failed") {
        setTimeout(refresh, 500);
      }
    });
  }, [refresh]);

  // Filter entries
  const entries = allEntries.filter((e) => {
    if (statusFilter !== "all" && statusFilter !== "archived" && e.status !== statusFilter) return false;
    if (!selectedCategories.has(e.category_id)) return false;
    return true;
  });

  // Counts for badges
  const counts = allEntries.reduce(
    (acc, e) => {
      acc[e.status] = (acc[e.status] || 0) + 1;
      acc.all = (acc.all || 0) + 1;
      return acc;
    },
    {} as Record<string, number>
  );

  const categoryName = (catId: number | null) => {
    if (catId === null) return null;
    return categories.find((c) => c.id === catId)?.name || null;
  };

  const handleSubmit = async (e: React.FormEvent, force: boolean = false) => {
    e.preventDefault();
    if (!url.trim()) return;
    setError(""); setDuplicateUrl(""); setLoading(true);
    try {
      await addToQueue(url.trim(), force, category || undefined);
      setUrl("");
      await refresh();
    } catch (err) {
      const msg = err instanceof Error ? err.message : "Failed to queue URL";
      if (msg.includes("already queued")) setDuplicateUrl(url.trim());
      setError(msg);
    } finally {
      setLoading(false);
    }
  };

  const handleForceDownload = async (e: React.FormEvent) => { await handleSubmit(e, true); setDuplicateUrl(""); };
  const handleCancel = async (id: number) => { try { await cancelQueue(id); await refresh(); } catch (err) { setError(err instanceof Error ? err.message : "Failed"); } };
  const handleRetry = async (entry: QueueEntry) => { try { await addToQueue(entry.url, true); await refresh(); } catch (err) { setError(err instanceof Error ? err.message : "Failed"); } };
  const handleArchive = async (id: number) => { try { await archiveQueue(id); await refresh(); } catch {} };
  const handleUnarchive = async (id: number) => { try { await unarchiveQueue(id); await refresh(); } catch {} };
  const handleArchiveAll = async (status?: string) => { try { await archiveAllQueue(status); await refresh(); } catch {} };

  // Drag and drop
  const handleDragStart = (id: number) => setDragId(id);
  const handleDragOver = (e: React.DragEvent, id: number) => { e.preventDefault(); if (dragId !== null && dragId !== id) setDragOverId(id); };
  const handleDragLeave = () => setDragOverId(null);
  const handleDrop = async (e: React.DragEvent, targetId: number) => {
    e.preventDefault(); setDragOverId(null);
    if (dragId === null || dragId === targetId) { setDragId(null); return; }
    const newEntries = [...allEntries];
    const dragIdx = newEntries.findIndex((e) => e.id === dragId);
    const targetIdx = newEntries.findIndex((e) => e.id === targetId);
    if (dragIdx === -1 || targetIdx === -1) return;
    const [moved] = newEntries.splice(dragIdx, 1);
    newEntries.splice(targetIdx, 0, moved);
    setAllEntries(newEntries);
    setDragId(null);
    try { await reorderQueue(newEntries.map((e) => e.id)); } catch { await refresh(); }
  };
  const handleDragEnd = () => { setDragId(null); setDragOverId(null); };

  const toggleCategory = (catId: number | null) => {
    setSelectedCategories((prev) => {
      const next = new Set(prev);
      if (next.has(catId)) next.delete(catId); else next.add(catId);
      return next;
    });
  };

  return (
    <div className="space-y-4">
      <h1 className="text-xl font-bold">Download Queue</h1>

      {/* URL Input */}
      <form onSubmit={handleSubmit} className="flex gap-2">
        <Input
          type="url"
          value={url}
          onChange={(e) => setUrl(e.target.value)}
          placeholder="Paste a URL to download..."
          className="flex-1 min-w-0 h-9"
        />
        <select
          value={category}
          onChange={(e) => setCategory(e.target.value)}
          className="rounded-md bg-background border border-input px-2 h-9 text-sm"
        >
          <option value="">Uncategorized</option>
          {categories.map((cat) => (
            <option key={cat.id} value={cat.name}>{cat.name}</option>
          ))}
        </select>
        <Button type="submit" disabled={loading || !url.trim()} size="sm" className="h-9">
          {loading ? "Queuing..." : "Queue"}
        </Button>
      </form>

      {error && (
        <div className="text-sm text-destructive bg-destructive/10 rounded-md px-3 py-2 flex items-center justify-between gap-2">
          <span>{error}</span>
          {duplicateUrl && (
            <Button variant="outline" size="sm" onClick={handleForceDownload} className="h-7 text-xs">
              Force re-download
            </Button>
          )}
        </div>
      )}

      {/* Filter bar */}
      <div className="flex flex-wrap items-center gap-3 border-b border-border pb-3">
        {/* Status tabs */}
        <div className="flex gap-1">
          {(["all", "pending", "downloading", "completed", "failed", "archived"] as StatusFilter[]).map((s) => (
            <Button
              key={s}
              variant={statusFilter === s ? "default" : "ghost"}
              size="sm"
              className="h-7 text-xs"
              onClick={() => setStatusFilter(s)}
            >
              {s.charAt(0).toUpperCase() + s.slice(1)}
              {s !== "archived" && counts[s] ? ` (${counts[s]})` : ""}
            </Button>
          ))}
        </div>

        <div className="w-px h-5 bg-border" />

        {/* Archive bulk actions */}
        {counts.completed > 0 && (
          <Button variant="ghost" size="sm" className="h-7 text-xs" onClick={() => handleArchiveAll("completed")}>
            Archive Completed
          </Button>
        )}
        {counts.failed > 0 && (
          <Button variant="ghost" size="sm" className="h-7 text-xs" onClick={() => handleArchiveAll("failed")}>
            Archive Failed
          </Button>
        )}

        {/* Category checkboxes */}
        {categories.length > 0 && (
          <>
            <div className="w-px h-5 bg-border" />
            <div className="flex items-center gap-3 text-xs">
              <span className="text-muted-foreground">Categories:</span>
              <label className="flex items-center gap-1.5 cursor-pointer">
                <Checkbox
                  checked={selectedCategories.has(null)}
                  onCheckedChange={() => toggleCategory(null)}
                />
                <span>Uncategorized</span>
              </label>
              {categories.map((cat) => (
                <label key={cat.id} className="flex items-center gap-1.5 cursor-pointer">
                  <Checkbox
                    checked={selectedCategories.has(cat.id)}
                    onCheckedChange={() => toggleCategory(cat.id)}
                  />
                  <span>{cat.name}</span>
                </label>
              ))}
            </div>
          </>
        )}
      </div>

      {/* Queue List */}
      <div className="space-y-1">
        {entries.length === 0 ? (
          <p className="text-muted-foreground text-sm py-8 text-center">
            {statusFilter === "archived" ? "No archived items." : "Queue is empty."}
          </p>
        ) : (
          entries.map((entry) => {
            const isExpanded = expandedId === entry.id;
            const isDragging = dragId === entry.id;
            const isDragOver = dragOverId === entry.id;
            const canDrag = entry.status === "pending";
            const liveLog = wsLogs[entry.id] || [];
            const dbLog = entry.output?.split("\n").filter(Boolean) || [];
            const outputLines = liveLog.length > 0 ? liveLog : dbLog;
            const catName = categoryName(entry.category_id);

            return (
              <div
                key={entry.id}
                draggable={canDrag}
                onDragStart={() => handleDragStart(entry.id)}
                onDragOver={(e) => handleDragOver(e, entry.id)}
                onDragLeave={handleDragLeave}
                onDrop={(e) => handleDrop(e, entry.id)}
                onDragEnd={handleDragEnd}
                className={`rounded border bg-card transition-all ${
                  isDragOver ? "border-primary border-2" : "border-border"
                } ${isDragging ? "opacity-40" : ""}`}
              >
                {/* Compact row */}
                <div
                  className="flex items-center gap-2 px-3 py-1.5 cursor-pointer text-xs"
                  onClick={() => setExpandedId(isExpanded ? null : entry.id)}
                >
                  {canDrag ? (
                    <span className="text-muted-foreground hover:text-foreground cursor-grab active:cursor-grabbing select-none">&#x2807;</span>
                  ) : (
                    <span className="w-3" />
                  )}

                  <Badge variant={STATUS_VARIANTS[entry.status] || "outline"} className="text-[10px] px-1.5 py-0">
                    {entry.status}
                  </Badge>

                  {entry.status === "downloading" && entry.progress && (
                    <span className="shrink-0 font-mono text-primary w-10 text-right text-[11px]">{entry.progress}</span>
                  )}

                  {catName && (
                    <Badge variant="secondary" className="text-[10px] px-1.5 py-0 bg-accent/20 text-accent-foreground">
                      {catName}
                    </Badge>
                  )}

                  <span className="flex-1 min-w-0 font-mono truncate text-muted-foreground" title={entry.url}>
                    {entry.url}
                  </span>

                  {/* Inline actions */}
                  {(entry.status === "completed" || entry.status === "failed") && !entry.archived && (
                    <Button
                      variant="ghost" size="sm" className="h-5 text-[10px] px-1.5"
                      onClick={(e) => { e.stopPropagation(); handleArchive(entry.id); }}
                    >
                      Archive
                    </Button>
                  )}
                  {entry.archived && (
                    <Button
                      variant="ghost" size="sm" className="h-5 text-[10px] px-1.5"
                      onClick={(e) => { e.stopPropagation(); handleUnarchive(entry.id); }}
                    >
                      Unarchive
                    </Button>
                  )}

                  <span className="text-muted-foreground shrink-0">
                    {isExpanded ? "\u25BE" : "\u25B8"}
                  </span>
                </div>

                {/* Progress bar */}
                {entry.status === "downloading" && entry.progress && (
                  <div className="w-full bg-muted h-0.5">
                    <div className="bg-primary h-0.5 transition-all duration-300" style={{ width: entry.progress.replace("%", "") + "%" }} />
                  </div>
                )}

                {/* Expanded detail */}
                {isExpanded && (
                  <div className="px-3 pb-3 pt-2 space-y-2 border-t border-border/50 bg-muted/30">
                    <p className="text-[11px] font-mono text-muted-foreground break-all">{entry.url}</p>

                    <div className="flex items-center gap-3 text-[11px] text-muted-foreground">
                      <span>Queued: {entry.created_at}</span>
                      {entry.error && <span className="text-destructive">{entry.error}</span>}
                    </div>

                    <div className="flex gap-1.5">
                      {entry.status === "pending" && (
                        <Button variant="outline" size="sm" className="h-6 text-[11px]" onClick={(e) => { e.stopPropagation(); handleCancel(entry.id); }}>
                          Cancel
                        </Button>
                      )}
                      {entry.status === "failed" && (
                        <Button variant="outline" size="sm" className="h-6 text-[11px]" onClick={(e) => { e.stopPropagation(); handleRetry(entry); }}>
                          Retry
                        </Button>
                      )}
                    </div>

                    {outputLines.length > 0 && <LogPanel lines={outputLines} />}
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
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const el = containerRef.current;
    if (el) {
      el.scrollTop = el.scrollHeight;
    }
  }, [lines.length]);

  return (
    <div
      ref={containerRef}
      className="h-48 overflow-y-auto rounded border border-border bg-black/40 p-2 font-mono text-[10px] text-green-400/80 leading-relaxed"
    >
      {lines.map((line, i) => (
        <div key={i} className="whitespace-pre-wrap break-all">{line}</div>
      ))}
    </div>
  );
}
