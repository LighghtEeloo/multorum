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

# Multorum 项目：架构参考

## 目录

1. [介绍](#介绍)
2. [核心模型](#核心模型)
3. [规则手册](#规则手册)
4. [工作空间模型](#工作空间模型)
5. [Worker 生命周期](#worker-生命周期)
6. [邮箱协议](#邮箱协议)
7. [合并管道](#合并管道)
8. [MCP Surface](#mcp-surface)
9. [指令参考](#指令参考)

---

## 介绍

Multorum 管理对同一代码库的多个同时视角。它专为协调开发工作流而设计，其中称为协调器（orchestrator）的代理将目标分解为任务并分配给隔离的 worker。每个 worker 在自己的工作空间中运行，可以查看整个代码库以供执行和分析，但只能修改策略声明的文件。

该系统存在是为了解决并行开发中的一个具体矛盾：

- worker 需要隔离以免相互干扰
- worker 需要完整的代码库上下文以使其代码、测试和工具仍有意义

Multorum 通过将创作范围与执行范围分离来解决这个问题。Worker 只能在其声明的写入集内写入，但可以针对整个代码库进行编译、测试和导航。

Multorum 是一种基础设施，而非代理。它强制执行不变量、具体化 worker 环境，并记录状态转换。所有协调智能都留在协调器中，每个状态转换都只是因为协调器或 worker 发出了明确的指令。

有一个规范的代码库处于版本控制中。Worker 从不直接修改它。所有更改都通过 Multorum 的合并管道流动，然后由协调器集成。

---

## 核心模型

### 协调器

协调器是唯一的协调权威。它可以是人、LLM 或混合体。其职责是：

- 将开发目标分解为任务
- 声明定义所有权边界的规则手册
- 创建、修改、合并、丢弃和删除 worker
- 接收 worker 报告并解决阻塞问题
- 随着时间推移演化规则手册

通信拓扑是严格的星形：

```
       协调器
      /    |    \
     /     |     \
 Worker A Worker B Worker C
```

Worker 之间从不直接通信。

### 规则手册、视角和 Worker

规则手册是项目对所有权边界的声明。它定义了命名的文件集、视角和合并时检查管道。

视角是规则手册中的命名角色。它声明：

- 写入集：该角色的 worker 可以修改的文件
- 读取集：该角色活跃时必须保持稳定的文件

任一集都可以为空（省略或设为 `""`），表示该视角不声明该角色的文件。写入集为空的视角不能修改任何文件。读取集为空的视角对代码库的其余部分没有稳定性约束。

写入集是现有文件的封闭集合。Worker 不得在写入集之外写入或创建文件。当被阻止的 worker 发现任务确实需要一个新文件时，协调器必须更新规范工作空间和规则手册，然后将受阻的竞标组转发到 HEAD 再解决阻塞。读取集声明哪些文件必须不受其他并发工作的干扰，并告知 worker 协调器认为哪些是稳定上下文。无论读取集如何，Worker 都可以读取代码库中的任何文件。

Worker 是视角的运行时实例。视角是静态策略。Worker 是有状态的临时执行。

### 竞标组

当协调器为某个视角创建第一个 worker 时，形成一个竞标组。该组的基线提交设置为创建时刻的 HEAD，其编译边界是在该快照上评估的视角。后续为同一视角创建的 worker 加入现有组并共享其基线提交和边界。

如果协调器想要为已有活跃竞标组的视角设置新的基线，必须先完全合并或丢弃现有组，或通过 `perspective forward` 将其转发到 HEAD。

一个竞标组中只能有一个 worker 被合并。一旦一个成员被合并，该组中的其余成员将被丢弃。

### 无冲突不变量

核心正确性不变量是：

> **一个文件可以被恰好一个活跃的竞标组写入，或者被任意数量的活跃竞标组读取，但不能同时被两者操作。**

对于任意两个不同的活跃竞标组 G 和 H：

- `write(G) ∩ write(H) = ∅`
- `write(G) ∩ read(H) = ∅`
- `read(G) ∩ write(H) = ∅`

在一个竞标组内，每个 worker 都有相同的边界。冲突检测属于竞标组级别，而不是视角名称级别：视角描述策略，竞标组是必须不相互干扰的并发运行时实体。

不变量扩展到规范分支。当任何竞标组活跃时，每个活跃组的读取和写入集的并集形成**协调器排除集**——协调器在拥有的 worker 被合并或丢弃之前不能提交的文件的集合。协调器只能提交排除集之外的文件。

Multorum 在 worker 创建时强制执行无冲突不变量。不变量是活跃竞标组的运行时属性，不是规则手册的静态属性——同一组视角在给定的仓库状态下可能冲突也可能不冲突，取决于它们的 glob 匹配哪些文件。

### 视角验证

协调器可以在创建 worker 前检查一组视角是否满足无冲突不变量。`perspective validate` 从当前规则手册编译命名视角，检查它们之间的冲突，并检查它们与活跃竞标组的冲突。使用 `--no-live` 时，检查仅覆盖命名的视角，忽略活跃组。

### 视角转发

`perspective forward` 将活跃竞标组从其当前基线提交移动到 HEAD，从当前规则手册重新编译视角边界。

重新编译的边界必须是组当前具体化边界的超集，读取集和写入集各自独立。允许边界扩展。拒绝边界缩减，因为这会破坏创建活跃 worker 所依据的契约。

在移动任何 worktree 之前，Multorum 验证整个活跃竞标组：每个活跃 worker 必须是非 `ACTIVE` 状态，必须有持久的重放检查点，并且在检查点时仍然是干净的。然后逐个转发 worktree。如果后续 worker 转发失败，Multorum 回滚它已经移动的每个 worker，不保存新的组基线或边界。因此原子性边界是持久的运行时状态，而不是每个单独的 Git 操作。

自动转发应用相同的操作，从协调器操作中推导出"在此 HEAD 下继续此视角"的意图。Multorum 只能在前向证明成功后自动转发。通过正常 `perspective forward` 规则证明可以成功转发整个活跃竞标组时，才能自动转发。

自动转发仅在效果上等同于协调器先运行 `perspective forward <perspective>` 然后重试原始命令时有效。当该证明不可用时，Multorum 保持组不变，并告诉用户如果仍想移动组，则明确运行 `multorum perspective forward <perspective>`。

规则是：

- 它处理一个视角的整个活跃竞标组，而不是孤立的单个 worker
- 除非该竞标组中的每个活跃 worker 都是非 `ACTIVE` 状态，否则被拒绝
- 它仅保留每个 worker 已记录的持久检查点的进度：对于 `BLOCKED` worker 是最新的阻塞 `report`，对于 `COMMITTED` worker 是提交的 head 提交
- 它拒绝脏的或漂移的 worktree，而不是试图发明恢复
- 它使每个转发的 worker 保持其当前非 `ACTIVE` 状态；被阻塞的 worker 仍然需要 `resolve`，已提交的 worker 仍然需要 `revise`、`merge` 或 `discard`
- 每个成功的自动转发都向调用者宣布

---

## 规则手册

规则手册位于 `.multorum/rulebook.toml`，与其管理的代码库一起提交到版本控制。也就是说，规则手册是一个声明性的视角声明，不绑定到特定版本。在某种程度上，它更像是一个关于项目结构、布局、测试和验证的便利速记。

### 文件集代数

Multorum 通过一小部分命名的文件集代数来描述所有权边界，为项目提供稳定的词汇来描述仓库的区域。

#### 语法

```text
path  ::= <glob pattern>              例如 "src/auth/**", "**/*.spec.md"
name  ::= <identifier>                例如 AuthFiles, SpecFiles
expr  ::= name                        引用
        | expr "|" expr               并集
        | expr "&" expr               交集
        | expr "-" expr               差集
        | "(" expr ")"                分组

definition ::= name ".path" "=" path  原始 - 将名称绑定到 glob
             | name "=" expr          复合 - 将名称绑定到表达式
```

`A | B` 产生任一集中的每个文件。`A & B` 仅保留两个集中都存在的文件。`A - B` 保留在 A 中但不在 B 中的文件。优先级是扁平的；分组重要时使用括号。

文件集名称和视角名称使用 CamelCase。Worker id 使用 kebab-case。

#### 命名定义

名称在 `[fileset]` 表中定义。名称可以通过 `.path` 绑定到原始路径，也可以通过引用其他名称的复合表达式绑定。

```toml
[fileset]
SpecFiles.path = "**/*.spec.md"
TestFiles.path = "**/test/**"

AuthFiles.path = "auth/**"
AuthSpecs = "AuthFiles & SpecFiles"
AuthTests = "AuthFiles & TestFiles"

[perspective.AuthImplementor]
read  = "AuthSpecs"
write = "AuthFiles - AuthSpecs - AuthTests"

[perspective.AuthTester]
read  = "AuthSpecs | AuthTests"
write = "AuthTests"
```

此示例使用交集来划分交叉子集，并使用差集来划分所有权。`AuthImplementor` 写入生产代码，`AuthTester` 写入测试，它们的写入集是不相交的，因此可以并发运行。

#### 编译和验证

文件集表达式仅是规则手册级别的语法。当 Multorum 需要具体边界时——在 worker 创建、视角验证或视角转发时——它通过针对工作树扩展 glob 并计算集合操作来将表达式编译为具体文件列表。

编译时验证检查以下内容：

- 文件集定义中没有循环
- 没有未定义的引用
- 空集是允许的，但会产生警告

编译过程证明规则手册在结构上是有效的。它不证明新 worker 可以与已活跃的 worker 并发运行——该检查在 worker 创建时发生。

### 检查管道

规则手册声明项目特定的合并管道：

```toml
[check]
pipeline = ["fmt", "clippy", "test"]

[check.command]
fmt = "cargo fmt --check"
clippy = "cargo clippy --workspace --all-targets -- -D warnings"
test = "cargo test --workspace"

[check.policy]
test = "skippable"
```

`[check.command]` 将检查名称映射到 shell 命令。`[check.policy]` 覆盖特定检查的默认行为。检查可以声明两种策略之一：

- `always`（默认）：检查无条件运行
- `skippable`：如果协调器接受提交的证据，则可以跳过

写入集范围检查始终是强制性的，不能配置。

### 默认模板

`multorum init` 创建下面显示的精简规则手册模板，并在 `.multorum/orchestrator/` 下准备空的协调器运行时脚手架（`group/`、`worker/` 和 `exclusion-set.txt`）：

```toml
# 首先定义共享的文件所有权词汇。
# `Name.path` 绑定一个 glob；`Name = "Expr"` 用 |、& 和 - 组合名称。
[fileset]

# 在 `[perspective.<Name>]` 下添加每个视角的表。
# `write` 命名该视角可以修改的文件（可选，默认为空）。
# `read` 命名并发工作不得写入的稳定上下文文件（可选，默认为空）。
[perspective]

# 按执行顺序添加预合并门。
# 在 `[check.command]` 下添加命令，在 `[check.policy]` 下添加可选的跳过策略。
[check]
pipeline = []
```

### 编写好的规则手册

规则手册成功的标志是视角可以并发运行，而协调器不需要持续调解边界冲突。目标是创建一套文件集和视角的词汇表，自然地映射到项目实际做的工作，而不是与工作对抗的官僚 overlay。

#### 首先构建文件集词汇表

从原始定义开始。每个原始定义将一个 glob 绑定到一个名称，该名称描述团队已经使用的仓库区域：`AuthFiles`、`ApiHandlers`、`MigrationScripts`。然后使用复合表达式来划分这些区域，使其符合工作实际划分的方式：规范与实现、测试与生产代码。

好的文件集名称读起来像领域词汇。它们描述区域中包含什么，而不是如何使用它。`AuthFiles` 比 `AuthWorkerScope` 更好，因为同一区域可能以不同角色出现在多个视角中。

保持原始 glob 足够具体，这样它们就不会在仓库增长时静默包含无关文件。`src/auth/**` 比 `**/*auth*` 更好，因为后者会匹配 `docs/auth-migration-plan.md` 和任何其他恰好包含子字符串的内容。

按顺序定义，先原始后复合，按子系统分组。读者应该能够从上到下扫描 `[fileset]` 表，在不跳来跳去的情况下理解仓库的所有权图。

#### 围绕并行工作设计视角

视角是角色，而非任务。为它授权的工作种类命名，而不是为正在处理的特定票据命名。`AuthImplementor` 是一个可以跨许多任务重用的角色。`FixLoginBug` 是一个一次性标签，对下一个读者来说关于它控制的边界什么也说不清。

每个视角声明两件事：

- **write**：此角色可以修改的现有文件的封闭集。Worker 不能在其中创建新文件。如果任务确实需要一个新文件，协调器必须先创建文件并更新规则手册，worker 才能继续。可以省略或为空以表示只读视角。
- **read**：此角色活跃时必须保持稳定的文件。读取集告知 Multorum 并发工作不得干扰哪些文件，并告知 worker 协调器认为哪些是稳定上下文。Worker 仍然可以读取整个代码库。可以省略或为空表示不需要稳定性保证。

无冲突不变量在竞标组级别运行：对于任意两个不同的活跃组，它们的写入集必须不相交，并且都不能写入对方的读取集。设计视角时，使得你打算并发运行的视角自然满足这一点。写入集重叠的两个视角实际上不是并行工作，因此必须顺序运行。

保持读取集窄小。将整个代码库列为读取依赖会阻止所有并发写入，这就违背了目的。只包括 worker 作为稳定上下文真正依赖的文件：规范、接口、共享类型、配置。项目自己的规则手册演示了这一点——视角读取 `ProjectSurfaceFiles`（清单、文档、入口点）而不是整个树。

#### 分区而非重叠

最有用的规则手册模式是分区：使用集合差集将子系统划分为不相交的写入集。设计文档中的运行示例展示了这一点：

```toml
AuthSpecs = "AuthFiles & SpecFiles"
AuthTests = "AuthFiles & TestFiles"

[perspective.AuthImplementor]
write = "AuthFiles - AuthSpecs - AuthTests"

[perspective.AuthTester]
write = "AuthTests"
```

`AuthImplementor` 写入生产 auth 代码。`AuthTester` 写入 auth 测试。它们的写入集通过构造是不相交的，因为一个减去了另一个拥有的内容。两者都读取规范，因此规范在任一角色活跃时保持稳定。

当视角必须共享对某个区域的感知而不写入它时，将共享文件放在两者的读取集中。当一个视角产生另一个视角消费的文件时，消费者读取它们，生产者写入它们——而不是两者都写入。

#### 为项目配置检查管道

检查管道是 worker 的提交到达规范代码库之前的最后一道关卡。按应运行的顺序声明检查。快速、廉价的检查优先——格式化、lint——这样像完整测试套件这样昂贵的检查只在通过基本卫生的代码上运行。

仅当协调器可以合理地从 worker 提交的证据判断检查会通过时，才将检查标记为 `skippable`。完整测试套件和整个工作空间的 lints 是常见的候选：其更改仅限于一个模块的 worker 可以提交相关测试通过的证据，协调器可以决定是否信任它。格式检查通常不值得跳过，因为它们快速且确定性。

强制性的写入集范围检查不在管道中声明。它始终首先运行，不能配置。管道仅包含跟随其后的项目定义检查。

每个声明的检查必须恰好出现在管道中一次，每个管道条目必须有对应的命令，并且没有命令可以为空。这些约束在编译时强制执行。

#### 增量演化规则手册

规则手册提交到版本控制并与其管理的代码一起进行版本控制。将其视为活的基础设施，而不是一次性配置。

当仓库的结构发生变化——出现新模块、子系统被重组、所有权边界转移——更新规则手册以匹配。为新区域添加新的文件集。调整当职责移动时的视角边界。删除不再对应于真实工作的文件集和视角。

Multorum 没有单独的规则手册激活步骤。编译策略的操作（`perspective list`、`perspective validate`、`worker create` 和 `perspective forward`）在运行时从当前工作树读取 `.multorum/rulebook.toml`。因此磁盘上的规则手册编辑立即影响后续操作，甚至在提交之前。为了可重复的编排决策，在创建 worker 之前提交规则手册编辑。活跃的 worker 仍然在其固定的快照下运行，并且只有当协调器将竞标组转发到 HEAD 时，它们的具体化边界才会改变。

为活跃竞标组扩展视角边界时，重新编译的边界必须是当前边界的超集。缩减被拒绝，因为它会破坏创建活跃 worker 所依据的契约。如果一个视角需要收缩，先结束其活跃的 worker。

---

## 工作空间模型

### Bundle

Bundle 是一个包含 `body.md` 主内容文件和 `artifacts/` 辅助文件子目录的目录。Bundle 是 Multorum 存储结构化内容的原子容器：邮箱消息携带一个，审计条目携带一个用于协调器的理由。

```text
<bundle-directory>/
  body.md          # 主 Markdown 内容
  artifacts/       # 可选的辅助文件
```

`body.md` 和 `artifacts/` 对 Multorum 是不透明的。运行时从用户提供的 payload 中具体化它们，但从不解析其内容。

当 payload 按路径提供文件时，Multorum 使用它们而不是复制它们。在成功发布时，运行时将文件移动到 bundle 存储并负责保留它们。

### 文件系统布局

Multorum 项目在仓库根目录添加一个 `.multorum/` 目录：

```text
<project-root>/
  .multorum/
    .gitignore          # 已提交 - 忽略运行时目录
    rulebook.toml       # 已提交 - 文件集、视角、检查管道
    audit/              # 已提交 - 追加式合并审计跟踪
    orchestrator/       # 被 gitignore 忽略 - 协调器本地控制平面
    tr/                 # 被 gitignore 忽略 - 托管的 worker worktree
  src/
  tests/
  ...
```

项目提交 `.multorum/rulebook.toml`、`.multorum/.gitignore` 和 `.multorum/audit/` 的内容。`.multorum/` 下的其他一切都是运行时状态，不随仓库携带。

`.multorum/.gitignore` 包含：

```text
orchestrator/
tr/
```

Multorum 在 `multorum init` 期间验证这些条目，如果缺失则发出警告。

运行时目录名称故意简短。`tr/` 保持托管 worktree 路径紧凑，而 `group/` 和 `worker/` 保持协调器控制平面浅层，而不会强制将不相关的状态更新放入一个 monolith 文件。

### 协调器运行时接口

协调器的控制平面位于 `.multorum/orchestrator/`，在 `multorum init` 期间创建：

```text
.multorum/orchestrator/
  group/
    <Perspective>.toml   # 每个视角一个竞标组记录
  worker/
    <worker>.toml        # 每个 worker id 一个 worker 记录
  exclusion-set.txt      # 具体化的协调器排除集
```

`group/<Perspective>.toml` 存储一个视角的组范围的运行时状态：视角名称、固定的基线提交，以及编译的边界（作为具体文件列表的读取和写入集）。

`worker/<worker>.toml` 存储一个 worker 的 worker 范围的运行时状态：worker id、拥有的视角、生命周期状态、托管的 worktree 路径，以及适用的提交 head 提交。

`multorum init` 创建空的 `group/` 和 `worker/` 目录。后续操作按如下方式更新它们：

- `worker create` 形成新组时，用视角、基线提交（HEAD）和编译边界写入 `group/<Perspective>.toml`，然后写入第一个 `worker/<worker>.toml`。
- `worker create` 加入现有组时，仅写入新的 `worker/<worker>.toml`。
- `worker merge` 将选定的 worker 标记为 `MERGED`，将兄弟标记为 `DISCARDED`，并清除 `group/<Perspective>.toml` 中的边界，以使该组不再贡献排除集。
- `worker discard` 将 `worker/<worker>.toml` 标记为 `DISCARDED`。如果组没有剩余的非终结成员，则清除 `group/<Perspective>.toml` 中的边界。
- `worker delete` 删除 `worker/<worker>.toml`。如果这是该视角的最后一个 worker，它也删除 `group/<Perspective>.toml`。
- `perspective forward` 用新的基线提交和重新编译的边界重写 `group/<Perspective>.toml`，然后重写每个转发 worker 的 `read-set.txt` 和 `write-set.txt` 以匹配。

Worker 也直接更新自己的条目：

- 确认 `task`、`resolve` 或 `revise` 收件箱消息会将 `worker/<worker>.toml` 更新为 `ACTIVE`。
- `local report` 将 `worker/<worker>.toml` 更新为 `BLOCKED`。
- `local commit` 将 `worker/<worker>.toml` 更新为 `COMMITTED` 并记录提交的 head 提交。

协调器仅写入终结状态 `MERGED` 和 `DISCARDED`。一旦条目达到任一终结状态，worker 必须将其视为只读。

`exclusion-set.txt` 是持久化组和 worker 状态的扁平化投影：仍有活跃 worker 的所有组的读取和写入集的并集。规范工作空间中的预提交钩子读取它并拒绝触及任何列出文件的提交。Multorum 在组或 worker 状态更改时重新生成它。当没有组携带边界时文件为空。

### 审计跟踪

合并审计跟踪位于 `.multorum/audit/`，是 `orchestrator/` 和 `tr/` 的同级目录：

```text
.multorum/audit/
  <audit-entry-id>/
    entry.toml
    body.md
    artifacts/
```

审计条目是已提交的项目历史。它们位于 `orchestrator/` 子树之外，随仓库携带。

每个条目在 `merge` 成功时原子性地写入，包含 worker、视角、基线提交、整合的 head 提交、更改的文件列表、运行的检查或跳过的检查，以及协调器提供的理由。审计条目 ID 格式为 `<worker>-<head-prefix6>`，其中 `<head-prefix6>` 是整合的 worker head 提交的前六个字符。理由是一个 bundle——在合并时由协调器附加的 `body.md` 和可选的 `artifacts/`——解释 worker 完成了什么以及为什么接受合并。Multorum 将 `entry.toml` 和理由文件写入同一个 audit-entry-id 目录。审计条目是追加式的；Multorum 永不修改或删除它们。

### Git Worktree

每个 worker 工作空间是一个从竞标组基线提交创建的 git worktree：

```text
git worktree add .multorum/tr/<worker> <base-commit>
```

同一竞标组中的 worker 共享相同的基线提交，在组中第一个 worker 创建时设置。不同竞标组中的 worker 可能有不同的基线提交。

当 worker 达到 `MERGED` 或 `DISCARDED` 后，其身份可以重用于新的 worker。重用始终是"在这里创建一个新 worker"，而不是"重新打开旧状态"。当重用显式 worker id（`--worker <worker>`）且最终化的工作空间仍然存在时，`worker create` 需要 `--overwriting-worktree` 来替换该保留的工作空间。如果最终化的工作空间已被删除，则重用不需要覆盖标志。

### Worker 运行时 Surface

每个 worker worktree 都有自己的 `.multorum/` 目录，与协调器的分开。在创建时，Multorum 具体化：

```text
.multorum/
  rulebook.toml      # 基线提交的快照
  contract.toml      # worker、视角、基线提交
  read-set.txt       # 编译的读取集
  write-set.txt      # 编译的写入集
  inbox/
    new/
    ack/
  outbox/
    new/
    ack/
```

这些文件仅供运行时使用，绝不能提交。Multorum 在每个 worktree 中安装本地忽略规则，以将它们保持在版本控制之外。

---

## Worker 生命周期

### 状态机

```
                 BLOCKED ──────►┐
                    ▲ │         │
             report │ │ resolve │
                    │ ▼         │
create ─────────► ACTIVE ──────►┼──────────► DISCARDED
                    │ ▲         │ discard
             commit │ │ revise  │
                    ▼ │         │
                 COMMITTED ────►┘
                     │
               merge │
                     ▼
                  MERGED
```

- `ACTIVE`：工作空间存在，可以继续执行
- `BLOCKED`：worker 报告了阻塞；一旦确认 `resolve` 收件箱消息就返回 `ACTIVE`，或被丢弃
- `COMMITTED`：worker 提交了提交；一旦确认 `revise` 收件箱消息就返回 `ACTIVE`，或被合并，或被丢弃
- `MERGED`：提交通过合并管道并被整合
- `DISCARDED`：worker 被最终化而没有合并

所有非终结状态转换都属于 worker：它在发出 `local report` 时写入 `BLOCKED`，在发出 `local commit` 时写入 `COMMITTED`，在 `local ack` `task`、`resolve` 或 `revise` 收件箱消息时写入 `ACTIVE`。协调器在解决和修改弧线中的部分是将收件箱消息发布；转换仅在 worker 确认时触发。协调器仅通过 `worker merge` 写入 `MERGED` 和通过 `worker discard` 写入 `DISCARDED` 来最终化 worker 的状态。Worker 一旦最终化就不能更新其条目。

协调器也可以在 worker 是 `ACTIVE` 时发布 `hint`。Hint 是建议性的而非转换性的：它携带新信息或要求 worker 采取后续行动（如报告阻塞），但发布或确认 hint 本身不会改变生命周期状态。

`worker create` 和 `worker resolve` 可以在自己的执行之前自动转发竞标组，但仅当上述完整的前向证明成功时。自动转发保持 worker 生命周期状态不变。如果证明失败，Multorum 保持组不变，并将用户指向手动 `perspective forward`。

对于故意不产生代码差异的纯分析任务，worker 仍应通过正常的提交/合并路径提交：创建一个空提交（例如 `git commit --allow-empty`），然后用 `local commit` 发布它，并在 `body.md` 和可选的 artifacts 中附加证据。协调器可以正常合并该提交，保留可审核的审计跟踪和明确的生命周期完成。

一旦竞标组中的一个 worker 达到 `MERGED`，该组中的每个兄弟 worker 变为 `DISCARDED`。

`delete` 不是生命周期转换。它删除 worktree 和 worker 的状态文件。如果该 worker 是其视角的最后一个成员，它也删除组的 state 文件。

`perspective forward` 也不是生命周期转换。它将活跃竞标组重新固定到 HEAD，同时保持 worker 状态不变。

### 转换

| 从 | 到 | 触发器 |
|---|---|---|
| *(创建)* | ACTIVE | worktree 和运行时 surface 具体化 |
| ACTIVE | BLOCKED | worker 发出 `report` |
| ACTIVE | COMMITTED | worker 发出 `commit` |
| ACTIVE | DISCARDED | 协调器发出 `discard` |
| ACTIVE | ACTIVE | 协调器发布 `hint` |
| BLOCKED | ACTIVE | worker 确认 `resolve` |
| BLOCKED | DISCARDED | 协调器发出 `discard` |
| COMMITTED | ACTIVE | worker 确认 `revise` |
| COMMITTED | MERGED | 协调器发出 `merge` 且检查通过 |
| COMMITTED | DISCARDED | 协调器发出 `discard` |

---

## 邮箱协议

所有协调器-worker 通信都是基于文件的。每个 worker 在其 `.multorum/` 目录中暴露两个邮箱树：

- `inbox/`：从协调器到 worker 的消息
- `outbox/`：从 worker 到协调器的消息

### 消息 Bundle

每条消息都是一个 bundle（见 [Bundle](#bundle)），扩展了一个 `envelope.toml`，携带邮箱路由元数据。信封是 Multorum 在邮箱 bundle 内部唯一解析的文件。

```
<mailbox>/new/<sequence>-<kind>/
  envelope.toml    # 机器可读的路由元数据
  body.md          # 主内容（始终存在，可能为空）
  artifacts/       # 可选的辅助文件
```

`envelope.toml` 字段：

```toml
protocol    = "multorum/v1"
worker      = "my-worker"        # 作者运行时身份
perspective = "AuthImplementor"  # 作者视角
kind        = "report"           # 消息分类
sequence    = 7                  # 每个作者的单调计数器
created_at  = "2026-03-24T10:00:00Z"
in_reply_to = 5                  # 可选，用于关联
head_commit = "a1b2c3d"          # 可选，用于提交类型
```

`kind` 字段分类消息：

- `task` — 协调器为 worker 分配或更新任务
- `hint` — 协调器向活跃 worker 发送建议性后续上下文
- `report` — worker 报告阻塞，转换 worker 为 `BLOCKED`
- `commit` — worker 提交完成的工作，转换 worker 为 `COMMITTED`
- `resolve` — 协调器解决阻塞
- `revise` — 协调器请求修改提交

邮箱 bundle 以原子方式发布：Multorum 写入 `new/` 内的临时名称，然后重命名到位。读者看到的是完整的 bundle 或什么都没有。序列号由作者在发布时分配，永不重用。

发布的 bundle 是不可变的。回执通过在与对应的 `ack/` 目录中写入具有相同序列号的确认文件来记录。所有交换中的唯一运行时身份是 worker，而不是视角名称。

### 所有权和确认

每个邮箱子树只有一个写入者：

- 协调器写入 `inbox/new/`
- worker 写入 `inbox/ack/`
- worker 写入 `outbox/new/`
- 协调器写入 `outbox/ack/`

---

## 合并管道

在 worker 的提交到达规范代码库之前，它必须通过两道关卡。

### 范围强制执行

Multorum 验证每个被修改的文件都在 worker 的编译写入集内。此检查不能跳过、放弃或覆盖。它是写入所有权的权威强制执行点。

故意为空的 worker 提交是有效的：它不修改任何文件，因此范围强制执行通过，空的已更改文件集。这支持以 bundle 内容而非代码差异携带证据的纯分析合并。

Worker worktree 中的客户端钩子仅作为早期警告；合并时的范围强制执行是权威的。

### 项目检查

范围强制执行通过后，Multorum 按顺序运行 `[check.pipeline]` 中声明的检查。这些可以是构建、测试、linters、格式检查或任何其他命令。

### 证据

Worker 可以随报告或提交一起提供证据，以支持合并的理由，或要求协调器跳过 `skippable` 检查。证据应包括实际输出或分析，而不仅仅是声明——当 worker 希望协调器做出判断时，失败的证据仍然是有效的。Multorum 携带证据但不判断；由协调器决定是否信任它。

### 审计

合并成功后，Multorum 将审计条目写入 `.multorum/audit/<worker>-<head-prefix6>/entry.toml`。条目记录 worker、视角、基线提交、整合的 head 提交、更改的文件、运行的检查、跳过的检查以及协调器的理由。理由是一个 bundle，通过 `--body-text` 或 `--body-path` 附加到 `merge` 命令，加上任何 `--artifact` 标志。Multorum 将理由文件写入 `.multorum/audit/<worker>-<head-prefix6>/`（见 [Bundle](#bundle)）。

审计理由应该是自包含的。在审计 bundle body 和 artifacts 中记录实际发现，而不是引用 worker outbox 路径，因为 worker worktree 和 outbox 是运行时状态，可能在合并确认后删除。

---

## MCP Surface

Multorum 通过 MCP 协议暴露运行时模型作为传输投影，而不是作为独立的真相来源。文件系统支持的运行时保持权威。

高级角色指导与二进制文件一起打包。CLI 通过 `multorum methodology <role>` 打印该指导，每个 MCP 服务器通过角色特定的 `methodology` 资源暴露相同的 Markdown。仓库本地的 skill 文件可能作为薄包装器存在，但它们不是第二个文档来源。

### 服务器模式

MCP surface 分为两个 stdio 服务器：

- orchestrator 模式
- worker 模式

每种模式仅暴露对该运行时角色有意义的工具和资源。

两个服务器都默认为进程启动时的工作目录。如果 `cwd` 是有效的工作空间或 worktree，运行时立即可用。否则，启动失败推迟到第一次工具或资源调用。`set_working_directory` 工具允许客户端随时将运行时重新绑定到不同目录。

### 工具

MCP 工具镜像显式的运行时指令。其参数在协议模式中类型化，因此主机可以正确验证和渲染它们：

- 字符串用于标识符、路径和提交引用
- 整数用于邮箱序列号
- 布尔值用于显式标志
- 字符串数组用于重复的路径或检查参数

工具结果是 JSON payload。运行时失败保持为工具级失败，而不是协议传输失败。

### 资源

MCP 资源暴露运行时状态的只读投影。

大多数资源返回 JSON 快照。角色方法论资源返回 Markdown，因为它们是供代理或人类直接使用的建议性操作指南。

#### 协调器模式资源

具体的：

| URI | 描述 |
|---|---|
| `multorum://orchestrator/methodology` | 与 Multorum 打包的高级协调器操作方法论。 |
| `multorum://orchestrator/status` | 完整协调器快照：活跃的视角和 worker。 |
| `multorum://orchestrator/perspectives` | 从当前规则手册编译的视角摘要。 |
| `multorum://orchestrator/workers` | 当前运行时的 worker 摘要列表。 |

模板：

| URI 模板 | 描述 |
|---|---|
| `multorum://orchestrator/workers/{worker}` | 一个 worker 的详细协调器端视图。 |
| `multorum://orchestrator/workers/{worker}/outbox` | 一个 worker 的发件箱邮箱列表。 |

#### Worker 模式资源

具体的：

| URI | 描述 |
|---|---|
| `multorum://worker/methodology` | 与 Multorum 打包的高级 worker 操作方法论。 |
| `multorum://worker/contract` | 活跃视角的不可变 worker 契约。 |
| `multorum://worker/inbox` | 活跃 worker 的收件箱邮箱列表。 |
| `multorum://worker/status` | 预计的 worker 生命周期状态。 |

Worker 模式资源不携带 worker 身份参数，因为服务器通过 `set_working_directory` 绑定到单个 worker worktree——身份是隐式的。

### 错误契约

MCP 可见的错误代码是稳定的协议值，与 Rust 枚举变体名称无关。工具级失败和资源读取失败应在可能的情况下保留底层域类别，例如区分无效参数与缺失的运行时对象。

---

## 指令参考

本节列出协调器和 worker 可能发出的指令，以 CLI 命令的形式。MCP 工具镜像相同的运行时操作并带有类型化参数。

### 初始化

- `multorum init` — 初始化 `.multorum/`，如果不存在则写入默认已提交的文件，准备 `.multorum/.gitignore`，并创建协调器运行时目录。

### 视角

- `multorum perspective list` — 从当前规则手册列出视角。
- `multorum perspective validate <perspectives>...` — 从当前规则手册编译命名视角，检查它们之间的冲突，并检查它们与活跃竞标组的冲突。使用 `--no-live` 时，仅检查命名的视角相互之间的冲突。
- `multorum perspective forward <perspective>` — 将 `perspective` 的整个活跃竞标组移动到 HEAD。从当前规则手册重新编译视角边界。除非该竞标组中的每个活跃 worker 都是非 `ACTIVE` 状态且重新编译的边界是当前具体化边界的超集，否则被拒绝。进度仅保留自每个 worker 已记录的持久检查点：对于 `BLOCKED` worker 是最新的阻塞 `report`，对于 `COMMITTED` worker 是提交的 head 提交。不是生命周期转换。

### 协调器 Worker 命令

每个 bundle 发布指令恰好需要一个 body 来源：`--body-text` 或 `--body-path`。Artifacts 保持可选。

- `multorum worker create <perspective> [--worker <worker>] [--overwriting-worktree] [--no-auto-forward] (--body-text <text> | --body-path <file>) [--artifact <file>]...` — 从当前规则手册针对工作树编译视角边界。如果该视角已存在竞标组，则加入它。否则，用基线提交设置为 HEAD 的新组，并检查与所有活跃竞标组的冲突。在创建 worker 之前，当完整前向证明成功时，Multorum 可能自动转发同一视角的现有活跃竞标组；`--no-auto-forward` 禁用该便利，使转发手动。创建 worker worktree 并具体化运行时 surface，始终创建初始 `task` 收件箱 bundle；必需的 body 填充该 bundle 的主内容和可选的 artifacts 添加支持文件。`--worker` 设置显式 worker 身份；当省略时，Multorum 从视角名称派生一个。重用一个显式 worker id 仅在该 worker 已最终化后才允许；如果其最终化的工作空间仍然存在，传递 `--overwriting-worktree` 来替换它。转换：新 worker 进入 `ACTIVE`。
- `multorum worker list` — 列出活跃 worker。
- `multorum worker show <worker>` — 返回一个 worker 的详细信息。
- `multorum worker outbox <worker> [--from <sequence>] [--to <sequence>] [--exact <sequence>]` — 列出 worker 发送给协调器的消息。`--from`/`--to` 定义一个包含范围；`--exact` 按序列号选择一条消息（与范围互斥）。不是生命周期转换。
- `multorum worker inbox <worker> [--from <sequence>] [--to <sequence>] [--exact <sequence>]` — 列出协调器发送给 worker 的消息。与 `outbox` 相同的过滤语义。不是生命周期转换。
- `multorum worker ack <worker> <sequence>` — 记录协调器对一个 worker outbox bundle 的回执。不是生命周期转换。
- `multorum worker hint <worker> [--reply-to <sequence>] (--body-text <text> | --body-path <file>) [--artifact <file>]...` — 向活跃 worker 收件箱发布一个 `hint` bundle。`--reply-to` 将 hint 与较早的 outbox 序列号关联。必需的 body 携带新项目信息或要求 worker 通过发出 `report` 优雅停止。不是生命周期转换。
- `multorum worker resolve <worker> [--no-auto-forward] [--reply-to <sequence>] (--body-text <text> | --body-path <file>) [--artifact <file>]...` — 向被阻塞的 worker 收件箱发布一个 `resolve` bundle。`--reply-to` 将解决与较早的 outbox 序列号关联。在发布 bundle 之前，当完整前向证明成功时，Multorum 可能自动转发 worker 的活跃竞标组；`--no-auto-forward` 禁用该便利，使转发手动。必需的 body 携带解决上下文给 worker。当 worker 确认该收件箱消息时返回 `ACTIVE`。
- `multorum worker revise <worker> [--reply-to <sequence>] (--body-text <text> | --body-path <file>) [--artifact <file>]...` — 向已提交的 worker 收件箱发布一个 `revise` bundle。`--reply-to` 将修改与较早的 outbox 序列号关联。必需的 body 携带修改上下文给 worker。当 worker 确认该收件箱消息时返回 `ACTIVE`。
- `multorum worker merge <worker> [--skip-check <check>]... (--body-text <text> | --body-path <file>) [--artifact <file>]...` — 验证提交的 head 提交，强制执行写入集，运行合并管道，如果检查通过则整合 worker。必需的 body 附加审计理由；此理由应包含自包含的发现，而不是引用 worker outbox 路径。转换：`COMMITTED` 到 `MERGED`。
- `multorum worker discard <worker>` — 不经整合最终化一个 worker。允许从 `ACTIVE`、`BLOCKED` 或 `COMMITTED`。转换：worker 进入 `DISCARDED`。工作空间保持直到删除。
- `multorum worker delete <worker>` — 删除 worktree 并移除 `worker/<worker>.toml`。如果该 worker 是其竞标组的最后一个成员，也删除 `group/<Perspective>.toml`。仅允许从 `MERGED` 或 `DISCARDED`。

### Worker 本地命令

- `multorum local contract` — 加载当前 worktree 的 worker 契约。
- `multorum local status` — 返回当前 worktree 的预计状态。
- `multorum local inbox [--from <sequence>] [--to <sequence>] [--exact <sequence>]` — 列出协调器发送给此 worker 的消息。`--from`/`--to` 定义一个包含范围；`--exact` 按序列号选择一条消息（与范围互斥）。不是生命周期转换。
- `multorum local outbox [--from <sequence>] [--to <sequence>] [--exact <sequence>]` — 列出此 worker 发送给协调器的消息。与 `inbox` 相同的过滤语义。不是生命周期转换。
- `multorum local ack <sequence>` — 确认一条收件箱消息。确认 `task`、`resolve` 或 `revise` 将 worker 转换为 `ACTIVE`。
- `multorum local report [--head-commit <commit>] [--reply-to <sequence>] (--body-text <text> | --body-path <file>) [--artifact <file>]...` — 从当前 worktree 发布一个阻塞报告。`--reply-to` 将报告与较早的收件箱序列号关联。必需的 body 携带阻塞详情，可选 artifacts 携带证据。转换：`ACTIVE` 到 `BLOCKED`。
- `multorum local commit --head-commit <commit> (--body-text <text> | --body-path <file>) [--artifact <file>]...` — 从当前 worktree 发布一个完成的 worker 提交。必需的 body 携带提交证据或结论。对于没有代码差异的纯分析结果，提交一个故意为空的提交（`git commit --allow-empty`）并发布该 `head_commit`。转换：`ACTIVE` 到 `COMMITTED`。

### 查询

- `multorum status` — 返回完整协调器状态快照，包括活跃 worker 和竞标组成员资格。

### 工具

- `multorum methodology orchestrator` — 将高级协调器方法论打印为 Markdown。此命令是自包含的，不需要托管的仓库。
- `multorum methodology worker` — 将高级 worker 方法论打印为 Markdown。此命令是自包含的，不需要托管的仓库。
- `multorum util completion <shell>` — 向 stdout 发出 shell 补全。支持 shells：`bash`、`zsh`、`fish`、`elvish`、`powershell`。

运行命令后，在你的 shell profile 中 source 输出以启用 tab 补全。

```bash
# bash
source <(multorum util completion bash)

# zsh
autoload -U compinit
compinit
source <(multorum util completion zsh)

# fish
multorum util completion fish | source
```

### MCP 服务器

- `multorum serve orchestrator` — 在 stdio 上启动协调器 MCP 服务器。默认为进程工作目录；客户端可以调用 `set_working_directory` 重新绑定。
- `multorum serve worker` — 在 stdio 上启动 worker MCP 服务器。默认为进程工作目录；客户端可以调用 `set_working_directory` 重新绑定。
