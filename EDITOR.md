---
category: 入门教程
description: 介绍小语文稿的使用
---
# 小语文稿
小语文稿是一个优雅，流畅，本地优先的 **Markdown 编辑应用**。它使用起来非常简单，但也能够胜任重度写作者的需求。你可以在 macOS，~~Windows 和 Linux~~（稍后推出）上使用它，我们希望这款简洁的编辑器能让所有人都能快速建立自己的知识库，阅读下面的指导帮助你更快的了解。

使用 `Markdown` 语言作为文本编辑格式，它非常适合文字工作者，在尽可能做到良好可读性的同时不干扰创作，小语文稿支持的语法可以通过右键菜单预览，你也可以用其标注的[快捷键](#表格语法)快速更改格式。另外也支持在空行内输入 `/` 来唤出格式菜单，键入关键字来快速选择需要插入的格式。
### 引用段落
> Markdown 格式的文档应当是可作为纯文本发布的，没有格式化的样式，它的语法完全由标点符号组成，这些符号经过仔细筛选，让其本身就代表了字面上的意思，例如星号标记实际看起来是强调，列表标记看起来就像列表。—— John Gruber
### 插入图片
![图片描述](https://cdn.cnbj1.fds.api.mi-img.com/product-images/new-product-images/xiaomi-17-ultra-byleica_820/images/5673.png)
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
### 行内语法
| 功能                  | 快捷键   |
| --------------------- | -------- |
| `Command + B`         | 加粗     |
| `Command + I`         | 斜体     |
| `Command + Shift + S` | 删除线   |
| `++`                  | 下划线   |
| `Command + E`         | 行内代码 |
### 行级语法
| 功能                   | 快捷键   |     |
| ---------------------- | -------- | --- |
| `Command + Option + C` | 代码块   |     |
| `Command + Option + Q` | 引用     |     |
| `> [!TIP]`             | 提示块   |     |
| `Command + Option + U` | 无序列表 |     |
| `Command + Option + O` | 有序列表 |     |
| `Command + Option + T` | 待办事项 |     |

| 功能                  | 快捷键   |
| --------------------- | -------- |
| `Command + K`         | 链接     |
| `[[`                  | 双向链接 |
| `Command + Shift + I` | 图片     |
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
用一个最简单的流程图来观察图表预览效果。

```mermaid
flowchart TD
  A["商品页加载<br/>content script document_start 注入"] --> B["登录态检查<br/>chrome.storage.local.pluginAuth"]
  B -->|未登录| B0["不进入商品分析<br/>只等登录态变化"]
  B -->|已登录| C["白名单门控<br/>focus + host 白名单 + Shopify 自动发现"]
  C -->|未命中| C0["静默退出<br/>页面完全不显示插件"]
  C -->|命中| D["挂载 Shadow DOM React Root<br/>但 pluginActive 仍可为 false"]
  D --> E["商品主管线准备<br/>XPath / Shopify JSON / URL PDP / 截图 OCR"]
  E -->|非 PDP| E0["不显示插件"]
  E -->|确认 PDP + 拿到商品| F["暂存 preparedAnalysis<br/>小女孩进入正常态，等待点击"]
  F --> G["用户点击小女孩"]
  G --> H["POST /api/plugin-ai/analyze<br/>带 pageData + extractedProduct + engine"]
  H --> I{"engine?"}
  I -->|server| J["服务端研究 + Tavily + 平台模型最终成稿"]
  J --> K["SSE: analysis_report_start/component/done"]
  I -->|user_ai| L["服务端只做品类 + 模板 + web prompt 备料"]
  L --> M["SSE: bridge_prompt + bridgeHandoff done"]
  M --> N["插件 Bridge Client 接管"]
  N --> O["Bridge Tab 驱动 ChatGPT/Gemini iframe"]
  O --> P["用户 AI 输出 JSONL 原文"]
  P --> Q["POST /analyze/bridge-complete"]
  Q --> R{"服务端验收通过?"}
  R -->|通过| S["返回同构 events 数组"]
  R -->|未 done 且有额度| O2["BRIDGE_CONTINUE 续写"]
  O2 --> P
  R -->|失败/额度尽| T["POST /analyze/bridge-fallback<br/>服务端兜底成稿"]
  T --> K
  S --> U["逐条喂 mapAnalyzeSseEvent"]
  K --> U
  U --> V["analysis-store streamingReport"]
  V --> W["SidePanel 渲染最终组件报告"]
```

### Mermaid 图表示例
```mermaid
flowchart LR 
  PDP["商品页 PDP<br/>Content Script + Shadow DOM"] --> ENTRY["白名单与商品页门控"]
  ENTRY --> EXTRACT["商品提取<br/>XPath / Shopify JSON / 截图 OCR"]
  EXTRACT --> CLICK["用户点击小女孩"]
  CLICK --> API["POST /api/plugin-ai/analyze<br/>SSE"]

  API --> CACHE{"分析缓存命中？"}
  CACHE -->|是| PLAYBACK["服务端回放缓存事件"]
  CACHE -->|否| ENGINE{"engine"}

  ENGINE -->|server| SERVER["服务端研究链<br/>品类 → Planner → Tavily → 最终模型"]
  ENGINE -->|user_ai| HANDOFF["服务端准备 Web Prompt<br/>创建 analysis_bridge_session"]

  HANDOFF --> SW["插件 Service Worker"]
  SW --> PROVIDER{"provider"}
  PROVIDER --> GEMINI["Gemini 网页私有接口"]
  PROVIDER --> GPT["ChatGPT 网页私有接口"]
  GEMINI --> COMPLETE["回答落库 + /complete 校验"]
  GPT --> COMPLETE

  SERVER --> FINALIZE["统一报告校验与事件化"]
  COMPLETE --> FINALIZE
  PLAYBACK --> RENDER["插件报告渲染器"]
  FINALIZE --> RENDER
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
## 行内扩展语法
- 下标示例：H~2~O，上标示例：E=mc^2^，可以用英文：Foo~zoo~ 或者 Bar^baz^ 等。
- 另外也支持==高亮==语法，让文本显得更加醒目强调。
- 还支持类似 `:rocket:` 这类 emoji shortcode 的语法，例如 :rocket: 。
- [^note]: 这里就是脚注行了，可以从其他地方跳转过来。

有关于更多 Markdown 格式的说明，可以参考 Github 的 [Markdown 语法指南](https://docs.github.com/en/get-started/writing-on-github/getting-started-with-writing-and-formatting-on-github/basic-writing-and-formatting-syntax)，了解和尝试更多基本写法。
