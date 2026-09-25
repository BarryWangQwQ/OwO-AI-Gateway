# OwO AI Gateway 命令行使用指南

OwO AI Gateway 是一个运行在本机的模型网关：在一个配置文件里定义好模型提供商、模型和密钥，Codex、Claude Code、Claude Desktop、Cursor 等 AI 编程应用就都能通过它使用这些模型。

所有功能都在一个可执行文件 `owo` 里。

---

## 快速上手

```bash
owo add anthropic         # 1. 添加提供商：提示输入密钥（不回显），自动写好配置
owo start -d              # 2. 在后台启动网关
owo connect claude        # 3. 把应用接到 OwO AI Gateway
```

不确定下一步做什么时，直接运行：

```bash
owo
```

它会显示当前状态，并告诉你下一步：

```text
OwO AI Gateway 0.1.0
config    C:\Users\you\.owo\config.toml
gateway   not running (127.0.0.1:8787)
models    4 available: claude-sonnet-5, claude-opus-5-5, claude-fable-5-1, claude-haiku-4-5
apps      codex            connected (CLI profile + Desktop)
          claude           connected

Next:
  owo start
```

---

## 命令一览

| 命令 | 作用 |
|---|---|
| `owo` | 总览：网关状态、可用模型、密钥问题、已接入的应用、今日用量、下一步建议 |
| `owo add <提供商>` | 添加提供商：存密钥 + 写配置，一步完成 |
| `owo start [-d]` | 运行网关；`-d` 在后台运行 |
| `owo stop` | 停止网关 |
| `owo status [应用]` | 总览，或查看某个应用的接入详情 |
| `owo check` | 不启动网关，检查配置、密钥和模型 |
| `owo usage` | token 用量统计，可按模型、应用、提供商或日期汇总 |
| `owo history [ID]` | 模型调用记录（成功 / 失败）；带 ID 查看单次调用详情 |
| `owo connect <应用>` | 把应用接到 OwO AI Gateway（可撤销） |
| `owo disconnect <应用>` | 撤销接入，恢复应用原来的设置 |
| `owo launch <应用> [参数…]` | 只在这一次运行中让应用使用 OwO AI Gateway，不改应用的任何文件 |
| `owo apps` | 支持的应用及接入状态 |
| `owo mcp` | MCP 服务器：统一定义一次，按应用开关（写入各应用自己的 MCP 配置） |
| `owo skills` | Agent Skills：统一放在 `~/.agents/skills`，从文件夹 / zip / GitHub 安装，按应用开关 |
| `owo models` | OwO AI Gateway 提供的模型 |
| `owo providers` | 已配置的提供商；`presets` 内置预设；`discover` 在线获取模型 |
| `owo key` | 列出配置引用的密钥及状态；`set` / `rm` 存 / 删密钥 |
| `owo config [path \| edit]` | 配置文件位置；用编辑器打开配置 |
| `owo init` | 生成一份带注释的示例配置（手动编辑时用） |

每个命令都可以加 `--help` 查看说明，例如 `owo add --help`。

---

## 添加提供商：`owo add`

```bash
owo add anthropic                          # 用内置预设；提示输入密钥，存进系统钥匙串
owo add deepseek -m deepseek-chat          # 只暴露指定的模型（逗号分隔）
owo add work --preset anthropic --url https://relay.example.com   # 同一预设换个名字，走中转
owo add relay --url https://llm.example.com/v1 -m some-model      # 任意 OpenAI 兼容接口
owo add relay2 --url https://… --adapter anthropic -m some-model  # 任意 Anthropic 兼容接口
owo add deepseek --env DEEPSEEK_API_KEY    # 密钥从环境变量读，不进钥匙串
owo add ollama --no-key -m qwen3:8b        # 本地服务，不需要密钥
```

