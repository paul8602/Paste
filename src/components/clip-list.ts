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

export function renderClips(
  clips: ClipSummary[],
  selectedIndex: number,
  list: HTMLOListElement,
  query: string,
  onChoose: (index: number) => void,
  onRefresh: () => Promise<void>
): void {
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
      ${clipKindHtml(clip.kind, clip.id)}
      <span class="clip-body">
        <strong>${escapeHtml(clip.textPreview || "(empty item)")}</strong>
        <small>${new Date(clip.createdAt).toLocaleString()}${clip.isPinned ? " · Pinned" : ""}</small>
      </span>
      <button class="pin" title="Pin">${clip.isPinned ? "Unpin" : "Pin"}</button>
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

  void loadThumbnails(list);
}
