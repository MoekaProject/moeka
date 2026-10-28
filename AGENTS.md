# Moeka 开发指南

本文件集中维护开发、验证和发布要点；实现细节以源码和回归测试为准，不保留一次性审计数字、迁移清单或历史测试结论。[docs](docs) 仅保留应用内使用的多语言 `help-*.md`，不要删除或改作开发说明。用户文档见 [README](README.md)，协作规范见 [CONTRIBUTING](CONTRIBUTING.md)。

## Build and test

- 使用满足 [Cargo.toml](Cargo.toml) 中 `rust-version` 的 stable Rust、rustfmt 和 Clippy。所有支持 `--release` 的 Cargo 命令均使用 release profile；验证加 `--locked`，不顺带更新依赖。
- macOS 需要 Xcode、SDK、命令行工具及 Metal 工具链；Windows 需要 MSVC Build Tools、Windows SDK 和 `fxc.exe`（可用 `GPUI_FXC_PATH` 指定）；Linux 需要 Clang、CMake、pkg-config 及音频、字体、TLS、Vulkan、Wayland/X11 开发库。跨平台打包入口以 [Makefile](Makefile) 和 [Windows 打包说明](packaging/windows/README.md) 为准。
- 普通构建不需要签名、公证或发布凭据。先跑最小相关测试，再按改动范围扩大；纯文档改动只检查内容和链接，除非影响编译引用或测试夹具。

```sh
cargo fmt --all -- --check
cargo clippy --release --locked --all-targets
cargo build --release --locked
cargo test --release --locked
cargo run --release --locked
git diff --check
```

- [Cargo.toml](Cargo.toml) 的 `default-run` 指向桌面程序 `moeka`，日常启动不需要 `--bin`。诊断工具使用显式 `--bin`；以后增加独立 `moeka-cli` 时保留此默认入口。
- 图标路径改动运行 `node scripts/check-assets.mjs`；发布逻辑改动运行 `make test-publish-release`，Homebrew 接线改动另运行 `make test-publish-homebrew`。这些离线测试不发布；禁止用 `make publish` 测试脚本。
- GUI 实测使用一次性工作区，自动保存和会话持久化仍会生效。按改动检查鼠标、IME、分栏、长文档、保存重开及目标平台；编译、数值测试和跨平台编译不等于原生实测。如实报告失败、跳过及未验证项，不引用旧统计证明当前版本正确。
- PR CI 入口为 [ci.yml](.github/workflows/ci.yml)：三平台 release 构建、Clippy 和测试，另有素材与离线发布脚本检查。修改工作流时保留只读权限、固定的第三方 action 和不持久化的 checkout 凭据；候选构建见 [release.yml](.github/workflows/release.yml)，不能替代正式发布。

## Architecture

- [main.rs](src/main.rs) 负责启动、窗口和绑定；[buffer.rs](src/buffer.rs)、[cursor.rs](src/cursor.rs)、[markdown.rs](src/markdown.rs)、[markdown_format.rs](src/markdown_format.rs)、[table_ops.rs](src/table_ops.rs) 承载可复用的文本、解析及转换逻辑。保持 UTF-8 字节、UTF-16、字素、源码行与显示坐标的单位区别。
- [editor/mod.rs](src/editor/mod.rs) 中的 `EditorView` 是状态和缓存的集成中心；`element.rs`、`helpers.rs`、`text_layout.rs` 负责测量和绘制，`render*.rs` 组装 UI，`input*.rs` 处理输入。文件拆分不代表状态隔离；优先小范围、行为不变的提取，不为减少行数引入架构层。
- 多个 `impl EditorView` 模块共享字段和 `super::*`，跨模块改动须追踪状态所有权、缓存失效和异步身份，而不只看单个文件。优先把可独立测试的纯计算与领域状态提取到现有对应模块；绘制和命中必须继续共用几何，不以机械拆文件代替边界设计。
- [info_panel.rs](src/editor/info_panel.rs) 只负责文件信息和资源引用面板；文档转换与导出编排归 [export.rs](src/editor/export.rs)，WebKit/CoreGraphics 后端归 [export/macos.rs](src/editor/export/macos.rs)。后端保留 GPUI `AsyncApp` 的主线程生命周期，不包含编辑器状态。HTML 生成在后台执行，最终写入复用原子文件操作；移动功能时同时移动测试和调用者，不留下兼容转发层。
- [pane.rs](src/editor/pane.rs) 使用状态交换：根字段通常是聚焦分栏的实时状态，其他分栏在 `PaneState`，聚焦分栏的存储槽只是占位。新增文档字段同步加入 `pane_state!`，窗口共享状态不要镜像。
- `with_pane()` 是临时交换，`focus_pane()` 是永久转移；禁止跨分栏嵌套交换或在临时交换内改焦点。低层 `begin_pane_swap()` / `end_pane_swap()` 必须成对，闭包不是 panic 恢复守卫；访问当前交换状态用 `current_pane_id()`。
- 异步结果安装前检查原始 pane/tab、文档/路径、epoch、取消及配置状态；关闭分栏后不得落到另一个文档。使用现有 `with_pane_if_alive()` 等守卫，测试切焦点、移动标签、关闭待处理分栏及独立的选择/滚动/撤销。
- 系统集成归入 [platform](src/platform)；遵守 [capabilities.rs](src/platform/capabilities.rs) 对 `mas` 的子进程、终端和自更新限制。设置/会话保持向后兼容，不提交真实工作区、恢复记录、凭据或个人路径。Git/diff 功能已移除，不重新引入仓库扫描、状态缓存或特殊 diff 标签状态；普通文件监听与代码语言高亮保持独立。

