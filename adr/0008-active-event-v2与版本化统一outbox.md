# ADR-0008：Active Event v2 Schema 与版本化统一 Outbox

- 状态：accepted
- 日期：2026-07-19
- 实现状态：第一实现段已完成（八个 Event v2 component-native Schema、manifest 61 entries、schema-tool Event claimant/catalog/bindings、generated Rust typed envelope 与 conformance）；migration 0003、mixed Outbox API、producer、Publisher、server 仍为 contract-only
- 合同开放问题：0；本ADR所列Event v2/Outbox docs决策已全部拍板，剩余项都是实现工作而非合同待定项

## 背景

现有仓库历史上只有 retained `EventEnvelope v1`、`CausationRef v1`、三个事件 payload 与 SQLite v1 Outbox。Event v2第一实现段现已补齐八个Schema、compiler/catalog与generated typed decode，但SQLite仍停留在v1 Outbox。v1 causation 只能表达 `command_request | event`，因此无法诚实表达 Child Task 由 Action 直接产生，也无法在 Action 自身状态事件中避免 self-causation。Approval v2 的 current-head 变化同样尚无producer/repository实现。若在migration 0003前先接入root v2、child materializer或Approval repository，并临时扩展旧Outbox，就会形成两张表、两套position/sequence或nullable causation列不断膨胀的长期债务。

本决策固定generic Core里程碑的Event v2 Schema闭包、Catalog权威和SQLite统一存储合同。第一实现段已经落地Schema/compiler/generated产物；它仍不启用production MethodVersionBinding、active producer或mixed Outbox runtime。

## 决策

### 1. 已落地的恰好八个 component-native Schema

Event v2第一实现段已一次性加入下表八项；不能只保留 Envelope、只保留payload，或用inline平行枚举绕过闭包：

| title | component | kind | compatibility | exact ID | exact source | schema_version_field | 直接 `$ref` |
|---|---|---|---|---|---|---|---|
| `ActionTransitionRefV1` | common | object | new-contract | `https://schemas.shittim.local/common/action_transition_ref/v1` | `schemas/source/common/action_transition_ref.v1.json` | null | 无 |
| `ConfirmationModeV1` | common | enum | new-contract | `https://schemas.shittim.local/common/confirmation_mode/v1` | `schemas/source/common/confirmation_mode.v1.json` | null | 无 |
| `CausationRefV2` | common | object | breaking-replacement | `https://schemas.shittim.local/common/causation_ref/v2` | `schemas/source/common/causation_ref.v2.json` | null | `ActionTransitionRefV1` |
| `ApprovalRecordKindV2` | policy | enum | new-contract | `https://schemas.shittim.local/policy/approval_record_kind/v2` | `schemas/source/policy/approval_record_kind.v2.json` | null | 无 |
| `ApprovalSubjectKindV2` | policy | enum | new-contract | `https://schemas.shittim.local/policy/approval_subject_kind/v2` | `schemas/source/policy/approval_subject_kind.v2.json` | null | 无 |
| `ActionStateChangedPayloadV1` | event | event_payload | new-contract | `https://schemas.shittim.local/event/action_state_changed_payload/v1` | `schemas/source/event/action_state_changed_payload.v1.json` | `schema_version` | retained `ActionStatus` |
| `ApprovalStateChangedPayloadV1` | event | event_payload | new-contract | `https://schemas.shittim.local/event/approval_state_changed_payload/v1` | `schemas/source/event/approval_state_changed_payload.v1.json` | `schema_version` | whole-schema root refs：`ConfirmationModeV1`、`ApprovalRecordKindV2`、`ApprovalSubjectKindV2` |
| `EventEnvelopeV2` | event | envelope | breaking-replacement | `https://schemas.shittim.local/event/event_envelope/v2` | `schemas/source/event/event_envelope.v2.json` | `schema_version` | whole-schema root refs：`CausationRefV2`与五个正式payload |

