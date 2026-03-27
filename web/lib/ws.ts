export interface ProgressUpdate {
  queue_id: number;
  progress: string;
  status: string;
  output?: string;
}

type ProgressCallback = (update: ProgressUpdate) => void;

let socket: WebSocket | null = null;
const listeners: Set<ProgressCallback> = new Set();
let reconnectTimer: ReturnType<typeof setTimeout> | null = null;

function getWsUrl(): string {
  // Connect directly to the Rust API server on port 8550.
  // Next.js rewrites can't proxy WebSocket upgrades.
  const proto = window.location.protocol === "https:" ? "wss:" : "ws:";
  return `${proto}//${window.location.hostname}:8550/api/ws`;
}

function connect() {
  if (socket?.readyState === WebSocket.OPEN) return;

  socket = new WebSocket(getWsUrl());

  socket.onmessage = (event) => {
    try {
      const update: ProgressUpdate = JSON.parse(event.data);
      listeners.forEach((cb) => cb(update));
    } catch {
      // ignore parse errors
    }
  };

  socket.onclose = () => {
    reconnectTimer = setTimeout(connect, 3000);
  };

  socket.onerror = () => {
    socket?.close();
  };
}

export function subscribeProgress(callback: ProgressCallback): () => void {
  listeners.add(callback);
  if (listeners.size === 1) connect();

  return () => {
    listeners.delete(callback);
    if (listeners.size === 0) {
      if (reconnectTimer) clearTimeout(reconnectTimer);
      socket?.close();
      socket = null;
    }
  };
}
