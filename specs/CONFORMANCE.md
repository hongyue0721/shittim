# CONFORMANCE.md

> 本文件定义核心、SDK、模型、可选 Extension Profile（含 Computer Use）、平台、安全和恢复的一致性测试与发布门槛。
>
> **套件分层原则**：
>
> 1. **Core / Extension SDK Base 无条件套件**：凡声称实现 Shittim Core 或 Extension SDK Base 的产物，必须通过；**不**依赖 Computer Use、跨平台桌面 Provider 或 Hyprland / niri 真机。
> 2. **Optional Profile 条件性套件**：统一读取 `ProfileClaim` 集合；存在对应精确 `claim_id` 时相关套件成为硬门，`distribution_asserted=true` 时还成为该发行物对外声明的发布门。未记录该 claim 不阻塞 Core 发布，但运行时必须按第 1.2 节返回唯一语义错误。
> 3. **声明即验收**：Manifest 文本 alone 不等于支持；一旦 `distribution_asserted=true` 或对外宣称支持，对应 claim 依赖闭包和全套测试硬性适用（见 `COMPUTER_USE.md` §3）。

## 1. 测试原则

测试重点是不变量和故障，不只是成功路径。

层级：

- Unit；
- Property；
- Contract；
- Integration；
- Platform；
- Security；
- Recovery；
- Resource/Performance。

### 1.1 条件性门控词汇与统一 Claim 集合

所有条件门只读取同一去重后的语言中立 `ProfileClaim` 集合 `C`；概念字段为 `{claim_id, maturity, distribution_asserted, evidence_refs}`，本阶段不创建 Schema。`maturity` 只允许 `contract-only | schema/SDK | composition | provider contract | real-platform`；distribution assertion 是正交布尔值，不是 maturity。

| 标记 | 含义 |
|---|---|
| **BASE** | Core 或 Extension SDK Base 无条件硬门 |
| **IF_CLAIM(expr)** | 对 `C` 中稳定 claim id 的布尔表达式门控。原子在对应精确 id 存在时为真；组合只允许 `ANY(a,b,...)` 与 `ALL(a,b,...)`。禁止 `computer.*`、`a|b`、省略 namespace 或自然语言替代 claim id |
| **IF_REAL(platform claim id)** | 仅当 `C` 中该精确 platform claim 的 `maturity = real-platform` 时为真；`provider contract` mock 不满足 |
| **NEVER_CORE_GATE** | 明确不得作为 Core 发布阻塞项 |

读取规则：

- 任一 `computer.*` claim 都先隐含 §11.0；各专节再按自己的 `IF_CLAIM` 增量适用；
- `distribution_asserted=true` 不改变 maturity，但要求该 claim、其机械依赖闭包和本文件映射的套件全部通过；
- 同一 `claim_id` 多条、证据不足、散文声明与集合冲突均使发布报告失败；不得从 family 名、包名、单独 Manifest 文本或“完整支持”自然语言补造 claim；
- “完整 Computer Use”的必需 claim 集合与依赖闭包只由 `COMPUTER_USE.md` §3.3 定义，Conformance runner 必须逐成员闭包，不能接受一个含糊 `ALL` 标签代替记录集合。

### 1.2 未声明时的行为验收

对任何 Optional Profile（含 Computer Use），错误按事实唯一选择：

- 当前安装、发行物或协商集合不提供 → `unsupported`；
- 已支持但 disabled、quarantined、health/backend/session 等当前暂不可用 → `unavailable`；
- SDK/Profile/operation Schema 版本无兼容交集 → `incompatible`；
- 能力已 discovery-visible 且版本兼容，但当前调用未获 grant → `capability_not_granted`；
- 禁止同一事实“任选其一”；合同缺失 **不是** Policy Default Allow；
- discovery-visible 与 invocable 必须分测：只读 observation 不需要副作用 Action grant，但仍需正式兼容 Schema、身份/scope 与数据策略；
- 不得把可选套件失败解读为“Core 不变量失败”，也不得把未跑可选套件解读为“已支持该 Profile”。

## 2. 核心不变量（BASE）

必须自动测试：

- 模型不能写权限；
- Provider 不能写 Task 状态；
- Extension 不能直接互调；
- 用户取消优先于 Agent 输入（输入路径存在时；无 Computer Use 时至少在 Action 取消边界验证）；
- 任务成功必须有验证；
- 无匹配 PolicyRule 的 Action 得到 `allow`；
- 匹配规则按 priority、确定性 specificity tuple、deny/confirm/allow effect、newest revision、rule ID UTF-8 字节序升序稳定决定；ID tie-breaker 不改变前述语义；
- 推荐确认模板未被显式启用时不改变默认 allow；
- Secret、敏感内容及认证数据可按 Task/Policy 流转至 Memory、模型或 Extension，且审计只记录配置要求的最小元数据；
- Child Task不能超出其**显式声明**的作用域和能力事实而不经过Policy；父子扩大delta本身不是hard deny；
- 扩展更新保留可解释的 Policy 决策、版本和回滚点；
- AI 自写扩展不能访问 Core 写权限；
- UI 断开不丢 Task；
- Agent Runtime 重启不重复不可逆 Action；
- Stop Fence 在模型、Provider、Extension、远程入口和租约边界均生效；
- 外部内容不能伪造 Kernel Command、Kernel Event、Policy mutation evidence 或 Broker 请求；
- **能力未协商 / Profile 未启用不得落入 Policy Default Allow**：当前集合不提供为 `unsupported`；已支持但当前暂不可用为 `unavailable`；版本无兼容交集为 `incompatible`；已发现但当前调用未获 grant 为 `capability_not_granted`。

## 3. Task Engine（BASE）

场景：

- 正常状态转换；
- 非法转换拒绝；
- Planner 重新规划版本；
- 并发资源冲突；
- Action 超时；
- cooperative cancel；
- partial side effect；
- rollback success/failure；
- Task 从 `running` / `partially_completed` / `failed` / `cancelled` 在存在需补偿的已发生外部副作用时进入 `rolling_back`，且该流程不能伪装为 SQLite rollback；
- Policy `confirm`使Action保持`pending`并关联Approval v2 request chain；approved resolution后进入`approved`，denied后`cancelled`，invalidation触发重评；
- `leased -> approved` 仅由 `lease_expired` 触发，且 Lease 失效、revision 更新、全部资源锁释放在同一 SQLite 事务中；取消只有在确定未派发时才进 `cancelled`，派发事实不确定时进 `unknown_side_effect`；
- 补偿 Action 按普通执行链进入 `completed` / `failed` / `unknown_side_effect`，原始 Action 才依据补偿结果进入 `rolled_back` / `rollback_failed`；
- crash before/after external action；
- idempotent retry；
- unknown external outcome。

## 4. Policy/Delegation（BASE）

测试：

- Policy URI pattern 规范化与 segment glob：Resource、Actor source、ContentOrigin source 均使用 URI；`*` 单段、`**` 多段，非法内嵌 glob 和 regex 被拒绝；
- active `task.create` v2只创建root；payload出现`parent_task_id`被拒绝；v1 request只可legacy Schema/fixture validation，active preflight按method-aware version返回`unsupported_schema_version`且不能进入dispatcher；
- 新child唯一通过`kernel.task/task.child.create` Action创建，父ID只取Action.task_id，proposal禁止child id/parent/status/revision/time；
- child Scope与Delegation完整显式，不继承/求交；父子scope/capability/delegation delta进入Policy context；扩大按Freedom-first裁决，Delegation authority缺失明确失败；
- child materialization故障矩阵证明Origin/Scope/Task/provenance/Audit/Event/Verification/Action completion全有或全无，同Action跨generation/idempotency最多一个child，canonical readback失败整体回滚；
- commit后丢响应重放返回原child；mapping/bundle不完整为stored_data_invalid/Safe Recovery，不局部补写；
- 不提供legacy direct-child migration/provenance；旧开发库open/start拒绝并返回稳定`reinitialize-required`，禁止自动清库/隐式升级/读后补写；
- operation/capability 只接受精确或末尾 `.*`，空数组不限制，exclude 优先，`side_effect_max` 按 S0..S5 ceiling；
- ContentOrigin 多值匹配要求同一条 origin 同时满足受限的 kind/source 维度，不得跨两条 origin 拼接命中；空数组/缺省维度不限制；
- specificity tuple 的每个字段按本次实际命中的最具体备选 pattern 计分；数组重排或增加未命中备选不改变结果，通配越少越具体，最终同分按 rule ID UTF-8 字节序升序；
- Policy confirmation词表全映射：PolicyRule v2五种mode逐一映射PD decision与Approval request canonical wire；`remote_signature→require_remote_signature`有正式Policy入口，通用mode中不存在算法名；PolicyRule v1仅按自身lifecycle保留Schema/fixture历史验证资产，无production store read API；
- authentication_level 按 `unauthenticated < asserted < platform_verified < system_authenticated` 比较，任一等级均不自动授权；
- time_window 使用 IANA timezone、weekday 和本地半开区间，覆盖同日、跨午夜、全天及 DST；
- rate_limit 的 count/window_seconds/key_scope 原子检查与消费；delegation/local presence 布尔条件精确匹配；
- 未知或不支持 condition 返回 `unsupported_policy_condition` 并 fail closed，不能当作无规则匹配而 Default Allow；
- 普通 URI/action/condition/resource 未匹配是 typed NotMatched，不是错误；不得用 magic message / `invalid_policy_rule` 哨兵表示未匹配；真实评估错误（含外部 RateLimitPort 返回的任意 `PolicyError`，即使 message 撞上历史 sentinel 文本）仍 fail closed；
- enabled 规则语义非法 fail closed；disabled 规则在语义校验前忽略；
- S0-S5 只作为风险、匹配、审计与恢复标签，不隐式产生 allow/confirm/deny；
- 无匹配规则时每个 Side-effect Class 均为 allow；
- ContentOrigin 完整 Schema、kind 闭集、Actor kind 保留 `owner` 预留值但它不产生认证或默认权限、外部伪造 Kernel receipt 被拒绝且 Kernel 为已接受内容创建 receipt、parent origin 链和缺失 upstream ID 使用 null；
- EntryPoint 闭集在 Envelope 使用，Actor 只保留 source，Actor 内重复 entry_point 或 Envelope 使用旧 `entry` 字段被拒绝；ContentOrigin 自身的 entry_point 仍作为内容来源字段保留；
- allow、confirm、deny 规则的结构化解析与版本化；
- priority 高于 specificity；
- 同 priority/specificity 时 deny 高于 confirm、confirm 高于 allow；
- 其余条件相同时 newest revision 获胜；
- 推荐确认模板默认未启用；
- scope 包含/越界；
- TaskScope resource containment 纯函数：include 空=不限制、exclude 优先、每个 resource 均须满足、resources 空在完整验证 patterns 后为 true；stored pattern 必须已规范化否则 `InvalidScopePattern`；非法 concrete URI 为 `InvalidResourceUri`；先完整验证全部输入，前面越界不得掩盖后面非法 URI；`Ok(true/false)` 只表示边界包含，不授权、不改 Scope；顺序/重复不影响结果且不修改数组；`*`/`**`/query/fragment 复用 Policy URI 语义；
- Exploration Scope 与 Task Scope 分离：探索发现不能扩张任务写入、外发或特权作用域；
- 入口信任差异；
- local confirmation；
- lease 过期/次数；
- delegation trigger；
- budget 超限；
- action whitelist；
- plan change requiring confirmation；
- revocation during task；
- 用户自然语言创建、修改、撤销 Policy Rule、Exploration Scope、Delegation 与 Trigger 后形成版本化结构化 mutation；
- Bot 的 Policy mutation Action 在第一版没有额外限制规则时遵循 Default Allow；命中 actor/entry_point/ContentOrigin/object-type 规则时按 `confirm` 或 `deny` 处理；
- mutation 审计包含 actor、entry_point、auth evidence（如有）、ContentOrigin 与 policy mutation authority；第一版不要求 Owner 或本机唯一身份，`policy_mutation_authority` 是后续认证与细粒度规则的预留上下文；
- ambiguous natural language never directly authorizes，必须先成为可解释的结构化候选。

