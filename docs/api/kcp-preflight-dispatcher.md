# KCP Value preflight 与注册式 dispatcher

> 状态：切片3b 起 `kernel-kcp` 实现 method-aware active preflight/dispatcher/handler。production `METHOD_VERSION_BINDINGS` / `select_request_version` 被 runtime 消费；`task.create` 仅 active v2 进入 registration，v1 create production 请求返回 `unsupported_schema_version`。五方法仍无正式 handler，server 阶段门保持关闭；§13.7 未闭合。

## 范围

本边界只接收调用方已经解析得到的 `serde_json::Value`。它不负责 bytes、UTF-8、JSON parse、4-byte length prefix frame、最大 frame、transport、连接身份、clock、backend 或 Kernel ID。实现位于现有 `kernel-kcp`，复用 handlers、ports、generated Catalog 与 response contract 门，没有建立平行 crate abstraction。

公开调用链固定分三步：

```rust
preflight_value(value)
-> narrow_to_registered(request)
-> TypedDispatcher::new(clock, ids, backend).dispatch(request)
```

没有 `process_value` 或语义等价的一站式全 Catalog API。首批八方法都有正式 Schema/Catalog，但目前只有三个方法拥有 typed handler。

## Public API

- `preflight_value(Value) -> PreflightResult`
- `TypedCatalogRequest::family()` / `method()`：只读查看已经确认的 family 与 generated discriminator；内部 envelope variant 私有，只能由 preflight 构造。
- `narrow_to_registered(TypedCatalogRequest) -> RegistrationResult`
- `RegisteredRequest::method()`：只读查看 registered method；内部 variant 私有，只能由 narrow 构造。
- `TypedDispatcher<'a, C, G, B>::new(&C, &G, &B)` / `dispatch(RegisteredRequest)`：借用现有 `KernelClock`、`KernelIdGenerator`、`TaskApplicationBackend`。
- `TaskCreateCommandRequestV2`：active root-only create 的已解码请求（public，供 handler/测试使用）；不是泛型 `TypedKcpCommandEnvelopeV2`。

`PreflightLocalRejection`、`KnownCatalogMethodNotImplemented`、`TypedCatalogRequest` 与 `RegisteredRequest` 均不实现 `Serialize`；测试使用负 trait assertion 锚定。生产 API 不接收 validator/catalog/bypass flag，response fault seam 只存在于 crate 私有单元测试。

## Preflight 结果与优先级

`preflight_value` 只产生：

- `Accepted(TypedCatalogRequest)`：V2 Envelope 结构 + active method/version payload Schema 与 typed decode 已通过；
- `Response(KcpResponseEnvelope)`：request ID 可关联的固定 preflight wire error，且最终 response 已通过 generated Response Schema；
- `LocalRejection(PreflightLocalRejection)`：不能关联 request，或 catalog/schema/generated/response 出现内部合同失败；不得发送 wire response。

固定短路优先级：

1. request ID response eligibility；
2. message kind / family；
3. protocol；
4. auth；
5. family-specific method（`KCP_ENVELOPE_AUTHORITY_*`）；
6. 根 `payload.schema_version` 形状 + `select_request_version`（Active 继续；LegacyValidationOnly / Unsupported → `unsupported_schema_version`）；
7. V2 Command/Query Envelope Schema + 所选 active request Schema + typed decode。

顶层 `request_id` 必须是 UUID parser 接受的 string；wire error 中逐字保留原字符串，不重新格式化。非 object、缺失/非 string/非法 UUID 都得到 `UncorrelatableRequest`。完整 Schema 阶段还要求 V2 Envelope 的 lowercase UUID pattern。

method-aware 版本矩阵：

| family | method | active | legacy validation | production 行为 |
|---|---|---|---|---|
| command | `task.create` | `[2]` | `[1]` | v2 Accepted；v1 → `unsupported_schema_version` |
| command | `stop.activate` | `[1]` | `[]` | v1 Accepted → KnownCatalogMethodNotImplemented |
| query | 其余六方法 | `[1]` | `[]` | v1 Accepted；`system.ping`/`task.get` Registered |

preflight 的 `unsupported_auth_schema` 只在实际执行 auth 判定时适用。它不能被 root official fixture harness 借用。

## 结构化 contract error

`kernel-contracts` 公开：

- `ContractFailureStage`：`CallerSchemaValidation`、`WireDecodeAfterSchema`、`PayloadDecodeAfterSchema`、`GeneratedDiscriminatorMapping`、`SchemaCatalog`；
- `ContractFailureClassification`：`CallerInvalid` / `InternalContractFailure`；
- `ContractError::stage()` 与 `classification_for_preflight()`。

preflight 先单独 `validate_json`（Envelope 再 payload）；只有 `SchemaValidation` 是 caller invalid，后续任何 decode/catalog 失败都本地 fail closed，不匹配错误文本。

## 固定错误

| code | 固定 message | details | retryable |
|---|---|---|---:|
| `invalid_request` | `request is invalid` | null | false |
| `unsupported_protocol_version` | `protocol version is not supported` | null | false |
| `unsupported_schema_version` | `payload schema version is not supported` | null | false |
| `unsupported_method` | `method is not supported` | null | false |
| `unsupported_auth_schema` | `authentication schema is not supported` | null | false |

response 构造复用 `kernel-kcp` crate-private 通用 validated error finalizer。最终 error response 若不能通过 generated Response Schema，则返回固定本地 `ContractFailure`，不发送未验证 response。

## Registration 与 dispatcher

全部八方法合法 **active** 请求都先得到 typed `Accepted`。narrow 结果：

| method | registration |
|---|---|
| `system.ping` | Registered → handler |
| `task.create`（v2 only） | Registered → root-only v2 handler |
| `task.get` | Registered → handler |
| `task.list` / `event.subscribe` / `event.poll` / `stop.activate` / `stop.status` | `KnownCatalogMethodNotImplemented` |

`KnownCatalogMethodNotImplemented` 不是 wire error，不得序列化，也不得伪装成 `unsupported_method` / `method_unavailable`。组合根在五方法拥有正式 handler 前必须 fail closed 且禁止启动 server。`InternalContractViolation`（当前唯一值 `TaskCreateV1AfterActivePreflight`）是主动 method-aware preflight 不可达的内部合同失败身份：typed `task.create` v1 只可能经构造旁路到达 narrowing；同样不得序列化，不得冒充任何 Catalog 方法或 wire error。

`TypedDispatcher` 是显式三方法注册表：ping 用 clock，create 用 clock/ids/backend，get 用 clock/backend；无损保留 `post_commit_notification_intents`。
