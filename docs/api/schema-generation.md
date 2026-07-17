# Schema 生成与契约类型

> 状态：已落地 target-scoped language-neutral graph + Rust projection renderer。JSON Schema 是唯一人工源；当前没有 TypeScript/Python 生成物。根目录已有零依赖 Node 24.18.0 / pnpm 11.3.0 工作区基座，但**未**接入 Schema→TS 生成；声明 `typescript` 时 generate 整体 fail，无部分写。

## 权威边界

- 字段、枚举、错误与兼容规则的事实源：`specs/` 与 `schemas/source/**/*.json`。
- 索引：`schemas/manifest.json`。
- Rust 生成物：`rust/crates/kernel-contracts/src/generated/`，禁止手改。
- 项目代码许可证：根目录 [`LICENSE`](../../LICENSE)（Apache-2.0）。
- CLI：`schema-tool`；运行时库：`kernel-contracts`。

## 当前产物

| 产物 | 路径 | 说明 |
|---|---|---|
| Schema 源 | `schemas/source/{audit,common,task,policy,event,kcp}/` | 41 个 Draft 2020-12 schema |
| Manifest | `schemas/manifest.json` | `$id`、source、kind、兼容与 `generation_targets`（当前 41 entries 均为 `[rust]`）；**`id_base`** 是权威 URL path 命名空间：必须 canonical absolute `http(s)`、无 fragment、以 `/` 结尾；每个 entry `$id` 必须落在该 namespace（scheme/host/port/path 组件语义，禁止字符串前缀伪装）；root `$id` / manifest id 必须 canonical absolute `http(s)`、无 fragment |
| 中立 graph | `schema-tool` `contract_model` | target-scoped `TargetContractGraph`；`ContractTypeId` = `$id` + 严格 RFC6901 JSON Pointer（root 空指针；fragment 用真实 definition pointer）；`TypeUse` 携带 source use-site；`TypeShape` 支持 scalar/any/array/nullable/object/enum/const；保存 `schema_title`/`SourceSchemaMetadata`，**无** language name/logical_title/hint/pascal；同一 `$defs` 在 graph 中唯一 node |
| 身份分离 | `ContractTypeId` ≠ `RustDeclarationId` | 中立 graph 按 canonical fragment 唯一；Rust renderer 按 **use-site lineage**（`Vec<SourceUseSite>`）投影 declaration，Rust name 独立于 identity；`active_by_canonical` 回边复用防止 lineage 无限增长；同一 `$defs` 可投影为多个 declaration（如 `PolicyRuleCreatedBy`/`PolicyRuleUpdatedBy`）；whole-schema `$ref` 为 `SharedRoot` |
| 生成目标模型 | `GenerationTarget` + `TargetPlan` | `rust` / `typescript`；非空、无重复、canonical order；每 target 独立 roots/closure（外部 `$ref`、local fragment 递归、envelope payload）；未知值 serde 失败；**无** `GenerationTarget::ALL` 闭集硬编码 |
| Artifact 规划 | `codegen` `ArtifactPlan::try_new` | 唯一构造入口：校验 roots/path/duplicate/component-safe/planned prefixes；字段全部 private，只读 getters `artifacts`/`roots`/`planned_prefixes`；`GeneratedArtifact` 字段 private，经 `new` + getters，最终 path 由 `try_new` 验证；`plan_artifacts` 必须走它；拒绝 `generated_evil` 前缀伪装、absolute、traversal、duplicate、outside root；unplanned empty/nonempty dir、missing/extra、symlink fail closed；先全部成功再写 |
| Rust projection | `RustProjection` / `project_rust` | 公开 API 只保留 `project_rust`、`render_types_module_from_projection`、`render_typed_module_from_projection`、`render_catalog_module`；禁止 graph 级 convenience re-project；`plan`/`render_rust_artifacts`/`lower_and_render_rust` 只 project 一次；catalog 直接读 graph |
| 生成类型 | `generated/types.rs` | struct/enum、const 单值类型、`NullOnly`；**string enum** 统一生成 `pub const ALL: &'static [Self]`（Schema declaration order 闭集，与 variants/`as_str` 共用同一有序 mapping；string const 不生成 ALL；nullable enum 过滤 null，null 仍由 `Option` 表达）；自动 `#[cfg(test)] mod string_enum_contracts` 覆盖全部 string enum（长度/顺序/`as_str` 唯一/serde roundtrip，共享 helper，无手写类型目录）；递归 SCC 对 direct Named 插入 `Box`，Array 不 box；optional 递归钉死 `Option<Box<T>>`（禁止 `Box<Option` / 仅因递归的 `Vec<Box`） |
| 生成目录 | `generated/catalog.rs` | 由 manifest 生成的 embedded schema 与方法/事件闭集（不经 projection） |
| Typed decode | `generated/typed.rs` | 从 envelope discriminator enum 与 `allOf if/then payload.$ref` 一一映射自动派生；**与 types 共用同一 `RustProjection`**，无平行 `project_envelope_field_type`；`decode` 先 Schema validation，再调用有明确前置条件的 `decode_after_validation` |
| JCS 向量 | `schemas/examples/jcs/`、`schemas/fixtures/kcp/task_create_normalized_hash.v1.json` | RFC 8785 示例、UTF-16 排序及 task.create receipt/idempotency 复合 hash fixture |
| 检查脚本 | `scripts/check-schema.sh` | 仓库当前统一门（历史名保留）：先 `node scripts/check-node-toolchain.mjs`（要求调用者 PATH 已指 Node 24.18.0 / pnpm 11.3.0），再 generate×2、meta/check、fmt、clippy、test、generated drift、最后 `FILE_MANIFEST` Git source set check；**不是** Rust-only |

