# Shittim SDK 文档

## 当前状态

仓库已有首批 Kernel 契约生成类型与校验 API（`kernel-contracts`）、Kernel 内部 Task create/get SQLite repository，以及不可连接的 Rust Value preflight、registration/typed dispatcher 与 application handlers（`kernel-kcp`）；其中生产 `SystemKernelClock` / `RandomKernelIdGenerator` 仍属于未来 `agentd` 的内部依赖，不是 SDK 能力。根目录已有零依赖 Node/pnpm 工作区基座（工具链钉死与 smoke），但**没有** `ts/*` 包、可发布的多语言 SDK 包、raw KCP 客户端实现或完整 conformance runner。`kernel-sqlite` repository（含 `WriteTransaction`、crate-private Outbox helper、`mark_delivered` convenience 的事务边界）与 `kernel-kcp` 的 preflight/dispatcher/handler/backend 类型都是 `agentd` 内部边界，不是外部 SDK；response Schema validator 完全内置且不在 public SDK API 中，Extension 或调用方不能替换该门。

## Extension SDK Base 与 Optional Profile 分层

Extension SDK Base 指通用 Extension/Capability 协议与生命周期，是基础产品仍需完成的必做能力；旧草案若出现“SDK Core”，只可视为本层历史别名，不能与 Shittim Core 混淆。Optional Profile package 在源码与 Schema 上依赖 SDK public contracts，再依赖 Core public contracts；Shittim Core/Extension SDK Base 不反向引用 Profile 类型。运行时则由 `Core host -> SDK host boundary -> negotiated Profile implementation` 发起协商、grant、invoke 与 cancel，两张图不可混称为同一依赖方向。Computer Use 只作为未来可选 Extension Profile，不是 Core 能力；当前没有 Computer Use Schema、生成包、Profile composition contract 或 Provider。

SDK类型必须从JSON Schema 2020-12唯一源生成，不能手写与Kernel契约平行的类型。生成/兼容策略见[`ADR-0002`](../../adr/0002-schema生成与兼容策略.md)。active KCP SDK只生成TaskCreate v2 root-only；Child Task不新增KCP create方法，而是Kernel-local Action。

## 未来 KCP SDK 边界

未来客户端SDK只生成active KCP method/version。`task.create`只生成v2 root-only payload；v1只能放在明确命名的legacy migration/reader包中，不能与active client入口重载或自动fallback。Child Task没有KCP SDK create方法，调用方只能提交/推进Core Action合同。

SDK 对 `task.create` 的 deadline 恢复必须保留同一 idempotency key 与同一业务投影；收到 `deadline_exceeded` 或 `internal_error` 时不得假定 commit 未发生。`retryable=true` 也不授权换新 key 或盲目创建第二个 Task。

Extension SDK公开合同若携带PermissionDecision/Approval引用，只能使用Core生成的v2稳定refs/fingerprints；Profile/Extension不得判断material等价、签发Approval或直接物化Child Task。

## 文档

- [Extension SDK](extension-sdk.md)

## 权威来源

- Extension 生命周期与协议：[`../../specs/EXTENSION_SDK.md`](../../specs/EXTENSION_SDK.md)
- KCP 与 Schema 生成：[`../../specs/IMPLEMENTATION_CONTRACTS.md`](../../specs/IMPLEMENTATION_CONTRACTS.md)
- Conformance：[`../../specs/CONFORMANCE.md`](../../specs/CONFORMANCE.md)

## 发布前条件

1. Schema 源与生成命令存在且可重复；
2. 生成物与 source schema 无漂移；
3. SDK conformance 测试通过；
4. 文档明确 Extension SDK Base 与 Optional Profile 的边界，以及 OS-enforced、host-enforced、declaration-only enforcement class；
5. 不提供绕过 Kernel、直接调用 Broker 或伪造 Kernel Event 的接口。
