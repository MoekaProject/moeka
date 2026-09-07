# macOS GPU diagnostics

Use a release build on the affected Mac. The runtime profile records CPU and memory samples together with cumulative `mouse_move_events` and actual `render_frames` counters.

```sh
./scripts/run_gpu_diagnostic.sh /Applications/Moeka.app
```

Test one short Markdown document in this order:

1. Keep the pointer outside the window for 30 seconds.
2. Keep the pointer still over the editor for 30 seconds.
3. Move the pointer continuously over plain text for 30 seconds without clicking or scrolling.
4. Move it repeatedly across code-block controls and the right scrollbar for 30 seconds.
5. Quit Moeka normally so the report is written.

Repeat once with a document containing a table, fenced code block, image, and generated preview.

Interpretation:

- `mouse_move_events` rises while `render_frames` stays nearly flat: the event path is not continuously repainting; inspect Metal/GPUI command submission and the Intel driver trace.
- Both counters rise together over plain text: an application or GPUI hover listener is invalidating every mouse event.
- Frames rise only while crossing controls: bounded hover repainting is expected, but animation tasks should settle after movement stops.
- GPU use continues rising after both counters stop: capture an Instruments Metal System Trace; the editor event handlers are not the continuing source.

Record the Mac model, macOS version, display resolution/scaling, refresh rate, Moeka version, and Activity Monitor GPU history with the JSON timeline and CPU folded stacks.

## CPU stack output and optional visualization

On macOS and Linux, runtime profiling still samples CPU stacks with `pprof` (99 Hz by default, configurable with `MOEKA_PROFILE_CPU_HZ`). Snapshots and stopping a profile write cumulative sampled stacks to `cpu-stacks-<reason>-<timestamp>.folded` alongside the unchanged JSON timeline. CPU sampling can be disabled with `MOEKA_PROFILE_CPU=0`; when disabled or unavailable, only the JSON timeline is written. Windows still records the resource timeline and diagnostics, but CPU stack sampling is unsupported.

Each folded line is `thread;root;caller;leaf count`, including inline symbols in root-to-leaf order. Counts are sample counts, not elapsed time. Identical displayed stacks are summed and lines are sorted; threads with the same name are grouped, while unnamed threads use their numeric ID. An empty report produces an empty folded file. Empty symbol names use `[unknown]`. Semicolons, percent signs, whitespace, and control characters in labels are percent-encoded as UTF-8 bytes (for example, `;` becomes `%3B` and a newline becomes `%0A`); other Unicode and punctuation are retained. External renderers display these escapes literally—do not decode them before parsing folded data.

Moeka no longer generates SVG flamegraphs in-process and does not enable `pprof`'s `flamegraph` feature or its optional `inferno` dependency. In particular, the `inferno` 0.11 dependency previously used for rendering is CDDL-1.0 licensed and is excluded from this profiling dependency path for GPL combined-distribution readiness. CPU sampling and symbol resolution do not require that renderer. This scoped change does not certify the licenses of the entire application dependency graph.

Visualization is optional: a separately installed folded-stack renderer, such as the FlameGraph project's `flamegraph.pl`, can read the folded file and write an SVG via standard output. Install and review the license of any chosen renderer independently, then pass the folded file as its input and save its output as an SVG. No renderer is bundled, downloaded, installed, or invoked automatically by Moeka. The folded file remains useful as plain text without any external tool.
