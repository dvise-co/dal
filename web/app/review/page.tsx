"use client";

import { useState, useEffect, useCallback } from "react";
import { getFiles, rateFile, deleteFile, FileEntry, formatBytes } from "@/lib/api";

type SortField = "date" | "rating" | "name";
type FilterMode = "all" | "unrated" | "incomplete";

export default function ReviewPage() {
  const [files, setFiles] = useState<FileEntry[]>([]);
  const [sort, setSort] = useState<SortField>("date");
  const [filter, setFilter] = useState<FilterMode>("all");
  const [error, setError] = useState("");
  const [ratingInputs, setRatingInputs] = useState<Record<number, string>>({});

  const refresh = useCallback(async () => {
    try {
      const data = await getFiles({
        unrated: filter === "unrated",
        incomplete: filter === "incomplete",
        sort,
      });
      setFiles(data);
    } catch {
      // silently fail
    }
  }, [sort, filter]);

  useEffect(() => {
    refresh();
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
      setRatingInputs((prev) => {
        const next = { ...prev };
        delete next[id];
        return next;
      });
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

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-bold">Review Files</h1>
        <button
          onClick={refresh}
          className="rounded-md border border-border px-3 py-1.5 text-xs hover:bg-zinc-800 transition-colors"
        >
          Refresh
        </button>
      </div>

      {/* Controls */}
      <div className="flex flex-wrap gap-4">
        <div className="flex items-center gap-2">
          <label className="text-sm text-muted-foreground">Filter:</label>
          <div className="flex rounded-lg border border-border overflow-hidden">
            {(["all", "unrated", "incomplete"] as FilterMode[]).map((f) => (
              <button
                key={f}
                onClick={() => setFilter(f)}
                className={`px-3 py-1.5 text-xs font-medium transition-colors ${
                  filter === f
                    ? "bg-zinc-700 text-white"
                    : "text-muted-foreground hover:bg-zinc-800"
                }`}
              >
                {f.charAt(0).toUpperCase() + f.slice(1)}
              </button>
            ))}
          </div>
        </div>
        <div className="flex items-center gap-2">
          <label className="text-sm text-muted-foreground">Sort:</label>
          <div className="flex rounded-lg border border-border overflow-hidden">
            {(["date", "rating", "name"] as SortField[]).map((s) => (
              <button
                key={s}
                onClick={() => setSort(s)}
                className={`px-3 py-1.5 text-xs font-medium transition-colors ${
                  sort === s
                    ? "bg-zinc-700 text-white"
                    : "text-muted-foreground hover:bg-zinc-800"
                }`}
              >
                {s.charAt(0).toUpperCase() + s.slice(1)}
              </button>
            ))}
          </div>
        </div>
      </div>

      {error && (
        <p className="text-sm text-destructive bg-red-500/10 rounded-lg px-4 py-2">{error}</p>
      )}

      {/* File Grid */}
      {files.length === 0 ? (
        <p className="text-muted-foreground text-sm py-8 text-center">
          No files to review. Download something first.
        </p>
      ) : (
        <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          {files.map((file) => (
            <div
              key={file.id}
              className="rounded-lg border border-border bg-zinc-900 p-4 space-y-3"
            >
              {/* Filename + status */}
              <div>
                <p className="text-sm font-medium truncate" title={file.filename}>
                  {file.filename}
                </p>
                <p
                  className="text-xs text-muted-foreground truncate mt-0.5"
                  title={file.path}
                >
                  {file.path}
                </p>
              </div>

              {/* Metadata */}
              <div className="flex items-center gap-3 text-xs text-muted-foreground">
                <span>{formatBytes(file.size_bytes)}</span>
                {file.duration && <span>{file.duration}</span>}
                {file.rating !== null && (
                  <span className="text-accent font-bold text-sm">
                    {file.rating}
                  </span>
                )}
                {file.filename.startsWith("INCOMPLETE - ") && (
                  <span className="text-warning font-medium">INCOMPLETE</span>
                )}
              </div>

              {/* Rating + Actions */}
              <div className="flex items-center gap-2">
                <input
                  type="number"
                  min={10}
                  max={99}
                  placeholder={file.rating?.toString() || "10-99"}
                  value={ratingInputs[file.id] ?? ""}
                  onChange={(e) =>
                    setRatingInputs((prev) => ({
                      ...prev,
                      [file.id]: e.target.value,
                    }))
                  }
                  onKeyDown={(e) => {
                    if (e.key === "Enter") handleRate(file.id);
                  }}
                  className="w-20 rounded-md bg-zinc-800 border border-border px-2 py-1 text-sm text-center focus:outline-none focus:ring-1 focus:ring-accent"
                />
                <button
                  onClick={() => handleRate(file.id)}
                  className="rounded-md bg-accent px-3 py-1 text-xs font-medium text-white hover:bg-blue-600 transition-colors"
                >
                  Rate
                </button>
                <button
                  onClick={() => handleDelete(file.id, file.filename)}
                  className="rounded-md border border-red-500/30 px-3 py-1 text-xs text-destructive hover:bg-red-500/10 transition-colors ml-auto"
                >
                  Delete
                </button>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
