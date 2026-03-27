"use client";

import { useState, useEffect, useCallback } from "react";
import { getFiles, rateFile, deleteFile, scanFiles, requeueFile, getCategories, FileEntry, Category, formatBytes } from "@/lib/api";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";

type SortField = "date" | "rating" | "name";
type FilterMode = "all" | "unrated" | "incomplete";

export default function ReviewPage() {
  const [files, setFiles] = useState<FileEntry[]>([]);
  const [sort, setSort] = useState<SortField>("date");
  const [filter, setFilter] = useState<FilterMode>("all");
  const [categoryFilter, setCategoryFilter] = useState<number | null>(null);
  const [categories, setCategories] = useState<Category[]>([]);
  const [error, setError] = useState("");
  const [ratingInputs, setRatingInputs] = useState<Record<number, string>>({});
  const [expandedId, setExpandedId] = useState<number | null>(null);

  const refresh = useCallback(async () => {
    try {
      const data = await getFiles({
        unrated: filter === "unrated",
        incomplete: filter === "incomplete",
        category_id: categoryFilter ?? undefined,
        sort,
      });
      setFiles(data);
    } catch {
      // silently fail
    }
  }, [sort, filter, categoryFilter]);

  useEffect(() => {
    const load = async () => {
      await refresh();
      getCategories().then(setCategories).catch(() => {});
    };
    load();
  }, [refresh]);

  const handleRate = async (id: number) => {
    const value = parseInt(ratingInputs[id] || "", 10);
    if (isNaN(value) || value < 10 || value > 99) {
      setError("Rating must be between 10 and 99");
      return;
    }
    setError("");
    try {
      await rateFile(id, value);
      setRatingInputs((prev) => { const next = { ...prev }; delete next[id]; return next; });
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to rate");
    }
  };

  const handleDelete = async (id: number, filename: string) => {
    if (!confirm(`Delete "${filename}"? This cannot be undone.`)) return;
    try {
      await deleteFile(id);
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to delete");
    }
  };

  const handleRequeue = async (id: number) => {
    if (!confirm("This will delete the current file and re-download. Continue?")) return;
    try {
      await requeueFile(id);
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to requeue");
    }
  };

  const categoryName = (catId: number | null) => {
    if (catId === null) return null;
    return categories.find((c) => c.id === catId)?.name || null;
  };

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-bold">Review Files</h1>
        <div className="flex gap-2">
          <Button variant="outline" size="sm" className="h-7 text-xs" onClick={async () => { await scanFiles(); await refresh(); }}>
            Scan Disk
          </Button>
          <Button variant="outline" size="sm" className="h-7 text-xs" onClick={refresh}>
            Refresh
          </Button>
        </div>
      </div>

      {/* Filter bar */}
      <div className="flex flex-wrap items-center gap-3 border-b border-border pb-3">
        <div className="flex gap-1">
          {(["all", "unrated", "incomplete"] as FilterMode[]).map((f) => (
            <Button
              key={f}
              variant={filter === f ? "default" : "ghost"}
              size="sm"
              className="h-7 text-xs"
              onClick={() => setFilter(f)}
            >
              {f.charAt(0).toUpperCase() + f.slice(1)}
            </Button>
          ))}
        </div>

        <div className="w-px h-5 bg-border" />

        <div className="flex gap-1">
          {(["date", "rating", "name"] as SortField[]).map((s) => (
            <Button
              key={s}
              variant={sort === s ? "secondary" : "ghost"}
              size="sm"
              className="h-7 text-xs"
              onClick={() => setSort(s)}
            >
              {s.charAt(0).toUpperCase() + s.slice(1)}
            </Button>
          ))}
        </div>

        {categories.length > 0 && (
          <>
            <div className="w-px h-5 bg-border" />
            <select
              value={categoryFilter ?? ""}
              onChange={(e) => setCategoryFilter(e.target.value ? Number(e.target.value) : null)}
              className="rounded-md bg-background border border-input px-2 h-7 text-xs"
            >
              <option value="">All Categories</option>
              {categories.map((cat) => (
                <option key={cat.id} value={cat.id}>{cat.name}</option>
              ))}
            </select>
          </>
        )}
      </div>

      {error && (
        <p className="text-sm text-destructive bg-destructive/10 rounded-md px-3 py-2">{error}</p>
      )}

      {/* File List */}
      <div className="space-y-1">
        {files.length === 0 ? (
          <p className="text-muted-foreground text-sm py-8 text-center">
            No files to review.
          </p>
        ) : (
          files.map((file) => {
            const isExpanded = expandedId === file.id;
            const catName = categoryName(file.category_id);

            return (
              <div key={file.id} className="rounded border border-border bg-card">
                {/* Compact row with inline actions */}
                <div
                  className="flex items-center gap-2 px-3 py-1.5 text-xs"
                >
                  {/* Exists indicator */}
                  <span className={`shrink-0 w-1.5 h-1.5 rounded-full ${file.exists ? "bg-green-500" : "bg-red-500"}`} title={file.exists ? "File exists" : "File missing"} />

                  {/* Rating badge */}
                  {file.rating !== null ? (
                    <Badge variant="default" className="text-[10px] px-1.5 py-0 min-w-[28px] justify-center">
                      {file.rating}
                    </Badge>
                  ) : (
                    <Badge variant="outline" className="text-[10px] px-1.5 py-0 min-w-[28px] justify-center text-muted-foreground">
                      —
                    </Badge>
                  )}

                  {file.filename.startsWith("INCOMPLETE - ") && (
                    <Badge variant="destructive" className="text-[10px] px-1.5 py-0">INC</Badge>
                  )}

                  {file.provisional && (
                    <Badge variant="outline" className="text-[10px] px-1.5 py-0 text-yellow-500 border-yellow-500/30">PROV</Badge>
                  )}

                  {catName && (
                    <Badge variant="secondary" className="text-[10px] px-1.5 py-0 bg-accent/20 text-accent-foreground">
                      {catName}
                    </Badge>
                  )}

                  <span
                    className="flex-1 min-w-0 font-mono truncate text-muted-foreground cursor-pointer"
                    title={file.filename}
                    onClick={() => setExpandedId(isExpanded ? null : file.id)}
                  >
                    {file.filename}
                  </span>

                  <span className="shrink-0 text-muted-foreground">{formatBytes(file.size_bytes)}</span>

                  {/* Inline rating + actions */}
                  <div className="flex items-center gap-1 shrink-0">
                    <Input
                      type="number"
                      min={10}
                      max={99}
                      placeholder={file.rating?.toString() || "10-99"}
                      value={ratingInputs[file.id] ?? ""}
                      onChange={(e) => setRatingInputs((prev) => ({ ...prev, [file.id]: e.target.value }))}
                      onKeyDown={(e) => { if (e.key === "Enter") handleRate(file.id); }}
                      className="w-14 h-5 text-[10px] text-center"
                    />
                    <Button variant="ghost" size="sm" className="h-5 text-[10px] px-1.5" onClick={() => handleRate(file.id)}>
                      Rate
                    </Button>
                    {file.provisional && (
                      <Button
                        variant="ghost" size="sm" className="h-5 text-[10px] px-1.5 text-yellow-500"
                        onClick={() => handleRequeue(file.id)}
                      >
                        Requeue
                      </Button>
                    )}
                    <Button
                      variant="ghost" size="sm" className="h-5 text-[10px] px-1.5 text-destructive"
                      onClick={() => handleDelete(file.id, file.filename)}
                    >
                      Del
                    </Button>
                  </div>

                  <span
                    className="text-muted-foreground shrink-0 cursor-pointer"
                    onClick={() => setExpandedId(isExpanded ? null : file.id)}
                  >
                    {isExpanded ? "\u25BE" : "\u25B8"}
                  </span>
                </div>

                {/* Expanded detail: path + metadata */}
                {isExpanded && (
                  <div className="px-3 pb-2 pt-1.5 border-t border-border/50 bg-muted/30">
                    <p className="text-[10px] font-mono text-muted-foreground break-all">{file.display_path}</p>
                    <div className="flex items-center gap-3 text-[10px] text-muted-foreground mt-1">
                      <span>{formatBytes(file.size_bytes)}</span>
                      {file.duration && <span>{file.duration}</span>}
                      <span>Added: {file.created_at}</span>
                      {file.rated_at && <span>Rated: {file.rated_at}</span>}
                      {!file.exists && <span className="text-destructive font-medium">FILE MISSING</span>}
                    </div>
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
