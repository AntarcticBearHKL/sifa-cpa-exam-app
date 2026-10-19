> **快照说明 / Snapshot notice**
>
> 本文档是 **0.01 版本之前** 的架构与设计记录，保留作为「设计意图 + 踩坑记录」。
> 部分内容**已过时**：应用已更名为 `sfzklearning`、
> 做题视图已改为整窗专注模式（隐藏左栏与标题行）、滚动条已全局隐藏。
> 与代码冲突时，**以代码为准**。

---

# sfzklearning — 法考 · 注会 学习助手（原生 Windows 桌面）

一个**原生 Windows 桌面应用**（Rust + eframe/egui，**无 web view / 无 HTML / 无 Tauri / 无 Electron**），
用来**练习** `D:\Dev\StudyApp\data\{law,cpa}`（整理后的法考 / 注会 **CMF** 数据，随二进制一起分发；
原始语料在 `data\corpus\{law,cpa}`，不参与分发）。

---

## 1. 产出物

| 文件 | 说明 |
| --- | --- |
| `target\release\sfzklearning.exe` | GUI（原生窗口，标题 `sfzklearning`） |
| `target\release\classify.exe` | 数据管线：把语料按官方考纲归类并重算 `summary.json`（见 §9） |
| `state\current.json` | app 写出的“当前上下文”文件（含用户备注 / 收藏标记） |
| `state\wrong-<domain>-<subjectId>.json` | 错题 id 的**派生投影**：由 `user.json` 每题统计（`attempts > 0 && correct < attempts`）推导，加载时按统计修剪并写回；app 不读它做筛选，仅供外部工具读取 |
| `state\user.json` | **本地用户缓存**：收藏题目 id + 每题备注 + 每题统计（看过 / 作答 / 正确，无需登录） |
| `src\lib.rs` | 数据类型 / 加载器 / current.json 契约 / `UserStore` |
| `src\bin\studyapp\main.rs` | GUI 入口（约 18 行：`mod` 声明 + `fn main`） |
| `src\bin\studyapp\app.rs` | `App` 状态 / 逻辑 / 持久化 / 快捷键 + `impl eframe::App` |
| `src\bin\studyapp\app\` | 视图子模块：`shell.rs`（顶栏 / 左栏 / 页面标题 / 工作区）、`practice.rs`（练习设置 + 会话）、`settings.rs`（设置弹窗） |
| `src\bin\studyapp\logic.rs` | 纯逻辑（无 UI）：筛选 / 洗牌 / 错题抽样 / 章节分组 / 搜索 / 判定 |
| `src\bin\studyapp\win.rs` | 无边框窗口：圆角 / 深色、边缘缩放、任意处拖动、`--probe`（不再向系统申请模糊） |
| `src\bin\studyapp\diag.rs` | 诊断落盘（GUI 子系统无控制台，警告与 `--probe` 结果写 `<state_dir>\log.txt`） |
| `src\bin\studyapp\theme\` | 设计令牌（`tokens.rs`，深/浅双模式）+ `Style`/`Visuals` 覆写 |
| `src\bin\studyapp\anim\` | 缓动曲线 + `Animator`（答错抖动 / 重绘调度） |
| `src\bin\studyapp\glass\` | 磨砂玻璃：`blur.rs`（桌面截图 + 高斯模糊）+ `paint.rs`（绘制原语）+ `widgets.rs`（自定义控件） |
| `src\bin\classify.rs` | 数据管线二进制（分层归类 + 单测，见 §9） |
| `data\` | 整理后的数据（**app 读这里，随二进制分发**）：`taxonomy.md/json` · `summary.json` · `subjects\` · `classification-report.md` |
| `data\corpus\` | 分类器的输入语料（**只读**），`classify --data-root` 的输入，**不参与分发**（已 gitignore） |

## 2. 依赖（版本互锁）

```
eframe = "0.36.2"
egui = "0.36.2"
egui_commonmark = "0.25.0"   # 基于 egui ^0.36.0，支持 GFM 表格
serde = { version = "1", features = ["derive"] }
serde_json = "1"
raw-window-handle = "0.6"    # 取到 Win32 HWND，供 DWM 圆角 / 深色属性与屏幕截图使用
```

## 3. 数据根目录（随二进制分发；语料只读）

app **不直接读语料**，而是读 `data\` 下由 `classify` 归类好的数据。`data\` 与二进制一起走 ——
clone 仓库或解压 release zip 就能跑，不需要任何绝对路径。

默认（`Domain::default_root()`）按顺序找：

1. `<exe>\data\<domain>` —— 便携布局（release zip 把 exe 和 `data\` 并排放）
2. `<exe>\..\..\data\<domain>` —— Cargo 布局（`target\<profile>` → 项目根）

* 法考：`data\law`（env `CMF_LAW_DATA`，CLI `--law-data <path>`）
* 注会：`data\cpa`（env `CMF_CPA_DATA`，CLI `--cpa-data <path>`）

优先级：**CLI > env > 默认**。分类器输入语料用 `--law-data` / `--cpa-data` / `CMF_LAW_DATA` / `CMF_CPA_DATA` 指向 `data\corpus\{law,cpa}`。

`data\`：

```
law\
  taxonomy.md / taxonomy.json     # 官方考点分类树（司法部《2026年国家统一法律职业资格考试大纲》，18 科目 / 205 章）
  summary.json                    # 按实际文件重算的统计
  subjects\<subjectId>.json       # 每科题目，chapter 已归位
  classification-report.md        # 覆盖率报告
