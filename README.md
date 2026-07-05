# ContextVC (`ctx`)

ContextVC 是一个纯开源、Git-native 的 AI coding agent 上下文控制工具。它把项目里的规则、决策、失败经验、操作说明和代码地图保存到仓库内的 `.context/`，再编译成 Claude Code、Cursor、Codex、GitHub Copilot、Gemini、Cline 等工具能直接读取的原生文件。

一句话：**把“agent 该记住什么、什么时候该拦一下、哪些经验需要 review”变成可版本控制、可审查、可复用的项目资产。**

## 解决什么问题

多人或多 agent 写代码时，经常会出现这些问题：

- 每个工具都有自己的记忆或规则文件，互相不同步。
- 规则写在 `AGENTS.md`、`CLAUDE.md`、Cursor Rules 里，改一处忘三处。
- Agent 会反复踩同一个坑，例如再次运行禁用命令、再次改错同一块代码。
- 失败经验只留在对话里，换工具、换机器、换分支后就丢了。
- 规则和经验没有 review 流程，无法判断是谁写入、为什么写入、是否过期。
- CI 只能检查代码，不能检查 agent 上下文是否过期或被手改。

ContextVC 的做法是：仓库内维护一份结构化真相源 `.context/`，所有 agent-facing 文件都由它生成；运行时通过 MCP、hooks 和 git hooks 查询这份真相源。

## 本地优先

- 核心功能只读写当前仓库和本机配置文件。
- 不需要 API key。
- 不上传代码、事件或规则。
- MCP server 是本地 stdio server。
- 所有长期上下文都可以通过 git review。

## 功能概览

| 能力 | 命令 / 文件 | 说明 |
| --- | --- | --- |
| 初始化上下文仓库 | `ctx init` | 创建 `.context/`，生成投影文件和 lockfile |
| 导入现有规则 | `ctx adopt` | 从 `AGENTS.md`、`CLAUDE.md`、`.cursor/rules/*.mdc` 导入对象 |
| 代码地图回填 | `ctx backfill` | 从 git history 生成 codemap 对象 |
| 编译投影 | `ctx render` | 生成各类 agent 原生规则文件，并保留人写区域 |
| CI 健康检查 | `ctx check` | 检查投影漂移、过期绑定、坏 schema、冲突规则、事件日志损坏 |
| 项目简报 | `ctx brief` | 输出给 agent 的简短项目上下文 |
| 运行时检索 | `ctx serve-mcp` | 提供 MCP tools：brief、search、precheck、log、propose、status |
| Hooks 集成 | `ctx install` | 安装 Claude / Cursor / Codex hooks、MCP 配置和 git hooks |
| 动作前拦截 | `ctx precheck` | 按路径或命令检查约束、历史失败和 stale hint |
| 临时静默 gate | `ctx snooze` | 对某个 gate hit 记录本地静默事件 |
| 失败写回 | `ctx log-event` / `ctx hook stop` | 从失败事件蒸馏出 proposal |
| 人审队列 | `ctx review` | proposal 经 accept/reject 后才进入正式对象 |
| 过期校验 | `ctx verify` | 校验对象绑定的文件 hash 或 source hash |
| 语义合并 | `ctx merge` / `ctx harvest` | 持久化语义冲突，约束冲突会阻断 check/render |
| 历史追踪 | `ctx log` / `ctx blame` / `ctx diff` / `ctx revert` | 用 git 语义查看、追踪和回滚上下文对象 |
| 本地诊断 | `ctx doctor` | 检查目录、配置、render.lock、hooks、潜在 secret |
| 格式规范 | `ctx schema` | 输出 OCL object JSON Schema |
| 基准验证 | `ctx repeatbench` | 跑 repeat-failure gate 场景，输出 JSON 或 JSONL |

## 安装

