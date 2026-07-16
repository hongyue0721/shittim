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

## 规则

- accepted 不等于代码已完成；实现状态见 [`../docs/PROGRESS.md`](../docs/PROGRESS.md)。
- 改变常驻进程、状态所有者、核心协议、技术栈、特权类别或 Core 边界必须新建或 supersede ADR。
- ADR 不得引入与 `AGENT.md` 或对应 `specs/` 冲突的第二套事实。
