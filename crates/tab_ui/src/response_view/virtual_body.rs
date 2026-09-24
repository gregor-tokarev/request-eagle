use std::ops::Range;

use gpui_kit::base::input::{self, Rope, RopeExt};
use gpui_kit::component::ActiveTheme;
use gpui_kit::*;

const FONT_SIZE: f32 = 13.;
const ROW_HEIGHT: f32 = 20.;
const GUTTER: f32 = 64.;
const SCROLLBAR: f32 = 12.;

/// A read-only viewport over the complete response. A compact list of byte
/// offsets indexes wrapped rows; only visible rows become shaped glyph layouts.
/// In particular, a one-line JSON response never becomes one giant ShapedLine.
pub(super) struct VirtualBody {
    pub(super) text: Rope,
    source: SharedString,
    pub(super) focus: FocusHandle,
    rows: Vec<usize>,
    font: Font,
    pub(super) wrap: bool,
    layout_width: Option<Pixels>,
    bounds: Bounds<Pixels>,
    pub(super) scroll: Point<Pixels>,
    pub(super) selection: Range<usize>,
    anchor: usize,
    dragging: bool,
    dragging_scrollbar: bool,
    reveal: Option<usize>,
    pub(super) painted: Vec<PaintedRow>,
    max_line_bytes: usize,
}

pub(super) struct PaintedRow {
    pub(super) range: Range<usize>,
    pub(super) line: ShapedLine,
    origin: Point<Pixels>,
    number: Option<ShapedLine>,
}

impl VirtualBody {
    pub(super) fn new(source: SharedString, wrap: bool, cx: &mut App) -> Self {
        let font = font(cx.theme().mono_font_family.clone());
        let text = Rope::from(source.as_ref());
        let max_line_bytes = source.split('\n').map(str::len).max().unwrap_or(0);

        Self {
            source,
            text,
            rows: vec![0],
            font,
            wrap,
            focus: cx.focus_handle(),
            layout_width: None,
            bounds: Bounds::default(),
            scroll: point(px(0.), px(0.)),
            selection: 0..0,
            anchor: 0,
            dragging: false,
            dragging_scrollbar: false,
            reveal: None,
            painted: Vec::new(),
            max_line_bytes,
        }
    }

    pub(super) fn set_wrap(&mut self, wrap: bool, cx: &mut Context<Self>) {
        self.reveal = Some(self.row_range(self.first_row()).start);
        self.wrap = wrap;
        self.layout_width = None;
        self.scroll.x = px(0.);
        cx.notify();
    }

    pub(super) fn select_match(&mut self, range: Option<Range<usize>>, cx: &mut Context<Self>) {
        if let Some(range) = range {
            self.reveal = Some(range.start);
            self.anchor = range.start;
            self.selection = range;
        } else {
            self.selection = self.selection.end..self.selection.end;
        }
        cx.notify();
    }

    fn first_row(&self) -> usize {
        (self.scroll.y / px(ROW_HEIGHT)).floor() as usize
    }

    pub(super) fn row_range(&self, row: usize) -> Range<usize> {
        let start = self.rows[row.min(self.rows.len() - 1)];
        let mut end = self.rows.get(row + 1).copied().unwrap_or(self.source.len());
        if end > start && self.source.as_bytes()[end - 1] == b'\n' {
            end -= 1;
            if end > start && self.source.as_bytes()[end - 1] == b'\r' {
                end -= 1;
            }
        }
        start..end
    }

    fn max_scroll_y(&self) -> Pixels {
        (px(self.rows.len() as f32 * ROW_HEIGHT) - self.bounds.size.height).max(px(0.))
    }

    fn clamp_scroll(&mut self) {
        self.scroll.y = self.scroll.y.clamp(px(0.), self.max_scroll_y());
        self.scroll.x = if self.wrap {
            px(0.)
        } else {
            self.scroll
                .x
                .clamp(px(0.), px(self.max_line_bytes as f32 * FONT_SIZE))
        };
    }

