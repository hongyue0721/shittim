# Schema 生成与契约类型

> 状态日期：2026-07-20。Manifest v2/walker/transaction基座已完成；schema-tool library已实现正式五值compatibility与kind组合门、精确production lifecycle ledger、component-native/source-title/root schema_version硬门、claimant式KCP V2 Envelope authority、独立EventEnvelopeV2 claimant/catalog、registry-level binding验证、TargetPlan-owned target-local authority/binding facts、authority/legacy正交catalog、typed request-version selector与显式Production/Synthetic registry profile。production `schemas/manifest.json` bindings仍为空；历史task creation 12项与Event v2八项 source/entries/generated root types已落地（manifest=61）；统一format assertion、neutral Alias resolution/audit、root transparent alias、通用validated decode与open-object无损typed round-trip已落地；中立strict RFC6901 selection/mutation library、`validate/canonicalize --pointer`与canonical Bytes/Hex/Hash模式CLI已落地；三份official task-creation fixtures及共享wrapper/production owner/独立CLI oracle harness已落地。kernel-kcp runtime cutover、repository、handler、materializer与可连接server仍未完成；SQLite migration 0003/mixed Outbox API已落地。

## 权威边界

- 字段、枚举、错误与兼容规则的事实源：`specs/` 与 `schemas/source/**/*.json`。
- 索引：`schemas/manifest.json`。
- Rust 生成物：`rust/crates/kernel-contracts/src/generated/`，禁止手改。
- 项目代码许可证：根目录 [`LICENSE`](../../LICENSE)（Apache-2.0）。
- CLI：`schema-tool`；运行时库：`kernel-contracts`。

## 当前产物

