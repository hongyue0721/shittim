# Shittim 实现进度

> 状态日期：2026-07-19（本docs-only切片接受ADR-0008并闭合Active Event v2八Schema、exact claimant/Catalog、typed envelope与SQLite migration 0003统一Outbox权威合同；**Event v2/Outbox合同开放问题=0**，剩余为实现任务；**未新增Schema、manifest entry、generated artifact、Rust或SQL**。仓库仍为53 Schema=41 retained+12 component-native，production MethodVersionBindings仍为空。既有Task creation pure library/official fixtures已实现；active repository/handler/materializer/cutover仍未实现。）

## 当前阶段

当前**代码事实**是v1 Rust/Schema基座、`domain-task`/`domain-policy`、legacy TaskCreate v1 create/get repository，以及不可连接的v1 preflight/dispatcher/三handler。manifest共有53个Schema（41 retained + 12 component-native）；首批12项的source、manifest entries与generated Rust root types已落地，但production MethodVersionBindings仍为空。Active Event v2的八Schema/claimant/generated catalog/migration 0003已经有唯一权威合同但仍未落地；当前generated event常量与SQLite Outbox都仍是legacy v1。active合同还包括TaskCreate v2 root-only、Action-only Child Task、Approval/PermissionDecision v2、Audit/ContentOrigin相关v2；repository、handler、method-aware preflight、cutover、server与SDK/client仍未完成。根Node/pnpm基座存在，但没有TS包、agentd、server、Publisher或Provider。

统一 Extension SDK Base 是基础产品的 Core 阻塞项：目前只有 `contract-only` 规范，没有正式 operation Schema、library、`composition`、public API 或 SDK 包；因此尚未达到 `schema/SDK`。`provider contract` 与 `real-platform` 是可选 Profile claim 的后续成熟度，`distribution_asserted` 则是与 maturity 正交的对外声明事实；当前两者都不存在。Computer Use 已从 Core 必做能力移为 Extension SDK Base 上的 optional Profile；当前同样仅为 `contract-only`，没有专用 Schema、crate、SDK composition、Provider 或真机测试，因而不阻塞 Core 完成。`desktop-client` 也不等同于 Computer Use。

`domain-task` 只计算状态图、不变量、revision/plan_version 和待持久化意图；`domain-policy` 只计算规则匹配、非持久 decision draft 与 canonical input。`kernel-sqlite` 拥有本批明确的 SQLite 基座和 Task create/get 事实，不伪造尚无权威表的其它跨对象一致性。

## 已完成

### 规范与工程基线

- [x] 双仓维护流程与同步 library/CLI 已实现并完成首次真实发布：主仓提交 `dcf39df07d8deedcff5813368b92382bc499f98e` 已推送，文档仓提交 `5dd8bf963e904ec8e54b3a41bef63868a724e932` 以 `文档: 同步shittim@dcf39df07d8deedcff5813368b92382bc499f98e` 记录来源；发布后 `check:docs-repository` 返回 `noop_idempotent`，两仓远端 SHA、本地 checkout、文档闭集和身份门均验证通过。后续每个切片继续执行同一闭环。
- [x] 落地零依赖主仓→纯文档镜像同步工具：`scripts/sync-docs-repository.mjs`（`--check`/`--sync`/`--self-test`，真正只读 check 临时 bare 审计、source master first-parent 权威、严格 manifest、线性普通 push、push outcome 分类、本地 checkout plumbing）与 `scripts/sync-docs-repository.test.mjs`（`node:test` + 本地 bare remotes 覆盖当前列明门禁）；根 scripts：`check:docs-repository`、`sync:docs-repository`、`test:docs-repository`。工具不提交/推送主仓，不 force，不验证 private 可见性（人工事实）。

- [x] 建立 Freedom-first、Kernel Owns Reality、Core 不可自改规范基线。
- [x] 补齐 Task/Action/Recovery、Policy、Event/Outbox、KCP 首批可编码契约。
- [x] 明确 `owner` 只是未认证预留标签；`stop.activate` 是首批 Emergency Stop 入口。
- [x] 接受工作区、Schema 生成和 KCP 本地传输 ADR。
- [x] 添加 Apache-2.0 根许可证。
- [x] 落地零依赖 Node/pnpm 根工作区基座：`packageManager pnpm@11.3.0`、`engines` exact `node 24.18.0` / `pnpm 11.3.0`、`pnpm-workspace.yaml` 声明 `ts/*`（不创建占位包）、`.npmrc engine-strict=true`、`.node-version 24.18.0`、零依赖 `pnpm-lock.yaml`，以及 `pnpm run check:toolchain`（`scripts/check-node-toolchain.mjs`）。smoke 硬校验当前 Node 进程和 PATH 中实际 `pnpm --version`；pnpm 11 的 engine warning 不冒充硬门。实际 Node 入口为 `~/.local/share/pnpm/node`；Corepack 不可用。

- [x] 接受ADR-0008并闭合Active Event v2 docs-only权威合同：下一批恰好八Schema的exact身份/direct whole-schema refs、`CausationRefV2` oneOf whole-ref骨架、三enum闭集、`ApprovalStateChangedPayloadV1.change_kind`四类真值表与repository equality边界、reserved identity+结构候选claimant、named `EventTypeBinding`与active/legacy bindings→types单源const投影、typed version gate、producer causation边界，以及SQLite migration 0003 exact version/name/single asset/transform descriptor、单表mixed API精确字段/CHECK/savepoint/rollback/backup和retained `event.poll` v1不得返回v2的边界。该条只表示规范完成，不表示Schema、generator、migration、API或producer实现。

