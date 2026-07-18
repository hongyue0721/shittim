# Schema 生成与契约类型

> 状态日期：2026-07-18。Manifest v2/walker、LockPort exact 矩阵与 TransactionFs fixed-point exact 矩阵已完成独立验收；当前仅声明单root/Linux real-platform，control-flow/fault conformance不等同真实断电介质模型，multi-root、non-Linux platform port与业务v2未完成。manifest **仅接受 v2**，root `id_base` 为 `https://schemas.shittim.local/`，41个既有 source `$id` 由component retained-ID namespace显式保留，fixture持续核对41份source hash。四个Rust生成文件相对迁移前基线未变是提交`7fb25cf`的一次性验收证据，不是永久fixture门；后续合法生成变更由Git基线和正常generate/check审阅。当前41个source与Rust生成物的对象版本仍均为v1，但不能整体标legacy：`task.create` request v1是legacy validation-only，其余首批method v1仍active；共享Actor/状态等v1继续被active合同引用；Approval/PD/EventEnvelope等在v2落地后按各自lifecycle转legacy。ADR-0006/0007要求的业务v2、可用MethodVersionBinding、TS renderer和KCP切换均尚未生成或实现。

## 权威边界

- 字段、枚举、错误与兼容规则的事实源：`specs/` 与 `schemas/source/**/*.json`。
- 索引：`schemas/manifest.json`。
- Rust 生成物：`rust/crates/kernel-contracts/src/generated/`，禁止手改。
- 项目代码许可证：根目录 [`LICENSE`](../../LICENSE)（Apache-2.0）。
- CLI：`schema-tool`；运行时库：`kernel-contracts`。

## 当前产物

| 产物 | 路径 | 说明 |
|---|---|---|
| Schema 源 | `schemas/source/{audit,common,task,policy,event,kcp}/` | 41 个 Draft 2020-12 schema；当前 source Schema 不含 Computer Use Profile Schema |
| Manifest | `schemas/manifest.json` | **仅manifest v2**：顶层为root `id_base=https://schemas.shittim.local/`、显式 `components`、required typed `method_version_bindings`和entries；本轮该binding数组必须显式为空，loader拒绝任意非空值，未生成八方法表。未来独立切片以真实业务v2 source替换empty gate，并生成完整binding catalog与验证。entry使用`component`（无`domain` alias）。component声明canonical direct namespace、跨component `allowed_refs`及`retained_ids`；当前41个历史`/v1/` `$id`还由`schemas/fixtures/manifest/retained_ids.v1.json`逐项固定id/component/source/source SHA-256，`SchemaRegistry::load`逐项核对实际source bytes，ledger是生产gate而非test-only oracle。该ledger只记录迁移41项，未来新ID不入ledger。entry source必须通过`SchemaSourcePath`：UTF-8 JSON string、POSIX且lexically normalized、repo-relative并以`schemas/source/`开头；拒绝absolute/backslash/空/dot/dotdot/prefix trick，实际source root/file及任一ancestor不得为symlink，canonical regular file必须仍位于canonical source root内。`LoadedSchema::source()`保存验证后相对路径，catalog renderer仅消费该事实。retained与component-native ID namespace互斥。四份Rust生成物hash只记录在ADR/IC为`7fb25cf`迁移验收证据，绝不进入持续fixture。registry以URL scheme/host/port/path组件而非字符串prefix校验root/component归属，拒绝prefix spoof、default-port、dot/double slash、encoded component等非canonical形状；跨component `$ref`在load时先过allow-list，随后generation target closure仍独立检查。当前41 entries均为`[rust]`；没有Computer Use Profile生成entry或生成包。 |
| 中立 graph | `schema-tool` `contract_model` | target-scoped `TargetContractGraph`；`ContractTypeId` = `$id` + 严格 RFC6901 JSON Pointer（root 空指针；fragment 用真实 definition pointer）；`TypeUse` 携带 source use-site；`TypeShape` 支持 scalar/any/array/nullable/object/**tagged union**/enum/const；保存 `schema_title`/`SourceSchemaMetadata`，**无** language name/logical_title/hint/pascal；同一 `$defs` 在 graph 中唯一 node |
| 身份分离 | `ContractTypeId` ≠ `RustDeclarationId` | 中立 graph 按 canonical fragment 唯一；Rust renderer 按 **use-site lineage**（`Vec<SourceUseSite>`）投影 declaration，Rust name 独立于 identity；`active_by_canonical` 回边复用防止 lineage 无限增长；同一 `$defs` 可投影为多个 declaration（如 `PolicyRuleCreatedBy`/`PolicyRuleUpdatedBy`）；whole-schema `$ref` 为 `SharedRoot` |
| 生成目标模型 | `GenerationTarget` + `TargetPlan` | `rust` / `typescript`；非空、无重复、canonical order；每 target 独立 roots/closure（外部 `$ref`、local fragment 递归、envelope payload）；未知值 serde 失败；**无** `GenerationTarget::ALL` 闭集硬编码 |
| Artifact 规划与提交 | `codegen` `ArtifactPlan::try_new` + `artifact_transaction` | 唯一plan构造入口要求distinct roots恰好一个，并校验path/duplicate/component-safe/planned prefixes；拒绝0/multi-root、`generated_evil`、absolute、traversal、duplicate、outside root。generate在完整plan/render后进入当前**单root/Linux real-platform verified** transaction：持久lock file使用Rust 1.97 OS advisory file lock（不unlink，owner metadata仅诊断），锁内先recover；durable `Preparing/Prepared/RollingBack/Committed` journal协调同filesystem sibling stage/backup/rollback-discard。transaction 边界在 FD advisory lock 成功且 owner metadata 发布后开始；LockPort（open/type/try-lock/owner metadata）独立验收，FD lock 权威、owner 仅诊断、不 unlink/PID 回收。TransactionFs 使用 typed `OperationEvent`（semantic phase、operation、Before/AfterSuccess、path role、journal target phase、仅用于重复操作的 occurrence）记录 mutation/durability boundary；read/metadata inspection 不进矩阵。正式 `Committed` journal rename 是提交点，之前 existing=Old/absent=Absent，之后=New。exact matrix 已通过真实 trace 自动发现 reachable target，断言即时完整snapshot、structured disposition、recover#1精确终态与recover#2 Noop；`StoredStateInvalid` 对正式journal损坏fail closed且不改变root。control-flow/fault conformance不等同真实断电介质模型；partial I/O只有明确fake effect才可声明覆盖。 |
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

