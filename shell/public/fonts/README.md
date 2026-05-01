# Self-hosted fonts (SH-010)

Place the following woff2 files here. They are not committed to the
repository because of size; the CI/CD pipeline (or a manual `pnpm fonts:download` 
script) should fetch them before building.

## Required files

### Inter (interface font)
- Inter-Regular.woff2    (weight 400)
- Inter-Medium.woff2     (weight 500)
- Inter-SemiBold.woff2   (weight 600)
- Inter-Bold.woff2       (weight 700)

Source: https://fonts.google.com/specimen/Inter (OFL-1.1)

### IBM Plex Serif (prose/text font)
- IBMPlexSerif-Regular.woff2         (weight 400)
- IBMPlexSerif-Italic.woff2          (weight 400, italic)
- IBMPlexSerif-Medium.woff2          (weight 500)
- IBMPlexSerif-SemiBold.woff2        (weight 600)

Source: https://fonts.google.com/specimen/IBM+Plex+Serif (OFL-1.1)

### JetBrains Mono (monospace/code font)
- JetBrainsMono-Regular.woff2  (weight 400)
- JetBrainsMono-Medium.woff2   (weight 500)

Source: https://www.jetbrains.com/lp/mono/ (OFL-1.1)

## Download script (one-time setup)
The files can be obtained from the Google Fonts API or the project
GitHub releases. A helper script lives at `scripts/download_fonts.sh`.