### Schema 与 Rust 契约

- [x] 创建 Rust workspace 与 `rust-toolchain.toml`（1.97.0）。
- [x] 创建 53 个 Draft 2020-12 Schema 和 `schemas/manifest.json`（41 retained + 12 component-native）；首批12项 source/entries 与 generated Rust root types 已落地。
- [x] `schema-tool generate/check/validate/canonicalize` 已有可运行的单root transaction；artifact transaction 的 typed operation trace、独立 LockPort、精确 reference-model matrix 与 structured failure 已完成根因级实现和独立验收。当前验收边界明确为单artifact-root、Linux real-platform；control-flow/fault conformance 不等同真实断电介质模型，multi-root 与 non-Linux platform port 未实现。现有实现：`ArtifactPlan`拒绝0/multi-root；持久lock file用Rust 1.97 OS advisory file lock且不unlink；锁内recover，durable `Preparing/Prepared/RollingBack/Committed` journal协调同filesystem stage/backup/rollback-discard，所有关键syscall前后checkpoint由production闭集枚举；普通journal pre-rename I/O错误须先删除本transaction regular temp并fsync parent才在线rollback，temp清理失败明确返回`rollback deferred/recovery required`并保留journal供下次恢复；commit outcome uncertain不回滚，rollback crash-idempotent，cleanup失败可恢复。成功除lock file外无transaction residue，且保留非planned文件让check继续报告drift；未来multi-root/其它平台port未实现，同用户恶意FS替换不在conformance安全边界。
- [x] 落地 target-scoped language-neutral graph 流水线：`SchemaRegistry -> ValidatedRegistry<Production|Synthetic> -> TargetPlan/TargetSchemaSet -> target-scoped IR (TargetContractGraph) -> RustProjection(single project_rust + use-site lineage + SCC Box layout) -> ArtifactPlan::try_new`；旧版实现的`manifest.id_base` URL namespace校验已由后续manifest v2 component/retained-ID gate替代；`url`+percent-encoding 解析 local/absolute/relative `$ref`；`ContractTypeId` ≠ `RustDeclarationId`；公开 Rust projection 仅 `project_rust` + `render_*_from_projection` + catalog；typed/types 共用同一 projection 实例；envelope 唯一分析（0 payload ref => untyped；≥1 双射，mixed branch fail）；`GeneratedArtifact`/`ArtifactPlan` 字段 private + 只读 getters，`TargetPlan`/`TargetSchemaSet` facts字段private + 只读 getters，path component-safe（`try_new` 唯一 plan 构造）；生产41个Schema无环，生成两次稳定；TS renderer 仍未实现（声明即整体 fail，无部分写）。
- [x] 落地 Schema TaggedUnion：`oneOf` 单一 Nullable/TaggedUnion/Unsupported 分类先于 object lowering；中立 `TypeShape::TaggedUnion` 保存 discriminator、canonical branch identity、`/oneOf/N` arm `SourceUseSite` 与 unknown-field 禁止策略，保持 use-site 与 Rust symbol 分离；inline/`$ref`、nested、one-branch、`unevaluatedProperties:false` 与 branch `additionalProperties:false`均严格验证，UEP 不会改写被引用 object 的自身策略；Rust projection 只消费 IR，生成内部 tag + `deny_unknown_fields` enum（不 flatten/optional mega-struct），并将 union variant fields 纳入 SCC `Option<Box>` / `Vec` layout。引入该 IR 时同步修正普通 Object unknown-field policy：3个 source `additionalProperties:true` 的 Envelope payload 生成 struct 移除错误 `deny_unknown_fields`，因此保留开放 payload 的真实 Schema/serde 行为；Envelope root 仍严格拒绝未知字段。新增 raw JSON duplicate/missing/unknown tag、ordinary duplicate field、每分支及嵌套 serde roundtrip/tag-once、cargo test、nullable/non-discriminated/ref-target/profile 负例、Envelope binding 不变、实际 CLI 四生成物 no-partial 及两次稳定性覆盖；测试数量不作为稳定合同，避免文档随新增用例漂移。
- [x] 从 Schema 自动生成 Rust 类型、catalog 及 Command/Query/Event typed decode。
- [x] string enum 生成 declaration-order `pub const ALL: &'static [Self]`（通用 `ProjectedShape::StringEnum` 路径；与 variants/`as_str` 共用有序 mapping；const 不生成 ALL；nullable 过滤 null）；`types.rs` 自动 `string_enum_contracts` 覆盖全部 string enum；`domain-task` 已删除手写 `TASK_STATUS_CATALOG` / `ACTION_STATUS_CATALOG` 与平行 exhaustiveness match，NxN、terminal 和 proptest 直接消费 `TaskStatus::ALL` / `ActionStatus::ALL`。
- [x] optional/non-null 字段由 Schema 元数据生成 `skip_serializing_if = "Option::is_none"`；required-nullable 仍输出显式 `null`；optional-nullable 保持 `None -> null` 不改 wire。
- [x] 完成manifest v2 root/component/retained-ID namespace migration：仅接受`schema_version=2`，root固定`https://schemas.shittim.local/`；6个显式component声明direct canonical namespace、cross-component `allowed_refs`及retained ownership，entry以`component`替代`domain`。41个既有`/v1/` source `$id`由版本控制的`schemas/fixtures/manifest/retained_ids.v1.json`逐项固定`id/component/source/source SHA-256`，`SchemaRegistry::load`逐项计算实际source bytes hash并核对，因此ledger是生产gate，协同移动retained list/entry component或单独篡改source bytes都不能绕过；retained/component-native identity class互斥。manifest `source`通过可复用`SchemaSourcePath`限制为UTF-8、POSIX、lexically normalized、`schemas/source/`下repo-relative路径，拒绝absolute/backslash/empty/dot/dotdot/prefix trick；filesystem source root/file及任一ancestor symlink均拒绝，canonical regular file必须仍在canonical source root内，renderer仅使用verified source事实。registry load在公开前使用单一`SchemaNode` walker（pointer/is_root/object-or-boolean callback）覆盖Draft 2020-12 restricted Schema-bearing map/single/array闭集，包含`unevaluatedItems`并严格验证容器/node类型；每document保存authoritative SchemaNode pointer index，public `schema_at`/`resolve_ref`拒绝`const/default/examples/enum`实例位置；identity audit、registry `$ref`/component gate、target closure和codegen support audit复用walker，raw lookup仅crate-private。nested非root `$id`/`$schema`、`$anchor`、`$dynamicAnchor`、`$dynamicRef`、`$recursiveAnchor`、`$recursiveRef`、`$vocabulary`立即fail closed；拒绝v1 alias、prefix/default-port/dot/double-slash/encoded component伪装、retained重绑/协同搬家/真实孤儿/重复及未声明跨component ref；component `$ref` gate与generation-target closure独立fail-closed。`method_version_bindings`执行完整通用验证，`SchemaRegistry::load`允许合法非空binding；SyntheticRegistry可据此生成绑定事实。ProductionSchemaStage/Production profile在production CLI `generate`/`check`以及进入ArtifactTransaction前要求production bindings精确为空。generated catalog已有`KCP_ENVELOPE_AUTHORITY_METHODS`与`KCP_LEGACY_V1_METHODS`两套八方法表，并有空`METHOD_VERSION_BINDINGS`；未来独立切片以真实source替换empty gate。
- [x] 使用 `serde_json_canonicalizer` 实现 RFC 8785，并提供共享测试向量。
- [x] 拍板 `task.create` repository 四项阻塞：规范化后的完整 payload receipt hash、精确幂等等价 projection、TaskScope/ContentOrigin 初值、固定 `task.creation_recorded` producer 与 `task.created` 上层 ID 边界；新增独立复合 hash fixture，并由 Rust 契约测试和 schema-tool 实际 CLI 双路径共同验证。
- [x] `scripts/check-schema.sh` 为仓库当前统一门（历史名保留，**不是** Rust-only）：最前 `node scripts/check-node-toolchain.mjs`（调用者 PATH 须已指 Node 24.18.0 / pnpm 11.3.0），再重复生成、fmt、Clippy、workspace tests、生成物 Git 漂移，最后 `FILE_MANIFEST.md` 的 Git Markdown source set 校验（`scripts/update-file-manifest.mjs --check`；路径严格 UTF-8 fail closed；不含 ignored target/node_modules）。不提供跨平台 npm `check:all`；执行方式为 PATH + `./scripts/check-schema.sh`。
- [x] 定义 `AuditRecord` v1：增加 `task.creation_recorded` 不可变创建快照，显式 `external_content_status` / PayloadManifest stable refs，并拍板 PermissionDecision/policy context、rollback 权威投影、实际 Provider/模型建议引用的双源一致性；Schema 内条件已有运行时测试，不自动公开为 Event/Outbox。
- [x] 明确 Event aggregate `sequence`：首条已提交事件为 `0`，后续严格连续 `+1`，回滚事务暂分配不占号。