    fn prepare(&mut self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let width = (bounds.size.width - px(GUTTER + SCROLLBAR)).max(px(FONT_SIZE));
        let font = font(cx.theme().mono_font_family.clone());
        let changed = self.layout_width != Some(width) || self.font != font;
        let anchor = self.row_range(self.first_row()).start;
        self.bounds = bounds;

        if changed {
            self.font = font;
            self.rows.clear();
            self.rows.push(0);
            let mut offset = 0;
            let mut wrapper = window
                .text_system()
                .line_wrapper(self.font.clone(), px(FONT_SIZE));
            for line in self.source.split_inclusive('\n') {
                let content = line.strip_suffix('\n').unwrap_or(line);
                let content = content.strip_suffix('\r').unwrap_or(content);
                if self.wrap {
                    self.rows.extend(
                        wrapper
                            .wrap_line(&[LineFragment::text(content)], width)
                            .map(|boundary| offset + boundary.ix),
                    );
                }
                offset += line.len();
                if line.ends_with('\n') {
                    self.rows.push(offset);
                }
            }
            self.layout_width = Some(width);
            let row = self
                .rows
                .partition_point(|start| *start <= anchor)
                .saturating_sub(1);
            self.scroll.y = px(row as f32 * ROW_HEIGHT);
        }

        if let Some(offset) = self.reveal.take() {
            let position = self.text.offset_to_point(offset);
            let row = self
                .rows
                .partition_point(|start| *start <= offset)
                .saturating_sub(1);
            self.scroll.y = px(row as f32 * ROW_HEIGHT) - bounds.size.height / 2.;
            if !self.wrap {
                let prefix = &self.source[self.text.line_start_offset(position.row)..offset];
                let cell = window
                    .text_system()
                    .advance(
                        window.text_system().resolve_font(&self.font),
                        px(FONT_SIZE),
                        'm',
                    )
                    .map(|size| size.width)
                    .unwrap_or(px(8.));
                self.scroll.x = (cell * prefix.chars().count() as f32 - width / 2.).max(px(0.));
            }
        }
        self.clamp_scroll();
        self.painted.clear();
        let start = self.first_row();
        let end = (start + (bounds.size.height / px(ROW_HEIGHT)).ceil() as usize + 1)
            .min(self.rows.len());

        for row in start..end {
            let mut range = self.row_range(row);
            if !self.wrap {
                // Clip before shaping, so horizontal scrolling also has bounded
                // glyph allocation. Work on borrowed text, never copy a full line.
                let text = &self.source[range.clone()];
                let mut wrapper = window
                    .text_system()
                    .line_wrapper(self.font.clone(), px(FONT_SIZE));
                let left = if self.scroll.x > px(0.) {
                    wrapper
                        .should_truncate_line(text, self.scroll.x, "", TruncateFrom::End)
                        .unwrap_or(text.len())
                } else {
                    0
                };
                let right = wrapper
                    .should_truncate_line(&text[left..], width, "", TruncateFrom::End)
                    .unwrap_or(text.len() - left);
                range = range.start + left..range.start + left + right;
            }
            let text: SharedString = self.source[range.clone()].to_owned().into();
            let run = TextRun {
                len: text.len(),
                font: self.font.clone(),
                color: cx.theme().foreground,
                ..Default::default()
            };
            let line = window
                .text_system()
                .shape_line(text, px(FONT_SIZE), &[run.clone()], None);
            let position = self.text.offset_to_point(range.start);
            let number = if position.column == 0 {
                let number: SharedString = (position.row + 1).to_string().into();
                Some(window.text_system().shape_line(
                    number.clone(),
                    px(FONT_SIZE),
                    &[TextRun {
                        len: number.len(),
                        color: cx.theme().muted_foreground,
                        ..run
                    }],
                    None,
                ))
            } else {
                None
            };
            self.painted.push(PaintedRow {
                range,
                line,
                number,
                origin: point(
                    bounds.left() + px(GUTTER),
                    bounds.top() + px(row as f32 * ROW_HEIGHT) - self.scroll.y,
                ),
            });
        }
    }

    fn scrollbar_thumb(&self) -> Bounds<Pixels> {
        let total = px(self.rows.len() as f32 * ROW_HEIGHT).max(self.bounds.size.height);
        let height = (self.bounds.size.height * (self.bounds.size.height / total))
            .max(px(24.))
            .min(self.bounds.size.height);
        let travel = self.bounds.size.height - height;
        let y = if self.max_scroll_y() > px(0.) {
            travel * (self.scroll.y / self.max_scroll_y())
        } else {
            px(0.)
        };
        Bounds::new(
            point(self.bounds.right() - px(8.), self.bounds.top() + y),
            size(px(5.), height),
        )
    }

    fn paint(&self, window: &mut Window, cx: &mut App) {
        window.with_content_mask(
            Some(ContentMask {
                bounds: self.bounds,
            }),
            |window| {
                for row in &self.painted {
                    let start = self.selection.start.max(row.range.start);
                    let end = self.selection.end.min(row.range.end);
                    if start < end {
                        let left = row.line.x_for_index(start - row.range.start);
                        let right = row.line.x_for_index(end - row.range.start);
                        window.paint_quad(fill(
                            Bounds::new(
                                row.origin + point(left, px(0.)),
                                size(right - left, px(ROW_HEIGHT)),
                            ),
                            cx.theme().selection,
                        ));
                    }
                    let _ = row.line.paint(
                        row.origin,
                        px(ROW_HEIGHT),
                        TextAlign::Left,
                        None,
                        window,
                        cx,
                    );
                    if let Some(number) = &row.number {
                        let origin = point(
                            self.bounds.left() + px(GUTTER - 14.) - number.width(),
                            row.origin.y,
                        );
                        let _ =
                            number.paint(origin, px(ROW_HEIGHT), TextAlign::Left, None, window, cx);
                    }
                }
                if self.max_scroll_y() > px(0.) {
                    window.paint_quad(fill(
                        self.scrollbar_thumb(),
                        cx.theme().muted_foreground.opacity(0.5),
                    ));
                }
            },
        );
    }