| 产物 | 路径 | 说明 |
|---|---|---|
| Schema 源 | `schemas/source/{audit,common,task,policy,event,kcp}/` | 当前61个Draft 2020-12 schema（41 retained + 20 component-native，含Event v2八项）；当前source不含Computer Use Profile Schema |
| Manifest | `schemas/manifest.json` | production为61 entries（41 retained + 20 component-native）与显式空`method_version_bindings`。**library implemented**：正式五值compatibility；component-native exact ID/source/title/version硬门；完整非空MethodVersionBinding validator；registry KCP V2 Envelope与EventEnvelopeV2 claimant唯一发现；production CLI `check`/`generate`在load后plan/render前调用`validate_production_manifest_stage`。family structure authority目录为`KCP_ENVELOPE_AUTHORITY_COMMAND_METHODS`/`KCP_ENVELOPE_AUTHORITY_QUERY_METHODS`/`KCP_ENVELOPE_AUTHORITY_METHODS`；retained v1目录显式为`KCP_LEGACY_V1_*`；Event目录为`EVENT_ACTIVE_*`/`EVENT_LEGACY_V1_*`；binding catalog为`METHOD_VERSION_BINDINGS`。authority不等于bound/executable。 |
| 中立 graph | `schema-tool` `contract_model` | target-scoped `TargetContractGraph`；`ContractTypeId` = `$id` + 严格 RFC6901 JSON Pointer；`TypeUse` 携带 source use-site；integer scalar携带language-neutral `IntegerConstraints { minimum, maximum }` JSON整数事实；`TypeShape::Alias`保留root/fragment identity；graph完成后由单一language-neutral resolver建立Alias依赖链与terminal事实，纯Alias SCC无terminal结构化拒绝，合法object/union递归仍交给声明SCC/layout；字段nullability消费同一resolution事实。`AliasResolution`由schema-tool crate root正式re-export。未来TypeScript renderer必须读取同一表，不得平行递归。 |
| 身份分离 | `ContractTypeId` ≠ `RustDeclarationId` | 中立 graph 按 canonical fragment 唯一；Rust renderer采用权威全序 **`SharedRoot > SharedCanonicalFragment > NominalInstantiation`**。whole-schema `$ref`固定为`SharedRoot`；任意named canonical fragment一旦被两个或更多source roots**实际投影使用**，`SharedCanonicalFragment`就在该canonical的所有use-site全局生效，会覆盖其中某个root内原本可生成的same-root nominal clones，并生成一个host-derived Rust declaration（当前四个task-create root的`$defs/proposer`共用`NormalizedRootTaskCreatePayloadV2Proposer`）；仅一个source root使用时才按use-site lineage保留`NominalInstantiation`，如`PolicyRuleCreatedBy`/`PolicyRuleUpdatedBy`。这是renderer语言策略，不改变neutral IR identity。新增第二root引用可能把既有公共字段类型/符号从多个nominal类型改成一个shared类型，属于生成Rust公共API变化，必须由generated drift与人工review明确接受，不能因canonical identity未变而静默视为无变化；symbol/member/artifact collision均fail closed。 |
| 生成目标模型 | `GenerationTarget` + `TargetPlan` | `rust` / `typescript`；非空、无重复、canonical order；每 target 独立 roots/closure（外部 `$ref`、local fragment 递归、envelope payload）；未知值 serde 失败；**无** `GenerationTarget::ALL` 闭集硬编码。流水线明确为 `SchemaRegistry -> ValidatedRegistry<Production|Synthetic> -> TargetPlan/TargetSchemaSet -> target-scoped IR`；`TargetPlan`只能由显式`ProductionRegistry`或`SyntheticRegistry`创建；裸`SchemaRegistry`仅可inspection/instance validation，不能plan/lower/render。 |
| Artifact 规划与提交 | `codegen` `ArtifactPlan::try_new` + `artifact_transaction` | 唯一plan构造入口要求distinct roots恰好一个，并校验path/duplicate/component-safe/planned prefixes；拒绝0/multi-root、`generated_evil`、absolute、traversal、duplicate、outside root。generate在完整plan/render后进入当前**单root/Linux real-platform verified** transaction：持久lock file使用Rust 1.97 OS advisory file lock（不unlink，owner metadata仅诊断），锁内先recover；durable `Preparing/Prepared/RollingBack/Committed` journal协调同filesystem sibling stage/backup/rollback-discard。transaction 边界在 FD advisory lock 成功且 owner metadata 发布后开始；LockPort（open/type/try-lock/owner metadata）独立验收，FD lock 权威、owner 仅诊断、不 unlink/PID 回收。TransactionFs 使用 typed `OperationEvent`（semantic phase、operation、Before/AfterSuccess、path role、journal target phase、仅用于重复操作的 occurrence）记录 mutation/durability boundary；read/metadata inspection 不进矩阵。正式 `Committed` journal rename 是提交点，之前 existing=Old/absent=Absent，之后=New。exact matrix 已通过真实 trace 自动发现 reachable target，断言即时完整snapshot、structured disposition、recover#1精确终态与recover#2 Noop；`StoredStateInvalid` 对正式journal损坏fail closed且不改变root。control-flow/fault conformance不等同真实断电介质模型；partial I/O只有明确fake effect才可声明覆盖。 |
| Rust projection | `RustProjection` / `project_rust` | 公开 API 只保留 `project_rust`、`render_types_module_from_projection`、`render_typed_module_from_projection`、`render_catalog_module`；禁止 graph 级 convenience re-project；`plan`/`render_rust_artifacts`/`lower_and_render_rust` 只 project 一次；catalog 直接读 graph |
| 生成类型 | `generated/types.rs` | struct/enum、const 单值类型、`NullOnly`；renderer为每个struct先建立统一Rust member namespace plan，声明字段经过snake_case/keyword归一后与synthetic `additional_properties` flatten member一并审计，碰撞错误携带Schema/declaration/字段身份；open object未知字段无损round-trip。integer仅在neutral IR同时证明inclusive min/max时选择最窄准确Rust整数类型；`1..=u32::MAX`生成`u32`，无完整安全范围的既有integer继续保持`i64` public bytes。**string enum**统一生成`ALL`并自动测试；递归SCC规则保持不变。 |
| 生成目录 | `generated/catalog.rs` | 由 manifest 生成的 embedded schema 与方法/事件闭集（不经 projection）。Event目录为named `EventTypeBinding`、`EVENT_ACTIVE_BINDINGS`/`EVENT_LEGACY_V1_BINDINGS`，并由bindings单源const投影`EVENT_ACTIVE_TYPES`/`EVENT_LEGACY_V1_TYPES`；已删除模糊`EVENT_V1_TYPES` alias。 |
| Typed decode | `kernel-contracts::decode_validated` + `generated/typed.rs` | 外部JSON通用入口固定Schema validate→typed deserialize，失败使用结构化`ContractError::DecodeAfterSchema`/`DecodeStage`；retained typed Envelope继续复用`decode_after_validation`内部阶段。12 root覆盖decode→serialize→revalidate；closed root拒绝unknown，required-nullable区分missing/null，validation-only约束不会被direct serde绕过。 |
| JCS 向量 | `schemas/examples/jcs/`、`schemas/fixtures/kcp/task_create_normalized_hash.v1.json`、三份official task creation fixtures | 现有 RFC 8785 示例、legacy fixture，以及root `schemas/fixtures/kcp/task_create_normalized_hash.v2.json`、child `schemas/fixtures/task/child_task_proposal_normalized_hash.v1.json`、allocation `schemas/fixtures/task/task_creation_allocations.v1.json`均已落地；official wrappers不进manifest |
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
  --instance /path/to/wrapper.json --pointer /instance
