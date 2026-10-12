# Clyde 魔改 — 项目进度记忆(唯一进度真相源)

> **给所有在此仓库工作的 agent(必读):**
> 1. 开工前先读完本文件,禁止重做「已完成」的任务
> 2. 领取任务:把对应任务状态改为「进行中」
> 3. 完工:状态改「已完成」,在「改动日志」追加一行(日期 / 任务 / 改动文件 / 验证结果)
> 4. 放弃某做法:状态改「放弃」并写明原因,防止后来的 agent 重复踩坑
> 5. 本文件记录与代码实际不符时,以代码为准,并 immediately 修正本文件
> 6. 详细的实施计划见会话计划文件;本文件只保留摘要,避免双写漂移

## 背景

- Clyde:AI coding agent 桌面宠物(Tauri 2.10 + Rust 后端 + Svelte 5 前端)
- fork 自 QingJ01/Clyde(**AGPL-3.0**),个人魔改使用,不分发、不提供网络服务 → 无开源义务
- 本地:`C:\Users\Rinse\Clyde`；上游基线 f537403(v0.1.6)，当前修复分支 `fix/edge-snap-20261011`
- 环境:Node 24 / Rust 1.94 / npm 11;`npm start` = Tauri dev;`npm test` = cargo test + hooks 测试
- 运行时数据目录:`~/.clyde`;hooks 注册进 `~/.claude/settings.json`
- 关键架构:事件→状态映射分散在 5 处(clyde-hook.js / copilot-hook.js / state_machine.rs / codex_monitor.rs / claude_monitor.rs),加新状态要全改

## 根因诊断摘要(2026-10-06 定稿,均已验证)

| # | 问题 | 根因 |
|---|------|------|
| 1 | 启动连闪 CMD | GUI 进程裸调 `wsl.exe`(codex_monitor.rs:234,248),本机 3 个发行版 → 4 个控制台窗口;缺 `CREATE_NO_WINDOW` |
| 2 | 托盘菜单状态陈旧 | 拖拽进出 mini、`toggle_dnd` 等路径改状态后不调 `tray::rebuild_menu`;点击穿透开启时 hit 窗口隐藏 → 宠物不可右键(lib.rs:201-221) |
| 3 | 吸附逻辑差 | ①用物理屏边缘非工作区(windows.rs:151-161),任务栏在侧边时宠物藏到任务栏下;②`do_enter_mini` 用贴边松手位覆写恢复坐标、`do_exit_mini` 又从它恢复 → 回吸死循环(mini.rs:212-288);③进入容差仅 30 逻辑像素 |
| 4 | 拖动卡顿 | 每 pointermove 一次 IPC(hit/App.svelte:93)+ 3 次显示器枚举 + pet/hit 双窗口移动(hit 还每次 set_size 触发 WebView2 重排)+ 每次 Moved 事件 spawn 修复 task(lib.rs:1345) |

## 任务清单(第一批)

| ID | 任务 | 状态 | 主要文件 |
|----|------|------|----------|
| T0 | 建立 MEMORY.md + CLAUDE.md 进度真相源 | 已完成 | MEMORY.md、CLAUDE.md |
| T1 | CMD 静默(CREATE_NO_WINDOW × 2 处) | 已完成 | src-tauri/src/codex_monitor.rs |
| T2 | 托盘重建补漏(do_enter/exit_mini、do_toggle_dnd)+ Standing hit 区微扩 | 已完成 | mini.rs、lib.rs、hit_regions.rs |
| T3 | 吸附修复:MonitorArea 改工作区 + 恢复位置防回吸 + 容差 30→40 | 已完成 | windows.rs、mini.rs |
| T4 | 拖动保守优化:rAF 合并 + 显示器缓存(TTL 500ms)+ sync_hit 去冗余 + snap-preview 差量 + 拖拽中不 spawn 修复 task | 已完成(待用户实测) | hit/App.svelte、lib.rs、windows.rs |

**备选未立项**:原生 `startDragging()` 拖拽重写(T4 后实测仍不满意才立项);点击穿透开启时的气泡提示(可复用 permission.rs mode-notice 机制)。

## 第二批修复

| ID | 任务 | 状态 | 主要文件 |
|----|------|------|----------|
| T5 | 移除蓝色吸附框/缩放预览；修正高 DPI 命中区域；主体边缘吸附及迟滞；拖拽收尾与动画互斥 | 已完成（待 Windows 原生实测） | hit/App.svelte、pet/App.svelte、lib.rs、mini.rs、windows.rs、tick.rs |
| T6 | v0.1.7 版本同步、Windows release 打包及 GitHub 发布 | 已完成 | package.json、锁文件、tauri.conf.json、update_check.rs、CHANGELOG.md、release.yml |

Git 留档：用户于 2026-10-11 授权提交；修复分支 `fix/edge-snap-20261011`，修复前快照 `478e4c2`，保留此前 T1–T4 的所有改动。

T5 验收：`npm test` 99 个 Rust + 18 个 JS 测试全部通过；Vite 生产构建、Tauri release + NSIS 打包成功。`hooks/tests/verify-hit-ui.py` 使用 Playwright/Edge、模拟 Tauri IPC，验证 100/125/150/200% DPI 命中区域、无蓝框、鼠标无默认焦点框、松手/取消及宠物尺寸/透明度不变。Windows 原生窗口移动、不同 DPI 显示器间拖拽手感仍待用户实测，不能将浏览器模拟验收当作原生验收。`npm run check` 仍为项目外 `C:\node_modules\@types` 缺少 undici-types/form-data 的 13 个预存错误，无项目 src 错误。

