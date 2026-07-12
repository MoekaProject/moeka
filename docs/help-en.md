---
category: Getting Started
description: Learn how to use Moeka
---
# Moeka
Moeka is an elegant, fluid, local-first **Markdown editing app**. It is simple to start with, yet capable enough for serious writing. You can use it on macOS, with ~~Windows and Linux~~ coming later. This short guide helps you build your own knowledge base more quickly.

Moeka uses `Markdown` as its text format. Markdown is ideal for writers because it stays readable without getting in the way of drafting. You can preview supported syntax from the context menu, use the listed [shortcuts](#table-interaction), or type `/` on an empty line to open the format menu and search for the format you need.

> A Markdown-formatted document should be publishable as plain text, without looking like it has been marked up with tags or formatting instructions. Its syntax is made entirely of punctuation characters, carefully chosen so they look like what they mean. Asterisks around a word actually look like emphasis, and list markers look like lists. -- John Gruber

![Image description](https://cdn.cnbj1.fds.api.mi-img.com/product-images/new-product-images/xiaomi-17-ultra-byleica_820/images/5673.png)
### Unordered List
- This is an unordered list.
  - This is a nested unordered list.
  - This is the second item.
- This is the second item.
### Ordered List
1. This is an ordered list.
   1. This is a nested ordered list.
   2. This is the second item.
2. This is the second item.
   - This is an unordered list.
   - This is the second item.
### Todo List
- [x] You can write todo items here.
  - [ ] Nested todo items are supported too.
  - [ ] Try pressing `Tab` or `Shift + Tab` on this line.
- [ ] Add more completed items later.
***
### Table Interaction
| Feature                    | Shortcut       |
| -------------------------- | -------------- |
| `Command + B`              | Bold           |
| `Command + I`              | Italic         |
| `Command + U`              | Underline |
| `Command + Shift + X`      | Strikethrough  |
| `Command + E`              | Inline code    |
| `Command + Option + C`     | Code block     |
| `Command + Option + Q`     | Quote          |
| `Command + Option + U`     | Unordered list |
| `Command + Option + O`     | Ordered list   |
| `Command + Option + Enter` | Todo           |
| `Command + K`              | Link           |
| `Command + Option + T`     | Table |
| `Command + 0`              | Paragraph |
| `/ image`                  | Image          |

| --------------------- | --------  |
| --------------------- | --------- |
| `[[`                  | Wiki link |
| `/ image`             | Image     |
## Code Block Syntax
```rust
fn greet(name: &str) {
  println!("hello, {name}");
}
```

### LaTeX Math
Enter a simple display formula to see a block preview in the document.

$$
E = mc^2
$$

### Mermaid Diagram Example
Use a simple diagram to see how chart previews work.

```mermaid
mindmap
  root((mindmap))
    Origins
      Long history
      ::icon(fa fa-book)
      Popularisation
        British popular psychology <br/>author Tony Buzan
    Research
      On effectiveness<br/>and features
      On Automatic creation
        Uses
            Creative techniques
            Strategic planning
            Argument mapping
    Tools
      Pen and paper
      Mermaid
```

```mermaid
pie title Pets adopted by volunteers
    "Dogs" : 386
    "Cats" : 85
    "Rats" : 15
```

## Inline Syntax
- Use **bold** or *italic* text for emphasis.
- Use ++underline++ for phrases that need attention; `~U~` is rendered as subscript, not underline.
- Underline examples: ++underline++ and the HTML form <u>underline</u>.
- Use `inline code` for keywords or short code fragments.
## Callout Syntax
> [!TIP] This is a callout title
> Keep writing body text inside the callout. When folded, it shows the matching emoji and keeps the title bold.

> [!WARNING] Notice
> NOTE, IMPORTANT, WARNING, CAUTION, and other callout types are supported.
## Link Syntax
- Use an [internal link](#moeka) to jump to a heading in the current file.
- Use [[My Design Style.md]] wiki links to connect files.
- Use an [external link](https://google.com) to open any web address.
- Use footnote syntax[^note] to jump to a reference line.
- Use a [file link](Feature Plan.md) to jump to a file or heading.
- Hold `Command` and click any of the above links to open them.
## Local Attachments
- Pasting an image or file, or dropping one from Finder into a Markdown document, copies it to the attachment location selected in Settings. Dropping a file from Moeka's sidebar inserts a link without copying the file.
- Attachment locations can be the document folder, workspace root, a workspace folder, or a subfolder beside the current document. Custom folders must be relative paths inside the workspace.
- Moeka can generate standard Markdown links or Obsidian-style WikiLinks. Path formats include document-relative, workspace-relative, and the shortest unique workspace path.
- Obsidian image embeds such as `![[assets/image.png]]` are displayed inline. Standard Markdown such as `![](assets/image.png)` remains the best choice when the same files are also edited in Typora or other Markdown tools.
- When automatic link updates are enabled, files and folders moved or renamed inside Moeka update local Markdown and WikiLink targets across the workspace. Renames made directly in Finder are not tracked automatically.
## Extended Inline Syntax
- Subscript example: H~2~O. Superscript example: E=mc^2^. English text also works: Foo~zoo~ or Bar^baz^.
- ==Highlight== is supported for stronger emphasis.
- Emoji shortcodes such as `:rocket:` are supported, for example :rocket: .
- [^note]: This is the footnote line that other places can jump to.

For more Markdown details, see GitHub's [Markdown syntax guide](https://docs.github.com/en/get-started/writing-on-github/getting-started-with-writing-and-formatting-on-github/basic-writing-and-formatting-syntax) and try more basic patterns.