cargo run --manifest-path rust/Cargo.toml -p schema-tool -- --repo-root "$PWD" \
  canonicalize /path/to/file.json --pointer /projection --hex
```

中立接口现已实现：`validate`与`canonicalize`都接受strict RFC6901 `--pointer <pointer>`，默认空串表示document root；非法`~`escape属于`PointerSyntax`，非法array index、缺失member/index、scalar traversal属于`PointerEvaluation`。`canonicalize`默认输出raw JCS bytes，`--hex`输出JCS UTF-8 lowercase hex，`--hash`输出SHA-256 lowercase；三种模式都无尾随newline，`--hex`与`--hash`由Clap互斥。library公开`JsonPointer`选择API、`JsonMutationOperation::{Add,Replace}`与结构化mutation错误，以及`ValidateSelectedRequest`、`CanonicalizeRequest`/`CanonicalOutputMode`。mutation的add object member要求不存在、array insert范围为`0..=len`；replace要求目标存在，root replace支持、root add拒绝，`-`始终拒绝。`resolve.rs`的raw JSON lookup复用同一select evaluator，但SchemaNode authority仍只由registry authoritative index判定。

`schema-tool`不识别Task字段、不执行URI/time normalization、不构造projection或解释tamper业务语义；这些属于已实现的`kernel-task-creation` pure library与official fixture业务harness。三份official wrapper的解析、闭集枚举与交叉不变量由`schema-tool::official_fixture`这一明确非stable、非production的test-artifact API单一拥有，CLI oracle与`kernel-task-creation` dev harness共同消费；它不是business Schema/SDK类型，不进入manifest或generated。

## 指针、URI 与 `$ref` 规则

`SchemaRegistry::load`先用唯一`schema_walk`遍历每份document并保存私有不可变的authoritative SchemaNode pointer index；restricted identity/ref audit、codegen support-profile audit和ref可达性均复用该walker callback，不维护另一套通用递归位置表。public `schema_at`与`resolve_ref`只承认index中的pointer；raw JSON pointer lookup仅crate-private。因此`$ref`到`/const`、`/default`、`/examples/0`、`/enum/0`即使值是`{"type":"string"}`也拒绝，而`/$defs/...`、`/properties/...`、`/items`等Schema-bearing位置可解析。

1. **Join**：`resolve_ref` 使用 `url::Url::join` 相对 base schema `$id`，同时支持 local（`#/pointer`）、absolute（`https://...`）与 relative（`./x.json#/pointer`）。三者解析到同一节点时 **identity 相同**。
2. **Fragment 处理顺序**（仅一次）：
   - 严格校验每个 `%` 后跟两个 hex digit（malformed / truncated 失败）；
   - `percent_encoding::percent_decode` + `decode_utf8`（非 UTF-8 失败）；
   - 再按 RFC 6901 解析 JSON Pointer。
