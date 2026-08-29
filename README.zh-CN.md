[English](README.md) | [简体中文](README.zh-CN.md)

# SkillSync

> 创建一次技能，在一处管理，自动在所有支持的 AI 编码工具中可用。

SkillSync 是一个本地优先的桌面应用 + CLI，为 **Agent Skills**（`SKILL.md` 目录）提供统一的控制平面，覆盖多种 AI 编码工具：

| 工具 | 全局技能目录（自动检测，可覆盖） |
|---|---|
| Claude Code | `~/.claude/skills` |
| Codex | `~/.codex/skills`（`$CODEX_HOME`） |
| Cursor | `~/.cursor/skills`（并原生读取 `~/.agents/skills`） |
| Gemini CLI | `~/.gemini/skills` |

技能统一存放在一份规范仓库（canonical store，默认 `~/.agents/skills`）中，再以派生安装（符号链接 / 复制 / 原生共享）的方式进入各工具。SkillSync 绝不覆盖未纳管技能，也绝不执行技能代码。

## 状态

**v0.1.0 已发布**（见 [Releases](https://github.com/vicode117/skill-sync/releases)），覆盖完整 MVP：

- **只读发现**：工具检测、技能目录发现、SKILL.md 解析与校验、托管状态分类、doctor 诊断；
- **规范仓库**：收编根目录、按内容指纹导入、冲突安全处理、备份、dry-run；
- **单向同步**：canonical → 工具，符号链接 + 复制回退，托管所有权追踪，逐项预览；
- **技能 × 工具矩阵**：按技能开关工具同步（停用只移除托管安装）；
- **冲突管理**：对比视图、显式备份后解决（采用规范版 / 采用目标版 / 两者保留 / 忽略）；
- **自动同步**（可选，防抖监视，默认关闭）；
- **Git 跨机器同步**：显式 status / pull --ff-only / commit / push；
- **中英文界面**：设置中切换，跟随系统默认。

## CLI

```bash
skillsync list                            # 所有发现的技能（canonical + 各工具）
skillsync tools                           # 检测到的工具、目录、能力
skillsync scan                            # 只读扫描所有工具技能目录
skillsync doctor                          # 环境诊断
skillsync adopt-root                      # 创建规范仓库目录（若缺失）
skillsync import <path>                   # 导入技能到规范仓库
skillsync import <path> --dry-run         # 仅预览计划
skillsync import <path> --keep-both       # 冲突：以 <name>-2 导入
skillsync import <path> --replace         # 冲突：备份 + 替换
skillsync sync --tool claude              # canonical -> 工具（symlink/copy）
skillsync sync --tool claude --dry-run    # 仅预览计划
skillsync sync --all                      # 所有已检测且启用的工具
skillsync disable tdd --tool gemini       # 仅移除托管安装
skillsync enable tdd --tool gemini        # 重新安装
skillsync conflicts                       # canonical 与未纳管的冲突
skillsync diff tdd --tool claude          # 文件 + 行级对比
skillsync resolve tdd --tool claude \
  --resolution use-canonical [--dry-run]  # 显式、带备份的解决
skillsync git status                      # 跨机器同步：分支 + 变更技能
skillsync git pull | commit | push        # 始终手动，绝不自动
skillsync scan --json                     # 所有命令支持机器可读输出
```

退出码：`0` 成功，`1` 操作错误 / doctor 发现错误，`2` 用法错误。

## 安装

从 [Releases](https://github.com/vicode117/skill-sync/releases) 下载最新构建：

- **macOS（Apple Silicon）** — `SkillSync-<版本>-aarch64-macos.dmg`。应用未签名，首次启动会提示 *"Apple 无法验证 SkillSync"*。两种一次性解锁方式：执行 `xattr -cr /Applications/SkillSync.app`，**或** 关闭弹窗后在 系统设置 → 隐私与安全性 点击 **仍要打开**。
- **Windows（x64）** — `SkillSync-<版本>-x64-windows-setup.exe`（按用户安装的 NSIS 安装包；SmartScreen 可能要求 *更多信息 → 仍要运行*）。

或从源码构建：`pnpm install && pnpm tauri build`。

## 开发

前提：Rust（stable）、Node LTS + pnpm、平台对应的 Tauri 依赖。

```bash
pnpm install
pnpm tauri dev            # 桌面应用
cargo test                # 核心测试（只使用临时目录与 fixtures/）
pnpm lint && pnpm typecheck && pnpm test && pnpm build
cargo fmt --check && cargo clippy --all-targets --all-features -- -D warnings
pnpm tauri build          # 为当前平台打包（macOS 为 .app；在 bundle.targets 中加 "dmg" 可出磁盘镜像）
```

仓库结构与架构规则：[`AGENTS.md`](AGENTS.md)、[`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)（英文，含完整的仓库架构与各 Slice 设计）。