cpa\
  taxonomy.md / taxonomy.json     # 中注协 2026 教材目录（7 科目 / 108 章）
  summary.json / subjects\ / classification-report.md
```

读取的文件（全部只读）：

* `<root>\summary.json` — 科目清单（`subjects[]`）
* `<root>\subjects\index.json` — 当 `summary.json` 缺失时的回退
* `<root>\subjects\<subjectId>.json` — 该科目的知识点与题目（按需惰性加载）

> `data\corpus\{law,cpa}\subjects\` 是**分类器的输入**（只读），**不随 app 分发**（gitignore）。
> app 与 `classify` 都不会写回它；`classify` 只写 `--out` 指向的 `data\{law,cpa}\`。

## 4. 构建

```powershell
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
cd D:\Dev\StudyApp
cargo build --release
```

若报 `link.exe not found`，在 VC 环境中构建：

```powershell
cmd /c '"C:\Program Files\Microsoft Visual Studio\18\Community\VC\Auxiliary\Build\vcvars64.bat" && cargo build --release'
```

## 5. 运行

```powershell
.\target\release\sfzklearning.exe                 # GUI
# 自定义数据根目录 / 状态目录：
.\target\release\sfzklearning.exe --law-data D:\Dev\StudyApp\data\corpus\law --cpa-data D:\Dev\StudyApp\data\corpus\cpa --state-dir D:\Dev\StudyApp\state
```

环境变量：

* `CMF_LAW_DATA` / `CMF_CPA_DATA` — 数据根目录
* `CMF_STATE_DIR` — 状态目录（`current.json`、`user.json`、错题文件）
* `CMF_START_DOMAIN` — 启动时默认领域：`law`（默认，若有数据）或 `cpa`
* `CMF_START_SUBJECT` — 启动时默认科目 id（如 `kuaiji`）；缺失或找不到时取该领域的第一个科目
* `CMF_START_SESSION` — 非空且不是 `0` 时，启动就对起始科目**直接开一局练习**（进会话视图，
  等同于点「开始练习」）；用于截图 / 自检时不必合成点击
* `CMF_START_KIND` — 启动时预选题型：`choice`（选择题）或 `subjective`（主观题）；其他值保持「全部」。
  与 `CMF_START_SESSION` 配合，用于截图 / 自检时直达某个题型视图

状态目录解析顺序：`--state-dir` > `CMF_STATE_DIR` > `<exe>/state`（便携布局）> `<exe>/../../state`（Cargo target 布局）。

截图（深/浅双模式）：`screenshots\dark-practice.png` 与 `screenshots\light-practice.png`
（练习设置 —— 左栏「怎么练」含 题型 / 来源 / 收藏 / 正确率 / 题量 / 顺序 / 搜索，右栏章节多选）。
做题会话：`screenshots\dark-session-counts.png` 与 `light-session-counts.png`
（题干下方的 `看过 N 次 · 作答 M 次 · 正确 K 次`，以及填满列宽的 来源/出处 行）。
`screenshots\dark-session-no-wrong-label.png`（会话头部只读 `单选 · 章节 · 年份`，没有 错题 标记 ——
错题与否只由每题统计决定，不再在题头贴标签）。

## 6. GUI 使用

**信息架构**：**没有启动页** —— 启动即进工作区（左栏科目导航 + 主区练习设置）。
现在**只有练习**：没有 学习 / 做题 的标签切换，进入工作区即是练习设置。

* **顶栏**：左侧是**模式按钮**（`法考` / `注会`）—— **点击即在原地切到另一个考试模式**
  （写入 `user.json` 的 `last_mode`，并自动选中该模式的第一个科目）；右侧是 **深色 / 浅色** 分段开关
  （立即生效、写入 `user.json`），再右是最小化 / 关闭。
* **左栏（模式导航）**：按官方分组列出科目 ——
  **注会** = 专业阶段（六科：会计 / 审计 / 财务成本管理 / 税法 / 经济法 / 公司战略与风险管理）+ 综合阶段；
  **法考** = **卷一 / 卷二**。每行显示科目名与该科目题量。
* **主区标题行**：科目名 + **`{N} 章 · {M} 题`**（`N` = 该科目题目里出现过的不同章节数，`M` = 题量）。
* **练习设置**（进入工作区的默认视图，左右两栏，**没有下拉框**）：
  * 左栏「怎么练」：**题型**（全部 / 选择题 / 主观题）、**来源**（可多选：`仅错题` / `未做过` / `做错过`）、
    **收藏**（不限 / 已收藏 / 未收藏）、**正确率**（不限 / 未作答 / 低于 60% / 低于 100% / 全对）、
    **题量**（全部 / 10 / 20 / 50 / **自定义** —— 选自定义时右边出现数字输入框，范围 1–500；
    预设项忽略自定义值，超出题库则取全部）、**顺序**（顺序 / 随机）、**关键词搜索**（题干 / 解析）。
  * 右栏「练哪些章节」：章节**多选列表**（每行带勾选框与该章题量），配 `全选` / `清空` / `反选`，
    以及一个只影响显示、不改选择的**章节筛选框**；表头显示「已选 n / m 章」。**一章都不选 = 全部章节**。
  * 实时显示「本范围共 N 题」。
* 点 **开始练习** → **会话视图**（**整窗专注模式：隐藏左栏与标题行**）：会话标签、`第 i / N 题`、进度条；
  **题干以 18px 作为视觉焦点**；选项（单选 radio / 多选 checkbox）；**提交答案** → 判定 + 解析，
  或 **显示答案** → 看答案（不记录作答，题目仍是「未做过」）；判定 / 看答案后 `下一题 ▶` 才可用，
  `←` 与 `Esc` 不受限（详见 §6.5）。题干正下方一行小字是**该题的刷题信息**
  `看过 N 次 · 作答 M 次 · 正确 K 次`（muted，12.5px；**没看过就整行不显示**，不会打印 `看过 0 次`）。
* **错题随机 10 题**：从错题（每题统计 `attempts > 0 && correct < attempts`，与「仅错题 / 做错过」同一判据）里
  随机抽 10 道开一局（会话标签为「错题随机抽查」）。
* `options:[]` 的条目走**主观题**（`logic::Kind::Subjective`，`matches = q.options.is_empty()`）：
  这是**只读展示**模式 —— 题干照常显示，正文只有一个 **显示答案** 动作，**从不判定**、
  **不受 `下一题` 闸门限制**（看完答案直接翻页）；数据里既没有答案也没有解析时打印
  「这道题在数据里没有答案」。**旧的「记忆自测」路径已整体移除**（`记得/不记得`、`Session::self_assessed`、
  `App::self_assess`、`render_memory` 都不在了）。

**快捷键**（无边框窗口没有系统快捷键，所以由应用提供；文本框获得焦点时自动让位）：

| 键 | 作用 |
| --- | --- |
| `←` / `→` | 做题会话中翻上一题 / 下一题（`→` 需当前题已判定，与 `下一题 ▶` 同规则） |
| `Enter` | 会话中提交答案 |
| `Esc` | 退出当前练习会话 |

**每题统计**：每次作答都写进 `state\user.json` 的 `stats`（`attempts` / `correct` / `last`），
所以「做过几次、正确率多少」是持久的、跨会话累积的。
**错题不再单独维护**：`wrong-*.json` 只是这些统计的派生投影（加载时按统计修剪并写回，
作答后再从统计重写），「仅错题 / 做错过 / 错题随机」一律读统计 ——
只看过没作答的题（`attempts == 0`）永远不是错题。
另有 `seen`（看过次数）：题目**成为当前题**时加一 —— 进入会话、`下一题`、`上一题` 各算一次，
**同一题再看一次就再计一次**；只在切题的那一处写盘，**渲染一帧不会计数**，
也**不碰 `attempts` / `correct`**。`last` 记的是最后一次交互（作答或查看）。
会话视图题干下方那行小字就是这三个计数。
`QStat.seen` 带 `#[serde(default)]`，所以旧 `user.json` 照常加载（读出来是 `seen == 0`）。

