## 小语文稿

小语文稿是一个优雅，流畅，专注**文本管理**的写作应用ddd。它使用起来非常简单，但也能够胜任重度写作者的需求。你可以在 macOS，~~Windows 和 Linux~~（稍后推出） 上使用它。我们希望这款简洁的工具能让所有人都能快速建立自己的文本知识库，阅读下面的指导帮助你更快的了解。

使用 `Markdown` 语言作为文本编辑格式，它非常适合文字工作者，在尽可能做到良好可读性的同时*不干扰创作*，目前小语文稿支持的语法如右键格式菜单所示，你也可以用格式菜单中标注的快捷键快速更改格式。

1. sidebar 右侧 border 在可拖拽感应范围内，都应该是拖拽光标样式；
2. FileTree item 应该有个透明 border，当 active 时 border 直接变为蓝色，这样可以防止布局抖动；
3. FileTree 的对齐辅助线应该是仅当目录 item 下方（不一定紧挨着）有同级 item（无论类型）时，才出现辅助线，另外辅助线应该是 dashed 效果的，写到 style.rs；
4. 看看 FileTree 的根目录为什么在 logo 上面了，应该紧靠着 FileTree 第一个 item 的。在滚动时，当整个 FileTree 顶部距离窗口 30px 时，再固定根目录 item，其他 item 可以继续滚动；
5. FileTree item 的命名输入框还是没有 focus 状态，并且光标样式应该和 editor 的光标一样，但高度是 item 的文本高度；
6. 需要构建一套组件之间的 focus 状态系统，仅当有 focus 状态的组件，能够接收到键盘响应，例如命名输入框，各种右键菜单；
7. 确认下菜单应该是个统一封装的组件，菜单过长在滚动后，鼠标 Hover item 时，Hover 状态还是会出现跳动的情况，需要修复，另外把所有目前实现的菜单，都使用中文；
8. Editor toolbar 的图标按钮，还是没看到阴影效果，需要仔细看看，也需要设置为 style.rs 变量，另外图标按钮在显隐时，加上透明度渐变动画；
9. Tab item 在 hover 时（无论鼠标在 tab item 哪里），都应该出现 close 按钮（非未保存状态），只是 close 按钮没有背景，hover 到 close 按钮时才有背景；
10. 菜单弹出时，应该计算菜单高度到窗口边缘的距离，能够上下左右自动偏移，以尽可能让所有 item 可见，实在不可见时再启用滚动；
11. sidebar 底部增加 workspace（根目录）切换功能，类似 obsidian，使用 assets/icons/filetree/workspace-expand.svg 图标，显示当前的根目录名称（全大写），点击后弹出菜单，菜单 item 分别是：
   - 已经打开的所有 workspace（也是全大写），对于当前 workspace，有个 assets/icons/check.svg 图标，对于所有 workspace item，右侧有个 x 图标，点击可以关闭 workspace。Workspace item 点击可以切换 FileTree 内容。
   - "打开 Workspace ..." 的 item，点击后是弹出打开目录对话框，用户选择后添加进来，注意 workspace 按照添加顺序排序；
   - 注意把整个 workspace 切换功能的元素风格都写到 style.rs 中，方便我修改；

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

有关于更多 Markdown 格式的说明，可以参考 Github 的 [Markdown 语法指南](https://docs.github.com/en/get-started/writing-on-github/getting-started-with-writing-and-formatting-on-github/basic-writing-and-formatting-syntax)，了解和尝试更多基本写法。

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
