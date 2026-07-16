# Schema 生成与契约类型

> 状态：已落地首批 Rust 生成链。JSON Schema 是唯一人工源；当前没有 TypeScript/Python 生成物。

## 权威边界

- 字段、枚举、错误与兼容规则的事实源：`specs/` 与 `schemas/source/**/*.json`。
- 索引：`schemas/manifest.json`。
- Rust 生成物：`rust/crates/kernel-contracts/src/generated/`，禁止手改。
- 项目代码许可证：根目录 [`LICENSE`](../../LICENSE)（Apache-2.0）。
- CLI：`schema-tool`；运行时库：`kernel-contracts`。

## 当前产物

| 产物 | 路径 | 说明 |
|---|---|---|
| Schema 源 | `schemas/source/{audit,common,task,policy,event,kcp}/` | 41 个 Draft 2020-12 schema |
| Manifest | `schemas/manifest.json` | `$id`、source、kind、兼容与当前 Rust 目标 |
| 生成类型 | `generated/types.rs` | struct/enum、const 单值类型、`NullOnly` |
| 生成目录 | `generated/catalog.rs` | 由 manifest 生成的 embedded schema 与方法/事件闭集 |
| Typed decode | `generated/typed.rs` | 从 envelope discriminator enum 与 `allOf if/then payload.$ref` 一一映射自动派生；无方法目录模板 |
| JCS 向量 | `schemas/examples/jcs/`、`schemas/fixtures/kcp/task_create_normalized_hash.v1.json` | RFC 8785 示例、UTF-16 排序及 task.create receipt/idempotency 复合 hash fixture；后者不是通用 example wrapper |
| 检查脚本 | `scripts/check-schema.sh` | generate×2、meta/check、fmt、clippy、test |

## 命令

```bash
cargo run --manifest-path rust/Cargo.toml -p schema-tool -- --repo-root "$PWD" generate
cargo run --manifest-path rust/Cargo.toml -p schema-tool -- --repo-root "$PWD" check
cargo run --manifest-path rust/Cargo.toml -p schema-tool -- --repo-root "$PWD" \
  validate --schema https://schemas.shittim.local/v1/common/actor.json \
  --instance /path/to/instance.json
cargo run --manifest-path rust/Cargo.toml -p schema-tool -- --repo-root "$PWD" \
  canonicalize /path/to/file.json --hash
./scripts/check-schema.sh
```

## 生成器支持矩阵

### 生成形状

- 支持：`object`、`properties`、`required`、`array/items`、string `enum`、string/integer/boolean/null `const`、`$ref`、单一非 null 类型与 `null` 的联合、nullable `oneOf: [null, T]`。
- `additionalProperties: true` 且没有声明字段的对象，才生成 `serde_json::Value`；这是 Schema 明确声明的 free-form object。
- KCP/Event 条件 payload 不使用弱 envelope 作为业务类型；生成器解析 discriminator property 的闭集 enum，以及每个 `allOf` 分支的 `if.properties.<discriminator>.const` 与 `then.properties.payload.$ref`。enum 和映射必须一一对应、无重复，Rust variant 碰撞会使生成失败。
- payload `$ref` 必须解析到 manifest 中的完整 Schema，Rust payload 类型名直接使用对应 manifest title；新增方法或事件只需修改 Schema/manifest，生成器会自动增加 variant 和 decode match。
- `type` 含多个非 null 分支、歧义 `oneOf`、schema-valued `additionalProperties`、`anyOf`、`not`、`patternProperties`、`dependentSchemas`、`prefixItems`、`contains`、`unevaluatedProperties` 等形状关键字明确失败，不降级为 `JsonValue`。

### Validation-only 关键字

`minimum/maximum`、`minLength/pattern/format`、`minItems/uniqueItems`、`allOf`、`if/then/else` 等保留在 source schema，由 `jsonschema` 0.28 Draft 2020-12 runtime validator 强制执行。AuditRecord 的 task creation、external status、PermissionDecision ref/context 条件即采用这一路径；生成 Rust 字段类型不等于编译期证明条件成立。`check` 还使用官方 2020-12 meta validator 验证 Schema 文档自身，并编译全部跨文件 `$ref`。

## Response Envelope 边界

Command、Query 和 Event 都在 Envelope 内携带方法/事件 discriminator，因而可以从条件 Schema 自动生成方法绑定的 typed decode。Response Envelope 只有 `status = ok | error`，成功 payload 是带 `schema_version` 的开放对象；具体类型取决于与 `request_id` 配对的原请求方法。

因此当前不生成 `TypedKcpResponseEnvelope`。客户端必须先校验通用 Response Envelope，再根据原请求方法使用对应的 `*_response.json` Schema 和生成类型解码 payload。若未来 Response 增加可靠的方法 discriminator，应先修改权威 Schema，再由生成器自动派生绑定。

## Const、null 与 JCS

- `protocol_version`、`message_kind`、`schema_version` 等 const 生成单值 enum/newtype，Serde 反序列化会拒绝错误值。
- JSON `null` 使用 `NullOnly`，只接受和输出 null。
- RFC 8785 由 `serde_json_canonicalizer = 0.3.2` 实现；仓库不维护自写简化 JCS。
- `task_create_normalized_hash.v1.json` 固定唯一 `normalized_payload`、receipt hash 和精确 idempotency projection/hash；`kernel-contracts` 测试验证 COMMAND、TaskCreate Schema、字段排除边界、数组顺序/重复保留、URI 规范化输出和两个 Rust SHA-256；`schema-tool` CLI smoke 会抽取两条 JSON 路径到临时文件，实际执行 `validate` 与两次 `canonicalize --hash` 并断言同一 fixture hash。

## 明确未实现

- TypeScript/Python 类型生成；
- agentd、KCP transport 与任何可运行服务端；
- Task/TaskScope/ContentOrigin/PermissionDecision 等业务 repository 与 KCP handler；
- `task.create` 把既有 Policy URI 规范化能力接入持久化 producer 的实现。

## 相关规范

- [`../../specs/IMPLEMENTATION_CONTRACTS.md`](../../specs/IMPLEMENTATION_CONTRACTS.md) §5、§6、§13
- [`../../specs/CONFORMANCE.md`](../../specs/CONFORMANCE.md) §5
- [`../../adr/0002-schema生成与兼容策略.md`](../../adr/0002-schema生成与兼容策略.md)
