# Worker 4 visual coverage

Baseline build: git HEAD, isolated Request Eagle (Theme audit 4) native macOS application, 1225 x 768 screenshots. Synthetic local HTTP fixtures only.

## Baseline coverage

All four variants were selected through Appearance UI: Molokai Light, Molokai Dark, Solarized Light, Solarized Dark.

For each variant I opened and visually inspected:

- Settings General, Appearance, Keybindings, and inline shortcut recorder with a duplicate Cmd+T conflict.
- POST request Params, Headers, and populated JSON Body.
- Populated JSON response Body, Raw format, response Headers and Cookies.
- HTML response body and connection-error response.
- HTTP method menu, selected sidebar row, request context menu.
- Response search with matching text, timing popover, size popover.
- Collection inline rename and delete confirmation, canceled without changes.

Screenshots for each state are in before/, including <theme-slug>-editors.jpg for clean populated POST/JSON editors. The Molokai Light editor screenshot predates opening HTML/error tabs; the remaining editor screenshots include those additional inactive tabs.

## Findings

- Molokai Dark: muted text is nearly unreadable in settings descriptions, sidebar navigation, field placeholders, inactive tabs, table labels, metadata labels, and shortcut keycaps. Muted foreground contrast on background is 2.45:1. Blue/cyan POST/PUT semantics and request-body JSON badge are too dark. Delete Yes and error text are weak. Editor gutters and active lines are black while editor canvas stays app charcoal.
- Molokai Light: JSON green strings and orange numeric/boolean tokens are weak; string contrast is 3.32:1 and numeric contrast 3.19:1. Method colors and 200 OK green are faint. Shortcut keycaps and muted helper text could be stronger. Send remains unrelated blue, while the footer is unrelated salmon.
- Solarized Light: pale grey descriptions, field placeholders, shortcut keycaps, line numbers, and metadata are faint; muted contrast is 2.48:1. JSON string contrast is 2.97:1. Send is unrelated blue and footer mustard.
- Solarized Dark: method colors, request JSON badge, 200 OK badge, connection error, shortcut conflict, delete Yes, and selected Dark mode label are too dark. Base semantic contrasts range from 1.44:1 to 2.99:1. General body text and editor syntax remain legible.

## Selection limitation

I attempted editor selection using Cmd+A, Shift+End, native drag, and AX selectText. The native editor does not expose a settable selected-text range, and background-window screenshots only show caret/active-line movement without selection fill. solarized-dark-selection.jpg records that observed state; it does not prove selected-text contrast. Search results were exercised in all four variants, but their highlights are also extremely subtle in the baseline background window.

## After coverage

Completed the same full state list in the corrected Request Eagle (Theme audit 4 after) application for all four variants. Matching screenshots are in after/. The Send button and footer use theme colors; descriptions, keycaps, method/status labels, errors, and delete confirmation labels are now readable. All four editor screenshots include populated request and response JSON.

## Final targeted verification

The final binary was inspected in Request Eagle (Theme audit 4 final), bundle com.egortokarev.requesteagle.theme-audit-4-final.

- Rechecked and replaced editor, selection, search, and HTML screenshots for all four variants. Added Raw and combined Find-plus-manual-selection screenshots for all four variants.
- Solarized Light General, Appearance, and Keybindings were rechecked after the final foreground adjustment. Text remains readable.
- Molokai Dark editor background now matches the charcoal application, and its active line is distinct. Also refreshed Params, timing, size, context, rename, and delete screenshots so none of those retains the intermediate black canvas.
- Solarized Dark now has visibly distinct cyan selection and search highlights. JSON remains readable when search, manual selection, and active-line backgrounds overlap.
- Both Molokai variants and Solarized Light remain readable under the same overlapping highlights.

No remaining contrast or color-coherence issue was found in the assigned variants after the final targeted verification. All screenshots are original native captures with no image editing or reencoding. Settings and request contents were changed only in isolated synthetic audit profiles; inline deletion prompts were always canceled.
