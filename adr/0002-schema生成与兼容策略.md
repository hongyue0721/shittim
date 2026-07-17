# ADR-0002：Schema 生成与兼容策略

- 状态：accepted
- 日期：2026-07-16
- 修订：2026-07-16（补充实际 crate/命令选型）；string enum `ALL` 闭集与自动合同测试已落地，domain-task 已删除手写 status catalog 并直接消费生成闭集

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
| 入口脚本 | `scripts/check-schema.sh`（仓库当前统一门：先 Node/pnpm 硬门 `check-node-toolchain.mjs`，再 Rust generate/check/fmt/clippy/test/generated drift，最后 Node `update-file-manifest.mjs --check`；清单本身非 Schema 产物；脚本名历史保留，**不是** Rust-only） |
| JCS | `serde_json_canonicalizer` 0.3.2（RFC 8785）+ `sha2` 0.10 |
| TS 生成 | 尚未实现 |
| 共享 IR / 目标模型 | 已落地：target-scoped language-neutral `TargetContractGraph`；`ContractTypeId` = schema `$id` + 严格 RFC6901 JSON Pointer（≠ `RustDeclarationId`）；use-site lineage 投影多个 Rust declarations；`url`+percent-encoding 解析 local/absolute/relative `$ref`；SCC Box 递归 layout；`GenerationTarget`（rust/typescript，无 ALL 闭集）+ TargetPlan 闭包；Rust projection renderer 已实现；TS renderer 仍未实现；response envelope intentionally untyped |

生成器流水线：`SchemaRegistry -> TargetPlan/TargetSchemaSet -> TargetContractGraph -> RustProjection (single project_rust + recursive SCC layout) -> ArtifactPlan::try_new`。IR identity 由 schema `$id` + 严格 JSON Pointer 组成（root pointer 为空；`$defs`/inline 使用真实 definition pointer），禁止以语言名字作 key；中立 graph 不得携带 rust/typescript 名、logical_title/hint/pascal、include 路径或 generated 路径。`ContractTypeId` 与 `RustDeclarationId` 分离：whole-schema `$ref` 共享 `SharedRoot`；fragment use-site 以 `NominalInstantiation { canonical, use_site_lineage: Vec<SourceUseSite> }` 克隆，Rust name 独立；`active_by_canonical` 回边复用防止 lineage 无限。`manifest.id_base` 是权威 URL path 命名空间：必须 canonical absolute `http(s)`、无 fragment、以 `/` 结尾；每个 entry `$id` 必须落在该 namespace（scheme/host/port/path 组件语义，禁止裸 `starts_with` 前缀伪装）。`$ref` 解析：`Url::join` 支持 local/absolute/relative；fragment 先严格 `%HH` 校验，再 percent-decode UTF-8 一次，再 RFC 6901 解析；pointer 本身允许字面 `%`；`$anchor`、nested 非 root `$id`、root 非 canonical absolute id 均 fail closed；relative external 解析后命中 registry 即可，但 target 闭包仍强制依赖同 target。递归 layout：Named/Nullable/Optional 为 direct 边，Array 为 indirect；同 SCC direct Named 包装 `Box`（optional 钉死 `Option<Box<T>>`，禁止 `Box<Option` / 仅因递归的 `Vec<Box`）。生成器必须：对当前支持的 shape 关键字保真 lower；多非 null `type` union、歧义 `oneOf`、未知 shape 关键字明确失败；只有 Schema 显式 `additionalProperties: true` 的 free-form object 可成为 `AnyJson`；生成文件含 GENERATED 标识；当前 41 无环输出与 HEAD bytes 一致。`schemas/manifest.json` 的 `generation_targets` 必须非空、无重复、按 canonical 顺序（rust then typescript）；每 target 显式 roots，外部 `$ref`（含 relative）与 local fragment 递归依赖及 envelope payload 必须同 target 闭包。`ArtifactPlan` 只能经 `try_new` 构造：校验 roots/path/duplicate/component-safe 并计算 planned directory prefixes；path/root component-safe（拒绝 `generated_evil` 前缀伪装）、traversal/absolute/duplicate 与 unplanned dir/symlink/extra/missing 均 fail closed。`RustProjection` 只计算一次，types/typed 从同一实例渲染；catalog 直接读 graph。同 target 下不同 declaration 映射到同 symbol 时 renderer 必须列出 canonical/use-site/name 失败。未来 TS 不得复制 lowering 语义，只能消费同一 target-scoped graph。

KCP Command/Query 与 Event 的条件 payload typed binding：唯一 envelope 分析——0 个 whole-schema payload `$ref` => untyped `None`；≥1 则所有 branch 完整且与 discriminator enum 双射，否则 error。Response envelope intentionally untyped。typed/types 共用同一 projection/layout，不平行 lower wire 字段。不得使用手写方法目录、expected 列表或 typed 模板。const 字段生成单值类型，JSON null 生成 `NullOnly`。string enum 在通用 projection 路径生成 declaration-order `pub const ALL: &'static [Self]`（与 variants/`as_str` 同一 mapping；const 不生成 ALL；nullable 过滤 null），并在 `types.rs` 自动生成 string enum 合同测试；`domain-task` 的 NxN、terminal 和 proptest 遍历直接消费 `TaskStatus::ALL` / `ActionStatus::ALL`，不再维护手写完整状态目录。

条件关键字 `if`/`then`/`else`/`allOf` 保留给运行时校验，不靠手写平行业务规则绕过 Schema。

## 备选方案

- 以 Rust struct 为唯一源：拒绝，跨语言和 JSON Schema 表达受实现语言绑架。
- 以 TypeScript interface 为唯一源：同样拒绝。
- 使用普通 `JSON.stringify`/serde 输出做 hash：拒绝，map 顺序和数值表示无法形成跨语言稳定契约。
- 手写 SDK 类型：拒绝，违反单一生成源。
- 直接使用 typify 0.7.0：评估后未采用为正式路径，因其对 2020-12 条件 Schema 与项目约束的保真度不足；改为自有受限生成器 + jsonschema 运行时。

## 影响

- Schema 变更必须走 source → generate → check，不得手改 `generated/`。
- 当前 41 个 manifest entry 仍只声明 `[rust]` 并只输出 Rust 四文件；TypeScript 目标模型与闭包校验已具备，但 TS 代码生成尚未实现，不得手写平行类型。