该切片落地后production manifest现为61 entries（41 retained + 20 component-native）。production `method_version_bindings`继续为空，因为Event Catalog不通过KCP MethodVersionBinding表达。该“仍为空”仅指本Event Schema/Catalog实现段；未来若升级KCP `event.poll` response以承载v2，必须在独立切片为poll新增/升级binding，不能借此句永久保持为空。

`ConfirmationModeV1`归属common，而不是policy。它是PolicyRule、PermissionDecision、Approval request和公共Approval事件共同使用的跨域算法无关词表；放入policy会让所有使用者依赖Approval领域，并阻止common层对象未来安全复用。common继续不引用其它component。event的`allowed_refs`已精确扩为`["common","policy"]`以复用policy拥有的record/subject discriminator；policy仍只引用common。完整DAG为`kcp→event|task|common`、`event→policy|common`、`policy→common`、`task→common`、`audit→common`，不存在反向边或cycle。

ADR涉及的三个enum闭集不在本文件另起版本：`ConfirmationModeV1=generic|local|system_authentication|remote_signature|plan_revision`、`ApprovalRecordKindV2=request|resolution|invalidation`、`ApprovalSubjectKindV2=operation|task_proposal|plan_revision`，权威交叉引用IC §13.6.2。本文“直接/whole-schema `$ref`”统一按IC §13.6.2定义为空fragment且解析到manifest root；inline或fragment ref不算。

### 2. 引用对象的版本不复制为业务字段

`ActionTransitionRefV1`精确字段为`kind:"action_transition"`、`action_id:UUID`、`transition_id:UUID`，全部required且branch封闭。它不携带`schema_version`、expected revision或execution generation：`V1`由Schema identity版本化；revision/generation属于不可变`ActionTransitionIntentV1`，repository通过`transition_id`读取并核对。把revision复制进Ref会产生可漂移双源。本八Schema批次只交付该Ref；Intent固定在后续task component `domain_object` Schema+Action repository/migration批次实现，exact ID/source以IC §6.14为准，producer不得先造临时类型。

`CausationRefV2`是按`kind`判别的四分支封闭联合；source必须至少采用下列语义骨架，不能把四支退化为开放对象或自由字符串：

```json
{
  "type": "object",
  "required": ["kind"],
  "properties": {
    "kind": {
      "type": "string",
      "enum": ["command_request", "event", "action", "action_transition"]
    }
  },
  "oneOf": [
    {
      "type": "object",
      "additionalProperties": false,
      "required": ["kind", "id"],
      "properties": {
        "kind": { "const": "command_request" },
        "id": { "type": "string", "format": "uuid" }
      }
    },
    {
      "type": "object",
      "additionalProperties": false,
      "required": ["kind", "id"],
      "properties": {
        "kind": { "const": "event" },
        "id": { "type": "string", "format": "uuid" }
      }
    },
    {
      "type": "object",
      "additionalProperties": false,
      "required": ["kind", "id"],
      "properties": {
        "kind": { "const": "action" },
        "id": { "type": "string", "format": "uuid" }
      }
    },
    {
      "$ref": "https://schemas.shittim.local/common/action_transition_ref/v1"
    }
  ],
  "unevaluatedProperties": false
}
```

前三支分别是`command_request {kind,id:UUID}`、`event {kind,id:UUID}`、`action {kind,id:UUID}`；第四支必须整支使用上面的whole-schema root `$ref`引用`ActionTransitionRefV1`，不得把其字段inline复制。联合层`kind` enum与四个branch const必须双射；零branch、多branch、未知kind和branch未知字段均拒绝。它同样不带`schema_version`字段，版本由被引用Schema ID决定。

### 3. Active Event Catalog由唯一Envelope权威派生

active五类精确为：

| type | aggregate | payload Schema |
|---|---|---|
| `task.created` | `task` | retained `TaskCreatedPayload v1` |
| `task.state_changed` | `task` | retained `TaskStateChangedPayload v1` |
| `action.state_changed` | `action` | `ActionStateChangedPayload v1` |
| `approval.state_changed` | `approval_chain` | `ApprovalStateChangedPayload v1` |
| `stop_fence.activated` | `stop_fence/global` | retained `StopFenceActivatedPayload v1` |