3. **JSON Pointer 本身**：canonical encode（`~`→`~0`，`/`→`~1`）；**允许字面 `%`**（URI 解码已在上一步完成）；array index 拒绝 `01`、`-`、非十进制。
4. **identity/ref restricted profile与SchemaNode walker**：`schema_walk`是唯一Schema-bearing遍历实现，pre-order callback提供canonical pointer、`is_root`与object/boolean node。位置闭集为map values `properties/patternProperties/dependentSchemas/$defs/definitions`，single Schema `additionalProperties/unevaluatedProperties/propertyNames/items/contains/unevaluatedItems/contentSchema/not/if/then/else`，array Schema `prefixItems/allOf/anyOf/oneOf`；存在但容器/node类型错误立即失败。registry identity audit、`$ref` resolution/component gate和target closure复用该walker，只在Schema node检查`$ref`，不进入`const/default/examples/enum`实例数据。map key恰为`$ref/$id/$schema/$dynamicRef`只作为普通名称，其value Schema仍正常遍历。named anchor和动态/recursive identity语义当前都不支持；registry load统一拒绝`$schema`（仅root允许）、`$anchor`、`$dynamicAnchor`、`$dynamicRef`、`$recursiveAnchor`、`$recursiveRef`、`$vocabulary`，不能等到validate/generate阶段。Rust projection为判断cross-root shared canonical另有一个只消费neutral `TypeShape`/`TypeExpr`的target-use walker；其`TypeShape`与`TypeExpr`均为无通配分支的exhaustive match，新增variant时编译必须失败并要求同步更新，synthetic traversal regression同时覆盖当前所有有嵌套边的shape（Object、TaggedUnion、Alias、Array、Nullable及TypeExpr容器）。稳定projector不为消除这项有边界的重复而重写。
5. **nested 非 root `$id`**：registry load即拒绝，错误信息包含真实 pointer 位置；不允许compound document identity重写。
6. **root `$id` / manifest id**：必须 canonical absolute `http(s)` URI，**无 fragment**（`Url` 序列化 exact equality）。
7. **source path confinement**：manifest `source`须为exact normalized POSIX repo-relative路径并以`schemas/source/`开头；拒绝absolute、backslash、空/dot/dotdot segment与prefix trick。source root、file、任一ancestor symlink均拒绝；canonical regular file须仍位于canonical source root内。renderer只消费`LoadedSchema`保存的verified source事实。
8. **manifest namespace / component**：只接受schema_version `2`；root `id_base`固定canonical `https://schemas.shittim.local/`。retained ID继续由41项ledger保护。component-native entry一般精确匹配title-derived `https://schemas.shittim.local/<component>/<snake_case_name>/vN`及镜像source路径；KCP envelope必须由hard gate按`component=kcp`、`kind=envelope`及精确title应用固定规范stem，去掉领域前缀`Kcp`：`KcpCommandEnvelopeV2`→`command_envelope`、`KcpQueryEnvelopeV2`→`query_envelope`，对应精确ID/source为`https://schemas.shittim.local/kcp/command_envelope/v2` + `schemas/source/kcp/command_envelope.v2.json`、`https://schemas.shittim.local/kcp/query_envelope/v2` + `schemas/source/kcp/query_envelope.v2.json`，不得生成`kcp_kcp...`。这不是任意例外，而是component已编码kcp命名空间的规范stem规则。其余exact authority/segments/version/root const/compatibility、query/fragment/encoding/dot/empty/额外segment/`.json` URL/version drift均由`SchemaRegistry::load`硬门验证；正式compatibility固定五值，synthetic/test manifest也不得使用额外值。通用MethodVersionBinding validator接受合法非空synthetic registry并验证IC §13.5全部closure；production empty只由`validate_production_manifest_stage`在production CLI check/generate的load后、plan/render前断言，非空synthetic end-to-end plan/render测试走library API与显式non-production registry profile，不调用production CLI stage gate，且禁止路径/env/fixture目录名/调用栈特判。`$ref` component allow-list与target closure继续独立。
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
  - nullability 从lowered `TypeUse`与neutral Alias terminal resolution事实推导；不再维护第二套无visited的raw Schema `$ref`递归；无法可靠推导则生成失败，不猜；
  - `$ref` 只允许 `title` / `description` 注释 sibling；带 `type`、约束或其它 shape sibling 时生成失败。
