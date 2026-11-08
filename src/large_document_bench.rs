use std::hint::black_box;
use std::time::{Duration, Instant};

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

use crate::buffer::Buffer;

const DOCUMENT_LINES: usize = 2_000_000;
const BLOCK_REPETITIONS: usize = 50_000;
const COMPLEX_BLOCK: &str = concat!(
    "## Section heading with `inline code`\n",
    "Paragraph with **bold**, *italic*, ~~strike~~, ==highlight==, H~2~O, and x^2^.\n",
    "A [link](https://example.com), ![image](missing.png), [[wiki note]], and :sparkles:.\n",
    "> [!NOTE] Callout title\n",
    "> Nested quote with **formatting** and `code`.\n",
    "- [ ] Open task\n",
    "- [x] Completed task\n",
    "  - Nested unordered item\n",
    "    1. Nested ordered item\n",
    "1. Ordered item with [reference][docs]\n",
    "2) Alternate ordered marker\n",
    "\n",
    "| Name | Value | Notes |\n",
    "| :--- | ---: | :---: |\n",
    "| alpha | 42 | **rich** |\n",
    "| beta | 3.14 | `code` |\n",
    "\n",
    "```rust\n",
    "fn calculate(value: usize) -> usize {\n",
    "    value.saturating_mul(2)\n",
    "}\n",
    "```\n",
    "\n",
    "```json\n",
    "{\"enabled\": true, \"items\": [1, 2, 3]}\n",
    "```\n",
    "\n",
    "Inline math $E = mc^2$ and escaped punctuation \\*literal\\*.\n",
    "Footnote usage[^perf] and an autolink <https://example.org>.\n",
    "[^perf]: Footnote text with `details`.\n",
    "[docs]: https://example.com/docs \"Documentation\"\n",
    "\n",
    "---\n",
    "Term\n",
    ": Definition-style content for parser stress.\n",
    "- List item after thematic break\n",
    "\n",
    "Plain text keeps the common path represented.\n",
    "Another plain line with CJK 文本 and emoji 🚀.\n",
    "\n",
);

fn complex_document() -> String {
    assert_eq!(
        COMPLEX_BLOCK.bytes().filter(|byte| *byte == b'\n').count(),
        40
    );
    let mut document = String::with_capacity(COMPLEX_BLOCK.len() * BLOCK_REPETITIONS);
    for _ in 0..BLOCK_REPETITIONS {
        document.push_str(COMPLEX_BLOCK);
    }
    assert_eq!(
        document.bytes().filter(|byte| *byte == b'\n').count(),
        DOCUMENT_LINES
    );
    document
}

fn rss_bytes(system: &mut System) -> u64 {
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[Pid::from_u32(std::process::id())]),
        ProcessRefreshKind::new().with_memory(),
    );
    system
        .process(Pid::from_u32(std::process::id()))
        .map(|process| process.memory())
        .unwrap_or(0)
}

fn elapsed_ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1_000.0
}

