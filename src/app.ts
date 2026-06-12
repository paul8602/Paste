import "./styles.css";
import { listen } from "@tauri-apps/api/event";
import {
  type AppSettings,
  type ClipSummary,
  type DiskUsage,
  type ImportResult,
  type Rule,
  type Tag,
  autoPrune,
  batchApplyRules,
  countByDateRange,
  countByType,
  countPrunable,
  createRule,
  createTag,
  deleteByDateRange,
  deleteByType,
  deleteClip,
  deleteRule,
  deleteSelected,
  deleteTag,
  exportToCsv,
  exportToJson,
  getDiskUsage,
  getSettings,
  hasAccessibilityPermission,
  hidePanel,
  importFromCsv,
  importFromJson,
  listRules,
  listTags,
  openAccessibilitySettings,
  pasteClip,
  pinClip,
  searchClips,
  updateRule,
  verifyDatabase
} from "./lib/commands";
import { renderClips, updateSelection } from "./components/clip-list";
import { renderSettings, setupSettings } from "./components/settings";

const app = document.querySelector<HTMLDivElement>("#app");

if (!app) {
  throw new Error("Missing #app root element");
}

const PAGE_SIZE = 40;

let query = "";
let clips: ClipSummary[] = [];
let selectedIndex = 0;
let hasMore = false;
let loadingMore = false;
let settings: AppSettings = {
  maxItems: 1000,
  maxPayloadBytes: 25 * 1024 * 1024,
  trimWhitespaceForTextDedup: true,
  useSamplingHash: true,
  retentionDays: 90
};

// ── Toast notification system ──────────────────────────────────────

function showToast(message: string, type: "success" | "error" | "info" = "info", duration = 3000): void {
  const container = document.getElementById("toast-container")!;
  const toast = document.createElement("div");
  toast.className = `toast toast--${type}`;
  toast.textContent = message;
  container.appendChild(toast);
  requestAnimationFrame(() => toast.classList.add("toast--visible"));
  setTimeout(() => {
    toast.classList.remove("toast--visible");
    toast.addEventListener("transitionend", () => toast.remove());
  }, duration);
}

// ── Confirm dialog helper ──────────────────────────────────────────

function showLoading(message = "Processing…"): void {
  const overlay = document.getElementById("loading-overlay")!;
  const msg = document.getElementById("loading-message")!;
  msg.textContent = message;
  overlay.classList.remove("hidden");
}

function hideLoading(): void {
  const overlay = document.getElementById("loading-overlay")!;
  overlay.classList.add("hidden");
}

function showConfirm(message: string): Promise<boolean> {
  return new Promise((resolve) => {
    const overlay = document.getElementById("confirm-overlay")!;
    const msg = document.getElementById("confirm-message")!;
    const okBtn = document.getElementById("confirm-ok")!;
    const cancelBtn = document.getElementById("confirm-cancel")!;
    msg.textContent = message;
    overlay.classList.remove("hidden");
    const cleanup = (result: boolean) => {
      overlay.classList.add("hidden");
      okBtn.removeEventListener("click", onOk);
      cancelBtn.removeEventListener("click", onCancel);
      resolve(result);
    };
    const onOk = () => cleanup(true);
    const onCancel = () => cleanup(false);
    okBtn.addEventListener("click", onOk);
    cancelBtn.addEventListener("click", onCancel);
  });
}

// ── HTML ───────────────────────────────────────────────────────────