### Task/Action纯领域状态机

- [x] 现有v1 generated TaskStatus/ActionStatus与domain-task状态图实现保持代码事实；本轮未修改Rust。
- [x] 新增 `rust/crates/domain-task`，直接使用生成的 TaskStatus/ActionStatus，不复制状态枚举。
- [x] 实现 CORE §10 Task 状态图、revision 和 plan_version 规则；兼容 `task.create` 的 `plan_version=0`。
- [x] `succeeded` 按 `TaskSpec.success_criteria` 完整字符串**多重集合**精确覆盖，每个 occurrence 均需 `verified_ok`。
- [x] `partially_completed` 和 `rolling_back` 均要求明确副作用引用，不凭状态猜测事实。
- [x] 实现 CORE §11 Action 状态图；confirm 是 pending metadata update，不是假装 approved。
- [x] `completed`/`failed` 要求 Verification 事实；不确定结果要求 crash/timeout/ambiguous 等结构化原因。
- [x] Lease 过期与确定未派发取消返回绑定 action_id 的原子释放意图。
- [x] 补偿身份只由 `ActionRequest.parent_action_id` 推导，不存在平行 ActionRole。
- [x] `retry_original` 仅在副作用明确未发生且幂等保障成立时合法。
- [x] 新增 NxN 矩阵、证据测试与 proptest；全部状态遍历由生成的 `TaskStatus::ALL` / `ActionStatus::ALL` 驱动，不维护平行状态闭集；回归以状态边、证据规则与属性行为覆盖为合同，不维护固定测试总数。
- [x] 新增 [`api/domain-task.md`](api/domain-task.md)；本批无外部 SDK API 变化。

### Freedom-first Policy matcher