    fn offset_at(&self, position: Point<Pixels>) -> usize {
        let Some(row) = self.painted.iter().min_by_key(|row| {
            ((row.origin.y + px(ROW_HEIGHT / 2.) - position.y).abs() / px(1.)) as usize
        }) else {
            return 0;
        };
        row.range.start + row.line.closest_index_for_x(position.x - row.origin.x)
    }

    fn select_to(&mut self, offset: usize) {
        self.selection = self.anchor.min(offset)..self.anchor.max(offset);
    }

    fn scroll_to_pointer(&mut self, position: Point<Pixels>) {
        let thumb = self.scrollbar_thumb();
        let travel = self.bounds.size.height - thumb.size.height;
        if travel > px(0.) {
            let ratio =
                ((position.y - self.bounds.top() - thumb.size.height / 2.) / travel).clamp(0., 1.);
            self.scroll.y = self.max_scroll_y() * ratio;
        }
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        if event.position.x >= self.bounds.right() - px(SCROLLBAR) {
            self.dragging_scrollbar = true;
            self.scroll_to_pointer(event.position);
        } else {
            let offset = self.offset_at(event.position);
            if !event.modifiers.shift {
                self.anchor = offset;
            }
            self.select_to(offset);
            self.dragging = true;
        }
        cx.notify();
    }

    fn mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if event.pressed_button != Some(MouseButton::Left) {
            return;
        }
        if self.dragging_scrollbar {
            self.scroll_to_pointer(event.position);
        } else if self.dragging {
            self.select_to(self.offset_at(event.position));
            if event.position.y < self.bounds.top() {
                self.scroll.y -= px(ROW_HEIGHT);
            }
            if event.position.y > self.bounds.bottom() {
                self.scroll.y += px(ROW_HEIGHT);
            }
            self.clamp_scroll();
        }
        cx.notify();
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selection = offset..offset;
        self.anchor = offset;
        self.reveal = Some(offset);
        cx.notify();
    }
}

impl Render for VirtualBody {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let prepare = cx.entity();
        let paint = cx.entity();
        div()
            .id("virtual-response")
            .debug_selector(|| "response-virtual-text".into())
            .size_full()
            .overflow_hidden()
            .key_context("Input")
            .track_focus(&self.focus)
            .aria_label("Response body")
            .cursor_text()
            .bg(cx
                .theme()
                .highlight_theme
                .style
                .editor_background
                .unwrap_or_else(|| cx.theme().input_background()))
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                let delta = event.delta.pixel_delta(px(ROW_HEIGHT));
                this.scroll.y -= delta.y;
                this.scroll.x -= delta.x;
                this.clamp_scroll();
                cx.notify();
                cx.stop_propagation();
            }))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_mouse_move(cx.listener(Self::mouse_move))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| {
                    this.dragging = false;
                    this.dragging_scrollbar = false;
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, _| {
                    this.dragging = false;
                    this.dragging_scrollbar = false;
                }),
            )
            .on_action(cx.listener(|this, _: &input::Copy, _, cx| {
                if !this.selection.is_empty() {
                    cx.write_to_clipboard(ClipboardItem::new_string(
                        this.source[this.selection.clone()].to_owned(),
                    ));
                }
                cx.stop_propagation();
            }))
            .on_action(cx.listener(|this, _: &input::SelectAll, _, cx| {
                this.anchor = 0;
                this.selection = 0..this.text.len();
                cx.notify();
                cx.stop_propagation();
            }))
            .on_action(cx.listener(|this, _: &input::MoveToStart, _, cx| this.move_to(0, cx)))
            .on_action(
                cx.listener(|this, _: &input::MoveToEnd, _, cx| this.move_to(this.text.len(), cx)),
            )
            .on_action(cx.listener(|this, _: &input::MovePageDown, _, cx| {
                this.scroll.y += this.bounds.size.height;
                this.clamp_scroll();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &input::MovePageUp, _, cx| {
                this.scroll.y -= this.bounds.size.height;
                this.clamp_scroll();
                cx.notify();
            }))
            .child(
                canvas(
                    move |bounds, window, cx| {
                        prepare.update(cx, |this, cx| this.prepare(bounds, window, cx))
                    },
                    move |_, _, window, cx| paint.update(cx, |this, cx| this.paint(window, cx)),
                )
                .size_full(),
            )
    }
}
