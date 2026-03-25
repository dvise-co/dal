const serverUrlInput = document.getElementById("serverUrl");
const urlInput = document.getElementById("urlInput");
const categorySelect = document.getElementById("categorySelect");
const queueBtn = document.getElementById("queueBtn");
const statusDiv = document.getElementById("status");
const badgeDiv = document.getElementById("badge");
const catList = document.getElementById("catList");
const newCatInput = document.getElementById("newCatInput");
const addCatBtn = document.getElementById("addCatBtn");

let categories = [];

function getBaseUrl() {
  return serverUrlInput.value || "http://localhost:8550";
}

// Load saved state
browser.storage.local.get(["serverUrl", "categories"]).then((data) => {
  serverUrlInput.value = data.serverUrl || "http://localhost:8550";
  if (data.categories) {
    categories = data.categories;
    renderCategories();
  }
  syncCategories();
  refreshBadge();
});

// Save server URL on change
serverUrlInput.addEventListener("change", () => {
  browser.storage.local.set({ serverUrl: serverUrlInput.value });
  syncCategories();
  refreshBadge();
});

// Enable queue button when URL is entered
urlInput.addEventListener("input", () => {
  queueBtn.disabled = !urlInput.value.trim();
});

// Queue URL
queueBtn.addEventListener("click", async () => {
  const url = urlInput.value.trim();
  if (!url) return;

  queueBtn.disabled = true;
  showStatus("", "");

  try {
    const body = { url };
    const cat = categorySelect.value;
    if (cat) body.category = cat;

    const response = await fetch(`${getBaseUrl()}/api/queue`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });

    if (!response.ok) {
      const data = await response.json().catch(() => ({}));
      throw new Error(data.error || response.statusText);
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

urlInput.addEventListener("keydown", (e) => {
  if (e.key === "Enter" && urlInput.value.trim()) queueBtn.click();
});

// Category management
addCatBtn.addEventListener("click", () => addCategory());
newCatInput.addEventListener("keydown", (e) => {
  if (e.key === "Enter") addCategory();
});

async function addCategory() {
  const name = newCatInput.value.trim();
  if (!name) return;

  try {
    const response = await fetch(`${getBaseUrl()}/api/categories`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ name }),
    });

    if (!response.ok) {
      const data = await response.json().catch(() => ({}));
      throw new Error(data.error || response.statusText);
    }

    newCatInput.value = "";
    await syncCategories();
  } catch (err) {
    showStatus(err.message, "err");
  }
}

async function deleteCategory(id) {
  try {
    const response = await fetch(`${getBaseUrl()}/api/categories/${id}`, {
      method: "DELETE",
    });
    if (!response.ok) {
      const data = await response.json().catch(() => ({}));
      throw new Error(data.error || response.statusText);
    }
    await syncCategories();
  } catch (err) {
    showStatus(err.message, "err");
  }
}

async function syncCategories() {
  try {
    const response = await fetch(`${getBaseUrl()}/api/categories`);
    if (response.ok) {
      categories = await response.json();
      await browser.storage.local.set({ categories });
      renderCategories();
    }
  } catch {
    // use cached
  }
}

function renderCategories() {
  // Update dropdown
  categorySelect.innerHTML = '<option value="">Uncategorized</option>';
  for (const cat of categories) {
    const opt = document.createElement("option");
    opt.value = cat.name;
    opt.textContent = cat.name;
    categorySelect.appendChild(opt);
  }

  // Update list
  catList.innerHTML = "";
  for (const cat of categories) {
    const li = document.createElement("li");
    li.innerHTML = `
      <span>${cat.name}</span>
      <div class="cat-row">
        <button class="btn-danger" data-id="${cat.id}">Delete</button>
      </div>
    `;
    li.querySelector(".btn-danger").addEventListener("click", () => {
      if (confirm(`Delete category "${cat.name}"? Files will be moved to uncategorized.`)) {
        deleteCategory(cat.id);
      }
    });
    catList.appendChild(li);
  }
}

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
    const response = await fetch(`${getBaseUrl()}/api/queue`);
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
