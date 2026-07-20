# Shittim 架构决策记录

ADR 记录规范允许范围内的已接受实施选择，不取代领域规范。

## 状态

- `proposed`：讨论中；
- `accepted`：已接受，可能尚未实现；
- `superseded`：被后续 ADR 替代。

## 索引

- [ADR-0001：Shittim 工作区与工具链](0001-shittim工作区与工具链.md) — accepted
- [ADR-0002：Schema 生成与兼容策略](0002-schema生成与兼容策略.md) — accepted
- [ADR-0003：KCP 本地传输](0003-kcp本地传输.md) — accepted
- [ADR-0004：Kernel SQLite 文件持久化基座](0004-kernel-sqlite文件持久化基座.md) — accepted
- [ADR-0005：Computer Use 可选 Profile 边界](0005-computer-use可选profile边界.md) — accepted
- [ADR-0006：Child Task 权威与 TaskCreate v2 迁移](0006-child-task权威与taskcreate-v2迁移.md) — accepted；**partial**（见 MATRIX/PROGRESS）
- [ADR-0007：Approval v2 不可变联合、身份与失效](0007-approval-v2不可变联合身份与失效.md) — accepted；**contract-only**
- [ADR-0008：Active Event v2 Schema 与版本化统一 Outbox](0008-active-event-v2与版本化统一outbox.md) — accepted；**partial**（见 MATRIX/PROGRESS）

## 规则

- accepted 不等于代码已完成；实现状态见 [`../docs/IMPLEMENTATION_MATRIX.md`](../docs/IMPLEMENTATION_MATRIX.md) 与 [`../docs/PROGRESS.md`](../docs/PROGRESS.md)。
- ADR 文首只允许一枚状态徽章 + 指向 MATRIX/PROGRESS 的链接；禁止在 ADR 维护字段级或切片级完成清单。
- 改变常驻进程、状态所有者、核心协议、技术栈、特权类别或 Core 边界必须新建或 supersede ADR。
- ADR 不得引入与 `AGENT.md` 或对应 `specs/` 冲突的第二套事实。