app.innerHTML = `
  <div id="toast-container" class="toast-container"></div>
  <div id="loading-overlay" class="loading-overlay hidden">
    <div class="loading-spinner"></div>
    <span id="loading-message" class="loading-message">Processing…</span>
  </div>
  <div id="context-menu" class="context-menu hidden">
    <button data-action="paste">Paste to Active App</button>
    <button data-action="copy">Copy to Clipboard</button>
    <button data-action="pin">Pin/Unpin</button>
    <button data-action="tags">Edit Tags…</button>
    <button data-action="export">Export Item</button>
    <button data-action="delete" class="danger">Delete</button>
  </div>
  <div id="confirm-overlay" class="confirm-overlay hidden">
    <div class="confirm-dialog">
      <p id="confirm-message"></p>
      <div class="confirm-actions">
        <button id="confirm-cancel" type="button">Cancel</button>
        <button id="confirm-ok" type="button" class="danger">Confirm</button>
      </div>
    </div>
  </div>
  <section class="panel">
    <div id="permission" class="permission hidden">
      Grant Accessibility permission in System Settings to paste into other apps.
      <button id="permission-open">Open Settings</button>
    </div>
    <header class="search-row">
      <div class="search-icon">⌘</div>
      <input id="search" type="search" placeholder="Search clipboard history" autofocus />
      <span class="search-spinner" hidden></span>
      <button id="select-toggle" title="Select items">Select</button>
      <button id="settings-toggle" title="Settings">Settings</button>
    </header>
    <ol id="clips" class="clip-list"></ol>
    <footer class="shortcut-hints">
      <span><kbd>↑↓</kbd> Navigate</span>
      <span><kbd>1-9</kbd> Quick paste</span>
      <span><kbd>Enter</kbd> Paste</span>
      <span><kbd>Esc</kbd> Close</span>
    </footer>
    <div id="select-bar" class="select-bar hidden">
      <span id="select-count" class="select-count">0 selected</span>
      <button type="button" id="select-all">Select All</button>
      <button type="button" id="deselect-all">Deselect All</button>
      <button type="button" id="delete-selected-btn" class="danger" disabled>Delete Selected</button>
      <button type="button" id="exit-select">Cancel</button>
    </div>
    <form id="settings" class="settings hidden">
      <h3>Storage</h3>
      <label>
        Max items
        <input id="max-items" type="number" min="50" max="10000" step="50" />
      </label>
      <label>
        Max item size (MB)
        <input id="max-size" type="number" min="1" max="500" step="1" />
      </label>
      <label>
        Retention (days, 0 = never)
        <input id="retention-days" type="number" min="0" max="3650" step="30" />
      </label>
      <h3>Deduplication</h3>
      <label class="checkbox">
        <input id="trim-dedup" type="checkbox" />
        Trim whitespace for text deduplication
      </label>
        <label class="checkbox">
        <input id="sampling-hash" type="checkbox" />
        Sampling hash for large items (&gt;256KB, faster but may miss duplicates)
      </label>
      <h3>Export / Import</h3>
      <div class="export-filters">
        <label>From <input id="export-date-from" type="date" /></label>
        <label>To <input id="export-date-to" type="date" /></label>
        <select id="export-type-filter">
          <option value="">All types</option>
          <option value="text">Text</option>
          <option value="html">HTML</option>
          <option value="rtf">RTF</option>
          <option value="image">Image</option>
          <option value="file_url">File URLs</option>
        </select>
      </div>
      <div class="settings-actions">
        <button type="button" id="export-json">Export JSON</button>
        <button type="button" id="export-csv">Export CSV</button>
        <button type="button" id="import-btn">Import JSON…</button>
        <input type="file" id="import-file" accept=".json" hidden />
        <button type="button" id="import-csv-btn">Import CSV…</button>
        <input type="file" id="import-csv-file" accept=".csv" hidden />
      </div>
      <div id="import-mode-panel" class="import-mode-panel hidden">
        <div class="import-mode-options">
          <label class="radio-option">
            <input type="radio" name="import-mode" value="merge" checked />
            <span><strong>Merge</strong> — add new items, skip duplicates</span>
          </label>
          <label class="radio-option">
            <input type="radio" name="import-mode" value="replace" />
            <span><strong>Replace</strong> — delete existing, import fresh</span>
          </label>
          <label class="radio-option">
            <input type="radio" name="import-mode" value="append" />
            <span><strong>Append</strong> — add all items regardless of duplicates</span>
          </label>
        </div>
        <div class="import-mode-actions">
          <button type="button" id="import-confirm">Import</button>
          <button type="button" id="import-cancel">Cancel</button>
        </div>
      </div>
      <div id="import-result" class="import-result hidden"></div>
      <h3>Data Management</h3>
      <div class="settings-actions">
        <button type="button" id="prune-now">Clean Up Old Items</button>
        <button type="button" id="delete-by-date-range-btn" class="danger">Delete by Date Range…</button>
        <button type="button" id="delete-by-type" class="danger">Delete by Type…</button>
        <button type="button" id="disk-usage">Disk Usage</button>
      </div>
      <div id="disk-usage-result" class="import-result hidden"></div>
      <div id="date-range-panel" class="date-range-panel hidden">
        <div class="date-range-inputs">
          <label>From <input id="date-from" type="date" /></label>
          <label>To <input id="date-to" type="date" /></label>
        </div>
        <span id="date-range-count" class="date-range-count"></span>
        <div class="date-range-actions">
          <button type="button" id="date-range-preview">Preview</button>
          <button type="button" id="date-range-confirm" class="danger" disabled>Delete</button>
          <button type="button" id="date-range-cancel">Cancel</button>
        </div>
      </div>
      <div id="type-delete-panel" class="type-delete-panel hidden">
        <select id="type-select">
          <option value="text">Text</option>
          <option value="html">HTML</option>
          <option value="rtf">RTF</option>
          <option value="image">Image</option>
          <option value="file_url">File URLs</option>
        </select>
        <span id="type-delete-count" class="type-delete-count"></span>
        <button type="button" id="type-delete-preview">Preview</button>
        <button type="button" id="confirm-type-delete" class="danger" disabled>Delete</button>
        <button type="button" id="cancel-type-delete">Cancel</button>
      </div>
      <h3>Tags</h3>
      <div class="tags-management">
        <div class="tag-create-row">
          <input id="new-tag-name" type="text" placeholder="Tag name" />
          <input id="new-tag-color" type="color" value="#6366f1" />
          <button type="button" id="create-tag-btn">Add Tag</button>
        </div>
        <div id="tags-list" class="tags-list"></div>
      </div>
      <h3>Rules</h3>
      <div class="rules-management">
        <div class="rule-create-row">
          <input id="new-rule-name" type="text" placeholder="Rule name" class="rule-input-name" />
          <select id="new-rule-pattern-type">
            <option value="literal">Literal</option>
            <option value="regex">Regex</option>
            <option value="url">URL</option>
            <option value="email">Email</option>
          </select>
          <input id="new-rule-pattern" type="text" placeholder="Pattern" class="rule-input-pattern" />
          <select id="new-rule-action">
            <option value="tag">Tag</option>
            <option value="delete">Delete</option>
            <option value="notify">Notify</option>
          </select>
          <input id="new-rule-action-value" type="text" placeholder="Tag name" class="rule-input-value" />
          <button type="button" id="create-rule-btn">Add Rule</button>
        </div>
        <div class="settings-actions">
          <button type="button" id="batch-apply-rules">Run Rules on All Clips</button>
        </div>
        <div id="rules-list" class="rules-list"></div>
      </div>
      <h3>About</h3>
      <div class="settings-actions">
        <button type="button" id="copy-error-report">Copy Error Report</button>
        <button type="button" id="verify-database">Verify Database</button>
      </div>
      <div id="verify-result" class="import-result hidden"></div>
      <button type="submit" class="primary">Save Settings</button>
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
const samplingHashInput = document.querySelector<HTMLInputElement>("#sampling-hash")!;

function handleTagClick(tagName: string): void {
  query = `tag:${tagName}`;
  searchInput.value = query;
  selectedIndex = 0;
  void refresh();
}

async function refresh(): Promise<void> {
  const spinner = document.querySelector<HTMLElement>(".search-spinner");
  if (spinner) spinner.hidden = false;

  clips = await searchClips(query, PAGE_SIZE, 0);
  hasMore = clips.length === PAGE_SIZE;
  selectedIndex = Math.min(selectedIndex, Math.max(clips.length - 1, 0));
  renderClips(clips, selectedIndex, list, query, chooseClip, refresh, hasMore, loadMore, handleTagClick);

  if (spinner) spinner.hidden = true;

  if (selectMode) {
    renderSelectCheckboxes();
  }
}

async function loadMore(): Promise<void> {
  if (loadingMore || !hasMore) return;
  loadingMore = true;

  const more = await searchClips(query, PAGE_SIZE, clips.length);
  if (more.length < PAGE_SIZE) {
    hasMore = false;
  }
  clips = clips.concat(more);
  renderClips(clips, selectedIndex, list, query, chooseClip, refresh, hasMore, loadMore, handleTagClick);
  loadingMore = false;
}

async function chooseClip(index: number): Promise<void> {
  const clip = clips[index];
  if (!clip) return;

  if (selectMode) {
    if (selectedIds.has(clip.id)) {
      selectedIds.delete(clip.id);
    } else {
      selectedIds.add(clip.id);
    }
    updateSelectBar();
    renderSelectCheckboxes();
    return;
  }

  try {
    await pasteClip(clip.id);
  } catch (error) {
    permissionBanner.classList.remove("hidden");
    console.error(error);
  }
}

const retentionDaysInput = document.querySelector<HTMLInputElement>("#retention-days")!;

const exportJsonBtn = document.querySelector<HTMLButtonElement>("#export-json")!;
const exportCsvBtn = document.querySelector<HTMLButtonElement>("#export-csv")!;
const importBtn = document.querySelector<HTMLButtonElement>("#import-btn")!;
const importFile = document.querySelector<HTMLInputElement>("#import-file")!;
const importResult = document.querySelector<HTMLDivElement>("#import-result")!;
const importModePanel = document.querySelector<HTMLDivElement>("#import-mode-panel")!;
const importConfirmBtn = document.querySelector<HTMLButtonElement>("#import-confirm")!;
const importCancelBtn = document.querySelector<HTMLButtonElement>("#import-cancel")!;
const pruneNowBtn = document.querySelector<HTMLButtonElement>("#prune-now")!;
const deleteByDateRangeBtn = document.querySelector<HTMLButtonElement>("#delete-by-date-range-btn")!;
const deleteByTypeBtn = document.querySelector<HTMLButtonElement>("#delete-by-type")!;
const diskUsageBtn = document.querySelector<HTMLButtonElement>("#disk-usage")!;
const diskUsageResult = document.querySelector<HTMLDivElement>("#disk-usage-result")!;
const dateRangePanel = document.querySelector<HTMLDivElement>("#date-range-panel")!;
const dateFromInput = document.querySelector<HTMLInputElement>("#date-from")!;
const dateToInput = document.querySelector<HTMLInputElement>("#date-to")!;
const dateRangeCount = document.querySelector<HTMLSpanElement>("#date-range-count")!;
const dateRangePreviewBtn = document.querySelector<HTMLButtonElement>("#date-range-preview")!;
const dateRangeConfirmBtn = document.querySelector<HTMLButtonElement>("#date-range-confirm")!;
const dateRangeCancelBtn = document.querySelector<HTMLButtonElement>("#date-range-cancel")!;
const typeDeletePanel = document.querySelector<HTMLDivElement>("#type-delete-panel")!;
const typeSelect = document.querySelector<HTMLSelectElement>("#type-select")!;
const typeDeleteCount = document.querySelector<HTMLSpanElement>("#type-delete-count")!;
const typeDeletePreviewBtn = document.querySelector<HTMLButtonElement>("#type-delete-preview")!;
const confirmTypeDelete = document.querySelector<HTMLButtonElement>("#confirm-type-delete")!;
const cancelTypeDelete = document.querySelector<HTMLButtonElement>("#cancel-type-delete")!;
const exportDateFrom = document.querySelector<HTMLInputElement>("#export-date-from")!;
const exportDateTo = document.querySelector<HTMLInputElement>("#export-date-to")!;
const exportTypeFilter = document.querySelector<HTMLSelectElement>("#export-type-filter")!;
const importCsvBtn = document.querySelector<HTMLButtonElement>("#import-csv-btn")!;
const importCsvFile = document.querySelector<HTMLInputElement>("#import-csv-file")!;
const newTagName = document.querySelector<HTMLInputElement>("#new-tag-name")!;
const newTagColor = document.querySelector<HTMLInputElement>("#new-tag-color")!;
const createTagBtn = document.querySelector<HTMLButtonElement>("#create-tag-btn")!;
const tagsList = document.querySelector<HTMLDivElement>("#tags-list")!;
const newRuleName = document.querySelector<HTMLInputElement>("#new-rule-name")!;
const newRulePatternType = document.querySelector<HTMLSelectElement>("#new-rule-pattern-type")!;
const newRulePattern = document.querySelector<HTMLInputElement>("#new-rule-pattern")!;
const newRuleAction = document.querySelector<HTMLSelectElement>("#new-rule-action")!;
const newRuleActionValue = document.querySelector<HTMLInputElement>("#new-rule-action-value")!;
const createRuleBtn = document.querySelector<HTMLButtonElement>("#create-rule-btn")!;
const rulesList = document.querySelector<HTMLDivElement>("#rules-list")!;
const batchApplyRulesBtn = document.querySelector<HTMLButtonElement>("#batch-apply-rules")!;
const selectToggle = document.querySelector<HTMLButtonElement>("#select-toggle")!;
const selectBar = document.querySelector<HTMLDivElement>("#select-bar")!;
const selectCount = document.querySelector<HTMLSpanElement>("#select-count")!;
const selectAllBtn = document.querySelector<HTMLButtonElement>("#select-all")!;
const deselectAllBtn = document.querySelector<HTMLButtonElement>("#deselect-all")!;
const deleteSelectedBtn = document.querySelector<HTMLButtonElement>("#delete-selected-btn")!;
const exitSelectBtn = document.querySelector<HTMLButtonElement>("#exit-select")!;

setupSettings(settingsForm, settingsToggle, {
  maxItemsInput, maxSizeInput, trimDedupInput, samplingHashInput, retentionDaysInput
}, () => settings, (updated) => {
  settings = updated;
});

settingsToggle.addEventListener("click", () => {
  if (settingsForm.classList.contains("hidden")) {
    void refreshTags();
    void refreshRules();
  }
});

// ── Export / Import handlers ───────────────────────────────────────

function getExportFilters() {
  const kind = exportTypeFilter.value || null;
  const dateFrom = exportDateFrom.value ? `${exportDateFrom.value}T00:00:00Z` : null;
  const dateTo = exportDateTo.value ? `${exportDateTo.value}T23:59:59Z` : null;
  return { kind, dateFrom, dateTo };
}

exportJsonBtn.addEventListener("click", async () => {
  try {
    showLoading("Exporting JSON…");
    const { kind, dateFrom, dateTo } = getExportFilters();
    const json = await exportToJson(null, kind, dateFrom, dateTo);
    hideLoading();
    const blob = new Blob([json], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `paste-export-${new Date().toISOString().slice(0, 10)}.json`;
    a.click();
    URL.revokeObjectURL(url);
    showToast("JSON export complete", "success");
  } catch (error) {
    hideLoading();
    showToast(`Export failed: ${error}`, "error");
  }
});

exportCsvBtn.addEventListener("click", async () => {
  try {
    showLoading("Exporting CSV…");
    const { kind, dateFrom, dateTo } = getExportFilters();
    const csv = await exportToCsv(null, kind, dateFrom, dateTo);
    hideLoading();
    const blob = new Blob([csv], { type: "text/csv;charset=utf-8" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `paste-export-${new Date().toISOString().slice(0, 10)}.csv`;
    a.click();
    URL.revokeObjectURL(url);
    showToast("CSV export complete", "success");
  } catch (error) {
    hideLoading();
    showToast(`CSV export failed: ${error}`, "error");
  }
});

let pendingImportJson: string | null = null;

importBtn.addEventListener("click", () => {
  importFile.click();
});

importFile.addEventListener("change", async () => {
  const file = importFile.files?.[0];
  if (!file) return;
  try {
    pendingImportJson = await file.text();
    importModePanel.classList.remove("hidden");
    importResult.classList.add("hidden");
  } catch (error) {
    showToast(`Failed to read file: ${error}`, "error");
  }
  importFile.value = "";
});

importConfirmBtn.addEventListener("click", async () => {
  if (!pendingImportJson) return;
  const modeRadio = document.querySelector<HTMLInputElement>('input[name="import-mode"]:checked');
  const mode = modeRadio?.value ?? "merge";
  try {
    showLoading("Importing…");
    const result: ImportResult = await importFromJson(pendingImportJson, mode);
    hideLoading();
    importResult.classList.remove("hidden");
    let msg = `Imported: ${result.added} added, ${result.skipped} skipped, ${result.failed} failed`;
    if (result.versionWarning) {
      msg += ` ⚠ ${result.versionWarning}`;
    }
    importResult.textContent = msg;
    importResult.className = "import-result success";
    showToast(`Import complete (${mode} mode)`, "success");
    await refresh();
  } catch (error) {
    hideLoading();
    importResult.classList.remove("hidden");
    importResult.textContent = `Import failed: ${error}`;
    importResult.className = "import-result error";
    showToast(`Import failed: ${error}`, "error");
  }
  importModePanel.classList.add("hidden");
  pendingImportJson = null;
});

importCancelBtn.addEventListener("click", () => {
  importModePanel.classList.add("hidden");
  pendingImportJson = null;
});

importCsvBtn.addEventListener("click", () => {
  importCsvFile.click();
});

importCsvFile.addEventListener("change", async () => {
  const file = importCsvFile.files?.[0];
  if (!file) return;
  try {
    const csv = await file.text();
    const confirmed = await showConfirm("Import CSV? This will merge new items and skip duplicates.");
    if (!confirmed) return;
    showLoading("Importing CSV…");
    const result = await importFromCsv(csv, "merge");
    hideLoading();
    importResult.classList.remove("hidden");
    importResult.textContent = `Imported: ${result.added} added, ${result.skipped} skipped, ${result.failed} failed`;
    importResult.className = "import-result success";
    showToast("CSV import complete", "success");
    await refresh();
  } catch (error) {
    hideLoading();
    showToast(`CSV import failed: ${error}`, "error");
  }
  importCsvFile.value = "";
});

// ── Tags management ──────────────────────────────────────────────

async function refreshTags(): Promise<void> {
  const tags = await listTags();
  tagsList.innerHTML = tags
    .map(
      (t) => `
    <div class="tag-item" data-id="${t.id}">
      <span class="tag-color" style="background:${t.color ?? "#6366f1"}"></span>
      <span class="tag-name">${t.name}</span>
      <button type="button" class="tag-delete" data-id="${t.id}">&times;</button>
    </div>`
    )
    .join("");

  tagsList.querySelectorAll(".tag-delete").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const id = Number((btn as HTMLElement).dataset.id);
      const confirmed = await showConfirm("Delete this tag? It will be removed from all clips.");
      if (!confirmed) return;
      await deleteTag(id);
      await refreshTags();
    });
  });
}

createTagBtn.addEventListener("click", async () => {
  const name = newTagName.value.trim();
  if (!name) return;
  try {
    await createTag(name, newTagColor.value);
    newTagName.value = "";
    await refreshTags();
    showToast(`Tag "${name}" created`, "success");
  } catch (error) {
    showToast(`Failed to create tag: ${error}`, "error");
  }
});

// ── Rules management ─────────────────────────────────────────────

async function refreshRules(): Promise<void> {
  const rules = await listRules();
  rulesList.innerHTML = rules
    .map(
      (r) => `
    <div class="rule-item" data-id="${r.id}">
      <span class="rule-name">${r.name}</span>
      <span class="rule-detail">${r.patternType}: ${r.pattern} &rarr; ${r.action}${r.actionValue ? `(${r.actionValue})` : ""}</span>
      <label class="rule-toggle"><input type="checkbox" ${r.enabled ? "checked" : ""} data-toggle="${r.id}" /> On</label>
      <button type="button" class="rule-delete" data-id="${r.id}">&times;</button>
    </div>`
    )
    .join("");

  rulesList.querySelectorAll(".rule-delete").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const id = Number((btn as HTMLElement).dataset.id);
      await deleteRule(id);
      await refreshRules();
    });
  });

  rulesList.querySelectorAll("input[data-toggle]").forEach((input) => {
    input.addEventListener("change", async () => {
      const el = input as HTMLInputElement;
      const id = Number(el.dataset.toggle);
      const rule = rules.find((r) => r.id === id);
      if (!rule) return;
      await updateRule(
        rule.id, rule.name, rule.pattern, rule.patternType,
        rule.action, rule.actionValue, el.checked, rule.priority
      );
    });
  });
}

createRuleBtn.addEventListener("click", async () => {
  const name = newRuleName.value.trim();
  const pattern = newRulePattern.value.trim();
  if (!name || !pattern) return;
  try {
    await createRule(name, pattern, newRulePatternType.value, newRuleAction.value, newRuleActionValue.value || null);
    newRuleName.value = "";
    newRulePattern.value = "";
    newRuleActionValue.value = "";
    await refreshRules();
    showToast(`Rule "${name}" created`, "success");
  } catch (error) {
    showToast(`Failed to create rule: ${error}`, "error");
  }
});

batchApplyRulesBtn.addEventListener("click", async () => {
  try {
    showLoading("Running rules on all clips…");
    const processed = await batchApplyRules();
    hideLoading();
    showToast(`Rules applied to ${processed} clips`, "success");
    await refresh();
  } catch (error) {
    hideLoading();
    showToast(`Failed to run rules: ${error}`, "error");
  }
});

// ── Multi-select mode ────────────────────────────────────────────

function updateSelectBar(): void {
  selectCount.textContent = `${selectedIds.size} selected`;
  deleteSelectedBtn.disabled = selectedIds.size === 0;
}

function renderSelectCheckboxes(): void {
  list.querySelectorAll(".clip").forEach((item) => {
    const el = item as HTMLElement;
    const id = el.dataset.id;
    if (!id) return;
    const existing = el.querySelector(".select-check");
    if (selectMode) {
      if (!existing) {
        const cb = document.createElement("span");
        cb.className = "select-check";
        cb.textContent = selectedIds.has(id) ? "☑" : "☐";
        el.prepend(cb);
      } else {
        existing.textContent = selectedIds.has(id) ? "☑" : "☐";
      }
      el.classList.toggle("select-checked", selectedIds.has(id));
    } else {
      existing?.remove();
      el.classList.remove("select-checked");
    }
  });
}

function enterSelectMode(): void {
  selectMode = true;
  selectedIds.clear();
  selectBar.classList.remove("hidden");
  selectToggle.textContent = "Cancel";
  updateSelectBar();
  renderSelectCheckboxes();
}

function exitSelectMode(): void {
  selectMode = false;
  selectedIds.clear();
  selectBar.classList.add("hidden");
  selectToggle.textContent = "Select";
  renderSelectCheckboxes();
}

selectToggle.addEventListener("click", () => {
  if (selectMode) {
    exitSelectMode();
  } else {
    enterSelectMode();
  }
});

exitSelectBtn.addEventListener("click", () => {
  exitSelectMode();
});

selectAllBtn.addEventListener("click", () => {
  clips.forEach((c) => selectedIds.add(c.id));
  updateSelectBar();
  renderSelectCheckboxes();
});

deselectAllBtn.addEventListener("click", () => {
  selectedIds.clear();
  updateSelectBar();
  renderSelectCheckboxes();
});

deleteSelectedBtn.addEventListener("click", async () => {
  if (selectedIds.size === 0) return;
  const confirmed = await showConfirm(`Delete ${selectedIds.size} selected items?`);
  if (!confirmed) return;
  try {
    showLoading("Deleting…");
    const deleted = await deleteSelected([...selectedIds]);
    hideLoading();
    showToast(`Deleted ${deleted} items`, "success");
    exitSelectMode();
    await refresh();
  } catch (error) {
    hideLoading();
    showToast(`Delete failed: ${error}`, "error");
  }
});

const copyErrorReportBtn = document.querySelector<HTMLButtonElement>("#copy-error-report")!;
copyErrorReportBtn.addEventListener("click", async () => {
  try {
    const usage = await getDiskUsage();
    const s = settings;
    const report = [
      "=== Paste Error Report ===",
      `Version: 1.0.9`,
      `Platform: ${navigator.platform}`,
      `User Agent: ${navigator.userAgent}`,
      `Total Items: ${usage.totalItems}`,
      `Total Storage: ${(usage.totalBytes / 1024 / 1024).toFixed(1)} MB`,
      `Settings: maxItems=${s.maxItems}, maxPayload=${Math.round(s.maxPayloadBytes / 1024 / 1024)}MB, retention=${s.retentionDays}d, samplingHash=${s.useSamplingHash}`,
      `Log Directory: ~/Library/Application Support/Paste/logs/`,
      `Time: ${new Date().toISOString()}`,
    ].join("\n");
    await navigator.clipboard.writeText(report);
    showToast("Error report copied to clipboard", "success");
  } catch (error) {
    showToast(`Failed to copy report: ${error}`, "error");
  }
});

const verifyDatabaseBtn = document.querySelector<HTMLButtonElement>("#verify-database")!;
const verifyResult = document.querySelector<HTMLDivElement>("#verify-result")!;
verifyDatabaseBtn.addEventListener("click", async () => {
  try {
    showLoading("Verifying database…");
    const report = await verifyDatabase();
    hideLoading();
    verifyResult.classList.remove("hidden");
    if (report.ok) {
      verifyResult.textContent = `Database OK. Cleaned ${report.orphanedBlobs} orphaned blob(s).`;
      verifyResult.className = "import-result success";
    } else {
      verifyResult.textContent = `Issue found: ${report.message}. Cleaned ${report.orphanedBlobs} orphaned blob(s).`;
      verifyResult.className = "import-result error";
    }
  } catch (error) {
    hideLoading();
    showToast(`Verification failed: ${error}`, "error");
  }
});

// ── Data Management handlers ───────────────────────────────────────

pruneNowBtn.addEventListener("click", async () => {
  try {
    const count = await countPrunable(settings.retentionDays);
    if (count === 0) {
      showToast("Nothing to clean up", "info");
      return;
    }
    const confirmed = await showConfirm(
      `Delete ${count} items older than ${settings.retentionDays} days? Pinned items are preserved.`
    );
    if (!confirmed) return;
    const deleted = await autoPrune(settings.retentionDays);
    showToast(`Deleted ${deleted} items`, "success");
    await refresh();
  } catch (error) {
    showToast(`Prune failed: ${error}`, "error");
  }
});

// ── Delete by Date Range ───────────────────────────────────────────

deleteByDateRangeBtn.addEventListener("click", () => {
  typeDeletePanel.classList.add("hidden");
  dateRangePanel.classList.toggle("hidden");
  dateRangeCount.textContent = "";
  dateRangeConfirmBtn.disabled = true;
});

dateRangeCancelBtn.addEventListener("click", () => {
  dateRangePanel.classList.add("hidden");
});

dateRangePreviewBtn.addEventListener("click", async () => {
  const from = dateFromInput.value;
  const to = dateToInput.value;
  if (!from || !to) {
    showToast("Please select both dates", "error");
    return;
  }
  try {
    const count = await countByDateRange(`${from}T00:00:00Z`, `${to}T23:59:59Z`);
    dateRangeCount.textContent = `${count} items will be deleted`;
    dateRangeConfirmBtn.disabled = count === 0;
  } catch (error) {
    showToast(`Preview failed: ${error}`, "error");
  }
});

dateRangeConfirmBtn.addEventListener("click", async () => {
  const from = dateFromInput.value;
  const to = dateToInput.value;
  if (!from || !to) return;
  const countText = dateRangeCount.textContent;
  const confirmed = await showConfirm(`${countText}. Are you sure?`);
  if (!confirmed) return;
  try {
    const deleted = await deleteByDateRange(`${from}T00:00:00Z`, `${to}T23:59:59Z`);
    showToast(`Deleted ${deleted} items`, "success");
    dateRangePanel.classList.add("hidden");
    await refresh();
  } catch (error) {
    showToast(`Delete failed: ${error}`, "error");
  }
});

// ── Delete by Type ─────────────────────────────────────────────────

deleteByTypeBtn.addEventListener("click", () => {
  dateRangePanel.classList.add("hidden");
  typeDeletePanel.classList.toggle("hidden");
  typeDeleteCount.textContent = "";
  confirmTypeDelete.disabled = true;
});

cancelTypeDelete.addEventListener("click", () => {
  typeDeletePanel.classList.add("hidden");
});

typeDeletePreviewBtn.addEventListener("click", async () => {
  const kind = typeSelect.value;
  try {
    const count = await countByType(kind);
    typeDeleteCount.textContent = `${count} items`;
    confirmTypeDelete.disabled = count === 0;
  } catch (error) {
    showToast(`Preview failed: ${error}`, "error");
  }
});

confirmTypeDelete.addEventListener("click", async () => {
  const kind = typeSelect.value;
  const countText = typeDeleteCount.textContent;
  const confirmed = await showConfirm(`Delete ${countText} of type "${kind}"? Pinned items are preserved.`);
  if (!confirmed) return;
  try {
    const deleted = await deleteByType(kind);
    showToast(`Deleted ${deleted} items of type "${kind}"`, "success");
    typeDeletePanel.classList.add("hidden");
    await refresh();
  } catch (error) {
    showToast(`Delete failed: ${error}`, "error");
  }
});

// ── Disk Usage ─────────────────────────────────────────────────────

diskUsageBtn.addEventListener("click", async () => {
  try {
    const usage: DiskUsage = await getDiskUsage();
    diskUsageResult.classList.remove("hidden");
    const totalMB = (usage.totalBytes / 1024 / 1024).toFixed(1);
    let html = `<strong>${usage.totalItems} items</strong> · ${totalMB} MB total`;
    if (usage.byType.length > 0) {
      html += "<br>By type: ";
      html += usage.byType
        .map((t) => `${t.kind}: ${t.count} (${(t.bytes / 1024 / 1024).toFixed(1)} MB)`)
        .join(" · ");
    }
    if (usage.byAge.length > 0) {
      html += "<br>By age: ";
      html += usage.byAge.map((a) => `${a.range}: ${a.count}`).join(" · ");
    }
    if (usage.totalBytes > 1024 * 1024 * 1024) {
      html += '<br><span class="disk-warning">Storage exceeds 1 GB — consider running auto-prune or deleting old items.</span>';
    } else if (usage.totalBytes > 512 * 1024 * 1024) {
      html += '<br><span class="disk-notice">Storage exceeds 512 MB — review old items for cleanup.</span>';
    }
    const oldItems = usage.byAge.find((a) => a.range === ">90 days");
    if (oldItems && oldItems.count > 100) {
      html += `<br><span class="disk-notice">${oldItems.count} items older than 90 days — consider enabling auto-prune.</span>`;
    }
    diskUsageResult.innerHTML = html;
    diskUsageResult.className = "import-result success";
  } catch (error) {
    showToast(`Failed to load disk usage: ${error}`, "error");
  }
});

let searchDebounce: ReturnType<typeof setTimeout>;
let selectMode = false;
const selectedIds = new Set<string>();

// ── Search history ────────────────────────────────────────────────

const SEARCH_HISTORY_KEY = "paste-search-history";
const MAX_SEARCH_HISTORY = 20;

function getSearchHistory(): string[] {
  try {
    return JSON.parse(localStorage.getItem(SEARCH_HISTORY_KEY) ?? "[]");
  } catch {
    return [];
  }
}

function saveSearchToHistory(q: string): void {
  const trimmed = q.trim();
  if (!trimmed) return;
  const history = getSearchHistory().filter((h) => h !== trimmed);
  history.unshift(trimmed);
  if (history.length > MAX_SEARCH_HISTORY) history.pop();
  localStorage.setItem(SEARCH_HISTORY_KEY, JSON.stringify(history));
}

const searchHistoryEl = document.createElement("div");
searchHistoryEl.id = "search-history";
searchHistoryEl.className = "search-history hidden";
document.querySelector(".search-row")?.after(searchHistoryEl);

function showSearchHistory(): void {
  const history = getSearchHistory();
  if (history.length === 0) {
    searchHistoryEl.classList.add("hidden");
    return;
  }
  searchHistoryEl.innerHTML = history
    .slice(0, 8)
    .map((h) => `<div class="search-history-item">${h}</div>`)
    .join("");
  searchHistoryEl.classList.remove("hidden");

  searchHistoryEl.querySelectorAll(".search-history-item").forEach((el) => {
    el.addEventListener("click", () => {
      query = el.textContent ?? "";
      searchInput.value = query;
      searchHistoryEl.classList.add("hidden");
      selectedIndex = 0;
      void refresh();
    });
  });
}

searchInput.addEventListener("focus", () => {
  if (!searchInput.value) {
    showSearchHistory();
  }
});

document.addEventListener("click", (e) => {
  if (!searchHistoryEl.contains(e.target as Node) && e.target !== searchInput) {
    searchHistoryEl.classList.add("hidden");
  }
});
searchInput.addEventListener("input", () => {
  clearTimeout(searchDebounce);
  searchDebounce = setTimeout(async () => {
    query = searchInput.value;
    if (query.trim()) {
      saveSearchToHistory(query);
    }
    selectedIndex = 0;
    await refresh();
  }, 150);
});

permissionOpen.addEventListener("click", () => {
  void openAccessibilitySettings();
});

// ── Context Menu ──────────────────────────────────────────────────

const contextMenu = document.querySelector<HTMLDivElement>("#context-menu")!;
let contextClipId: string | null = null;

function showContextMenu(x: number, y: number, clipId: string): void {
  contextClipId = clipId;
  const clip = clips.find((c) => c.id === clipId);
  if (!clip) return;

  const pinBtn = contextMenu.querySelector("[data-action='pin']")!;
  pinBtn.textContent = clip.isPinned ? "Unpin" : "Pin";

  contextMenu.style.left = `${x}px`;
  contextMenu.style.top = `${y}px`;
  contextMenu.classList.remove("hidden");

  const closeMenu = (e: Event) => {
    if (!contextMenu.contains(e.target as Node)) {
      contextMenu.classList.add("hidden");
      document.removeEventListener("click", closeMenu);
    }
  };
  setTimeout(() => document.addEventListener("click", closeMenu), 0);
}

contextMenu.addEventListener("click", async (event) => {
  const btn = (event.target as HTMLElement).closest("button");
  if (!btn || !contextClipId) return;
  const action = btn.dataset.action;
  const clipId = contextClipId;
  contextMenu.classList.add("hidden");

  switch (action) {
    case "paste":
      await pasteClip(clipId);
      break;
    case "copy": {
      const clip = clips.find((c) => c.id === clipId);
      if (clip) {
        await navigator.clipboard.writeText(clip.textPreview);
        showToast("Copied to clipboard", "success");
      }
      break;
    }
    case "pin": {
      const clip = clips.find((c) => c.id === clipId);
      if (clip) {
        await pinClip(clipId, !clip.isPinned);
        await refresh();
      }
      break;
    }
    case "tags":
      showToast("Tag editing: use Settings > Tags", "info");
      break;
    case "export": {
      const clip = clips.find((c) => c.id === clipId);
      if (clip) {
        const json = await exportToJson([clipId]);
        const blob = new Blob([json], { type: "application/json" });
        const url = URL.createObjectURL(blob);
        const a = document.createElement("a");
        a.href = url;
        a.download = `clip-${clipId.slice(0, 8)}.json`;
        a.click();
        URL.revokeObjectURL(url);
        showToast("Item exported", "success");
      }
      break;
    }
    case "delete":
      if (await showConfirm("Delete this clip?")) {
        await deleteClip(clipId);
        await refresh();
      }
      break;
  }
});

list.addEventListener("contextmenu", (event) => {
  event.preventDefault();
  const clipEl = (event.target as HTMLElement).closest(".clip") as HTMLElement;
  if (!clipEl) return;
  const clipId = clipEl.dataset.id;
  if (clipId) {
    showContextMenu(event.clientX, event.clientY, clipId);
  }
});

// ── Keyboard shortcuts ───────────────────────────────────────────

document.addEventListener("keydown", async (event) => {
  const isInInput = (event.target as HTMLElement).tagName === "INPUT" || (event.target as HTMLElement).tagName === "SELECT";

  if (event.key === "Escape") {
    if (selectMode) {
      exitSelectMode();
      return;
    }
    if (!contextMenu.classList.contains("hidden")) {
      contextMenu.classList.add("hidden");
      return;
    }
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

  if (event.key === "Delete" || event.key === "Backspace") {
    if (!isInInput && clips[selectedIndex]) {
      event.preventDefault();
      await deleteClip(clips[selectedIndex].id);
      await refresh();
      return;
    }
  }

  // Cmd+F / Ctrl+F: focus search
  if ((event.metaKey || event.ctrlKey) && event.key === "f") {
    event.preventDefault();
    searchInput.focus();
    searchInput.select();
    return;
  }

  // Cmd+P / Ctrl+P: pin/unpin selected
  if ((event.metaKey || event.ctrlKey) && event.key === "p") {
    if (!isInInput && clips[selectedIndex]) {
      event.preventDefault();
      await pinClip(clips[selectedIndex].id, !clips[selectedIndex].isPinned);
      await refresh();
      return;
    }
  }

  // Shift+Enter: toggle multi-select mode
  if (event.key === "Enter" && event.shiftKey) {
    event.preventDefault();
    if (selectMode) {
      exitSelectMode();
    } else {
      enterSelectMode();
    }
    return;
  }

  if (/^[1-9]$/.test(event.key) && !event.metaKey && !event.ctrlKey && !event.altKey && !isInInput) {
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
renderSettings(settings, { maxItemsInput, maxSizeInput, trimDedupInput, samplingHashInput, retentionDaysInput });
if (!(await hasAccessibilityPermission())) {
  permissionBanner.classList.remove("hidden");
}
await refresh();
await listen("clips-changed", () => {
  void refresh();
});
