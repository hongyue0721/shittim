# ADR-0001：Shittim 工作区与工具链

- 状态：accepted
- 日期：2026-07-16
- 修订：2026-07-17（落地零依赖 Node/pnpm 根工作区基座；记录实际入口与 Corepack 不可用）

## 背景

Shittim 将包含 Rust Kernel、TypeScript/Pi runtime、Tauri 桌面端、Schema 生成物和多语言 SDK。编码前需要固定首批工作区与工具链原则，同时诚实记录当前环境。

当前实际环境：

- `rustc 1.97.0`；
- `cargo 1.97.0`；
- Node **24.18.0** 的**实际入口**是 pnpm 用户 runtime：`~/.local/share/pnpm/node`（将 `$HOME/.local/share/pnpm` 放在 `PATH` 最前时，`node -v` 为 `v24.18.0`）；
- 默认系统 `PATH` 上的 `node` 仍可能是 **Node 26.x**（本机验证为 `v26.4.0`），**不得**用默认 node 生成 lockfile 或宣称满足 24 LTS 约束；
- `pnpm 11.3.0`（`pnpm -v`）；
- **Corepack 不可用**（本环境无 `corepack` 命令）；根 `package.json` 的 `packageManager` / `engines`、`.node-version` 与 `.npmrc engine-strict=true` 提供版本元数据，真正的硬验收门是 `pnpm run check:toolchain` 对当前 Node 进程及 PATH 中实际 `pnpm --version` 的双重校验，不依赖 Corepack enable。

`AGENT.md` 要求 TypeScript 使用 Node LTS。Node 24 LTS 目标版本的环境阻塞**已解除（实际 24.18.0）**。根工作区**已存在**（`package.json`、`pnpm-workspace.yaml`、零依赖 `pnpm-lock.yaml`、`scripts/check-node-toolchain.mjs`），但 **尚无** `ts/*` 包、TypeScript 依赖、Schema→TS 生成物或 SDK/client。

## 决策

1. 产品/仓库品牌为 **Shittim**；`Companion` 保留为系统中的 AI 交互角色概念。
2. Rust 使用 stable channel；仓库 `rust-toolchain.toml` 锁定 **1.97.0**。不承诺尚未测试的最低支持 Rust 版本。
3. Node 锁定 **24 LTS（exact 24.18.0）**。凡生成 lockfile、安装依赖或运行 Node 脚本，必须使用上述实际入口（`PATH` 优先 `~/.local/share/pnpm`）；不得用默认 Node 26.x 生成产物后宣称满足约束。
4. JavaScript 包管理器选择 **pnpm 11.3.0**（exact）。根 `package.json` 写明 `packageManager: "pnpm@11.3.0"` 与 `engines.node` / `engines.pnpm`；`.npmrc` 启用 `engine-strict=true` 作为包管理器兼容提示，但 pnpm 11.3.0 在本环境对根 engine mismatch 仍可能只警告，因此不得把它当作硬门；`.node-version` 为 `24.18.0`，`check:toolchain` 必须实际执行 PATH 中的 `pnpm --version`。因 Corepack 不可用，不将 Corepack 作为运行前提。
5. 根工作区基座为零依赖：`private` 根包、`pnpm-workspace.yaml` 声明 `packages: - "ts/*"`，**不**预先创建 `ts/` 占位目录或包；**不**添加 deps/devDeps；**不**添加空的 build/lint/test 脚本。根 `package.json` scripts 仅：`check:toolchain`、`check:file-manifest`、`write:file-manifest`、`test:file-manifest`（均为零依赖 Node 脚本）。**不**提供跨平台 `check:all` npm 脚本；仓库统一门是 bash：`export PATH="$HOME/.local/share/pnpm:$PATH"` 后执行 `./scripts/check-schema.sh`（先 Node/pnpm 硬门，再 Rust schema/cargo，最后 FILE_MANIFEST）。
6. Tauri、React、Ant Design 的具体依赖版本不在文档中猜测；由 Node 24.18.0 + pnpm 11.3.0 环境中的首次真实依赖解析和提交的 lockfile 固定，并经构建/测试验证。当前根 lockfile 仅记录根 workspace importer，不含业务依赖。
7. Monorepo 目录以 `specs/IMPLEMENTATION_CONTRACTS.md` 为方向；Schema source/generator 与 Rust workspace 已创建；TypeScript 包 / SDK client / Pi `agent-runtime` 仍未创建。

## 备选方案

- 直接使用 Node 26.x 默认环境：拒绝，因为不满足已接受的 Node 24 LTS 目标；pnpm 11.3.0 对根 `engines` mismatch 可能只警告，必须由 `check:toolchain` 硬失败。
- 使用 npm/yarn：可行，但首批统一选择 pnpm 11.3.0 减少工作区差异。
- 依赖 Corepack 自动切换：本环境 Corepack 不可用；改为 `packageManager` + engines + smoke 脚本。
- 在 ADR 写死 Tauri/React/AntD 猜测版本：拒绝，版本应由真实 registry 解析和 lockfile 证明。

## 影响

- Schema 源与 Rust codegen 本身不依赖 Node；但仓库统一门 `scripts/check-schema.sh` **依赖** PATH 上的 Node 24.18.0 / pnpm 11.3.0：先跑 `check-node-toolchain.mjs`，错误版本在长 Rust 步骤前 fail。
- 根 Node/pnpm 基座已可验收：`export PATH="$HOME/.local/share/pnpm:$PATH"` 后 `pnpm run check:toolchain` 应通过；用默认 Node 26 直接执行同一脚本或 `./scripts/check-schema.sh` 应在早期硬失败。
- 仍无 TypeScript 业务包、无 TS Schema 生成、无 SDK/client、无 Tauri/React/AntD。
- Rust workspace 成员目前为 `kernel-contracts`、`schema-tool`、`domain-task`、`domain-policy`、`kernel-sqlite` 与 `kernel-kcp`；仍无 `agentd`。
