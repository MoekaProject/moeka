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

Record the Mac model, macOS version, display resolution/scaling, refresh rate, Moeka version, and Activity Monitor GPU history with the JSON timeline and flamegraph.
