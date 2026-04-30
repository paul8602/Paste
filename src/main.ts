import "./styles.css";
import { listen } from "@tauri-apps/api/event";
import {
  type AppSettings,
  type ClipSummary,
  deleteClip,
  getSettings,
  hasAccessibilityPermission,
  hidePanel,
  openAccessibilitySettings,
  pasteClip,
  pinClip,
  saveSettings,
  searchClips
} from "./lib/commands";

const app = document.querySelector<HTMLDivElement>("#app");

if (!app) {
  throw new Error("Missing #app root element");
}

let query = "";
let clips: ClipSummary[] = [];
let selectedIndex = 0;
let settings: AppSettings = {
  maxItems: 1000,
  maxPayloadBytes: 25 * 1024 * 1024,
  trimWhitespaceForTextDedup: true
};

app.innerHTML = `
  <section class="panel">
    <div id="permission" class="permission hidden">
      Grant Accessibility permission in System Settings to paste into other apps.
      <button id="permission-open">Open Settings</button>
    </div>
    <header class="search-row">
      <div class="search-icon">⌘</div>
      <input id="search" type="search" placeholder="Search clipboard history" autofocus />
      <button id="settings-toggle" title="Settings">Settings</button>
    </header>
    <ol id="clips" class="clip-list"></ol>
    <form id="settings" class="settings hidden">
      <label>
        Max items
        <input id="max-items" type="number" min="50" max="10000" step="50" />
      </label>
      <label>
        Max item size (MB)
        <input id="max-size" type="number" min="1" max="500" step="1" />
      </label>
      <label class="checkbox">
        <input id="trim-dedup" type="checkbox" />
        Trim whitespace for text deduplication
      </label>
      <button type="submit">Save</button>
    </form>
  </section>
`;

const searchInput = document.querySelector<HTMLInputElement>("#search")!;
const permissionBanner = document.querySelector<HTMLDivElement>("#permission")!;
const permissionOpen = document.querySelector<HTMLButtonElement>("#permission-open")!;
const list = document.querySelector<HTMLOListElement>("#clips")!;
const settingsForm = document.querySelector<HTMLFormElement>("#settings")!;
const settingsToggle = document.querySelector<HTMLButtonElement>("#settings-toggle")!;
const maxItemsInput = document.querySelector<HTMLInputElement>("#max-items")!;
const maxSizeInput = document.querySelector<HTMLInputElement>("#max-size")!;
const trimDedupInput = document.querySelector<HTMLInputElement>("#trim-dedup")!;

function clipIcon(kind: ClipSummary["kind"]): string {
  switch (kind) {
    case "image":
      return "IMG";
    case "file_url":
      return "FILE";
    case "html":
      return "HTML";
    case "rtf":
      return "RTF";
    case "text":
      return "TXT";
  }
}

function renderClips(): void {
  list.innerHTML = "";

  if (clips.length === 0) {
    const empty = document.createElement("li");
    empty.className = "empty-state";
    empty.textContent = query ? "No matching clipboard items" : "Copy something to begin";
    list.append(empty);
    return;
  }

  clips.forEach((clip, index) => {
    const item = document.createElement("li");
    item.className = `clip ${index === selectedIndex ? "selected" : ""}`;
    item.dataset.id = clip.id;

    const number = index < 9 ? `${index + 1}` : "";
    item.innerHTML = `
      <span class="clip-number">${number}</span>
      <span class="clip-kind">${clipIcon(clip.kind)}</span>
      <span class="clip-body">
        <strong>${escapeHtml(clip.textPreview || "(empty item)")}</strong>
        <small>${new Date(clip.createdAt).toLocaleString()}${clip.isPinned ? " · Pinned" : ""}</small>
      </span>
      <button class="pin" title="Pin">${clip.isPinned ? "Unpin" : "Pin"}</button>
      <button class="delete" title="Delete">Delete</button>
    `;

    item.addEventListener("mousemove", () => {
      selectedIndex = index;
      renderClips();
    });
    item.addEventListener("dblclick", () => chooseClip(index));
    item.querySelector<HTMLButtonElement>(".pin")?.addEventListener("click", async (event) => {
      event.stopPropagation();
      await pinClip(clip.id, !clip.isPinned);
      await refresh();
    });
    item.querySelector<HTMLButtonElement>(".delete")?.addEventListener("click", async (event) => {
      event.stopPropagation();
      await deleteClip(clip.id);
      await refresh();
    });

    list.append(item);
  });
}

function escapeHtml(value: string): string {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#039;");
}

async function refresh(): Promise<void> {
  clips = await searchClips(query, 40);
  selectedIndex = Math.min(selectedIndex, Math.max(clips.length - 1, 0));
  renderClips();
}

async function chooseClip(index: number): Promise<void> {
  const clip = clips[index];
  if (!clip) {
    return;
  }

  try {
    await pasteClip(clip.id);
  } catch (error) {
    permissionBanner.classList.remove("hidden");
    console.error(error);
  }
}

function renderSettings(): void {
  maxItemsInput.value = String(settings.maxItems);
  maxSizeInput.value = String(Math.round(settings.maxPayloadBytes / 1024 / 1024));
  trimDedupInput.checked = settings.trimWhitespaceForTextDedup;
}

searchInput.addEventListener("input", async () => {
  query = searchInput.value;
  selectedIndex = 0;
  await refresh();
});

settingsToggle.addEventListener("click", () => {
  settingsForm.classList.toggle("hidden");
  renderSettings();
});

settingsForm.addEventListener("submit", async (event) => {
  event.preventDefault();
  settings = await saveSettings({
    maxItems: Number(maxItemsInput.value),
    maxPayloadBytes: Number(maxSizeInput.value) * 1024 * 1024,
    trimWhitespaceForTextDedup: trimDedupInput.checked
  });
  settingsForm.classList.add("hidden");
});

permissionOpen.addEventListener("click", () => {
  void openAccessibilitySettings();
});

document.addEventListener("keydown", async (event) => {
  if (event.key === "Escape") {
    await hidePanel();
    return;
  }

  if (event.key === "ArrowDown") {
    event.preventDefault();
    selectedIndex = Math.min(selectedIndex + 1, clips.length - 1);
    renderClips();
    return;
  }

  if (event.key === "ArrowUp") {
    event.preventDefault();
    selectedIndex = Math.max(selectedIndex - 1, 0);
    renderClips();
    return;
  }

  if (event.key === "Enter") {
    event.preventDefault();
    await chooseClip(selectedIndex);
    return;
  }

  if (/^[1-9]$/.test(event.key) && !event.metaKey && !event.ctrlKey && !event.altKey) {
    const index = Number(event.key) - 1;
    if (index < clips.length) {
      event.preventDefault();
      await chooseClip(index);
    }
  }
});

window.addEventListener("focus", () => {
  searchInput.focus();
  void refresh();
});

settings = await getSettings();
renderSettings();
if (!(await hasAccessibilityPermission())) {
  permissionBanner.classList.remove("hidden");
}
await refresh();
await listen("clips-changed", () => {
  void refresh();
});
