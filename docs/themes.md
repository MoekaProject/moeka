# Theme color guidance

Moeka has 16 light and 16 dark themes. Each owns a full set of surfaces, readable text, interface semantics, Markdown colors and code-token colors; changing only the accent is not enough. Theme names, saved preferences, monochrome Markdown and the optional dark sidebar remain compatible.

## Art direction

The document comes first. Offer both genuine pure-white canvases and tinted neutrals, with a small set of related accent families for Markdown. Code may need more hues to distinguish grammar roles, but they should share the theme's temperature and contrast rather than compete with the prose.

| Direction | Light examples | Dark examples |
| --- | --- | --- |
| Quiet, precise ink | Porcelain Mist, Paper White, Silver Morning | One Dark, GitHub Dark |
| Warm paper, olive and copper | Linen Paper, Vanilla Latte, Sunlit Canvas | Milky Dark, Gruvbox Dark, Obsidian Ember |
| Botanical | Sage Garden, Willow Mist | Everforest Dark |
| Glacial / marine | Studio Daylight, Nordic Snow, Coastal Fog | Nordic Night, Solarized Dark, Moonlight Harbor |
| Crimson / violet | Rose Dawn, Pearl Morning, Lavender Haze | Rose Pine, Catppuccin Mocha, Dracula Night |
| Warm expressive accents | Apricot Bloom, Solarized Light | Ayu Mirage, Monokai Pro, Cobalt Velvet |

These are directions, not identical palettes. Light themes deliberately change the color relationships of common code roles, not just their surface tint. Markdown has a related but separate editorial hierarchy. Dark themes retain the previous palette pass; for example, Moonlight is marine teal, distinct from Cobalt's deep indigo.

### White and seven-hue coverage

Paper White and Studio Daylight both use an opaque `#FFFFFF` document canvas and elevated surface, including in Monochrome mode. Paper White pairs neutral gray chrome with quiet ink; Studio Daylight pairs blue-tinted chrome with a more colorful IDE palette. Code blocks and inline code keep separate pale backgrounds. This does not override the optional dark sidebar.

The spectrum is represented by each theme's canvas, interface accent and Colorful Markdown headings—not merely a stray syntax token:

| Family | Representative | Direction |
| --- | --- | --- |
| Red | Rose Dawn | Pale red paper and crimson ink, rather than magenta |
| Orange | Apricot Bloom | Apricot paper and burnt-orange ink, rather than vermilion |
| Yellow | Sunlit Canvas | Pale butter-yellow paper and deep-gold ink |
| Green | Sage Garden | Sage paper and forest ink; Willow Mist adds yellow-green |
| Cyan | Coastal Fog | Sea-glass paper and teal ink |
| Blue | Nordic Snow | Glacial paper and navy ink; Studio Daylight adds a white-canvas choice |
| Purple | Lavender Haze | Lavender paper and violet ink; Pearl Morning adds berry accents |

Small yellow and orange text must use deeper gold/orange ink for contrast, not bright spectrum swatches on a light background. Monochrome deliberately neutralizes Markdown headings while retaining the canvas, UI accent and code palette.

### The sixteen light code directions

| Theme | Canvas | Keywords | Strings | Functions / calls |
| --- | --- | --- | --- | --- |
| Porcelain Mist | Cool porcelain | Crimson | Navy | Orchid |
| Linen Paper | Warm flax paper | Coffee | Wine | Olive |
| Studio Daylight | Pure white / blue chrome | Cobalt | Brick | Magenta |
| Rose Dawn | Pale red paper | Crimson | Lapis | Aubergine |
| Vanilla Latte | Vanilla / caramel | Caramel | Amethyst | Raspberry |
| Sage Garden | Blue-green sage | Forest | Rust | Plum |
| Nordic Snow | Glacial blue | Navy | Cyan | Heather |
| Solarized Light | Old ivory | Olive | Jade | Azure |
| Sunlit Canvas | Butter-yellow canvas | Saffron | Petrol | Vermilion |
| Paper White | Pure white / neutral chrome | Graphite | Mahogany | Navy |
| Pearl Morning | Opaline violet | Mulberry | Indigo | Emerald |
| Willow Mist | Yellow-green mist | Moss | Amethyst | Claret |
| Coastal Fog | Sea glass | Teal | Cobalt | Copper |
| Lavender Haze | Blue lavender | Violet | Emerald | Berry |
| Apricot Bloom | Apricot paper | Burnt orange | Olive | Cobalt |
| Silver Morning | Cool steel | Steel blue | Ruby | Graphite |