**题目 → 知识点** 关联：仍用 `q.knowledgePoints`（标题）在本科目 KP 里做**标题精确匹配**（重复标题取首个），
结果写进 `state\current.json`；会话视图**已不再展示**这些关联 chip。

**中文字体**：启动时从 `C:\Windows\Fonts` 依次尝试 `msyh.ttc`、`msyhbd.ttc`、`simhei.ttf`、`Deng.ttf`、`simsun.ttc`
（`.ttc` 使用 collection index），注册为默认 Proportional + Monospace。找不到时**写 stderr**（不再有底部状态栏）。

**备注 / 收藏（本地用户缓存，无需登录）**：会话里每题都有 `☆ 收藏` 与 `备注`。备注多行输入，
**失焦或切题时自动保存**（打字过程中不写盘）。数据落在 `state\user.json`（`UserStore`），
`data\corpus\` 语料**依旧只读**。收藏、备注与每题统计都会写进 `current.json`。

## 6.5 UI 设计系统（毛玻璃 / 排版 / 动效）

GUI 拆成入口 + 六个模块：`main.rs`（入口，仅 `mod` 声明 + `fn main`）、
`app.rs`（`App` 状态 / 逻辑 / 持久化 / 快捷键 + `impl eframe::App`，视图为 `app/` 下的子模块
`shell.rs` / `practice.rs` / `settings.rs`）、
`logic.rs`（纯逻辑，无 UI，可单测）、`win.rs`（无边框窗口 / 圆角 / 缩放 / 拖动）、
`theme`（设计令牌 + `Style`/`Visuals` 覆写）、`anim`（缓动 + `Animator`）、
`glass`（`blur.rs` 桌面截图 + 高斯模糊、`paint.rs` 绘制原语、`widgets.rs` 自定义控件）。

**布局风格：整块分隔式（不是浮动卡片）**。窗口是一张**连续的磨砂面**，由发丝线分成几个区域：

1. 底层是 `LayerId::background()` 上的**桌面本身**：应用截图**整个虚拟屏幕**并做**真高斯模糊**（`glass::blur`），
   按屏幕位置取 UV 画满客户区，再盖上 scrim（alpha = 透明度设置）。截图不可用时才退回
   `paint::backdrop` 的柔和垂直渐变 + 软斑。
2. 每个区域是**平整的磨砂填充**（`glass::panel_fill`）：无圆角、无外间距、**无投影** —— 没有任何东西浮在背景上。
3. 区域之间用**发丝分隔线**（`glass::divider_bottom` / `divider_right`，色 `glass.divider`）。
4. **面板自身没有渐变。** 真实磨砂玻璃是均匀的，它的明暗变化**全部来自背后的光** —— 面板越透，透上来的层次越明显。
5. 刻意**没有**的东西：边缘发光、顶边亮带、多彩色斑、强投影、颗粒噪点、圆角浮动卡片。那些是「玻璃拟态」装饰，不是玻璃。
6. 窗口底色透明（`clear_color`），`panel_fill` 透明；**任何地方都不硬编码颜色**。

**双模式配色：深色 / 浅色**。`theme/tokens.rs` 里 `Mode::{Dark, Light}` 各一套中性调色
（深色不是蓝黑、浅色不是冷灰白），共用同一个**低饱和石板蓝**强调色。顶栏右侧的 **深色 / 浅色**
分段开关立即切换，并驱动四个地方：`Tokens` 调色板、egui `Visuals::dark()/light()`、
`ctx.set_theme(ThemePreference::…)`（原生窗口色）与 DWM 沉浸式深色标志（Dark=1 / Light=0）。
选择写入 `state\user.json` 的 `theme`（缺省 `dark`），下次启动沿用。

**整体布局**：`顶栏（模式 + 深色/浅色 + 窗口按钮）` / `左栏（按官方分组列科目 + 底部统计块）` /
`主区（页面标题行：科目名 + 章节·题量 + 已做·正确率，再是当前视图）`。

**调色 / 调质感** —— 全部集中在 `theme/tokens.rs`（`Palette::dark()` / `Palette::light()`）：

| 想要的效果 | 改哪里 |
| --- | --- |
| 界面更黑 / 更灰 | `background.base_top` / `base_bottom`（深色 30→16；浅色 247→238） |
| 背景更透（看到更多桌面） | 设置 → 外观的**透明度**滑杆（scrim alpha = `255*(1-t/100)`，默认 20 → 204） |
| 表面更亮 / 更分明 | 深色：`glass.fill` / `fill_strong` 的 alpha（白 18 / 32）；浅色：**不透明**表面色（`rgb(255,255,255)` / `rgb(240,241,244)`） |
| 描边更淡 / 边缘不抢戏 | `glass.stroke`（深色白 **14**，低于 `fill` 18；浅色 `rgb(213,216,224)`） |
| 分隔线更明显 / 更淡 | `glass.divider`（深色 alpha 24；浅色 `rgb(225,227,232)`） |
| 下拉 / 提示浮层更亮或更暗 | `glass.solid`（浮层实心底色：深色 rgb(30,30,33) / 浅色 rgb(255,255,255)） |
| 换色调 | `background.blobs` 与 `accent.*`（深色 rgb(110,132,168) / 浅色 rgb(10,99,194)） |

> `scrim` 是两种模式的关键：它必须够不透明，否则窗口后面若是浅色桌面，深色 UI 会被冲成灰白。
> 浅色模式的 scrim 颜色取自参考项目 3D 资源库的 `window-bg`（`rgba(244,245,247,alpha)`），
> alpha 由**透明度**设置驱动（默认 20 → 204）。
>
> **浅色模式照抄参考项目的分层**：它的面板是 `transparent`（根节点只画一次 `window-bg`），
> 只有 `surface` / `surface-2` / `well` 是不透明的。所以本应用的三个区域
> （顶栏 / 左栏 / 主区，`glass.fill`）**不带自己的底色**，浅色下就是那张半透明的底 ——
> 桌面透出来，正文是磨砂的；而**行、井、弹窗保持不透明**（`fill_strong` `#F0F1F4`、
> `fill_sunken` `#F2F3F6`、`solid` `#FFFFFF`），所以控件和设置弹窗始终清晰、永远「是对的」。
>
> 代价是算术，不是 bug：浅色底在**深色**桌面上的读数 = `0.8 × 244 + 0.2 × 桌面`，
> 默认 20 时约 **200**；想要更亮就把**透明度**滑到 0（读数 244，完全不透明）。
> 深色模式不动：它的洗色本来就压在同为深色的桌面上，读数约 34，
> 与参考项目的 `#1E1E1E` 几乎一致。

