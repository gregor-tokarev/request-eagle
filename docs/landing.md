# Landing page

The site at <https://requesteagle.tokarev.work> is the [Astro](https://astro.build)
project in `landing/`. It builds to static files in `landing/dist/`:

- `src/pages/` holds the landing, the downloads page and the 404 page.
- `src/layouts/Page.astro` is the shared head, navigation and footer.
- `src/styles/base.css` styles what the pages share. `landing.css` builds on
  it for the landing's own sections.
- `src/scripts/` holds one script per page.
- `src/assets/` holds the captures, icon and fonts that the build optimises.
- `public/` is copied as it is.

## Preview

Needs Node.js 22.12 or newer.

```sh
cd landing
npm install
npm run dev      # live preview while editing
npm run build    # writes dist/
npm run preview  # serves dist/
```

## Deploy

The page is hosted on Cloudflare Pages as the `requesteagle` project, with
`requesteagle.tokarev.work` as its custom domain. The project is connected to
this repository with these build settings:

| Setting | Value |
| --- | --- |
| Root directory | `landing` |
| Build command | `npm run build` |
| Build output directory | `dist` |

`public/_headers` lets browsers keep the fingerprinted files under `/_astro/`
for a year. Without the 404 page, Cloudflare would answer every unknown address
with the landing.

Cloudflare deploys by itself when a push to `main` changes a file under
`landing/`. Other branches and pull requests are not deployed.

To deploy `main` again without a change, with the [Cloudflare CLI](https://www.npmjs.com/package/cf):

```sh
cf pages projects deployments create requesteagle --branch main
```

## Downloads

`src/pages/downloads.astro` is the page at `/downloads/`, and the landing's
download buttons lead there. It offers the macOS disk image and ZIP archive,
the Windows installer and the Debian package, from either
[update track](releases.md): stable by default, or nightly after choosing it.
`/downloads/#nightly` opens the page on the nightly track.

Its script, `src/scripts/downloads.js`, reads the releases from the GitHub API
in the visitor's browser, so the buttons follow each new or promoted release
without an edit or a deploy. Each button takes the newest file for its
platform on the chosen track. The page shows no version number. The CLI
executables are not offered, because the CLI is installed from the app.

Without scripts, or when GitHub cannot be reached, the buttons lead to the
latest release on GitHub instead. A platform with no file on the chosen track
leads to the list of releases.

## Motion

`src/scripts/landing.js` drives the scroll-linked and pointer-driven motion;
CSS handles the rest. The page stays complete without scripts, and `prefers-reduced-motion`
turns every animation off. Pointer effects only attach on devices that hover.

## Fonts

Archivo and Geist Mono are self-hosted from `src/assets/fonts/` under the SIL
Open Font License. Their licence texts are in `public/assets/fonts/` and are
published with the page, as the licence requires.

The files are subsets, under a third of the size of the full fonts, made with
[fontTools](https://fonttools.readthedocs.io):

- Characters: printable ASCII, plus the no-break space, `©`, `·`, `×`, dashes,
  curly quotes, the bullet and the ellipsis. A character outside this set is
  drawn in the fallback font, so widen the subset before using one.
- Archivo: weights 400 to 800 at widths 100% to 112%.
- Archivo Italic: weights 700 to 900 at widths 112% to 125%.
- Geist Mono: weights 400 to 500.

The `@font-face` rules in `base.css` declare the same ranges. Text styled
outside them is drawn at the nearest weight or width the file has.

## Screenshots

`src/assets/shots/` holds captures of the real app (v0.1.15) running a demo
"Flight API" collection against a local mock server. They come from the Linux
build, so the page draws the macOS window controls over the title bar in CSS
(`.window__lights`). Replace them with macOS captures when available. Shortcuts
in the copy use the macOS modifier (⌘).

The build resizes each capture to several widths, as AVIF with a WebP
fallback, and the page picks one by the `sizes` set in `index.astro`. Keep
those in step with the layout when a capture's column changes width. The theme
picker swaps its captures from the script, so they are WebP only.

## Loading speed

The landing loads about 120 KB before it is usable, and both pages score 100
in every Lighthouse category on mobile and desktop. What keeps it there:

- The stylesheet is inlined, so nothing blocks the first paint.
- Only the two Archivo files are preloaded. Everything below the first screen
  loads lazily.
- No image is larger than it is drawn.

Check a change with Lighthouse against a build served with compression:

```sh
npm run build
npx serve dist
npx lighthouse http://localhost:3000/
```

## Claims to keep current

- The startup figures in the race: 0.22 s for Request Eagle, 3.86 s for
  Postman, and "18 times". They come from [the startup benchmark](startup-benchmark.md).
- Eleven theme families, and the seven themes offered by the picker.
- Interface sizes from 12 to 24 px.
- The macOS build targets Apple Silicon; Windows and Debian builds target x64.
