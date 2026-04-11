## 小语文稿

小语文稿是一个优雅，流畅，专注**文本管理**的写作应用。它使用起来非常简单，但也能够胜任重度写作者的需求。你可以在 macOS，~Windows 和 Linux~（稍后推出） 上使用它。我们希望这款简洁的工具能让所有人都能快速建立自己的文本知识库，阅读下面的指导帮助你更快的了解。

使用 `Markdown` 语言作为文本编辑格式，它非常适合文字工作者，在尽可能做到良好可读性的同时*不干扰创作*，目前小语文稿支持的语法如右键格式菜单所示，你也可以用格式菜单中标注的快捷键快速更改格式。

### 语法介绍

> Markdown 格式的文档应当是可作为纯文本发布的，没有格式化的样式，它的语法完全由标点符号组成，这些符号经过仔细筛选，让其本身就代表了字面上的意思，例如星号标记实际看起来是强调，列表标记看起来就像列表。—— John Gruber

---

小语文稿编辑器专注于纯文本编辑，可以插入类似如下的块级别代码：

``` javascript
function toggleSidebar() {
  sidebarVisible.value = !sidebarVisible.value;
  setTimeout(() => monacoEditorRef.value?.layout(), 50);
}
```

| 表头1   | 表头2   |
| ------- | ------- |
| 单元格1 | 单元格2 |
| 单元格3 | 单元格4 |

有关于更多 Markdown 格式的说明，可以参考 Github 的 [Markdown 语法指南](https://docs.github.com/en/get-started/writing-on-github/getting-started-with-writing-and-formatting-on-github/basic-writing-and-formatting-syntax))，了解和尝试更多基本写法。

![图片描述](/Users/imeoer/test.png)

- [x] 可以在这里录入一些待办事项；
  - [x] 也可以有二级代办事项；
  - [ ] 或者添加更多内容；
- [x] 也可以在这里录入一些已完成事项；

- 这是一个无序列表；
  - 这是二级无序列表；
  - 这是第二项；
- 这是第二项；

1. 这是一个有序列表；
   1. 这是二级有序列表；
   2. 这是第二项；
2. 这是第二项；
   - 这是一个无序列表；
   - 这是第二项；

---

This is a **bold** and *italic* demo.

You can write `inline code` and:

```rust
fn greet(name: &str) {
  println!("hello, {name}");
}
```

## Features

- Syntax markers hide when cursor leaves the range
- .md files open into real tabs
- FileTree now lives in a fixed left pane
- `inline code` with background
- Headings with different sizes
- [Visit Google](https://google.com) for search
- Check out ![Logo](crates/zed/resources/app-icon.png) inline image