`SchemaRegistry::load`先用唯一`schema_walk`遍历每份document并保存私有不可变的authoritative SchemaNode pointer index；restricted identity/ref audit、codegen support-profile audit和ref可达性均复用该walker callback，不维护另一套通用递归位置表。public `schema_at`与`resolve_ref`只承认index中的pointer；raw JSON pointer lookup仅crate-private。因此`$ref`到`/const`、`/default`、`/examples/0`、`/enum/0`即使值是`{"type":"string"}`也拒绝，而`/$defs/...`、`/properties/...`、`/items`等Schema-bearing位置可解析。

1. **Join**：`resolve_ref` 使用 `url::Url::join` 相对 base schema `$id`，同时支持 local（`#/pointer`）、absolute（`https://...`）与 relative（`./x.json#/pointer`）。三者解析到同一节点时 **identity 相同**。
2. **Fragment 处理顺序**（仅一次）：
   - 严格校验每个 `%` 后跟两个 hex digit（malformed / truncated 失败）；
   - `percent_encoding::percent_decode` + `decode_utf8`（非 UTF-8 失败）；
   - 再按 RFC 6901 解析 JSON Pointer。
3. **JSON Pointer 本身**：canonical encode（`~`→`~0`，`/`→`~1`）；**允许字面 `%`**（URI 解码已在上一步完成）；array index 拒绝 `01`、`-`、非十进制。
4. **identity/ref restricted profile与SchemaNode walker**：`schema_walk`是唯一Schema-bearing遍历实现，pre-order callback提供canonical pointer、`is_root`与object/boolean node。位置闭集为map values `properties/patternProperties/dependentSchemas/$defs/definitions`，single Schema `additionalProperties/unevaluatedProperties/propertyNames/items/contains/unevaluatedItems/contentSchema/not/if/then/else`，array Schema `prefixItems/allOf/anyOf/oneOf`；存在但容器/node类型错误立即失败。registry identity audit、`$ref` resolution/component gate和target closure复用该walker，只在Schema node检查`$ref`，不进入`const/default/examples/enum`实例数据。map key恰为`$ref/$id/$schema/$dynamicRef`只作为普通名称，其value Schema仍正常遍历。named anchor和动态/recursive identity语义当前都不支持；registry load统一拒绝`$schema`（仅root允许）、`$anchor`、`$dynamicAnchor`、`$dynamicRef`、`$recursiveAnchor`、`$recursiveRef`、`$vocabulary`，不能等到validate/generate阶段。
5. **nested 非 root `$id`**：registry load即拒绝，错误信息包含真实 pointer 位置；不允许compound document identity重写。
6. **root `$id` / manifest id**：必须 canonical absolute `http(s)` URI，**无 fragment**（`Url` 序列化 exact equality）。
7. **source path confinement**：manifest `source`须为exact normalized POSIX repo-relative路径并以`schemas/source/`开头；拒绝absolute、backslash、空/dot/dotdot segment与prefix trick。source root、file、任一ancestor symlink均拒绝；canonical regular file须仍位于canonical source root内。renderer只消费`LoadedSchema`保存的verified source事实。
8. **manifest namespace / component**：只接受schema_version `2`；root `id_base`固定canonical `https://schemas.shittim.local/`。component namespace必须是root下唯一、未编码、无dot/double-slash、无default port伪装的直接路径段；entry以`component`归属，不接受旧`domain`。required typed `method_version_bindings`当前必须为显式空数组，loader拒绝非空值；未来独立切片以真实v2 source实现IC §13.5完整binding验证和生成。41个retained ID必须精确匹配版本控制的迁移ledger（id/component/source/source hash），且`SchemaRegistry::load`核对实际source bytes SHA-256；retained不得落在任何component namespace，新component-native ID也不得占retained ID。`$ref`解析成功后，跨component引用必须出现在源component的`allowed_refs`，registry load已完成此gate；此规则与target closure独立，不能相互替代。
9. **ref closure**：component gate之后，registry ref audit与target closure都通过统一SchemaNode walker；seen-set按resolved `ContractTypeId`，不是raw `$ref`字符串；relative external `$ref`依赖同样必须声明同一target；递归cycle由seen-set闭合。
10. **解析结果类型**：唯一 `ResolvedSchemaRef`（无平行 `ResolvedRef`/`OwnedResolvedRef` 命名）。

