# Theme review

These native screenshots show the request and response editors for all 11 bundled theme families in both light and dark mode. Four audit workers used separate application instances. Each before/after pair uses the same theme, and every capture is 1225 by 768 pixels. No screenshots were cropped or recolored.

Some captures were losslessly converted from JPEG to PNG during the audit. This report preserves the available capture files without further conversion. Their extensions identify the actual JPEG or PNG format.

The baseline source is `c1d3ea1`; the final fixes are at `0fa1770`. Screens affected by the final changes were recaptured. Unchanged screens retain their corrected-build captures.

The audit covered settings, keybinding recording, request and response tabs, JSON and HTML editors, search and text selection, method menus, response details, collection actions, and error states. Corrections strengthen muted text, method/status/error labels, and syntax colors. Shared controls now use the selected theme; editor selection and backgrounds are consistent with it. Validation completed with 144 tests passing and five benchmarks ignored.

Coverage records describe the inspected screens, findings, and limitations: [worker 1](coverage/worker-1.md), [worker 2](coverage/worker-2.md), [worker 3](coverage/worker-3.md), [worker 4](coverage/worker-4.md).

The [self-contained HTML comparison](comparison.html) includes all 22 editor pairs with filters and full-size viewing. Download it or open it in an HTML preview; its screenshots and coverage records are embedded. The full local audit at `target/theme-audit/all-screens.html` also includes the other captured screens.

| Theme | Before | After |
| --- | --- | --- |
| Default Light | [Before JPEG](before/default-light-editors.jpg) | [After JPEG](after/default-light-editors.jpg) |
| Default Dark | [Before JPEG](before/default-dark-editors.jpg) | [After JPEG](after/default-dark-editors.jpg) |
| Ayu Light | [Before JPEG](before/ayu-light-editors.jpg) | [After JPEG](after/ayu-light-editors.jpg) |
| Ayu Dark | [Before JPEG](before/ayu-dark-editors.jpg) | [After JPEG](after/ayu-dark-editors.jpg) |
| Catppuccin Latte | [Before JPEG](before/catppuccin-latte-editors.jpg) | [After JPEG](after/catppuccin-latte-editors.jpg) |
| Catppuccin Mocha | [Before JPEG](before/catppuccin-mocha-editors.jpg) | [After JPEG](after/catppuccin-mocha-editors.jpg) |
| Everforest Light | [Before PNG](before/everforest-light-editors.png) | [After JPEG](after/everforest-light-editors.jpg) |
| Everforest Dark | [Before PNG](before/everforest-dark-editors.png) | [After JPEG](after/everforest-dark-editors.jpg) |
| Flexoki Light | [Before PNG](before/flexoki-light-editors.png) | [After JPEG](after/flexoki-light-editors.jpg) |
| Flexoki Dark | [Before PNG](before/flexoki-dark-editors.png) | [After JPEG](after/flexoki-dark-editors.jpg) |
| Gruvbox Light | [Before PNG](before/gruvbox-light-editors.png) | [After JPEG](after/gruvbox-light-editors.jpg) |
| Gruvbox Dark | [Before PNG](before/gruvbox-dark-editors.png) | [After JPEG](after/gruvbox-dark-editors.jpg) |
| Hybrid Light | [Before JPEG](before/hybrid-light-editors.jpg) | [After JPEG](after/hybrid-light-editors.jpg) |
| Hybrid Dark | [Before JPEG](before/hybrid-dark-editors.jpg) | [After JPEG](after/hybrid-dark-editors.jpg) |
| macOS Classic Light | [Before JPEG](before/macos-classic-light-editors.jpg) | [After JPEG](after/macos-classic-light-editors.jpg) |
| macOS Classic Dark | [Before JPEG](before/macos-classic-dark-editors.jpg) | [After JPEG](after/macos-classic-dark-editors.jpg) |
| Mellifluous Light | [Before JPEG](before/mellifluous-light-editors.jpg) | [After JPEG](after/mellifluous-light-editors.jpg) |
| Mellifluous Dark | [Before JPEG](before/mellifluous-dark-editors.jpg) | [After JPEG](after/mellifluous-dark-editors.jpg) |
| Molokai Light | [Before JPEG](before/molokai-light-editors.jpg) | [After JPEG](after/molokai-light-editors.jpg) |
| Molokai Dark | [Before JPEG](before/molokai-dark-editors.jpg) | [After JPEG](after/molokai-dark-editors.jpg) |
| Solarized Light | [Before JPEG](before/solarized-light-editors.jpg) | [After JPEG](after/solarized-light-editors.jpg) |
| Solarized Dark | [Before JPEG](before/solarized-dark-editors.jpg) | [After JPEG](after/solarized-dark-editors.jpg) |