> **选中 = 染色填充，不是描边。** 六个「选中 / 主要」状态（分段指示器、开关 chip、勾选框、主按钮、
> chip、左栏选中行）一律用 `accent.soft` 系的**低透明染色填充**表达；描边保持中性（`glass.stroke`）
> 或低透明强调色。满强度强调色的 1px 描边在近黑面上会读成「霓虹线框」，这是上一版最大的问题。

> **浮层（提示气泡等）与面板相反：必须实心。** 面板的底是桌面，所以透明；而浮层的底是应用自身，
> 一透明就会把下面的内容透上来，看起来像一张发白的磨砂浮卡（踩过这个坑）。所以浮层走 `glass.solid`
> （不透明）+ 发丝描边 + **无投影** —— 与「整块分隔式」一致，不浮起来。`Frame::popup` / `Frame::menu`
> 读的正是 `window_fill` / `menu_corner_radius` / `popup_shadow`。

`theme/` 里十个测试守住两种模式的调色与浮层规则：`palette_is_dark_and_readable` /
`light_palette_is_light_and_readable`（各自的底色 / 文字对比，浅色 scrim 浅且近乎实心，深色 scrim 深且近乎实心）/
`accent_is_muted_not_saturated`（深色保持中性；浅色是参考项目的实蓝色） / `surfaces_are_translucent_but_visible`（深色：
表面够透但可辨；浅色：区域不带底色、行 / 井 / 弹窗不透明且留在本模式色系） / `edges_recede_and_selection_is_a_tint`（描边低于表面、
染色填充是透明色不是实心块） / `mode_ids_round_trip`（`user.json` 的 `theme` 值解析），
以及 `popups_are_flush_not_floating_glass`（**两种模式**：`Frame::popup` 读的 `window_fill` 必须不透明、
`popup_shadow` 必须为 `NONE`、无顶边亮带、`menu_corner_radius` 与按钮一致）
和 `pressed_widgets_get_a_tint_not_a_neon_outline`（按下态只有低透明染色，没有满强度强调色描边），
还有 `scrollbars_are_hidden_everywhere`（**两种模式**：滚动条宽 0、各状态不透明度全为 0，任何状态都画不出滚动条）
和 `scrim_keeps_body_text_at_aa_over_the_worst_case_desktop`（scrim 在最坏桌面底色上仍让正文达到 AA 对比）。