| 参数 | 含义 |
|---|---|
| `<名字>` | 预设名（`owo providers presets` 查看），或自定义名字（需配合 `--url`） |
| `--preset <预设>` | 以某个预设为基础，但用另一个名字（例如第二个账号、中转） |
| `--url <地址>` | 接口地址：预设的中转地址，或任意兼容接口 |
| `--adapter <协议>` | 自定义接口的协议：`openai-chat`（默认）或 `anthropic` |
| `-m, --models a,b` | 要暴露的模型；不传则用预设自带的模型列表 |
| `--env <变量名>` | 从环境变量读密钥 |
| `--no-key` | 不需要密钥 |
| `-f, --force` | 替换同名的已有提供商 |

- 密钥默认存进系统钥匙串，名字与提供商相同（`keyring:<名字>`）；已经存过就沿用，不会重复询问。
- 写入前会校验整个配置，**不合法就不写**，原文件保持不变；写入会保留文件里的注释和格式。
- 预设没有默认模型时，用 `owo providers discover <名字> --add …` 补上（见“模型与提供商”）。
- 网关运行中添加了提供商，需要重启网关（`owo stop` 后再 `owo start -d`）。

---

## 配置文件

默认位置：

| 系统 | 路径 |
|---|---|
| Windows | `C:\Users\<你>\.owo\config.toml` |
| macOS / Linux | `~/.owo/config.toml` |

`owo config` 显示实际路径，`owo config edit` 用编辑器打开（依次使用 `$VISUAL`、`$EDITOR`，否则用系统默认：Windows 记事本、macOS 文本编辑、Linux `xdg-open`）。

`owo add` 写出的配置大致如下，也可以手动编辑：

```toml
name = "OwO"                       # 可选：应用里显示的提供商 / 账户名（默认 OwO），必须写在所有 [表] 之前

[providers.anthropic]
api_key = "keyring:anthropic"      # 密钥引用，不是密钥本身
models = ["claude-sonnet-5", "claude-opus-5-5", "claude-haiku-4-5"]

[providers.relay]
adapter = "openai-chat"
base_url = "https://llm.example.com/v1"
api_key = "env:RELAY_KEY"
models = ["some-model"]
model_defaults = { price = { input = 1, output = 2 } }   # 可选：这个提供商所有模型的默认价格

# 可选：给模型起显示名、给某个应用起别名、设置价格
[[models]]
id = "claude-sonnet-5"
provider = "anthropic"
display_name = "Claude Sonnet 5"
aliases = { codex = "sonnet" }
price = { input = 3, output = 15, cache_read = 0.3, cache_write = 3.75 }   # 美元 / 百万 token

# 可选：某个应用默认使用的模型（应用名与 `owo connect` 相同）
[clients.codex]
model = "claude-sonnet-5"

[clients.claude]
model = "claude-opus-5-5"
```

要点：

- **`api_key` 推荐写引用**：`keyring:名字`（系统钥匙串，`owo key set 名字` 存入）、`env:变量名`（环境变量）或 `none`（不需要密钥）。
- **也可以直接填密钥**，例如 `api_key = "sk-..."`，能正常使用，但 `owo start`、`owo check` 和 `owo` 总览会给出警告，提醒它以明文保存在配置文件里。OwO AI Gateway 的任何输出（日志、`owo providers`、`owo key`、控制 API）都只显示为 `inline`，不会打印密钥本身。
- **应用名与命令行一致**：`[clients.<应用>]` 和 `aliases = { <应用> = … }` 里只能写 `owo connect` 用的应用名：`codex`、`codex-desktop`、`claude`、`claude-desktop`、`cursor`、`grok`、`opencode`、`mcode`、`zcode`、`copilot`，写错名字会报错。每个应用各自独立，例如 `codex` 和 `codex-desktop` 的默认模型、别名分别设置。`[clients.<应用>]` 里可以写 `model`（默认模型）和 `name`（只改这个应用里显示的名字）。
- **显示名称**：Codex（CLI 和 Desktop）的提供商名、Claude Desktop 配置库里的条目名、Cursor 的模型分组名，以及 OpenCode、ZCode、MiniMax Code 里的提供商名，默认都是 `OwO`。顶层 `name` 统一修改，`[clients.<应用>] name` 单独修改某个应用；改完重新运行一次 `owo connect <应用>` 生效（Cursor 在网关重启时生效）。
- **第三方中转**：提供商下加 `base_url = "..."`；若中转要求 `Authorization: Bearer` 而不是 `x-api-key`，再加 `auth = "bearer"`。
- 手动改完用 `owo check` 检查；网关运行中改了配置，需要重启网关。
- OwO AI Gateway 自己的运行状态（例如 Cursor 是否接入）存在 `~/.owo/state/`，不会写进你的配置文件。

