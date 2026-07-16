# Shittim SDK 文档

## 当前状态

仓库已有首批 Kernel 契约生成类型与校验 API（`kernel-contracts`），以及 Kernel 内部的 Task create/get SQLite repository；`system.ping` / `task.create` / `task.get` 的未来 typed application handler 边界已经闭合，但代码尚未实现。当前**没有**可发布的多语言 SDK 包、KCP 客户端实现或完整 conformance runner。`kernel-sqlite` 的 Rust repository API 是 `agentd` 内部持久化边界，不是外部 SDK，也不能被 Extension 绕过 Kernel 直接调用。

SDK 类型必须从 JSON Schema 2020-12 唯一源生成，不能手写一套与 Kernel 契约平行的类型。生成和兼容策略见 [`ADR-0002`](../../adr/0002-schema生成与兼容策略.md)；当前生成命令见 [`../api/schema-generation.md`](../api/schema-generation.md)。

## 未来 KCP SDK 边界

未来客户端 SDK 只能处理已经公开的 KCP Envelope 与方法 payload：发送 Command/Query、按 `request_id` 配对 Response，并依**原请求方法**选择成功 payload Schema，因为 Response 本身没有 method discriminator。SDK 不拥有 Kernel 时钟、Task/Scope/Origin/receipt/Audit/Event ID、correlation/dedup、repository transaction 或 post-commit Publisher intent。

SDK 对 `task.create` 的 deadline 恢复必须保留同一 idempotency key 与同一业务投影；收到 `deadline_exceeded` 或 `internal_error` 时不得假定 commit 未发生。`retryable=true` 也不授权换新 key 或盲目创建第二个 Task。

本批不实现 SDK，不提供 raw transport/client，也不把内部 typed handler/backend trait 公开为 SDK API。

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
4. 文档明确 OS-enforced、host-enforced、declaration-only 边界；
5. 不提供绕过 Kernel、直接调用 Broker 或伪造 Kernel Event 的接口。
