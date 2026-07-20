# Event Catalog

> 状态：**Event v2 Schema/source/generated/catalog/typed decode 与SQLite migration 0003/mixed Outbox API已实现**；active business producer、Publisher与KCP poll mixed cutover仍未实现。唯一事实源是[`IMPLEMENTATION_CONTRACTS.md` §5.6](../../specs/IMPLEMENTATION_CONTRACTS.md#56-首批正式-event-catalog)、§6.14–6.15、§13.6.2与[ADR-0008](../../adr/0008-active-event-v2与版本化统一outbox.md)。

## Schema闭包（已落地）

已一次加入八项：

- common：`ActionTransitionRefV1`、`ConfirmationModeV1`、`CausationRefV2`；
- policy：`ApprovalRecordKindV2`、`ApprovalSubjectKindV2`；
- event：`ActionStateChangedPayloadV1`、`ApprovalStateChangedPayloadV1`、`EventEnvelopeV2`。

`ConfirmationModeV1`归common，因为它被Policy、PermissionDecision、Approval和公共事件共同复用；record/subject kind仍归policy。event `allowed_refs=[common,policy]`，形成`event→policy→common`的无环DAG，不复制平行枚举。精确ID/source/kind/version/compatibility见IC §13.6.2。

## CausationRef v2

source最小语义骨架固定为closed `oneOf` whole-ref联合：union层required `kind` enum=`command_request|event|action|action_transition`；前三个branch各为`additionalProperties:false`的`{kind:<const>,id:UUID}`对象；第四branch整支使用`$ref:"https://schemas.shittim.local/common/action_transition_ref/v1"`，union以`unevaluatedProperties:false`闭合。不得inline复制action transition字段或使用fragment ref。

- `command_request{kind,id:UUID}`：command直接产生事实。
- `event{kind,id:UUID}`：既有已提交event触发事实。
- `action{kind,id:UUID}`：Action产生其它aggregate事实，例如child `task.created`。
- `ActionTransitionRefV1 {kind:"action_transition",action_id:UUID,transition_id:UUID}`：仅Action自身状态event；CausationRef branch整支直接引用该正式wire对象。

Ref不复制revision/generation；repository通过transition ID读取不可变intent并验证。event self-ID、future cause、Action self-causation、同chain Approval event因果与不可解析/cyclic relation均失败。

## 首批active Catalog

| type | aggregate | payload | 直接因果边界 |
|---|---|---|---|
| `task.created` | task / Task ID | retained `TaskCreatedPayload v1` | root=command；child=Action |
| `task.state_changed` | task / Task ID | retained `TaskStateChangedPayload v1` | 真实command/event/action |
| `action.state_changed` | action / Action ID | `ActionStateChangedPayload v1` | 必须是ActionTransitionRefV1 |
| `approval.state_changed` | approval_chain / chain ID | `ApprovalStateChangedPayload v1` | 真实外部command/event/action |
| `stop_fence.activated` | stop_fence / `global` | retained `StopFenceActivatedPayload v1` | 激活调用真实来源 |

Envelope版本与payload版本正交；EventEnvelope v2正式承载payload v1。

### Action payload

全部字段required；PD/Approval/child refs为required-nullable，verification refs为required数组。Schema表达completed/failed verification、child result只属于completed、Approval ref要求PD ref等单记录条件；合法状态边、revision恰增1、Lease generation、PD/Approval current关系与commit后Action一致由repository验证。

### Approval payload

全部字段required；不适用ref与`from_record_kind`使用显式null。新增required `change_kind`，闭集精确为`initial_request|resolution|invalidation_without_replacement|replacement_request`，因此initial与replacement即使都`to_record_kind=request`也不会歧义。四类的完整from/to kind及`from_head_ref/to_head_ref/request_ref/resolution_ref/invalidation_ref/replacement_request_ref` null/non-null真值表以IC §5.6为权威，Schema必须逐branch表达；repository再验证字符串ID exact equality、canonical subject、confirmation mode及PD/Action绑定。replacement直接表达`old head→replacement request`，同时携带invalidation ref，不发布中间稳定head。

## Catalog权威与typed envelope

active集合只能由manifest中的reserved identity/structure候选闭包选出：exact ID/title/source任一占用者永远是candidate；结构候选只宽松要求`component=event,kind=envelope`、`schema_version.const=2`、非空string `type` enum及至少一个`if.properties.type.const` + `then.properties`含`aggregate_type/payload`的mapping-like branch。宽松门不调用strict parser、不产生mapping facts，也不要求root `type=object`、`additionalProperties=false`、完整required、whole-root ref或双射；所以缺失/true additionalProperties、错误root type、缺required与partial branch近似伪装都会进入candidate并fail closed。候选必须恰好一个且metadata exact；随后exact root与registry load时缓存的strict `EnvelopeConditionalBinding`再要求closed root、required、五值enum/branch双射与whole-root refs。普通Schema不得误抓，禁止按suffix或文件名发现。

generated Rust API固定：

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

private fixed-size binding arrays分别按v2/v1 Envelope root type enum声明顺序；TYPE arrays必须由对应bindings通过通用`const fn project_event_types<const N: usize>`投影，再暴露上述slices，event type字符串不得再维护第二份。该API必须有真实Rust编译测试。不得保留模糊`EVENT_V1_TYPES` alias。typed decode必须显式区分Envelope v1/v2，并按type选择exact payload Schema；未知type/version或payload mismatch不推进cursor。

## 统一版本化Outbox

migration 0003已实现为一张`outbox`表：v1/v2共享全局`outbox_position`与aggregate sequence。causation使用RFC 8785 canonical `causation_json`，payload使用canonical `payload_json`；不扩张nullable branch列、不保存`envelope_json`双源。ledger已在0003同一事务从四列升级为带nullable历史/新行必填的`descriptor_hash`与`descriptor_format_version`。

format-v1 descriptor与0003 identity按合同落地：exact version/name/single raw asset及transform三元组；single descriptor hash同时写checksum/hash，0001/0002保留legacy SQL-only checksum。transform逐legacy row exact decode、JCS causation copy、逐rowreadback、row count/sequence/position/sqlite_sequence闭包和原子table swap；corruption或DDL/descriptor drift整体rollback。

公开API固定为`StoredEventEnvelope::{LegacyV1(TypedEventEnvelope),ActiveV2(TypedEventEnvelopeV2)}`与`OutboxRecord { envelope, delivered_at }`；类型owner固定`kernel-sqlite/src/outbox.rs`并由crate root re-export。写入固定为`WriteTransaction::append_legacy_event_v1(PendingLegacyEventV1)`和`append_active_event_v2(PendingActiveEventV2)`，共用私有`append_versioned_event`。legacy pending逐字段维持现有PendingEvent caller事实并做v1 mapping验证；active pending精确使用`event_id:Uuid`、`EventAggregateId::{Task,Action,ApprovalChain,StopFenceGlobal}`、`occurred_at`、generated `CausationRefV2`、nonempty correlation/dedup及generated closed `EventEnvelopeV2Payload`，不接受caller event_type/aggregate_type/schema_version/sequence/position。store从payload variant+generated binding派生type/aggregate并核对typed aggregate ID与payload ID。旧`PendingEvent/append_event`不保留alias。读取按row Envelope version选择exact Schema/typed decoder；unknown/corrupt version/type/JCS/mapping返回`stored_data_invalid`且不推进cursor。`mark_delivered`写前必须验证目标row仍可完整读取，不能用delivery mark隐藏corruption。

## KCP `event.poll`版本边界

retained `EventPollResponse v1`的`events.items` exact引用EventEnvelope v1，所以它绝不返回v2。统一Outbox的mixed read是存储/Publisher能力，不等于KCP poll已升级；v1 poll遇到下一条v2不得跳过、降解或推进cursor，且在未来新增/升级versioned response Schema、MethodVersionBinding、typed handler与Conformance之前不得作为可连接handler启用。

## Consumer gate

消费者按`outbox_position`严格递增读取，先选择Envelope版本，再验证type→aggregate→payload、causation、canonical JSON及repository事实。未知version/type、payload mismatch或corrupt relation均fail closed且不推进cursor。`delivered_at`只表示Publisher已发布，不表示任一订阅者已经处理。