---

## 密钥：`owo key`

```bash
owo key                        # 列出配置引用的每个密钥：谁在用、现在能否读到
owo key set anthropic          # 交互输入，不回显
echo "sk-..." | owo key set x  # 也可以从管道读入（脚本里用）
owo key rm anthropic
```

`owo key` 的输出示例：

```text
KEY                            USED BY                      STATUS
keyring:anthropic              provider anthropic           ok
env:RELAY_KEY                  provider relay               missing — set the environment variable RELAY_KEY
```

密钥保存在系统自带的安全存储里：

| 系统 | 存储位置 |
|---|---|
| Windows | 凭据管理器 |
| macOS | 钥匙串（首次读取时可能弹出授权，选“始终允许”） |
| Linux | Secret Service（gnome-keyring / KWallet） |

**服务器、SSH、WSL、容器**里通常没有可用的钥匙串，请改用环境变量（`owo add <名字> --env <变量名>`，或在配置里写 `api_key = "env:…"`），也可以整体关闭钥匙串：

```toml
[credentials]
backend = "env"
```

运行中的网关会缓存读到的密钥 5 分钟，用 `owo key set` 换了密钥后，最多 5 分钟生效（或重启网关）。

---

## 启动与停止网关：`owo start` / `owo stop`

```bash
owo start                           # 前台运行，Ctrl+C 停止
owo start -d                        # 后台运行，日志写到 ~/.owo/logs/owo.log
owo start --listen 127.0.0.1:9000   # 临时换端口（前台、后台都可用）
owo stop                            # 停止网关（前台、后台启动的都可以）
```

- 默认监听 `127.0.0.1:8787`。
- **接入的应用都依赖网关：网关不运行，应用就连不上。**
- `owo stop` 会让网关正常退出，与按 Ctrl+C 一样做完清理（例如恢复 Cursor 的直连设置）。用 `--listen` 换过端口的网关也能找到。
- 前台运行时，关闭终端窗口、系统注销/关机、服务管理器发出的停止信号也会正常退出并清理；后台运行的网关不受关闭终端影响。
- 只有网关默认输出运行日志；其他命令只显示警告和错误。需要详细日志时加 `-v`（`-vv` 更详细），或设置环境变量 `OWO_LOG`。

---

## 接入应用：`owo connect` / `owo disconnect`

```bash
owo connect                        # 不带参数：列出所有应用
owo connect claude                 # 接入 Claude Code
owo connect codex-desktop -m claude-sonnet-5
owo disconnect claude              # 撤销，恢复原设置
```

### 支持的应用

| 名字 | 应用 | 接入方式 |
|---|---|---|
| `codex` | Codex CLI | 新增配置档 `owo`，用 `codex -p owo` 启动；平时的 `codex` 不受影响 |
| `codex-desktop` | Codex Desktop | 修改 `~/.codex/config.toml`，让 Desktop 默认使用 OwO AI Gateway；与 `codex` 互不影响 |
| `claude` | Claude Code（命令行及 IDE 插件） | 写入 `~/.claude/settings.json` 的 `env`，无需登录 Claude 账号 |
| `claude-desktop` | Claude Desktop | 使用 Desktop 官方的“第三方推理”配置，无需登录 claude.ai |
| `cursor` | Cursor | OwO AI Gateway 模型出现在 Cursor 自己的模型旁边；你的 Cursor 账号照常使用 |
| `grok` | Grok Build | 在 `~/.grok/config.toml` 末尾写入一个带标记的配置块 |
| `opencode` | OpenCode | 写入全局配置 `~/.config/opencode/opencode.json` |
| `mcode` | MiniMax Code | 写入 `~/.minimax/config.yaml` |
| `zcode` | ZCode | 写入 ZCode 的提供商配置 |
| `copilot` | GitHub Copilot 应用 | 只能在应用里手动添加，`owo connect copilot` 会打印要填的内容 |

