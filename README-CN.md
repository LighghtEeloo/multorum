---
本文档由LLM协助翻译。虽已尽力而为，但若有不准确之处，请以英文版本为准。
翻译时，遇到代码或图表时，请保持原文不变，并尽可能在上下文中解释它们的含义和用途。
以下是术语表。在第一次出现时，术语会以斜体显示，并在括号中提供原文：
- Multorum：墨缇斯
- agent：代行者
- orchestration：野蜂飞舞
- orchestrator：女王蜂
- worker：工蜂
- rulebook：指导意见
- perspective：切入点
- bidding group：竞标组
- workspace：工作区
- worktree：工作树
- bundle：包裹
- mailbox：邮箱
- A、B、C：甲、乙、丙
- read-set：读文件集合
- write-set：写文件集合
- MCP：模型上下文协议
- invariant：不变条件
- ACTIVE：工作中
- BLOCKED：停滞
- COMMITTED：已提交
- MERGED：已验收
- DISCARDED：已废弃
- create：创建
- resolve：解决
- revise：修订
- report：报告
- commit：提交
- merge：验收
- discard：废弃
- forward：前移
- body.md：正文
- artifacts：附件
- check pipeline：验收检查流程
- audit：审计
---

# Multorum

Multorum 是为单一代码库上的协调并行开发而构建的基底，它支持隔离的工作空间、明确的文件所有权，以及通过架构设计实现的冲突自由。它不关心你使用的具体工作流程、代理工具链或开发流程。它是一个通用工具，用于使并行工作安全且高效，无论你选择如何组织和执行你的计划。

> Belua multorum es capitums!
> 
> <p align="center">
>   <img src="assets/multorum-20260327.png" alt="Multorum logo" width="40%">
> </p>
>
> [EN](README.md) | [CN](README-CN.md)

## 于否认中定义

Multorum 并非代理。它不会计划、协商、构思，也不会幻想自己正在做管理工作。它是协调并行工作的基底：隔离的工作空间、明确的所有权。它是确保各个 worker 不会相互干扰的硬性保证。

## 何苦？

问题在于：并行开发会以两种方式崩溃。

- 要么人们自由工作，之后在合并选择的地狱中付出代价……
- 要么……他们被限制在非常狭窄的沙盒中，失去了完成工作所需的代码上下文支持。

Multorum 旨在避免这种权衡。每个 worker 都保留完整的代码库作为可读上下文，但写入权限被限制在明确的写入范围内，因此系统既保留了全局理解，又保持了局部隔离。

不靠感觉、不靠握手协议、不靠"请尽量不要碰那个"。

我们的方法旨在通过架构设计实现正确性。协调不是靠自觉、猜测或事后清理来完成的；它直接编码到模型中。Orchestrator 保持绝对权威，允许范围在开始前声明，冲突的访问在工作开始前便会被拒绝，而不是事后修复。因此这个项目不是试图创造神奇的自主团队协作，而是使并行工作在机械上安全、可检查，并且设计上有组织有纪律。

> 详细设计参考见 [DESIGN-CN.md](DESIGN-CN.md)。

## 模型

一切围绕三件事展开：orchestrator、规则手册和 worker。

**orchestrator（协调器）** 是唯一的协调者。可以是人、模型或者都有等等——Multorum 不关心。它只关心工作如何拆分，哪个角色获得哪个任务，以及哪些结果值得保留。

**规则手册（rulebook）**（`.multorum/rulebook.toml`）定义了命名的视角（perspective）。视角为一个角色，有两个边界：**写入集**（它可以修改的内容）和**读取集**（它工作时必须保持稳定的內容）。读取集不是可见性过滤器Worker 可以检查整个代码库。它的存在是为了让 Multorum 知道哪些并发工作禁止被干扰。

**Worker** 是视角的运行时实例。Multorum 为它提供一个隔离的 **git worktree**、一个固定的基线快照，以及具体化的边界文件。契约成为具有真实限制的真实工作空间。在 worker 的更改落地之前，Multorum 通过规则手册中声明的 **git hooks** 强制执行写入范围合规性并运行项目检查（构建、lint、测试）。

从同一视角创建的多个 worker 形成一个**竞标组（bidding group）**。它们共享相同的边界，从相同的快照开始，独立竞争。最多只有一个合并，其他被丢弃。

## 保证

Multorum 围绕一个不变量构建：

> 一个文件可以被恰好一个活跃的竞标组写入，或者被任意数量的活跃竞标组读取，但不能同时被两者操作。

这即是核心——产品的灵魂。其他都是实现细节。

并发写入范围不应重叠，没有活跃组可以写入另一个组作为稳定上下文所依赖的文件。Multorum 在工作开始前拒绝不良重叠。毕竟，与其在几个小时的工作以灾难性的合并冲突结束后才面对现实，不如在工作开始前就快速失败。

## 生命周期