**测试（不开窗口、不点鼠标）**：`cargo test` 直接跑 —— 共 **104 passed, 0 failed**
（14 lib + 18 classify + 72 sfzklearning），UI 只负责渲染结果。

* `logic.rs`（16 个）：按题型 / 章节（可多选）/ 来源（错题 · 未做过 · 做错过）/ 收藏（不限 · 已收藏 · 未收藏）/
  正确率（不限 · 未作答 · 低于 60% / 低于 100% / 全对）/ 关键词筛选（新条件与旧条件 AND 组合、不限是真 no-op）、
  **错题判据 `is_wrong`**（没作答不算、全对不算、错过才算）、
  `chapter_counts` 章节计数、`draw_plan` 先排序再按题量截断、错题抽样只取统计判定的错题且按上限截断、
  `shuffled` 是排列且同种子确定、判定的边界（未选 / 无选项）、
  **下一题的判定闸门**（提交 / 显示答案；主观题从不被闸门拦住）、**显示答案的可用性**（有答案或有解析）、
  错答文案在无答案时的兜底。
* `win.rs`（5 个）：headless 注入指针事件后断言窗口指令 —— 按**空白处**发 `StartDrag`、
  按**控件上**不发、**八个边/角**各发对应方向的 `BeginResize`、按边缘**不**发 `StartDrag`、
  悬停各边/角返回对应的缩放光标。
* `theme/`（10 个）：深色调色板深且可读、浅色调色板浅且可读（逆不变量）、强调色不饱和（两种模式）、
  表面够透但可辨（两种模式）、描边低于表面且选中是染色不是描边、`theme` 值解析往返、
  **浮层实心且无投影**（两种模式，两个 style slot 都断言）、按下态无霓虹描边、
  **滚动条全局隐藏**（宽 0、各状态不透明度 0）、**scrim 最坏桌面下正文仍达 AA**。
* `glass/`（19 个）：`paint.rs` 的径向网格几何 / 边缘渐隐 / 退化输入不 panic / 软斑不出界 / alpha 混合 /
  **透明度 → scrim alpha 映射**（0→255、100→0、单调、钳制、默认 ≈ 旧值）；
  `blur.rs` 的**高斯模糊**（平坦图均值不变 / 阶跃边单调且平滑 / σ=0 精确复制 / 1×1 与 0 尺寸不 panic）、
  `downscale`（ceil 半尺寸 / 平坦图保持平坦）、`params`（0 不模糊、越大越糊、halvings ≤ 3）；
  `widgets.rs` 的指示器滑动与钳制 / 通道插值 / 统计文本 / 抖动只动 x / 内容置于平移后的矩形 /
  **滑杆的几何 → 取值映射**。
* `app.rs`（7 个）：毛玻璃**截图规则**（`glass_work`：第一帧截一次；**移动 / 缩放永不重截**；
  模糊度改变走 `Reblur`，只重跑降采样 + 高斯 + 上传，不碰屏幕；`重新采样背景` 与 **0.5 s 定时器**
  才截第二张，**拖动中落下的定时 tick 跳过**；失败的截图不每帧重试）、**取景几何**（`uv_in_capture`：窗口移动多少，取样就深入桌面多少 ——
  同尺寸、无 NaN）、**合成多显示器布局**（右侧 / 左侧 / 上方 / 混合堆叠各一套虚拟屏幕与窗口矩形，
  断言 UV 落点、裁剪尺寸，以及 UV 反算回物理像素的 0 px 误差；窗口不在截图内时仍然有限不 panic）、
  **显示答案**只置 `shown`（不动 `answered` / `correct`），并打开 `下一题` 闸门、
  **加载时按每题统计修剪 `wrong-*.json`**（从未作答的 id 被剔除，真正错题的留下并写回文件）。
* `practice.rs`（3 个）：**来源/出处那一行** —— 打开 `break_anywhere` 后首行填满整列
  （默认断行会提前停在最后一个空格处、右侧空一大截），在**换行横向布局**里也确实填到列右缘，
  短出处与原来逐点相同。
* `anim/`（12 个）：`easing::clamp` 边界、`shake` 衰减端点与边界、抖动重启与结束、`is_live` / `prune`；
  `motion` 的**三段时长**（FAST / BASE / SLOW 有序且都短）、**两条曲线**（端点精确、单调不回退）、
  `progress_since` 到 1 且不过冲、**中途重定向**从当前位置续走、重定向到同目标不重启时钟、
  过渡到达目标即静止，以及**运行中的过渡才请求重绘**（跑完就停）。