- [x] 新增 `rust/crates/domain-policy`，直接使用生成 PolicyRule/Actor/ContentOrigin/EntryPoint/SideEffectClass/decision enum。
- [x] 实现 URI 规范化、segment glob、capability/operation `.*`、exclude、side-effect ceiling；公开单项 `normalize_uri` / `normalize_uri_pattern` 复用同一 parser，供未来 Task repository 保序、保重复地逐项调用。
- [x] 实现 TaskScope resource containment 纯函数 `resource_refs_within_task_scope`：include 空=不限制、exclude 优先、全量先验证再返回布尔；stored pattern 必须已规范化；不授权、不改 Scope、不复用 PolicyRule `match_resources`。
- [x] 按 SECURITY §2.3 实现 specificity 与 priority/effect/revision/ID 稳定排序，只计算实际命中备选。
- [x] 实现 time window、Delegation/local-presence 精确布尔和 authoritative `RateLimitPort` winner-only 原子消费重选。
- [x] Stop Fence/Recovery invariant 优先返回独立 Blocked，不创建隐藏 deny；S0–S5 无规则均 Default Allow。
- [x] 生成非持久 `PermissionDecisionDraft`、RFC 8785 key params hash 与 `CanonicalEvaluationInput`，不伪造持久 revision/hash。
- [x] 补充 ContentOrigin 多值同一-origin 匹配语义及 Conformance 锚点。
- [x] matcher 内部以私有 typed `MatchOutcome`/`MatchResult` 区分 Matched / NotMatched / 真实 `PolicyError`；删除用 `InvalidRule` + magic message 表示普通未匹配的架构债；公开 API/错误码/Default Allow/排序/winner-only rate-limit 不变；回归覆盖旧 sentinel 经 `RateLimitPort` 仍 fail closed，以及 URI/action/condition/resource 普通未匹配 Default Allow。
- [x] 新增 [`api/domain-policy.md`](api/domain-policy.md)。

### Kernel SQLite 文件持久化基座

- [x] 接受 ADR-0004，使用 `rusqlite` bundled、文件 DB、WAL、foreign keys、显式 busy timeout 与 checksum migration。
- [x] 新增 `rust/crates/kernel-sqlite` 和 migration 0001；重复 open 与两个线程首次并发 open 幂等，pending migration 的 DDL/ledger 原子，漂移、未知版本与过新 schema 使用稳定 machine code 拒绝。
- [x] AuditRecord 以 RFC 8785 canonical JSON 单源不可变存储，expression index 支持 ID/type/time/task/action；插入和读取均重验正式 Schema。
- [x] 实现 `sent` 至少一个 producer/causation 支撑引用的 repository 内单记录规则；Audit 失败可与同事务其它写整体回滚。
- [x] Outbox 使用规范化列与 payload JSON；每次 append 先预检并在内部 SAVEPOINT 中原子分配 sequence/position、插入和最终 decode，调用者忽略单次错误并继续 commit 也不留下脏行或空洞。
- [x] 实现十进制 cursor、严格 `>` 分页、历史读取、未投递重复读取/重启后重投的 at-least-once 语义与第一次 `delivered_at` 不可覆盖。
- [x] `mark_delivered` 完整纳入 ADR-0004 统一写事务：Store convenience 委托 `with_write_transaction`；crate-private helper 绑定 `WriteTransaction`；保留 conditional UPDATE + 同事务 exists SELECT；覆盖 helper Err/panic rollback 后重试 Marked、unhealthy fail closed、writer contention→`sqlite_busy`、双 store 争 position 恰好一 Marked/一 AlreadyMarked 且 winner 时间保留。
- [x] 写事务对 closure panic 安全：panic 前写入回滚，释放连接 mutex guard 后恢复原 payload，后续同 store 可继续读写且锁不 poison。
- [x] 实现只能从 `WriteTransaction` 获取的生产 `RateLimitPort`；preview 不消费，winner-only 在同一 `BEGIN IMMEDIATE` 中重新计数并插入。
- [x] 新增 migration 0002 与 Task create/get repository：canonical Task/TaskScope/ContentOrigin 单源、generated-column FK/index、关系 ordinal 镜像、幂等 replay/conflict、固定 Audit/Event 和严格 fail-closed 读取；不实现 list/update/KCP。
- [x] 使用真实文件验证 generated UNIQUE parent key、deferred Task↔Scope FK、fixture hash、完整 Audit/Event 公开读取、outer panic 全事实回滚与无号重试、重复分配 ID 矩阵、非法 URI/pattern 稳定错误码、幂等 canonical/hash 与 parent relation corruption、v1→v2 保留升级、多 store replay/conflict 串行，以及 parent/delegation 失败；并补齐 `mark_delivered` 事务边界/并发/fail-closed 真实文件测试。回归以持久化不变量和故障行为覆盖为合同，不维护固定测试总数。
- [x] 新增 [`api/kernel-sqlite.md`](api/kernel-sqlite.md)。

### KCP typed application handler 合同

