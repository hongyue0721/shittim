# Task创建、Child materialization 与 repository 硬合同

> 状态徽章：**partial**（首批 12 Schema + `kernel-task-creation` pure library + official fixtures/harness **已实现**；production MethodVersionBindings 仍为空；active v2 repository/handler/materializer/cutover **未实现**；legacy TaskCreate v1 create/get 已实现但不得进入 future production server）

## 唯一事实源

| 主题 | 锚点 |
|---|---|
| Input Scope / ContentOrigin 输入 Schema | [`IMPLEMENTATION_CONTRACTS.md` §5.3](../../specs/IMPLEMENTATION_CONTRACTS.md#53-contentorigin-与输入-scope-schema) |
| 投影 / JCS preimage / root·child normalize / fixtures | [`IMPLEMENTATION_CONTRACTS.md` §5.3.1](../../specs/IMPLEMENTATION_CONTRACTS.md#531-可编码投影jcs-preimage-与规范化) |
| 稳定错误码 | [`IMPLEMENTATION_CONTRACTS.md` §5.7](../../specs/IMPLEMENTATION_CONTRACTS.md#57-v2权威-error-catalog) |
| Approval allocation / repository 闭集硬门 | [`IMPLEMENTATION_CONTRACTS.md` §6.10.6](../../specs/IMPLEMENTATION_CONTRACTS.md#6106-approval-event-allocationrepository闭集唯一键与恢复硬门) |
| 首批 12 Schema / fragment 宿主规则 | [`IMPLEMENTATION_CONTRACTS.md` §13.6](../../specs/IMPLEMENTATION_CONTRACTS.md#136-manifest-namespace迁移合同) |
| V2ProductionWriteCutover | [`IMPLEMENTATION_CONTRACTS.md` §13.7](../../specs/IMPLEMENTATION_CONTRACTS.md#137-v2productionwritecutover唯一谓词) |
| Child Task 唯一入口与迁移决策 | [ADR-0006](../../adr/0006-child-task权威与taskcreate-v2迁移.md) |
| 域状态 | [`../IMPLEMENTATION_MATRIX.md`](../IMPLEMENTATION_MATRIX.md) · [`../PROGRESS.md`](../PROGRESS.md) |

## 范围

本页导航 root TaskCreate v2、child Action materialization、allocation/projection、official fixtures 与 repository 硬门。lifecycle 摘要：active `task.create` = v2 root-only（未实现 runtime）；legacy v1 仅 validation/read 且须退出 active dispatcher；child 唯一新写入口为 `kernel.task/task.child.create`。字段级合同、七 UUID / 十 UUID、JCS 形状与 tamper 矩阵不在本页复述。

## 导航

- [kernel-kcp typed handler](kernel-kcp.md)
- [kernel-sqlite API](kernel-sqlite.md)
- [Error Catalog](error-catalog.md)
- [Approval 合同](approval-contract.md)
- [Schema 生成操作手册](schema-generation.md)
- [API 索引](README.md)
