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
- candidate group：候选组
- workspace：工作区
- worktree：工作树
- bundle：包裹
- mailbox：邮箱
- A、B、C：甲、乙、丙
- read-set：读文件集合
- write-set：写文件集合
- MCP：模型上下文协议
- invariant：不变条件
- parallel：并行
- concurrent：并发
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

# 墨缇斯 Multorum

墨缇斯是为单一代码库上的协调并行开发而构建的基底，支持隔离的*工作区*（workspace）、明确的文件所有权，以及通过架构设计实现的冲突自由。它不关心你使用的具体工作流程、*代行者*（agent）工具链或开发流程。它是一个通用工具，用于使并行工作安全且高效，无论你选择如何组织它。

> Belua multorum es capitums!
>
> <p align="center">
>   <img src="assets/multorum-20260327.png" alt="Multorum logo" width="40%">
> </p>
>
> [EN](README.md) | [CN](README-CN.md)

## 于否认中定义

墨缇斯并非代行者。它不会计划、协商、构思，也不会幻想自己正在做管理工作。它是协调并行工作的基底：隔离的工作区、明确的所有权，以及确保各个*工蜂*（worker）不会悄悄相互破坏的硬性保证。

## 何苦？

并行开发会以两种方式崩溃：

- 要么人们自由工作，之后在合并地狱中付出代价……
- 要么……他们被限制在过于狭窄的沙盒中，失去了完成工作所需的上下文。

墨缇斯旨在避免这种权衡。每个工蜂都保留完整的代码库作为可读上下文，但写入权限被限制在明确的写入范围内，因此系统既保留了全局理解，又保持了局部隔离。

不靠感觉。不靠握手协议。不靠"请尽量不要碰那个"。

我们的方法旨在通过架构设计实现正确性。协调不是靠自觉、猜测或事后清理来完成的；它直接编码到模型中。*女王蜂*（orchestrator）保持唯一权威，范围在开始前声明，冲突的访问模式在工作开始前便会被拒绝，而不是事后修复。因此，这个项目不是试图创造神奇的自主团队协作，而是使并行工作在机械上安全、可检查，并且在设计上有组织有纪律。

> 详细设计参考见 [DESIGN-CN.md](DESIGN-CN.md)。

## 模型

一切围绕三件事展开：女王蜂、*指导意见*（rulebook）和工蜂。

**女王蜂**是唯一的协调者。人、模型、或者两者兼有——墨缇斯不关心。它决定如何拆分工作，哪个角色获得哪个任务，以及哪些结果值得保留。

**指导意见**（`.multorum/rulebook.toml`）定义了命名的*切入点*（perspective）。切入点是一个角色，有两个边界：*写文件集合*（write-set，它可以修改的内容）和*读文件集合*（read-set，它工作时必须保持稳定的内容）。读文件集合不是可见性过滤器。工蜂可以查看整个代码库。它的存在是为了让 墨缇斯知道哪些并发工作禁止被干扰。

**工蜂**是切入点的运行时实例。墨缇斯为它提供一个隔离的 git *工作树*（worktree）、一个固定的基线快照，以及具体化的边界文件。契约成为具有真实限制的真实工作区。在工蜂的更改落地之前，墨缇斯通过指导意见中声明的 **git hooks** 强制执行写文件集合合规性并运行项目检查（构建、lint、测试）。

从同一切入点创建的多个工蜂形成一个*候选组*（candidate group）。它们共享相同的边界，从相同的快照开始，独立竞争。最多只有一个被验收，其他被废弃。

## 保证

墨缇斯围绕一个*不变条件*（invariant）构建：

> 一个文件可以被恰好一个活跃的候选组写入，或者被任意数量的活跃候选组读取，但不能同时被两者操作。

这即是核心。产品的灵魂。其余都只是管道。

并发的写文件集合不应重叠，没有活跃组可以写入另一个组作为稳定上下文所依赖的文件。墨缇斯在工作开始前拒绝不良重叠。毕竟，与其在数小时的工作以灾难性的合并冲突收场后才面对现实，不如快速失败，给人以切合实际的失望。

## 生命周期

女王蜂编写指导意见，然后从其切入点创建工蜂。每个工蜂在自己的工作树内操作，通过运行时界面报告进度，最终提交工作。女王蜂可以验收结果、修订或废弃它。工蜂可以在工作树中持续工作，当它完成后，工作树可以随工蜂本身一起删除。


## 安装

如果以上内容让你感兴趣（或者至少没有吓跑你），以下是入门方法。