## 5. Kernel Control Protocol、Schema 与事件（BASE）

必须自动测试：

- KCP首批方法名仍为`system.ping`、`task.create`、`task.get`、`task.list`、`event.subscribe`、`event.poll`、`stop.activate`、`stop.status`；未知方法返回`unsupported_method`；
- MethodVersionBinding的完整8方法覆盖必须从KcpCommandEnvelopeV2/KcpQueryEnvelopeV2的family method enum事实派生；测试可构造synthetic manifest，但不得在fixture/代码手写另一份expected production method表；
- `task.create`active v2 root-only：Envelope task_id/expected_revision null，payload无parent；method-aware preflight拒绝v1 production write；root allocation为七UUID，legacy v1 allocation才是六UUID；official root fixture只执行`KcpCommandEnvelopeV2` raw Schema、`TaskCreateRequestV2` payload Schema与`kernel-task-creation` normalization/projection/hash，不执行preflight；因此auth非null固定为`raw_schema_rejected` + `public_error.code=invalid_request`/`details=null` + 两hash `not_computed`，不得借用`unsupported_auth_schema`；独立preflight仍验证`unsupported_auth_schema`；
- 本批Schema清单必须持续精确为IC §13.6的12项；title均带V1/V2。`InputTaskScopeV1`/`InputContentOriginV1`独立封闭对象正反例覆盖required/nullable/unknown field；本批所有string array覆盖empty合法、empty element非法、顺序与重复保留、无`uniqueItems`；所有普通string值出现时non-empty，特别覆盖`risk_hint=null|非空合法|空串非法`与`upstream_stable_id=null|非空合法|空串非法`，并证明普通字符串不trim；`InputTaskScopeV1.expires_at`现行source以Draft 2020-12 `pattern + format:date-time`双门实现，Conformance必须持续覆盖：pattern精确为`^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-5][0-9](?:\.0+)?(?:Z|[+-][0-9]{2}:[0-9]{2})$`，只接受大写T/Z、Z或offset、无fraction/全零fraction并拒绝非零fraction；format独立拒绝非法日期/时间/offset。测试必须证明双门各自不可删除，合法offset解析instant后规范化为UTC秒精度，禁止截断/四舍五入；
- raw/normalized root/child四root Schema共享`NormalizedRootTaskCreatePayloadV2#/$defs`这一canonical task-create proposal field contract；宿主root properties用local fragment，其余三root逐字段使用absolute fragment，测试通过mutant证明任一共享字段约束只改一处即可同时影响四root；禁止第13个Schema与四份复制约束漂移；`TaskCreateResponseV2.task`解析到retained active TaskSpec v1，且不存在虚构TaskSpec v2 entry；
- `RootTaskCreateIdempotencyProjectionV1`的required `schema_version=1`出现在JCS bytes并影响hash；两allocation分别required `schema_version=2/1`但不计入七/十UUID数量。Schema仅验证UUID/opaque字段自身；producer/conformance另证跨字段UUID互异、外部ID不相等和opaque独立；opaque只断言non-empty，不断言hex；
- KcpCommandEnvelopeV2/KcpQueryEnvelopeV2 ID、protocol 1.0、family结构与positive root payload version逐项验证；每个新Envelope的method payload conditional `$ref`计数必须为0，生成物不得出现`TypedKcpCommandEnvelopeV2`、`TypedKcpQueryEnvelopeV2`或由这两个0-ref Envelope同义产生的wrapper；不得笼统禁止其它有正式判别映射的`Typed*V2`；retained v1 Envelope bytes和条件refs不变；KcpQueryEnvelopeV2虽payload仍v1，breaking理由固定为两阶段选择架构；
- payload版本选择由MethodVersionBinding提供：`task.create active=[2], legacy_validation=[1]`；其余七个首批方法active=[1], legacy=[]；同一binding同时给出request/response schema ids，response按family+method+request version选择，不只按method猜测；
- Actor 带单调 revision；`owner` 仅预留标签；Envelope v1 不执行身份认证，只解析并记录 actor/entry_point；在实际执行Value preflight的auth判定层，`auth`只能为 null，非null返回 `unsupported_auth_schema`；official root fixture harness不执行preflight，非null raw Schema拒绝固定投影为`raw_schema_rejected` + `public_error.code=invalid_request`/`details=null`，且receipt/idempotency不计算；
- 已过期 deadline 与处理期间超时均返回 `deadline_exceeded`，不得静默丢弃；已开始且不可安全取消的外部动作先持久化为 `unknown_side_effect`/恢复待查，不伪称未生效；
- 以下v1 hash/idempotency/accepted_at/producer断言是legacy实现回归；active v2必须新增独立fixture并将Envelope task_id/expected_revision固定为null、payload无parent；
- legacy v1 receipt fixture `task_create_normalized_hash.v1.json`继续自动验证，但不得作为v2向量；v2 receipt/idempotency hash另建fixture；
- root v2与child materialization各自固定一个accepted/materialized_at并将bundle内时间一致投影；legacy v1 accepted_at测试继续保留；
- `task.create`的v1 fixed TaskScope/ContentOrigin/Audit/Event投影测试属于legacy回归；active v2按新Schema/provenance测试，child按Action carrier/delta/bundle测试；
- legacy v1 `task.creation_recorded`固定producer继续回归；active v2 Audit增加provenance，child producer增加Action/PD/Verification与Action causation；
- root v2 `task.created` command causation；child `task.created` Action causation；两者ID/correlation/dedup上层分配、sequence=0、payload精确一致；root使用七UUID的`RootTaskCreateAllocationV2`，child使用十UUID allocation；
- `NormalizedRootTaskCreatePayloadV2`字段表与`TaskCreateRequestV2`一一对应、无parent：receipt就是normalized payload的JCS；Envelope `context`只在idempotency projection出现一次；URI规范化与所有数组保序/重复、InputContentOriginV1、raw/normalized/JCS/hash/tamper独立fixture逐项验证，禁止复用v1/child向量；`origin.source_uri`、resource pattern、exclusion normalization失败统一映射`invalid_scope_pattern`，details分别精确为`origin_source_uri/null`、`resource_pattern/index`、`exclusion/index`；post-normalize Schema或typed失败只能是本地internal contract failure；
- 以下为legacy v1实现回归：非null parent task/parent origin存在性、所有非null delegation固定not found、v1 producer/hash/command causation。它们不属于active v2 Conformance；
- 以下为legacy v1实现回归：`task.create`幂等、candidate初值、单一`task.created`与Origin/Scope/Task/Audit/Outbox事务；active v2必须使用独立fixture/provenance并强制root-only；
- `task.list` 的 parent_filter 明确区分 any/root/exact，稳定排序、limit 边界和 opaque cursor；cursor 编码技术选择留待 repository 实现前的 ADR/API 拍板，本批不把任一编码写成事实；
- Value preflight 输入只接受已由调用方解析的 `serde_json::Value`；bytes、UTF-8、JSON parse、frame、最大尺寸、clock/backend/ID 不进入该层；优先加入现有 `kernel-kcp` 并复用 handler/ports/response 门，禁止复制 Catalog/response/handler abstraction；公开调用必须分成 `preflight_value -> narrow_to_registered -> TypedDispatcher.dispatch`，不得用一站式 `process_value` 暗示全 Catalog 可执行；
- preflight 严格短路优先级为 request_id response eligibility > message_kind/family > protocol > auth > family-specific method > 根 payload.schema_version > 完整 Envelope/方法 Schema + generated typed decode；测试必须构造多个同时错误的输入证明高优先级结果稳定胜出；
- 非 object，或顶层 request_id 缺失/非 string/非法 UUID，固定得到本地 `PreflightLocalRejection::UncorrelatableRequest { kind, message }` 且不产生 wire response；合法 UUID string 在所有 error response 中逐字原样保留；本地 ContractFailure 也只有固定 safe kind/message，不暴露内部 schema ID/detail；
- request_id 可关联后，message_kind 缺失/非 string/response/未知均为 `invalid_request`；family 确定后只解释对应 discriminator，另一 family discriminator 留给最终 Schema 作为未知字段；protocol 缺失/非 string为 `invalid_request`、string 非 `1.0` 为 `unsupported_protocol_version`；auth 缺失为 `invalid_request`、任意非 null为 `unsupported_auth_schema`；
- command 只按 generated command Catalog 检查 command_type，query 只按 generated query Catalog 检查 query_type；字段缺失/非 string为 `invalid_request`，不属于所选 family 为 `unsupported_method`，包括 query `task.create` 与 command `task.get` 等跨 family 错配；测试必须证明实现没有手写第二份八方法目录；
- payload missing/non-object，或根 schema_version missing/JSON number 非 i64/u64 integer（包括 `1.0` 与超出 i64/u64 可表示范围的数字）/<=0 为 `invalid_request`；根正 integer不在所选family+method的generated active request version集合中为`unsupported_schema_version`（`task.create`: active `[2]`、legacy validation `[1]`；其余首批方法active `[1]`）；嵌套 schema_version 与其它业务字段/enum/format/unknown field失败只为 `invalid_request`；
- 完整 family Envelope Schema、方法 payload Schema 与 generated typed decode 对八方法逐项有 Accepted 用例；五个已知未实现方法也必须先完整 decode，畸形 payload 不得提前变成 KnownCatalogMethodNotImplemented；
- `kernel-contracts` 必须提供并测试结构化 `ContractFailureStage` 等价分类：`CallerSchemaValidation` 唯一映射 wire `invalid_request`；`WireDecodeAfterSchema`、`PayloadDecodeAfterSchema`、`GeneratedDiscriminatorMapping`、`SchemaCatalog` 均本地 ContractFailure；`UnknownSchema`/Catalog 与 post-Schema serde 失败逐项有定向测试，禁止匹配 ContractError/Schema/serde message；
- 五个已知但未注册方法 narrow 后得到不可序列化的本地 `KnownCatalogMethodNotImplemented`，不能成为 `KcpError`、`unsupported_method`、`method_unavailable` 或 `internal_error`；三个 registered variant 分别只路由 ping/create/get；
- dispatcher 构造只复用现有 `KernelClock`、`KernelIdGenerator`、`TaskApplicationBackend`，按 ping/create/get variant 传递所需端口，不创造平行接口；它不重复 Schema/deadline、不改写 `HandlerResult`；fake port 矩阵证明 response、ContractFailure 与 post-commit notification intents 均无损透传，family/discriminator/payload variant 错配不能进入 RegisteredRequest；
- 五类 preflight wire error 逐项断言固定 code/message、`schema_version=1`、`details=null`、`retryable=false`、protocol/message/status/payload/error互斥；最终 error envelope 必须通过不可替换 generated response Schema；crate-private fault seam证明最终门失败转本地 ContractFailure且不发送 wire response；
- public API/trait bounds 测试或 compile-fail 锚点证明 preflight response validator 不可注入、`PreflightLocalRejection` 与 `KnownCatalogMethodNotImplemented` 不实现 Serialize，且没有公开一站式全 Catalog 执行入口；
- 未实现五方法 handler、bytes/frame/transport/server 的情况下组合根必须拒绝启动 server；不得以“运行时返回 known-unimplemented”替代启动阶段完整性检查；
- typed application handler 输入必须已经通过对应 Envelope Schema、方法 payload Schema 与 typed decode；正常 dispatcher 路径还必须先完成三方法 registration narrow。现有公共 `handle_*` 的错配输入继续产生本地 InputMethodMismatch，但 handler 不得重复 protocol/auth/method/schema 分类，也不得把内部错误重新归为 `invalid_request`；
- `system.ping`、`task.create`、`task.get` handler 使用 fake backend/fake clock/deterministic fake ID generator 做库级矩阵；Value preflight/registration/dispatcher 同样只做不可连接库级测试，本阶段不得启动 Socket/Named Pipe 或构造可连接 server；
- 三方法第一次可观察操作是 clock 读取，并在任何 ID/backend 前按 `now >= deadline` 检查；入口已过期不访问 backend、不分配 ID；
- `system.ping` 复用第一次时间为 `kernel_time`，完成时第二次读 clock；`task.get` backend 后第二次读 clock；两者完成时到期均返回 `deadline_exceeded`；backend 错误与 deadline 同时出现时，成功读取到的到期结果优先，完成 clock 自身失败则 `internal_error`；
- legacy v1 `task.create` handler的deterministic fake generator必须证明恰好分配Task/Scope/Origin/receipt/Audit/Event六个合法、两两不同的UUID及独立非空correlation/dedup；active root v2必须以独立测试证明`RootTaskCreateAllocationV2`恰好分配Task/Scope/Origin/receipt/CreationProvenance/Audit/Event七个合法、两两不同UUID及独立opaque值。生产UUID版本不固定且不得从caller字段派生；handler在backend前验证对应allocation的格式与互异，失败为`internal_error`；
- deadline 必须把 Envelope RFC 3339 文本与 clock UTC instant 按时间点比较，禁止字符串字典序；deadline 解析失败在任何 ID/backend 前映射 `internal_error`；
- `task.create` adapter 只通过 backend 高阶端口调用现有 repository；测试 spy 必须证明 handler 不复制 normalize/hash/Audit/Event，SQLite adapter 只在 `SqliteStore::with_write_transaction` 内调用 `create_task`，不暴露 transaction/SQL；typed Envelope 到 `TaskCreateEnvelopeFacts` / request / allocation 的字段映射逐项断言；
- backend error 使用 §5.10.2 的闭集分类；SQLite adapter 对当前每个 `StoreErrorCode` 穷举转换，同名公开分类或 `Internal`，新增 Store code 导致编译/测试更新，不允许 wildcard 或 message 匹配；
- Created 与 Replayed 都返回 backend 给出的当前 Task；Created 必须同时返回与本次 operation Event UUID 相等的 `committed_event_id`，adapter 不能证明绑定时返回 Internal；仅 Created 携带一个 `TaskCreatedCommitted {task_id,event_id}` post-commit notification intent，Replayed 无 intent；Created 后即使 response contract 失败成为本地 HandlerContractFailure 也必须保留 intent；notifier 在事务外，失败不改变 response、不回滚事实、不声称 delivered；
- `task.create` 事务内不做第二次 clock 读取、不轮询/取消；backend 的 Created/Replayed/错误返回后才第二次读 clock。完成到期优先返回 `deadline_exceeded`；完成 clock 失败优先 `internal_error`；未到期才映射 backend 结果。Created post-commit 到期/clock failure 均保留事实与 intent；同一幂等键重放返回当前 Task；
- `task.get` 的 `None` 精确映射 `task_not_found`；Created/Replayed/get-found/not-found 均有独立用例；
- 下列legacy v1 handler测试继续保留用于代码事实与迁移回归：`parent_task_not_found`、v1 projection/fixture、v1 Created/Replayed；它们不得被计入active v2 Conformance；
- 每个成功 payload 在装 Envelope 前用原方法 response Schema 验证；每个最终成功/错误 Response Envelope 再用通用 Schema 验证，并断言 request_id 原样、protocol `1.0`、message `response`、success/error 互斥；Response 无 method discriminator，测试按原方法选择 payload Schema；
- 构造成功 payload 的 response Schema 失败必须安全转换为 `internal_error`；最终 error Envelope 若也无法验证则产生本地 HandlerContractFailure，不发送未验证响应；Created 路径必须证明该 failure 仍向组合根返回 post-commit intent；
- Event cursor 只使用十进制 `outbox_position`，按严格递增位置轮询；拒绝 event ID、时间戳或 aggregate sequence cursor；
- CausationRef v2允许`command_request | event | action | action_transition`；EventEnvelope/ContentOrigin/Audit active producer使用相应v2，v1历史继续读取；child `task.created`直接causation Action，Action自身`action.state_changed`必须用transition anchor，不允许伪造`action.requested` Event或self-causation；
- `delivered_at` 只代表 Publisher 发布，不因某订阅者未消费而回滚，也不伪称全部订阅者已消费；
- `mark_delivered` 是 public 业务写 convenience，必须委托统一 `BEGIN IMMEDIATE` / `with_write_transaction`；只有 `COMMIT` 成功后才允许返回 `Marked | AlreadyMarked | NotFound`。transaction-bound crate-private helper 在外层主动 `Err` 或 panic 时必须 rollback，公共重试可再次 `Marked`；unhealthy store 上 public mark fail closed 且另一健康 store 确认未改变；writer contention 映射 `sqlite_busy`，释放后 retry 得到 `Marked`；两个独立 store/connection 争同一 position、不同 timestamp 时，成功路径恰好一个 `Marked` 与一个 `AlreadyMarked`，DB 时间等于 winner 传入值且 loser 不覆盖；
- at-least-once 重投允许同一 event/outbox记录重复投递，但 `outbox_position` 全局唯一且只分配一次；消费者按 dedup_key/event_id 幂等，跨聚合 outbox_position 不被解释为领域因果；
- AuditRecord v1 是 `agentd` 拥有的不可变本地事实，不带 revision；全部字段 required，无关联事实使用显式 null/空数组；未知字段、未知 audit_type、`policy_context` 未知字段和非 `system_internal` 的 null actor 被 Schema 拒绝；`task.creation_recorded` 必须有 UUID task_id 与严格创建快照（revision=1、goal、origin、proposer），其他 audit_type 的该快照必须为 null；Actor 非空时必须保存完整 revision 快照；v1 `audit_type` 闭集仅七类，不得误当作 v2 完整闭集；
- AuditRecord v2 active 合同（IC §6.16）：完整 wire 非“v1加字段”；`audit_type` v2 闭集必须精确包含 `task.creation_recorded`、`command.accepted`、`permission.evaluated`、`kernel.invariant_blocked`、`event.published`、`recovery.recorded`、`config.changed`、`approval.requested`、`approval.resolved`、`approval.invalidated`、`identity.challenge_expired`、`identity.credential_registered`、`identity.credential_rotated`、`identity.credential_revoked`、`identity.local_presence_recorded`、`identity.system_authentication_recorded`；未知 type 拒绝；列入闭集不等于 producer 已实现；
- AuditRecord v2 producer 固定矩阵（IC §6.16.2）逐项断言 approval initial/resolution/invalidation 与 challenge expiry 的 `audit_type`/`level`/actor·entry authority/task·action·PD·approval refs/`external_content_status`/`rollback_capability`/`outcome`/`reason_codes`/`summary`/`details`/null·empty/causation·correlation；禁止把上述业务塞进 `details` 或互相借用 type；Challenge expiry 固定 `identity.challenge_expired` 且 approval/PD refs 全 null；
- `AuditAllocationV2`（`audit_record_id,correlation_id,occurred_at,causation_ref`）是独立 Audit 路径正式对象；challenge expiry 必须消费该对象且禁止 `ApprovalEventAllocationV1`；root/child/approval 可从 bundle allocation 投影同一四元组语义；CAS loser/replay 不得重分配；
- AuditRecord 的稳定引用闭包必须结构化回答任务创建原因、Delegation、模型建议/推理、VerificationResult、修改资源、是否外发、回滚能力、Stop Fence/恢复影响，以及匹配规则、排序依据、policy mutation authority 与 auth evidence；`not_sent` 拒绝非空 manifest refs，`sent` 的 producer 至少提供 content origin/artifact/resource/model call/payload manifest/causation 支撑，`unknown` 必须有 reason code；ModelCallRecord/PayloadManifest/Delegation 当前只使用非空 stable ref，不声称已有 source Schema 或 UUID；
- `permission_decision_ref` 非空时 `policy_context` 必须非空；未来 Audit repository 必须将 nullable matched_rule_ref、policy_set_revision 与不可变 PermissionDecision 比对，失配使 Audit/业务/Outbox 同事务回滚；该跨对象一致性当前只有 Conformance 契约，没有 SQLite 实现或自动化测试；
- Computer Use Protected Surface/target/Snapshot generation/destination/operation/resource scope等参与判定事实必须按`MaterialAuthorizationProjectionV1`与`ObservationEvidenceProjectionV1`分层；任一observation变化使旧PermissionDecision/Lease不可消费并要求新decision，material变化必须invalidating Approval；只有Core证明material hash相等才可复用approved resolution，Profile只产出证据且不得写入Core权威字段；
- `rollback_capability` 必须由 ActionRequest.rollback_policy、Verification、Recovery 权威事实投影且不可独立编辑；事实缺失/不可判定用 unknown，可解析事实冲突使事务失败；`provider_id` 表示实际操作 Provider，`model_call_refs` 表示建议/推理参与者，二者可并存；同一模型操作的 provider 一致性属于未来 repository 检查；
- AuditRecord 不自动成为首批公开 Event、不自动进入 Outbox；业务契约要求审计时，业务事实、AuditRecord 与该业务事实要求的 Outbox 在同一事务提交，Schema/跨对象一致性/插入失败整体回滚；固定归因必须有顶层 required 字段，不能仅藏进 details，但 Schema 无法完全禁止开放 details 重复这些值；正文默认最小记录但不硬禁 Secret；
- SchemaGraph `TaggedUnion`测试必须证明discriminator enum与branch const双射、nested `record_kind`/`subject_kind` union、零/多tag与重复const fail closed、branch unknown field拒绝、collision与recursive layout处理；Rust生成serde enum并完成round-trip/negative decode，未来TypeScript从同一IR生成discriminated union且有narrowing fixture；
- 所有下述canonical projection与crypto对象都必须有official fixture：原始输入、规范化对象、精确JCS UTF-8 bytes的lowercase `jcs_utf8_hex`、SHA-256 lowercase `sha256`、字段边界断言；不得重复保存canonical JSON string。数组顺序/重复、required-null/empty与URI规范化均有tamper向量；
- task creation fixture三路径与wrapper必须精确匹配IC §5.3.1/§6.10.6：root `schemas/fixtures/kcp/task_create_normalized_hash.v2.json`、child `schemas/fixtures/task/child_task_proposal_normalized_hash.v1.json`、allocation合并`schemas/fixtures/task/task_creation_allocations.v1.json`。fixture wrapper是测试制品合同，不是business Schema、不得进manifest或生成业务类型；tamper使用strict RFC6901 pointer、`add|replace`与结构化expected，非法pointer/operation使harness失败；
- authorization projection official fixtures四路径与generic wrapper必须精确匹配IC §5.3.1：child delta `schemas/fixtures/task/child_task_delta_projection.v1.json`、material `schemas/fixtures/policy/material_authorization_projection.v1.json`、observation not_applicable `schemas/fixtures/policy/observation_evidence_not_applicable.v1.json`、observation observed `schemas/fixtures/policy/observation_evidence_observed.v1.json`。每份含raw Facts JSON、normalized object、lowercase `jcs_utf8_hex`/`sha256`与tamper；wrapper由`schema-tool::official_fixture::ProjectionFixture`解析，不进manifest；`kernel-authorization` harness从raw重算projection并比对normalized/JCS/sha256，`schema-tool` CLI oracle只对`/normalized_object`做validate/canonicalize；禁止复用v1/task-creation向量；observed必须覆盖snapshot成对空值、证据排序与伪provider负例；
- root tamper必须证明：auth非null先执行`KcpCommandEnvelopeV2` raw Schema并固定为`raw_schema_rejected`，随后不执行payload Schema、normalization或hash；`public_error={code:"invalid_request",details:null}`，receipt/idempotency两hash均为`not_computed`；deadline/request_id/idempotency_key合法变化两hash不变；context只改变idempotency hash；任一payload纳入字段改变两hash；parent_task_id raw拒绝；URI lexical equivalent normalize后两hash不变；数组顺序或重复数变化影响相应hash。child按单hash合同覆盖同类payload事实；
- `NormalizedChildTaskProposalV1`继续由`kernel-task-creation`跨Rust/未来TS得到相同JCS与hash；切片1b新增`kernel-authorization`，唯一拥有`ChildTaskDeltaProjectionV1`、`MaterialAuthorizationProjectionV1`、`ObservationEvidenceProjectionV1`的typed authoritative input、构造/规则验证/Schema再验证/JCS/SHA-256。两crate边界不可合并；`SubjectProjectionV1`仍留1c；
- `kernel-task-creation`继续作为root/child proposal normalization、root receipt/idempotency、child proposal/receipt hash与root/child allocation validation的唯一owner；`kernel-authorization`负责三authorization projection。两者都依赖`kernel-contracts`并调用`domain-policy`唯一URI parser，不依赖SQLite/KCP、不分配ID、不读repository、不写存储，全部事实由caller typed input注入；`domain-task`不得增加policy依赖。长期中立URI crate必须另立ADR，当前不得复制第二实现；repository/handler/materializer与V2InitialBuildActive仍未完成；
- allocation helper测试先过对应Schema，再使用typed `RootTaskCreateExternalUuidRefsV1`与`ChildTaskMaterializationExternalUuidRefsV1`验证七/十内部UUID互异、与完整external snapshot互异、opaque nonempty且彼此不同；测试/compile-fail必须证明API不接受自由UUID bag、每个Option/Vec字段都必须显式构造、字段新增会强制caller更新。allocation fixture的`external_uuid_refs`是与typed snapshot同形状的对象，并含Schema-valid/domain-invalid的内部重复、external碰撞和opaque重复向量及Schema-invalid空opaque向量；Schema invalid必须短路typed decode和domain validator，空opaque固定`schema_valid=false/domain_result=not_evaluated`，wrapper domain闭集不增加空opaque专用结果，不含JCS/hash，且不得声称证明generator非派生性；
- Approval v2 outer/inner tagged union、完整field table、predecessor required-nullable、新链append-only/replacement-only-through-invalidation与request/resolution/invalidation条件矩阵均由Schema正反例覆盖；五种mode approved/denied专属ref和`evidence_refs` exact set/null/empty逐格测试；invalidation producer对旧head种类分支断言：失效approved resolution时Audit `approval_resolution_ref`精确指该resolution，失效尚未resolution的request时必须为null，禁止伪造resolution ref；
- Remote signature v1使用RFC8032 Ed25519 primitive vector与项目preimage vector，覆盖algorithm tagged union、SubjectProjection hash、valid/tamper/replay/expiry/revocation和并发；Identity repository register/rotate/revoke/issue/get/consume/**expire-with-expected-issued**/revoke、canonical readback与同BEGIN IMMEDIATE transaction-bound helper均有并发/故障注入测试。Challenge get只读；remote/system resolve与consume的过期并发恰有一个CAS expiry Audit（`audit_type=identity.challenge_expired`，消费独立`AuditAllocationV2`），所有观察expired者均为`challenge_expired`且无Approval event；
- V2InitialBuildActive切片1a精确新增IC §13.6.3四项；该批落地时manifest=65（41 retained + 24 component-native），切片1b后现行为70；production bindings仍空。自动测试逐项断言exact title/ID/source/component/kind/version/compatibility/schema_version_field/direct refs，component DAG为`audit/task→common,policy→common`且无环；41 retained entry、ledger与source bytes不变；
- V2InitialBuildActive切片1b精确新增IC §13.6.4五项；该批落地时manifest=70（41 retained + 29 component-native），切片1c-i后为75，切片1c-ii后现行为83；production bindings仍空。自动测试逐项断言exact identity/direct refs、typed round-trip、required/unknown/bounds、联合分支/nullability、projection set/multiset/URI/time/hash稳定性、pseudo-provider拒绝、retained VerificationResult复用边界及四份authorization projection official fixtures/harness/oracle；
- V2InitialBuildActive切片1c-i精确新增IC §13.6.5五项；该批落地时manifest=75（41 retained + 34 component-native），production bindings仍空。自动测试覆盖五root identity/DAG、PermissionDecision mode映射、PolicyRule闭集、Approval双层tagged union、三Subject branch、request challenge/nullability、resolution专属证据槽、v1字段泄漏、Allocation UTC-second/CausationRefV2，以及SubjectProjection三branch official fixture/JCS/hash/逐字段tamper；
- V2InitialBuildActive切片1c-ii精确新增IC §13.6.6八项；manifest=83（41 retained + 42 component-native），production bindings仍空。自动测试覆盖八root identity/DAG、RemoteSignatureAlgorithm tagged union、allowed_decisions精确有序array const、nonce/key/signature长度、challenge state闭集、local/system evidence闭集、response禁字段、preimage JCS/SHA-256、v1字段泄漏及八份official fixtures；
- ContentOriginV2按IC §5.3完整stored wire覆盖全部required、Kernel-owned id/time/carrier/receipt、五值carrier闭集、UUID/hash/time/string约束与typed/JCS round-trip；`action_transition` carrier、v1 version和v1 action carrier泄漏均拒绝；
- AuditRecordV2按IC §6.16完整top-level wire与封闭task_creation_context/policy_context生成，16值audit_type精确闭集；逐字段missing/extra/type错、creation context条件、PD→policy context、not_sent manifest、unknown reason、actor/entry条件及v1/v2字段双向泄漏均拒绝；
- TaskCreationProvenanceV1必须由schema-tool原生TaggedUnion生成serde tagged enum，仅root_command_v2/child_action_v2两支；逐支required/null/hash/time边界、legacy kind、混支字段、缺字段/错类型均拒绝；
- AuditAllocationV2因§6.16.0a要求正式required对象、Schema验证且跨语言边界传递而source化为new-contract；四字段全部required，CausationRefV2四支、empty correlation、missing/extra/type错与typed/JCS round-trip覆盖；repository/producer关系互异与真实causation仍留后续切片；
- Event v2 Schema切片已精确新增IC §13.6.2的八项。自动测试逐项断言exact title/ID/source/component/kind/version/compatibility/schema_version_field/whole-schema direct refs；`ConfirmationModeV1`归common，event `allowed_refs=common,policy`后DAG保持`event→policy→common`无环；禁止第九个平行enum/近义Schema。该Event切片落地时manifest为61项；切片1a后为65项，切片1b后production总数为70项（41 retained + 29 component-native），production bindings仍空；三enum闭集精确为confirmation `generic|local|system_authentication|remote_signature|plan_revision`、record `request|resolution|invalidation`、subject `operation|task_proposal|plan_revision`；
- Event compiler专项证明Catalog facts与typed `EnvelopeWireBinding`只从同一strict `EnvelopeConditionalBinding` IR投影；IR保持discriminator、enum声明顺序、whole-root payload identity、branch string const与source order，不携带Rust命名。active exact五项和值/顺序/aggregate/payload ID+version、legacy exact三项均为编译器硬门；target closure中active envelope/payload任一方向partial均失败，legacy-only合法；
- `ActionTransitionRefV1`精确为封闭`{kind:"action_transition",action_id:UUID,transition_id:UUID}`且不复制revision/generation/schema_version；`CausationRefV2` source精确使用whole-ref `oneOf`四支：前三支是closed inline `{kind const,id UUID}`对象，第四支整支直接root `$ref` `ActionTransitionRefV1`，union层required `kind`四值enum与branch const双射并以`unevaluatedProperties:false`闭合。零/多tag、unknown branch/field、非UUID、把action_transition inline复制或fragment-ref均拒绝；
- `ActionStateChangedPayloadV1`逐字段required/required-nullable、整数/时间/UUID/unique array与Schema条件有正反例。`ApprovalStateChangedPayloadV1.change_kind`四值enum必须与四branch双射，逐格覆盖from/to kind及`from_head_ref,to_head_ref,request_ref,resolution_ref,invalidation_ref,replacement_request_ref`的required null/non-null真值表；特别证明initial与replacement同`to_record_kind=request`仍不歧义。未来repository还必须测试exact equality：initial `to=request`、resolution `from=request/to=resolution`、invalidation/replacement旧head/ref关系、replacement predecessor；Schema不能表达的任意字符串等值、合法状态边、revision+1、generation/PD/Approval/canonical chain关系由repository验证；
- `EventEnvelopeV2`字段精确沿用v1集合并固定schema_version=2；不得新增`emitted_at`或把`delivered_at`放入Envelope；outbox_position为正ASCII十进制，本批兼容v1继续接受前导零，未来repository重建输出无前导零普通十进制。五类type enum与五个conditional mapping双射，复用三个retained payload v1并绑定两个新payload v1，证明Envelope/payload版本正交；
- 注：下列 migration 0003 / Outbox 条目描述**切片3c 当前代码事实**（ADR-0009）：不执行 v1 业务数据迁移；production Outbox 为 v2-only；旧库 open reinitialize-required。
- migration 0003 descriptor identity精确固定`migration_version=3`、`name=versioned_event_outbox`、唯一SQL asset path=`rust/crates/kernel-sqlite/migrations/0003_versioned_event_outbox.sql`；该单asset覆盖ledger ALTER、replacement table、全部index/constraints/table-swap DDL。先在事务内把现有四列ledger升级为含`descriptor_hash/descriptor_format_version`，历史0001/0002保持SQL-only checksum+null descriptor；0003 exact三元组固定`shittim.kernel-sqlite.outbox-v1-to-versioned-v1`/`1`/`kernel_sqlite::migration::outbox_v1_to_versioned_v1`，与唯一asset raw SHA组成完整JCS descriptor。**transform 不迁移 v1 行**：非空 pre-0003 Outbox → `stored_data_invalid` + message 含 `reinitialize-required`；空表仅 shape 升级并 swap。DB CHECK 仍允许 `schema_version IN (1,2)`（asset 字节稳定），但 production decoder/open 只接受 v2。descriptor/ledger drift/`database_schema_too_new` 语义保持。
- production append 仅 `append_active_event_v2` 走私有`append_versioned_event` SAVEPOINT/sequence/position内核；public类型精确为`StoredEventEnvelope::ActiveV2(TypedEventEnvelopeV2)`、`OutboxRecord { envelope, delivered_at }`、closed `EventAggregateId::{Task(Uuid),Action(Uuid),ApprovalChain(Uuid),StopFenceGlobal}`及`PendingActiveEventV2 { event_id:Uuid,aggregate_id:EventAggregateId,occurred_at,causation_ref:CausationRefV2,correlation_id,dedup_key,payload:EventEnvelopeV2Payload }`。`PendingLegacyEventV1`/`append_legacy_event_v1`/`StoredEventEnvelope::LegacyV1` 已删除且无alias。active pending不得有caller可写event_type/aggregate_type/schema_version/sequence/position；store从payload variant+generated binding唯一派生type/aggregate并在分配前核对aggregate ID enum/payload ID，correlation/dedup非空。append失败即使外层继续commit也不占号；read按position稳定排序；`schema_version!=2` 或 corrupt row → `stored_data_invalid`，不推进cursor且不能mark delivered；writer contention、unhealthy/rollback与mark-delivered语义保持；cursor 本批继续接受前导零输入，DB重建输出普通十进制；
- migration 0005（`drop_v1_business_tables`）descriptor identity固定 version=5、name=`drop_v1_business_tables`、asset=`rust/crates/kernel-sqlite/migrations/0005_drop_v1_business_tables.sql`、transform `shittim.kernel-sqlite.ddl-only-v1`/`1`/`kernel_sqlite::migration::drop_v1_business_tables_ddl_only_v1`；在 content_origins/audit_records/task_create_idempotency 与 outbox schema_version=1 均为空时 drop 死 v1 表；非空 → reinitialize-required。open 后再次 `reject_legacy_v1_business_data`；
- 切片3c 删除测试：无 `create_task`/`TaskCreateCommand` production 符号；无 legacy Outbox append 入口；v1 write 路径测试移除；旧库拒绝与 v2-only Outbox/corruption/savepoint/root v2 测试覆盖；
- retained `event.poll` response v1的events item exact为EventEnvelope v1，绝不允许返回v2。当前production Outbox为v2-only（切片3c已删除legacy append与LegacyV1 runtime variant）；v1 poll遇到下一条v2不得跳过、降解或推进cursor。在未来versioned KCP response/MethodVersionBinding/handler切片完成前，不得启用可连接poll handler；
- 本次Event八Schema只实现`ActionTransitionRefV1` wire/ref；切片1b现已落地`ActionTransitionIntentV1` source/generated type，但Action repository/migration/producer仍未实现。compile/API负测继续禁止producer引入临时intent struct/自由JSON或仅凭类型宣称producer闭环；
- producer authority测试证明payload只由owner repository从同事务canonical facts投影；causation target必须先存在，event self-ID、Action self-causation、future target、同chain Approval事件因果与可检测cycle均整笔失败；
- `action.state_changed`与`approval.state_changed`正式Catalog producer均有同事务fixture，逐字段核对aggregate sequence、ActionTransitionRef/真实外部causation、correlation、allocation IDs/time及统一Outbox；Approval三CAS方法均消费`ApprovalEventAllocationV1`并投影对应`approval.requested|resolved|invalidated` Audit v2；approval replacement只发一个逻辑head事件；Challenge expiry只写`identity.challenge_expired` Audit而不产生Approval event；
- repository hard-gate测试覆盖Action/PD/Approval/Identity/ActionTransition/root/child闭集API、必要unique key、PD↔Approval↔Action authority、current-head/revision/lease CAS、四种child错误判定、commit后重放、corrupt reconciliation、旧库reinitialize-required拒绝与v1 write路径删除；
- active Event Catalog候选谓词逐项负测：reserved exact ID/title/source任一占用；结构候选要求`component=event,kind=envelope,schema_version.const=2`以及完整closed enum/whole-root mapping；普通event object/payload/envelope不得误抓。候选0/重复/partial/mixed metadata全部fail closed。exact root覆盖whole-schema ref、inline、fragment、missing、duplicate、enum无branch、branch无enum与branch形状偏移；生成精确包含named `EventTypeBinding { event_type:&'static str, aggregate_type:&'static str, payload_schema_id:&'static str, payload_schema_version:u64 }`、private fixed-size binding/type arrays及public `EVENT_ACTIVE_BINDINGS`、`EVENT_LEGACY_V1_BINDINGS`、`EVENT_ACTIVE_TYPES`、`EVENT_LEGACY_V1_TYPES` slices。bindings顺序分别等于v2/v1 Envelope root type enum声明顺序，type arrays由通用`const fn project_event_types<const N: usize>`从bindings投影，event type字符串只在binding初始化出现一次；生成artifact真实cargo编译通过，不得保留`EVENT_V1_TYPES`alias或第二份平行mapping/type数据；typed decode按Envelope版本选择且未知type/version/payload mismatch fail closed；
- 当前Schema机械检查必须证明manifest=83、production MethodVersionBindings精确为IC §13.5八方法集、source→generated byte稳定、失败无partial artifacts；Event migration/Outbox focused tests覆盖descriptor raw bytes/hash、ledger shape/too-new、空表0003升级与非空 legacy reinitialize-required、v2-only 事件矩阵、共享position/sequence、active aggregate mismatch、stored corruption（含 schema_version=1）/delivery gate、savepoint poison、migration 0005 drop 死表；并通过full `check-schema.sh`与`git diff --check`。不得把child/Action/PD/Approval、Publisher、versioned KCP poll或server写成已完成；
- Computer Use负向测试证明Profile只提交observation facts，不能写material fingerprint、Approval resolution/invalidation、PermissionDecision或Lease；Core v2 Schema不得倒灌Snapshot/Coordinate等Profile私有固定字段；
- 首批事件类型及payload严格为`task.created`、`task.state_changed`、`action.state_changed`、`approval.state_changed`、`stop_fence.activated`，使用点号小写；
- `stop.activate` 就是首批 Emergency Stop 入口：先持久化 Fence generation 与事件；随后撤销**所有受 Stop 影响且仍活跃的副作用 Action Lease**，并在同一原子状态变更中释放其全部 Resource Lock；撤销所有临时 Permission Lease / Privilege Lease，且后续消费必须拒绝；向**所有 in-flight Extension 调用**发送 cancel。无副作用只读诊断的已取得事实不得被误删或强制转未知，Fence 后仍可发起新的只读诊断；只有已开始且不能安全取消、其副作用结果不确定的 Action 进入 `unknown_side_effect`。重复调用保持同一 active generation；Fence 不因 Security Mode 恢复而解除，KCP 第一版不存在清除方法；
- PermissionDecision v2包含不可变id/evaluated_at/policy_set_revision、material authorization fingerprint与observation evidence fingerprint；同一Action decision_revision严格递增，ActionRequest引用current decision；
- Approval v2 Schema是真正判别联合：record_kind=request|resolution|invalidation，subject exactly-one operation|task_proposal|plan_revision；错误组合、零/多subject均拒绝；
- Approval repository不可变append + current-head CAS；并发resolution/invalidation恰好一个成功，失败`approval_head_conflict`；v1 production write拒绝；
- implicit allow与Delegation authority不生成Approval；operation subject绑定Action、PermissionDecision与material fingerprint；task_proposal/plan_revision绑定对应revision/hash；
- material fingerprint变化追加invalidation并重评；纯observation fingerprint刷新使旧PD/Lease失效，只有Core证明material等价后新PD可复用旧approved resolution，Profile不得判断；
- canonical mode映射：generic按Policy入口；local只证明local presence；system_authentication必须覆盖Kernel issue `SystemAuthenticationChallengeV1`、OS authority evidence、同事务current/expiry/binding/evidence/head校验与consume，取消/失败不写resolution；remote_signature要求challenge+nonce+audience+task/SubjectProjection/material+expiry+signature且nonce单次消费；两类challenge过期都以expected-issued CAS持久化、并发观察统一`challenge_expired`且不发Approval event；owner标签不构成认证；
- plan_revision approved后相关Action全部重新Policy evaluation，不自动继承operation approval；
- Schema生成链专项可执行断言：schema-tool与kernel-contracts必须调用同一`contract_validator_options`，已知`uuid`/`date-time`format被assert，synthetic unknown format在`SchemaRegistry`后的`compile_all`、production `check`及runtime `SchemaCatalog`编译路径均结构化fail closed，且不得修改retained source；
- Alias专项可执行断言：root pure-ref生成transparent public alias且`AliasResolution`可从crate root导入，chain按canonical identity记录到首个non-Alias terminal，fragment alias不生成平行declaration，pure Alias cycle无terminal时fail closed；nullable terminal经Alias保持字段nullability，Alias参与合法object/union recursive引用时仍由声明SCC layout闭合而非误判为pure cycle；
- 首批12 root逐项执行Schema validate→`decode_validated`→serialize→revalidate；开放V2 Envelope payload额外字段flatten后无损，closed root unknown field拒绝；renderer统一member namespace必须让声明`additional_properties`及snake_case归一同名的`additionalProperties`与synthetic flatten member均fail closed，错误含Schema identity与字段；
- integer range专项：neutral IR记录inclusive min/max而不含Rust名；V2 Envelope payload `schema_version`编译时类型断言精确为`u32`，`u32::MAX` Schema/serde round-trip通过，`u32::MAX + 1` Schema拒绝；没有完整安全范围的41 retained integer public bytes保持既有`i64`；
- 通用decode taxonomy专项：构造Schema通过但synthetic typed `T`拒绝的类型，精确断言`ContractError::DecodeAfterSchema`→`ContractFailureStage::TypedDecodeAfterSchema`与internal classification；safe structured stage/classification/schema_id不得包含serde detail，原始错误detail只留内部诊断；
- MethodVersionBinding完整validator接受合法非空synthetic 8-method registry；canonical排序为command<query再method UTF-8，version数组positive严格升序唯一互斥、map key canonical decimal且request/response key闭包精确；entry kind/version/root const/compatibility/generation target closure逐项负测；legacy request仅legacy-validation-only，active request/response不得legacy，projection/allocation kind不可绑定；synthetic/test manifest同样只允许正式compatibility五值，无测试旁路；八方法expected set直接从registry唯一选中的active V2 Envelope discriminator enum派生，不读generated catalog、不手写第二份生产表；
- `validate_production_manifest_stage`独立断言production `schemas/manifest.json` bindings精确等于IC §13.5八方法目标表（切片3a起；method覆盖从registry V2 Envelope facts派生，lifecycle为task.create active=[2]/legacy=[1]、其余active=[1]），并由production CLI `check`/`generate`在load后、plan/render前调用；非空synthetic binding catalog的end-to-end plan/render测试必须走library API并使用显式non-production registry profile。synthetic registry不靠路径、env、fixture目录名或调用栈隐式特判，production CLI始终调用该stage gate；generated binding catalog只证明library facts，不证明dispatcher/handler/server/SDK可用；未满足IC §13.7不得宣称runtime可用；
- compatibility闭集精确五值并与binding lifecycle正交；`breaking-replacement`/`new-contract`/`legacy-validation-only`/`legacy-read-only`按IC一般规则正反例覆盖，synthetic/test manifest也不得出现额外值；新12 entry与四个retained compatibility演化标签按IC §13.6矩阵验证，其余37 retained保持v1-stable，改标不改变retained ledger/source hash；`new-contract` projection/allocation靠kind+unreferenced约束保持internal，不增造internal compatibility；
- component-native ID精确匹配`https://schemas.shittim.local/<component>/<snake_case_name>/vN`并镜像`schemas/source/<component>/<snake_case_name>.vN.json`；一般title-derived name按末尾`V<version>`后的canonical snake_case得到，但KCP envelope必须由hard gate按`component=kcp`、`kind=envelope`及精确title应用固定规范stem：`KcpCommandEnvelopeV2`去掉领域前缀`Kcp`得到`command_envelope`，`KcpQueryEnvelopeV2`得到`query_envelope`，对应精确ID/source为`https://schemas.shittim.local/kcp/command_envelope/v2` + `schemas/source/kcp/command_envelope.v2.json`、`https://schemas.shittim.local/kcp/query_envelope/v2` + `schemas/source/kcp/query_envelope.v2.json`；不得生成`kcp_kcp...`。这不是任意例外，而是component已编码kcp命名空间的规范stem规则。exact authority/segments/version/root const/compatibility全部由`SchemaRegistry::load`硬门验证，query/fragment/percent encoding/dot/empty/extra/tail slash/`.json` URL、版本不一致全部拒绝；测试明确证明当前namespace-only实现已被替换；12个新ID不进入41项ledger；
- active Catalog authority选择逐family验证registry中`component=kcp,kind=envelope,title=KcpCommandEnvelopeV2|KcpQueryEnvelopeV2,exact V2 ID/version,compatibility=breaking-replacement`恰好一项；0或>1 fail closed，读取其root discriminator enum。负测证明不再按ID suffix选取，retained v1 Envelope不成为active authority；生成常量精确为`KCP_ENVELOPE_AUTHORITY_COMMAND_METHODS`、`KCP_ENVELOPE_AUTHORITY_QUERY_METHODS`、`KCP_ENVELOPE_AUTHORITY_METHODS`，active目录不再名为`KCP_V1_METHODS`；
- 本批12项逐source `$ref`依赖表按IC §13.6正向解析，并以mutation拒绝未列出的直接ref、relative跨document ref、缺失absolute fragment、allocation含ref及component closure缺失；
- namespace migration测试保留旧`$id` bytes、component归属/ref closure并拒绝prefix伪装；`SchemaRegistry::load`必须在任何公开registry前核对41项ledger的id/component/source与实际source SHA-256，合法Schema bytes篡改也失败；manifest source拒绝absolute、`..`、source-root外repo-relative、backslash、空/dot segment与prefix trick，filesystem拒绝source file/ancestor symlink并验证canonical containment；单一SchemaNode walker表驱动覆盖`properties/patternProperties/dependentSchemas/$defs/definitions`、`additionalProperties/unevaluatedProperties/propertyNames/items/contains/unevaluatedItems/contentSchema/not/if/then/else`、`prefixItems/allOf/anyOf/oneOf`及root/boolean，严格拒绝错误容器/node类型；loader以walker为每个document建立authoritative SchemaNode pointer index，`resolve_ref`/public `schema_at`拒绝`/const`、`/default`、`/examples/0`、`/enum/0`实例位置，即使值长得像Schema，合法`$defs/properties/items` pointer通过；identity audit、registry ref/component gate、target closure与codegen support audit复用同一walker，不得有第二套通用位置递归；map key恰为`$ref/$id/$schema/$dynamicRef`时按普通名称处理但其value内真实ref仍受gate；nested非root `$id`和named/dynamic/recursive identity/ref keyword在load统一fail closed，CLI validate/check/generate对`$dynamicRef`均在load失败；local/relative/absolute refs与递归cycle保持覆盖；
- artifact transaction只接受恰好一个artifact root，多root/零root拒绝。LockPort 与 TransactionFs 分层：transaction 从 FD advisory lock 成功且 owner metadata 发布后开始；lock open/create/type/symlink/try_lock/owner I/O 独立验收，FD lock 权威、owner 仅诊断、不 unlink/PID 回收。TransactionFs 只验收语义 mutation/durability boundary，read/metadata inspection 不进入 fault 闭集；typed `OperationEvent` 必含 semantic phase、operation、Before/AfterSuccess、logical path roles、journal target phase 和仅用于重复文件/目录操作的 occurrence。正式 Committed journal rename 是提交点：之前 existing=Old/absent=Absent，之后=New。FailureDisposition 包含 `NoMutation`、`RolledBackBeforeReturn`、`RecoveryRequired { RestoreOriginal | CleanResidue }`、`CommitOutcomeUncertain`、`CleanupDeferred` 与 `StoredStateInvalid`；后者表示正式journal存在但无法解码或验证，恢复必须fail closed且不改变root、不授权rollback/cleanup。矩阵从真实 trace 自动发现每个 reachable target，逐项 CrashBefore/CrashAfterSuccess/IoNoEffect 与明确声明的 IoPartial；Crash sentinel 单独传播，IoNoEffect 不执行 operation，禁止 after hook 冒充 I/O。每例精确断言 target hit 一次、trace prefix、即时完整 TreeSnapshot、结构化 FailureDisposition、recover#1 路线/终态和 recover#2 Noop/空 mutation trace；禁止 old|new、message contains 或 occurrence 猜 phase。snapshot 包含 root/stage/backup/discard/formal journal/temp/lock，独立 reference model 只消费 typed operation effect。control-flow fault conformance 不等同真实断电介质模型；partial I/O 无 journal-temp/stage-write/copy/remove 明确 fake effect 时不得宣称覆盖。正式journal损坏保留并 fail closed；成功无stage/backup/discard/journal/temp residue（persistent lock允许）。其它平台未real-platform验证并fail closed。
- `V2InitialBuildActive`测试覆盖source/generated/repository/handler、v1 write路径删除、旧库`reinitialize-required`拒绝、以及bindings+v2 dispatcher+v2 repository作为fresh baseline初始交付；证明无v1/v2双写、无自动清库/隐式升级/读后补写；Publisher与poll v2不在本里程碑；
- Error Catalog mirror drift测试证明docs不独有code/message/details/retryable，并证明已删除无稳定触发的旧child重复错误码；
- allocation/producer测试逐项验证RootTaskCreateAllocationV2与ChildTaskMaterializationAllocationV1 ID purpose、互异、opaque correlation/dedup和重放来源，并验证ApprovalEventAllocationV1的五个required字段、ID互异性和三CAS方法消费，以及`AuditAllocationV2`四字段、challenge expiry独立消费与approval路径禁止混用；
- RecoveryDecisionCandidate 的 retry_original 只在副作用未发生且幂等保障成立时合法；RecoveryAttemptRef 不可变追加；Verification recommendation=retry 不直接执行重放；
- JSON Schema 全部声明 2020-12，RFC 8785 canonical JSON 测试向量跨 Rust/TypeScript 产生同一 SHA-256；
- Schema 生成运行两次 byte-for-byte 一致，生成物无手改漂移、`$id` 唯一、`$ref` 可解析；
- Unix Domain Socket 与 Windows Named Pipe 的传输合同测试共用同一 KCP Schema；不得用 JSON-RPC 特有字段替代 KCP Envelope；
- **KCP / Core Schema 不得因 Computer Use 而扩张固定字段**；Desktop model、Snapshot、Coordinate、Input Session Lease 等不得出现在 Core 顶层必选对象中（BASE 负向测试）。