`mmx`（MiniMax CLI）只能用 `owo launch`，见下一节。

### 通用参数

所有应用含义相同；某个应用用不上的参数会直接报错，不会被忽略。

| 参数 | 含义 | 适用 |
|---|---|---|
| `-m, --model <模型>` | 应用默认使用的模型 | codex、codex-desktop、claude、claude-desktop |
| `-f, --force` | connect：覆盖接入后被改过、或不是 OwO AI Gateway 写的设置（先备份）；disconnect：连你后来改过的值也一起还原 | 除 cursor 外全部 |
| `--dir <目录>` | 应用的配置目录不在默认位置时指定 | codex、codex-desktop、claude、claude-desktop |
| `--remove-ca` | 仅 `disconnect cursor`：同时从系统移除 OwO AI Gateway 的本地证书 | cursor |

不传 `--model` 时，依次使用配置里的 `[clients.<应用>] model`，再退回到第一个可用模型（Claude Code 则用它自己的默认档位）。

### 安全性

- 每次修改应用配置前都会**备份**，并记下被改动项原来的值。
- `disconnect` 时：文件自接入后没被动过，就**逐字节恢复**；被你改过，就只撤掉 OwO AI Gateway 自己的条目，你的其他修改保留。
- OwO AI Gateway 写入的内容被别人改了，或者碰到不是 OwO AI Gateway 写的同名条目，默认拒绝覆盖，需要加 `-f`。
- 不读取、不修改任何应用的登录凭据（如 Codex 的 `auth.json`），不伪造任何账号或订阅权益。

### 各应用补充说明

- **Codex CLI**（`codex`）：接入后用 `codex -p owo` 启动；只新增一个配置档文件，不碰 `config.toml`。
- **Codex Desktop**（`codex-desktop`）：修改 `config.toml`，接入后需重启 Codex Desktop。Desktop 未登录 ChatGPT 时，OwO AI Gateway 会自动把模型放进 Desktop 能显示的模型位置。两个 Codex 接入各自独立：可以只接其中一个，`disconnect` 一个不影响另一个。
- **Claude Code**：接入后直接运行 `claude`。`/model` 里能看到 OwO AI Gateway 的模型；非 Claude 模型会显示为 `claude-owo--<模型>`。
- **Claude Desktop**：接入后**完全退出**（包括托盘图标）再打开。
- **Cursor**：
  - 首次接入会创建一个本地证书，并请求管理员权限把它加入系统信任。Windows 弹出 UAC 确认；macOS 和 Linux 在终端里输入 `sudo` 密码。这个证书只能签发 `*.cursor.sh` 域名。
  - 接入后需要**重启网关**，再完全退出并重新打开 Cursor。
  - 网关停止时，Cursor 会自动恢复直连。
  - 彻底移除用 `owo disconnect cursor --remove-ca`。
- **OpenCode / Grok / MiniMax Code / ZCode**：再次执行 `owo connect <应用>` 会刷新模型列表。
  - Grok Build 会自动重新加载配置。
  - 其他应用需要新开会话或重启。
- **Copilot 应用**：按 `owo connect copilot` 打印的内容，在应用的 Settings → Model providers → Add provider 里添加。

---

## MCP 服务器：`owo mcp`

MCP 服务器在 `config.toml` 的 `[mcp.<名字>]` 里定义一次，再按应用开关：开启时 OwO AI Gateway 把它写进该应用自己的 MCP 配置，关闭时移除。与 `owo connect` 无关，应用不必接入 OwO AI Gateway。