## 命令

```bash
# 统一门：PATH 须先指到 Node 24.18.0（例如 export PATH="$HOME/.local/share/pnpm:$PATH"）
# 未提供跨平台 npm `check:all`；请直接执行本脚本。
export PATH="$HOME/.local/share/pnpm:$PATH"
./scripts/check-schema.sh

cargo run --manifest-path rust/Cargo.toml -p schema-tool -- --repo-root "$PWD" generate
cargo run --manifest-path rust/Cargo.toml -p schema-tool -- --repo-root "$PWD" check
cargo run --manifest-path rust/Cargo.toml -p schema-tool -- --repo-root "$PWD" \
  validate --schema https://schemas.shittim.local/v1/common/actor.json \
  --instance /path/to/instance.json
cargo run --manifest-path rust/Cargo.toml -p schema-tool -- --repo-root "$PWD" \
  canonicalize /path/to/file.json --hash
```

## 指针、URI 与 `$ref` 规则

1. **Join**：`resolve_ref` 使用 `url::Url::join` 相对 base schema `$id`，同时支持 local（`#/pointer`）、absolute（`https://...`）与 relative（`./x.json#/pointer`）。三者解析到同一节点时 **identity 相同**。
2. **Fragment 处理顺序**（仅一次）：
   - 严格校验每个 `%` 后跟两个 hex digit（malformed / truncated 失败）；
   - `percent_encoding::percent_decode` + `decode_utf8`（非 UTF-8 失败）；
   - 再按 RFC 6901 解析 JSON Pointer。
3. **JSON Pointer 本身**：canonical encode（`~`→`~0`，`/`→`~1`）；**允许字面 `%`**（URI 解码已在上一步完成）；array index 拒绝 `01`、`-`、非十进制。
4. **`$anchor` / 非 pointer fragment**：不支持，fail closed。
5. **nested 非 root `$id`**：不支持，错误信息包含真实 pointer 位置。
6. **root `$id` / manifest id**：必须 canonical absolute `http(s)` URI，**无 fragment**（`Url` 序列化 exact equality）。
7. **`manifest.id_base`**：canonical absolute `http(s)`、无 fragment、**必须以 `/` 结尾**；每个 entry `$id` 必须在其 URL path namespace 下（比较 scheme/host/port/path 组件，不是裸 `starts_with`；拒绝 `v1_evil` 前缀伪装）。Relative `$ref` 解析后只要命中 manifest registry 即可。
8. **walk_refs / target closure**：seen-set 按 resolved `ContractTypeId`，不是 raw `$ref` 字符串；relative external `$ref` 依赖同样必须声明同一 target。
9. **解析结果类型**：唯一 `ResolvedSchemaRef`（无平行 `ResolvedRef`/`OwnedResolvedRef` 命名）。

## 生成器支持矩阵

### 生成形状

- 支持：`object`、`properties`、`required`、`array/items`、string `enum`、string/integer/boolean/null `const`、`$ref`、单一非 null 类型与 `null` 的联合、nullable `oneOf: [null, T]`。
- **String enum 闭集 `ALL`**（已完成）：在通用 `ProjectedShape::StringEnum` 路径生成 `pub const ALL: &'static [Self]`，顺序严格为 Schema enum declaration order；variants / `ALL` / `as_str` 共用 renderer-local 有序 mapping，禁止按字典序重排，也不硬编码 `TaskStatus`/`ActionStatus`/schema id。string const 不生成 `ALL`。nullable string enum 在 lowering 已过滤 `null`，`ALL` 只含非 null 成员，字段仍是 `Option<Enum>`，`None` 序列化为显式 `null`。wire→variant 碰撞 fail closed，错误含 type/wire/variant。
- **自动合同测试**：`types.rs` 尾部 `#[cfg(test)] mod string_enum_contracts` 按 projection 为每个 string enum 生成调用；共享 `assert_string_enum_contract` 验证 ALL 长度/顺序、`as_str` 唯一、serde `to_value` 等于 wire string、`from_value` roundtrip。**不**手写类型目录。
- **domain-task 手写 catalog 仍未删除**（下一 commit）：`domain-task` 的 `TASK_STATUS_CATALOG` / `ACTION_STATUS_CATALOG` 仍存在；本批只交付生成侧 `ALL`，不改 catalog/typed/mod，也不替换 domain-task 消费点。
- Serde omission 由 Schema 元数据确定性推导，不手改 generated：
  - `required=false` 且属性类型不允许 `null` → `Option<T>` + `#[serde(skip_serializing_if = "Option::is_none")]`，`None` 省略字段；
  - `required=true` 且允许 `null` → `Option<T>` 且**不** skip，`None` 仍输出显式 `null`；
  - `required=false` 且允许 `null` → 保持 `None -> null`（不 skip），避免无合同的 wire 变化；
  - nullability 从 `type`/`type` 数组/`oneOf [null,T]`/nullable string enum/`const null`/`$ref` 解析结果推导；无法可靠推导则生成失败，不猜；
  - `$ref` 只允许 `title` / `description` 注释 sibling；带 `type`、约束或其它 shape sibling 时生成失败。
