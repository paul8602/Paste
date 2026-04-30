# Paste

Paste is a keyboard-first clipboard history app for macOS, built with Tauri.
It works like a lightweight mix of Maccy and Paste: press a shortcut, search your clipboard history, then paste an item with the keyboard.

## Features

- Menu bar app that stays available in the macOS status bar
- Hidden Dock icon for a cleaner background-app experience
- Global shortcut: `Cmd+Shift+V`
- Spotlight-style floating clipboard panel
- Fuzzy search over clipboard previews
- Number keys `1` through `9` paste visible results directly
- Clipboard history for plain text, RTF/HTML, images, and file URLs
- SQLite-backed history with blob storage for larger payloads
- Pin, delete, deduplication, and basic retention settings
- Accessibility permission helper for simulated paste

## Usage

1. Install the latest DMG from `dist/Paste_0.1.0_x64.dmg` or build it locally.
2. Open Paste from Applications.
3. Grant Accessibility permission when prompted so Paste can send `Cmd+V` to the active app.
4. Copy content from any app.
5. Press `Cmd+Shift+V` to open Paste.
6. Search, use arrow keys, press `Enter`, or press `1`-`9` to paste a visible item.

Paste runs from the menu bar. Click the menu bar icon to show the panel, or use the menu to quit.

## Permissions

Paste needs macOS Accessibility permission to paste into the currently focused application.
Without this permission, it can still collect clipboard history, but simulated paste will fail.

To grant permission manually:

1. Open System Settings.
2. Go to Privacy & Security.
3. Open Accessibility.
4. Enable Paste.

## Development

Install Node.js, Rust, and the Tauri prerequisites, then run:

```sh
npm install
npm run tauri:dev
```

## Build

Create a production build and DMG:

```sh
npm run tauri:build
```

The DMG is produced by Tauri under `src-tauri/target/release/bundle/dmg/`.
This project also copies release DMGs into `dist/` when doing local packaging work.

## Project Structure

- `src/`: Tauri frontend panel UI
- `src/lib/commands.ts`: frontend command wrappers
- `src-tauri/src/`: Rust backend, clipboard watcher, storage, search, and macOS bridge
- `src-tauri/icons/`: app icon assets
- `src-tauri/tauri.conf.json`: Tauri application and bundle configuration

## Tech Stack

- Tauri 2
- TypeScript and Vite
- Rust
- SQLite via `rusqlite`
- macOS `NSPasteboard` and `CGEvent` integration
