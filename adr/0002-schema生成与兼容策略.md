# ADR-0002：Schema 生成与兼容策略

- 状态：accepted
- 日期：2026-07-16

## 背景

KCP、持久对象、Event payload 和 Extension SDK 会跨 Rust、TypeScript 与未来 SDK 使用。手写平行类型会产生漂移，且 PermissionDecision hash、幂等比较和审计需要确定性 JSON 表示。

## 决策

1. 正式 Schema 使用 **JSON Schema Draft 2020-12**。
2. 人工维护的唯一源位于 `schemas/source/`；首次实现时创建 `schemas/manifest.json` 记录 `$id`、schema version、兼容关系和生成目标。
3. Rust/TypeScript/SDK 类型、validator 与可生成 API reference 片段都从 source schema 生成；生成目录标记 `GENERATED`，禁止手改。
4. canonical JSON 使用 **RFC 8785 JCS**；契约未另行指定时 hash 使用 SHA-256 小写十六进制。
5. KCP Envelope 使用 `protocol_version`，payload/持久对象/Event payload 使用 `schema_version`，两者不得混用。
6. 每个 Schema 通过 `additionalProperties` 或 `unevaluatedProperties` 明确未知字段策略；未知 enum、Policy condition 和权限语义不能默认映射为 allow。
7. CI 运行 schema meta-validation、唯一 `$id`、完整 `$ref`、示例校验、兼容检查和重复生成；第二次生成必须 byte-for-byte 无变化，生成后工作树必须 clean。
8. breaking 变化创建新 schema version 并提供迁移/兼容记录；数据迁移遵循 preflight、backup、verify、rollback。

## 备选方案

- 以 Rust struct 为唯一源：拒绝，跨语言和 JSON Schema 表达受实现语言绑架。
- 以 TypeScript interface 为唯一源：同样拒绝。
- 使用普通 `JSON.stringify`/serde 输出做 hash：拒绝，map 顺序和数值表示无法形成跨语言稳定契约。
- 手写 SDK 类型：拒绝，违反单一生成源。

## 影响

- Schema generator 选型仍需在实现时比较工具能力，但不得改变本 ADR 的输入/输出和确定性约束。
- 当前没有 `schemas/` 目录或生成物；accepted 只表示策略已确定。