## 6. Memory（BASE）

测试：

- candidate 不能直接有效；
- 来源缺失；
- 用户明确纠正；
- 冲突；
- 过期；
- deletion cascade policy，包括敏感内容、Secret、摘要、索引和派生记忆；
- provider index unavailable；
- scope leakage between private/domain/channel；
- profile evidence；
- self preference deletion；
- sensitive attribute inference is labeled with ContentOrigin and governed by applicable Policy rather than blocked by inference category；
- Memory 可在明确来源、作用域和规则下保存敏感内容、认证数据与 Secret；
- mid-term summary not automatically permanent。

## 7. Initiative（BASE）

测试：

- Opportunity 不直接执行；
- L0-L5；
- read-only boundary；
- duplicate suggestions suppression；
- daily/model budget；
- delegation matching；
- user revocation；
- no infinite task generation；
- system maintenance vs user task labeling。

## 8. Extension SDK Base（BASE）

通用（无条件）：

- handshake/version；
- permission projection；
- invoke/schema；
- timeout；
- cancel；
- progress；
- Extension Event 当前没有正式 Schema/transport/persistence/subscription 合同；BASE 负向测试证明 Profile 私有事件不能进入 Core EventEnvelope/Outbox，且产品不得声称 Kernel 已可保存、索引、订阅或稳定转发；
- object handle expiry；
- crash/quarantine；
- reconfigure；
- update/rollback；
- permission/source declaration diff；
- Native Extension 的 OS-enforced、host-enforced、declaration-only 风险展示准确，声明不会被当作隔离；
- Native、AI 自写和社区 Extension 在无匹配拒绝/确认规则时可安装、运行和使用其声明网络能力；
- incompatible profile；
- Manifest 声明 ≠ discovery-visible ≠ invocable：只有正式兼容的 versioned operation payload/result Schema（事件能力再加 event Schema）、negotiation 与 probe 全部成立才可 discovery-visible；当前调用仍须独立满足 grant（若需要）与边界检查；
- 跨扩展直接调用应在测试环境失败；
- Privilege Profile 不得直连 Broker；CapabilityNeed 不得绕过 Registry；
- Core 自改防护（扩展侧无法获得写 Core 的合法 API）。