* `lib.rs`（14 个）：数据加载 / 判定 / `UserStore`（收藏 · 备注 · 统计 · 主题）往返、
  **`user.json` 缺 `glass_*` 字段仍能反序列化**（旧文件兼容）、
  **`stats` 条目缺 `seen` 字段仍能反序列化**（旧文件读作 `seen == 0`），
  以及 **`seen` 只在「看」时加一、`attempts` / `correct` 不受影响**。

**窗口：无边框 + 应用内毛玻璃（桌面截图 + 真高斯模糊）**

* `NativeOptions` 用 `with_decorations(false)` + `with_transparent(true)`：没有标题栏，客户区透明
  （圆角处因此露出桌面）。**透明度仍然必需**，但背景不再依赖系统模糊。
* **后端必须是 `glow`**：`renderer: eframe::Renderer::Glow`。默认的 `wgpu` 后端对普通 HWND 只报
  `CompositeAlphaMode::Opaque`（wgpu-hal `dx12/adapter.rs`），**永远做不出半透明窗口**；
  OpenGL 走 GDI 重定向表面，DWM 认它。
* **模糊由应用自己做**（`glass/blur.rs`）：`BitBlt` 屏幕 DC 截**整个虚拟屏幕**（`SM_XVIRTUALSCREEN/Y` +
  `SM_CXVIRTUALSCREEN/Y`；多显示器时虚拟原点是负的，`BitBlt` 的源坐标就用这个原点 ——
  `GetDC(NULL)` 的 DC 本就覆盖整个虚拟桌面）→ 2×2 降采样（`0..=3` 次）
  → **可分离高斯模糊**（半径 `ceil(3σ)`，横向 + 纵向，边缘钳制）→ `TextureOptions::LINEAR` 纹理，
  画满客户区后盖 scrim。截图时窗口**不能出现在自己的截图里**，两条路径（`glass::blur::Hide`）：
  * **首选**：`SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE)`（`0x11`，Windows 10 2004+）。
    合成器把窗口从所有截图中去掉、但照常画给用户看，所以**屏幕上什么都没有变**，没有透明帧可被
    DWM 呈现 —— 这条路径不会闪。`BitBlt` 前后各一次调用（前 `0x11`、后 `WDA_NONE` `0x0`），
    窗口只在这几毫秒里对截图隐藏，用户自己的截图照常包含它。
  * **回退**（该调用失败，即 2004 之前的系统）：窗口用 `SetLayeredWindowAttributes(hwnd, 0, 0, LWA_ALPHA)`
    把自己变全透明、截完立刻恢复 255 —— 远短于一帧，DWM 通常不会呈现这一帧。
  `log.txt` / `--probe` 会记下**该调用的原始返回值**（`affinity_ret`，非 0 = 被接受），
  所以「这台机器走的哪条路、系统答了什么」可事后查证。
* **为什么不再用系统模糊**：`SetWindowCompositionAttribute` 的 accent policy **没有半径参数**，
  「模糊度」滑杆在它上面根本无从实现；`DwmEnableBlurBehindWindow` 在 Windows 10/11 上是实测无效的
  （16px 黑白网格：只调 blur-behind 与完全不调**逐字节相同**）。旧机制的历史实测数据保留：
  accent policy 能把窗口内网格对比度从 12.4 压到 0.52；「窗口平移 500×120px」时，锐利背景有
  39.9% 的像素跟着变，磨砂背景只剩 1.5%。现在 `win.rs` 不再向系统申请任何模糊。
* **两个用户设置 + 一个手动动作**（设置 → 外观，写入 `state\user.json`）：
  **透明度** `0..=100`（scrim alpha = `255 * (1 - t/100)`，默认 20 → 204）与
  **模糊度** `0..=100`（映射到降采样次数 + sigma，集中在 `glass::blur::params`；默认 100，即最大模糊；0 = 桌面保持清晰）。
  旁边的 **`重新采样背景`**（次要按钮样式，带悬停说明）用于桌面内容变了之后**手动**刷新；
  后台还有一个 **0.5 s 定时器**（`GLASS_INTERVAL`）会自动重截，所以它不再是唯一的重截时机。
* **截图节奏（启动 + 0.5 s 定时器 + 手动）**：截图是**整个虚拟屏幕**的一张图，纹理按窗口的**物理像素客户区**取 UV（`App::glass_uv`；
  矩形来自 Win32 `GetClientRect` + `ClientToScreen`，不再用 egui 点数 × `pixels_per_point` —— 多显示器
  DPI 不同时单一缩放系数根本没法命名所有屏幕上的位置。**必须是客户区而不是 `GetWindowRect`**：winit 的
  无边框窗口为投影阴影把客户区整体下移 1px，实测 `ClientToScreen(0,0)` = 窗口矩形 y + 1，直接用窗口矩形
  会让背景整整偏 1px；实测两台显示器上 UV 反算回物理像素的误差都是 **0 px**），
  所以移动窗口时模糊的桌面**不跟着窗口滑**，而是像真的贴在桌面上 —— 这也正是**移动 / 缩放窗口
  不需要重截**的原因：它只改变取哪一块（`uv_in_capture`）。截图**会在这些时机发生**：
  **启动后的第一帧**、**每 0.5 s 一次的定时器**（`GLASS_INTERVAL`，桌面内容变了自动跟上），
  以及用户点击 **`重新采样背景`**；**落在按住主键拖动中的定时 tick 会被跳过**（`glass_work` 的 `dragging`），
  拖动因此仍然廉价、也不与「移动 / 缩放不重截」的规则打架。
  **模糊度改变只重跑 `downscale` + `gaussian` + 纹理上传**（截图原图存在 `App::glass_shot`，
  不再碰屏幕）；**透明度完全不碰截图**（只改 scrim alpha）。
  截图只在**其他窗口**变化时才过时 —— 0.5 s 定时器正是为了自动跟上这种变化。
  截不到（远程桌面 / 老系统）时保留上一张，一张都没有则回退为应用内渐变背景（`paint::backdrop`），
  窗口永远可读。
