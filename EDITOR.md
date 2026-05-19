---
category: 入门教程
description: 介绍小语文稿的使用
---

# 小语文稿

小语文稿是一个优雅，流畅，本地优先的 **Markdown 编辑应用**。它使用起来非常简单，但也能够胜任重度写作者的需求。你可以在 macOS，~~Windows 和 Linux~~（稍后推出）上使用它，我们希望这款简洁的编辑器能让所有人都能快速建立自己的知识库，阅读下面的指导帮助你更快的了解。

使用 `Markdown` 语言作为文本编辑格式，它非常适合文字工作者，在尽可能做到良好可读性的同时不干扰创作，小语文稿支持的语法可以通过右键菜单预览，你也可以用其标注的[快捷键](#表格语法)快速更改格式。另外也支持在空行内输入 `/` 来唤出格式菜单，键入关键字来快速选择需要插入的格式。

## 行级语法

### 引用段落

> Markdown 格式的文档应当是可作为纯文本发布的，没有格式化的样式，它的语法完全由标点符号组成，这些符号经过仔细筛选，让其本身就代表了字面上的意思，例如星号标记实际看起来是强调，列表标记看起来就像列表。—— John Gruber

### 分割线

---

### 插入图片

![图片描述](https://cdn.cnbj1.fds.api.mi-img.com/product-images/new-product-images/xiaomi-17-ultra-byleica_820/images/5673.png)

---

## 列表语法

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

## 快捷键

### 行内语法

| 功能                  | 快捷键   |
| --------------------- | -------- |
| `Command + B`         | 加粗     |
| `Command + I`         | 斜体     |
| `Command + Shift + S` | 删除线   |
| `Command + E`         | 行内代码 |

### 行级语法

| 功能                   | 快捷键   |
| ---------------------- | -------- |
| `Command + Option + C` | 代码块   |
| `Command + Option + Q` | 引用     |
| `Command + Option + U` | 无序列表 |
| `Command + Option + O` | 有序列表 |
| `Command + Option + T` | 待办事项 |  |

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

## 数学公式与 Mermaid 预览

### LaTeX 数学公式

下面给出几个稍复杂一些的展示公式，便于观察块级预览在多行、矩阵、分段函数和优化目标下的表现。

$$
\begin{aligned}
\nabla \cdot \mathbf{E} &= \frac{\rho}{\varepsilon_0} \\
\nabla \times \mathbf{B} - \mu_0 \varepsilon_0 \frac{\partial \mathbf{E}}{\partial t} &= \mu_0 \mathbf{J} \\
\nabla \cdot \mathbf{B} &= 0 \\
\nabla \times \mathbf{E} &= -\frac{\partial \mathbf{B}}{\partial t}
\end{aligned}
$$

$$
\min_{\theta \in \mathbf{R}^n}
\left(
\frac{1}{m}\sum_{i=1}^{m} L\!\left(f_{\theta}(x_i), y_i\right)
+ \lambda \left\| \theta \right\|_2^2
\right)
$$

$$
A = \begin{bmatrix}
1 & \alpha & \alpha^2 \\
0 & 1 & 2\alpha \\
0 & 0 & 1
\end{bmatrix},
\qquad
f(x) =
\begin{cases}
x^2 \sin \frac{1}{x}, & x \ne 0 \\
0, & x = 0
\end{cases}
$$

### Mermaid 图表示例

先放一个偏流程编排的图，里面包含条件判断、子图和异步保存路径。

```mermaid
flowchart LR
  A[打开文稿] --> B{是否有未保存修改}
  B -->|是| C[写入本地快照]
  B -->|否| D[直接进入编辑区]
  C --> D
  D --> E[触发 Markdown 解析]
  E --> F[计算标题与列表装饰]
  E --> G[提取 LaTeX 与 Mermaid 块]
  G --> H[生成 SVG 预览卡片]
  H --> I[缓存图像并回流布局]
  I --> J[绘制正文与侧边栏]

  subgraph AutoSave[自动保存]
    C --> K[延迟 500ms 去抖]
    K --> L[写入磁盘]
  end
```

再放一个时序图，方便观察多参与者和往返调用下的预览效果。

```mermaid
sequenceDiagram
  participant U as User
  participant E as Editor
  participant P as Parser
  participant C as Cache

  U->>E: 编辑 Mermaid 或公式块
  E->>P: reparse(buffer)
  P-->>E: block_preview(start_line, end_line)
  E->>C: refresh_generated_block_previews()
  C-->>E: 返回 SVG 图像
  E-->>U: 在文稿中显示预览
```

## 行内语法

- 可以使用**加粗**或者带有 *italic* 斜体语法来表示强调。
- 也可以使用`行内代码`来标记某个关键词或代码行。

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
