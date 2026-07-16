# ADR-0001：Shittim 工作区与工具链

- 状态：accepted
- 日期：2026-07-16

## 背景

Shittim 将包含 Rust Kernel、TypeScript/Pi runtime、Tauri 桌面端、Schema 生成物和多语言 SDK。编码前需要固定首批工作区与工具链原则，同时诚实记录当前环境。

当前实际环境：

- `rustc 1.97.0`；
- `cargo 1.97.0`；
- `node 26.4.0`；
- `pnpm 11.3.0`。

`AGENT.md` 要求 TypeScript 使用 Node LTS。Node 26.4.0 不是本项目选定的 Node 24 LTS，不能把“版本更高”写成已经满足 LTS 约束。

## 决策

1. 产品/仓库品牌为 **Shittim**；`Companion` 保留为系统中的 AI 交互角色概念。
2. Rust 使用 stable channel；首批环境记录为 rustc/cargo 1.97.0。实现时用仓库工具链文件锁定已验证 stable 版本，不承诺尚未测试的最低支持 Rust 版本。
3. Node 锁定 **24 LTS**。在 Node 24 LTS 环境可用并通过检查前，TypeScript/Pi/Tauri 前端 workspace 标记为**工具链阻塞**，不得用 Node 26.4.0 生成 lockfile 后宣称满足 Node LTS。
4. JavaScript 包管理器选择 **pnpm 11.3.0**，后续在根 package metadata 与 Corepack 配置中精确锁定。
5. Tauri、React、Ant Design 的具体依赖版本不在文档中猜测；由 Node 24 LTS 环境中的首次依赖解析和提交的 lockfile 固定，并经构建/测试验证。
6. Monorepo 目录以 `specs/IMPLEMENTATION_CONTRACTS.md` 为方向，首批创建顺序为 Schema source/generator、Rust workspace、TypeScript workspace、SDK；目录创建不改变状态所有权。

## 备选方案

- 直接使用 Node 26.4.0：拒绝，因为不满足已接受的 Node 24 LTS 目标且会伪造工具链完成状态。
- 使用 npm/yarn：可行，但首批统一选择 pnpm 11.3.0 减少工作区差异。
- 在 ADR 写死 Tauri/React/AntD 猜测版本：拒绝，版本应由真实 registry 解析和 lockfile 证明。

## 影响

- Rust 规范和纯文档工作不受 Node 阻塞影响。
- Node 相关脚手架、依赖安装、生成与构建必须等待 Node 24 LTS。
- 当前仅接受决策，没有 workspace 或 lockfile 实现。
