import { type AppSettings, saveSettings } from "../lib/commands";

export interface SettingsElements {
  maxItemsInput: HTMLInputElement;
  maxSizeInput: HTMLInputElement;
  trimDedupInput: HTMLInputElement;
}

export function renderSettings(settings: AppSettings, elements: SettingsElements): void {
  elements.maxItemsInput.value = String(settings.maxItems);
  elements.maxSizeInput.value = String(Math.round(settings.maxPayloadBytes / 1024 / 1024));
  elements.trimDedupInput.checked = settings.trimWhitespaceForTextDedup;
}

export function setupSettings(
  form: HTMLFormElement,
  toggle: HTMLButtonElement,
  elements: SettingsElements,
  getSettings: () => AppSettings,
  onSaved: (settings: AppSettings) => void
): void {
  toggle.addEventListener("click", () => {
    form.classList.toggle("hidden");
    renderSettings(getSettings(), elements);
  });

  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    const settings = await saveSettings({
      maxItems: Number(elements.maxItemsInput.value),
      maxPayloadBytes: Number(elements.maxSizeInput.value) * 1024 * 1024,
      trimWhitespaceForTextDedup: elements.trimDedupInput.checked
    });
    form.classList.add("hidden");
    onSaved(settings);
  });
}
