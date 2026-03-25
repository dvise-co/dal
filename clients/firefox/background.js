let categories = [];
let pendingLink = null;

// Build context menu from categories
async function rebuildMenu() {
  await browser.contextMenus.removeAll();

  // Parent menu
  browser.contextMenus.create({
    id: "dal-parent",
    title: "Queue in DAL",
    contexts: ["link"],
  });

  // Uncategorized option
  browser.contextMenus.create({
    id: "dal-queue-uncategorized",
    parentId: "dal-parent",
    title: "Uncategorized",
    contexts: ["link"],
  });

  // Category items
  for (const cat of categories) {
    browser.contextMenus.create({
      id: `dal-queue-cat-${cat.id}`,
      parentId: "dal-parent",
      title: cat.name,
      contexts: ["link"],
    });
  }

  // Separator + New Category
  browser.contextMenus.create({
    id: "dal-separator",
    parentId: "dal-parent",
    type: "separator",
    contexts: ["link"],
  });

  browser.contextMenus.create({
    id: "dal-new-category",
    parentId: "dal-parent",
    title: "New Category...",
    contexts: ["link"],
  });
}

// Fetch categories from server
async function syncCategories() {
  try {
    const { serverUrl } = await browser.storage.local.get("serverUrl");
    const base = serverUrl || "http://localhost:8550";
    const response = await fetch(`${base}/api/categories`);
    if (response.ok) {
      categories = await response.json();
      await browser.storage.local.set({ categories });
      await rebuildMenu();
    }
  } catch {
    // Load from cache on failure
    const cached = await browser.storage.local.get("categories");
    if (cached.categories) {
      categories = cached.categories;
    }
    await rebuildMenu();
  }
}

// Initialize on install/startup
browser.runtime.onInstalled.addListener(() => syncCategories());
browser.runtime.onStartup.addListener(() => syncCategories());

// Sync every 60 seconds
setInterval(syncCategories, 60000);

// Handle context menu click
browser.contextMenus.onClicked.addListener(async (info, tab) => {
  if (!info.linkUrl) return;

  const { serverUrl } = await browser.storage.local.get("serverUrl");
  const base = serverUrl || "http://localhost:8550";

  if (info.menuItemId === "dal-new-category") {
    // Store the link and open popup to get category name
    pendingLink = info.linkUrl;
    // Use a tab script to prompt since background can't show UI
    try {
      const results = await browser.tabs.executeScript(tab.id, {
        code: 'prompt("Enter new category name:")',
      });
      const name = results && results[0];
      if (name && name.trim()) {
        await queueWithCategory(base, pendingLink, name.trim());
      }
      pendingLink = null;
    } catch {
      console.error("DAL: Could not prompt for category name");
      pendingLink = null;
    }
    return;
  }

  let category = null;
  if (info.menuItemId.startsWith("dal-queue-cat-")) {
    const catId = parseInt(info.menuItemId.replace("dal-queue-cat-", ""), 10);
    const cat = categories.find((c) => c.id === catId);
    if (cat) category = cat.name;
  }

  try {
    await queueUrl(base, info.linkUrl, category);
    await updateBadge(base);
  } catch (err) {
    console.error("DAL: Failed to queue URL:", err);
  }
});

async function queueUrl(baseUrl, url, category) {
  const body = { url };
  if (category) body.category = category;

  const response = await fetch(`${baseUrl}/api/queue`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });

  if (!response.ok) {
    const data = await response.json().catch(() => ({}));
    throw new Error(data.error || response.statusText);
  }
}

async function queueWithCategory(baseUrl, url, categoryName) {
  try {
    // Create category (server will return existing if duplicate)
    await fetch(`${baseUrl}/api/categories`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ name: categoryName }),
    });

    await queueUrl(baseUrl, url, categoryName);
    await syncCategories();
    await updateBadge(baseUrl);
  } catch (err) {
    console.error("DAL: Failed to queue with new category:", err);
  }
}

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
