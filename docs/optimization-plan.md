# Paste 项目优化方案

## 1. 性能优化

### 1.1 搜索路径优化 ✅ 已完成（Phase 1）

**现状**: `search()` 每次从 SQLite 加载最多 1000 条记录到内存，再做 fuzzy match 和排序。

**优化方案**:
- 空查询直接走 SQL `LIMIT ?1`，避免全量加载
- 非空查询加载全量后做 fuzzy match（后续可加 SQL 前置过滤）
- `src-tauri/src/history/store.rs` — `search()` 方法已拆分为空查询/非空查询两条路径

### 1.2 Settings 缓存 ✅ 已完成（Phase 1）

**现状**: `insert_clip()` 每次调用 `get_settings()` 查询数据库，剪贴板高频复制场景下每秒 2-3 次不必要 SQL 查询。

**优化方案**:
- 在 `HistoryStore` 中增加 `cached_settings: Mutex<Option<AppSettings>>` 字段
- `get_settings()` 优先读缓存，命中则跳过 SQL；首次 miss 后回填缓存
- `save_settings()` 写入 DB 后同步更新缓存

### 1.3 Prune 策略优化 ✅ 已完成（Phase 3）

**现状**: 每次 `insert_clip()` 后执行 `prune()`，逐条 `delete_clip()` 删除过期记录。

**优化方案**:
- 降低 prune 频率：每 N 次 insert 才执行一次（`PRUNE_INTERVAL = 10`）
- 使用子查询批量 DELETE 替代逐条删除
- blob 文件先收集再批量删除

### 1.4 前端渲染优化 ✅ 已完成（Phase 3）

**现状**: `renderClips()` 每次替换整个 `list.innerHTML`，包括所有事件监听器重新绑定。

**优化方案**:
- `mousemove` 改为 `mouseenter`，只调用 `updateSelection()` 切换 CSS class，不触发完整重渲染
- 图片缩略图异步加载，不阻塞列表渲染

### 1.5 Hash 计算优化

**现状**: `hash_item()` 对所有 payload（包括大文件）做 SHA-256 全量哈希。

**优化方案**:
- 对大 payload 使用采样哈希（头部+尾部+长度）
- 或先比较 `kind + payload_count + total_size` 快速排除

---

## 2. 可靠性优化

### 2.1 RwLock 方案 ❌ 不可行

原计划将 `Mutex` 替换为 `RwLock`，但 `rusqlite::Connection` 内部的 `StatementCache` 使用了 `RefCell`，不满足 `Sync` trait。`RwLock<T>: Sync` 要求 `T: Send + Sync`，而 `Mutex<T>: Sync` 只要求 `T: Send`，因此当前 `Mutex` 方案是正确选择。

如果未来需要读写分离，可考虑将读操作放到独立的 `read-only Connection` 上。

### 2.2 线程优雅退出 ✅ 已完成（Phase 4）

**现状**: `start_clipboard_watcher()` 启动无限循环 `thread::spawn`，应用退出时没有取消机制。

**优化方案**:
- 添加 `SHUTDOWN: AtomicBool` 静态标志
- watcher 循环每轮检查标志并 break
- `RunEvent::Exit` 时设置标志，触发优雅退出

### 2.3 SQLite 连接与事务

**现状**: 单一 `Connection` 用于所有读写操作，每次 `insert_clip` 隐式事务又调用 `prune`。

**优化方案**:
- `insert_clip` 内使用显式 `BEGIN/COMMIT` 事务包裹全部写操作 + prune
- 考虑定期执行 `PRAGMA wal_checkpoint(TRUNCATE)` 防止 WAL 文件无限增长

### 2.4 Paste 的 80ms 魔法延迟

**现状**: `thread::sleep(Duration::from_millis(80))` 确保剪贴板写入完成后再发送粘贴键。

**优化方案**:
- 改为指数退避重试：先等 40ms，检查是否写入成功，不成功再等
- 或监听 `NSPasteboard` 的 `changeCount` 确认写入完成

---

## 3. 代码质量优化

### 3.1 lib.rs 模块拆分 ✅ 已完成（Phase 2）

