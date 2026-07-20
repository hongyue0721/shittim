# Schema 生成与契约类型

> 状态徽章：**library implemented**（manifest v2 / walker / transaction / Event·KCP authority 与 catalog 已落地；production bindings 仍为 `[]`；`V2InitialBuildActive` production bindings 初始交付 / server **未完成**）
> 完成状态见 [`../IMPLEMENTATION_MATRIX.md`](../IMPLEMENTATION_MATRIX.md) · [`../PROGRESS.md`](../PROGRESS.md)。

## 权威边界

- 字段、枚举、错误与兼容规则：`specs/` 与 `schemas/source/**/*.json`（合同细节见 IC §5、§6、§13 与 [ADR-0002](../../adr/0002-schema生成与兼容策略.md)）。
- 索引：`schemas/manifest.json`（production=70 entries；41 retained + 29 component-native；`method_version_bindings=[]`）。
- Rust 生成物：`rust/crates/kernel-contracts/src/generated/`（禁止手改）。
- CLI：`schema-tool`；运行时库：`kernel-contracts`。
- 许可证：根目录 [`LICENSE`](../../LICENSE)（Apache-2.0）。

## 命令

```bash
# 统一门：PATH 须先指到 Node 24.18.0
export PATH="$HOME/.local/share/pnpm:$PATH"
./scripts/check-schema.sh

cargo run --manifest-path rust/Cargo.toml -p schema-tool -- --repo-root "$PWD" generate
cargo run --manifest-path rust/Cargo.toml -p schema-tool -- --repo-root "$PWD" check
cargo run --manifest-path rust/Cargo.toml -p schema-tool -- --repo-root "$PWD" \
  validate --schema https://schemas.shittim.local/v1/common/actor.json \
  --instance /path/to/wrapper.json --pointer /instance
cargo run --manifest-path rust/Cargo.toml -p schema-tool -- --repo-root "$PWD" \
  canonicalize /path/to/file.json --pointer /projection --hex
```

- `validate` / `canonicalize` 接受 strict RFC6901 `--pointer`（默认 root）；`canonicalize` 默认 raw JCS bytes，`--hex` / `--hash` 互斥且无尾随 newline。
- `schema-tool` 不做 Task 字段 normalize、URI/time 业务语义或 projection 解释；那些属于 `kernel-task-creation`、`kernel-authorization` 与各自 official fixture harness。CLI oracle 只对 fixture 中 pointer 选中的 normalized object 做 validate/canonicalize。

## 产物路径

| 产物 | 路径 |
|---|---|
| Schema 源 | `schemas/source/{audit,common,task,policy,event,kcp}/` |
| Manifest | `schemas/manifest.json` |
| 生成类型 | `rust/crates/kernel-contracts/src/generated/types.rs`（含切片1a四root及切片1b `ActionRequestV2`、`ActionTransitionIntentV1`、三projection） |
| Catalog | `rust/crates/kernel-contracts/src/generated/catalog.rs` |
| Typed decode | `rust/crates/kernel-contracts/src/generated/typed.rs` + `kernel-contracts::decode_validated` |
| JCS / fixture | `schemas/examples/jcs/`、`schemas/fixtures/**`（official task-creation 与 authorization projection wrappers **不**进 manifest） |
| 统一门 | `scripts/check-schema.sh` |

## V2InitialBuildActive切片1a

切片1a新增四个component-native source root：

- `common/content_origin.v2.json`：stored Origin v2，carrier闭集不含`action_transition`；
- `audit/audit_record.v2.json`：完整v2 wire与封闭task/policy context，不从v1 optional拼装；
- `task/task_creation_provenance.v1.json`：schema-tool原生TaggedUnion，仅`root_command_v2|child_action_v2`；
- `audit/audit_allocation.v2.json`：IC §6.16.0a要求独立Audit路径对象被Schema验证并跨语言边界传递，因此作为`new-contract` source root，而不是只留Rust typed input。

manifest DAG将`audit`、`task`的`allowed_refs`固定为`[common,policy]`；`common`无外依赖，`policy→common`，保持无环。production bindings仍为空。本切片只交付Schema/manifest/generated/conformance，不实现repository、handler或producer。

## V2InitialBuildActive切片1b

切片1b新增五个component-native source root：

- `task/action_transition_intent.v1.json`：IC §6.14 immutable transition authority wire；
- `task/action_request.v2.json`：active Action persisted shape，替换缺少generation/chain/result/lease generation的retained v1；
- `task/child_task_delta_projection.v1.json`：父子scope/capability/delegation delta；
- `policy/material_authorization_projection.v1.json`：material fingerprint唯一preimage；
- `policy/observation_evidence_projection.v1.json`：`not_applicable|observed`真正TaggedUnion。

manifest=70（41 retained + 29 component-native），DAG保持`policy→common`、`task→common,policy`无环，bindings仍空。`kernel-authorization` pure crate接受caller typed authoritative facts，负责三projection构造、规则验证、Schema再验证、JCS/SHA-256；不读SQLite/repository、不分配ID、不写存储、不替代`domain-policy` matcher。IC §5.3.1四份official projection fixtures已落地：`schemas/fixtures/task/child_task_delta_projection.v1.json`、`schemas/fixtures/policy/material_authorization_projection.v1.json`、`schemas/fixtures/policy/observation_evidence_not_applicable.v1.json`、`schemas/fixtures/policy/observation_evidence_observed.v1.json`；generic `ProjectionFixture` wrapper 由 `schema-tool::official_fixture` 解析，production harness 在 `kernel-authorization`，CLI oracle 在 `schema-tool` tests。retained `VerificationResultV1`完整满足child materialization验证记录，未新造版本；SubjectProjection与ApprovalEventAllocation留1c。

## 门禁与流水线概述


1. **Toolchain**：`node scripts/check-node-toolchain.mjs` 硬校验 Node 24.18.0 / pnpm 11.3.0。
2. **Generate×2 + check**：`schema-tool generate` 两次稳定；`check` 报告 drift。
3. **Rust 质量**：fmt、Clippy（`-D warnings`）、workspace tests。
4. **生成物 Git 漂移**：`generated/` 必须与 source/manifest 一致。
5. **FILE_MANIFEST**：`node scripts/update-file-manifest.mjs --check`。

流水线语义（单 root、Linux real-platform verified transaction、Production/Synthetic registry profile、KCP/Event claimant、MethodVersionBinding 两阶段启用）的合同细节见 [ADR-0002](../../adr/0002-schema生成与兼容策略.md) 与 [`IMPLEMENTATION_CONTRACTS.md` §13](../../specs/IMPLEMENTATION_CONTRACTS.md#13-schemacanonical-json-与生成物)。本页不复述 IR、TaggedUnion、artifact journal 或 binding 内部实现。

## 相关入口

- [`../../specs/IMPLEMENTATION_CONTRACTS.md`](../../specs/IMPLEMENTATION_CONTRACTS.md) §5、§6、§13
- [`../../specs/CONFORMANCE.md`](../../specs/CONFORMANCE.md) §5
- [`../../adr/0002-schema生成与兼容策略.md`](../../adr/0002-schema生成与兼容策略.md)
- [Task repository 合同](task-repository-contract.md)
- [API 索引](README.md)
