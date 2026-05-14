import { type ClipSummary, deleteClip, getClipThumbnail, pinClip } from "../lib/commands";

export function escapeHtml(value: string): string {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#039;");
}

function createKindBadge(kind: string): HTMLSpanElement {
  const span = document.createElement("span");
  span.className = "clip-kind";
  span.textContent = kind.toUpperCase();
  return span;
}

function clipKindHtml(kind: ClipSummary["kind"], id: string): string {
  if (kind === "image") {
    return `<img class="clip-thumb" data-clip-id="${escapeHtml(id)}" alt="thumbnail" />`;
  }
  const label = (() => {
    switch (kind) {
      case "file_url": return "FILE";
      case "html": return "HTML";
      case "rtf": return "RTF";
      case "text": return "TXT";
      default: return "TXT";
    }
  })();
  return `<span class="clip-kind">${label}</span>`;
}

export function updateSelection(
  list: HTMLOListElement,
  oldIndex: number,
  newIndex: number
): void {
  const items = list.querySelectorAll<HTMLElement>(".clip");
  items[oldIndex]?.classList.remove("selected");
  items[newIndex]?.classList.add("selected");
}

async function loadThumbnails(list: HTMLOListElement): Promise<void> {
  const thumbs = list.querySelectorAll<HTMLImageElement>(".clip-thumb:not([src])");
  for (const img of thumbs) {
    const clipId = img.dataset.clipId;
    if (!clipId) continue;
    try {
      const dataUrl = await getClipThumbnail(clipId);
      if (dataUrl) {
        img.src = dataUrl;
      } else {
        img.replaceWith(createKindBadge("image"));
      }
    } catch {
      img.replaceWith(createKindBadge("image"));
    }
  }
}

/**
 * Highlight matching substrings of `query` within `text`, returning safe HTML.
 * Each matched character is wrapped in a <mark> tag.
 */
function highlightMatch(text: string, query: string): string {
  if (!query.trim()) return escapeHtml(text);

  const escaped = escapeHtml(text);
  const q = query.trim();
  // Build case-insensitive matching pattern
  const pattern = q.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const regex = new RegExp(`(${pattern})`, "gi");
  return escaped.replace(regex, "<mark>$1</mark>");
}

function renderEmptyState(query: string): HTMLElement {
  const empty = document.createElement("li");
  empty.className = "empty-state";

  if (query) {
    empty.textContent = "No matching clipboard items";
    return empty;
  }

  empty.innerHTML = `
    <div class="empty-guide">
      <h2>Welcome to Paste</h2>
      <p>Your clipboard history appears here automatically.</p>
      <div class="empty-steps">
        <div class="empty-step">
          <span class="empty-step-icon">1</span>
          <span>Copy text or images in any app with <kbd>Cmd+C</kbd></span>
        </div>
        <div class="empty-step">
          <span class="empty-step-icon">2</span>
          <span>Press <kbd>Cmd+Shift+V</kbd> to open Paste</span>
        </div>
        <div class="empty-step">
          <span class="empty-step-icon">3</span>
          <span>Type to search, then press <kbd>Enter</kbd> to paste</span>
        </div>
      </div>
    </div>
  `;
  return empty;
}

export function renderClips(
  clips: ClipSummary[],
  selectedIndex: number,
  list: HTMLOListElement,
  query: string,
  onChoose: (index: number) => void,
  onRefresh: () => Promise<void>,
  hasMore = false,
  onLoadMore?: () => Promise<void>
): void {
  list.innerHTML = "";

  if (clips.length === 0) {
    list.append(renderEmptyState(query));
    return;
  }

  clips.forEach((clip, index) => {
    const item = document.createElement("li");
    item.className = `clip ${index === selectedIndex ? "selected" : ""}`;
    item.dataset.id = clip.id;

    const timestamp = new Date(clip.createdAt).toLocaleString();
    const fullPreview = clip.textPreview || "(empty item)";
    const displayPreview = highlightMatch(fullPreview, query);

    const number = index < 9 ? `${index + 1}` : "";
    item.innerHTML = `
      <span class="clip-number">${number}</span>
      ${clipKindHtml(clip.kind, clip.id)}
      <span class="clip-body" title="${escapeHtml(fullPreview)} · ${timestamp}${clip.isPinned ? " · Pinned" : ""}">
        <strong>${displayPreview}</strong>
        <small>${timestamp}${clip.isPinned ? " · Pinned" : ""}</small>
      </span>
      <button class="pin" title="${clip.isPinned ? "Unpin" : "Pin"}">${clip.isPinned ? "Unpin" : "Pin"}</button>
      <button class="delete" title="Delete">Delete</button>
    `;

    item.addEventListener("mouseenter", () => {
      if (selectedIndex !== index) {
        const old = selectedIndex;
        selectedIndex = index;
        updateSelection(list, old, index);
      }
    });
    item.addEventListener("dblclick", () => onChoose(index));
    item.querySelector<HTMLButtonElement>(".pin")?.addEventListener("click", async (event) => {
      event.stopPropagation();
      await pinClip(clip.id, !clip.isPinned);
      await onRefresh();
    });
    item.querySelector<HTMLButtonElement>(".delete")?.addEventListener("click", async (event) => {
      event.stopPropagation();
      await deleteClip(clip.id);
      await onRefresh();
    });

    list.append(item);
  });

  // "Load more" button
  if (hasMore && onLoadMore) {
    const loadMoreItem = document.createElement("li");
    loadMoreItem.className = "load-more-item";
    const btn = document.createElement("button");
    btn.className = "load-more-btn";
    btn.textContent = "Load more...";
    btn.addEventListener("click", () => {
      void onLoadMore();
    });
    loadMoreItem.append(btn);
    list.append(loadMoreItem);
  }

  void loadThumbnails(list);
}