```bash
owo mcp                                        # 列出服务器及每个应用的状态（--json 供程序使用）
owo mcp add github --command npx --arg -y --arg @modelcontextprotocol/server-github \
    --env GITHUB_PERSONAL_ACCESS_TOKEN=keyring:mcp-github-GITHUB_PERSONAL_ACCESS_TOKEN --app claude,cursor,codex
owo mcp add linear --url https://mcp.linear.app/mcp --header "Authorization=env:LINEAR_AUTH" --app cursor
owo mcp add legacy --url http://127.0.0.1:9000/sse --sse --app claude
owo mcp enable github --app opencode           # --app 可写多个（逗号分隔），或 all（所有已安装的应用）
owo mcp disable github --app cursor
owo mcp add github --replace …                 # 修改已有服务器；不给 --app 时保留原来的应用
owo mcp remove github                          # 从所有应用中移除，再从配置删除
owo mcp scan                                   # 各应用配置里已有、但不由 OwO AI Gateway 管理的服务器
owo mcp import cursor fetch                    # 导入并接管；import all <名字> 从所有有它的应用导入；--all 导入全部
owo mcp import all gh --keyring                # 导入时把像密钥的值（TOKEN、KEY、Authorization…）移入系统钥匙串
owo mcp sync                                   # 按配置重写所有应用（手改配置或换了密钥之后）
owo mcp apps                                   # 每个应用的 MCP 配置文件位置、是否已安装
```

配置示例：

```toml
[mcp.github]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-github"]
env = { GITHUB_PERSONAL_ACCESS_TOKEN = "keyring:mcp-github-GITHUB_PERSONAL_ACCESS_TOKEN" }
apps = ["claude", "codex", "cursor"]

[mcp.linear]
url = "https://mcp.linear.app/mcp"            # 默认 streamable HTTP；旧式 SSE 加 transport = "sse"
headers = { Authorization = "env:LINEAR_AUTH" }
apps = ["cursor"]
```

| 应用 | 写入位置 | 支持 |
|---|---|---|
| `codex`（CLI 与 Desktop 共用，`codex-desktop` 等同） | `~/.codex/config.toml` 的 `[mcp_servers.<名字>]` | stdio、HTTP（不支持 SSE） |
| `claude` | `~/.claude.json` 的 `mcpServers` | stdio、HTTP、SSE |
| `claude-desktop` | `claude_desktop_config.json` 的 `mcpServers`（普通模式与第三方推理模式的目录都写） | 仅 stdio |
| `cursor` | `~/.cursor/mcp.json` 的 `mcpServers` | stdio、HTTP、SSE |
| `opencode` | `~/.config/opencode/opencode.json` 的 `mcp` | stdio、HTTP、SSE |
| `grok` | `~/.grok/config.toml` 的 `[mcp_servers.<名字>]` | stdio、HTTP、SSE |
| `mcode` | `~/.minimax/mcp.json` 的 `mcpServers` | stdio、HTTP、SSE |
| `zcode` | `~/.zcode/cli/config.json` 的 `mcp.servers` | stdio、HTTP、SSE |
| `copilot` | `~/.copilot/mcp-config.json` 的 `mcpServers`（Copilot CLI 及基于它的应用） | stdio、HTTP、SSE |

`mmx` 没有 MCP 功能；`cwd`（工作目录）只有 Codex 和 Grok Build 支持。

- **密钥**：`env` 和 `headers` 的值可以写 `keyring:名字` 或 `env:变量名`，只在写进应用配置时才解析（应用需要明文）；OwO AI Gateway 的状态文件只记摘要，不存值。明文值若名字像密钥，会得到警告，列表中也只显示为隐藏。
- **只动自己的条目**：应用里已有的同名服务器、以及 OwO AI Gateway 写入后被别人改过的条目都算冲突，默认不覆盖、不删除，整个应用什么都不写；确认后加 `-f`。失败的应用不会被记为已启用。
- **备份与恢复**：每个文件第一次修改前备份到 `~/.owo/backups/mcp-<应用>/`；最后一个服务器关闭时，文件若只被 OwO AI Gateway 改过就逐字节恢复，否则只移除 OwO AI Gateway 的条目。
- **导入即接管**：`import` 把条目加入 OwO AI Gateway 的列表并由它管理，之后关闭会把它从该应用移除；OwO AI Gateway 没有对应字段的设置（如 `startup_timeout_sec`）会被丢弃，需加 `-f` 确认。
- 未安装的应用会跳过，装好后运行 `owo mcp sync`。