Envelope版本与payload版本正交；`EventEnvelopeV2`可以合法承载payload v1，不得把payload版本推断为2。

生成目录必须输出以下唯一事实结构，声明顺序严格等于对应Envelope root `type` enum的Schema声明顺序：

```rust
pub struct EventTypeBinding {
    pub event_type: &'static str,
    pub aggregate_type: &'static str,
    pub payload_schema_id: &'static str,
    pub payload_schema_version: u64,
}

pub const EVENT_ACTIVE_BINDINGS: &[EventTypeBinding] = &EVENT_ACTIVE_BINDING_ARRAY;
pub const EVENT_LEGACY_V1_BINDINGS: &[EventTypeBinding] = &EVENT_LEGACY_V1_BINDING_ARRAY;
pub const EVENT_ACTIVE_TYPES: &[&str] = &EVENT_ACTIVE_TYPE_ARRAY;
pub const EVENT_LEGACY_V1_TYPES: &[&str] = &EVENT_LEGACY_V1_TYPE_ARRAY;
```

Rust renderer必须先生成private fixed-size `EVENT_ACTIVE_BINDING_ARRAY`与`EVENT_LEGACY_V1_BINDING_ARRAY`，再以上述public slice constants借用它们；一个通用`const fn project_event_types<const N: usize>(&[EventTypeBinding; N]) -> [&'static str; N]`按索引从同一binding array投影private fixed-size `EVENT_ACTIVE_TYPE_ARRAY`/`EVENT_LEGACY_V1_TYPE_ARRAY`，最后暴露上述`&[&str]` type slices。该形态必须在workspace Rust toolchain上编译测试。event type字符串只能出现在binding初始化中一次；禁止再手写第二份type成员/顺序表。删除当前模糊`EVENT_V1_TYPES`，不保留同义alias。active binding五项依次是`task.created/task/retained TaskCreatedPayload v1`、`task.state_changed/task/retained TaskStateChangedPayload v1`、`action.state_changed/action/ActionStateChangedPayload v1`、`approval.state_changed/approval_chain/ApprovalStateChangedPayload v1`、`stop_fence.activated/stop_fence/retained StopFenceActivatedPayload v1`；legacy binding三项按retained root顺序。`payload_schema_id`必须是manifest exact root ID，`payload_schema_version`取该payload root自己的版本事实，不从Envelope版本推断。

active authority发现仍必须使用IC §5.6的宽松候选、严格exact两阶段谓词：reserved exact ID/title/source任一占用者永远是候选；结构候选只要求`component=event,kind=envelope`、`schema_version.const=2`、非空string `type` enum与至少一个`if(type const)/then`同时出现`aggregate_type/payload`键的mapping-like branch。候选门不调用strict parser、不派生mapping facts，也不要求root `type=object`、`additionalProperties=false`、完整required、whole-schema ref或双射，因此近似伪装都先进入candidate再fail closed。候选必须恰好一个且metadata全部exact；exact root随后要求closed object/required，并消费registry load时每个KCP/Event envelope恰好解析一次、按schema id缓存的strict `EnvelopeConditionalBinding`，硬验五值type enum与五个一一对应whole-root mapping branch。inline payload、fragment `$ref`、缺失/重复/mixed branch、额外mapping或一个branch覆盖多个type全部拒绝。Catalog、target closure与typed生成只读取同一缓存IR；通用IR位于独立`conditional_envelope`模块，`event_catalog`只做authority投影。

### 4. EventEnvelope v2不增加第二套时间或投递字段

`EventEnvelopeV2`精确沿用以下字段并把`schema_version`固定为2：

`event_id,type,schema_version,aggregate_type,aggregate_id,sequence,outbox_position,occurred_at,causation_ref,correlation_id,dedup_key,payload`。