#[test]
#[ignore = "2M-line release benchmark; run explicitly with --ignored --nocapture"]
fn two_million_line_markdown_core_benchmark() {
    let mut system = System::new();
    let rss_start = rss_bytes(&mut system);

    let generate_started = Instant::now();
    let document = complex_document();
    let generate_ms = elapsed_ms(generate_started);
    let rss_document = rss_bytes(&mut system);

    let index_started = Instant::now();
    let mut buffer = Buffer::from_text(&document);
    let index_ms = elapsed_ms(index_started);
    let rss_buffer = rss_bytes(&mut system);
    assert_eq!(buffer.line_count(), DOCUMENT_LINES + 1);

    let parse_started = Instant::now();
    let decorations = crate::markdown::parse_decorations(buffer.text());
    let parse_ms = elapsed_ms(parse_started);
    let rss_decorations = rss_bytes(&mut system);
    assert_eq!(decorations.len(), buffer.line_count());
    black_box(&decorations);

    let middle = buffer.line_start(DOCUMENT_LINES / 2);
    let mut edit_samples = Vec::with_capacity(120);
    for _ in 0..120 {
        let insert_started = Instant::now();
        buffer.insert(middle, "x");
        let insert_elapsed = insert_started.elapsed();
        let delete_started = Instant::now();
        buffer.delete(middle..middle + 1);
        let delete_elapsed = delete_started.elapsed();
        edit_samples.push(insert_elapsed + delete_elapsed);
    }
    edit_samples.sort_unstable();
    let edit_p50 = edit_samples[edit_samples.len() / 2];
    let edit_p95 = edit_samples[edit_samples.len() * 95 / 100];
    let edit_max = *edit_samples.last().unwrap_or(&Duration::ZERO);

    println!(
        concat!(
            "2m-markdown bytes={} lines={} line_decoration_bytes={}\n",
            "generate_ms={:.2} buffer_index_ms={:.2} markdown_parse_ms={:.2}\n",
            "edit_pair_p50_ms={:.3} edit_pair_p95_ms={:.3} edit_pair_max_ms={:.3}\n",
            "rss_start_mb={:.1} rss_document_mb={:.1} rss_buffer_mb={:.1} rss_decorations_mb={:.1}"
        ),
        document.len(),
        buffer.line_count(),
        std::mem::size_of::<crate::markdown::LineDecoration>(),
        generate_ms,
        index_ms,
        parse_ms,
        edit_p50.as_secs_f64() * 1_000.0,
        edit_p95.as_secs_f64() * 1_000.0,
        edit_max.as_secs_f64() * 1_000.0,
        rss_start as f64 / 1_048_576.0,
        rss_document as f64 / 1_048_576.0,
        rss_buffer as f64 / 1_048_576.0,
        rss_decorations as f64 / 1_048_576.0,
    );
}

#[test]
#[ignore = "2M-line lazy viewport benchmark; run explicitly with --ignored --nocapture"]
fn two_million_line_lazy_markdown_benchmark() {
    let mut system = System::new();
    let document = complex_document();
    let buffer = Buffer::from_text(&document);
    let rss_buffer = rss_bytes(&mut system);

    let initialize_started = Instant::now();
    let (mut decorations, mut lazy) =
        crate::editor::new_lazy_markdown_decorations(buffer.line_count());
    let initialize_ms = elapsed_ms(initialize_started);
    let rss_slots = rss_bytes(&mut system);

    let first_view_started = Instant::now();
    crate::editor::parse_lazy_markdown_range(&buffer, &mut decorations, &mut lazy, 0..256);
    let first_view_ms = elapsed_ms(first_view_started);
    let rss_first_view = rss_bytes(&mut system);

    let mut samples = Vec::with_capacity(100);
    for sample in 0..100 {
        let line = (sample * 19_919) % (DOCUMENT_LINES - 256);
        let started = Instant::now();
        crate::editor::parse_lazy_markdown_range(
            &buffer,
            &mut decorations,
            &mut lazy,
            line..line + 256,
        );
        samples.push(started.elapsed());
    }
    samples.sort_unstable();
    let p50 = samples[samples.len() / 2];
    let p95 = samples[samples.len() * 95 / 100];
    let p99 = samples[samples.len() * 99 / 100];
    let rss_after_scroll = rss_bytes(&mut system);

    let outline_started = Instant::now();
    let mut outline = crate::editor::MarkdownOutlineIndex::rebuild(&buffer);
    let outline_rebuild_ms = elapsed_ms(outline_started);
    let outline_update_started = Instant::now();
    outline.update_after_edit(&buffer, 1_000_000, 1_000_128, 1_000_128);
    let outline_update_ms = elapsed_ms(outline_update_started);

    println!(
        concat!(
            "2m-lazy lines={} initialize_ms={:.2} first_view_ms={:.2}\n",
            "scroll_chunk_p50_ms={:.3} p95_ms={:.3} p99_ms={:.3}\n",
            "outline_headings={} outline_rebuild_ms={:.2} outline_update_ms={:.3}\n",
            "rss_buffer_mb={:.1} rss_slots_mb={:.1} rss_first_view_mb={:.1} rss_after_scroll_mb={:.1}"
        ),
        buffer.line_count(),
        initialize_ms,
        first_view_ms,
        p50.as_secs_f64() * 1_000.0,
        p95.as_secs_f64() * 1_000.0,
        p99.as_secs_f64() * 1_000.0,
        outline.headings.len(),
        outline_rebuild_ms,
        outline_update_ms,
        rss_buffer as f64 / 1_048_576.0,
        rss_slots as f64 / 1_048_576.0,
        rss_first_view as f64 / 1_048_576.0,
        rss_after_scroll as f64 / 1_048_576.0,
    );
}