---

## 技能：`owo skills`

技能（Agent Skills）是带 `SKILL.md`（YAML 头含 `name`、`description`）的文件夹。OwO AI Gateway 把它管理的技能统一放在 `~/.agents/skills/<名字>/`，多数应用直接读取这个目录；只读自己目录的应用（Claude Code）则得到一个指向它的链接。

```bash
owo skills                                     # 已安装的技能、本机其他技能、每个应用的状态（--json 供程序使用）
owo skills discover                            # 在内置的 GitHub 仓库里找技能（--refresh 重新获取，--repo 指定仓库）
owo skills install github:anthropics/skills/skills/pdf        # 从 GitHub 安装一个（可加 @分支/标签/提交）
owo skills install D:\skills\my-skill          # 从文件夹安装；也可以是 .zip 或含多个技能的文件夹（--all / --skill 名字）
owo skills install ./pack.zip --app codex,claude               # 只为这些应用启用，其余能关的都关掉（--app none：全关）
owo skills disable pdf --app codex,opencode    # 按应用关闭 / enable 重新开启
owo skills update --check                      # 检查更新；update <名字> / update --all 更新（先备份当前文件）
owo skills adopt my-skill                      # 接管 ~/.agents/skills 里已有、不是 OwO AI Gateway 装的技能（不改动文件）
owo skills remove pdf                          # 移除：文件夹移入备份，删掉 OwO AI Gateway 建的链接和写的关闭设置
owo skills repos add owner/repo/skills@main    # 管理 discover 用的仓库（list / add / remove / reset）
owo skills apps                                # 每个应用怎么加载技能、是否已安装
```

| 应用 | 读取位置 | 开关方式 |
|---|---|---|
| `codex`（CLI 与 Desktop 共用） | `~/.agents/skills` | 关闭 = `~/.codex/config.toml` 里一条 `[[skills.config]] path = ".../SKILL.md"`、`enabled = false` |
| `claude` | 只读 `~/.claude/skills` | 开启 = 在那里建目录联接（Windows）/ 符号链接，指向 `~/.agents/skills/<名字>`；都建不了时复制一份 |
| `cursor` | `~/.agents/skills`（也读 `~/.claude/skills`、`~/.codex/skills`） | 始终开启：没有 OwO AI Gateway 可改的单技能开关 |
| `opencode` | `~/.agents/skills`（也读 `~/.claude/skills`） | 关闭 = `opencode.json` 里 `permission.skill.<名字> = "deny"` |
| `grok` | `~/.agents/skills`（也读 `~/.claude/skills`） | 关闭 = `~/.grok/config.toml` 里 `[skills] disabled = ["<名字>"]` |
| `zcode` | `~/.agents/skills` | 始终开启；在 ZCode 的「设置 → 技能」里单独关闭 |
| `copilot` | `~/.agents/skills` | 关闭 = `~/.copilot/settings.json` 里 `disabledSkills` |
| `mcode`、`claude-desktop` | — | 不支持：MiniMax Code 只从插件加载技能，Claude Desktop 的技能上传到 claude.ai 账户 |

- **只动自己的东西**：不是 OwO AI Gateway 安装的技能只列出、不修改、不删除，`adopt` 之后才由它管理；同名目录、用户自己写的开关条目都不会被覆盖。
- **备份**：移除或更新前，技能文件夹移入 `~/.owo/backups/skills/<名字>-<时间>/`；改应用配置文件前备份到 `~/.owo/backups/skills/config/<应用>/`（第一次的原件另存为 `original-*`）。
- **安全检查**：压缩包里的绝对路径、`..`、盘符一律拒绝，符号链接不解压；单个文件 50 MB、单个技能 100 MB 为上限；名字须是小写字母、数字和单个连字符（1–64 个字符）。
- **重复**：为 Claude Code 开启后，Cursor、OpenCode、Grok Build 也会经 `~/.claude/skills` 看到同一份文件（可能列出两次）；OpenCode 和 Grok 的关闭设置按名字生效，两处都会关掉。
- **GitHub**：不使用令牌；每个仓库一次 API 调用解析提交、一次 codeload 下载，结果缓存在 `~/.owo/state/skills-cache/`。API 额度（每小时 60 次）用完时改为按分支下载并从压缩包注释读出提交。

