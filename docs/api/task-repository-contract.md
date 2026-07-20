# Task创建、Child materialization 与 repository 硬合同

> 状态徽章：**partial**（首批 12 Schema + `kernel-task-creation` pure library + official fixtures/harness，切片1b Action/child 授权 Schema + `kernel-authorization` pure crate，切片2 `kernel-sqlite` **active root TaskCreate v2 repository**（migration 0004），切片3a/3b bindings+kcp runtime，切片3c v1 write 删除/Outbox v2-only/旧库拒绝 **已实现**；child materializer / Action·PD·Approval repositories **未实现**）

## 唯一事实源

| 主题 | 锚点 |
|---|---|
| Input Scope / ContentOrigin 输入 Schema | [`IMPLEMENTATION_CONTRACTS.md` §5.3](../../specs/IMPLEMENTATION_CONTRACTS.md#53-contentorigin-与输入-scope-schema) |
| 投影 / JCS preimage / root·child normalize / fixtures | [`IMPLEMENTATION_CONTRACTS.md` §5.3.1](../../specs/IMPLEMENTATION_CONTRACTS.md#531-可编码投影jcs-preimage-与规范化) |
| 稳定错误码 | [`IMPLEMENTATION_CONTRACTS.md` §5.7](../../specs/IMPLEMENTATION_CONTRACTS.md#57-v2权威-error-catalog) |
| Approval allocation / repository 闭集硬门 | [`IMPLEMENTATION_CONTRACTS.md` §6.10.6](../../specs/IMPLEMENTATION_CONTRACTS.md#6106-approval-event-allocationrepository闭集唯一键与恢复硬门) |
| 首批 12 Schema / fragment 宿主规则 | [`IMPLEMENTATION_CONTRACTS.md` §13.6](../../specs/IMPLEMENTATION_CONTRACTS.md#136-manifest-namespace迁移合同) |
| V2InitialBuildActive | [`IMPLEMENTATION_CONTRACTS.md` §13.7](../../specs/IMPLEMENTATION_CONTRACTS.md#137-v2initialbuildactive唯一谓词) |
| Child Task 唯一入口 | [ADR-0006](../../adr/0006-child-task权威与taskcreate-v2迁移.md) |
| v2 从零构建 / 取消 v1 数据迁移 | [ADR-0009](../../adr/0009-v2从零构建并取消v1数据迁移.md) |
| 域状态 | [`../IMPLEMENTATION_MATRIX.md`](../IMPLEMENTATION_MATRIX.md) · [`../PROGRESS.md`](../PROGRESS.md) |

## 范围

本页导航 root TaskCreate v2、child Action materialization、allocation/projection、official fixtures 与 repository 硬门。lifecycle 摘要：

- active `task.create` = v2 root-only；**repository 已实现**（`WriteTransaction::create_root_task_v2`）；**KCP method-aware preflight / handler / adapter 已接**（切片3b → `create_root_task_v2`）；
- v1 仅 Schema/fixture 历史验证，production 请求得 `unsupported_schema_version`；v1 repository write **已删除**（切片3c），不是持续维护路径；
- child 唯一新写入口为 `kernel.task/task.child.create`（materializer 未实现）；Action / Approval repositories 未实现；
- 不支持 v1 业务数据迁移；旧开发库 `reinitialize-required`（切片3c 已落地）；Outbox production API 为 v2-only；
- production bindings + v2 dispatcher + v2 repository 是 `V2InitialBuildActive` 初始交付，不是 cutover。

- `kernel-task-creation`唯一拥有root/child proposal normalize、receipt/idempotency与allocation validation；
- `kernel-sqlite` root v2 repository 消费上述 pure API，不复制 hash/normalize 语义；
- `kernel-authorization`唯一拥有ChildTaskDelta、MaterialAuthorization、ObservationEvidence/Subject projection的typed facts→构造/验证/JCS/SHA-256；
- retained `VerificationResultV1`已覆盖child materialization所需验证事实，继续复用；
- pure crate均不读repository/SQLite、不分配ID、不写存储；authorization crate不替代`domain-policy` matcher。

字段级合同、七 UUID / 十 UUID、JCS 形状与 tamper 矩阵不在本页复述。

## 导航

- [kernel-kcp typed handler](kernel-kcp.md)
- [kernel-sqlite API](kernel-sqlite.md)
- [Error Catalog](error-catalog.md)
- [Approval 合同](approval-contract.md)
- [Schema 生成操作手册](schema-generation.md)
- [API 索引](README.md)