- [x] 闭合 `system.ping` / `task.create` / `task.get` 的 typed validated input 边界；当时未包含 Value preflight，现已由后续 §5.11 合同单独闭合。
- [x] 固定成功 payload 方法级 Schema 门、最终 Response Envelope Schema 门、request_id 原样与 ok/error 互斥。
- [x] 固定可注入 `KernelClock`、UUID/opaque ID generator 和闭集 `BackendError` 高阶 Task backend；实现 `SystemKernelClock` 与使用可失败 OS 随机源的 `RandomKernelIdGenerator`，随机源失败进入 `IdGenerationError` 而非 panic；SQLite adapter 穷举 `StoreErrorCode`，复用 `with_write_transaction` + 现有 repository，不暴露 transaction/SQL。
- [x] 固定 deadline RFC 3339 UTC instant 比较与两次读取：入口先检查；create 事务不可中途取消，commit 后到期返回 `deadline_exceeded` 但事实保留并以同一 idempotency key 恢复。
- [x] 固定 **legacy v1 handler** 六个 Kernel UUID（Task/Scope/Origin/receipt/Audit/Event，版本不限定）与独立 correlation/dedup 生成，不把 Kernel-owned 标识伪装成 caller-owned 或固定派生规则；**active root `task.create` v2** 改由 `RootTaskCreateAllocationV2` 七 UUID（含 `creation_provenance_id`）分配，见 IC §5.10 / §6.10.6，不得与六 UUID 混写。
- [x] 固定 Created/Replayed 均返回当前 Task；Created 同时返回可信绑定的 committed Event ID，只有 Created 产生 durable Outbox 的 post-commit Publisher wake-up intent，通知失败不回滚、不声明 delivered。
- [x] 固定三个方法按 backend/`StoreErrorCode` 的 KCP code、safe message、details=null、retryable 映射，不匹配错误 message。
- [x] 增加完整 fake backend/clock/ID、deadline pre/post、Created/Replayed/get/notfound、每项错误与 payload/envelope Schema 的 Conformance 矩阵。
- [x] 新增 `rust/crates/kernel-kcp`，只接收 generated typed envelope；实现三个 handler、闭集 ports、稳定 response/error 与 `HandlerContractFailure`，不提供 raw JSON/frame/dispatcher/server。
- [x] 实现 `SqliteTaskBackend`，在 `with_write_transaction` 中复用现有 repository；当前 `StoreErrorCode` 无 wildcard 穷举映射。Created 的 operation Event UUID 由 repository append/verify + 外层 commit 证明，真实文件 SQLite 测试通过公开 Store API 绑定 intent、Outbox、Audit、Task、Scope 与 Origin，replay 不新增事实。
- [x] 公共 `handle_*` 固定内置 generated Schema response 门；validator fault seam 只存在于 crate 私有 unit test，不是 public API/feature/SDK。
- [x] `kernel-kcp` 的测试以行为与闭集映射为合同；不在进度文档维护固定测试总数，避免新增回归场景后数字漂移。
- [x] 新增 [`api/kernel-kcp.md`](api/kernel-kcp.md)。

### KCP Value preflight 与注册式 dispatcher 实现

- [x] 在 `kernel-contracts` 增加 `ContractFailureStage`、`ContractFailureClassification`、`ClassifiedContractFailure`、`ContractError::stage()` 与 `classification_for_preflight()`；caller Schema violation 与 post-Schema/generated/catalog failure 结构化区分。
- [x] schema-tool生成`decode_after_validation`相关代码；`kernel-contracts::decode_validated`提供通用Schema-first typed decode，CLI/runtime共享启用format assertion的同一validator配置；Alias统一resolution/audit与root transparent alias已落地。以上仍不表示active runtime cutover完成。
- [x] 在 `kernel-kcp` 实现 `preflight_value(Value)`，按 request_id > family > protocol > auth > generated family method > 根 payload version > 完整 Schema/typed decode 固定优先级短路。
- [x] 固定五类 wire error 的 code/message/details/retryable，并复用 crate-private generated Response Schema finalizer；final response fault seam 本地 fail closed。
- [x] 实现 private-state `TypedCatalogRequest` / `RegisteredRequest`，避免调用方构造 family/discriminator/payload 错配；公开只读 family/method introspection。
- [x] `narrow_to_registered` 对 generated payload enum 穷举，无 wildcard：三 registered + 五不可序列化 Known enum。
- [x] 实现 borrowing `TypedDispatcher<C,G,B>`，直接调用三个 public `handle_*`，不增加平行 ports、不重复 deadline/Schema、不改写 `HandlerResult` 或 intent。
- [x] 增加 static negative Serialize assertions、八方法合法 Value、priority/field/cross-family/root/nested version、known malformed/valid、固定 error response、private unknown-schema/final-response fault seam、dispatcher response/ContractFailure/Created intent 与 clock 路由测试。
- [x] `kernel-contracts`、`schema-tool`、`kernel-kcp`均有对应自动化回归；测试数量随新增场景演进，不在进度文档中维护易漂移总数。
- [x] 当前 retained v1 Value preflight/registration/dispatcher 与三个 handler 已实现；未新增 bytes/UTF-8/JSON parse/frame/transport/server/agentd、五方法 handler或 `process_value`。active method-aware payload version preflight与runtime cutover仍未完成。

## ADR-0006首批实现与contract-only的ADR-0007/0008