* **缩放窗口**：无边框窗口没有系统缩放边框，所以由 `win::chrome` 自己判定 ——
  指针进入窗口边缘 8px（角 16px）时**设置对应的缩放光标**（`ResizeHorizontal` / `ResizeVertical` / `ResizeNwSe` / `ResizeNeSw`），
  按下则发 `BeginResize` 触发**原生**缩放。光标提示是必需的：否则无边框窗口上根本看不出哪里能拖。
* **拖动不限顶栏**：`win::chrome` 每帧检查指针 —— 按在**任何 egui 未在使用的区域**（也就是所有不可交互处）
  都会拖动整个窗口。它**不用** `egui::Area`：全屏前台层会盖在所有面板之上
  并吞掉全部点击（这是踩过的坑，代码里有注释）。
* `win::chrome_button`：**用线条画**的最小化 / 关闭，避免字体缺字形（msyh 既没有窗口控制字形，也没有勾叉）。
* 自检：`sfzklearning.exe --probe` 打印**从系统读回**的属性与毛玻璃状态后立即退出，例如
  `probe dark=1 corner=2 (2=round) border=read-failed (=-2 none) glass=captured (display-affinity, affinity_ret=1)`
  （`dark` 跟随当前模式：深色 1 / 浅色 0；`corner` 2 = 圆角；`border` 读到 `-2`（`DWMWA_COLOR_NONE`）
  就是没有系统描边，这台机器上读不到，所以显示 `read-failed`；`glass` 表示桌面截图是否成功、
  走的哪条隐藏路径、以及 `SetWindowDisplayAffinity` 的原始返回值 —— 非 0 就是被系统接受）。
  每次截图都会往 `log.txt` 写一行
  `glass: capture #N ok (hide=… affinity_ret=… origin=x,y size=WxH)`（`origin`/`size` 就是这次
  截图读到的**虚拟屏幕**范围，单显示器时是 `0,0` + `1920x1080`），
  所以「移动 / 缩放窗口后是否又偷偷截图」可以直接数日志，而不是靠肉眼判断。

**做题会话：先判定，再下一题。** 每道题都必须先有结果才能翻页 —— 选择题走
`选答案 → 提交答案 → 判定` 或 `显示答案 → 看答案`，两条路都会打开 `下一题 ▶`（以及 `→`）；
**主观题**（无选项、只读展示）没有可判定的东西，**从不被闸门拦住**，看完答案直接翻页。
`←` 上一题与 `Esc` 退出不受影响，所以禁用态不会困住人。**显示答案刻意不记录任何作答**：它只把 `shown` 置真 ——
选择题按「提交后」的同一套渲染高亮正确项并展开解析 —— 不写 `answered` / `correct`、不写每题统计、不进错题集，
题目仍是「未做过」，下次还会出现，这正是复习它的意义。数据里既没有答案也没有解析的题，
按钮照常出现但**禁用**（悬停提示「这道题在数据里没有答案」），不会画出空解析框。

**动效（刻意克制）**：

* 分段选项卡的指示器随选中项**滑动并拉伸**（`animate_value_with_time`）。
* 行悬停高亮。
* 答错时选项块**短暂水平抖动**（`Animator::shake`，振幅 `anim::SHAKE_AMPLITUDE`；几何有单测 + headless egui 断言）。
* **重绘调度**：`animate_*` 由 egui 自动按需重绘；手写时钟动画只在 `Animator::is_live()` 为真时
  `request_repaint()`，空闲即让进程睡眠 —— 不空转烧 CPU。

**改视觉时不可破坏的约束**：

* 动画 / 悬停状态**绝不置 `dirty`**（否则每帧写 `current.json`，磁盘抖动 + 污染当前上下文文件）。
* **egui 面板自带的分隔线要关掉**（`.show_separator_line(false)`）：`Panel` 默认会在自己的边界再画一条线
  （用 `noninteractive.bg_stroke`），和 `glass::divider_top/bottom/right` 叠起来就会出现「多了一条分割线」。
  所有发丝线一律由 `glass::divider_*` 用 `glass.divider` 画。
* 自定义控件用 `Sense::CLICK`，**不要**用 `Sense::click()` —— 后者可获焦点，会让
  `egui_wants_keyboard_input()` 变真，从而**杀死 `←/→` 键盘导航**。
* `show_rows` 行高必须恒为 `glass::widgets::ROW_HEIGHT`，**不要**动画行高。
* 含交互子元素（链接 / chip）的容器**不要**加 `Sense::CLICK`，否则吞掉子元素点击。
* 不新建 `FontFamily`（只调字号），以免破坏 CJK 回退而出现豆腐块。

## 7. `state\current.json` 契约

GUI 在任何“当前题目 / 领域 / 科目”变化时写入（临时文件 + rename 原子替换）：

