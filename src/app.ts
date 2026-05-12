import "./styles.css";
import { listen } from "@tauri-apps/api/event";
import {
  type AppSettings,
  type ClipSummary,
  getSettings,
  hasAccessibilityPermission,
  hidePanel,
  openAccessibilitySettings,
  pasteClip,
  searchClips
} from "./lib/commands";
import { renderClips, updateSelection } from "./components/clip-list";
import { renderSettings, setupSettings } from "./components/settings";

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
      <span class="search-spinner" hidden></span>
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

async function refresh(): Promise<void> {
  const spinner = document.querySelector<HTMLElement>(".search-spinner");
  if (spinner) spinner.hidden = false;

  clips = await searchClips(query, 40);
  selectedIndex = Math.min(selectedIndex, Math.max(clips.length - 1, 0));
  renderClips(clips, selectedIndex, list, query, chooseClip, refresh);

  if (spinner) spinner.hidden = true;
}

async function chooseClip(index: number): Promise<void> {
  const clip = clips[index];
  if (!clip) return;

  try {
    await pasteClip(clip.id);
  } catch (error) {
    permissionBanner.classList.remove("hidden");
    console.error(error);
  }
}

setupSettings(settingsForm, settingsToggle, {
  maxItemsInput, maxSizeInput, trimDedupInput
}, () => settings, (updated) => {
  settings = updated;
});

let searchDebounce: ReturnType<typeof setTimeout>;
searchInput.addEventListener("input", () => {
  clearTimeout(searchDebounce);
  searchDebounce = setTimeout(async () => {
    query = searchInput.value;
    selectedIndex = 0;
    await refresh();
  }, 150);
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
    const newIndex = Math.min(selectedIndex + 1, clips.length - 1);
    if (newIndex !== selectedIndex) {
      updateSelection(list, selectedIndex, newIndex);
      selectedIndex = newIndex;
    }
    return;
  }

  if (event.key === "ArrowUp") {
    event.preventDefault();
    const newIndex = Math.max(selectedIndex - 1, 0);
    if (newIndex !== selectedIndex) {
      updateSelection(list, selectedIndex, newIndex);
      selectedIndex = newIndex;
    }
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
renderSettings(settings, { maxItemsInput, maxSizeInput, trimDedupInput });
if (!(await hasAccessibilityPermission())) {
  permissionBanner.classList.remove("hidden");
}
await refresh();
await listen("clips-changed", () => {
  void refresh();
});
