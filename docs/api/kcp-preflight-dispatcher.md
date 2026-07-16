# KCP Value preflight 与注册式 dispatcher

> 状态：规范/测试锚点已闭合，Rust 尚未实现。唯一事实源是 [`IMPLEMENTATION_CONTRACTS.md` §5.11](../../specs/IMPLEMENTATION_CONTRACTS.md#511-serde_jsonvalue-preflight-与三方法注册式-dispatcher)，自动化矩阵见 [`CONFORMANCE.md` §5](../../specs/CONFORMANCE.md#5-kernel-control-protocolschema-与事件)。

## 范围

本边界只接收调用方已经解析得到的 `serde_json::Value`。它不负责 bytes、UTF-8、JSON parse、4-byte length prefix frame、最大 frame、transport、连接身份、clock、backend 或 Kernel ID。实现优先加入现有 `kernel-kcp`，复用已有 handlers、ports 与 response contract 门，不建立平行 crate abstraction。

公开调用链固定分三步：

```text
preflight_value(Value)
-> narrow_to_registered(TypedCatalogRequest)
-> TypedDispatcher.dispatch(RegisteredRequest)
```

不得提供一站式 `process_value`，因为首批八方法都有正式 Schema/Catalog，但目前只有三个方法拥有 typed handler。

## Preflight 结果

`preflight_value` 只产生三类结果：

- `Accepted(TypedCatalogRequest)`：完整 generated Envelope/方法 Schema 与 typed decode 已通过；
- `Response(KcpResponseEnvelope)`：request ID 可关联的固定 preflight wire error，且最终 response 已通过 generated Response Schema；
- `LocalRejection(PreflightLocalRejection)`：不能关联 request，或 catalog/schema/generated mapping 出现内部合同失败；不得发送 wire response。

固定优先级：

1. request ID response eligibility；
2. message kind / family；
3. protocol；
4. auth；
5. family-specific method；
6. 根 `payload.schema_version`；
7. 完整 Envelope/方法 Schema + generated typed decode。

顶层 `request_id` 必须是合法 UUID string；wire error 中逐字保留原字符串。非 object、缺失/非 string/非法 UUID 都属于不可关联请求。

`PreflightLocalRejection` 只允许两个固定安全形状：`UncorrelatableRequest { message: "request cannot be correlated" }` 与 `ContractFailure { message: "preflight contract failure" }`；不携带 schema ID、method、原始输入或内部 detail。

## 固定错误

| 条件 | code | message | retryable |
|---|---|---|---:|
| caller 请求不合法 | `invalid_request` | `request is invalid` | false |
| protocol string 非 `1.0` | `unsupported_protocol_version` | `protocol version is not supported` | false |
| 根 payload schema integer 非 `1` | `unsupported_schema_version` | `payload schema version is not supported` | false |
| method 不属于所选 family Catalog | `unsupported_method` | `method is not supported` | false |
| auth 非 null | `unsupported_auth_schema` | `authentication schema is not supported` | false |

所有错误固定 `schema_version=1`、`details=null`。缺失/错误类型通常是 `invalid_request`；只有已确认类型后的不支持值进入对应 `unsupported_*`。根 payload 版本参与优先分类，嵌套版本和普通业务字段失败仍是 `invalid_request`。

实现不能分析 `ContractError` 文本。`kernel-contracts` 必须提供结构化来源阶段：`CallerSchemaValidation` 映射 wire `invalid_request`；`WireDecodeAfterSchema`、`PayloadDecodeAfterSchema`、`GeneratedDiscriminatorMapping`、`SchemaCatalog` 映射本地 ContractFailure。preflight 先单独做完整 Schema validation，成功后再 typed decode；`schema-tool` 必须让 generated wire/payload decode 失败携带后两个明确阶段。

## Catalog 与 registration

method family 必须复用 generated command/query Catalog：

- Command：`task.create`、`stop.activate`；
- Query：`system.ping`、`task.get`、`task.list`、`event.subscribe`、`event.poll`、`stop.status`。

跨 family 名称固定为 `unsupported_method`，例如 query `task.create`。全部八方法的合法请求都必须 preflight 为 typed Accepted。

`narrow_to_registered` 只注册：

- `system.ping`；
- `task.create`；
- `task.get`。

其余五方法在完整 typed decode 后返回本地 `KnownCatalogMethodNotImplemented`。该值不可序列化，不是 KCP error，不能转换为 `unsupported_method`、`method_unavailable` 或 `internal_error`。

## Dispatcher

`TypedDispatcher` 只按 `RegisteredRequest` variant 路由三个现有 handler，并复用现有 `KernelClock`、`KernelIdGenerator`、`TaskApplicationBackend` ports。它不得：

- 创造平行 clock/backend/ID 接口，或向某方法传入它不需要的能力；
- 再次校验 Schema、protocol、auth、method 或 payload version；
- 再次检查 deadline；
- 改写 handler response、local ContractFailure 或 post-commit notification intents；
- 接受 method string + raw payload 形成平行路由。

## Response 门与阶段门

preflight error response 的 generated Response Schema 门是不可替换的生产事实。fault seam 只能 crate-private 用于测试；最终 response 失败时返回本地 ContractFailure，不发送未验证内容。

五个 Catalog 方法没有正式 handler、transport 生命周期也未关闭，所以当前仍禁止启动 Socket/Named Pipe server。known-unimplemented 是本地注册完整性事实，不是允许 server 接入后临时返回的协议能力。
