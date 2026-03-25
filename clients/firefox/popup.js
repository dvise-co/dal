const serverUrlInput = document.getElementById("serverUrl");
const urlInput = document.getElementById("urlInput");
const queueBtn = document.getElementById("queueBtn");
const statusDiv = document.getElementById("status");
const badgeDiv = document.getElementById("badge");

// Load saved server URL
browser.storage.local.get("serverUrl").then(({ serverUrl }) => {
  serverUrlInput.value = serverUrl || "http://localhost:8550";
  refreshBadge();
});

// Save server URL on change
serverUrlInput.addEventListener("change", () => {
  browser.storage.local.set({ serverUrl: serverUrlInput.value });
  refreshBadge();
});

// Enable queue button when URL is entered
urlInput.addEventListener("input", () => {
  queueBtn.disabled = !urlInput.value.trim();
});

// Queue URL on button click
queueBtn.addEventListener("click", async () => {
  const url = urlInput.value.trim();
  if (!url) return;

  queueBtn.disabled = true;
  showStatus("", "");

  try {
    const base = serverUrlInput.value || "http://localhost:8550";
    const response = await fetch(`${base}/api/queue`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ url }),
    });

    if (!response.ok) {
      const body = await response.json().catch(() => ({}));
      throw new Error(body.error || response.statusText);
    }

    showStatus("Queued successfully!", "ok");
    urlInput.value = "";
    refreshBadge();
  } catch (err) {
    showStatus(err.message, "err");
  } finally {
    queueBtn.disabled = !urlInput.value.trim();
  }
});

// Also queue on Enter key
urlInput.addEventListener("keydown", (e) => {
  if (e.key === "Enter" && urlInput.value.trim()) {
    queueBtn.click();
  }
});

function showStatus(message, type) {
  if (!message) {
    statusDiv.style.display = "none";
    return;
  }
  statusDiv.textContent = message;
  statusDiv.className = `status ${type}`;
  statusDiv.style.display = "block";
}

async function refreshBadge() {
  try {
    const base = serverUrlInput.value || "http://localhost:8550";
    const response = await fetch(`${base}/api/queue`);
    if (response.ok) {
      const items = await response.json();
      const pending = items.filter((i) => i.status === "pending" || i.status === "downloading").length;
      const completed = items.filter((i) => i.status === "completed").length;
      badgeDiv.textContent = `${pending} active · ${completed} completed`;
    }
  } catch {
    badgeDiv.textContent = "Unable to connect to server";
  }
}