## 生成器支持矩阵

### 生成形状

- 支持：`object`、`properties`、`required`、`array/items`、string `enum`、string/integer/boolean/null `const`、`$ref`、单一非 null 类型与 `null` 的联合、nullable `oneOf: [null, T]`、以及受限对象判别 `oneOf`。
- **TaggedUnion source profile**（已完成）：`oneOf` 由单一分类器严格分成 Nullable / TaggedUnion / Unsupported，先于 object lowering；TaggedUnion 要求 union 层唯一且 **required** 的 string enum discriminator、分支 required string const 与 enum 双射、inline或完整 `$ref` 的 closed object 分支。每个分支同时保存 canonical object identity 与实际 `/oneOf/N` `SourceUseSite`（`$ref` arm 二者刻意不同），供诊断与 collision 使用。仅 TaggedUnion classifier 可消费精确的 `unevaluatedProperties:false`；普通 object、nullable `oneOf` 和其他值一律失败。UEP 的 closed 证明不把 branch `additionalProperties:true`、schema-valued AP 或 `patternProperties` 变成合法分支，也不改写被 `$ref` object 的独立 Schema 语义。引入 TaggedUnion 同步修正通用 Object unknown-field policy：3个 source `additionalProperties:true` 的 Event/Command/Query Envelope payload 生成 struct 不再带错误 `deny_unknown_fields`，所以其 raw JSON extra field 可 serde decode；对应 Envelope root 仍由 Schema 与 serde 拒绝未知字段。普通 nullable `oneOf` 与 Envelope `allOf` payload 分析路径隔离。Rust projection 仅消费 IR，生成真正 `#[serde(tag = "…", deny_unknown_fields)]` enum，variant 不重复 discriminator；分支字段严格拒绝未知字段，参与 SCC 的 direct `Box` layout。`schema-tool/tests/tagged_union.rs` 覆盖 raw JSON 重复/missing/unknown tag、ordinary duplicate field、每个 branch及嵌套 serialize/deserialize、nested/inline/ref/one-branch、UEP/AP、nullable/non-discriminated/ref-target、Envelope binding 不变、union recursive `Option<Box>`/`Vec` 的真实 cargo test；`kernel-contracts/tests/contract_validation.rs`证明三类开放 payload 与严格 envelope root 的边界；实际 CLI 对 TypeScript tagged source 与 invalid union 分别比较四个 Rust artifact，失败前后 bytes 不变。
- **String enum 闭集 `ALL`**（已完成）：在通用 `ProjectedShape::StringEnum` 路径生成 `pub const ALL: &'static [Self]`，顺序严格为 Schema enum declaration order；variants / `ALL` / `as_str` 共用 renderer-local 有序 mapping，禁止按字典序重排，也不硬编码 `TaskStatus`/`ActionStatus`/schema id。string const 不生成 `ALL`。nullable string enum 在 lowering 已过滤 `null`，`ALL` 只含非 null 成员，字段仍是 `Option<Enum>`，`None` 序列化为显式 `null`。wire→variant 碰撞 fail closed，错误含 type/wire/variant。
- **自动合同测试**：`types.rs` 尾部 `#[cfg(test)] mod string_enum_contracts` 按 projection 为每个 string enum 生成调用；共享 `assert_string_enum_contract` 验证 ALL 长度/顺序、`as_str` 唯一、serde `to_value` 等于 wire string、`from_value` roundtrip。**不**手写类型目录。
- **domain-task 直接消费生成闭集**（已完成）：`domain-task` 已删除 `TASK_STATUS_CATALOG` / `ACTION_STATUS_CATALOG`、对应平行 exhaustiveness match 和 catalog-only 测试；NxN、terminal 与 proptest 直接遍历 `TaskStatus::ALL` / `ActionStatus::ALL`。状态合法边、证据准备和稳定边数断言仍是领域语义测试，不属于状态闭集重复。
- Serde omission 由 Schema 元数据确定性推导，不手改 generated：
  - `required=false` 且属性类型不允许 `null` → `Option<T>` + `#[serde(skip_serializing_if = "Option::is_none")]`，`None` 省略字段；
  - `required=true` 且允许 `null` → `Option<T>` 且**不** skip，`None` 仍输出显式 `null`；
  - `required=false` 且允许 `null` → 保持 `None -> null`（不 skip），避免无合同的 wire 变化；
  - nullability 从 `type`/`type` 数组/`oneOf [null,T]`/nullable string enum/`const null`/`$ref` 解析结果推导；无法可靠推导则生成失败，不猜；
  - `$ref` 只允许 `title` / `description` 注释 sibling；带 `type`、约束或其它 shape sibling 时生成失败。