## Editor invariants

- 绘制、测量、命中、选择、光标与 IME 共用源文映射和实际字形几何；全量、增量和惰性解析须一致。表格、Front Matter、折叠、换行和内联公式都不能各算一套坐标。
- 一次用户操作保持一个撤销事务；多光标基于同一原始缓冲区规划，先合并重叠目标再修改并统一映射。原生替换、组合输入及粘贴保持字面语义，不误走自动配对；尊重原生字符边界、只读状态和失效回调屏障。
- 普通编辑/撤销在光标仍可见时保持视口，不恢复历史滚动快照。搜索、大纲和模式切换使用测量后的视觉锚点；新滚动/导航/编辑优先于旧异步结果。预览更新保留已测高度和隐藏行，不清空整份布局；需要重试时只发布最终接受的绘制和命中几何。
- 模式按 Edit → Reading → Source 循环并按标签/分栏持久化。Source 是保留 Markdown 字体语义的源码视图，不是代码编辑器；规范解析元数据与呈现分离，`source_word_wrap` 独立保存。Reading/只读不暴露编辑手柄；搜索源码的临时展开不是编辑授权。
- Markdown 表格按单元格/语义片段编辑，保护转义管道、接受的 `<br>` 和行列拓扑；共享单元格与手柄菜单逻辑，保持矩形/文本选择的命令归属。Front Matter 按 YAML 源范围编辑，保留注释、引号、缩进、CRLF 和锚点，不经 Markdown 表格序列化。上下文不确定时拒绝辅助转换，不猜测破坏性重写。
- 语法高亮统一使用 `two-face` / `syntect` 的 fancy-regex 注册表，覆盖代码文件、围栏、选择器及导出；未知语言回落纯文本。Linguist 名称/扩展匹配不等于实际支持，运行时测试优先。可用 `cargo run --release --locked --bin language-support-report` 按需生成报告到 `target/language-support-audit.md`，不提交生成报告。

## Rendering and themes

- 应用 UI 的共享呈现放在 [ui](src/ui)，相似控件使用同一实现；业务动作、文档和分栏状态留在调用方。所有可调外观、组件角色和状态规则归 [style.rs](src/style.rs)，颜色语义和默认色板归 [theme.rs](src/theme.rs)，不要在业务渲染层另设默认值、直接派生颜色或借用无关正文样式。正文排版、语法色、选区及 IME 与应用 UI 风格保持独立。
- 自绘控件同样遵守组件与样式归属；测量、布局、绘制和命中共用已解析 metrics，逻辑尺寸只缩放一次。新增样式参数须有实际消费者及非默认值回归；平台约束、作者内容和协议颜色明确区分，不以未接线字段声称支持。
- Mermaid 经 Ondine 生成 SVG，编辑预览跟随当前主题，导出用 Paper White；共用渲染路径，保留透明画布和作者显式颜色。[mermaid_worker.rs](src/editor/mermaid_worker.rs) 为窗口共享状态：一个实际运行任务、最多八个排队任务。过期运行任务完成前仍占槽位，不能借取消启动重叠 CPU 工作；保留队列外的强制刷新意图及其他分栏使用的资源。
- [Mermaid 夹具](tests/fixtures/mermaid-gallery.md) 保留可执行的图表样例；`mermaid_gallery_diagrams_render_in_light_and_dark_themes` 检查全部主题。原生验证另检查冷预览、主题切换、滚动/拖动滚动条、大纲跳转和分栏关闭时的稳定性。
- [theme.rs](src/theme.rs) 的浅/深主题须同时区分画布、界面、Markdown 和代码色板；保留 Paper White / Studio Daylight 的纯白画布、单色 Markdown 和独立深色侧栏。颜色离线设计并存 sRGB，不在绘制路径动态生成；小文本对实际合成背景至少 4.5:1，色板差异须用感知距离而非 RGB 不等判断。
- 使用专用语义颜色及背景/边框 token；保留内联代码样式优先级。高亮缓存键包含完整色板；主题、字体和缩放变化失效所有分栏、非活跃标签及临时交换中的文本缓存，几何按需重建，避免每次交换全量重置。
- 导出宽度取归一化的内容宽度设置而非当前分栏或缩放；HTML/PDF/PNG 共用几何单位转换。实际检查公式/图表输出，数值测试不代替 WebKit 原生导出检查。

