# Worker 1 native visual audit

Audited real native app `Request Eagle (Theme audit 1)` with an isolated fixture collection and preferences. Baseline binary built from git HEAD by root. All screenshots are unaltered native captures at 1225 x 768. User-facing controls were opened through native UI.

## Baseline coverage

Every variant below has 21 captured states under `before/` using its slug:

- Default Light (`default-light`)
- Default Dark (`default-dark`)
- Ayu Light (`ayu-light`)
- Ayu Dark (`ayu-dark`)
- Catppuccin Latte (`catppuccin-latte`)
- Catppuccin Mocha (`catppuccin-mocha`)

States inspected for each variant: Settings General, Appearance, Keybindings, shortcut recorder; populated POST JSON request and JSON response editors; request Params and Headers; response Cookies and Headers; method menu; response Raw; response search matches; status, timing and size popovers; collection request context menu; HTML response syntax; connection error; selected JSON request text; inline request rename; inline delete confirmation. Rename was cancelled, deletion was explicitly cancelled with No, recorder was cancelled without saving.

The `editors.jpg` screenshots contain clean populated POST request JSON plus JSON response, with no menu or text selection.

Default Light also has three folder-level captures: context menu, inline rename and inline delete confirmation. Folder deletion was cancelled with No. The other variants use the shared request-row rename/delete renderer.

## Baseline findings

- Default Light: GET, POST and PUT text in the sidebar and method menu are pale. Green 200 OK text on its tinted badge is faint. Body syntax is readable. Keycaps and muted captions could be darker.
- Default Dark: Main copy, methods, selected request, menus, editor syntax and errors are readable. Yellow JSON literals are comparatively dim but visible. No broad palette redesign is warranted.
- Ayu Light: Muted helper copy, inactive navigation, placeholder labels and keycaps are too faint. Cyan primary labels, JSON object keys, green strings and purple literals lack contrast. Method colors and status badge are pale. Timing popup labels are weak.
- Ayu Dark: Muted helper copy, inactive navigation, keycaps, recorder placeholder, editor line numbers and response metadata nearly disappear into dark backgrounds. PUT method text is especially weak. Main syntax is otherwise readable.
- Catppuccin Latte: Green JSON strings are extremely faint on the lavender canvas, with weak orange literals. PUT and POST method text and the status badge need darker colors. Muted captions/keycaps and selected Light mode label are faint.
- Catppuccin Mocha: Editor syntax and methods are readable. Muted helper copy, keycaps, input placeholders and timing-popup labels need stronger contrast. No need to replace the family colors.
- All palettes: Send remains the same saturated blue across families in the baseline, and the light workspace bottom bar is a broad gray band. These shared token issues are assigned to root.
- Text selection remains visible in all six baseline variants, but Ayu Light and Catppuccin Latte selected syntax inherits their poor text contrast. Inline rename text is readable. Inline delete Yes uses a faint red tinted treatment, particularly in light modes.

## After coverage

Repeated all 21 states for all six variants in the isolated `Request Eagle (Theme audit 1 after)` application. Each matching after capture is under `after/`; every image was visually inspected. The folder-only Default Light baseline extras were not repeated because the request-row controls share their renderer.

Muted helper copy, navigation, keycaps, placeholders, response details and editor gutters now remain readable. Light method/status colors are darker. Ayu Light keys and strings and Catppuccin Latte strings/literals are much clearer. Family backgrounds and syntax character remain intact. Send follows each theme's primary color, and the light bottom bar blends with the application chrome. Request/response tabs, HTML syntax, connection errors, menus, rename and cancelled delete prompts remain readable.

The first corrected build introduced an editor-selection regression in Default Light and Default Dark: selected text was barely distinguishable from the editor canvas. This was reported to root and corrected in the final build. Ayu and Catppuccin selection is subtle but visible.

## Final-build follow-up

In `Request Eagle (Theme audit 1 final)`, repeated and replaced `editors`, `selection`, `response-raw`, `search`, and `html` after captures for Default Light, Default Dark, Ayu Light, Ayu Dark and Catppuccin Latte. Added `search-selection` captures for these five variants. Repeated Ayu Light and Ayu Dark Appearance, General and Keybindings after their final foreground adjustment. Catppuccin Mocha was unchanged in this follow-up and retains the complete prior corrected-build pass.

Default Light and Default Dark now have clear blue text selection with readable syntax. All five final editor variants remain readable with search highlighting and selection together on the active line. The overlap capture selects line 6 containing the active `theme` search match with Command-Left followed by Command-Shift-Right, so active-line background, search match and selection are exercised simultaneously. Ayu Light/Dark final settings and HTML syntax remain readable. No additional visual defect was found in the final targeted pass.

An extra baseline Default Light `search-selection` screenshot records the same overlapping active-line state. The evidence set totals 130 baseline JPEG captures and 131 after JPEG captures. Matching filenames represent matching screens; the five additional after overlap captures include four without baseline counterparts.
