export interface QueueEntry {
  id: number;
  url: string;
  status: "pending" | "downloading" | "completed" | "failed";
  progress: string | null;
  error: string | null;
  output: string | null;
  category_id: number | null;
  position: number | null;
  archived: boolean | null;
  attempts: number | null;
  created_at: string;
  updated_at: string;
}

export interface HistoryEntry {
  id: number;
  queue_id: number;
  status: string;
  error: string | null;
  output: string | null;
  created_at: string;
}

export interface FileEntry {
  id: number;
  queue_id: number | null;
  filename: string;
  original: string;
  path: string;
  rating: number | null;
  size_bytes: number | null;
  duration: string | null;
  thumbnail: string | null;
  category_id: number | null;
  created_at: string;
  rated_at: string | null;
}

export interface Category {
  id: number;
  name: string;
  created_at: string;
}

const API_BASE = "/api";

async function fetchJSON<T>(url: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`${API_BASE}${url}`, {
    ...init,
    headers: {
      "Content-Type": "application/json",
      ...init?.headers,
    },
  });
  if (!res.ok) {
    const body = await res.json().catch(() => ({ error: res.statusText }));
    throw new Error(body.error || res.statusText);
  }
  if (res.status === 204) return undefined as T;
  return res.json();
}

export async function getQueue(params?: {
  status?: string;
  archived?: boolean;
  include_archived?: boolean;
}): Promise<QueueEntry[]> {
  const searchParams = new URLSearchParams();
  if (params?.status) searchParams.set("status", params.status);
  if (params?.archived) searchParams.set("archived", "true");
  if (params?.include_archived) searchParams.set("include_archived", "true");
  const qs = searchParams.toString();
  return fetchJSON(`/queue${qs ? `?${qs}` : ""}`);
}

export async function archiveQueue(id: number): Promise<void> {
  return fetchJSON(`/queue/${id}/archive`, { method: "POST" });
}

export async function unarchiveQueue(id: number): Promise<void> {
  return fetchJSON(`/queue/${id}/unarchive`, { method: "POST" });
}

export async function archiveAllQueue(status?: string): Promise<void> {
  return fetchJSON("/queue/archive-all", {
    method: "POST",
    body: JSON.stringify({ status }),
  });
}

export async function retryQueue(id: number): Promise<void> {
  return fetchJSON(`/queue/${id}/retry`, { method: "POST" });
}

export async function retryAllQueue(): Promise<void> {
  return fetchJSON("/queue/retry-all", { method: "POST" });
}

export async function getQueueHistory(id: number): Promise<HistoryEntry[]> {
  return fetchJSON(`/queue/${id}/history`);
}

export async function addToQueue(
  url: string,
  force: boolean = false,
  category?: string
): Promise<QueueEntry> {
  const body: Record<string, unknown> = { url, force };
  if (category) body.category = category;
  return fetchJSON("/queue", {
    method: "POST",
    body: JSON.stringify(body),
  });
}

export async function cancelQueue(id: number): Promise<void> {
  return fetchJSON(`/queue/${id}`, { method: "DELETE" });
}

export async function reorderQueue(order: number[]): Promise<void> {
  return fetchJSON("/queue/reorder", {
    method: "PUT",
    body: JSON.stringify({ order }),
  });
}

export async function getFiles(params?: {
  unrated?: boolean;
  incomplete?: boolean;
  category_id?: number;
  sort?: string;
}): Promise<FileEntry[]> {
  const searchParams = new URLSearchParams();
  if (params?.unrated) searchParams.set("unrated", "true");
  if (params?.incomplete) searchParams.set("incomplete", "true");
  if (params?.category_id) searchParams.set("category_id", String(params.category_id));
  if (params?.sort) searchParams.set("sort", params.sort);
  const qs = searchParams.toString();
  return fetchJSON(`/files${qs ? `?${qs}` : ""}`);
}

export async function rateFile(
  id: number,
  rating: number
): Promise<FileEntry> {
  return fetchJSON(`/files/${id}/rate`, {
    method: "POST",
    body: JSON.stringify({ rating }),
  });
}

export async function deleteFile(id: number): Promise<void> {
  return fetchJSON(`/files/${id}`, { method: "DELETE" });
}

// Categories
export async function getCategories(): Promise<Category[]> {
  return fetchJSON("/categories");
}

export async function createCategory(name: string): Promise<Category> {
  return fetchJSON("/categories", {
    method: "POST",
    body: JSON.stringify({ name }),
  });
}

export async function deleteCategory(id: number): Promise<void> {
  return fetchJSON(`/categories/${id}`, { method: "DELETE" });
}

export function formatBytes(bytes: number | null): string {
  if (bytes === null || bytes === 0) return "—";
  const units = ["B", "KB", "MB", "GB"];
  let i = 0;
  let size = bytes;
  while (size >= 1024 && i < units.length - 1) {
    size /= 1024;
    i++;
  }
  return `${size.toFixed(i === 0 ? 0 : 1)} ${units[i]}`;
}
