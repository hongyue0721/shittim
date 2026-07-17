# Event Catalog

> 状态：legacy EventEnvelope v1/SQLite Outbox已实现；active EventEnvelope/CausationRef v2以及`action.state_changed`、`approval.state_changed`仍为contract-only。字段与producer唯一事实源是 [`IMPLEMENTATION_CONTRACTS.md` §5.6](../../specs/IMPLEMENTATION_CONTRACTS.md#56-首批正式-event-catalog) 和 §6.14–6.16；本页只导航。

## CausationRef v2

- `command_request{id}`：command直接产生事实。
- `event{id}`：已提交event触发事实。
- `action{id}`：Action产生其它aggregate事实，例如child `task.created`。
- `ActionTransitionRefV1 {kind:"action_transition",action_id,transition_id}`：仅Action自身状态event；CausationRef branch直接`$ref`该正式wire对象。其权威定义（连同Intent、CAS、replay与reconciliation）在IC §6.14，不在Audit合同重复定义。

## 首批active Catalog

| type | aggregate | 摘要 |
|---|---|---|
| `task.created` | task / Task ID | root由command，child由Action产生 |
| `task.state_changed` | task / Task ID | 合法Task transition |
| `action.state_changed` | action / Action ID | 使用ActionTransitionRefV1；payload绑定PD/Approval/Verification/result |
| `approval.state_changed` | approval_chain / chain ID | 每次current-head成功变化恰好一条；atomic replacement无中间head event |
| `stop_fence.activated` | stop_fence / `global` | generation激活事实 |

### Approval可观察性

初始request、resolution、invalidation、invalidation+replacement均由Approval repository固定生产，并各自消费正式`ApprovalEventAllocationV1`。事件payload携带from/to head与record kind、subject kind、canonical confirmation mode、request/resolution/invalidation/replacement refs、PD/Action、reason/time。CAS loser或replay不新增event；Challenge expired只写identity/security Audit，绝不作为Approval head事件。

## Consumer gate

消费者必须按type/version选择payload Schema，校验aggregate、sequence、causation、canonical payload与repository事实；未知type/version或payload mismatch fail closed且不推进cursor。`outbox_position`只表示全局投递顺序，`delivered_at`只表示Publisher已发布。