- root pure-ref生成透明Rust `pub type Root = Terminal`，root Alias仍保留自己的Schema identity；fragment Alias保留canonical fragment identity但不生成平行声明，只透明使用terminal target。Alias chain统一解析到首个非Alias terminal；pure Alias cycle因无terminal而fail closed；Alias指向nullable terminal时字段nullability沿同一resolution事实推导；Alias与合法object/union递归可共存，后者继续由声明SCC/layout处理。`$ref`仅允许`title`/`description`annotation siblings，validation/shape siblings fail closed。
- `additionalProperties:true`且存在声明字段的object生成`#[serde(flatten)]`扩展map，typed round-trip保留未知字段；synthetic member先进入renderer统一member plan，与声明字段经过reserved keyword/snake conversion后的最终Rust名一起审计。声明`additional_properties`或如`additionalProperties`这类归一后同名字段均结构化失败，错误列Schema identity与字段；neutral IR不保存Rust字段名。无声明字段的自由对象仍生成`serde_json::Value`。
- **Conditional envelope IR（已完成）**：registry load对每个manifest `kind=envelope`恰好调用一次strict parser，并按schema id缓存language-neutral `EnvelopeConditionalBinding`（discriminator、root enum声明顺序、whole-root payload `ContractTypeId`、branch string const、source order）。通用实现独立位于`conditional_envelope.rs`；target closure、Event Catalog与typed `EnvelopeWireBinding`只借用该不可变缓存，不得再次读取`allOf`。branch形状要求exact `if/then`对象、单一const discriminator、exact required、pure whole-root payload ref、enum/mapping双射；0个whole-root payload ref仍是intentionally untyped。Event authority在此基础上额外硬验active exact五项与legacy exact三项的顺序、aggregate、payload ID/version；target active envelope/payload closure任一方向partial均fail closed，legacy-only合法。
- KCP/Event 条件 payload：discriminator property闭集enum与每个`allOf`分支mapping必须一一对应、无重复；typed renderer消费上述IR，不建立第二parser。
- payload `$ref` 必须解析到 manifest 中的完整 Schema。
- `type` 含多个非 null 分支、歧义 `oneOf`、schema-valued `additionalProperties`、`anyOf`、`not`、`patternProperties`、`dependentSchemas`、`prefixItems`、`contains`、`unevaluatedProperties`（仅 non-null TaggedUnion classifier 的精确 `false` 例外）等形状关键字明确失败。
- Event v2 claimant discovery是命名的专用registry分析器，不扩张通用Envelope detector：reserved ID/title/source任一占用者永远是候选；结构候选只宽松检查`component=event,kind=envelope`、`schema_version.const=2`、非空string `type` enum及任一mapping-like `allOf`分支（type const + aggregate_type/payload键）。该宽松谓词不调用strict parser、不产生mapping fact，也不要求root type、additionalProperties、required、完整branch或双射；近似伪装统一进入candidate后由metadata/root exact门与registry缓存的strict IR fail closed。普通event object/payload或缺少上述最小形状的其它envelope保持非候选。

