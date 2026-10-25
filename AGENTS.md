# Release

仅在用户明确要求发布时执行以下流程。发布实现和安全约束见 [docs/releasing.md](docs/releasing.md)，以现有本地发布脚本为入口。

## 准备

1. 确认当前为跟踪 `origin/main` 的 `main` 分支，检查待发布差异和上次发布后的提交。保留用户改动，不自动切换分支、覆盖文件或强制推送。
2. 将 `Cargo.toml` 中 Moeka 的 PATCH 版本加一，并同步 `Cargo.lock` 中同名根包版本；不顺带升级依赖。标签必须为对应的 `vX.Y.Z`。
3. 对本次变更执行必要验证。Rust 测试和构建使用 `cargo test --release --locked`、`cargo build --release --locked`；检查 `cargo fmt --all -- --check` 和 `git diff --check`。已验证且未再变更的代码不重复验证；未完成的原生实测要如实说明。
4. 将待发布代码、版本号和本规范的改动一并提交，推送至私有源仓库的 `origin/main`。确认工作区干净且 HEAD、跟踪分支和远端 `main` 一致。

## 发布

1. 执行 `make publish-check TAG=vX.Y.Z`，通过只读预检后，直接执行 `make publish vX.Y.Z`。预检不是发布；必须等待发布命令完成并检查退出结果。
2. 所有构建、签名、公证和上传都在本地流程中完成，不触发或等待 GitHub Actions。不要用 Actions 候选产物替换正式构建，也不要通过覆盖构建命令跳过检查。
3. 正式二进制发布到 `MoekaProject/community`；源码标签仅留在私有 origin，不向公开仓库推送源码历史。由脚本管理标签、草稿及八个发布附件，不手动提前公开草稿。
4. 所有 GitHub 查询和修改都显式指定仓库，不依赖 `gh` 的默认仓库配置；自动执行时设置 `GH_PAGER=cat` 禁用交互分页。发布后，用 `gh release edit vX.Y.Z --repo MoekaProject/community --notes ...` 更新简洁、面向用户的英文说明，包含 `Features`、`Fixes`、`Improvements` 三部分；只描述本次实际交付的变化，不罗列内部实现细节。
5. 查询该 Release，确认非草稿、非预发布、版本和八个附件正确，并确认英文说明已写入。最后提供 Release 链接及对应的中文发行说明。

## 失败处理

- 先根据实际错误修复，再从失败步骤继续；把可复用的问题处理方式补充到本文件。涉及发布脚本的修改先运行 `make test-publish-release` 离线回归。
- 公证上传出现 S3 `deadlineExceeded`、`abortedUpload` 或 `notarytool` 异常退出时，保留错误日志并检查上传阶段；签名脚本使用 `--no-s3-acceleration` 避免加速上传路径故障。仍须等待公证成功并通过 staple、签名及 Gatekeeper 校验；失败不得继续打包或上传。
- 在创建标签或草稿之前修复了受版本控制的文件，须重新验证、提交并推送，再重新预检。构建期间不要修改源码或移动 HEAD。
- 已创建草稿时保留原始 `dist` 文件，用同一版本的 `make publish vX.Y.Z` 续传；不要重新构建替换已签名附件，不删除、覆盖附件或强制移动标签。
- 已发布版本不可重新发布；仅发行说明更新失败时，重试更新说明，不再执行发布命令。永久禁止修改或重新发布 `v0.1.12`。
- 遇到密钥、密码或需要人工确认的权限/标签冲突时停止并说明阻塞，不索取明文凭据，不绕过签名或完整性检查。发布后才修改的规范应另行提交并推送，不移动已发布标签。