- `event_id`为UUID；
- `sequence`为非负整数，聚合首条已提交事件固定0；
- `outbox_position`为正ASCII十进制字符串；为保持retained v1 cursor/wire兼容，本批Schema接受前导零，repository从INTEGER position重建时必须输出无前导零普通十进制；未来若收紧须版本化，不能在0003暗改v1接受集；
- `occurred_at`是Kernel权威业务发生时间；不新增`emitted_at`，Publisher发布时间由Audit或transport观测，不是Envelope事实；
- `delivered_at`只属于Outbox delivery metadata，不进入Envelope；
- `correlation_id`与`dedup_key`是non-empty opaque string；
- `payload`先要求正整数`schema_version`，再由type conditional整支替换为exact payload Schema。

v2 source必须完整闭合五个conditional mapping。Schema可固定type→aggregate type、stop fence aggregate id和payload shape；aggregate ID与payload ID相等、跨对象revision/transition/head关系仍由producer/repository验证。

### 5. 新payload是完整快照，不是自由details

`ActionStateChangedPayloadV1`全部required：`schema_version,action_id,task_id,from_status,to_status,action_revision,execution_generation,permission_decision_ref,approval_resolution_ref,materialized_child_task_ref,verification_result_refs,reason_code,changed_at`。三个单值ref required-nullable；verification数组required且可空、成员UUID且不得重复；revision最小1，generation最小0，reason非空。Schema至少表达：`to_status=completed|failed`时verification非空；`materialized_child_task_ref`非null时`to_status=completed`且verification非空；`approval_resolution_ref`非null时`permission_decision_ref`非null。合法Action边、from≠to、revision恰增1、generation/current Lease、PD/Approval可消费、child operation身份及payload与commit后Action逐字段相等由repository验证。

`ApprovalStateChangedPayloadV1`全部required，并新增显式`change_kind`判别字段，闭集为`initial_request|resolution|invalidation_without_replacement|replacement_request`。它消除initial与replacement同为`to_record_kind=request`的歧义；不得再靠from/to/null组合猜事件种类。其余字段为`schema_version,approval_chain_id,from_head_ref,to_head_ref,from_record_kind,to_record_kind,subject_kind,confirmation_mode,request_ref,resolution_ref,invalidation_ref,replacement_request_ref,permission_decision_ref,action_id,reason_code,changed_at`。Schema按IC §5.6完整真值表固定四类的from/to kind与每个head/request/resolution/invalidation/replacement ref的required-null/non-null；ID exact equality、整链canonical subject、mode投影、operation subject的PD/Action绑定及old/new head真实关系由repository验证。replacement只形成一个`old head→replacement request`事件，中间invalidation record只通过`invalidation_ref`携带。

### 6. Producer authority与因果图

Event payload不是caller可写事实。caller只可提供版本化allocation中的event ID、correlation、dedup、业务时间和真实外部causation；拥有业务聚合的repository必须从同事务canonical records投影type、aggregate、sequence和payload。

所有causation target必须在新事件提交前已存在并可验证，禁止future placeholder。`event`不得引用自身event ID；新Event只能引用既有已提交Event，因此Event→Event边天然无环。`action`用于Action产生其它aggregate事实，不能用于该Action自身`action.state_changed`。Action自身事件必须引用已持久化的transition intent，且Ref action ID等于Envelope aggregate/payload action ID、transition ID与event/action ID不同。Approval事件不得以自身、同chain Approval event或future head为原因。发现direct self-reference、不可解析target、跨对象闭包不一致或因果循环时整笔业务事务失败。

### 7. migration 0003使用一张版本化Outbox

后续`kernel-sqlite` migration 0003必须把现有v1与active v2记录放进同一`outbox`表，共享唯一`outbox_position`和`aggregate_event_sequences`。禁止`outbox_v2`双表、按版本各自position或各自aggregate sequence。

现有ledger事实是`schema_migrations(version,name,checksum,applied_at)`，其中checksum只覆盖SQL bytes。0003必须在同一`BEGIN IMMEDIATE`事务开始阶段先执行ledger升级DDL，为表增加`descriptor_hash TEXT`（升级后对新行必须64位lowercase hex；历史0001/0002允许null）与`descriptor_format_version INTEGER`（历史允许null，新行固定1）；随后执行Outbox transform，最后以新列写0003 ledger row。不能依赖一个先行已提交的“ledger-only 0003”再用同version执行transform，因为中途状态无法表示；也不能把升级改名成0004来绕过本合同。