#[test]
#[ignore = "2M-line edit-layer benchmark; run explicitly with --ignored --nocapture"]
fn two_million_line_edit_layers_benchmark() {
    let document = complex_document();
    let positions = [0, DOCUMENT_LINES / 2, DOCUMENT_LINES - 1];

    for line in positions {
        let snapshot_buffer = Buffer::from_text(&document);

        let snapshot_started = Instant::now();
        let snapshot = snapshot_buffer.text().to_string();
        let snapshot_ms = elapsed_ms(snapshot_started);
        black_box(snapshot);

        let mut character_buffer = Buffer::from_text(&document);
        let character_offset = character_buffer.line_start(line);
        let character_started = Instant::now();
        character_buffer.insert(character_offset, "x");
        let character_insert_ms = elapsed_ms(character_started);

        let mut newline_buffer = Buffer::from_text(&document);
        let newline_offset = newline_buffer.line_start(line);
        let newline_started = Instant::now();
        newline_buffer.insert(newline_offset, "\n");
        let newline_insert_ms = elapsed_ms(newline_started);

        let reserved_text = {
            let mut text = document.clone();
            text.reserve(document.len() / 16);
            text
        };
        let mut reserved_buffer = Buffer::from_string(reserved_text);
        let reserved_offset = reserved_buffer.line_start(line);
        let reserved_character_started = Instant::now();
        reserved_buffer.insert(reserved_offset, "x");
        let reserved_character_ms = elapsed_ms(reserved_character_started);
        reserved_buffer.delete(reserved_offset..reserved_offset + 1);
        let reserved_newline_started = Instant::now();
        reserved_buffer.insert(reserved_offset, "\n");
        let reserved_newline_ms = elapsed_ms(reserved_newline_started);

        println!(
            "2m-edit-layer line={line} snapshot_ms={snapshot_ms:.3} character_insert_ms={character_insert_ms:.3} newline_insert_ms={newline_insert_ms:.3} reserved_character_ms={reserved_character_ms:.3} reserved_newline_ms={reserved_newline_ms:.3}",
        );
    }
}

#[test]
fn lazy_markdown_matches_full_parse_around_500_lines() {
    let text = COMPLEX_BLOCK.repeat(3000);
    let buffer = Buffer::from_string(text.clone());
    let full = crate::markdown::parse_decorations(&text);
    let (mut lazy_decorations, mut lazy) =
        crate::editor::new_lazy_markdown_decorations(buffer.line_count());
    for start in [480, 920, 9_920, 99_920, 119_700] {
        let end = (start + 220).min(buffer.line_count());
        crate::editor::parse_lazy_markdown_range(
            &buffer,
            &mut lazy_decorations,
            &mut lazy,
            start..end,
        );

        for line in start..end {
            assert_eq!(
                format!("{:?}", lazy_decorations[line]),
                format!("{:?}", full[line]),
                "lazy decoration mismatch at line {line}: {:?}",
                buffer.line_content(line),
            );
        }
    }
}
