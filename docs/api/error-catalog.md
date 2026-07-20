# Error Catalog

> 状态徽章：**contract-index**（本页不定义独有 wire 事实；code / message / details / retryable 以 IC 为准）

## 唯一事实源

| 主题 | 锚点 |
|---|---|
| v2 权威 Error Catalog 全表 | [`IMPLEMENTATION_CONTRACTS.md` §5.7](../../specs/IMPLEMENTATION_CONTRACTS.md#57-v2权威-error-catalog) |
| MethodVersionBinding 结构与启用阶段 | [`IMPLEMENTATION_CONTRACTS.md` §13.5](../../specs/IMPLEMENTATION_CONTRACTS.md#135-methodversionbinding权威结构与两阶段启用) |
| 域状态 | [`../IMPLEMENTATION_MATRIX.md`](../IMPLEMENTATION_MATRIX.md) · [`../PROGRESS.md`](../PROGRESS.md) |

## 范围

本页只作稳定错误码导航入口。preflight、存储、Task/Event、Policy/Action/Lease、Approval/Identity 等分类与关键消歧（含 `child_materialization_conflict` / `idempotency_conflict` / challenge 终态）均以 IC §5.7 为唯一权威；任何差异以 IC 为准且文档漂移检查必须失败。

## 导航

- [Event Catalog](event-catalog.md)
- [Approval 合同](approval-contract.md)
- [Task repository 合同](task-repository-contract.md)
- [API 索引](README.md)