需要 Rust stable 工具链。没有安装 Rust 的话，先按 [rustup](https://rustup.rs/) 官方说明安装。

```bash
git clone https://github.com/HaochengLu/contextvc.git
cd contextvc
cargo install --locked --path .
ctx --help
```

也可以不安装，直接使用 release build：

```bash
cargo build --release
./target/release/ctx --help
```

## 快速开始

进入任意已有项目：

```bash
cd your-project
ctx init --install-hooks
ctx status
ctx check
```

初始化后会生成：

```text
.context/
AGENTS.md
CLAUDE.md
.cursor/rules/*.mdc
.github/copilot-instructions.md
GEMINI.md
.cline/memory-bank/contextvc.md
```

`ctx init --install-hooks` 的副作用包括：

- 创建 `.context/` 目录。
- 写入或合并 agent 投影文件。
- 写入 `.mcp.json` 和 `.claude/.mcp.json`。
- 写入 Claude / Cursor / Codex hook adapter。
- 链式写入 git `pre-commit` 和 `post-merge` hooks。

这些 hook 安装是幂等的。卸载时可以删除对应的 `contextvc-hook.sh`、`.mcp.json` 条目，以及 git hooks 中 `ctx:begin` / `ctx:end` 管理块。

如果项目里已经有 `AGENTS.md`、`CLAUDE.md` 或 Cursor Rules：

```bash
ctx init --skip-adopt
ctx adopt
ctx render --force
ctx check
```

生成一个项目简报：

```bash
ctx brief
ctx brief --task "修改认证模块"
```

从 git history 自动生成代码地图：

```bash
ctx backfill
ctx render --force
ctx check
```

## 核心概念

### `.context/` 是真相源

所有可长期保存的上下文都放在 `.context/objects/` 下，一条知识一个 Markdown 文件。对象类型包括：

- `constraint`：硬约束或软约束，例如禁用某个命令、某目录必须走特定模式。
- `decision`：项目决策，例如服务分层、依赖选择、接口约定。
- `failure`：历史失败经验，例如某命令曾失败、某路径有已知坑。
- `howto`：操作说明，例如如何修改某模块。
- `codemap`：代码地图，例如高变更文件、关键入口。
- `preference`：偏好，例如测试、格式化、提交方式。

对象经过 `ctx render` 后会投影到不同 agent 的原生文件里。

### 人写区和机器区分离

ContextVC 只管理带有 `ctx:begin` / `ctx:end` 标记的 managed block。你可以在生成文件里保留人写说明，`ctx render` 不会覆盖 managed block 之外的内容。

### 先提案，再入库

运行时捕获到失败事件后，ContextVC 不会直接污染正式知识库，而是生成 proposal：

```bash
ctx review list
ctx review accept <id>
ctx review reject <id>
```

accept 后对象进入 `.context/objects/`，随后重新 render；reject 会留下去重记录，避免同一个失败无限重复出现。

### Gate 是确定性的

`ctx precheck` 不调用模型。它只根据结构化对象、scope、binding 和命令 token 匹配做判断：

```bash
ctx precheck --path src/auth/session.rs
ctx precheck --command "npm install"
```

约束可以是：

- `warn`：提示但允许。
- `ask`：要求人工确认。
- `block`：阻断。

静默一个 gate hit：

```bash
ctx snooze <object-id>
```

## 常用工作流

### 让 CI 检查上下文是否健康

```bash
ctx check
```

`ctx check` 会失败于：

- `.context/VERSION` 或 `render.lock` 缺失。
- 对象变了但没有重新 render。
- 投影文件 managed block 被手动改坏。
- 绑定文件 hash 过期。
- 对象 schema 字段非法。
- event JSONL 损坏。
- 约束处于 conflicted 状态。

### 把历史经验写成约束

```bash
mkdir -p .context/objects/constraints
cat > .context/objects/constraints/use-pnpm.md <<'EOF'
---
id: c-usepnpm
type: constraint
title: Use pnpm
scope: ["**"]
status: active
trust: human
confidence: 1.0
evidence: []
bindings:
  - kind: command
    pattern: "npm install"
    enforcement: block
created: init
---

Use `pnpm install`; do not run `npm install` in this repository.
EOF

ctx render --force
ctx check
```

之后 agent 或人运行：

```bash
ctx precheck --command "npm install"
```

会得到 block 结果。

### 接入 MCP

```bash
ctx install mcp
ctx serve-mcp
```

MCP tools：

- `context_brief`
- `context_search`
- `context_precheck`
- `context_log`
- `context_propose`
- `context_status`

`context_log` 会在服务端重新生成 event id、actor 等保留字段，不信任客户端伪造的人类身份。

### 安装 hooks

```bash
ctx install all
```

会安装：

- Claude Code hooks
- Cursor hooks
- Codex hooks
- git `pre-commit`
- git `post-merge`
- `.mcp.json`

pre-action 类 hook 可把 `block` / `ask` 返回给宿主工具。Cursor / Codex 的阻断类 hook 在 ContextVC 不可用时会 fail-closed，避免静默放行。

### 处理分支合并后的上下文冲突

```bash
ctx merge
ctx check
```

语义冲突会写回对象状态：

```yaml
status: conflicted
```

普通对象冲突会 warning；约束冲突会阻断 `ctx check` 和 `ctx render`，直到人处理。

### 运行 RepeatBench

```bash
ctx repeatbench --json
ctx repeatbench --output target/repeatbench-results.jsonl
```

RepeatBench 用 fixture 验证“已知失败是否被 gate 捕获”，也会检查 false-positive action，避免安全命令被误拦。

## 传统记忆管理没有、ContextVC 有的能力

传统记忆管理通常把“记忆”当成一段摘要、一组向量、一份用户偏好，或者某个 agent 私有的历史记录。它能帮助模型想起过去说过什么，但很难把这些记忆变成可验证、可审查、可合并的工程资产。

ContextVC 补上的不是更长的摘要，而是下面这些工程能力：

| 传统记忆管理通常缺少 | ContextVC 的做法 |
| --- | --- |
| 记忆不跟随代码仓库 | 所有正式对象保存在 repo 内 `.context/objects/`，随分支、PR、clone 一起流转 |
| 只能“回忆”，不能“拦截” | `ctx precheck`、MCP 和 hooks 可以在危险命令或敏感路径操作前给出 `warn` / `ask` / `block` |
| 记忆写入缺少审查 | 失败事件先生成 proposal，必须 `ctx review accept` 后才进入正式知识库 |
| 记忆不知道代码是否变了 | file/source binding 记录 hash，`ctx check` 能发现 stale binding |
| 多个工具各写一份规则 | `.context/` 是唯一真相源，`ctx render` 编译到 AGENTS、Claude、Cursor、Copilot、Gemini、Cline 等原生文件 |
| 只能搜历史，不能证明当前投影正确 | `render.lock`、schema 和 managed block drift 都能在 CI 里检查 |
| 分支合并时上下文冲突不清楚 | `ctx merge` / `ctx harvest` 会把语义冲突写回对象；冲突约束会 fail-closed |
| 记忆回滚只能手工删改 | `ctx blame` / `ctx diff` / `ctx revert` 使用 git 语义追踪和回滚上下文对象 |
| agent 客户端可以伪造上下文事件 | MCP `context_log` 在服务端重建保留字段，并做 secret redaction |
| 很难测试“记忆是否真的减少重复失败” | `ctx repeatbench` 用 fixture 验证已知失败是否被 gate 捕获，并检查误拦 |

所以 ContextVC 不是普通 rules 同步器，也不是对话记录仓库。它把 agent memory 变成 repo 级控制平面：能被版本控制、能被 review、能在动作前参与决策、能在 CI 里证明没有漂移。

## 开发和验证

```bash
cargo fmt --all
cargo test --all
cargo build --release
target/release/ctx repeatbench --json --output target/repeatbench-results.jsonl
git diff --check
```

## 常见排错

### `render.lock missing`

```bash
ctx render --force
ctx check
```

### `render.lock stale`

对象已经变化，但投影没有重新生成：

```bash
ctx render --force
ctx check
```

### `content drift`

投影文件 managed block 被手动改动。想保留改动：

```bash
ctx adopt --file AGENTS.md
ctx review list
```

想恢复机器生成区：

```bash
ctx render --force
```

### `stale binding`

对象绑定的文件或 source 变了：

```bash
ctx verify --mark
ctx review list
```

如果变化是预期的，更新对象正文或 binding 后重新 render。

### `conflicted constraint`

约束存在语义冲突，需要人工处理 `.context/objects/constraints/` 中相关对象：

```bash
ctx merge
ctx check
```

release binary smoke：

```bash
root="$(pwd)"
tmp="$(mktemp -d)"
cp -R tests/fixtures/golden-path/. "$tmp"/
git -C "$tmp" init
(cd "$tmp" && "$root/target/release/ctx" init --skip-backfill)
"$root/target/release/ctx" --repo "$tmp" check
```

## 仓库结构

```text
src/                         Rust 实现
tests/integration.rs          端到端测试
tests/fixtures/golden-path/   初始化和 adopt fixture
docs/ocl-v0.md                OCL 格式说明
docs/design.md                架构说明
docs/schema/                  OCL JSON Schema
benchmarks/repeatbench/       RepeatBench 场景和 fixture
```

## 许可证

Apache-2.0。
