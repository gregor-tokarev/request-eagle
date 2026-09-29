# Landing page

The site at <https://requesteagle.tokarev.work> is the static page in
`landing/`. It has no build step: `index.html`, `styles.css`, `script.js` and
`assets/`.

## Preview

```sh
python3 -m http.server 4173 --directory landing
```

## Deploy

The page is hosted on Cloudflare Pages as the `requesteagle` project, with
`requesteagle.tokarev.work` as its custom domain. The project is connected to
this repository and serves `landing/` as it is, without a build command.

Cloudflare deploys by itself when a push to `main` changes a file under
`landing/`. Other branches and pull requests are not deployed.

To deploy `main` again without a change, with the [Cloudflare CLI](https://www.npmjs.com/package/cf):

```sh
cf pages projects deployments create requesteagle --branch main
```

## Motion

`script.js` drives the scroll-linked and pointer-driven motion; CSS handles the
rest. The page stays complete without scripts, and `prefers-reduced-motion`
turns every animation off. Pointer effects only attach on devices that hover.

## Fonts

Archivo and Geist Mono are self-hosted in `assets/fonts/` under the SIL Open
Font License. Their licence texts sit beside them and are published with the
page, as the licence requires.

## Screenshots

`assets/shots/` holds captures of the real app (v0.1.15) running a demo
"Flight API" collection against a local mock server. They come from the Linux
build, so the page draws the macOS window controls over the title bar in CSS
(`.window__lights`). Replace them with macOS captures when available. Shortcuts
in the copy use the macOS modifier (⌘).

## Claims to keep current

- The startup figures in the race: 0.22 s for Request Eagle, 3.86 s for
  Postman, and "18 times". They come from [the startup benchmark](startup-benchmark.md).
- Eleven theme families, and the seven themes offered by the picker.
- Interface sizes from 12 to 24 px.
- The macOS build targets Apple Silicon; Linux builds from source.