**现状**: `lib.rs`（258 行）混合了 IPC 命令、剪贴板监听、托盘图标、全局快捷键等职责。

**拆分方案**:
- `commands.rs` — 所有 `#[tauri::command]` 函数
- `watcher.rs` — `start_clipboard_watcher` 逻辑
- `tray.rs` — `setup_tray` 逻辑
- `lib.rs` — 只保留 `run()` 入口、AppState 定义和 `show_panel`

### 3.2 引入 thiserror 错误类型 ✅ 已完成（Phase 2）

**现状**: 所有命令返回 `Result<T, String>`，丢失错误上下文。

**方案**:
- 使用 `thiserror` 定义 `PasteError` 枚举（`error.rs`）
- 实现 `Serialize` 以支持 Tauri IPC
- 所有 command 函数返回 `Result<T, PasteError>`（`commands.rs`）

### 3.3 引入 tracing 日志框架 ✅ 已完成（Phase 2）

**现状**: 使用 `eprintln!` 输出错误，无法控制日志级别和输出目标。

**方案**:
- 引入 `tracing` + `tracing-subscriber`（已添加到 Cargo.toml）
- 在 `run()` 中初始化 `tracing_subscriber::fmt()`，支持 `RUST_LOG` 环境变量
- watcher 中 `eprintln!` 已替换为 `tracing::error!`

### 3.4 消除重复代码 ✅ 已完成（Phase 4）

`summarize_text` 在 `platform.rs` 和 `platform_win.rs` 中有不同实现。已提取到 `macos_bridge/mod.rs` 公共模块，统一使用 Unicode 安全的 `chars()` 版本。

### 3.5 前端模块化 ✅ 已完成（Phase 4）

`main.ts` 已拆分为：
- `components/clip-list.ts` — 列表渲染、缩略图加载、escapeHtml
- `components/settings.ts` — 设置面板渲染与事件
- `app.ts` — 入口、键盘事件、状态管理

---

## 4. 安全优化

### 4.1 SQL 注入防护 ✅ 已完成（Phase 1）

**现状**: `prune()` 用 `format!` 拼接 SQL：`format!("...LIMIT -1 OFFSET {max_items}")`。

**优化**: 已改为参数化查询 `params![max_items as i64]`。

### 4.2 前端 XSS 防护验证 ✅ 已完成（Phase 4）

已审计所有 `innerHTML` 模板中的用户内容：`textPreview`、`clipId`（用于 data 属性）均通过 `escapeHtml()` 处理。其他值均为静态字符串或 Date API 输出。

### 4.3 Blob 文件路径校验 ✅ 已完成（Phase 4）

`delete_blob_files` 和 `prune` 中的 blob 文件删除前，通过 `is_safe_filename()` 校验文件名不含 `..`、`/`、`\`。

---

## 5. UX 优化

| 优化项 | 优先级 | 说明 |
|---|---|---|
| 图片缩略图预览 | 中 | ✅ 已完成 — 异步加载 base64 data URL 显示缩略图 |
| 加载状态指示器 | 中 | ✅ 已完成 — 搜索栏旁旋转 spinner |
| 分页/虚拟滚动 | 低 | 当前固定 40 条，历史多时无法浏览更多 |
| 空状态引导优化 | 低 | "Copy something to begin" 可以更友好 |
| 失焦隐藏延迟 | 低 | ✅ 已完成 — 150ms 延迟+重新检查焦点再隐藏 |

---

## 执行进度

| 阶段 | 内容 | 状态 | 预计工作量 |
|---|---|---|---|
| **Phase 1** | RwLock 评估 + Settings 缓存 + SQL 注入修复 + 空查询优化 | ✅ 已完成 | ~2h |
| **Phase 2** | lib.rs 拆分 + thiserror 错误类型 + tracing 日志 | ✅ 已完成 | ~3h |
| **Phase 3** | Prune 批量化 + 前端 diff 渲染 + 图片缩略图 | ✅ 已完成 | ~4h |
| **Phase 4** | 线程优雅退出 + 前端模块化 + UX 改善 + 安全加固 | ✅ 已完成 | ~3h |