每个binary migration必须有稳定descriptor bytes。format v1固定为UTF-8、LF、无BOM、末尾单LF的JSON JCS对象：`{"descriptor_format_version":1,"migration_version":N,"name":"...","sql_assets":[{"path":"rust/crates/kernel-sqlite/migrations/NNNN_name.sql","sha256":"..."},...],"transform":{"algorithm_id":"<stable transform algorithm id>","version":V,"implementation_id":"<stable closed Rust implementation id>"}|null}`。`algorithm_id`必须标识一个具体、稳定、可执行语义的transform算法，不能写migration transform族名、dispatcher名或“Rust transform”泛称；`version`版本化该算法；`implementation_id`标识实现该算法版本的稳定闭集实现。三元组`algorithm_id/version/implementation_id`共同唯一确定transform，任一变化都构成descriptor drift。`sql_assets`按path UTF-8升序且hash原始文件bytes；transform字段不可使用Rust函数地址、crate版本、Git SHA或构建时间等不稳定值。`descriptor_hash=SHA-256(JCS descriptor bytes)` lowercase hex；`checksum`重新定义为同一descriptor hash，二者对新行必须exact相等，保留`descriptor_hash`列是为了让ledger显式标注新语义并与legacy SQL-only行区分。0001/0002继续按旧算法验证`checksum=SHA-256(sql bytes)`且descriptor两列null，保证旧库兼容；0003及以后只按descriptor验证。代码升级若保留同一migration version/name但SQL asset、algorithm/version/implementation identity任一变化，hash必变并返回`migration_drift`，不得重跑或接受。

0003的descriptor identity完整固定为：`migration_version=3`、`name="versioned_event_outbox"`、且`sql_assets`恰有一个元素，path精确为`rust/crates/kernel-sqlite/migrations/0003_versioned_event_outbox.sql`。该单一asset包含本migration全部SQL DDL：ledger列升级、replacement `outbox`表定义、全部CHECK/UNIQUE约束、索引、表替换所需DDL；不得把其中一部分藏进第二asset或未受descriptor覆盖的自由SQL。Rust transform与该asset在同一个0003 `BEGIN IMMEDIATE` transaction中按实现定义的阶段执行，负责legacy row exact decode、JCS causation转换、参数化copy、readback/sequence/position验证及原子替换编排；没有独立提交边界。

0003完整descriptor在将asset真实SHA-256代入后必须等价于下列JCS对象：`{"descriptor_format_version":1,"migration_version":3,"name":"versioned_event_outbox","sql_assets":[{"path":"rust/crates/kernel-sqlite/migrations/0003_versioned_event_outbox.sql","sha256":"<sha256 of exact raw asset bytes>"}],"transform":{"algorithm_id":"shittim.kernel-sqlite.outbox-v1-to-versioned-v1","version":1,"implementation_id":"kernel_sqlite::migration::outbox_v1_to_versioned_v1"}}`。该algorithm是具体v1 Outbox→versioned format-v1 transform，不是算法族；此三元组只表示0003这一exact transform。未知`descriptor_format_version`、未知transform三元组、历史行出现半填descriptor、或新行checksum/hash不等均`migration_drift`；数据库version高于binary仍优先`database_schema_too_new`。rollback会回滚ledger ALTER、replacement table、copy和0003 row；rollback自身失败则store unhealthy。成功后不支持down-migration，只能恢复迁移前验证备份。

