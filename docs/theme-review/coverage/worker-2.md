# Worker 2 visual theme audit

Native app: `Request Eagle (Theme audit 2)`, bundle `com.egortokarev.requesteagle.theme-audit-2`. Isolated preferences, keybindings and fixture collection. All UI interactions and screenshots used CUA. The local fixture server returned JSON, HTML, response headers and a cookie, plus a connection-error response.

## Baseline coverage

Everforest Light, Everforest Dark, Flexoki Light, Flexoki Dark, Gruvbox Light and Gruvbox Dark were each selected through Appearance and visually inspected in both modes.

Each variant has 21 actual native screenshots and corresponding AX records in `before/`:

- Settings Appearance, General, Keybindings and shortcut recorder.
- Populated POST request JSON body with JSON response, request Params and request Headers including generated headers.
- Response Cookies, Headers and Raw/JSON selector.
- Response search results and actual dragged text selection.
- Status, timing and size detail popovers.
- Method menu, collection selection/context menu, inline rename and inline delete confirmation. Rename and delete were cancelled.
- HTML response syntax and connection-error text.

The initial capture helper held its first filename prefix. Files affected by that naming defect were recovered or recaptured; final filesystem verification confirmed 21 unique screenshots per variant, 126 total.

## Baseline findings

| Variant | Actual visual findings |
| --- | --- |
| Everforest Light | Pastel JSON/HTML syntax, method/status text, selected Light label, helper text, keycaps, search highlights and error text are washed out. |
| Everforest Dark | Main syntax and body text are usable. Muted descriptions, keycaps, placeholders, inactive tabs and metadata are too dim. Timing popover labels are particularly weak. |
| Flexoki Light | Ordinary text, tables and descriptions read well. Active Light mode text, green status/method text and mustard POST text are pale. Pink JSON null and HTML doctype need darker colors. |
| Flexoki Dark | Orange properties/booleans, teal strings and purple constants/numbers are too dim, especially with selection. Method/status and error colors are weak. Selected Dark label and delete-confirmation Yes are weak. |
| Gruvbox Light | Active Light label, yellow JSON null/HTML doctype, green GET/status text and muted descriptions/keycaps are pale. |
| Gruvbox Dark | Ordinary body text and most syntax are usable. Olive GET/status text and red DELETE/error text are dim. Muted helper text/keycaps are weak. Selection makes green strings harder to read. |

All variants retain the baseline shared blue Send button. Light variants show a bright yellow bottom status bar. These were reported to the parent for shared-token review.

## Corrected build

The corrected app `Request Eagle (Theme audit 2 after)` used bundle `com.egortokarev.requesteagle.theme-audit-2-after`. Every screen listed above was repeated and visually reviewed for all six variants. The `after/` directory contains 21 native screenshots per variant, 126 total, with matching theme and screen names in `before/`. Each image is 1225 by 768 pixels.

The revised palettes improve the reported syntax, semantic and muted-text contrast. Active mode buttons use filled theme colors; Send follows each theme; the footer follows the theme rather than showing the old yellow strip. Selection keeps code readable. Inline rename and delete confirmation remain readable in every variant, with a clearer Yes label, and were cancelled. No further color changes were identified in this worker's corrected screen review. The final app `Request Eagle (Theme audit 2 final)` (`com.egortokarev.requesteagle.theme-audit-2-final`) was then used to repeat clean JSON editors, Raw response, HTML response, search, and combined Find plus Select All for all six variants. Visible search matches and text selections remained readable. Those 30 screenshots replaced the corresponding after captures. Everforest Light and Dark General, Appearance, and Keybindings were also refreshed after their foreground adjustment. No remaining color issue was identified in these final checks. Final native screenshot bytes were saved directly without reencoding.
