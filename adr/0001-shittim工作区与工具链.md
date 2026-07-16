# ADR-0001：Shittim 工作区与工具链

- 状态：accepted
- 日期：2026-07-16
- 修订：2026-07-16（记录 Node 24.18.0 可用；Rust workspace 已落地）

## 背景

Shittim 将包含 Rust Kernel、TypeScript/Pi runtime、Tauri 桌面端、Schema 生成物和多语言 SDK。编码前需要固定首批工作区与工具链原则，同时诚实记录当前环境。

当前实际环境：

- `rustc 1.97.0`；
- `cargo 1.97.0`；
- Node **24.18.0** 已通过 pnpm 用户 runtime 安装（binary 预计在 `~/.local/share/pnpm/nodejs/24.18.0/bin`）；默认 shell 的 `node` 仍可能指向其他版本，创建 TS workspace 时必须显式使用 24.18.0；
- `pnpm 11.3.0`。

`AGENT.md` 要求 TypeScript 使用 Node LTS。Node 24 LTS 目标版本的环境阻塞**已解除（实际 24.18.0）**；但这不表示 TypeScript workspace 或 lockfile 已经存在。

## 决策

1. 产品/仓库品牌为 **Shittim**；`Companion` 保留为系统中的 AI 交互角色概念。
2. Rust 使用 stable channel；仓库 `rust-toolchain.toml` 锁定 **1.97.0**。不承诺尚未测试的最低支持 Rust 版本。
3. Node 锁定 **24 LTS（实际验证 24.18.0）**。TypeScript/Pi/Tauri 前端 workspace 在创建前必须使用该 Node；不得用非 24 LTS 默认 `node` 生成 lockfile 后宣称满足约束。
4. JavaScript 包管理器选择 **pnpm 11.3.0**，后续在根 package metadata 与 Corepack 配置中精确锁定。
5. Tauri、React、Ant Design 的具体依赖版本不在文档中猜测；由 Node 24 LTS 环境中的首次依赖解析和提交的 lockfile 固定，并经构建/测试验证。
6. Monorepo 目录以 `specs/IMPLEMENTATION_CONTRACTS.md` 为方向；首批已创建 Schema source/generator 与 Rust workspace；TypeScript workspace / SDK 包仍未创建。

## 备选方案

- 直接使用 Node 26.x 默认环境：拒绝，因为不满足已接受的 Node 24 LTS 目标。
- 使用 npm/yarn：可行，但首批统一选择 pnpm 11.3.0 减少工作区差异。
- 在 ADR 写死 Tauri/React/AntD 猜测版本：拒绝，版本应由真实 registry 解析和 lockfile 证明。

## 影响

- Rust 与 Schema 工作不依赖 Node。
- Node 相关脚手架、依赖安装与前端构建在使用 24.18.0 后方可开始；当前仍无 TypeScript workspace。
- Rust workspace 成员目前为 `kernel-contracts`、`schema-tool`、`domain-task`、`domain-policy` 与 `kernel-sqlite`；仍无 `agentd`。
