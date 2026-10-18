# sfzklearning

法考 / 注会 刷题桌面应用 —— 原生 Windows，Rust + eframe/egui，**无 web view / 无 HTML / 无 Tauri / 无 Electron**。

## 直接使用

1. 从 [Releases](../../releases) 下载 `sfzklearning-v0.03-windows-x64.zip`
2. 解压到任意目录，运行 `sfzklearning.exe`

**解压即可用** —— 题目数据（`data\`）就在包内，不需要任何配置或外部路径。
需要 64 位 Windows 10/11（以及常见的 Microsoft Visual C++ 2015-2022 运行库）。

## 功能

- **两个考试模式**：法考（18 科目）/ 注会（7 科目）。左上角模式按钮**点击即原地切换**，没有中间的"二选一"页面。
- **练习设置**：题型（全部 / 选择题 / 记忆自测）、来源（可多选：仅错题 / 仅收藏 / 未做过 / 做错过）、
  题量（全部 / 10 / 20 / 50 / **自定义**）、顺序（顺序 / 随机）、章节多选列表、关键词搜索。
- **做题会话**：整窗专注模式（隐藏左栏与标题行）；单选 / 多选 / 记忆自测；提交后判定 + 解析。
  键盘：`←` `→` 翻题（`→` 需先判定）、`Enter` 提交、`Esc` 退出。
- **先判定再下一题**：选择题可「提交答案 → 判定」或「显示答案 → 看答案」；显示答案**不记入作答**
  （题目仍是「未做过」），判定 / 看答案前 `下一题` 不可用，记忆题先自评。
- **收藏与备注**：每题可收藏、写多行备注，自动保存到本地（无需登录）。
- **错题随机 10 题**：从错题里抽 10 道开一局。
- **深色 / 浅色**主题 + 毛玻璃窗口；设置 → 外观里可调**透明度**与**模糊度**（桌面截图 + 真高斯模糊，实时生效）。
- **MCP 服务**（`sfzklearning-mcp.exe`）：只暴露一个工具 `get_current_study_context`，
  把"用户当前正在做的题 + 关联知识点"交给 Agent 讲解。配置示例见 `mcp.json`
  （里面的路径是本机绝对路径，换机器需要改）。

## 从源码构建

```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
cargo build --release
```

产出 `target\release\{sfzklearning.exe, sfzklearning-mcp.exe, classify.exe}`。
`cargo test` 可跑数据层与逻辑层单测。

## 目录结构

```
data\law\  data\cpa\        # 题目数据（随包分发）
  taxonomy.md / taxonomy.json   # 官方考点分类树
  summary.json                  # 科目清单
  subjects\<subjectId>.json     # 每科题目（chapter 已归位）
  classification-report.md      # 覆盖率报告
state\                      # 你的本地记录：收藏 / 备注 / 每题统计 / 错题 / 当前上下文
                            # （个人数据，不进版本库）
src\lib.rs                  # 数据类型 / 加载器 / current.json 契约 / UserStore
src\bin\studyapp\           # GUI（app.rs + app\ 下的视图子模块）
src\bin\sfzklearning-mcp.rs # stdio MCP 服务
src\bin\classify.rs         # 数据管线：语料 → 官方考纲
data\corpus\                # 分类器的输入语料（仅开发用；不进版本库、不随包分发）
```

## 数据是怎么来的

三方题库（42 个上游仓库）→ 提取成 CMF 语料 → `classify.exe` 按**司法部考纲 / 中注协教材目录**
把每道题归到「官方科目 → 章」，写出 `data\{law,cpa}\`。
分类是**确定性、可重跑**的：同样的输入产出逐字节相同的输出（仅 `summary.json` 带时间戳）。
`classify` 从不写回输入语料。

## 文档

设计与实现细节（信息架构、设计令牌、数据管线、MCP 契约、快捷键）见 [`DESIGN.md`](DESIGN.md)。
