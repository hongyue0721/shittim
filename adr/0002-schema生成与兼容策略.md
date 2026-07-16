# ADR-0002：Schema 生成与兼容策略

- 状态：accepted
- 日期：2026-07-16
- 修订：2026-07-16（补充实际 crate/命令选型）

## 背景

KCP、持久对象、Event payload 和 Extension SDK 会跨 Rust、TypeScript 与未来 SDK 使用。手写平行类型会产生漂移，且 PermissionDecision hash、幂等比较和审计需要确定性 JSON 表示。

## 决策

1. 正式 Schema 使用 **JSON Schema Draft 2020-12**。
2. 人工维护的唯一源位于 `schemas/source/`；`schemas/manifest.json` 记录 `$id`、schema version、兼容关系和生成目标。
3. Rust/TypeScript/SDK 类型、validator 与可生成 API reference 片段都从 source schema 生成；生成目录标记 `GENERATED`，禁止手改。
4. canonical JSON 使用 **RFC 8785 JCS**；契约未另行指定时 hash 使用 SHA-256 小写十六进制。
5. KCP Envelope 使用 `protocol_version`，payload/持久对象/Event payload 使用 `schema_version`，两者不得混用。
6. 每个 Schema 通过 `additionalProperties` 或 `unevaluatedProperties` 明确未知字段策略；未知 enum、Policy condition 和权限语义不能默认映射为 allow。
7. CI 运行 schema meta-validation、唯一 `$id`、完整 `$ref`、示例校验、兼容检查和重复生成；第二次生成必须 byte-for-byte 无变化，生成后工作树必须 clean。
8. breaking 变化创建新 schema version 并提供迁移/兼容记录；数据迁移遵循 preflight、backup、verify、rollback。

### 实际落地选型（首批）

| 项 | 选择 |
|---|---|
| Rust 生成器 | 自有受限确定性 codegen（`schema-tool`），**不**采用 typify 作为正式路径 |
| 运行时校验 | `jsonschema` **0.28**，Draft 2020-12 |
| 生成目标 crate | `kernel-contracts`（生成类型/目录 + typed envelope decode + `validate_json` + JCS/SHA-256 API） |
| CLI | `schema-tool`：`generate` / `check` / `validate` / `canonicalize` |
| 入口脚本 | `scripts/check-schema.sh`（仅 cargo/Rust，无 Node） |
| JCS | `serde_json_canonicalizer` 0.3.2（RFC 8785）+ `sha2` 0.10 |
| TS 生成 | 尚未实现 |

生成器必须：从 Schema 解析；对当前支持的 shape 关键字保真生成；多非 null `type` union、歧义 `oneOf`、未知 shape 关键字明确失败；只有 Schema 显式 `additionalProperties: true` 的 free-form object 可生成 `JsonValue`；生成文件含 GENERATED 标识；重复生成稳定。`schemas/manifest.json` 同时生成可审查的 embedded catalog，禁止 validator 手工维护平行目录。

KCP Command/Query 与 Event 的条件 payload typed binding 直接解析 Envelope Schema：discriminator enum 必须与 `allOf` 中每个 `if.properties.<discriminator>.const` → `then.properties.payload.$ref` 一一对应；payload 类型名来自 manifest title，variant 与 decode match 由生成器确定性输出。不得使用手写方法目录、expected 列表或 typed 模板。弱 envelope struct 不作为业务解码入口。const 字段生成单值类型，JSON null 生成 `NullOnly`。

条件关键字 `if`/`then`/`else`/`allOf` 保留给运行时校验，不靠手写平行业务规则绕过 Schema。

## 备选方案

- 以 Rust struct 为唯一源：拒绝，跨语言和 JSON Schema 表达受实现语言绑架。
- 以 TypeScript interface 为唯一源：同样拒绝。
- 使用普通 `JSON.stringify`/serde 输出做 hash：拒绝，map 顺序和数值表示无法形成跨语言稳定契约。
- 手写 SDK 类型：拒绝，违反单一生成源。
- 直接使用 typify 0.7.0：评估后未采用为正式路径，因其对 2020-12 条件 Schema 与项目约束的保真度不足；改为自有受限生成器 + jsonschema 运行时。

## 影响

- Schema 变更必须走 source → generate → check，不得手改 `generated/`。
- 首批仅 Rust 目标；TypeScript/Python 生成需后续扩展 `generation_targets` 与生成器，不得手写平行类型。
