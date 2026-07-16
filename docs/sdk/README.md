# Shittim SDK 文档

## 当前状态

仓库已有首批 Kernel 契约生成类型与校验 API（`kernel-contracts`），以及 Kernel 内部的 Task create/get SQLite repository，但**没有**可发布的多语言 SDK 包、KCP 客户端实现或完整 conformance runner。`kernel-sqlite` 的 Rust repository API 是 `agentd` 内部持久化边界，不是外部 SDK，也不能被 Extension 绕过 Kernel 直接调用。

SDK 类型必须从 JSON Schema 2020-12 唯一源生成，不能手写一套与 Kernel 契约平行的类型。生成和兼容策略见 [`ADR-0002`](../../adr/0002-schema生成与兼容策略.md)；当前生成命令见 [`../api/schema-generation.md`](../api/schema-generation.md)。

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