- [x] 接受ADR-0006：active KCP TaskCreate v2 root-only；v1 legacy冻结；Child Task唯一通过父Task的`kernel.task/task.child.create` S1 Action创建；child事实直接Action causation，Action自身状态事件使用transition anchor；显式Scope/Delegation delta；原子materialization与legacy provenance。
- [x] 接受ADR-0007：Approval v2 request/resolution/invalidation判别联合、subject exactly-one、不可变current-head CAS、material/observation fingerprint分离、真实身份/remote challenge证据与plan re-evaluation。
- [x] 接受ADR-0006/0007并补齐完整contract；本轮进一步闭合首批正好12个Schema的component-native exact validator目标、五值compatibility一般规则、`NormalizedRootTaskCreatePayloadV2#/$defs`中立宿主、逐source `$ref`依赖、allocation/projection schema_version、Envelope V2 registry发现、active Catalog命名与MethodVersionBinding production-stage gate。
- [x] **首批12 business-v2 Schema source/manifest/generated types（本切片）**：12 component-native entries；shared `$defs` host + absolute fragment refs；V2 Envelope family authority生效后 `KCP_ENVELOPE_AUTHORITY_METHODS`=8，legacy仍8，`METHOD_VERSION_BINDINGS=[]`；该authority不代表bound active version或executable registration。
- [x] **schema-tool library implemented（本切片）**：正式五值`compatibility`；component-native exact ID/source/title/version硬门；完整非空`MethodVersionBinding` validator；registry exact V2 Envelope authority发现；显式`ProductionRegistry`/`SyntheticRegistry` profile proof成为TargetPlan、target graph与artifact planning的唯一入口，裸`SchemaRegistry`仅inspection/instance validation；`validate_production_manifest_stage`挂到production CLI check/generate；catalog生成`KCP_ENVELOPE_AUTHORITY_METHODS`/`KCP_LEGACY_V1_*`/`METHOD_VERSION_BINDINGS`；production bindings仍为空；neutral Alias resolution/root transparent alias、root API re-export、shared format assertion/unknown fail-closed、通用validated decode taxonomy、open-object统一member collision audit与`1..=u32::MAX -> u32`准确生成均已完成。
- [x] **task creation fixture前置合同闭合（文档切片）**：固定root/child/allocation三路径、wrapper字段与strict RFC6901 tamper结构；JCS只存`jcs_utf8_hex`/`sha256`，allocation不存JCS/hash；拍板`expires_at` Draft 2020-12 `pattern + format`双门、三类URI normalization统一`invalid_scope_pattern` details、post-normalize internal failure、`kernel-task-creation`冻结职责与URI唯一owner、allocation typed external-ref snapshots/opaque validation。该合同切片完成当时尚不表示official fixture或repository已经完成；后续pure library、通用pointer CLI及official fixtures/harness均已按下列独立条目实现。
- [x] **task creation纯library实现**：`InputTaskScopeV1.expires_at` source已加pattern+format双门并重新生成；`kernel-contracts::canonicalize_rfc3339_seconds`严格不trim、拒绝非零亚秒、offset转UTC秒；新增纯crate `kernel-task-creation`，root/child共用同一JSON字段normalize算法，生产root receipt/idempotency与child proposal/receipt JCS/SHA-256，并以闭合typed external UUID snapshot验证root/child allocation。未接DB/KCP，不分配ID。
- [x] **task creation official fixtures与harness**：三份固定路径wrapper已固化且不进manifest；`kernel-task-creation/tests/official_task_creation_fixtures.rs`按Envelope→payload→production owner与allocation Schema-first顺序执行完整tamper矩阵，load后统一校验wrapper交叉不变量，pointer字段为strict `JsonPointer`；独立`schema-tool` CLI进程oracle覆盖baseline validate/canonicalize与allocation全部tamper的真实`validate --pointer`。分工不是三套独立JCS算法：production owner负责业务hash关系，独立CLI进程/Schema路径负责中立选择验证与JCS模式输出，stored bytes/hash自一致性共享唯一JCS authority。其中 root/child 的 43/34 与 allocation root/child 各 7 是**官方 fixture 合同闭集计数**（由 fixture 文件 + harness 常量 + parser 共同锁定；变更必须版本化更新 fixture 与常量），不是 crate/workspace 测试总数政策，也不随普通回归用例增减漂移。
- [x] `schema-tool`中立JSON Pointer与selected validate/canonicalize：公开strict RFC6901 `JsonPointer` selection，syntax/evaluation错误分层，Serde反序列化强制经过strict parser，array index拒绝leading zero与`-`，literal `%`不做URI解码；所有生产JSON lookup统一使用`JsonPointer + select_json_value`，动态property name从decoded segments编码；selection/mutation共用单一decoded traversal规则，mutation错误携带精确`token_index`且失败保持atomic。`resolve.rs`在SchemaNode authority命中后若raw lookup失败，返回保留pointer source的结构化internal invariant错误。library-first `ValidateSelectedRequest`与`CanonicalizeRequest`/`CanonicalOutputMode::{Bytes,Hex,Hash}`已实现；CLI `validate/canonicalize --pointer`、`canonicalize --hex|--hash`互斥、所有canonical输出无newline，nested validate成功信息含pointer且root旧格式保持。工具不做Task normalize。
- [x] production retained lifecycle改标：TaskCreateRequest/KcpCommandEnvelope/KcpQueryEnvelope v1=`legacy-validation-only`，TaskCreateResponse v1=`legacy-read-only`，其余37=`v1-stable`；不改retained ledger/source bytes。

## 未完成