- `additionalProperties: true` 且没有声明字段的对象，才生成 `serde_json::Value`。
- KCP/Event 条件 payload：discriminator property 闭集 enum 与每个 `allOf` 分支的 `if.properties.<discriminator>.const` + `then.properties.payload.$ref` 必须一一对应、无重复。
- payload `$ref` 必须解析到 manifest 中的完整 Schema。
- `type` 含多个非 null 分支、歧义 `oneOf`、schema-valued `additionalProperties`、`anyOf`、`not`、`patternProperties`、`dependentSchemas`、`prefixItems`、`contains`、`unevaluatedProperties`（仅 non-null TaggedUnion classifier 的精确 `false` 例外）等形状关键字明确失败。

### 递归 layout（SCC Box）

投影完成后建立 **direct-value dependency graph**：

| 边类型 | 规则 |
|---|---|
| `Named` | direct |
| `Nullable` / 字段 `Optional` | 仍 direct（解包后看内层） |
| `Array` | **indirect**（不建 direct 边） |

对每个声明做确定性 SCC；**同一 recursive SCC 内的 direct `Named` 边**包装为 `RustTypeExpr::Boxed`。optional 自递归钉死 `Option<Box<T>>`（禁止 `Box<Option<T>>`）；Array 递归保持 `Vec<T>` / `Option<Vec<T>>`，不因递归插入 `Vec<Box<T>>`。三节点 direct SCC（A→B→C→A）每条 direct 边均 box。非递归 sibling use-site 对同一 canonical 生成多个 Nominal declaration；仅 active backedge 复用。当前生产 41 schema 无环；除上述3行开放 Envelope payload unknown-field policy 根因修复外，其余生成语义稳定，重复生成保持 byte-stable。

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
- ApprovalRecord `target`当前仍是全部可选成员对象且exactly-one未强制；这是**legacy v1已知事实**。active Approval v2必须使用真正判别联合，不能修补generated v1或在repository里长期维持平行shape。
- RFC 8785 由 `serde_json_canonicalizer = 0.3.2` 实现。

## 明确未实现

- 业务active v2 source/manifest entries与generated `MethodVersionBinding` catalog（`task.create active=[2], legacy=[1]`；其余首批方法active=[1]）仍未实现；当前empty gate明确拒绝任何非空binding，未来独立切片必须以真实source生成完整表；
- TaskCreateRequest/Response v2 root-only；
- ChildTaskProposal/Delta/TaskCreationProvenance；
- CausationRef/EventEnvelope/ContentOrigin/Audit v2；
- ApprovalRecord/PermissionDecision/auth challenge/evidence v2；
- `agentd`、KCP bytes/frame/transport 与任何可运行服务端；
- Task 更新/list、Action、PermissionDecision 等后续业务 repository 与 KCP handler；
- Delegation authority 正向查询；`task.create` 中非 null `delegation_ref` 当前固定失败；
- `$anchor`、dynamic/recursive ref、nested 非 root `$id` 的compound document identity语义；当前restricted source profile在registry load统一fail closed。

未来 Profile Schema 必须从属于 Extension SDK Base 的通用 Schema，并保持单向依赖：Profile 可以引用通用 SDK Schema，Core 不能反向引用 Profile Schema。当前没有 Computer Use Profile Schema、生成包或 composition。

## 相关规范

- [`../../specs/IMPLEMENTATION_CONTRACTS.md`](../../specs/IMPLEMENTATION_CONTRACTS.md) §5、§6、§13
- [`../../specs/CONFORMANCE.md`](../../specs/CONFORMANCE.md) §5
- [`../../adr/0002-schema生成与兼容策略.md`](../../adr/0002-schema生成与兼容策略.md)
