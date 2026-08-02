---
category: 入门教程
description: 介绍小语文稿的使用
---
# 小语文稿
小语文稿是一个优雅，流畅，本地优先的 **Markdown 编辑应用**。它使用起来非常简单，但也能够胜任重度写作者的需求。你可以在 macOS，~~Windows 和 Linux~~（稍后推出）上使用它，我们希望这款简洁的编辑器能让所有人都能快速建立自己的知识库，阅读下面的指导帮助你更快的了解。

使用 `Markdown` 语言作为文本编辑格式，它非常适合文字工作者，在尽可能做到良好可读性的同时不干扰创作，小语文稿支持的语法可以通过右键菜单预览，你也可以用其标注的[快捷键](#表格语法)快速更改格式。另外也支持在空行内输入 `/` 来唤出格式菜单，键入关键字来快速选择需要插入的格式。

> Markdown 格式的文档应当是可作为纯文本发布的，没有格式化的样式，它的语法完全由标点符号组成，这些符号经过仔细筛选，让其本身就代表了字面上的意思，例如星号标记实际看起来是强调，列表标记看起来就像列表。—— John Gruber

![Moeka 网站](https://moeka.app/images/website.png)
### 无序列表
- 这是一个无序列表；
  - 这是二级无序列表；
  - 这是第二项；
- 这是第二项；
### 有序列表
1. 这是一个有序列表；
   1. 这是二级有序列表；
   2. 这是第二项；
2. 这是第二项；
   - 这是一个无序列表；
   - 这是第二项；
### 待办列表
- [x] 可以在这里录入一些待办事项。
  - [ ] 也可以有二级代办事项。
  - [ ] 试试在该行内按下 `Tab` 或 `Shift + Tab`。
- [ ] 追加录入一些已完成事项。
***
### 表格交互
| 功能                       | 快捷键   |
| -------------------------- | -------- |
| `Command + B`              | 加粗     |
| `Command + I`              | 斜体     |
| `Command + U`              | 下划线   |
| `Command + Shift + X`      | 删除线   |
| `Command + E`              | 行内代码 |
| `Command + Option + C`     | 代码块   |
| `Command + Option + Q`     | 引用     |
| `Command + Option + U`     | 无序列表 |
| `Command + Option + O`     | 有序列表 |
| `Command + Option + Enter` | 待办事项 |
| `Command + K`              | 链接     |
| `Command + Option + T`     | 表格     |
| `Command + 0`              | 正文     |
| `/ image`                  | 图片     |

| --------------------- | -------- |
| --------------------- | -------- |
| `[[`                  | 双向链接 |
| `/ image`             | 图片     |
## 代码块语法
```rust
fn greet(name: &str) {
  println!("hello, {name}");
}
```

### LaTeX 数学公式
输入一个简单的行级公式，即可在文稿中看到块级预览效果。

$$
E = mc^2
$$

### Mermaid 图表示例
小语文稿使用纯 Rust 渲染器显示 Mermaid 图表。下面覆盖当前可渲染的 27 种图表类型；ZenUML 采用无浏览器兼容子集。

#### 1. 流程图 Flowchart
```mermaid
flowchart LR
  User([用户]) --> Gateway{已登录?}
  Gateway -->|否| Login[登录与二次验证]
  Login --> Gateway
  Gateway -->|是| Router[请求路由]
  subgraph Services[业务服务]
    Router --> Editor[文稿服务]
    Router --> Search[全文搜索]
    Router --> Export[导出服务]
  end
  Editor --> DB[(文稿数据库)]
  Search --> Index[(搜索索引)]
  Export --> Queue[[任务队列]]
  Queue --> PDF[生成 PDF]
  PDF --> Done([下载文件])
```

#### 2. 时序图 Sequence Diagram
```mermaid
sequenceDiagram
  autonumber
  actor U as 用户
  participant E as 编辑器
  participant A as API
  participant D as 数据库
  U->>E: 打开文稿并输入内容
  E->>A: 保存增量版本
  activate A
  A->>D: 开启事务并写入
  alt 写入成功
    D-->>A: 返回版本号
    A-->>E: 200 + version
    E-->>U: 显示“已保存”
  else 发生冲突
    D-->>A: version conflict
    A-->>E: 409 + remote patch
    E-->>U: 打开冲突对比
  end
  deactivate A
```

#### 3. 类图 Class Diagram
```mermaid
classDiagram
  class Document {
    +String id
    +String title
    +save() Result
    +render() Preview
  }
  class MarkdownDocument {
    +String source
    +parse() Ast
  }
  class PreviewRenderer {
    <<interface>>
    +render(Document) Image
  }
  class MermaidRenderer {
    +Theme theme
    +render(Document) Svg
  }
  Document <|-- MarkdownDocument
  PreviewRenderer <|.. MermaidRenderer
  MarkdownDocument "1" *-- "0..*" Attachment
  MarkdownDocument --> PreviewRenderer : uses
```

#### 4. 状态图 State Diagram
```mermaid
stateDiagram-v2
  [*] --> Idle
  Idle --> Editing : 输入文字
  Editing --> Saving : 自动保存
  Saving --> Synced : 写入成功
  Saving --> Conflict : 版本冲突
  Conflict --> Editing : 合并修改
  Synced --> Editing : 继续输入
  state Previewing {
    [*] --> Parsing
    Parsing --> Rendering
    Rendering --> Ready
  }
  Editing --> Previewing : 打开预览
  Previewing --> Editing : 返回编辑
  Synced --> [*] : 关闭文稿
```

#### 5. 实体关系图 ER Diagram
```mermaid
erDiagram
  WORKSPACE ||--o{ DOCUMENT : contains
  DOCUMENT ||--o{ VERSION : records
  DOCUMENT ||--o{ ATTACHMENT : owns
  USER ||--o{ WORKSPACE : manages
  USER }o--o{ DOCUMENT : edits
  WORKSPACE {
    uuid id PK
    string name
    datetime created_at
  }
  DOCUMENT {
    uuid id PK
    uuid workspace_id FK
    string title
    text content
  }
  VERSION {
    uuid id PK
    uuid document_id FK
    int revision
    datetime saved_at
  }
```

#### 6. 甘特图 Gantt
```mermaid
gantt
  title 2.0 版本发布计划
  dateFormat YYYY-MM-DD
  axisFormat %m/%d
  excludes weekends
  section 设计
  需求确认           :done, req, 2026-08-03, 3d
  交互原型           :done, proto, after req, 4d
  section 开发
  编辑器内核         :active, core, after proto, 8d
  Mermaid 预览       :mermaid, after proto, 6d
  导出与打印         :export, after core, 5d
  section 质量
  回归测试           :test, after mermaid, 5d
  发布候选版         :milestone, rc, after test, 0d
```

#### 7. 用户旅程图 Journey
```mermaid
journey
  title 从灵感到发布
  section 捕捉
    新建文稿: 5: 作者
    快速记录想法: 5: 作者
    添加参考资料: 3: 作者
  section 整理
    调整章节结构: 4: 作者
    插入图表与公式: 4: 作者, 编辑器
    检查链接: 3: 编辑器
  section 发布
    预览排版: 5: 作者, 编辑器
    导出 PDF: 4: 编辑器
    分享成品: 5: 作者, 读者
```

#### 8. 饼图 Pie Chart
```mermaid
pie showData
  title 一周写作时间分配
  "研究与阅读" : 28
  "初稿写作" : 34
  "修订润色" : 20
  "图表与排版" : 10
  "发布归档" : 8
```

#### 9. Git 分支图 Git Graph
```mermaid
gitGraph LR:
  commit id: "bootstrap"
  branch develop
  checkout develop
  commit id: "editor-core"
  branch feature/mermaid
  checkout feature/mermaid
  commit id: "parser"
  commit id: "themes" tag: "preview"
  checkout develop
  commit id: "export"
  merge feature/mermaid id: "merge-preview"
  checkout main
  merge develop id: "release" tag: "v2.0"
```

#### 10. 思维导图 Mindmap
```mermaid
mindmap
  root((小语文稿))
    写作
      Markdown
      双向链接
      表格与任务
    可视化
      Mermaid
        流程与时序
        数据图表
      LaTeX
    本地优先
      工作区
      版本历史
      附件管理
    输出
      PDF
      图片
      纯文本
```

#### 11. 时间线 Timeline
```mermaid
timeline
  title Markdown 编辑器演进
  2024 : 基础编辑器
       : Markdown 实时渲染
  2025 : 工作区与全文搜索
       : Git 历史和文件对比
  2026 Q1 : 数学公式与图表预览
          : PDF 导出
  2026 Q2 : 纯 Rust Mermaid 引擎
          : 深浅色主题
  未来 : 协作审阅
       : 更多发布格式
```

#### 12. 四象限图 Quadrant Chart
```mermaid
quadrantChart
  title 功能价值与实现成本
  x-axis 低成本 --> 高成本
  y-axis 低价值 --> 高价值
  quadrant-1 战略投入
  quadrant-2 优先交付
  quadrant-3 暂缓处理
  quadrant-4 谨慎评估
  自动保存: [0.18, 0.92]
  全文搜索: [0.32, 0.80]
  Mermaid 预览: [0.48, 0.76]
  实时协作: [0.88, 0.90]
  自定义动画: [0.72, 0.30]
  主题市场: [0.60, 0.45]
```

#### 13. XY 图表 XY Chart
```mermaid
xychart-beta
  title "预览性能基准"
  x-axis [100, 250, 500, 1000, 2000, 4000]
  y-axis "渲染耗时（ms）" 0 --> 160
  line [8, 13, 22, 41, 78, 145]
  line [4, 7, 12, 20, 37, 69]
  bar [6, 10, 18, 34, 63, 118]
```

#### 14. 桑基图 Sankey
```mermaid
sankey-beta
访问首页,打开工作区,820
访问首页,离开,180
打开工作区,新建文稿,310
打开工作区,继续编辑,510
新建文稿,完成初稿,240
新建文稿,放弃,70
继续编辑,完成修订,430
继续编辑,暂存,80
完成初稿,导出,180
完成修订,导出,360
导出,分享,420
导出,仅归档,120
```

#### 15. 需求图 Requirement Diagram
```mermaid
requirementDiagram
  requirement autosave {
    id: REQ-01
    text: 编辑内容必须自动持久化
    risk: high
    verifymethod: test
  }
  functionalRequirement conflict_detection {
    id: REQ-02
    text: 保存前检测远端版本冲突
    risk: medium
    verifymethod: demonstration
  }
  element editor {
    type: application
    docref: editor-spec.md
  }
  element storage {
    type: service
    docref: storage-spec.md
  }
  editor - satisfies -> autosave
  storage - verifies -> autosave
  autosave - contains -> conflict_detection
  editor - satisfies -> conflict_detection
```

#### 16. C4 架构图
```mermaid
C4Context
  title 本地优先写作系统
  Person(author, "作者", "撰写和整理内容")
  Person(reader, "读者", "阅读发布内容")
  System_Boundary(moeka, "小语文稿") {
    System(editor, "桌面编辑器", "Rust / GPUI")
    System(indexer, "搜索索引", "本地全文索引")
    SystemDb(store, "工作区文件", "Markdown 与附件")
  }
  System_Ext(cloud, "可选同步服务", "跨设备备份")
  Rel(author, editor, "编辑文稿")
  Rel(editor, store, "读写文件")
  Rel(editor, indexer, "更新索引")
  Rel(editor, cloud, "加密同步", "HTTPS")
  Rel(reader, cloud, "访问分享内容")
```

#### 17. 架构图 Architecture
```mermaid
architecture-beta
  group client(cloud)[客户端]
  service desktop(internet)[桌面编辑器] in client
  service preview(server)[预览引擎] in client
  group core(server)[核心服务]
  service parser(server)[Markdown 解析器] in core
  service renderer(server)[Mermaid 渲染器] in core
  group data(database)[本地数据]
  service files(disk)[文稿文件] in data
  service index(database)[搜索索引] in data
  desktop:R --> L:parser
  parser:R --> L:preview
  preview:B --> T:renderer
  desktop:B --> T:files
  parser:B --> T:index
```

#### 18. 块图 Block Diagram
```mermaid
block-beta
  columns 4
  Client["编辑器"]:1
  space:1
  Gateway["命令总线"]:2
  Parser["解析器"]
  Preview["预览器"]
  Search["搜索器"]
  Export["导出器"]
  Files[("文稿")]:2
  Index[("索引")]:2
  Client --> Gateway
  Gateway --> Parser
  Parser --> Preview
  Parser --> Search
  Preview --> Export
  Search --> Index
  Export --> Files
```

#### 19. 数据包图 Packet Diagram
```mermaid
packet-beta
  title 文稿同步帧（64 bit）
  0-3: "版本"
  4-7: "标志"
  8-15: "消息类型"
  16-31: "文稿 ID"
  32-47: "修订号"
  48-55: "分片序号"
  56-62: "负载长度"
  63: "校验位"
```

#### 20. 看板 Kanban
```mermaid
kanban
  backlog[待处理]
    research[调研 Mermaid 语法]@{ assigned: "设计", priority: "High" }
    docs[补充帮助文档]@{ assigned: "文档", priority: "Low" }
  progress[进行中]
    renderer[接入纯 Rust 渲染器]@{ assigned: "核心", priority: "Very High" }
    themes[完善深浅色主题]@{ assigned: "界面", priority: "High" }
  review[审核]
    regression[运行回归测试]@{ assigned: "质量", priority: "High" }
  done[已完成]
    parser[解析器迁移]@{ assigned: "核心", priority: "Low" }
```

#### 21. 矩形树图 Treemap
```mermaid
treemap-beta
  title 工作区文件占用
  "文稿"
    "长篇写作": 42
    "日常笔记": 28
    "项目文档": 18
  "附件"
    "图片": 35
    "PDF": 16
    "音频": 8
  "缓存"
    "搜索索引": 12
    "预览图片": 9
```

#### 22. 雷达图 Radar
```mermaid
radar-beta
  title 编辑器能力对比
  axis Speed["速度"], Quality["排版"], Offline["离线"], Search["搜索"], Export["导出"], Ext["扩展性"]
  curve Moeka["小语文稿"]{88, 92, 100, 86, 84, 72}
  curve Browser["浏览器编辑器"]{68, 80, 35, 78, 75, 92}
  curve Plain["纯文本工具"]{96, 45, 100, 52, 38, 65}
  ticks 5
  min 0
  max 100
  graticule polygon
  showLegend true
```

#### 23. 韦恩图 Venn
```mermaid
venn-beta
  title 本地优先写作能力
  set Writing["写作"]:30
    text W1["Markdown"]
    text W2["大纲"]
  set Knowledge["知识管理"]:26
    text K1["双向链接"]
  set Publishing["发布"]:24
    text P1["PDF"]
  union Writing,Knowledge["结构化笔记"]:12
    text WK["标签与搜索"]
  union Writing,Publishing["成稿"]:10
  union Knowledge,Publishing["知识分享"]:8
  union Writing,Knowledge,Publishing["完整工作流"]:4
```

#### 24. ZenUML（兼容子集）
```mermaid
zenuml
  title 文稿发布流程
  Author
  Editor
  Exporter
  Author->Editor: 提交发布请求
  Editor.Validate() {
    Editor->Editor: 检查链接与资源
  }
  if(valid) {
    Editor->Exporter: 生成 PDF
    Exporter-->Author: 返回下载地址
  } else {
    Editor-->Author: 返回校验问题
  }
  opt {
    Author->Editor: 保存发布快照
  }
```

#### 25. 树视图 Tree View
```mermaid
treeView-beta
  title 工作区目录
  "我的工作区"
    "写作"
      "长篇小说"
        "第一章.md"
        "第二章.md"
      "随笔.md"
    "研究"
      "阅读笔记"
      "参考资料"
        "论文.pdf"
        "数据.csv"
    "assets"
      "images"
      "exports"
```

#### 26. 鱼骨图 Ishikawa
```mermaid
ishikawa-beta
  预览渲染不清晰
    字体
      缺少 Emoji 字体
      CJK 字宽估算错误
    布局
      节点间距不足
      长标签未换行
    颜色
      文本对比度不足
      深浅色变量混用
    输入
      复杂 Mermaid 语法
      自定义样式覆盖
    输出
      SVG 兼容性
      位图缩放失真
```

#### 27. 事件建模图 Event Modeling
```mermaid
eventmodeling
  timeframe 01 ui EditorOpened
  timeframe 02 command CreateDocument { title: "新文稿" } ->> 01
  timeframe 03 event DocumentCreated [[DocumentData]] ->> 02
  timeframe 04 view DocumentList ->> 03
  timeframe 05 command EditContent { revision: 1 } ->> 04
  timeframe 06 event ContentChanged [[RevisionData]] ->> 05
  timeframe 07 view LivePreview ->> 06
  resetframe 08 event WorkspaceRestored ->> 03 ->> 06

  data DocumentData {
    id: "doc-001"
    title: "新文稿"
  }
  data RevisionData {
    documentId: "doc-001"
    revision: 1
  }
```

## 行内语法
- 可以使用**加粗**或者带有 *italic* 斜体语法来表示强调。
- 也可以使用++下划线++来标记需要补充注意的短语；`~U~` 会作为下标，而不是下划线。
- 下划线示例：++underline++，也兼容 HTML 写法 <u>underline</u>。
- 也可以使用 `行内代码` 来标记某个关键词或代码行。
## 提示块语法
> [!TIP] 这是一条提示块标题
> 可以在提示块里继续书写正文；折叠后会显示为对应的 emoji，标题会使用加粗样式。

> [!WARNING] 注意
> 也可以使用 NOTE、IMPORTANT、WARNING、CAUTION 等不同类型。
## 链接语法
- 可以使用[内部链接](#英文示例)，跳转到当前文件内的标题行。
- 通过 [[我的设计风格.md]] 双向链接语法在文件间互相引用。
- 或者使用[外部链接](https://google.com)，跳转到外部的任意链接。
- 还可以使用脚注语法[^note]跳转到脚注行用于引用参考。
- 另外可以使用[文件链接](功能规划.md)跳转到指定文件的指定标题。
- 以上所有语法，都可以按住 `Command` 键并点击后跳转。
## 本地附件
- 在 Markdown 文档中粘贴图片或文件，或者从 Finder 拖入文件时，小语文稿会把资源复制到“设置”中选择的附件位置。从小语文稿侧边栏拖入文件只会插入链接，不会重复复制。
- 附件可以保存在当前文档同级、工作区指定目录，或当前文档旁的子文件夹。自定义目录必须是工作区内的相对路径。
- 新插入的附件使用文档相对路径和标准 Markdown 链接，例如 `![](../assets/image.png)`。
- 已有的 Markdown 链接和 `![[assets/image.png]]` 形式的 WikiLink 均可解析和预览。
- 移动或重命名文件、文件夹后，现有文档中的链接不会自动更新。
## 行内扩展语法
- 下标示例：H~2~O，上标示例：E=mc^2^，可以用英文：Foo~zoo~ 或者 Bar^baz^ 等。
- 另外也支持==高亮==语法，让文本显得更加醒目强调。
- 还支持类似 `:rocket:` 这类 emoji shortcode 的语法，例如 :rocket: 。
- [^note]: 这里就是脚注行了，可以从其他地方跳转过来。

有关于更多 Markdown 格式的说明，可以参考 Github 的 [Markdown 语法指南](https://docs.github.com/en/get-started/writing-on-github/getting-started-with-writing-and-formatting-on-github/basic-writing-and-formatting-syntax)，了解和尝试更多基本写法。