Color names describe the relationships, not fixed textbook swatches. Small tokens use deep, contrast-checked ink. Variables, types, operators, numbers and comments complete each palette without forcing all sixteen themes into green strings and blue calls. Low-chroma Paper White and Silver remain deliberate quieter choices, but use different string/call relationships.

## Mermaid geometry and theme colors

Mermaid diagrams are compiled to SVG by Ondine. Their background and foreground colors are derived from the current editor theme, so light and dark appearances update automatically; the SVG canvas remains transparent.

## Token ownership

- `Theme::curated` connects surface specs, `MarkdownSyntaxPalette` and `CodeSyntaxPalette`.
- Markdown headings establish hierarchy; bold/italic provide supporting ink, links remain identifiable, and source markers stay subordinate. Color does not replace weight, italic style, underlines or structural markers.
- Markdown's role colors are intentionally authored separately from code grammar roles. A heading is not automatically the same color as a code keyword.
- Italic text uses `theme.italic`; inline code retains its own foreground/background/font even inside emphasis. Monochrome changes Markdown ink but preserves code syntax and links.
- Inline code uses the explicit opaque background. Code-block borders use the dedicated alpha-bearing border token, not a substituted UI hairline; frames should remain quiet.
- Identified code languages use all nine syntax tokens; unlabelled code retains the normal theme text. Source code/math/diagram/table highlighting shares the palette and must remain readable on the document canvas too.
- Highlighter cache keys include the entire code palette. Global appearance changes also invalidate inactive pane/tab shaped-text caches, with lazy geometry reset rather than an eager document re-layout.

## Validation

Author colors in perceptual space (OKLab/OKLCH), then bake sRGB literals into `src/theme.rs`. No palette generation, contrast search, new dependency or network call belongs in the render path.

Automated guards cover:

- 4.5:1 for small Markdown text in Colorful and Monochrome, including source markers, strikes and the actual inline-code background.
- 4.5:1 for all nine code tokens against both the code-block and raw-source backgrounds.
- Primary/muted interface text across canvas, sidebar, elevated, hover and selected surfaces, plus semantic labels and text-on-accent.
- Alpha-composited contrast, opaque light surfaces, distinct chrome/inline-code backgrounds and restrained code borders. Exact `#FFFFFF` canvas/elevated/active-tab surfaces in both white themes survive Colorful/Monochrome and interface contrast normalization.
- Seven-hue representative identities across canvas, accent and Colorful heading, checked with OKLCH hue/chroma rather than incidental code-token colors.
- A representative OKLab palette-distance floor within each catalog; bytewise inequality alone does not demonstrate visual variety.
- A separate code-only check over all 120 light-theme pairs: keyword/operator/string/function RMS ≥ 0.08, chromatic a/b RMS ≥ 0.065, and at least three of those four roles differing by ≥ 0.05. Surfaces, Markdown, comments, rare types and numbers cannot compensate for near-identical common code colors; the chromatic check also prevents merely changing lightness. These are catalog regression guards, not empirical token-frequency measurements, design substitutes or color-vision accessibility certification.
- Production text-run mapping for all 16 light themes across Rust, JavaScript and Python, in both fenced Markdown and raw-code-file routes. Verify complete token ranges, byte boundaries, monospace/comment styles, real lexer role assignment and A → B → A warm-cache behavior for formerly similar light-theme pairs.
- Both sidebar preferences and pane-state invalidation.

```sh
cargo test --release --locked --bin moeka theme::tests
cargo test --release --locked --bin moeka style::tests
cargo test --release --locked --bin moeka editor::helpers::tests
cargo test --release --locked --bin moeka global_layout_invalidation_
```

For native review, compare the same document across themes with ordinary text, headings, emphasis, links, inline code, tables, checked tasks, and Rust/JavaScript code. Also inspect raw math and Mermaid in Source mode. Switch Colorful/Monochrome, toggle the dark-sidebar preference, warm two code-file panes, cycle light → dark → light, and reactivate inactive tabs. Confirm readable selection/search states, no stale foreground colors, quiet borders and stable scrolling. Static palette studies and numerical tests are not native app screenshots or GUI verification.