## Filesystem and diagnostics

- 所有可能访问挂载卷的路径探测、文件操作、watcher 注册/销毁均离开 UI 线程，使用 [fs_task.rs](src/platform/fs_task.rs) 的限额与超时机制。渲染和导航不得探测文件系统；超时任务直到实际结束仍占 worker，不无限起替代线程。
- 超时只限制等待，不保证内核操作取消。保存、移动、删除可能稍后成功；保留脏缓冲和恢复记录，不自动重试结果不明的写操作，也不提前标记已保存/关闭。会话先恢复身份和视图再异步加载，不能用空加载状态覆盖持久记录。
- watcher/搜索/目录查询失败保留原内容；只有确认 `NotFound` 才视为删除。错误包含操作、路径和原始原因，使用现有通知机制；异步结果始终检查根目录/文档身份。
- [trash.rs](src/platform/trash.rs) 仅对确认的网络卷使用永久删除；本地或未知卷走原生 Trash，探测或 Trash 失败不降级为永久删除。处理符号链接本身而非目标，拒绝根目录等危险目标。复制后源删除失败须保留完整目标供恢复。Windows Trash 在独立线程初始化 COM；原生挂载卷行为须用一次性数据实测。
- GPU 排查：macOS 用 `./scripts/run_gpu_diagnostic.sh /Applications/Moeka.app`，对比静止/移动指针时 `mouse_move_events` 和 `render_frames`；Windows 可用 `MOEKA_GPU_ADAPTER=software` 排查驱动，Linux 的 `MOEKA_FORCE_X11` 仅在有 `DISPLAY` 时有效。记录系统、显卡、缩放、驱动及实测环境。
- CPU profiling 在 macOS/Linux 输出 folded stacks（`MOEKA_PROFILE_CPU=0` 关闭采样），Windows 仅资源时间线；不自动安装或捆绑火焰图渲染器。诊断输出不得包含私密文档或路径。

## Dependencies and distribution

- [vendor/gpui](vendor/gpui) 是 [Cargo.toml](Cargo.toml) 指向的已修改依赖，不是构建缓存。升级以校验 SHA-256 的已发布 crate 为基线，不以 VCS SHA 代替发布内容；完整比较并保留本地差异、Apache-2.0 许可和通知，不假设省略的上游测试已通过。
- GPUI 更新重点验证 Windows 预编译/运行时 shader 和显示句柄、Blade/WGSL/Wayland 的布局与圆角裁剪、macOS 外观及私有 API 移除，以及各平台剪贴板、IME、缩放。旧文件数或补丁清单不是当前完整差异的证明。
- X11 初始与后续窗口标题共用 [标题属性实现](vendor/gpui/src/platform/linux/x11/window.rs)：`_NET_WM_NAME` 使用 UTF8_STRING，旧版 `WM_NAME` 使用 Latin-1（无法表示的字符替换为 `?`）。修改时验证两条属性、错误传播和标题更新，并在 X11 原生环境用 `xprop` 与窗口管理器实测。
- 图标常量集中在 [assets.rs](src/editor/assets.rs)，保留 `mini` 光学变体、浅深 logo、SVG 几何及原始许可。目录分组不代表授权；已知 Lucide 搜索替换图标的 ISC 文本见 [licenses](licenses/lucide-icons.txt)。
- 原创代码为 GPL-3.0-or-later，第三方代码/图标/字体/品牌仍遵守各自条款。发布前人工核实来源、依赖及打包库的许可、通知、对应源码义务和敏感信息；未知素材来源是分发阻塞，成功构建、secret scan 或 `mas` feature 都不是法律批准。维护 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) 和实际许可文本，打包保留 `LICENSE`、通知及相关 `licenses/`。
- 源仓库保持私有；公开源码、压缩/重写历史、删除远端 refs 或改变可见性必须另获明确授权。历史“开源准备”记录不代表已完成许可审查；如授权公开，重新审查最终快照、全部公开 refs、二进制及示例，并记录版本、证据和人工确认。

## Release

仅在用户明确要求发布时执行，以 [Makefile](Makefile)、[publish-release.sh](scripts/publish-release.sh) 和 [publish-homebrew.sh](scripts/publish-homebrew.sh) 为入口。脚本校验不能替代上述人工分发审查。

### 准备