### 8.1 Optional Profile 挂载规则（BASE 元测试）

- SDK 必须按固定语义拒绝：当前集合不提供为 `unsupported`，已支持但暂不可用为 `unavailable`，版本无兼容交集为 `incompatible`，已发现但当前调用未获 grant 为 `capability_not_granted`；
- 声明了某 Profile 的扩展在版本协商失败时不得进入 ready 并对外提供该 Profile 能力；
- 正式 versioned operation payload/result Schema 缺失、不兼容或未协商时，Registry 不得标为 discovery-visible；事件能力还必须有正式 event Schema；
- Profile 专用深度测试挂在第 11 节及后续 IF_CLAIM 套件，不进入本节无条件清单。

## 9. Model Provider（BASE）

- Responses；
- Chat Completions；
- Anthropic Messages；
- custom base URL；
- local endpoint；
- streaming；
- structured output；
- tool call；
- cancellation；
- context overflow；
- fallback；
- 云调用不按敏感类别审查、脱敏或阻断任务数据（含认证数据）；
- Context Pack 仅以任务相关性最小化选择内容，不能因内容分类擅自阻断；
- usage accounting 与最小元数据记录；
- 所有外部调用携带 Task、Action、ContentOrigin、适用规则和最小恢复元数据，Provider 文本或外形相似的外部 JSON 不得升级为 Kernel 事实。