### 递归 layout（SCC Box）

投影完成后建立 **direct-value dependency graph**：

| 边类型 | 规则 |
|---|---|
| `Named` | direct |
| `Nullable` / 字段 `Optional` | 仍 direct（解包后看内层） |
| `Array` | **indirect**（不建 direct 边） |

对每个声明做确定性 SCC；**同一 recursive SCC 内的 direct `Named` 边**包装为 `RustTypeExpr::Boxed`。optional 自递归钉死 `Option<Box<T>>`（禁止 `Box<Option<T>>`）；Array 递归保持 `Vec<T>` / `Option<Vec<T>>`，不因递归插入 `Vec<Box<T>>`。三节点 direct SCC（A→B→C→A）每条 direct 边均 box。非递归 sibling use-site 对同一 canonical 生成多个 Nominal declaration；仅 active backedge 复用。当前生产61个Schema的既有递归布局事实保持不变。

### Validation-only 关键字

`minimum/maximum`、`minLength/pattern/format`、`minItems/uniqueItems`、`allOf`、`if/then/else` 等保留在source schema。integer `minimum/maximum`另作为language-neutral inclusive bounds进入IR；renderer只有在上下界都存在且足以证明准确语言类型时才收窄，故V2 Envelope开放payload的`schema_version: 1..=4294967295`生成`u32`，`u32::MAX`可serde round-trip，`4294967296`由Schema拒绝；只有单边或无界的既有integer仍为`i64`，避免41 retained生成类型的无意public变更。`kernel-contracts::contract_validator_options`是schema-tool CLI与runtime catalog的单一Draft2020-12 compiler配置，显式`should_validate_formats(true)`并对unknown format在Schema编译时fail closed。

## Envelope 分析（唯一路径）

对每个 envelope只调用一次strict conditional mapping parser：

1. 扫描是否存在whole-schema payload `$ref`；
2. **0 个** → 合法 **untyped** `None`（response envelope成功路径：只有`status`条件，无方法级payload binding）；
3. **≥1 个** → 每个branch必须满足exact whole-root mapping形状，并与root discriminator closed enum双射，否则error；
4. parser输出唯一language-neutral `EnvelopeConditionalBinding`，并在registry load时按schema id缓存。typed `EnvelopeWireBinding`、Event catalog facts与target closure检查都借用该缓存实例，不得再分析raw `allOf`。

Command/legacy Event有payload discriminator，因此生成typed decode。Response Envelope只有`status = ok | error`，成功payload是带`schema_version`的开放对象，**intentionally untyped**：不生成`TypedKcpResponseEnvelope`。KCP v2 family Envelope没有conditional payload mapping，由MethodVersionBinding另行选择业务payload。Typed envelope字段复用与`types.rs` **同一套** projection/layout。

## Const、null 与 JCS

- const 生成单值 enum/newtype；JSON `null` 使用 `NullOnly`。
- optional/non-null 与 required-nullable 的 wire 语义不同：前者 `None` 省略，后者 `None` 必须是显式 `null`。
- ApprovalRecord `target`当前仍是全部可选成员对象且exactly-one未强制；这是**legacy v1已知事实**。active Approval v2必须使用真正判别联合，不能修补generated v1或在repository里长期维持平行shape。
- RFC 8785 由 `serde_json_canonicalizer = 0.3.2` 实现。

## Task creation fixture与生产owner合同（official fixtures/harness已完成；repository/handler/materializer/cutover未完成）