- `additionalProperties: true` 且没有声明字段的对象，才生成 `serde_json::Value`。
- KCP/Event 条件 payload：discriminator property 闭集 enum 与每个 `allOf` 分支的 `if.properties.<discriminator>.const` + `then.properties.payload.$ref` 必须一一对应、无重复。
- payload `$ref` 必须解析到 manifest 中的完整 Schema。
- `type` 含多个非 null 分支、歧义 `oneOf`、schema-valued `additionalProperties`、`anyOf`、`not`、`patternProperties`、`dependentSchemas`、`prefixItems`、`contains`、`unevaluatedProperties` 等形状关键字明确失败。

### 递归 layout（SCC Box）

投影完成后建立 **direct-value dependency graph**：

| 边类型 | 规则 |
|---|---|
| `Named` | direct |
| `Nullable` / 字段 `Optional` | 仍 direct（解包后看内层） |
| `Array` | **indirect**（不建 direct 边） |

对每个声明做确定性 SCC；**同一 recursive SCC 内的 direct `Named` 边**包装为 `RustTypeExpr::Boxed`。optional 自递归钉死 `Option<Box<T>>`（禁止 `Box<Option<T>>`）；Array 递归保持 `Vec<T>` / `Option<Vec<T>>`，不因递归插入 `Vec<Box<T>>`。三节点 direct SCC（A→B→C→A）每条 direct 边均 box。非递归 sibling use-site 对同一 canonical 生成多个 Nominal declaration；仅 active backedge 复用。当前生产 41 schema 无环，输出与 HEAD **byte-identical**。

### Validation-only 关键字

`minimum/maximum`、`minLength/pattern/format`、`minItems/uniqueItems`、`allOf`、`if/then/else` 等保留在 source schema，由 `jsonschema` 0.28 Draft 2020-12 runtime validator 强制执行。

## Envelope 分析（唯一路径）

删除宽门 `has_conditional_envelope_root`。对每个 envelope：

1. 统计 allOf 分支上 **whole-schema payload `$ref`** 数量；
2. **0 个** → 合法 **untyped** `None`（response envelope 成功路径：只有 `status` 条件，无方法级 payload binding）；
3. **≥1 个** → 每个 branch 必须完整且与 discriminator enum **双射**，否则 error。

Command/Query/Event 有方法/事件 discriminator，因此生成 typed decode。Response Envelope 只有 `status = ok | error`，成功 payload 是带 `schema_version` 的开放对象，**intentionally untyped**：不生成 `TypedKcpResponseEnvelope`。response root object 仍进入 graph，供 untyped wire 类型使用。Typed envelope 字段复用与 `types.rs` **同一套** projection/layout。

## Const、null 与 JCS

- const 生成单值 enum/newtype；JSON `null` 使用 `NullOnly`。
- optional/non-null 与 required-nullable 的 wire 语义不同：前者 `None` 省略，后者 `None` 必须是显式 `null`。
- ApprovalRecord `target` 当前仍是全部可选成员的对象：exactly-one 是已知缺口。
- RFC 8785 由 `serde_json_canonicalizer = 0.3.2` 实现。

## 明确未实现

- TypeScript/Python 类型渲染与产物（target-scoped graph 与 `typescript` target 校验已完成；声明 `typescript` 时 generate 整体 fail closed，无部分写）；
- `agentd`、KCP bytes/frame/transport 与任何可运行服务端；
- Task 更新/list、Action、PermissionDecision 等后续业务 repository 与 KCP handler；
- Delegation authority 正向查询；`task.create` 中非 null `delegation_ref` 当前固定失败；
- `$anchor`、nested 非 root `$id` 的 compound document identity 重写。

## 相关规范

- [`../../specs/IMPLEMENTATION_CONTRACTS.md`](../../specs/IMPLEMENTATION_CONTRACTS.md) §5、§6、§13
- [`../../specs/CONFORMANCE.md`](../../specs/CONFORMANCE.md) §5
- [`../../adr/0002-schema生成与兼容策略.md`](../../adr/0002-schema生成与兼容策略.md)