1. 确认当前为跟踪 `origin/main` 的 `main` 分支，检查待发布差异和上次发布后的提交。保留用户改动，不自动切换分支、覆盖文件或强制推送。
2. 将 `Cargo.toml` 中 Moeka 的 PATCH 版本加一，并同步 `Cargo.lock` 中同名根包版本；不顺带升级依赖。标签必须为对应的 `vX.Y.Z`。
3. 执行本次变更需要的 release-mode 验证、格式和差异检查；已验证且未再变更的代码不重复验证，未完成的原生实测如实说明。
4. 将待发布代码、版本号和本规范的改动一并提交，推送至私有源仓库的 `origin/main`。确认工作区干净且 HEAD、跟踪分支和远端 `main` 一致。

### 发布

1. 执行 `make publish-check TAG=vX.Y.Z`，通过只读预检后，直接执行 `make publish vX.Y.Z`。预检不是发布；必须等待发布命令完成并检查退出结果。
2. 所有构建、签名、公证和上传均在本地完成，不触发或等待 GitHub Actions；Actions 仅产出非正式候选。不得替换正式构建或覆盖构建命令跳过检查。显式要求预发布时使用 `make publish-beta vX.Y.Z`，不更新稳定版 Tap。
3. 二进制发布到 `MoekaProject/community`；源码标签仅留在私有 origin，公开标签指向 community 自身的 `main`。不向公开仓库推送私有源码历史。脚本管理草稿及八个附件（双架构 macOS DMG、macOS/Windows/Linux ZIP），核对大小和 SHA-256；无远端摘要时下载逐字节比较，禁止只比较大小或手动提前公开草稿。发布由单一维护者串行执行。
4. 所有 GitHub 查询和修改都显式指定仓库，不依赖 `gh` 的默认仓库配置；自动执行时设置 `GH_PAGER=cat` 禁用交互分页。发布后，用 `gh release edit vX.Y.Z --repo MoekaProject/community --notes ...` 更新简洁、面向用户的英文说明，包含 `Features`、`Fixes`、`Improvements` 三部分；只描述本次实际交付的变化，不罗列内部实现细节。
5. 查询该 Release，确认非草稿、预发布标志与请求一致、版本和八个附件正确，并确认英文说明已写入。稳定版另验证 Tap 版本。最后提供 Release 链接及对应的中文发行说明。

### Homebrew

- 稳定版 `make publish` 成功发布应用后自动更新 `MoekaProject/homebrew-moeka`，只复用公开的已签名 DMG，不运行 Actions。需要本地 `git`、`gh`、`jq`、`shasum`、`ruby`、Git 作者配置和 Tap 写权限。
- 独立只读检查用 `make homebrew-check TAG=vX.Y.Z`；已有正式版同步或失败重试用 `make publish-homebrew TAG=vX.Y.Z`。双架构下载须与 Release 大小及 SHA-256 一致，缺摘要、草稿、预发布、降级和同版本换摘要均拒绝；版本和摘要一致则无提交。
- 仅在独立公开 Tap clone 中修改 Cask 版本/摘要，保留 livecheck 等配置，非强制推送。Tap 失败时应用已发布，只重试 Tap 命令；并发推送冲突后重新读取校验，不 rebase 旧摘要或重发应用。安装/升级见 [Homebrew 说明](packaging/homebrew/README.md)。

### 失败处理

- 先根据实际错误修复，再从失败步骤继续；把可复用的问题处理方式补充到本文件。涉及发布脚本的修改先运行 `make test-publish-release` 离线回归。
- 公证上传出现 S3 `deadlineExceeded`、`abortedUpload` 或 `notarytool` 异常退出时，保留错误日志并检查上传阶段；签名脚本使用 `--no-s3-acceleration` 避免加速上传路径故障。仍须等待公证成功并通过 staple、签名及 Gatekeeper 校验；失败不得继续打包或上传。
- 网络、认证或 API 错误不等于 Release 不存在；先恢复只读查询，仅确定的 404 配合成功的草稿列表检查可判缺失。保留原始 `dist` 和已创建的标签/草稿，不重复创建或覆盖附件。
- 在创建标签或草稿之前修复了受版本控制的文件，须重新验证、提交并推送，再重新预检。构建期间不要修改源码或移动 HEAD。
- 已创建草稿时保留原始 `dist` 文件，用同一版本的 `make publish vX.Y.Z` 续传；不要重新构建替换已签名附件，不删除、覆盖附件或强制移动标签。
- 已发布版本不可重新发布；仅发行说明更新失败时，重试更新说明，不再执行发布命令。永久禁止修改或重新发布 `v0.1.12`。
- 遇到密钥、密码或需要人工确认的权限/标签冲突时停止并说明阻塞，不索取明文凭据，不绕过签名或完整性检查。发布后才修改的规范应另行提交并推送，不移动已发布标签。
