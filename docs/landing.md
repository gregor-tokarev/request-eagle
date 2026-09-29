# Landing page

The site at <https://requesteagle.tokarev.work> is the static page in
`landing/`. It has no build step: `index.html`, `styles.css`, `script.js` and
`assets/`.

## Preview

```sh
python3 -m http.server 4173 --directory landing
```

## Deploy

The page is hosted on Cloudflare Pages as the `request-eagle` project.

```sh
wrangler pages deploy landing --project-name request-eagle --branch main
```

## Motion

`script.js` drives the scroll-linked and pointer-driven motion; CSS handles the
rest. The page stays complete without scripts, and `prefers-reduced-motion`
turns every animation off. Pointer effects only attach on devices that hover.

## Screenshots

`assets/shots/` holds captures of the real app (v0.1.15) running a demo
"Flight API" collection against a local mock server. They come from the Linux
build, so the page draws the macOS window controls over the title bar in CSS
(`.window__lights`). Replace them with macOS captures when available. Shortcuts
in the copy use the macOS modifier (⌘).

## Claims to keep current

- Eleven theme families, and the seven themes offered by the picker.
- Interface sizes from 12 to 24 px.
- The macOS build targets Apple Silicon; Linux builds from source.
