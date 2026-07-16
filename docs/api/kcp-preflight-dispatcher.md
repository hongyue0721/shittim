# KCP Value preflight 与注册式 dispatcher

> 状态：已实现不可连接的 Rust 库级边界。唯一事实源是 [`IMPLEMENTATION_CONTRACTS.md` §5.11](../../specs/IMPLEMENTATION_CONTRACTS.md#511-serde_jsonvalue-preflight-与三方法注册式-dispatcher)，自动化矩阵见 [`CONFORMANCE.md` §5](../../specs/CONFORMANCE.md#5-kernel-control-protocolschema-与事件)。

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

`PreflightLocalRejection`、`KnownCatalogMethodNotImplemented`、`TypedCatalogRequest` 与 `RegisteredRequest` 均不实现 `Serialize`；测试使用负 trait assertion 锚定。生产 API 不接收 validator/catalog/bypass flag，response fault seam 只存在于 crate 私有单元测试。

## Preflight 结果与优先级

`preflight_value` 只产生：

- `Accepted(TypedCatalogRequest)`：完整 generated Envelope/方法 Schema 与 `decode_after_validation` 已通过；
- `Response(KcpResponseEnvelope)`：request ID 可关联的固定 preflight wire error，且最终 response 已通过 generated Response Schema；
- `LocalRejection(PreflightLocalRejection)`：不能关联 request，或 catalog/schema/generated/response 出现内部合同失败；不得发送 wire response。

固定短路优先级：

1. request ID response eligibility；
2. message kind / family；
3. protocol；
4. auth；
5. family-specific method；
6. 根 `payload.schema_version`；
7. 完整 Envelope/方法 Schema + generated typed decode。

顶层 `request_id` 必须是 UUID parser 接受的 string；wire error 中逐字保留原字符串，不重新格式化。非 object、缺失/非 string/非法 UUID 都得到 `UncorrelatableRequest`。

method 判定直接使用 generated `KCP_COMMAND_METHODS` / `KCP_QUERY_METHODS`。跨 family 名称是 `unsupported_method`。根 payload version 只接受 JSON i64/u64 正整数形态的 `1`；`1.0`、非正数、超出 i64/u64 的数值是 `invalid_request`，其它正整数是 `unsupported_schema_version`。嵌套 version 与业务字段错误仍是 `invalid_request`。

## 结构化 contract error

`kernel-contracts` 公开：

- `ContractFailureStage`：`CallerSchemaValidation`、`WireDecodeAfterSchema`、`PayloadDecodeAfterSchema`、`GeneratedDiscriminatorMapping`、`SchemaCatalog`；
- `ContractFailureClassification`：`CallerInvalid` / `InternalContractFailure`；
- `ContractError::stage()` 与 `classification_for_preflight()`。

schema-tool 生成的 typed decoder 现在先由 `decode` 验证 Schema，再调用公开且有明确前置条件的 `decode_after_validation`。generated raw wire、payload 与 discriminator default 分别返回独立结构化变体。preflight 先单独 `validate_json`；只有 `SchemaValidation` 是 caller invalid，后续任何 decode/catalog 失败都本地 fail closed，不匹配错误文本。

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

全部八方法合法请求都先得到 typed `Accepted`。narrow 结果：

- registered：`system.ping`、`task.create`、`task.get`；
- known-unimplemented：`task.list`、`event.subscribe`、`event.poll`、`stop.activate`、`stop.status`。

`narrow_to_registered` 对 generated payload enum 穷举匹配，无 wildcard。Known 值是本地注册完整性事实，不是 `KcpError`，不会转换为 `unsupported_method`、`method_unavailable` 或 `internal_error`。

`TypedDispatcher` 只调用现有公共 `handle_system_ping`、`handle_task_create`、`handle_task_get`，不重复 Schema/protocol/auth/method/payload version/deadline 检查，不改写 `HandlerResult` 或 post-commit intents，也不创建平行端口。

## 阶段门

五个 Catalog 方法仍缺正式 handler，bytes/frame/transport/server 生命周期也未关闭。因此当前仍禁止启动 Socket/Named Pipe server；known-unimplemented 不能作为“先启动 server、收到请求后再报错”的兜底。
