// Create context menu on install
browser.runtime.onInstalled.addListener(() => {
  browser.contextMenus.create({
    id: "dal-queue",
    title: "Queue in DAL",
    contexts: ["link"],
  });
});

// Handle context menu click
browser.contextMenus.onClicked.addListener(async (info) => {
  if (info.menuItemId !== "dal-queue" || !info.linkUrl) return;

  try {
    const { serverUrl } = await browser.storage.local.get("serverUrl");
    const base = serverUrl || "http://localhost:8550";

    const response = await fetch(`${base}/api/queue`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ url: info.linkUrl }),
    });

    if (!response.ok) {
      const body = await response.json().catch(() => ({}));
      throw new Error(body.error || response.statusText);
    }

    // Update badge to show queue count
    await updateBadge(base);
  } catch (err) {
    console.error("DAL: Failed to queue URL:", err);
  }
});

async function updateBadge(baseUrl) {
  try {
    const response = await fetch(`${baseUrl}/api/queue?status=pending`);
    if (response.ok) {
      const items = await response.json();
      const count = items.length;
      browser.action.setBadgeText({ text: count > 0 ? String(count) : "" });
      browser.action.setBadgeBackgroundColor({ color: "#3b82f6" });
    }
  } catch {
    // ignore badge update failures
  }
}