## 10. Companion（BASE）

- 承认 AI 身份；
- 不冒充用户；
- 区分自己的建议和用户观点；
- 普通聊天不启动 Planner；
- 简单任务不加载无关工具；
- 不把情绪表达自动转任务；
- 能解释权限拒绝；
- 能承认不确定和失败；
- 在 Computer Use 未启用时不得向用户暗示“已支持桌面操控”（IF 负向 / BASE 产品诚实性）。

## 11. Optional Profile：Computer Use

> **门控**：本节各专节使用精确 `IF_CLAIM(...)` 并统一读取 `ProfileClaim` 集合；完整 Computer Use 的固定成员与机械闭包见 `COMPUTER_USE.md` §3.3。
> **NEVER_CORE_GATE**：本节整体不得作为纯 Core 发布阻塞项。
> 深度场景在声明后为硬门；未声明时仅要求第 1.2 节诚实错误。

### 11.0 启用、Schema 与诚实性（BASE 负向；任一 Computer Use claim 时正向）

- 当前安装/发行/协商集合不提供 operation → `unsupported`；
- 已支持但 disabled/quarantined/backend/session 暂不可用 → `unavailable`；
- SDK/Profile/operation payload/result Schema 版本无兼容交集 → `incompatible`；声明事件能力时 event Schema 同样纳入版本兼容检查；
- capability 已 discovery-visible 但当前调用未获 grant → `capability_not_granted`；
- Manifest 或 probe 孤立存在均不得报告 discovery-visible；正式 versioned operation payload/result，以及声明 `computer.event` 时的 event Schema 必须存在、兼容并完成协商；
- discovery-visible 不等于 invocable；只读 observation 无需副作用 Action grant，但仍需 Schema、身份/scope 与数据策略；
- 合同缺失 fail closed，不得当作 Policy allow；
- maturity token 只能是 `contract-only`、`schema/SDK`、`composition`、`provider contract`、`real-platform`；`distribution_asserted` 是独立 boolean，不得塞进 maturity。