打包命令：`npm.cmd run tauri -- build --bundles nsis`。
安装包：`src-tauri/target/release/bundle/nsis/Clyde on Desk_0.1.6_x64-setup.exe`（2,214,543 bytes）。
SHA256：`73C33B17079EC7255FAB747F06A6A415BAAFCC1BD9379E6AF62A3D180F476FCE`。
浏览器回归脚本在构建后运行：`python hooks/tests/verify-hit-ui.py`（需 Python Playwright 和 Edge；进程内临时 HTTP 服务自动关闭）。

### v0.1.7 发布（2026-10-12）

- 用户授权推送和打包发布；按现有 v0.1.6 序列升级为 v0.1.7，发布目标 `RinseDuke/Clyde`，不改动原作者仓库。
- `package.json`、`package-lock.json`、`Cargo.toml`、`Cargo.lock`、`tauri.conf.json` 同步为 0.1.7；更新检查地址改为本 fork，CHANGELOG 和自动发布说明同步。
- 99 个 Rust + 18 个 JS 测试通过；Vite / Tauri release / NSIS 构建成功；浏览器模拟回归四档 DPI 全通过。
- Windows x64 安装包：`src-tauri/target/release/bundle/nsis/Clyde-on-Desk_0.1.7_windows-x64-setup.exe`，2,214,659 bytes。
- SHA256：`c1338f5caabb823ca4136e0c13aa3ba603ffcd73a2eb0c97d8fcfe79f1a2213d`；同目录 `.exe.sha256` 文件供下载校验。
- GitHub 正式 Release 已发布：https://github.com/RinseDuke/Clyde/releases/tag/v0.1.7 。安装包与 SHA256 文件已上传，GitHub 返回的安装包摘要与本地一致。
- 版本提交 `2d85155`、标签 `v0.1.7` 已推送至 `fork`；修复分支 `fix/edge-snap-20261011`，原作者 `origin` 和远程 main 未改动。Windows 原生跨屏拖动仍待用户实测。
- 普通 Python 3.13 未安装 Playwright，浏览器验证使用 Codex bundled Python：`C:\Users\Rinse\.cache\codex-runtimes\codex-primary-runtime\dependencies\python\python.exe`。

## 改动日志

| 日期 | 任务 | 摘要 |
|------|------|------|
| 2026-10-08 | T0 | 计划定稿(4 问题诊断 + T0–T4 任务分解),创建本文件与 CLAUDE.md |
| 2026-10-08 | T0 完工 | 新建 MEMORY.md + CLAUDE.md,协作规则/诊断/任务表/约定就位 |
| 2026-10-08 | T1 完工 | codex_monitor.rs 新增 silenced_wsl()(CREATE_NO_WINDOW),两处 wsl 调用改走它;cargo test 85 通过 |
| 2026-10-08 | T2 完工 | mini.rs 两函数尾补 rebuild_tray();lib.rs do_toggle_dnd 补 rebuild;hit_regions.rs Standing 区扩至 y 0.12–0.90;cargo test 85 通过 |
| 2026-10-08 | T3 完工 | windows.rs monitor_area 改 work_area;mini.rs do_enter_mini 不覆写已有坐标、do_exit_mini 加 nudge_out_of_snap_band、容差 30→40;新增 7 个单测(含任务栏两侧场景);cargo test 92 通过 |
| 2026-10-08 | T4 完工 | hit/App.svelte rAF 合并(松手前补发最终坐标);windows.rs 新增 MonitorCache(TTL 500ms)+ sync_hit_window 比对后跳过未变 set;lib.rs DragState.last_snap_side 差量发 preview、Moved 分支拖拽中不 spawn、ScaleFactorChanged 清缓存;npm test 92+11 全过,vite build 通过,svelte-check 无 src 错误(13 个 node_modules 错误为预存环境问题)。**未做视觉验收,待用户 npm start 实测拖动/吸附/托盘** |
| 2026-10-11 | T5 完工 | 移除 hit 蓝框与 pet 缩小；命中区域用物理像素比例生成 CSS 百分比；按下/松手立即采样系统物理坐标，串行 IPC、每次拖拽 token、取消不吸附；固定主体边缘 12px 进入/24px 退出迟滞，缓存目标屏 DPI，按实际窗口尺寸维持跨屏抓取点；普通拖动保留睡眠序列，拖动取消 mini 移动/暂停探头；mini 拖出保留松手位置。新增 JS 竞态/DPI 和 Rust 吸附回归、浏览器模拟验收；99+18 测试与 release/NSIS 构建通过，安装包信息见上。 |
| 2026-10-12 | T6 完工 | 同步五处版本元数据为 0.1.7，更新检查转向 RinseDuke/Clyde，补充 CHANGELOG 并修正 CI 发布说明链接；99+18 测试、四档 DPI 浏览器模拟和 Windows release/NSIS 构建通过；推送版本提交 2d85155 与标签 v0.1.7，正式 Release 附带 Windows x64 安装包和 SHA256 文件，上传摘要校验一致。 |

**备注**:上游 .gitignore 第 12 行本就忽略 `CLAUDE.md`(本地 agent 自动加载不受影响,仅不会被提交);`MEMORY.md` 不在忽略列表,是否提交由用户决定。

## 约定

- **不主动 `git commit`**,用户明确要求才提交;工作区基线仅 package-lock.json / Cargo.lock 的版本号噪声差异
- Windows 下删除文件遵循回收站约定(见全局 memory)
- 每任务完成必须 `npm test` 全绿后才更新状态为「已完成」
- Rust 侧锁纪律:调 `tray::rebuild_menu` / `prefs::save` 前必须先 drop 手上的 prefs/state 锁(非重入 Mutex,否则死锁)