Orchestrator 编写规则手册，然后从其视角创建 worker。每个 worker 在自己的 worktree 内操作，通过运行时 surface 报告进度，最终提交工作。协调器可以合并结果、修改或丢弃它。Worker 可以随时在 worktree 中工作，当它完成后，worktree 可以随 worker 本身一起删除。

## 安装

如果以上内容让你感兴趣（或者说至少没有吓跑你），在安装 [Rust](https://rust-lang.org/tools/install) 后，可以用 Cargo 安装 Multorum：

```bash
cargo install multorum
```

## 将 Multorum 与 MCP 和方法论结合使用

如果你的 Agent 是支持 MCP 的代理，你可以通过工具调用运行整个 Multorum 循环，而不是临时 shell 编排。

Multorum 将高级 orchestrator 和 worker 指导内容打包在二进制文件中，并通过 MCP surface 暴露。该指导是规范的自举文本。仓库本地的 skills（如果存在）只是指向打包方法论的薄包装器。

<details>
<summary><strong>MCP 安装指南</strong></summary>

### 1) 添加 Orchestrator MCP 服务器

将其添加到你的 MCP 主机配置。可选地，显式传递规范的工作空间根目录，以防止服务器从意外的宿主 `cwd` 绑定自己。

```json
{
  "mcpServers": {
    "multorum-orchestrator": {
      "command": "/absolute/path/to/multorum",
      "args": ["serve", "orchestrator"],
      "cwd": "/absolute/path/to/your/repo"
    }
  }
}
```

### 2) 添加 worker MCP 服务器

为每个 worker worktree 重复。可选地，传递该特定 worktree，以防止 worker 服务器意外绑定到规范根目录或其他仓库。

```json
{
  "mcpServers": {
    "multorum-worker": {
      "command": "/absolute/path/to/multorum",
      "args": ["serve", "worker"],
      "cwd": "/absolute/path/to/worker-worktree"
    }
  }
}
```

### 3) 重新加载并验证

重新加载你的 MCP 主机。如果它报告未托管的仓库或根目录不匹配，则传入 `args` 的显式根目录对该服务器角色是错误的。

</details>

<details>
<summary><strong>方法论引导指南</strong></summary>

在第一次运行时操作之前使用打包的方法论。CLI 和 MCP surface 暴露相同的角色指导。

### CLI

直接从二进制文件打印角色方法论：

```bash
multorum methodology orchestrator
multorum methodology worker
```

这些命令是自包含的。它们不需要托管的仓库，适合用于引导提示或宿主端代理设置。

### MCP

每个服务器将相同的方法论作为 Markdown 资源暴露：

- `multorum://orchestrator/methodology`
- `multorum://worker/methodology`

在调用工具之前读取与服务器角色匹配的方法论资源。Orchestrator 方法论属于规范工作空间服务器。Worker 方法论属于 worker-worktree 服务器。

### 最小化宿主提示

如果你的代理运行时需要微小的角色提示，保持它薄并将真正的指导推迟到 Multorum 本身：

- Orchestrator："读取 `multorum methodology orchestrator` 或 `multorum://orchestrator/methodology`，然后仅通过Orchestrator CLI 或 MCP surface 操作。"
- Worker："读取 `multorum methodology worker` 或 `multorum://worker/methodology`，然后仅通过 worker 本地 CLI 或 MCP surface 操作。"

这将使角色指导与打包的二进制文件保持版本同步，而不是在外部提示文件中复制。

### 可选的薄 skills

仓库也可以为自动发现提示文件的主机打包微小的 skills。它们应该保持薄：

- Orchestrator skill："你是 Orchestrator。在行动之前读取 `multorum://orchestrator/methodology`。"
- Worker skill："你是 worker。在行动之前读取 `multorum://worker/methodology`。"

那些文件是便利包装器，而不是独立的真相来源。

</details>

## 再次强调，Multorum 不是氛围型代理 Agent 系统

Multorum 不是带着更好态度的合并工具。它不是假装成运行时的聊天协议。它不是编排逻辑的替代品。它不是假设并行工作如果每个人都表达清楚就会表现良好的系统。

它做相反的假设，因为它相信硬边界和机械保证的力量。

## 版本控制

Multorum 遵循语义版本控制，但有点无聊。所以，另外 Multorum 遵循 *shift versioning™*。
- 第一个版本将是 `0.0.1`，在核心模型被很好地理解且没有明显问题时发布。
- 第二个版本将是 `0.1.0`，在实现细节稳定并经过实战测试后发布。
- 第三个版本将是 `1.0.0`，在所有接口稳定并准备好用于生产时发布。

之后将是无限且无聊的维护版本，开发将转向生态系统集成、生活质量改进和日常维护。我真诚地希望这些版本永远对任何人来说都不会感到难绷。

## 结语 (❁´◡`❁)

Multorum 用于单一代码库上的协调并行开发，具有隔离的工作空间、明确的文件所有权，以及通过架构设计实现的受限的自由。

如果你正在对单一代码库运行多个 worker，并且厌倦了将合并痛苦视为宇宙不变的法则，Multorum 就是你的工具。