三份official测试制品路径、外层字段名与tamper结构以IC §5.3.1/§6.10.6为唯一事实源。JCS对象只保存`jcs_utf8_hex`与`sha256`，不重复canonical string；allocation fixture不保存JCS/hash。wrapper是测试制品合同，不是Draft 2020-12 business Schema，不进入manifest、不生成类型。`schema-tool::official_fixture`是唯一wrapper parser/invariant owner，常规公开仅为避免workspace依赖环并让本crate integration test与`kernel-task-creation` dev harness消费同一实现；该API明确非stable test artifact，不承诺production或SDK兼容性。

root official fixture harness只执行`KcpCommandEnvelopeV2` raw Schema、`TaskCreateRequestV2` payload Schema与`kernel-task-creation` normalization/projection/hash，不执行Value preflight或dispatcher。因而auth非null在该harness固定为`raw_schema_rejected`、`public_error={code:"invalid_request",details:null}`、两hash `not_computed`；`unsupported_auth_schema`只属于独立preflight层，禁止跨层借用。allocation wrapper固定Schema-first：Schema invalid不typed decode、不调用domain validator；空opaque固定`schema_valid=false/domain_result=not_evaluated`，domain闭集不增加空opaque专用结果。

正式生产task creation owner已实现为纯crate `kernel-task-creation`。本阶段范围只含root/child proposal normalize、root receipt/idempotency projection与hash、child proposal/receipt hash、root/child allocation validation；`ChildTaskDeltaProjectionV1`、`MaterialAuthorizationProjectionV1`、`ObservationEvidenceProjectionV1`未来由专门authorization projection owner或独立切片实现。crate依赖`kernel-contracts`，并调用`domain-policy`当前唯一权威URI parser/normalizer；不得复制第二实现，长期抽取中立URI crate须另立ADR。全部业务事实由caller typed input注入，crate不依赖SQLite/KCP、不分配ID、不读取repository、不写存储。allocation external refs使用闭合版本化Rust snapshot类型，不是business Schema、不进manifest；字段新增必须编译期更新caller。official fixtures/harness已完成：production owner负责业务hash关系，独立CLI进程/Schema路径负责中立validate/canonicalize，stored bytes/hash自一致性共享唯一JCS authority（不是三套独立JCS算法）；repository/handler/materializer/cutover仍未完成，不能把fixture完成标成runtime composition。

## 首批12 Schema权威导航（合同、source/generated与official fixtures已实现；bindings/cutover未完成）

精确清单、ID、kind与compatibility见IC §13.6。关键边界：

- `InputTaskScopeV1`是task component独立封闭输入对象；`InputContentOriginV1`是common component独立封闭输入对象；
- `NormalizedRootTaskCreatePayloadV2`是canonical task-create proposal field contract的`$defs`宿主：自身properties使用local `#/$defs/...`，`TaskCreateRequestV2`/`ChildTaskProposalV1`/`NormalizedChildTaskProposalV1`使用absolute fragment refs；禁止第13个Schema与复制约束；所有string值出现时non-empty，包含非null `risk_hint`和`upstream_stable_id`，普通字符串不trim；
- 12项逐source直接`$ref`依赖表见IC §13.6；特别是Projection `task→common`，TaskCreate Request/Response `kcp→task/common`，两个Envelope `kcp→common`，两个allocation无refs；
- RootTaskCreateIdempotencyProjectionV1/RootTaskCreateAllocationV2/ChildTaskMaterializationAllocationV1自身分别required `schema_version=1/2/1`；
- TaskCreateResponseV2.task `$ref` retained active TaskSpec v1；
- KcpCommandEnvelopeV2/KcpQueryEnvelopeV2保持protocol 1.0，0个method payload conditional `$ref`；只禁止这两个generic typed decode及其0-ref同义wrapper；family structure authority由registry exact V2 Envelope facts唯一选择，不按ID suffix；authority常量为`KCP_ENVELOPE_AUTHORITY_COMMAND_METHODS`/`KCP_ENVELOPE_AUTHORITY_QUERY_METHODS`/`KCP_ENVELOPE_AUTHORITY_METHODS`，binding另行选择业务payload/version；
- production bindings继续为空；后续通用loader接受synthetic 8-method manifest，`validate_production_manifest_stage`由check/generate单独挂载production-empty gate，最终cutover才启用production。