---

## 临时启动：`owo launch`

只对这一次运行生效，**不修改应用的任何文件**；应用自己的参数原样放在后面：

```bash
owo launch claude                      # Claude Code，通过环境变量接到 OwO AI Gateway
owo launch claude -p "解释这段代码"
owo launch codex                       # Codex，通过 -c 参数传入 OwO AI Gateway 的配置
owo launch codex exec "修复这个测试"
owo launch opencode                    # OpenCode，通过内联配置注入 OwO AI Gateway
owo launch opencode run "你好"
owo launch mmx text chat --model claude-sonnet-5 --message "你好"
owo launch mmx text repl
```

| 应用 | 说明 |
|---|---|
| `claude` | 不改 `~/.claude/settings.json`。如果 settings.json 里自己设了 `ANTHROPIC_BASE_URL`，它会覆盖这次启动，OwO AI Gateway 会提示 |
| `codex` | 不改 `~/.codex` 下的任何文件；模型列表放在 OwO AI Gateway 自己的 `~/.owo/state/codex/` |
| `opencode` | 通过 `OPENCODE_CONFIG_CONTENT` 注入，磁盘上的 OpenCode 配置不变 |
| `mmx` | 只支持 `text chat` / `text repl`，拒绝 `--api-key`、`--base-url`、`--region`；使用只含占位密钥的临时配置目录，不读取 `~/.mmx` 里的登录信息 |

`launch` 适合临时试用；想长期使用，用 `owo connect`。

---

## 查看状态与排查

```bash
owo                    # 总览 + 下一步建议
owo status claude      # 某个应用的接入详情（写了哪些设置、备份在哪）
owo status --json      # 运行中网关的原始状态（JSON）
owo apps               # 所有应用的接入状态
owo check              # 逐项检查配置、每个提供商的密钥、可用模型
owo key                # 只看密钥
```

常见问题：

| 现象 | 处理 |
|---|---|
| 应用报连接失败 | 网关没运行：`owo start -d`（`owo` 看 gateway 一行） |
| `no keyring entry` / `owo key` 显示 missing | `owo key set <名字>`，或设置对应环境变量 |
| Linux 上提示 `no Secret Service is reachable` | 改用 `env:` 引用，或设置 `[credentials] backend = "env"` |
| `model ... is not configured` | 该模型没在配置里：`owo models` 查看，`owo providers discover <提供商> --add <模型>` 添加 |
| `... was changed after OwO AI Gateway wrote it` | 你手动改过 OwO AI Gateway 写入的设置；确认后加 `-f` |
| `provider ... is already in config.toml` | 同名提供商已存在；换个名字，或加 `-f` 替换 |
| Cursor 看不到 OwO AI Gateway 模型 | 重启网关，再完全退出并重新打开 Cursor |
| 某个应用突然用不了 | `owo history --failed` 看最近失败的调用和错误原因 |

---

## 用量统计与调用记录：`owo usage` / `owo history`

网关运行期间，每一次模型调用都会记一条记录，不论来自哪个应用（包括 Cursor）。记录内容：

- 时间、应用、请求的模型和实际路由到的模型、提供商。
- 结果：成功、失败、取消（应用中途断开）。失败时记录错误类型、上游 HTTP 状态码和错误信息。
- 耗时、首个 token 的耗时。
- token 明细：输入（其中缓存命中、缓存写入）、输出（其中推理）。

**不记录提示词和回复内容。** 错误信息写入前会去掉网址里的查询参数，避免把放在网址里的密钥带进记录。

```bash
owo usage                  # 最近 7 天（含今天）按模型汇总
owo usage --days 1         # 只看今天
owo usage --days 30 --by app       # 最近 30 天按应用汇总；还可以 --by provider / --by day
owo history                # 最近 20 次调用
owo history -n 50 --failed # 最近 50 次中失败的
owo history --app claude --model claude-sonnet-5   # 按应用、模型筛选
owo history 42             # 第 42 次调用的完整信息（错误全文、token 明细、request id）
```