安装 [Rust](https://rust-lang.org/tools/install) 后，可以用 Cargo 安装稳定版 Multorum：

```bash
cargo install multorum
```

分发包和预编译二进制文件正在筹备中，待项目进入生产就绪状态后发布。

如果你想要最新但可能不稳定的功能，可以直接从 GitHub 仓库安装：

```bash
cargo install --git https://github.com/LighghtEeloo/multorum.git
```

使用 `cargo uninstall multorum` 可以完全卸载。我们不会留下垃圾。承诺。

<details>
<summary><strong>Shell 补全</strong></summary>
墨缇斯可以为 Bash、Zsh、Fish、Elvish 和 PowerShell 生成 shell 补全脚本。你可以直接从二进制文件 source 这些脚本，以获得始终最新的体验。

```bash
# bash
command -v multorum &>/dev/null && source <(multorum util completion bash)

# zsh
autoload -U compinit
compinit
command -v multorum &>/dev/null && source <(multorum util completion zsh)

# fish
command -v multorum &>/dev/null && multorum util completion fish | source

# elvish
command -v multorum &>/dev/null && source <(multorum util completion elvish)

# powershell
multorum util completion powershell | Out-String | Invoke-Expression
```
</details>

## 然后呢？

墨缇斯被设计为人类和 LLM 均可使用，但 LLM 拥有不公平的优势——它们生成的指令和上下文远比凡人（比如我）详细得多。因此，你可以直接跳到[配置 MCP 服务器](#将-multorum-与-mcp-结合使用)。

### 通过 CLI 使用墨缇斯（主要面向人类）

墨缇斯为人类用户提供了相当不错的 CLI 体验。
- 按 Tab 键查看可用选项。
- 在所有层级的命令中查看 `--help`，你应该就能掌握足够的知识来手动运行女王蜂和工蜂。
- 在 [DESIGN-CN.md](DESIGN-CN.md) 中搜索任何有疑问的概念，你应该能找到详细解释。
- 要理解核心理念，运行 `multorum util methodology <role>` 查看相关主题的简短 Markdown 文档。

我认为大多数人类用户能在大约 10 分钟的探索中理解大部分概念。

### 将墨缇斯与 MCP 结合使用

如果你的女王蜂是一个支持*模型上下文协议*（MCP）的代行者，你可以通过工具调用运行整个墨缇斯循环，而不是临时的 shell 编排。

墨缇斯将高级的女王蜂和工蜂指导内容打包在二进制文件和 MCP 界面中。该指导是规范的引导文本。仓库本地的 skills（如果存在）只是指向已打包方法论的薄包装器。

<details>
<summary><strong>MCP 安装指南</strong></summary>

#### 1) 添加女王蜂 MCP 服务器

将其添加到你的 MCP 主机配置。可选地，显式传递规范的工作区根目录，以防止服务器从意外的宿主 `cwd` 绑定自身。

```json
{
  "mcpServers": {
    "multorum-orchestrator": {
      "command": "multorum",
      "args": ["serve", "orchestrator"],
    }
  }
}
```

#### 2) 添加工蜂 MCP 服务器

为每个工蜂工作树重复此操作。可选地，显式传递该特定工作树，以防止工蜂服务器意外绑定到规范根目录或其他仓库。

```json
{
  "mcpServers": {
    "multorum-worker": {
      "command": "multorum",
      "args": ["serve", "worker"],
    }
  }
}
```

#### 3) 重新加载并验证

重新加载你的 MCP 主机，确认服务器已启动。
</details>

<details>
<summary><strong>可选的 skill 引导指南</strong></summary>

本仓库还为自动发现提示文件的主机打包了最小化的 skills。它们应保持简薄：

- 女王蜂 skill："你是女王蜂。在行动之前读取 `multorum://orchestrator/methodology`。"
- 工蜂 skill："你是工蜂。在行动之前读取 `multorum://worker/methodology`。"

你可以使用以下命令安装：

```bash
npx skills add LighghtEeloo/multorum
```

</details>

## 再次强调，墨缇斯不是氛围淹没型代行者编排系统

墨缇斯不是带着更好态度的合并工具。它不是假装成运行时的聊天协议。它不是编排逻辑的替代品。它不是假设并行工作如果每个人都表达清楚就会表现良好的系统。

它做相反的假设，因为它相信硬边界和机械保证的力量。

## 版本控制

墨缇斯遵循语义版本控制，但有点无聊。所以在此基础上，墨缇斯遵循 *shift versioning™*。
- 第一个版本将是 `0.0.1`，在核心模型被充分理解且没有明显问题时发布。
- 第二个版本将是 `0.1.0`，在实现细节稳定并经过实战测试后发布。
- 第三个版本将是 `1.0.0`，在所有接口稳定并准备好用于生产时发布。

之后将是无尽的无聊维护版本，开发将转向生态系统集成、生活质量改进和日常维护。我真诚地希望这些版本永远不会令任何人感到意外。

## 结语 (❁´◡`❁)

墨缇斯用于单一代码库上的协调并行开发，具有隔离的工作区、明确的文件所有权，以及通过架构设计实现的冲突自由。

如果你也经常在单一代码库并行开发，而且也厌倦了合并冲突的痛苦，那墨缇斯就是你最趁手的工具。

## 致谢

我想在此感谢所有为墨缇斯的设计和开发做出贡献的人，特别是作为早期的支持者和使用者的 [@17-qxm](https://github.com/17-qxm)、[@Yanmeeei](https://github.com/Yanmeeei) 和 [@TomCC7](https://github.com/TomCC7)。
