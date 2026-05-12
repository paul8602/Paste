# Windows 平台支持方案

## 背景

Paste 当前仅支持 macOS。代码已有 `ClipboardBridge` 平台抽象层（`src-tauri/src/macos_bridge/`），非 macOS 平台编译时使用空实现 stub。需要将 stub 替换为真正的 Windows 实现。

---

## 需要修改的文件

| 文件 | 修改内容 |
|------|----------|
| `src-tauri/Cargo.toml` | 添加 Windows 平台依赖 |
| `src-tauri/src/macos_bridge/mod.rs` | 重构平台选择逻辑，添加 Windows 分支 |
| `src-tauri/src/macos_bridge/platform_win.rs` | **新建** — Windows 平台实现 |
| `src-tauri/src/lib.rs` | 为 `open_accessibility_settings` 添加 cfg 守卫 |
| `src-tauri/tauri.conf.json` | 添加 Windows bundle 配置 |
| `package.json` | 添加 Windows 构建脚本 |
| `.github/workflows/release-windows.yml` | **新建** — Windows CI/CD |

---

## 1. Windows ClipboardBridge 实现（核心）

**新建文件**：`src-tauri/src/macos_bridge/platform_win.rs`

使用 `windows` crate 调用 Win32 API：

### change_count()
```rust
// 使用 GetClipboardSequenceNumber() 检测剪贴板变化
// 返回值每次剪贴板内容改变时递增
extern "system" { fn GetClipboardSequenceNumber() -> u32; }
```

### read_clip()
```rust
// OpenClipboard → GetClipboardData → GlobalLock → 读取数据 → GlobalUnlock → CloseClipboard
// 按优先级读取：CF_HDROP(文件) → CF_DIB/CF_BITMAP(图片) → CF_HTML → CF_UNICODETEXT
```

### write_clip()
```rust
// OpenClipboard → EmptyClipboard → SetClipboardData → CloseClipboard
// 文本用 CF_UNICODETEXT，图片用 CF_DIB
```

### has_accessibility_permission()
```rust
// Windows 不需要辅助功能权限即可模拟按键
// 始终返回 true
```

### send_paste_keystroke()
```rust
// 使用 SendInput 模拟 Ctrl+V
// INPUT 结构：KeyDown(VK_CONTROL) → KeyDown('V') → KeyUp('V') → KeyUp(VK_CONTROL)
```

### Windows 平台依赖
```toml
[target.'cfg(target_os = "windows")'.dependencies]
windows = { version = "0", features = [
    "Win32_System_DataExchange",
    "Win32_System_Memory",
    "Win32_System_Input",
    "Win32_UI_Input_KeyboardAndMouse",
    "Win32_UI_Shell",
    "Win32_Storage_FileSystem",
]}
```

---

## 2. 平台选择逻辑重构

**修改**：`src-tauri/src/macos_bridge/mod.rs`

当前结构：
```
mod.rs         — 共享类型 + cfg 选择
platform.rs    — macOS 实现（仅 cfg(target_os = "macos")）
```

改为：
```
mod.rs           — 共享类型 + cfg 选择
platform.rs      — macOS 实现（cfg(target_os = "macos")）
platform_win.rs  — Windows 实现（cfg(target_os = "windows")）
```

cfg 选择逻辑：
```rust
#[cfg(target_os = "macos")]
mod platform;

#[cfg(target_os = "windows")]
mod platform_win;

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod platform { /* 现有 stub */ }

// 统一导出
#[cfg(target_os = "windows")]
pub use platform_win::ClipboardBridge;
#[cfg(not(target_os = "windows"))]
pub use platform::ClipboardBridge;
```

---

## 3. lib.rs 平台守卫

**修改**：`src-tauri/src/lib.rs`

`open_accessibility_settings` 命令需要按平台分离：

```rust
#[tauri::command]
fn open_accessibility_settings() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg("x-apple.systempreferences:...")
            .status()?;
    }
    #[cfg(target_os = "windows")]
    {
        // Windows 不需要此权限，或打开"设置 > 隐私 > 辅助功能"
        Command::new("ms-settings:easeofaccess-keyboard").status()?;
    }
    Ok(())
}
```

`ActivationPolicy::Accessory` 仅 macOS 需要，已有 cfg 守卫。

---

## 4. tauri.conf.json

添加 Windows bundle 配置：
```json
"windows": {
  "webviewInstallMode": { "type": "embedBootstrapper" }
}
```

更新产品描述为跨平台。

---

## 5. CI/CD

**新建**：`.github/workflows/release-windows.yml`

```yaml
on:
  workflow_dispatch:
  push:
    tags: ["v*"]

jobs:
  build-windows:
    runs-on: windows-latest
    steps:
      - checkout, setup node, setup rust
      - npm ci && npm run tauri:build
      - upload MSI/NSIS artifact
      - create GitHub release (on tag)
```

---

## 6. package.json 构建脚本

```json
"tauri:build:win": "tauri build"
```

（Windows 无需指定 target，默认构建当前平台）

---

## 已知限制

| 项目 | 说明 |
|------|------|
| 图片格式 | Windows 剪贴板用 CF_DIB/BMP，macOS 用 PNG/TIFF。存储格式会不同 |
| HTML | Windows CF_HTML 有额外的 Header 格式，需要解析 |
| 系统托盘 | Windows 托盘行为与 macOS 略有差异（左键/右键） |
| 全局快捷键 | `Super+Shift+V` 在 Windows 上是 Win+Shift+V，与系统快捷键可能冲突，可改为 `Ctrl+Shift+V` |
| 窗口透明 | Windows 上透明窗口需要 DWM 支持，部分旧系统可能不完美 |

---

## 验证方式

1. `cargo build --target x86_64-pc-windows-msvc` 编译通过（或在 Windows 机器上 `cargo build`）
2. `cargo test` 所有测试通过（新增 Windows 平台测试）
3. 在 Windows 上 `npm run tauri:dev` 启动，验证：
   - 剪贴板监听正常工作（复制文本后列表更新）
   - 粘贴功能正常（Ctrl+V 到其他应用）
   - 全局快捷键响应
   - 系统托盘图标显示和菜单工作
