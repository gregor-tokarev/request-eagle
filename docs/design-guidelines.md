# Request Eagle interface geometry

Follow the [GPUI Kit design guide](https://gpui-kit.com/versions/main/docs/design-guides/#radius-spacing-and-density).
The app uses the GPUI Kit 0.6.2 APIs already pinned in the workspace.

- Use theme radius tiers: `sm` for inset and compact controls, `md` for rows,
  tabs and control frames, and `lg` for surfaces. Full rounding belongs to
  intentional dots or pills. Inset backgrounds must follow the outer corner;
  keep focus rings outside clipping regions.
- Use the relative spacing helpers: 2, 4, 8, 12, 16, 24 and 32 px at the
  default 16 px interface size. Labels stay close to their controls; sections
  receive more space. Settings pages share their insets and width constraints
  through `settings_ui::geometry`.
- Use `text_xs`, `text_sm`, `text_base`, `text_lg` and `text_xl` for interface
  hierarchy. HTTP methods and counts remain readable at the smallest tier.
- Keep standard medium controls in forms and dialogs. Use small controls in
  request toolbars and tables. App-owned collection and tab rows use a 2 rem
  frame. Let component sizing determine control typography and hit targets.

Interface font size is also interface zoom. Pane constraints, tab virtualization,
theme-card layout, cached keybinding rows and response wrapping must measure the
same relative geometry they render. Preserve selection and source position when
remeasuring response text.

Pixels remain appropriate for native title-bar insets, platform window bounds,
hairlines and focus-ring strokes. Resolve relative dimensions to pixels at APIs
that require measured geometry. The initial window minimum reserves 40 × 40 rem
for the workspace at the selected font size.

For geometry changes, exercise 12, 16 and 24 px interface sizes, light and dark
themes, keyboard navigation, resizing, selection and offscreen virtual content.