### 11.1 Scene — `IF_CLAIM(computer.scene)`

- multi-display；
- workspace；
- focus；
- window generation；
- partial visibility。

### 11.2 Semantic — `IF_CLAIM(computer.semantic)`

- element tree；
- action；
- unavailable/empty tree；
- stale element ref。

### 11.3 Capture — `IF_CLAIM(computer.capture)`

- display/window/region；
- scaling；
- redaction；
- object handle；
- frame expiration；
- **截图尺寸/频率预算**（声明 capture 时硬门；未声明则不要求启动视觉管线）。

### 11.4 Input — `IF_CLAIM(computer.input)`

- absolute/relative；
- wrong focus protection；
- user takeover；
- lock screen revocation；
- protected surface；
- Profile-local Input Session Lease 闭合与 Core Action Lease / Resource Lock / Stop Fence 协作；
- Input Session Lease **不是** Kernel Permission Lease 的替代。

### 11.5 Snapshot / Operation Snapshot — `IF_CLAIM(computer.snapshot)`

- `computer.snapshot` 必须同时声明至少一个实际观察来源 facet：`computer.scene`、`computer.semantic`、`computer.capture`、`computer.visual_grounding` 或 `computer.application`；

- merge/deduplicate；
- number layout；
- task filtering；
- stale “click 7”；
- new snapshot on UI change；
- Profile-local 对象不倒灌 Core Schema 固定字段。