```json
{
  "ts": "2026-10-15T03:49:09Z",
  "domain": "cpa",
  "tab": "practice",
  "subjectId": "kuaiji",
  "subjectName": "会计",
  "question": {
    "id": "...", "stem": "...", "type": "single_choice", "typeRaw": "single",
    "chapter": "...", "year": null,
    "options": [ { "label": "A", "text": "..." } ],
    "answer": ["A"], "answerText": null, "explanation": "...",
    "knowledgePoints": ["..."],
    "source": { "repo": "...", "path": "...", "url": null }
  },
  "knowledgePoints": [
    { "id": "...", "title": "...", "chapter": "...", "content": "...",
      "mnemonic": null, "summary": null,
      "source": { "repo": "...", "path": "...", "url": null } }
  ]
}
```

* `question` 不在做题会话中（如停留在练习设置）时为 `null`，`knowledgePoints` 也为空。
* `tab` 字段保留但**恒为 `"practice"`** —— 学习 tab 已移除，现在只有练习。
* `knowledgePoints` = 当前题目的关联知识点；会话视图不再展示这些 chip，但数据仍写入 `current.json`。

## 8. 出处

每个知识点与题目都携带 `source { repo, path, nativeId, url }`，GUI 在**做题会话**中展示
`repo · path · url`，并给出可点击的原文链接（当 `url` 存在时）。

**这一行和题干 / 选项同宽**（会话列宽 `col_w`，上方发丝线也正好是它）。
出处字符串里常有一整条路径或 URL —— 一个**断不开的长词**：默认断行会优先在最后一个**空格**处断，
于是整条路径 / URL 被丢到下一行、当前行右侧空出一大截（用户报的「没占满宽度」）。
所以这一行的 `LayoutJob` 打开 `break_anywhere`（`practice.rs::provenance_job`）：
行会一直填到列的右边缘再断；`Label` 负责首行缩进（让过「来源/出处：」）与换行宽度，短出处不受影响。

## 9. 数据管线：`classify`（语料 → 官方考纲）

`data\` 由 `classify.exe` 生成。它读一份科目语料 + 一棵官方考纲树，把每道题归到官方 **科目 → 章**，
写出 `subjects\<id>.json` + 重算的 `summary.json` + `classification-report.md`。
**四个参数都必填**；确定性、可重跑（同输入 → 逐字节相同输出，仅 `summary.json` 带 `generatedAt` 时间戳），**从不写回语料**：

```powershell
.\target\release\classify.exe --domain law `
  --data-root D:\Dev\StudyApp\data\corpus\law `
  --taxonomy D:\Dev\StudyApp\data\law\taxonomy.json `
  --out D:\Dev\StudyApp\data\law
```

* `--domain` — `law` 或 `cpa`
* `--data-root` — 科目语料根目录（原始 CMF 数据）
* `--taxonomy` — 官方考点分类树 `taxonomy.json`
* `--out` — 输出目录（写入 `subjects\` / `summary.json` / `classification-report.md`）

**分类分层（策展字段优先）**：

| 层 | 依据 |
| --- | --- |
| Tier 1 | 题目自带的 `chapter` **精确命中官方章名**（或互相包含，取最长匹配） |
| Tier 2 | 从 `chapter` 提取章号（`第X章` / `12.1`），唯一对应某章 |
| Tier 3 | `knowledgePoints` 标题精确命中某章 |
| Tier 4 | 兜底：加权 n-gram 混合（3.0×知识点 + 2.5×自带 chapter + 0.5×题干）；领先第二名不足 20% 则降级为 `low` |

每题写 `chapter`（官方章名或 `null`）、`chapterOfficial`、`chapterConfidence`（`high` / `low` / `none`）。
注会另有 `excluded` 桶，收纳初级 / 中级会计的串考污染。

**科目归属（试卷约束）**：重派某题的科目时，分类器会把它**限制在该题所属试卷允许的科目**里 ——
法考（2018+）用考纲自带的 **卷一 / 卷二**，司考（≤2017）按该考试固定结构（卷一 综合、卷二 刑事与行政、
卷三 民商事；卷四 主观题，不约束）。约束**只在题源自带的 `subjectId` 不是官方科目时**才生效。
本轮有 **933 题**因此改了科目，而它们此前**全部落在自己试卷之外**，所以这次改动**按构造是非退化的**。
**局限（如实说）**：它只保证「落在正确的**试卷**」，并不保证「落在试卷内的正确**科目**」。

**当前实测（据实，不夸大）**：

* 法考 52,710 题 / 注会 14,428 题，总量与语料**完全一致**。
* 法考：high 32,060 · low 18,679 · none 1,971；注会：high 9,396 · low 3,561 · none 1,471。
* 法考语料原有 **69 道题干乱码**：4 道从语料里的干净副本修复，其余 **65 道**因**上游原文同样损坏**
  （乱码是上游仓库的字节丢失，不是可重新解码的编码混用）而删除 —— 这正是 52,775 → 52,710 的来源；
  注会数据不变。

**已知局限**：法考约 70% / 注会约 54% 的题仍落到 Tier 4 —— 语料自带的 `chapter` 是**节级**标签，
与官方章名对不上，且考纲对民法 / 商法 / 经济法 / 环境资源法 / 劳动社保 / 宪法**没有分节**。部分章因此**过度承接**
（如商法 / 公司法约 4,000 题）。`low` / `none` 的含义是「**不可信**」—— app 会把它们照实呈现，而不是悄悄判错。

`classify.rs` 有 **18 个单测**守住分层与去重逻辑（Tier 1–4、章号解析、试卷约束（法考 / 司考各期）、CPA 串考排除、输出字段兼容）。
