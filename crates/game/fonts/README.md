# Fonts for public play

The two unmodified TrueType fonts in this directory come from the
[Liberation Fonts 2.1.5 release](https://github.com/liberationfonts/liberation-fonts/releases/tag/2.1.5),
specifically its
[TTF archive](https://github.com/liberationfonts/liberation-fonts/files/7261482/liberation-fonts-ttf-2.1.5.tar.gz).
They are distributed under the **SIL Open Font License 1.1**, with the upstream
copyright notice and complete license in [LICENSE](LICENSE). Include that file
when distributing these fonts or builds containing them.

SHA-256 of the upstream files (also checked by the publication guard):

| File | SHA-256 |
|---|---|
| `LiberationSans-Regular.ttf` | `76d04c18ea243f426b7de1f3ad208e927008f961dc5945e5aad352d0dfde8ee8` |
| `LiberationSans-Bold.ttf` | `788abee4c806d660e8aee46689dd8540cd4bb98da03dcc9d171ce3efd99a9173` |

`TextMasks::bundled()` embeds both files with `include_bytes!` and rasterizes
their glyphs using fontdue, whose version is pinned in `Cargo.lock`. No font
installation, downloads, Java, game strings, or private captures are needed at
runtime. The four MIDP roles use bold 14 px, bold 10 px, regular 10 px, and regular
12 px. The renderer preserves the shell's nominal line heights and baseline,
rounds each character advance to an integer, disables kerning, and blends glyph
coverage into the RGB framebuffer. Drawing and wrapping share those advances.
Unsupported characters display `?` with the same advance used for layout.

These fonts provide a portable public renderer. They do not reproduce the
reference emulator's fonts pixel for pixel; text widths, wrapping, and appearance
can differ. The original game archive inspected during development uses English
text; broader localization support is outside this font change.

Private comparison tests continue to call `TextMasks::load` explicitly. The
playable tools always use the bundled fonts, even if a local reference-mask file
is present. Do not copy captured text masks or game text into this directory.