- [ ] 实现ADR-0008八Schema source/manifest/generated types与exact Event claimant：`ApprovalStateChangedPayloadV1`含显式`change_kind`四值enum/完整真值表，claimant使用reserved identity+结构候选fail-closed谓词；届时manifest从53变61，production bindings仍须为空。
- [ ] 实现generated `EventTypeBinding`、`EVENT_ACTIVE_BINDINGS`/`EVENT_LEGACY_V1_BINDINGS`及从bindings单源const投影的`EVENT_ACTIVE_TYPES`/`EVENT_LEGACY_V1_TYPES`，并实现v1/v2 typed EventEnvelope；删除当前模糊`EVENT_V1_TYPES`，不保留alias或平行mapping表。
- [ ] 实现SQLite migration 0003统一Outbox：exact version=3/name=`versioned_event_outbox`/single asset=`rust/crates/kernel-sqlite/migrations/0003_versioned_event_outbox.sql`，先同事务升级ledger descriptor列，再以format-v1 JCS descriptor单hash覆盖该asset与exact Rust transform三元组；单表mixed read/write、canonical `causation_json`、post-0003 CHECK、corruption/backup/rollback测试；当前仍是v1列与API。
- [ ] 实现固定mixed API：`StoredEventEnvelope::{LegacyV1,ActiveV2}`、`OutboxRecord`、逐字段legacy pending、`EventAggregateId`与无caller type/aggregate的`PendingActiveEventV2`、`append_legacy_event_v1/append_active_event_v2`；旧`PendingEvent/append_event`无alias。随后实现五类producer；当前没有Action/Approval producer、Publisher或consumer runtime。retained `event.poll` response v1仍只能返回v1，mixed KCP response须后续独立升级。
- [ ] 后续独立Action-transition Schema/authority批次实现`ActionTransitionIntentV1`（task component exact ID/source见IC §6.14）及Action持久对象/repository/migration；本八Schema批次只交付ref/wire，producer不得临时造类型。

- [ ] active Action合同需升级：`approval_chain_id`、execution generation、materialized child result与Approval v2消费；现有`approval_record_ref`/deferred文案是legacy实现，不得声称满足v2。
- [ ] 后续其它v2 Schema仍包括四投影/SubjectProjection/CreationProvenance、ContentOrigin/Audit、ActionTransitionIntent与Action持久对象、ApprovalRecord/PermissionDecision/PolicyRule、signature/credential/challenge/evidence等。CausationRef/EventEnvelope与Action/Approval event payload已属于前述Event八Schema，不在此重复列项。
- [ ] 将Value preflight改为method-aware payload version；active `task.create`只接受2，v1仅migration validator；替换registered v1 handler。
- [ ] 实现root v2 repository/handler与child Action materializer；同Action最多一child、bundle全有或全无、canonical readback与reconciliation。
- [ ] 实现Action/PermissionDecision/Approval v2 repositories、current-head CAS、fingerprint失效/复用、身份challenge验证与plan Action重评。
- [ ] 实现legacy direct-child provenance migration，不伪造历史Action/Approval/Verification。

- [ ] 为其它 Command 实现请求幂等与乐观锁；`task.create` scope/projection/生命周期已持久化。
- [ ] 为五个已知未注册方法实现正式 handler；完成前 `KnownCatalogMethodNotImplemented` 只作为本地注册完整性结果，server 阶段门保持关闭。
- [ ] 实现 Unix Domain Socket / Windows Named Pipe KCP server/client（受上述阶段门阻塞）。
- [ ] 实现 `agentd` 组合根和首批八个 KCP 方法处理（本批三方法合同不等于八方法可用）。
- [ ] 在已有根工作区基座上创建 `ts/*` 包、SDK client 与 Pi `agent-runtime`（当前仅有零依赖根基座，无 TS 包/生成/SDK）。
- [ ] 实现统一 Extension SDK Base：当前为 `contract-only`；没有正式 operation Schema、library、`composition`、public API 或 SDK 包，尚未达到 `schema/SDK`。这是基础产品未完成的阻塞项；Extension SDK Base 完成不要求任何 Computer Use 或其他 Profile 的 `provider contract` / `real-platform`。
- [ ] 实现 optional Computer Use Profile：当前 `contract-only`；没有专用 Schema、crate、SDK `composition`、Provider 或真机测试。它不阻塞 Core 完成，也不由 `desktop-client` 替代。
- [ ] 创建 Tauri/React/Ant Design 蓝白桌面客户端。
- [ ] 实现 Provider、Memory、Initiative 与 Broker。
- [ ] 完成 `specs/CONFORMANCE.md` 的全部 BASE 套件，以及本发行物实际声明的 Profile / Platform 对应 `IF_CLAIM` / `IF_REAL` 条件套件。

## 当前阻塞

- 当前最大阻塞已从“五方法handler”扩展为active contract migration：现有v1 `task.create`必须退出production dispatcher，v2 Schema/handler/repository与method-aware preflight完成前更不能启动server。
- legacy `task.create v1` repository已完成；其Delegation正向路径未实现，非null固定not found。active v2必须实现真实Delegation authority，不能沿用该行为。
- Task list cursor 仍保持 opaque；编码技术选择必须在 repository 实现前通过 ADR/API 拍板，不属于三方法 handler。
- AuditRecord 的 Schema 内条件、SQLite immutable/canonical Store、`sent` 单记录支撑引用检查，以及 Task create 的固定 Audit/Event canonical 一致性已完成。仍缺 PermissionDecision/policy context 跨对象字段相等、rollback 权威投影、Provider/ModelCall 跨对象一致性与其它业务 producer；不得用默认值或单记录校验冒充这些跨对象事实。
- `system_internal` null actor 的“确无可归因注册主体”仍由上层 producer 证明。
- Node 24.18.0 / pnpm 11.3.0 根基座已落地；默认 PATH 仍可能是 Node 26.x，必须显式使用 `~/.local/share/pnpm` 入口。尚无 `ts/*` 包与 Schema→TS/SDK。
- 统一 Extension SDK Base 仍为 `contract-only`：没有正式 operation Schema、library、`composition`、public API 或 SDK 包，尚未达到 `schema/SDK`，因而阻塞基础产品完整性；Extension SDK Base 不要求 `provider contract` 或 `real-platform`。不得把现有 Extension 规范或根 Node 工作区冒充为 SDK 实现。
- optional Computer Use Profile 也仅为 `contract-only`：没有专用 Schema、crate、SDK `composition`、Provider 或真机测试；它不阻塞 Core，且 `desktop-client` 不等同于该 Profile。
- 真实模型 Provider、远程 Channel、跨平台 Provider 与 Privilege Broker 仍需要后续真实环境和用户选择；当前没有伪造支持。