## Event v2八Schema（Schema/catalog/typed 与 migration 0003/mixed Outbox API 已落地；active producer/Publisher 尚未实现）

精确清单见IC §13.6.2与ADR-0008：common三项、policy两项、event三项。production manifest 现为 61 entries。

- `ConfirmationModeV1`放common，供Policy/PD/Approval/Event共同引用；event `allowed_refs=[common,policy]`，policy仍只指common，保持无环DAG；
- `CausationRefV2.action_transition`整支引用`ActionTransitionRefV1`，不得复制字段；source最小骨架固定为union层required `kind`四值enum + closed `oneOf`，前三支inline `{kind const,id UUID}`，第四支whole-schema root `$ref`，并以`unevaluatedProperties:false`闭合；
- `EventEnvelopeV2`是active Event claimant；发现谓词独立于KCP MethodVersionBinding：reserved exact ID/title/source任一占用者或event-envelope结构候选都进入候选集，恰好一个metadata exact后再校验root enum+五个完整mapping；普通Schema不误抓；
- mapping payload必须是whole-schema root `$ref`（空fragment、解析后等于manifest root）；inline、fragment ref、missing/duplicate/mixed mapping全部fail closed；
- Envelope v2复用三个retained payload v1并引用两个新payload v1，Envelope/payload版本正交；`ApprovalStateChangedPayloadV1`新增显式`change_kind`四值enum与四branch真值表；
- generator已输出named `EventTypeBinding`、private fixed-size binding arrays与public `EVENT_ACTIVE_BINDINGS`/`EVENT_LEGACY_V1_BINDINGS` slices，并由通用`const fn project_event_types<const N: usize>`投影`EVENT_ACTIVE_TYPES`/`EVENT_LEGACY_V1_TYPES`；不保留`EVENT_V1_TYPES`；typed decode显式区分`TypedEventEnvelope`/`TypedEventEnvelopeV2`；
- production MethodVersionBindings在本Event Schema/Catalog切片继续为空；未来`event.poll` response升级为可承载v2时必须在独立切片新增/升级poll binding。

## 已实现

- 正式五值`compatibility`（无test-only旁路）；retained 4项compatibility演化标签已改标，其余37 `v1-stable`；
- component-native exact硬门与`schema_version_field`完整const校验；
- 通用合法非空MethodVersionBinding validator + synthetic 8-method tests；
- registry exact V2 Envelope family authority（删除ID suffix发现）；
- `validate_production_manifest_stage`挂production CLI；
- catalog：`KCP_ENVELOPE_AUTHORITY_METHODS`表达V2 family structure authority，`KCP_LEGACY_V1_*`表达retained validation/read目录，`METHOD_VERSION_BINDINGS`表达bound active/legacy versions；三者不等于dispatcher executable registration。
- `InputTaskScopeV1.expires_at` source Schema的Draft 2020-12 `pattern + format:date-time`双门与重新生成已完成；pattern固定大写T/Z、秒`00..59`、Z/offset及零fraction，format负责真实日期/时间/offset合法性；
- `kernel-task-creation`生产helper crate已实现root/child proposal normalize、hash与allocation validation；尚未接入TaskCreate v2 repository/handler。

## 仍未实现

- production非空MethodVersionBinding / V2ProductionWriteCutover；
- TaskCreateRequest/Response v2 root-only runtime handler/repository（包括把已实现的`kernel-task-creation` helper接入生产路径）；
- ChildTaskDelta/TaskCreationProvenance 及其余未列入本批12项的对象；
- Event v2 Schema/source/compiler/catalog/generated typed decode与SQLite migration 0003/mixed Outbox API；这些已经实现。仍未实现的是active producer/Publisher/runtime，以及ContentOrigin/Audit v2；
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
