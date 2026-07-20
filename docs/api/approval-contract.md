# Approval v2 与 PermissionDecision 授权合同

> 状态徽章：**contract-only**（accepted；legacy v1 仅供读取/迁移；无 repository 实现）

## 唯一事实源

| 主题 | 锚点 |
|---|---|
| 可编码投影 / subject / confirmation mode / JCS preimage | [`IMPLEMENTATION_CONTRACTS.md` §5.3.1](../../specs/IMPLEMENTATION_CONTRACTS.md#531-可编码投影jcs-preimage-与规范化) |
| Event payload / `change_kind` 真值表 | [`IMPLEMENTATION_CONTRACTS.md` §5.6](../../specs/IMPLEMENTATION_CONTRACTS.md#56-首批正式-event-catalog) |
| 稳定错误码 | [`IMPLEMENTATION_CONTRACTS.md` §5.7](../../specs/IMPLEMENTATION_CONTRACTS.md#57-v2权威-error-catalog) |
| PermissionDecision v2 | [`IMPLEMENTATION_CONTRACTS.md` §6.6](../../specs/IMPLEMENTATION_CONTRACTS.md#66-permissiondecision-v2) |
| ApprovalRecord v2 联合身份 / CAS / material·observation | [`IMPLEMENTATION_CONTRACTS.md` §6.10](../../specs/IMPLEMENTATION_CONTRACTS.md#610-approvalrecord-v2) |
| Event allocation / repository 闭集 / 唯一键 / 恢复硬门 | [`IMPLEMENTATION_CONTRACTS.md` §6.10.6](../../specs/IMPLEMENTATION_CONTRACTS.md#6106-approval-event-allocationrepository闭集唯一键与恢复硬门) |
| 决策背景与边界 | [ADR-0007](../../adr/0007-approval-v2不可变联合身份与失效.md) |
| 域状态 | [`../IMPLEMENTATION_MATRIX.md`](../IMPLEMENTATION_MATRIX.md) · [`../PROGRESS.md`](../PROGRESS.md) |

## 范围

本页只导航 Authorization / Approval v2 合同边界：不可变 request/resolution/invalidation 链、subject 精确绑定、material 与 observation 分离、identity/challenge 证据与 current-head CAS。不复述字段、hash、真值表或 repository API 形状。

## 导航

- [Error Catalog](error-catalog.md)
- [Event Catalog](event-catalog.md)
- [Task repository 合同](task-repository-contract.md)
- [API 索引](README.md)