## 下一步

1. 先实现ADR-0008的八Schema、`CausationRefV2` oneOf whole-ref骨架、reserved identity+结构候选exact claimant、`change_kind` Approval真值表、`EventTypeBinding` active/legacy bindings单源投影/typed decode；保持production MethodVersionBindings为空。
2. 再实现SQLite migration 0003 exact descriptor identity（version/name/single asset/transform三元组）与单表mixed v1/v2 Outbox，以及固定命名/精确字段的active v2/legacy v1 API，不接业务producer；retained KCP poll仍不得返回v2。
3. 独立实现`ActionTransitionIntentV1` Schema + Action持久对象/transition repository/migration，禁止producer临时类型；再做method-aware preflight、root v2 repository/handler 与 child Action原子materializer，以及最终 V2ProductionWriteCutover。
4. 再补ADR-0006/0007其余v2 Schema与generated artifacts，满足cutover前closure。
5. 实现Approval/PermissionDecision/Action repositories及current-head CAS，因为child materialization依赖它们。
6. 完成Task creation provenance migration和reconciliation，不伪造历史Action/Approval/Verification。
7. 再实现剩余五个Catalog handler与可连接server；禁止先接v1 server。
8. 随后实现Publisher、Extension SDK Base与TypeScript/client。

## 最近验证

```text
export PATH="$HOME/.local/share/pnpm:$PATH"
export TMPDIR=/mnt/data/shittim-build-tmp
export CARGO_TARGET_DIR=/mnt/data/shittim-cargo-target
mkdir -p "$TMPDIR" "$CARGO_TARGET_DIR"
# 文档 sync 测试另用专用根（仍在 /mnt/data；与全仓 TMPDIR 约定分层，不互相替代）
export TMPDIR=/mnt/data/shittim-docs-sync-tests
pnpm run check:toolchain
pnpm run test:file-manifest
pnpm run write:file-manifest
pnpm run check:file-manifest
pnpm run test:docs-repository
node scripts/sync-docs-repository.mjs --self-test
# 主仓 dirty/未推送时 --check 预期 source_dirty（或 source_not_pushed）失败；不得在本切片对真实远端 --sync
# 统一门前恢复编译用 TMPDIR/CARGO_TARGET_DIR
export TMPDIR=/mnt/data/shittim-build-tmp
export CARGO_TARGET_DIR=/mnt/data/shittim-cargo-target
pnpm install --frozen-lockfile
# 统一门（先 Node 硬门，再 Rust，再 FILE_MANIFEST）；无跨平台 npm check:all
./scripts/check-schema.sh
git diff --check
```

Node/pnpm 基座：`check:toolchain` 通过。`FILE_MANIFEST.md` 由 `scripts/update-file-manifest.mjs` 从 Git source set 生成（tracked + untracked non-ignored `*.md`，路径严格 UTF-8、禁止手改、不扫 ignored build 产物）；`check-schema.sh` 最前跑 toolchain 硬门，最后跑 `--check`。`test:docs-repository` 在 `/mnt/data/shittim-docs-sync-tests` 用 bare remotes 覆盖文档列明的门禁与状态分支（含 `source_identity`/`docs_identity` name+email），不宣称穷尽所有 Git/网络故障组合。本阶段编译/测试运行命令必须显式 `TMPDIR`/`CARGO_TARGET_DIR` 到 `/mnt/data`；Rust 库自身不硬编码 host path。仓库全量以 `export PATH=...` + 显式 TMP/CARGO + `./scripts/check-schema.sh` 为准。主仓未 clean/未推送前不得宣称文档镜像已与当前工作区同步。

## 事实来源

- 全局不变量：[`../AGENT.md`](../AGENT.md)
- 状态机与恢复：[`../specs/CORE_ARCHITECTURE.md`](../specs/CORE_ARCHITECTURE.md)
- 实现契约：[`../specs/IMPLEMENTATION_CONTRACTS.md`](../specs/IMPLEMENTATION_CONTRACTS.md)
- 验收：[`../specs/CONFORMANCE.md`](../specs/CONFORMANCE.md)
- Schema：[`api/schema-generation.md`](api/schema-generation.md)
- 状态机 API：[`api/domain-task.md`](api/domain-task.md)
- Policy matcher API：[`api/domain-policy.md`](api/domain-policy.md)
- Value preflight/dispatcher 合同：[`api/kcp-preflight-dispatcher.md`](api/kcp-preflight-dispatcher.md)
- Typed handler API：[`api/kernel-kcp.md`](api/kernel-kcp.md)
- Task repository 契约：[`api/task-repository-contract.md`](api/task-repository-contract.md)
- SQLite API：[`api/kernel-sqlite.md`](api/kernel-sqlite.md)