post-0003规范列保留现有position/event/type/envelope version/aggregate/sequence/time/correlation/dedup/payload/delivery事实；把`causation_kind + causation_id`替换为单列canonical `causation_json`。不得为四个branch增加一组nullable列。`payload_json`与`causation_json`都必须是RFC 8785 JCS；不保存`envelope_json`双源。DB CHECK至少固定`schema_version IN(1,2)`、schema_version×event_type允许矩阵和五类event→aggregate映射：`task.created|task.state_changed→task`、`action.state_changed→action`、`approval.state_changed→approval_chain`、`stop_fence.activated→stop_fence/global`；`payload_json/causation_json`只做`json_valid`且root object。payload版本与branch shape/JCS/UUID/date-time仍由应用层exact payload/Envelope Schema和canonical byte equality权威，DB不冒充完整Schema。读取按row `schema_version`选择exact EventEnvelope v1或v2 Schema和typed decoder，再从规范化列重建Envelope；unknown version/type、non-canonical JSON、mapping不闭合或relation mismatch返回`stored_data_invalid`，不推进cursor、不mark delivered。

0003必须是Rust-assisted、checksum保护的单个`BEGIN IMMEDIATE` migration：在同一transaction创建replacement table，按position读取并完整验证每个legacy v1 row，用共享JCS authority构造`CausationRefV1` canonical JSON，显式保留原position/sequence/event/delivery/dedup/payload，验证row count、逐row canonical readback、每aggregate `0..last_sequence`闭包与sequence table一致、`MAX(position)`及下一AUTOINCREMENT位置，然后原子替换表和索引并写ledger。禁止把SQLite `json_object`输出假定为JCS。

任一row损坏、descriptor/checksum drift、copy/readback/count/sequence失败或DDL失败都回滚整个0003，不写ledger、不留下半表；rollback自身失败则store unhealthy并fail closed。migration成功后旧binary必须以`database_schema_too_new`健康拒绝，禁止自动down-migration；版本回滚只能恢复迁移前已验证备份，因为v2事件无法无损降回v1。

### 8. 读写API与savepoint

统一私有append内核负责exact prevalidation、SAVEPOINT、聚合sequence分配、单表insert、position分配、最终Envelope重建/Schema/typed decode和失败rollback。公开API形态固定如下，不留实现者发明空间：

```text
pub enum StoredEventEnvelope {
    LegacyV1(TypedEventEnvelope),
    ActiveV2(TypedEventEnvelopeV2),
}

pub struct OutboxRecord {
    pub envelope: StoredEventEnvelope,
    pub delivered_at: Option<DateTime<Utc>>,
}

pub struct PendingLegacyEventV1 {
    pub event_id: String,
    pub event_type: EventEnvelopeType,
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub occurred_at: DateTime<Utc>,
    pub causation_ref: CausationRef,
    pub correlation_id: String,
    pub dedup_key: String,
    pub payload: serde_json::Value,
}

pub enum EventAggregateId {
    Task(uuid::Uuid),
    Action(uuid::Uuid),
    ApprovalChain(uuid::Uuid),
    StopFenceGlobal,
}

pub struct PendingActiveEventV2 {
    pub event_id: uuid::Uuid,
    pub aggregate_id: EventAggregateId,
    pub occurred_at: DateTime<Utc>,
    pub causation_ref: CausationRefV2,
    pub correlation_id: String,
    pub dedup_key: String,
    pub payload: EventEnvelopeV2Payload,
}

impl WriteTransaction {
    pub fn append_legacy_event_v1(PendingLegacyEventV1) -> Result<OutboxRecord, StoreError>;
    pub fn append_active_event_v2(PendingActiveEventV2) -> Result<OutboxRecord, StoreError>;
}

impl SqliteStore {
    pub fn read_after(...) -> Result<Vec<OutboxRecord>, StoreError>;
    pub fn read_undelivered(...) -> Result<Vec<OutboxRecord>, StoreError>;
    pub fn latest_position(...) -> Result<Option<OutboxPosition>, StoreError>;
    pub fn mark_delivered(...) -> Result<MarkDeliveredResult, StoreError>;
}
```

