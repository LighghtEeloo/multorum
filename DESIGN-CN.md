---
本文档由LLM协助翻译。虽已尽力而为，但若有不准确之处，请以英文版本为准。
翻译时，遇到代码或图表时，请保持原文不变，并尽可能在上下文中解释它们的含义和用途。
以下是术语表。在第一次出现或语境适合时，术语会以斜体显示，并在括号中提供原文：
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

# 墨缇斯项目：架构参考

## 目录

1. [介绍](#介绍)
2. [核心模型](#核心模型)
3. [指导意见](#指导意见)
4. [工作区模型](#工作区模型)
5. [工蜂生命周期](#工蜂生命周期)
6. [邮箱协议](#邮箱协议)
7. [验收管道](#验收管道)
8. [MCP 接口](#mcp-接口)
9. [指令参考](#指令参考)

---

## 介绍

*墨缇斯*（Multorum）管理同一代码库上的多个并行视角。它面向*野蜂飞舞*（orchestration）式的协调开发工作流而设计：一个称为*女王蜂*（orchestrator）的协调*代行者*（agent）将目标分解为任务，再分配给隔离的*工蜂*（worker）。每只工蜂在自己的*工作区*（workspace）中运行，能看到整个代码库以供执行和分析，但只能修改策略所声明的文件。

该系统的存在是为了解决并行开发中的一个具体矛盾：

- 工蜂需要隔离，以免彼此干扰
- 工蜂需要完整的代码库上下文，使其代码、测试和工具仍然有意义

墨缇斯通过将创作范围与执行范围分离来解决这个问题。工蜂只能在其声明的*写文件集合*（write set）内写入，但可以面向整个代码库进行编译、测试和导航。

墨缇斯是基础设施，不是代行者。它强制执行*不变条件*（invariant），实例化工蜂环境，并记录状态转换。所有协调智能都留在女王蜂中，每次状态转换都只因女王蜂或工蜂发出了明确的指令。

有一个规范的代码库处于版本控制之下。工蜂从不直接修改它。所有变更都通过墨缇斯的*验收*（merge）管道流转，然后由女王蜂整合。

---

## 核心模型

### 女王蜂

女王蜂是唯一的协调权威。它可以是人、LLM 或混合体。其职责是：

- 将开发目标分解为任务
- 声明定义所有权边界的*指导意见*（rulebook）
- 创建、*修订*（revise）、验收、*废弃*（discard）和删除工蜂
- 接收工蜂*报告*（report）并*解决*（resolve）阻塞
- 随时间演进指导意见

通信拓扑是严格的星形：

```
      女王蜂
   /    |    \
  /     |     \
工蜂甲 工蜂乙 工蜂丙
```

工蜂之间从不直接通信。

### 指导意见、切入点和工蜂

指导意见是项目对所有权边界的声明。它定义了命名的文件集、*切入点*（perspective）和验收时的检查流程。

切入点是指导意见中的一个命名角色。它声明：

- 写文件集合：该角色的工蜂可以修改的文件
- *读文件集合*（read set）：该角色活跃时必须保持稳定的文件

任一集合都可以为空（省略或设为 `""`），表示该切入点不对该角色声明文件。写文件集合为空的切入点不能修改任何文件。读文件集合为空的切入点对代码库的其余部分没有稳定性约束。

写文件集合是已有文件的封闭列表。工蜂不得在其外写入或创建文件。当受阻的工蜂发现任务确实需要一个新文件时，女王蜂必须更新规范工作区和指导意见，然后将受阻的*候选组*（candidate group）*前移*（forward）到 HEAD，再解决阻塞。读文件集合声明哪些文件不得被其他并发工作改动，并告知工蜂女王蜂认为哪些是稳定上下文。无论读文件集合如何，工蜂都可以读取代码库中的任何文件。

工蜂是切入点的运行时实例。切入点是静态策略。工蜂是有状态的短暂执行。

### 候选组

当女王蜂为某个切入点创建第一只工蜂时，形成一个候选组。该组的基线*提交*（commit）设置为创建时的 HEAD，其编译后的边界是在该快照上求值的切入点。后续为同一切入点创建的工蜂加入已有的组，共享其基线提交和边界。

如果女王蜂想为已有活跃候选组的切入点设置新的基线，必须先完全验收或废弃已有的组，或通过 `perspective forward` 将其前移到 HEAD。

一个候选组中只有一只工蜂可以被验收。一旦一个成员被验收，其余成员被废弃。

### 无冲突不变条件

核心正确性不变条件是：

> **一个文件要么只被恰好一个活跃候选组写入，要么被任意数量的活跃候选组读取，但不可兼得。**

对于任意两个不同的活跃候选组 G 和 H：

- `write(G) ∩ write(H) = ∅`
- `write(G) ∩ read(H) = ∅`
- `read(G) ∩ write(H) = ∅`

在同一候选组内，每只工蜂的边界相同。冲突检测在候选组层面进行，而非切入点名称层面：切入点描述策略，候选组是必须互不干扰的并发运行时实体。

不变条件延伸到规范分支。当任何候选组活跃时，所有活跃组的读文件集合与写文件集合的并集形成**女王蜂排除集**——在相关工蜂被验收或废弃之前，女王蜂不得提交的文件集。女王蜂只能自由提交排除集以外的文件。

墨缇斯在工蜂创建时强制执行无冲突不变条件。该不变条件是活跃候选组的运行时属性，而非指导意见的静态属性——同一组切入点在给定的仓库状态下可能冲突也可能不冲突，取决于其 glob 匹配了哪些文件。

### 切入点验证

女王蜂可以在创建工蜂之前检查一组切入点是否满足无冲突不变条件。`perspective validate` 从当前指导意见编译命名的切入点，检查它们之间的冲突，并检查它们与活跃候选组的冲突。使用 `--no-live` 时，检查仅覆盖命名的切入点，忽略活跃组。

### 切入点前移

`perspective forward` 将一个活跃候选组从当前基线提交移动到 HEAD，从当前指导意见重新编译切入点边界。

重新编译的边界必须是组当前实例化边界的超集，读文件集合和写文件集合各自独立判断。允许边界扩展。拒绝边界缩减，因为这会破坏创建活跃工蜂时所依据的契约。

在移动任何*工作树*（worktree）之前，墨缇斯验证整个活跃候选组：每只活跃工蜂必须是非*工作中*（`ACTIVE`）状态，必须有持久的重放检查点，并且在该检查点时仍然干净。然后逐个前移工作树。如果后续工蜂的前移失败，墨缇斯回滚所有已移动的工蜂，不保存新的组基线或边界。因此原子性边界是持久化的运行时状态，而非单个 Git 操作。

自动前移对已含"在当前 HEAD 下继续该切入点"之意的女王蜂操作应用相同的步骤。墨缇斯只有在以常规 `perspective forward` 规则证明整个活跃候选组可以成功前移后，才能执行自动前移。

自动前移仅在其效果等同于女王蜂先运行 `perspective forward <perspective>` 再重试原始命令时才有效。当该证明不可得时，墨缇斯保持组不变，并告知用户若仍想移动该组，需显式运行 `multorum perspective forward <perspective>`。

规则是：

- 它处理一个切入点的整个活跃候选组，而非单独的某只工蜂
- 除非该候选组中的每只活跃工蜂都是非工作中状态，否则被拒绝
- 它仅保留每只工蜂已记录的持久检查点的进度：对于*停滞*（`BLOCKED`）的工蜂是最新的阻塞报告，对于*已提交*（`COMMITTED`）的工蜂是已提交的 head commit
- 它拒绝脏的或已漂移的工作树，而非尝试自行恢复
- 它保持每只前移后的工蜂处于其当前的非工作中状态；停滞的工蜂仍需解决，已提交的工蜂仍需修订、验收或废弃
- 每次成功的自动前移都向调用者宣告

---

## 指导意见

指导意见位于 `.multorum/rulebook.toml`，与其管辖的代码库一起提交到版本控制中。不过，指导意见是一种不绑定到特定版本的短暂声明，在某种程度上更像一种关于项目结构与布局、以及被驾驭的生产如何测试与验证的便捷速记。

### 文件集代数

墨缇斯通过一个小型的命名文件集代数来描述所有权边界，为项目提供一套稳定的词汇来描述仓库的各个区域。

#### 语法

```text
path  ::= <glob pattern>              例如 "src/auth/**", "**/*.spec.md"
name  ::= <identifier>                例如 AuthFiles, SpecFiles
expr  ::= name                        引用
        | expr "|" expr               并集
        | expr "&" expr               交集
        | expr "-" expr               差集
        | "(" expr ")"                分组

definition ::= name ".path" "=" path  原语 - 将名称绑定到 glob
             | name "=" expr          复合 - 将名称绑定到表达式
```

`A | B` 产生任一集中的所有文件。`A & B` 仅保留两个集中都存在的文件。`A - B` 保留在 A 中但不在 B 中的文件。优先级是平坦的；分组重要时使用括号。

文件集名称和切入点名称使用 CamelCase。工蜂 id 使用 kebab-case。

#### 命名定义

名称在 `[fileset]` 表中定义。名称可以通过 `.path` 绑定原语路径，也可以通过引用其他名称的复合表达式绑定。切入点在其 `read` 和 `write` 字段中引用这些名称。

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

此例使用交集来划出交叉子集，使用差集来划分所有权。`AuthImplementor` 写生产代码，`AuthTester` 写测试，它们的写文件集合互不相交，因此可以并发运行。

#### 编译与验证

文件集表达式仅是指导意见层面的语法。当墨缇斯需要具体边界时——在工蜂创建、切入点验证或切入点前移时——它通过在工作树上展开 glob 并执行集合运算，将表达式编译为具体的文件列表。

编译时验证检查：

- 文件集定义中没有循环
- 没有未定义的引用
- 空集是允许的，但会产生警告

编译证明指导意见在结构上是有效的。它不证明新工蜂能与已活跃的工蜂并发运行——该检查在工蜂创建时发生。

### 验收检查流程

指导意见声明项目特定的验收管道：

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
- `skippable`：如果女王蜂接受所提交的证据，则可跳过

写文件集合的范围检查始终是强制性的，不可配置。

### 默认模板

`multorum init` 创建下面所示的精简已提交指导意见模板，并在 `.multorum/orchestrator/` 下准备空的女王蜂运行时脚手架（`group/`、`worker/` 和 `exclusion-set.txt`）：

```toml
# 首先定义共享的文件所有权词汇。
# `Name.path` 绑定一个 glob；`Name = "Expr"` 用 |、& 和 - 组合名称。
[fileset]

# 在 `[perspective.<Name>]` 下为每个切入点添加一张表。
# `write` 命名该切入点可以修改的文件（可选，默认为空）。
# `read` 命名并发工作不得写入的稳定上下文文件（可选，默认为空）。
[perspective]

# 按执行顺序添加预验收关卡。
# 在 `[check.command]` 下添加命令，在 `[check.policy]` 下添加可选的跳过策略。
[check]
pipeline = []
```

### 编写好的指导意见

指导意见成功的标志是各切入点可以并发运行，而女王蜂无需不断调解边界冲突。目标是建立一套文件集和切入点的词汇，自然映射到项目实际进行的工作，而不是与工作对抗的官僚负担。

#### 首先构建文件集词汇

从原语开始。每条原语将一个 glob 绑定到一个名称，该名称以团队已在使用的术语描述仓库的某个区域：`AuthFiles`、`ApiHandlers`、`MigrationScripts`。然后用复合表达式将这些区域切分为与实际工作划分方式相匹配的子集：规范与实现、测试与生产代码。

好的文件集名称读起来像领域词汇。它们描述区域中存放的内容，而非如何使用它。`AuthFiles` 优于 `AuthWorkerScope`，因为同一区域可能以不同角色出现在多个切入点中。

保持原语 glob 足够具体，使其不会在仓库增长时悄然涵盖无关文件。`src/auth/**` 优于 `**/*auth*`，因为后者会匹配 `docs/auth-migration-plan.md` 和任何碰巧包含该子串的内容。

按顺序定义：原语在前，复合在后，按子系统分组。读者应当能从上到下扫描 `[fileset]` 表，无需来回跳转即可理解仓库的所有权图。

#### 围绕并行工作设计切入点

切入点是角色，而非任务。以它授权的工作种类命名，而非以正在处理的特定工单命名。`AuthImplementor` 是一个可在多个任务间复用的角色。`FixLoginBug` 是一次性标签，后来的读者无法从中了解它控制的边界。

每个切入点声明两件事：

- **write**：此角色可以修改的已有文件的封闭集。工蜂不能在其外创建文件。如果任务确实需要新文件，女王蜂必须先创建该文件并更新指导意见，工蜂才能继续。可省略或留空以表示只读切入点。
- **read**：此角色活跃时必须保持稳定的文件。读文件集合告知墨缇斯并发工作不得干扰哪些文件，并告知工蜂女王蜂认为哪些是稳定上下文。工蜂仍可读取整个代码库。可省略或留空表示不需要稳定性保证。

无冲突不变条件在候选组层面运作：对于任意两个不同的活跃组，其写文件集合必须互不相交，且都不能写入对方的读文件集合。设计切入点时，应使你打算并发运行的切入点自然满足这一条件。写文件集合重叠的两个切入点实际上不是并行工作，必须顺序运行。

保持读文件集合窄小。将整个代码库列为读依赖会阻止所有并发写入，这就违背了目的。只纳入工蜂作为稳定上下文真正依赖的文件：规范、接口、共享类型、配置。项目自身的指导意见演示了这一点——切入点读取 `ProjectSurfaceFiles`（清单、文档、入口点）而非整棵树。

#### 分区而非重叠

最有用的指导意见模式是分区：用集合差集将子系统划分为互不相交的写文件集合。设计文档中的示例展示了这一点：

```toml
AuthSpecs = "AuthFiles & SpecFiles"
AuthTests = "AuthFiles & TestFiles"

[perspective.AuthImplementor]
write = "AuthFiles - AuthSpecs - AuthTests"

[perspective.AuthTester]
write = "AuthTests"
```

`AuthImplementor` 写生产认证代码。`AuthTester` 写认证测试。它们的写文件集合由构造保证互不相交，因为前者减去了后者所拥有的部分。两者都读取规范，因此在任一角色活跃时规范保持稳定。

当切入点需要共享对某区域的感知而不写入它时，将共享文件放入两者的读文件集合。当一个切入点产出另一个切入点消费的文件时，消费者读取它们，生产者写入它们——绝不两者同时写入。

#### 为项目配置验收检查流程

*验收检查流程*（check pipeline）是工蜂的提交到达规范代码库之前的最后关卡。按应运行的顺序声明检查。快速、廉价的检查排在前面——格式化、lint——这样像完整测试套件那样昂贵的检查只在已通过基本检查的代码上运行。

仅当女王蜂能从工蜂提交的证据合理判断检查会通过时，才将检查标记为 `skippable`。完整测试套件和全工作区 lint 是常见的候选：变更仅限于一个模块的工蜂可以提交相关测试通过的证据，由女王蜂决定是否信任。格式检查通常不值得跳过，因为它们快速且确定。

强制性的写文件集合范围检查不在管道中声明。它始终首先运行，不可配置。管道仅包含其后的项目自定义检查。

每项声明的检查必须在管道中恰好出现一次，每个管道条目必须有对应的命令，且命令不得为空。这些约束在编译时强制执行。

#### 增量演进指导意见

指导意见提交到版本控制中，与其管辖的代码一起版本化。将其视为活的基础设施，而非一次性配置。

当仓库结构发生变化——出现新模块、子系统重组、所有权边界迁移——更新指导意见以匹配。为新区域添加新的文件集。当职责转移时调整切入点边界。移除不再对应实际工作的文件集和切入点。

墨缇斯没有单独的指导意见激活步骤。编译策略的操作（`perspective list`、`perspective validate`、`worker create` 和 `perspective forward`）在运行时从当前工作树读取 `.multorum/rulebook.toml`。因此磁盘上的指导意见编辑立即影响后续操作，甚至在提交之前。为了可重现的编排决策，应在创建工蜂之前提交指导意见的编辑。活跃的工蜂仍在其固定的快照下运行，只有当女王蜂将候选组前移到 HEAD 时，其实例化的边界才会改变。

为活跃候选组扩展切入点边界时，重新编译的边界必须是当前边界的超集。缩减被拒绝，因为它会破坏创建活跃工蜂时所依据的契约。如果某个切入点需要收缩，先终结其活跃的工蜂。

---

## 工作区模型

### 包裹

*包裹*（bundle）是一个包含 `body.md`（*正文*）主内容文件和 `artifacts/`（*附件*）辅助文件子目录的目录。包裹是墨缇斯在需要原子地存储结构化内容时所使用的共享容器：*邮箱*（mailbox）消息携带一个，*审计*（audit）条目为女王蜂的理由携带一个。

```text
<bundle-directory>/
  body.md          # 主 Markdown 内容
  artifacts/       # 可选的辅助文件
```

正文和附件对墨缇斯是不透明的。运行时从用户提供的载荷中实例化它们，但从不解析其内容。

当载荷按路径提供文件时，墨缇斯消费它们而非复制。成功发布后，运行时将文件移入包裹存储，并负责保留它们。

### 文件系统布局

墨缇斯项目在仓库根目录添加一个 `.multorum/` 目录：

```text
<project-root>/
  .multorum/
    .gitignore          # 已提交 - 忽略运行时目录
    rulebook.toml       # 已提交 - 文件集、切入点、验收检查流程
    audit/              # 已提交 - 追加式验收审计跟踪
    orchestrator/       # 被 gitignore - 女王蜂本地控制面
    tr/                 # 被 gitignore - 托管的工蜂工作树
  src/
  tests/
  ...
```

项目提交 `.multorum/rulebook.toml`、`.multorum/.gitignore` 和 `.multorum/audit/` 的内容。`.multorum/` 下的其他一切都是运行时状态，不随仓库传播。

`.multorum/.gitignore` 内容为：

```text
orchestrator/
tr/
```

墨缇斯在 `multorum init` 期间验证这些条目，缺失时发出警告。

运行时目录名称故意简短。`tr/` 使托管工作树路径保持紧凑，`group/` 和 `worker/` 使女王蜂控制面保持浅层结构，而不必将无关的状态更新强制塞入单一文件。

### 女王蜂运行时接口

女王蜂的控制面位于 `.multorum/orchestrator/`，在 `multorum init` 期间创建：

```text
.multorum/orchestrator/
  group/
    <Perspective>.toml   # 每个切入点一条候选组记录
  worker/
    <worker>.toml        # 每个工蜂 id 一条工蜂记录
  exclusion-set.txt      # 实例化的女王蜂排除集
```

`group/<Perspective>.toml` 存储一个切入点的组级运行时状态：切入点名称、固定的基线提交，以及编译后的边界（作为具体文件列表的读文件集合和写文件集合）。

`worker/<worker>.toml` 存储一只工蜂的工蜂级运行时状态：工蜂 id、所属切入点、生命周期状态、托管的工作树路径，以及适用时已提交的 head commit。

`multorum init` 创建空的 `group/` 和 `worker/` 目录。后续操作按如下方式更新它们：

- `worker create` 形成新组时，以切入点、基线提交（HEAD）和编译后的边界写入 `group/<Perspective>.toml`，然后写入第一条 `worker/<worker>.toml`。
- `worker create` 加入已有组时，仅写入新的 `worker/<worker>.toml`。
- `worker merge` 将选定的工蜂标记为*已验收*（`MERGED`），将兄弟标记为*已废弃*（`DISCARDED`），并清除 `group/<Perspective>.toml` 中的边界，使该组不再贡献排除集。
- `worker discard` 将 `worker/<worker>.toml` 标记为已废弃。如果组内没有剩余的非终结成员，则清除 `group/<Perspective>.toml` 中的边界。
- `worker delete` 删除 `worker/<worker>.toml`。如果这是该切入点的最后一只工蜂，也删除 `group/<Perspective>.toml`。
- `perspective forward` 以新基线提交和重新编译的边界重写 `group/<Perspective>.toml`，然后重写每只被前移工蜂的 `read-set.txt` 和 `write-set.txt` 以匹配。

工蜂也直接更新自己的条目：

- 确认 `task`、`resolve` 或 `revise` 收件箱消息时，将 `worker/<worker>.toml` 更新为工作中。
- `local report` 将 `worker/<worker>.toml` 更新为停滞。
- `local commit` 将 `worker/<worker>.toml` 更新为已提交，并记录已提交的 head commit。

女王蜂仅写入终结状态：已验收和已废弃。一旦条目达到任一终结状态，工蜂必须将其视为只读。

`exclusion-set.txt` 是持久化组和工蜂状态的平铺投影：仍有活跃工蜂的所有组的读文件集合与写文件集合的并集。规范工作区中的预提交钩子读取它，并拒绝触及任何所列文件的提交。墨缇斯在组或工蜂状态变更时重新生成它。当没有组携带边界时，文件为空。

### 审计跟踪

验收审计跟踪位于 `.multorum/audit/`，是 `orchestrator/` 和 `tr/` 的同级目录：

```text
.multorum/audit/
  <audit-entry-id>/
    entry.toml
    body.md
    artifacts/
```

审计条目是已提交的项目历史。它们位于 `orchestrator/` 子树之外，随仓库传播。

每条条目在验收成功时原子写入，包含工蜂、切入点、基线提交、整合的 head commit、变更的文件列表、运行或跳过的检查，以及女王蜂提供的理由。审计条目 id 格式为 `<worker>-<head-prefix6>`，其中 `<head-prefix6>` 是整合的工蜂 head commit 的前六个字符。理由是一个包裹——在验收时由女王蜂附加的正文和可选附件——解释工蜂完成了什么以及为何接受验收。墨缇斯将 `entry.toml` 和理由文件写入同一个 audit-entry-id 目录。审计条目只追加；墨缇斯从不修改或删除它们。

### Git Worktree

每只工蜂的工作区是一个从候选组基线提交创建的 git 工作树：

```text
git worktree add .multorum/tr/<worker> <base-commit>
```

同一候选组中的工蜂共享相同的基线提交，在组内第一只工蜂创建时设置。不同候选组中的工蜂可能有不同的基线提交。

工蜂达到已验收或已废弃后，其身份可被新工蜂复用。复用始终是"在这里创建一只新工蜂"，而非"重新打开旧状态"。复用显式工蜂 id（`--worker <worker>`）且已终结的工作区仍然存在时，`worker create` 需传入 `--overwriting-worktree` 来替换该保留的工作区。如果已终结的工作区已被删除，则复用不需要覆盖标志。

### 工蜂运行时接口

每只工蜂的工作树都有自己的 `.multorum/` 目录，与女王蜂的分开。创建时，墨缇斯实例化：

```text
.multorum/
  rulebook.toml      # 基线提交的快照
  contract.toml      # 工蜂、切入点、基线提交
  read-set.txt       # 编译后的读文件集合
  write-set.txt      # 编译后的写文件集合
  inbox/
    new/
    ack/
  outbox/
    new/
    ack/
```

这些文件仅供运行时使用，绝不可提交。墨缇斯在每个工作树中安装本地忽略规则，使其处于版本控制之外。

---

## 工蜂生命周期

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

- 工作中（`ACTIVE`）：工作区存在，可以继续执行
- 停滞（`BLOCKED`）：工蜂报告了阻塞；确认解决收件箱消息后返回工作中，或被废弃
- 已提交（`COMMITTED`）：工蜂已提交成果；确认修订收件箱消息后返回工作中，或被验收，或被废弃
- 已验收（`MERGED`）：提交通过验收管道并被整合
- 已废弃（`DISCARDED`）：工蜂未经验收即被终结

所有非终结状态转换属于工蜂：它在发出 `local report` 时写入停滞，在发出 `local commit` 时写入已提交，在 `local ack` `task`、`resolve` 或 `revise` 收件箱消息时写入工作中。女王蜂在解决和修订弧中的职责是发布收件箱消息；转换仅在工蜂确认时触发。女王蜂仅通过 `worker merge` 写入已验收和通过 `worker discard` 写入已废弃来终结工蜂的状态。工蜂一旦被终结就不可更新其条目。

女王蜂也可以在工蜂处于工作中时发出 `hint`。Hint 是建议性的而非转换性的：它携带新信息或要求工蜂采取后续行动（如发出报告），但发布或确认 hint 本身不改变生命周期状态。

`worker create` 和 `worker resolve` 可在自身执行之前自动前移候选组，但仅当上述完整前移证明成功时。自动前移保持工蜂生命周期状态不变。如果证明失败，墨缇斯保持组不变，并引导用户执行手动 `perspective forward`。

对于故意不产生代码差异的纯分析任务，工蜂仍应通过正常的提交/验收路径提交：创建一个空提交（例如 `git commit --allow-empty`），然后用 `local commit` 发布，并在正文和可选附件中附上证据。女王蜂可以正常验收该提交，保留可审查的审计跟踪和明确的生命周期完成。

一旦候选组中的一只工蜂达到已验收，该组中的所有兄弟工蜂变为已废弃。

`delete` 不是生命周期转换。它删除工作树和工蜂的状态文件。如果该工蜂是其切入点的最后一个成员，也删除组的状态文件。

`perspective forward` 也不是生命周期转换。它将活跃工蜂全为非工作中状态的候选组重新固定到 HEAD，同时保持工蜂状态不变。

### 转换

| 从 | 到 | 触发 |
|---|---|---|
| *（创建）* | 工作中 | 工作树和运行时接口实例化 |
| 工作中 | 停滞 | 工蜂发出报告 |
| 工作中 | 已提交 | 工蜂发出提交 |
| 工作中 | 已废弃 | 女王蜂发出废弃 |
| 工作中 | 工作中 | 女王蜂发布 `hint` |
| 停滞 | 工作中 | 工蜂确认解决 |
| 停滞 | 已废弃 | 女王蜂发出废弃 |
| 已提交 | 工作中 | 工蜂确认修订 |
| 已提交 | 已验收 | 女王蜂发出验收且检查通过 |
| 已提交 | 已废弃 | 女王蜂发出废弃 |

---

## 邮箱协议

女王蜂与工蜂之间的所有通信都基于文件。每只工蜂在其 `.multorum/` 目录中暴露两棵邮箱树：

- `inbox/`：从女王蜂到工蜂的消息
- `outbox/`：从工蜂到女王蜂的消息

### 消息包裹

每条消息都是一个包裹（见[包裹](#包裹)），附带一个 `envelope.toml` 用于承载邮箱路由元数据。信封是墨缇斯在邮箱包裹内唯一解析的文件。

```
<mailbox>/new/<sequence>-<kind>/
  envelope.toml    # 机器可读的路由元数据
  body.md          # 主内容（始终存在，可能为空）
  artifacts/       # 可选的辅助文件
```

`envelope.toml` 字段：

```toml
protocol    = "multorum/v1"
worker      = "my-worker"        # 发件者运行时身份
perspective = "AuthImplementor"  # 发件者切入点
kind        = "report"           # 消息种类
sequence    = 7                  # 每个发件者的单调计数器
created_at  = "2026-03-24T10:00:00Z"
in_reply_to = 5                  # 可选，用于关联
head_commit = "a1b2c3d"          # 可选，用于提交类消息
```

`kind` 字段对消息进行分类：

- `task` — 女王蜂为工蜂分配或更新任务
- `hint` — 女王蜂向工作中的工蜂发送建议性后续上下文
- `report` — 工蜂报告阻塞，工蜂转为停滞
- `commit` — 工蜂提交完成的工作，工蜂转为已提交
- `resolve` — 女王蜂解决阻塞
- `revise` — 女王蜂请求修订提交

邮箱包裹以原子方式发布：墨缇斯先写入 `new/` 内的临时名称，再重命名到位。读取方看到的要么是完整的包裹，要么什么也看不到。序列号由发件者在发布时分配，永不复用。

已发布的包裹不可变。回执通过在对应的 `ack/` 目录中写入带有相同序列号的确认文件来记录。所有交换中的唯一运行时身份是工蜂，而非切入点名称。

### 所有权与确认

每棵邮箱子树恰有一个写入方：

- 女王蜂写入 `inbox/new/`
- 工蜂写入 `inbox/ack/`
- 工蜂写入 `outbox/new/`
- 女王蜂写入 `outbox/ack/`

---

## 验收管道

在工蜂的提交到达规范代码库之前，它必须通过两道关卡。

### 范围强制

墨缇斯验证每个被修改的文件都在工蜂编译后的写文件集合之内。此检查不可跳过、不可豁免、不可覆盖。它是写入所有权的权威执行点。

故意为空的工蜂提交是有效的：它不修改任何文件，因此范围强制以空的变更文件集通过。这支持以包裹内容（而非代码差异）承载证据的纯分析验收。

工蜂工作树中的客户端钩子仅作为早期警告；验收时的范围强制才是权威的。

### 项目检查

范围强制通过后，墨缇斯按顺序运行 `[check.pipeline]` 中声明的检查。这些可以是构建、测试、lint、格式检查或任何其他命令。

### 证据

工蜂可以随报告或提交一起附上证据，以支持验收的理由或请求女王蜂跳过 `skippable` 检查。证据应包含实际输出或分析，而非仅仅是声明——当工蜂希望女王蜂做出判断时，失败的证据仍然有效。墨缇斯承载证据但不做判断；由女王蜂决定是否信任。

### 审计

验收成功后，墨缇斯将审计条目写入 `.multorum/audit/<worker>-<head-prefix6>/entry.toml`。条目记录工蜂、切入点、基线提交、整合的 head commit、变更的文件、运行的检查、跳过的检查以及女王蜂的理由。理由是一个包裹，通过 `--body-text` 或 `--body-path` 之一附加到 `merge` 命令，加上任何 `--artifact` 标志。墨缇斯将理由文件写入 `.multorum/audit/<worker>-<head-prefix6>/`（见[包裹](#包裹)）。

审计理由应当自包含。在审计包裹正文和附件中记录实际发现，而非引用工蜂发件箱路径，因为工蜂工作树和发件箱是运行时状态，可能在验收确认后被删除。

---

## MCP 接口

墨缇斯通过*模型上下文协议*（MCP）暴露运行时模型作为传输投影，而非独立的真相来源。以文件系统为基础的运行时保持权威地位。

高级角色指导随二进制文件一起打包。CLI 通过 `multorum util methodology <role>` 输出该指导，每个 MCP 服务器通过角色对应的 `methodology` 资源暴露相同的 Markdown。仓库本地的技能文件可作为薄层包装存在，但它们不是第二份文档来源。

### 服务器模式

MCP 接口分为两个 stdio 服务器：

- 女王蜂模式
- 工蜂模式

每种模式仅暴露对该运行时角色有意义的工具和资源。

两个服务器均默认使用启动时的进程工作目录。如果 `cwd` 是有效的工作区或工作树，运行时立即可用。否则，启动失败推迟到首次工具或资源调用时。`set_working_directory` 工具允许客户端随时将运行时重新绑定到另一目录。

### 工具

MCP 工具镜像显式的运行时指令。其参数在协议模式中有类型定义，以便宿主正确验证和渲染：

- 字符串用于标识符、路径和 commit 引用
- 整数用于邮箱序列号
- 布尔值用于显式标志
- 字符串数组用于重复的路径或检查参数

工具结果是 JSON 载荷。运行时失败保持为工具级失败，而非协议传输失败。

### 资源

MCP 资源暴露运行时状态的只读投影。

大多数资源返回 JSON 快照。角色方法论资源返回 Markdown，因为它们是供代行者或人类直接使用的建议性操作指南。

#### 女王蜂模式资源

具体资源：

| URI | 描述 |
|---|---|
| `multorum://orchestrator/methodology` | 随墨缇斯打包的高级女王蜂操作方法论。 |
| `multorum://orchestrator/status` | 完整女王蜂快照：活跃的切入点和工蜂。 |
| `multorum://orchestrator/perspectives` | 从当前指导意见编译的切入点摘要。 |
| `multorum://orchestrator/workers` | 当前运行时的工蜂摘要列表。 |

模板资源：

| URI 模板 | 描述 |
|---|---|
| `multorum://orchestrator/workers/{worker}` | 一只工蜂的女王蜂侧详细视图。 |
| `multorum://orchestrator/workers/{worker}/outbox` | 一只工蜂的发件箱邮箱列表。 |

#### 工蜂模式资源

具体资源：

| URI | 描述 |
|---|---|
| `multorum://worker/methodology` | 随墨缇斯打包的高级工蜂操作方法论。 |
| `multorum://worker/contract` | 当前切入点的不可变工蜂契约。 |
| `multorum://worker/inbox` | 当前工蜂的收件箱邮箱列表。 |
| `multorum://worker/status` | 投影的工蜂生命周期状态。 |

工蜂模式资源不携带工蜂身份参数，因为服务器通过 `set_working_directory` 绑定到单一工蜂工作树——身份是隐含的。

### 错误契约

MCP 可见的错误代码是稳定的协议值，与 Rust 枚举变体名称无关。工具级失败和资源读取失败应尽可能保留底层领域类别，例如区分无效参数与缺失的运行时对象。

---

## 指令参考

本节列出女王蜂和工蜂可发出的指令，以 CLI 命令的形式呈现。MCP 工具镜像相同的运行时操作，带有类型化参数。

### 初始化

- `multorum init` — 初始化 `.multorum/`，若不存在则写入默认的已提交文件，准备 `.multorum/.gitignore`，并创建女王蜂运行时目录。

### 切入点

- `multorum perspective list` — 从当前指导意见列出切入点。
- `multorum perspective validate <perspectives>...` — 从当前指导意见编译命名的切入点，检查它们之间的无冲突性，并检查它们与活跃候选组的冲突。使用 `--no-live` 时，仅检查命名切入点之间的冲突。
- `multorum perspective forward <perspective>` — 将 `perspective` 的整个活跃候选组移至 HEAD。从当前指导意见重新编译切入点边界。除非该候选组中每只活跃工蜂都是非工作中状态且重新编译的边界是当前实例化边界的超集，否则被拒绝。进度仅保留自每只工蜂已记录的持久检查点：对于停滞工蜂是最新的阻塞报告，对于已提交工蜂是已提交的 head commit。非生命周期转换。

### 女王蜂工蜂命令

每条包裹发布指令恰好需要一个正文来源：`--body-text` 或 `--body-path`。附件保持可选。

- `multorum worker create <perspective> [--worker <worker>] [--overwriting-worktree] [--no-auto-forward] (--body-text <text> | --body-path <file>) [--artifact <file>]...` — 从当前指导意见针对工作树编译切入点边界。如果该切入点已有候选组，则加入它。否则，以基线提交设为 HEAD 的新组创建，并检查与所有活跃候选组的无冲突性。在创建工蜂之前，当完整前移证明成功时，墨缇斯可自动前移同一切入点的已有活跃候选组；`--no-auto-forward` 禁用此便利，使前移需手动执行。创建工蜂工作树并实例化运行时接口，始终创建初始 `task` 收件箱包裹；必需的正文填充该包裹的主内容，可选附件添加支持文件。`--worker` 设置显式工蜂身份；省略时，墨缇斯从切入点名称派生。复用显式工蜂 id 仅在该工蜂已终结后才允许；如果其终结后的工作区仍然存在，需传入 `--overwriting-worktree` 来替换。转换：新工蜂进入工作中。
- `multorum worker list` — 列出活跃工蜂。
- `multorum worker show <worker>` — 返回一只工蜂的详细信息。
- `multorum worker outbox <worker> [--from <sequence>] [--to <sequence>] [--exact <sequence>]` — 列出工蜂发送给女王蜂的消息。`--from`/`--to` 定义包含范围；`--exact` 按序列号选择一条消息（与范围互斥）。非生命周期转换。
- `multorum worker inbox <worker> [--from <sequence>] [--to <sequence>] [--exact <sequence>]` — 列出女王蜂发送给工蜂的消息。过滤语义同 `outbox`。非生命周期转换。
- `multorum worker ack <worker> <sequence>` — 记录女王蜂对一条工蜂发件箱包裹的回执。非生命周期转换。
- `multorum worker hint <worker> [--reply-to <sequence>] (--body-text <text> | --body-path <file>) [--artifact <file>]...` — 向工作中工蜂的收件箱发布一个 `hint` 包裹。`--reply-to` 将 hint 与较早的发件箱序列号关联。必需的正文携带新的项目信息或请求工蜂通过发出报告来优雅停止。非生命周期转换。
- `multorum worker resolve <worker> [--no-auto-forward] [--reply-to <sequence>] (--body-text <text> | --body-path <file>) [--artifact <file>]...` — 向停滞工蜂的收件箱发布一个 `resolve` 包裹。`--reply-to` 将解决与较早的发件箱序列号关联。在发布包裹之前，当完整前移证明成功时，墨缇斯可自动前移工蜂的活跃候选组；`--no-auto-forward` 禁用此便利，使前移需手动执行。必需的正文携带解决上下文。工蜂确认该收件箱消息后返回工作中。
- `multorum worker revise <worker> [--reply-to <sequence>] (--body-text <text> | --body-path <file>) [--artifact <file>]...` — 向已提交工蜂的收件箱发布一个 `revise` 包裹。`--reply-to` 将修订与较早的发件箱序列号关联。必需的正文携带修订上下文。工蜂确认该收件箱消息后返回工作中。
- `multorum worker merge <worker> [--skip-check <check>]... (--body-text <text> | --body-path <file>) [--artifact <file>]...` — 验证已提交的 head commit，强制执行写文件集合，运行验收管道，检查通过则整合工蜂。必需的正文附加审计理由；此理由应包含自包含的发现，而非引用工蜂发件箱路径。转换：已提交到已验收。
- `multorum worker discard <worker>` — 不经整合终结工蜂。允许从工作中、停滞或已提交状态。转换：工蜂进入已废弃。工作区保留至删除。
- `multorum worker delete <worker>` — 删除工作树并移除 `worker/<worker>.toml`。如果该工蜂是其候选组的最后一个成员，也删除 `group/<Perspective>.toml`。仅允许从已验收或已废弃状态。

### 工蜂本地命令

- `multorum local contract` — 加载当前工作树的工蜂契约。
- `multorum local status` — 返回当前工作树的投影状态。
- `multorum local inbox [--from <sequence>] [--to <sequence>] [--exact <sequence>]` — 列出女王蜂发送给此工蜂的消息。`--from`/`--to` 定义包含范围；`--exact` 选择一条消息（与范围互斥）。非生命周期转换。
- `multorum local outbox [--from <sequence>] [--to <sequence>] [--exact <sequence>]` — 列出此工蜂发送给女王蜂的消息。过滤语义同 `inbox`。非生命周期转换。
- `multorum local ack <sequence>` — 确认一条收件箱消息。确认 `task`、`resolve` 或 `revise` 使工蜂转为工作中。
- `multorum local report [--head-commit <commit>] [--reply-to <sequence>] (--body-text <text> | --body-path <file>) [--artifact <file>]...` — 从当前工作树发布阻塞报告。`--reply-to` 将报告与较早的收件箱序列号关联。必需的正文携带阻塞详情，可选附件携带证据。转换：工作中到停滞。
- `multorum local commit --head-commit <commit> (--body-text <text> | --body-path <file>) [--artifact <file>]...` — 从当前工作树发布已完成的工蜂提交。必需的正文携带提交证据或结论。对于没有代码差异的纯分析结果，提交一个故意为空的 commit（`git commit --allow-empty`）并发布该 `head_commit`。转换：工作中到已提交。

### 查询

- `multorum status` — 返回完整的女王蜂状态快照，包括活跃工蜂和候选组成员关系。

### 实用工具

- `multorum util methodology orchestrator` — 以 Markdown 输出高级女王蜂方法论。此命令是自包含的，不需要托管的仓库。
- `multorum util methodology worker` — 以 Markdown 输出高级工蜂方法论。此命令是自包含的，不需要托管的仓库。
- `multorum util completion <shell>` — 向 stdout 输出 shell 补全。支持的 shell：`bash`、`zsh`、`fish`、`elvish`、`powershell`。

运行命令后，在 shell profile 中 source 输出以启用 tab 补全。

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

### MCP 服务器

- `multorum serve orchestrator` — 在 stdio 上启动女王蜂 MCP 服务器。默认使用进程工作目录；客户端可调用 `set_working_directory` 重新绑定。
- `multorum serve worker` — 在 stdio 上启动工蜂 MCP 服务器。默认使用进程工作目录；客户端可调用 `set_working_directory` 重新绑定。