### 11.6 Target Resolution、动作路径与 Execute Gate — `IF_CLAIM(computer.operation.execute)`

> 声明 `computer.semantic`、`computer.application` 等 facet 不自动等于声明副作用执行；只有 `computer.operation.execute` 触发本套件。只读 scene/capture/event/semantic/application/visual-grounding 实现不因此被强迫通过 input/副作用执行套件。

- Core Execution Boundary 与 Profile Readiness Gate **分离**：后者不能替代前者；
- 当前焦点窗口/元素不得作为唯一目标绑定；焦点只能用于排序或 Prepare 提示；执行必须绑定 stable provider/semantic/Snapshot generation/user-selected candidate 等引用；违规返回结构化失败且 Profile Readiness Gate 失败；
- Prepare 后 reobserve；
- stale generation / 裸旧坐标拒绝；
- Policy allow 但 Snapshot 过期 → 不得执行；
- Snapshot 新鲜但 Stop Fence 激活 → 不得执行；
- capability 未 grant → 不得执行。

### 11.7 Verification — `IF_CLAIM(computer.operation.verify)`

- `computer.operation.verify` 必须与 `computer.operation.execute` 一起声明；纯只读 scene/capture/event/semantic/application 不被强迫产生副作用验证；
- semantic success；
- visual success；
- file/external confirmation；
- false provider success；
- 验证政策按匹配规则与能力处理不可验证的高风险结果：可允许、确认、拒绝或进入恢复，不能以风险等级默认阻断。

### 11.8 Event — `IF_CLAIM(computer.event)`

- operation/event payload 使用正式 versioned Schema，且版本已协商；
- event type/version、source extension instance、scope、extension-local sequence 与可选 task/action correlation 可测；
- 当前无 Extension Event transport/persistence/subscription 合同时，负向测试证明事件不得进入 Core EventEnvelope / Outbox，也不得宣称可保存、索引、订阅或稳定转发；
- 未来若建立正式 Extension Event 合同，仍不得自动晋升 Core Event Catalog。

### 11.9 Visual Grounding — `IF_CLAIM(computer.visual_grounding)`

- 只产生候选，不直接输入；
- 与语义候选去重；
- 低置信度不默认阻断（Policy 决定）。

### 11.10 Application Adapter — `IF_CLAIM(computer.application)`

- 不绕过 Task/Policy/Lease/Lock/Stop；
- 不直连 Broker；
- 仍通过完整 Execute Gate。

### 11.11 远程监督与降级 — `IF_CLAIM(ANY(computer.supervision.automatic,computer.supervision.approval,computer.supervision.cooperative,computer.supervision.observe_only))`

- 只测试实际声明的 supervision mode claim；
- automatic / approval / cooperative / observe-only；
- observe-only 不要求 `computer.input`、副作用 Action Lease 或副作用 Execute Gate，但仍要求观察 Schema、scope 与数据策略；
- 任何副作用降级路径仍过完整 Execute Gate；
- degraded 状态可见，不静默假装完整。

## 12. Hyprland — `IF_CLAIM(computer.platform.hyprland)`；声明真机时再加 `IF_REAL(computer.platform.hyprland)`

- outputs/workspaces/clients；
- focus and event stream；
- special workspace；
- fractional scaling；
- animation stabilization；
- Provider crash/fallback；
- AT-SPI and Capture composition。

**NEVER_CORE_GATE**：未声明 Hyprland 支持时不阻塞 Core 发布。

## 13. niri — `IF_CLAIM(computer.platform.niri)`；声明真机时再加 `IF_REAL(computer.platform.niri)`

- scrolling layout；
- window outside viewport；
- focus moves viewport；
- reobserve before input；
- partially visible target；
- dynamic capture source（若声明）；
- event stabilization。

**NEVER_CORE_GATE**：未声明 niri 支持时不阻塞 Core 发布。

## 14. 其他平台 Provider 合同

### 14.1 跨平台非 Linux Provider

- **NEVER_CORE_GATE**：Core 发布**不**要求至少一个跨平台非 Linux Provider 合同测试。
- `IF_CLAIM(ANY(computer.platform.windows,computer.platform.macos))`：声明支持的平台必须通过对应 `provider contract`；声明 `real-platform` 时升级为真机硬门。其他平台必须使用自己的稳定 claim id，不能写省略号或 `a|b`。