上述Outbox public types由`rust/crates/kernel-sqlite/src/outbox.rs`拥有并从`kernel-sqlite` crate root公开re-export；不得移入业务owner crate或建立另一套同义类型。`EventEnvelopeType`、`CausationRef`、`CausationRefV2`、`EventEnvelopeV2Payload`、`TypedEventEnvelope`与`TypedEventEnvelopeV2`都来自`kernel-contracts` generated public API；`EventEnvelopeV2Payload`是按EventEnvelopeV2 mapping生成的closed tagged variant，variant顺序与`EVENT_ACTIVE_BINDINGS`一致。

`PendingLegacyEventV1`逐字段维持现有`PendingEvent`合同：legacy caller仍显式给出type/aggregate与JSON payload，但append必须用retained v1 Schema+typed decode验证其type→aggregate→payload映射；`event_id`继续由Schema验证UUID。`PendingActiveEventV2`不得接受caller可写`event_type`或`aggregate_type`：store从`EventEnvelopeV2Payload` variant经`EVENT_ACTIVE_BINDINGS`唯一派生二者。`EventAggregateId`拒绝自由String：`TaskCreated/TaskStateChanged`只匹配`Task(Uuid)`，`ActionStateChanged`只匹配`Action(Uuid)`，`ApprovalStateChanged`只匹配`ApprovalChain(Uuid)`，`StopFenceActivated`只匹配`StopFenceGlobal`；前三类还必须核对enum内UUID等于payload对应task/action/approval_chain ID。任一variant/aggregate ID不匹配在sequence/position分配前失败。`correlation_id`与`dedup_key`必须非空；`event_id`由上层allocation传入，store不生成。两个pending struct都不暴露caller可写`schema_version/sequence/outbox_position`；payload自身版本只来自generated payload variant。

私有`append_versioned_event`是唯一共享内核，不导出；它从legacy validated事实或active binding事实构造内部规范化输入，再负责exact prevalidation、SAVEPOINT、sequence/position分配、insert、最终Envelope重建与版本对应的Schema/typed decode。现有`PendingEvent`/`append_event`在0003 API切片中重命名，不保留deprecated alias；legacy入口仅供现有未发布compatibility producer并受最终cutover write gate关闭。不得通过Schema失败后fallback另一版本。mixed read固定返回上述variant；Publisher按同一position流处理v1/v2。`mark_delivered`在同一写事务先验证目标row仍可按其版本完整读取，再做第一次时间不可覆盖的conditional update；corrupt row不能被mark后隐藏。两种append失败都由局部SAVEPOINT回滚，外层事务即使继续commit也不占sequence/position、不留partial row。

## 拒绝的替代方案

- **新增独立v2 Outbox表**：拒绝；会产生两个cursor、两个position和跨表排序竞态。
- **在旧表追加action_id/transition_id等nullable列**：拒绝；每新增causation branch都扩列，并允许不一致组合。
- **只存完整Envelope JSON**：拒绝；会丢失稳定索引或与投影列形成双源。
- **Catalog按文件名/ID suffix发现**：拒绝；无法证明manifest身份与Envelope实际mapping一致。
- **EventEnvelope v2强制payload v2**：拒绝；Envelope与业务payload版本正交。
- **继续使用`EVENT_V1_TYPES`表示active集合**：拒绝；名称与事实冲突。
- **自动把v2 row降解为v1给旧consumer**：拒绝；会丢失Action/Approval causation与payload事实。

## 实现顺序与验收

1. 先实现八Schema、manifest DAG、exact claimant、active/legacy catalog常量和v1/v2 typed decode；production bindings仍空。
2. 再实现migration 0003与统一Outbox mixed read/write；此时仍不实现active业务producer。
3. 后续root v2、child、Action transition、Approval repository只能消费上述active入口；不得各自创建临时Event存储。
4. Conformance必须覆盖Schema正反例、claimant/mapping drift、mixed migration、JCS、sequence/position、SAVEPOINT、corruption、rollback/backup与legacy write gate。

本ADR的合同与第一实现段已经完成：当前仓库有61 Schema，Event v2八Schema、active/legacy Catalog与v1/v2 typed decode已落地；SQLite仍是legacy EventEnvelope v1 Outbox。尚无migration 0003、mixed API、active producer、Publisher、repository、handler或server。
