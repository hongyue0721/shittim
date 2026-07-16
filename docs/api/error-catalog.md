# Error Catalog

> 状态：仅规范，未实现。机器错误码的权威定义见 [`IMPLEMENTATION_CONTRACTS.md` 第 5.7 节](../../specs/IMPLEMENTATION_CONTRACTS.md#57-首批错误目录) 及各 KCP 方法条目。

## 通用错误

| code | 含义 |
|---|---|
| `invalid_request` | Envelope 或 payload 不满足 Schema |
| `unsupported_protocol_version` | KCP protocol 不支持 |
| `unsupported_schema_version` | payload/object schema 不支持 |
| `unsupported_method` | 方法不在当前 KCP Catalog |
| `unsupported_auth_schema` | v1 收到非 null auth |
| `deadline_exceeded` | 请求开始前已过期或处理期间超过 deadline |
| `revision_conflict` | expected revision 与当前事实不一致 |
| `idempotency_conflict` | 同 scope/key 对应不同 canonical request |
| `stop_fence_active` | Stop Fence 在执行边界拦截创建或推进新副作用；不是普通 PolicyRule deny |
| `unsupported_policy_condition` | 未知或未实现 Policy condition，fail closed |
| `internal_error` | 未分类 Kernel 错误，不泄漏 Secret |

## 方法专属错误

规范还定义 Task/Scope/Delegation/Origin/Subscription/Cursor/Event type 等 not-found 或 unsupported 错误。实现必须直接依据方法 Schema 和规范生成文档，不得由客户端自行猜测。

## 客户端规则

- 只根据响应中的 `retryable` 判断是否建议重试；
- `deadline_exceeded` 后不得假设命令完全未生效，需按方法幂等与查询语义恢复；
- `unsupported_policy_condition` 不是“无规则命中”，不得转换成 Default Allow；
- `stop_fence_active` 期间已存在的 pending Action 保持 pending；第一版没有 Fence 解除 API，切回 Normal 也不能暗中解除；
- `stop.activate` 同时触发规范定义的 Emergency Stop 副作用集；
- `internal_error` 不应展示内部 Secret、Token、完整敏感 payload 或堆栈。
