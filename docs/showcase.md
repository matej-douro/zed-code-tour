# Showcase

A 30 second recording of the extension in Zed 1.26.2, made on a copy of this
repository. The tour file is [showcase.tour.md](../showcase.tour.md). It walks
through the extension entry point, the language server binary, the parser, the
body hint, and the step control row.

[![zed-code-tour showcase](media/zed-code-tour-showcase-poster.png)](media/zed-code-tour-showcase.mp4)

The clip has no audio. Click the poster, or open
[media/zed-code-tour-showcase.mp4](media/zed-code-tour-showcase.mp4) directly.

The recording uses the headless setup from [docs/e2e.md](e2e.md): a real Zed
window under Xvfb, the built extension and `tour-lsp`, and real Start, Next,
Overview, and Stop lens clicks. The frames were captured with ffmpeg and cut
together with ImageMagick and ffmpeg.
