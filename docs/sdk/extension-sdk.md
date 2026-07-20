# Extension SDK

> 状态徽章：**contract-only**（无可发布 SDK 包、无 `ts/*` 生成物、无 composition/public API）
> 唯一规范事实源：[`specs/EXTENSION_SDK.md`](../../specs/EXTENSION_SDK.md)。本文不定义新 API。
> 完成状态：[`../IMPLEMENTATION_MATRIX.md`](../IMPLEMENTATION_MATRIX.md) · [`../PROGRESS.md`](../PROGRESS.md)。

## 当前实现边界

仓库已有 Kernel 契约生成类型与校验（`kernel-contracts`）、legacy Task create/get SQLite repository，以及不可连接的 Rust Value preflight / registration / typed dispatcher / handlers（`kernel-kcp`）。根目录有零依赖 Node/pnpm 工作区基座，但**没有** `ts/*` 包、可发布多语言 SDK、raw KCP 客户端或完整 conformance runner。

`kernel-sqlite` 与 `kernel-kcp` 的 repository / preflight / dispatcher / handler / backend 类型都是 `agentd` 内部边界，不是外部 SDK；response Schema validator 内置，Extension 或调用方不能替换该门。生产 `SystemKernelClock` / `RandomKernelIdGenerator` 属于未来 `agentd` 内部依赖，不是 SDK 能力。

## Extension SDK Base 与 Optional Profile

Extension SDK Base 指通用 Extension/Capability 协议与生命周期，是基础产品仍需完成的必做能力；旧草案“SDK Core”仅指本层历史别名，不得与 Shittim Core 混淆。Optional Profile package 在源码与 Schema 上依赖 SDK public contracts，再依赖 Core public contracts；Core / Extension SDK Base 不反向引用 Profile 类型。运行时协商方向是 `Core host -> SDK host boundary -> negotiated Profile implementation`。

Computer Use 只作为未来可选 Extension Profile，不是 Core 能力；当前没有 Computer Use Schema、生成包、Profile composition contract 或 Provider。

SDK 类型必须从 JSON Schema 2020-12 唯一源生成，不能手写与 Kernel 契约平行的类型。生成/兼容策略见 [ADR-0002](../../adr/0002-schema生成与兼容策略.md)。active KCP SDK 只生成 TaskCreate v2 root-only；Child Task 不新增 KCP create 方法，唯一入口是 Core 的 `kernel.task/task.child.create` Action。

## 设计边界

- Extension 默认进程外，不加载进 `agentd` 地址空间；
- 只能通过公开 Extension protocol 被 Kernel 调用；
- 不能直接互调、直接调用 Privilege Broker、写 Task/Policy/Audit Kernel 事实，也不能直接创建 Child Task；
- Native Extension 权限声明必须区分 OS-enforced、host-enforced 与 declaration-only；Profile 不得把 declaration-only 伪装成已执行的权限；
- Extension RPC 与 KCP 是不同协议，不能共享 Envelope 或混用方法目录；
- Profile/Extension 只消费 Core 注入的 PermissionDecision/Approval v2 引用；不得判断 material fingerprint 等价、复用/失效 Approval 或签发 Lease。

## 未来 KCP SDK 边界

未来客户端 SDK 只生成 active KCP method/version。`task.create` 只生成 v2 root-only payload；v1 仅保留 Schema/fixture 历史验证资产，production 不设 legacy migration/reader 包（ADR-0009），不能与 active client 入口重载或自动 fallback。

SDK 对 `task.create` 的 deadline 恢复必须保留同一 idempotency key 与同一业务投影；收到 `deadline_exceeded` 或 `internal_error` 时不得假定 commit 未发生。`retryable=true` 也不授权换新 key 或盲目创建第二个 Task。

Extension SDK 公开合同若携带 PermissionDecision/Approval 引用，只能使用 Core 生成的 v2 稳定 refs/fingerprints；不得导出 `isMaterialEquivalent`、`approveImplicitly` 或直接 ChildTask create 等越权 helper。

## 未来生成物

实现阶段将依据正式 Schema 生成 Extension SDK Base 产物：

- Manifest 和 Capability 类型；
- handshake/invoke/cancel/progress/health/error/event 类型；
- Rust/TypeScript/Python SDK 类型与 validator；
- conformance fixtures 与兼容测试。

## 实现者阅读顺序

1. [`AGENT.md`](../../AGENT.md)
2. [`specs/EXTENSION_SDK.md`](../../specs/EXTENSION_SDK.md)
3. [`specs/IMPLEMENTATION_CONTRACTS.md`](../../specs/IMPLEMENTATION_CONTRACTS.md)
4. [`specs/SECURITY_PRIVILEGE.md`](../../specs/SECURITY_PRIVILEGE.md)
5. [`specs/CONFORMANCE.md`](../../specs/CONFORMANCE.md)

## 与 KCP 的关系

KCP 服务于 Kernel 客户端；Extension RPC 服务于 Kernel 与 Extension/Provider 进程。KCP 当前首批八个方法不包含 Extension invoke；Extension RPC 的“JSON-RPC 风格”描述不意味着 KCP 使用 JSON-RPC。

## 发布前条件

1. Schema 源与生成命令存在且可重复；
2. 生成物与 source schema 无漂移；
3. SDK conformance 测试通过；
4. 文档明确 Extension SDK Base 与 Optional Profile 的边界，以及 OS-enforced、host-enforced、declaration-only enforcement class；
5. 不提供绕过 Kernel、直接调用 Broker 或伪造 Kernel Event 的接口。

## 权威来源

- Extension 生命周期与协议：[`../../specs/EXTENSION_SDK.md`](../../specs/EXTENSION_SDK.md)
- KCP 与 Schema 生成：[`../../specs/IMPLEMENTATION_CONTRACTS.md`](../../specs/IMPLEMENTATION_CONTRACTS.md)
- Conformance：[`../../specs/CONFORMANCE.md`](../../specs/CONFORMANCE.md)
