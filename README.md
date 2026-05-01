# Paste

Paste 是一款 macOS 剪贴板历史管理器，以键盘操作为核心，基于 Tauri 2 构建。它结合了 Rust 后端（系统级剪贴板访问、存储和搜索）与 TypeScript/Vite 前端（UI 面板），类似于 Maccy 和 Paste 的轻量混合体——按下快捷键，搜索剪贴板历史，然后用键盘粘贴选中的内容。

## 功能特性

- **菜单栏常驻应用**——隐藏 Dock 图标，在 macOS 状态栏中随时可用
- **全局快捷键**：`Cmd+Shift+V` 快速呼出面板
- **Spotlight 风格浮动面板**——毛玻璃暗色主题，始终置顶
- **模糊搜索**——基于 skim 算法，智能大小写匹配
- **快捷粘贴**——数字键 `1` 至 `9` 可直接粘贴对应位置的历史项
- **多类型支持**——纯文本、RTF、HTML、PNG/TIFF 图片、文件 URL
- **SQLite 存储**——WAL 模式，大体积内容（>256KB）自动存储为磁盘 Blob 文件
- **去重机制**——基于 SHA-256 哈希，文本支持可选的空白字符裁剪
- **置顶与删除**——常用内容可固定，历史记录可手动删除
- **可配置设置**——最大条目数（默认 1000）、最大载荷大小、空白去重开关
- **辅助功能权限管理**——内置权限检测与引导

## 使用方法

1. 从 `dist/` 目录安装最新的 DMG，或在本地构建后安装。
2. 从"应用程序"文件夹打开 Paste。
3. 首次运行时授予**辅助功能（Accessibility）权限**，以便 Paste 向当前应用发送 `Cmd+V` 粘贴操作。
4. 在任意应用中复制内容。
5. 按下 `Cmd+Shift+V` 打开 Paste 面板。
6. 输入关键词搜索，使用方向键导航，按 `Enter` 粘贴，或按 `1`-`9` 快速粘贴可见项。

Paste 运行在菜单栏中。点击菜单栏图标可显示面板，通过菜单可退出应用。

## 权限说明

Paste 需要 macOS **辅助功能（Accessibility）权限**才能模拟 `Cmd+V` 键盘事件，将内容粘贴到当前活跃的应用中。若未授予权限，剪贴板收集功能仍可正常工作，但粘贴到其他应用的操作将失败。

手动授予权限的步骤：

1. 打开 **系统设置（System Settings）**
2. 进入 **隐私与安全性（Privacy & Security）**
3. 打开 **辅助功能（Accessibility）**
4. 启用 Paste

## 快捷键

| 快捷键 | 功能 |
|---|---|
| `Cmd+Shift+V` | 显示/隐藏面板 |
| `↑` / `↓` | 上下导航 |
| `Enter` | 粘贴选中项 |
| `1` - `9` | 快速粘贴对应位置的历史项 |
| `Escape` | 关闭面板 |
| 双击列表项 | 粘贴该项 |

## 项目结构

```
Paste/
├── .github/workflows/          # CI/CD 配置（macOS 通用构建）
├── src/                        # 前端源码
│   ├── main.ts                 # UI 逻辑、事件处理、渲染
│   ├── styles.css              # 样式（暗色毛玻璃主题）
│   └── lib/commands.ts         # Tauri IPC 命令封装
├── index.html                  # Vite 入口 HTML
├── package.json                # npm 配置
├── vite.config.ts              # Vite 配置
├── tsconfig.json               # TypeScript 配置
└── src-tauri/                  # Rust 后端 / Tauri 配置
    ├── Cargo.toml              # Rust 依赖
    ├── tauri.conf.json         # Tauri 应用与打包配置
    ├── Entitlements.plist      # macOS 权限配置（关闭沙盒）
    ├── icons/                  # 应用图标资源
    └── src/
        ├── main.rs             # 程序入口
        ├── lib.rs              # 核心逻辑：Tauri 配置、命令、托盘、剪贴板监听
        ├── history/
        │   └── store.rs        # SQLite 历史存储
        ├── search/
        │   └── mod.rs          # 模糊搜索引擎
        └── macos_bridge/
            ├── mod.rs          # 平台抽象层
            └── platform.rs     # macOS FFI 实现（NSPasteboard / CGEvent）
```

## 开发环境

### 前置条件

- Node.js（CI 使用 v22）
- Rust 工具链（通用构建需安装 `x86_64-apple-darwin` 和 `aarch64-apple-darwin` 目标）
- macOS 开发环境（Xcode Command Line Tools 等）

### 启动开发服务器

```sh
npm install
npm run tauri:dev
```

这将启动 Vite 开发服务器（端口 1420）并以开发模式运行 Tauri 应用。

### 生产构建

```sh
npm run tauri:build              # 默认目标
npm run tauri:build:x64          # 仅 x86_64
npm run tauri:build:arm64        # 仅 arm64
npm run tauri:build:universal    # 通用二进制（x86_64 + arm64）
```

DMG 文件输出至 `src-tauri/target/release/bundle/dmg/`。

### npm 脚本

| 脚本 | 命令 |
|---|---|
| `dev` | `vite --host 127.0.0.1` |
| `build` | `tsc && vite build` |
| `tauri:dev` | `tauri dev` |
| `tauri:build` | `tauri build` |
| `tauri:build:x64` | `tauri build --target x86_64-apple-darwin` |
| `tauri:build:arm64` | `tauri build --target aarch64-apple-darwin` |
| `tauri:build:universal` | `tauri build --target universal-apple-darwin` |

## 技术栈

| 技术 | 用途 |
|---|---|
| Tauri 2 | 桌面应用框架（含 `macos-private-api`、`tray-icon` 特性） |
| Rust | 后端核心语言 |
| TypeScript + Vite | 前端开发 |
| rusqlite | SQLite 数据库存储 |
| fuzzy-matcher | skim 模糊搜索算法 |
| sha2 / hex | 内容哈希去重 |
| cocoa / objc / core-foundation | macOS Objective-C FFI |
| tauri-plugin-global-shortcut | 全局快捷键注册 |

## 许可证

MIT