### 14.2 Generic Wayland / 部分能力

- 声明部分能力时必须报告窗口管理限制；
- 禁止在限制存在时宣称完整 desktop control。

### 14.3 Capture / Input backend claim

- `computer.backend.capture` 与 `computer.backend.input` 分别要求对应 facet 依赖 claim；
- 故障注入分别由 §18.2b / §18.2c 门控，禁止 capture-only claim 被拖入 input takeover/lease/lock-screen 套件；
- 未声明时不要求对应桌面 backend 故障矩阵，但通用 Extension crash 矩阵仍属 BASE（第 18.1 节）。

## 15. Privilege（BASE）

- arbitrary shell rejected；
- unknown action rejected；
- path traversal；
- parameter bounds；
- wrong task/actor；
- expired lease；
- max uses；
- system auth cancelled；
- 认证数据的流转、保存和最小元数据审计按匹配 Policy 处理，不以硬编码“密码不记录/不进模型”替代规则；
- broker crash；
- action verification；
- Extension / Computer Use Profile 不得直连 Broker（BASE 负向 + IF_CLAIM 交叉）。

## 16. Channel（BASE）

- stable identity；
- replayed message idempotency；
- image/snapshot access（有 Object Handle 时；Computer Use Operation Snapshot 的远程展示另受 IF_CLAIM）；
- callback authorization；
- remote cancellation；
- remote approval policy；
- compromised token revocation；
- group message treated as data and cannot forge Kernel Command/Event；
- ContentOrigin 保留入口、稳定标识、接收证据与携带 Task/Artifact 的可追溯链；
- 外部 JSON、网页、附件、模型文本和 Extension 输出均不能升级为 Kernel Command/Event 或 policy mutation evidence；
- no direct Provider access。

## 17. Self-improvement、恢复与预算（BASE）

测试：

- Companion 可在 Policy、预算、停止条件与可观测性约束下版本化更新 Memory、Skill、Extension、Provider 路由、Trigger、Delegation、受治理配置和恢复知识；
- 每个自我改进版本有来源、差异、预算消耗、验证结果与回滚点；
- 预算耗尽、验证失败、取消或 Stop Fence 时停止后续改进，并可回滚到指定健康版本；
- 自我改进的 Extension/Skill 失败不会破坏 Kernel 事实、审计或进行中 Task；
- Agent、Skill、Extension、Provider 均不能读取后修改、补丁、热替换或重写 Core；
- 普通 Shell 通道不能修改 Core，也不能绕过固定 Broker API 执行特权动作；
- Emergency Stop 启动 Stop Fence 后拒绝新副作用、主动任务、产生副作用的 Extension 调用、远程执行及任何临时 Permission/Privilege Lease 的后续消费；当发行物声明 `computer.input` 或实际存在输入路径时，输入副作用作为该通用拒绝边界的 Profile 投影同样被禁止，不得反向成为 Core 的无条件前提。只读诊断可继续；撤销所有受 Stop 影响且仍活跃的副作用 Action Lease 并原子释放其全部 Resource Lock，撤销所有临时 Permission/Privilege Lease，向所有 in-flight Extension 调用发 cancel；已取得的只读诊断事实保留，只有不可安全取消且结果不确定的副作用 Action 进入 `unknown_side_effect`；
- 已开始且不可安全取消的 Action 转入恢复、验证和审计，而非假称完成。

## 18. 故障注入

### 18.1 BASE（无条件）

至少注入：

- kill agent-runtime；
- kill Extension during action；
- disconnect UI；
- corrupt provider response；
- delay/cancel model stream；
- DB busy/full disk；
- object missing；
- network loss；
- permission revoked mid-task。

### 18.2a Platform fault suites — `IF_CLAIM(ANY(computer.platform.hyprland,computer.platform.niri,computer.platform.windows,computer.platform.macos))`

- 对声明平台注入 platform session/compositor/window-system restart；
- platform facts 重建后旧 provider/window/generation refs 必须失效，诚实返回 `stale_state` / `unavailable` 并按能力恢复；
- 本套件不隐含 capture backend、input lease、用户接管或锁屏测试。

### 18.2b Capture fault suites — `IF_CLAIM(ANY(computer.capture,computer.backend.capture))`

- capture backend loss/recovery；
- 已发 Capture Frame/Object Handle 的过期与读取失败可辨识；
- capture-only claim **不得**被要求运行 Input Session Lease、user takeover、lock screen revocation 或 input Resource Lock 套件。

### 18.2c Input fault suites — `IF_CLAIM(ANY(computer.input,computer.backend.input))`

- input backend loss/recovery；
- 用户接管抢占 input Resource Lock，并闭合或挂起 Input Session Lease；
- 锁屏/session unavailable 撤销 Input Session Lease；
- Stop Fence、Action Lease 与 Resource Lock 丢失后禁止继续输入。


## 19. 资源与性能

### 19.1 BASE

不是追求固定数字，而是确保：

- 基础空闲不启动视觉/Python/WASI（无声明/无任务需要时）；
- Tool Schema 按需；
- 日志和对象有保留策略；
- Extension 闲置可回收；
- Memory 检索有预算；
- Agent Runtime 重启可恢复；
- Channel 不造成无界队列。

### 19.2 `IF_CLAIM(ANY(computer.capture,computer.visual_grounding))`

- Screenshot 有尺寸/频率预算；
- 视觉/解析管线按需启动，不得在无 Computer Use 任务时常驻拖垮空闲基线。

## 20. 发布门槛

### 20.1 Core 发布门槛（BASE，无条件）

核心发布必须：

- 所有 BASE 不变量通过；
- Schema 兼容验证；
- 无 v1 业务数据迁移义务；旧库 reinitialize-required 拒绝；仅允许 fresh baseline 的 schema ledger/DDL 演进有 preflight/rollback；
- Privilege 安全测试；
- Extension SDK **Base** conformance（第 8 节）；
- 远程入口安全回归；
- Freedom-first 默认 allow、确定性规则排序、Condition fail-closed、自然语言治理、ContentOrigin 与 Stop Fence 回归；
- 首批 KCP、事件/错误目录、Outbox cursor、deadline/auth/schema version 与 RFC 8785 生成链回归；
- 自我改进版本/预算/回滚与 Core 不可自改回归；
- 文档单一事实源检查；
- **能力未协商 / 可选 Profile 未声明时的诚实 unsupported 回归**；
- **明确不要求**：Computer Use 全套、跨平台桌面 Provider 合同、Hyprland 真机、niri 真机、截图预算（除非该 Core 发行物额外作出对应 claim）。

### 20.2 Optional Profile / Platform 发行门槛（条件性硬门）

发行物、安装包、Manifest、用户文档或发布说明一旦声明支持下列任一项，则对应套件**全部**成为该发行物的硬门：

| 声明 | 硬门套件 |
|---|---|
| `computer.scene` | 11.0 + 11.1 |
| `computer.semantic` | 11.0 + 11.2 |
| `computer.capture` | 11.0 + 11.3 + 18.2b + 19.2 |
| `computer.input` | 11.0 + 11.4 + 18.2c |
| `computer.event` | 11.0 + 11.8 |
| `computer.visual_grounding` | 11.0 + 11.9 + 19.2 |
| `computer.application` | 11.0 + 11.10 |
| `computer.snapshot` | 11.0 + 11.5；必须同时声明至少一个实际观察来源 facet |
| `computer.operation.execute` | 11.0 + 11.6；并按实际执行路径声明所用 facet；输入路径必须另有 `computer.input` |
| `computer.operation.verify` | 11.0 + 11.7；必须同时声明 `computer.operation.execute` |
| `computer.supervision.automatic` / `computer.supervision.approval` / `computer.supervision.cooperative` / `computer.supervision.observe_only` | 11.0 + 11.11 中对应模式；`observe_only` 不绑定 input 套件 |
| `computer.platform.hyprland` | 11.0 + 第 12 节 + 18.2a；同一 claim maturity 为 `real-platform` 时再加 `IF_REAL(computer.platform.hyprland)` |
| `computer.platform.niri` | 11.0 + 第 13 节 + 18.2a；同一 claim maturity 为 `real-platform` 时再加 `IF_REAL(computer.platform.niri)` |
| `computer.platform.windows` / `computer.platform.macos` | 11.0 + 第 14 节对应 `provider contract` + 18.2a；同一 claim maturity 为 `real-platform` 时再加对应 IF_REAL |
| `computer.backend.capture` | 11.0 + 11.3 + 18.2b + 19.2；并要求 `computer.capture` 依赖 claim |
| `computer.backend.input` | 11.0 + 11.4 + 18.2c；并要求 `computer.input` 依赖 claim |

规则：

- 发布门读取与 `IF_CLAIM` / `IF_REAL` 相同的 `ProfileClaim` 集合，不从散文另造第二份状态；
- `distribution_asserted=true` 与 maturity 正交：它要求对外承诺的 claim 及机械依赖闭包全部通过，但不把 maturity 自动升级；
- “完整 Computer Use”必须精确满足 `COMPUTER_USE.md` §3.3 的固定必需集合、依赖闭包、至少一个 supervision mode 与至少一个 `real-platform` platform claim；禁止以单个 `ALL(...)` 字符串、family 通配符或“相关全部”替代成员记录；
- 若发行物对外展示 Operation Snapshot，必须显式记录 `computer.snapshot`，不能从 scene/capture 等 facet 暗推；
- 仅 `contract-only` 或 mock `provider contract` 证据时，禁止把同一 claim maturity 标为 `real-platform`；
- 撤回 distribution assertion 后，可选套件不再阻塞后续 Core 发布，但已发布产物的变更记录须可审计。

### 20.3 套件关系图

```text
                    ┌──────────────────────────┐
                    │ Core / Extension SDK Base │  ← 始终硬门
                    └────────────┬─────────────┘
                                 │
          ┌──────────────────────┬──────────────────────┐
          │ IF_CLAIM(expr)       │ IF_CLAIM(expr)
          ▼                      ▼
   Computer Use facets     Computer Use platform claims
   (COMPUTER_USE.md)       Hyprland/niri/Win/mac
          │                      │
          └──────────┬───────────┘
                     ▼
          IF_REAL / distribution_asserted
          → 真机成熟度与产品宣传级硬门
```

## 21. 与其他规范的交叉

| 主题 | 事实源 |
|---|---|
| Computer Use 领域合同与双层 Execute Gate | `COMPUTER_USE.md` |
| Extension Profile / probe / Manifest | `EXTENSION_SDK.md` |
| Task / Action Lease / Lock / Stop Fence | `CORE_ARCHITECTURE.md` |
| Freedom-first / Broker / Protected Surface 安全语义 | `SECURITY_PRIVILEGE.md` |
| KCP 与固定 Schema（禁止 Computer Use 倒灌） | `IMPLEMENTATION_CONTRACTS.md` |
