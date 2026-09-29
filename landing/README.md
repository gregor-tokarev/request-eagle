# Landing page variants

Four static candidates for the Request Eagle site. Each variant is one
self-contained `index.html`; they share only `assets/`.

| Variant | Directory | Direction |
| --- | --- | --- |
| A | `flight-deck/` | Dark, product-led, feature grid |
| B | `plain-text/` | Light, editorial, "requests are just files" |
| C | `keyboard/` | Monospace man page for terminal and Vim users |
| D | `velocity/` | Bold, performance-led |

`index.html` is a chooser with live previews of all four.

## Preview

```sh
python3 -m http.server 4173 --directory landing
```

## Choosing one

Move the chosen variant's `index.html` to `landing/index.html`, change its
`../assets/` references to `assets/`, and delete the other variant directories.

## Screenshots

`assets/shots/` holds captures of the real app (v0.1.15) running a demo
"Flight API" collection against a local mock server. They come from the Linux
build, so the pages draw the macOS window controls over the title bar in CSS
(`.window__lights`). Replace them with macOS captures before launch. Shortcuts
in the copy use the macOS modifier (⌘).

## Claims to keep current

- Version `v0.1.15` and the 19 MB download size.
- Eleven theme families.
- Benchmark figures in `velocity/` (8.33 ms budget, 10,000 tabs, 10 MiB
  responses) come from the benchmarks described in the repository README.