`owo usage` 的表格里，INPUT 包含 CACHED（缓存命中的输入），OUTPUT 包含 REASONING（推理 token）；缓存写入、推理这两列只在有数据时出现。`owo` 总览的 Today 一行显示今天的调用次数、token 总数、估算费用和失败次数。

**费用估算**：给模型配置价格后，`owo usage` 多出 COST 列，`owo history` 显示每次调用的费用，`owo models` 显示已配置的价格。价格单位是美元 / 百万 token：

```toml
[[models]]
id = "claude-sonnet-5"
provider = "anthropic"
price = { input = 3, output = 15, cache_read = 0.3, cache_write = 3.75 }
```

- `input`：未命中缓存的输入；`output`：输出（含推理）。两者必填。
- `cache_read` / `cache_write`：缓存读取 / 写入的价格，不填按 `input` 计算（宁可估高）。
- 一个提供商下所有模型同价时，写在提供商上：`[providers.<名字>]` 下加 `model_defaults = { price = { input = …, output = … } }`；单个模型的 `price` 优先。
- 费用在每次调用结束时按**当时的价格**算好存进记录，之后改价格不影响历史记录；配置价格之前的调用没有费用。
- 没配价格的模型不计入 COST；表格里带 `*` 的合计表示其中有未计价的调用。
- 这是按 token 数和你填的价格做的估算，不是提供商的账单，实际费用以提供商为准。

记录保存在 `~/.owo/state/usage.db`，保留 400 天，网关启动时自动清理更早的记录。

---

## 模型与提供商

```bash
owo models                                   # 所有模型：ID、所属提供商、是否可用、上游模型名
owo models --app codex                       # 某个应用看到的模型 ID（含别名）
owo providers                                # 已配置的提供商
owo providers presets                        # 内置预设
owo providers discover deepseek              # 在线列出提供商现有的模型，✓ 表示已在配置里
owo providers discover deepseek --add deepseek-chat,deepseek-reasoner   # 把选中的模型写进配置
owo providers discover openrouter --all      # 把列出的全部模型写进配置
```

`discover --add` 写入的是配置文件里该提供商的 `models` 列表；写完需要重启网关生效。

---

## 全局选项

| 选项 | 含义 |
|---|---|
| `--config <路径>` | 使用指定的配置文件 |
| `--portable` | 便携模式：配置文件放在可执行文件旁边，数据放在 `./data` |
| `-v` / `-vv` | 更多日志 |
| `-h, --help` / `-V, --version` | 帮助 / 版本 |

环境变量：

| 变量 | 作用 |
|---|---|
| `OWO_HOME` | 改变 OwO AI Gateway 的主目录（默认 `~/.owo`） |
| `OWO_LOG` | 日志过滤，例如 `OWO_LOG=debug` |
| `VISUAL` / `EDITOR` | `owo config edit` 使用的编辑器 |
| `CODEX_HOME`、`CLAUDE_CONFIG_DIR` 等 | 各应用自己的目录变量，OwO AI Gateway 会遵循 |

---

## 文件位置

| 内容 | 位置（`~` 为用户主目录） |
|---|---|
| 配置 | `~/.owo/config.toml` |
| 接入状态、Cursor 数据、运行中网关的 PID | `~/.owo/state/` |
| 调用记录与 token 用量（保留 400 天） | `~/.owo/state/usage.db` |
| 修改应用配置前的备份 | `~/.owo/backups/<应用>/`（MCP：`~/.owo/backups/mcp-<应用>/`；技能：`~/.owo/backups/skills/`） |
| 技能状态、GitHub 缓存 | `~/.owo/state/skills.json`、`~/.owo/state/skills-cache/`（技能本身在 `~/.agents/skills/`） |
| 日志（含后台网关日志 `owo.log`） | `~/.owo/logs/` |

`owo config` 会打印当前实际使用的这些路径。

Cursor 的调用记录（`~/.owo/state/cursor/` 下的数据库）只保留最近 30 天，网关启动时自动清理更早的已完成记录。
