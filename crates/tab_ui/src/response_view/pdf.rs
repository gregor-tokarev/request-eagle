use std::{ops::RangeInclusive, sync::Arc};

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{prelude::FluentBuilder as _, *};
use hayro::{
    RenderCache, RenderSettings, hayro_interpret::InterpreterSettings, hayro_syntax::Pdf,
    vello_cpu::color::palette::css::WHITE,
};

/// Pages render at twice their size in points, for sharp text on dense displays.
const SCALE: f32 = 2.;
/// The longer side of a page bitmap is at most this many pixels.
const MAX_SIDE: f32 = 4096.;
/// Pages kept rendered on each side of those in view; farther pages render again.
const KEPT_PAGES: usize = 4;
/// The list measures, and so renders, pages this far past the view. The page
/// after the view must be measured before the list can scroll to it.
const OVERDRAW: Pixels = px(800.);

/// A PDF body as a column of pages. Pages render off the UI thread as they
/// scroll into view, and only those near the view keep their bitmaps.
pub(super) struct PdfPreview {
    document: Option<Result<Arc<Pdf>, SharedString>>,
    pages: Vec<Page>,
    /// The pages in view and a few on each side, as of the last render. Only
    /// these render and keep their bitmaps.
    kept: RangeInclusive<usize>,
    list: ListState,
    _load: Task<()>,
}

struct Page {
    /// Points, as the page is shown.
    size: Size<f32>,
    image: Option<Arc<RenderImage>>,
    rendering: Option<Task<()>>,
    failed: bool,
}

impl PdfPreview {
    pub(super) fn new(bytes: Vec<u8>, cx: &mut Context<Self>) -> Self {
        let loading = cx.background_spawn(async move {
            // The document comes from a server; a malformed one must not take
            // the app down with it.
            let loaded = std::panic::catch_unwind(|| {
                let pdf = Pdf::new(bytes).ok()?;
                let sizes = pdf
                    .pages()
                    .iter()
                    .map(|page| {
                        let (width, height) = page.render_dimensions();
                        size(width, height)
                    })
                    .collect::<Vec<_>>();

                Some((Arc::new(pdf), sizes))
            });

            match loaded {
                Ok(Some((pdf, sizes))) if !sizes.is_empty() => Ok((pdf, sizes)),
                _ => Err(SharedString::from("This PDF could not be displayed")),
            }
        });
        let load = cx.spawn(async move |this, cx| {
            let loaded = loading.await;
            let _ = this.update(cx, |this, cx| {
                this.document = Some(loaded.map(|(pdf, sizes)| {
                    this.pages = sizes
                        .into_iter()
                        .map(|size| Page {
                            size,
                            image: None,
                            rendering: None,
                            failed: false,
                        })
                        .collect();
                    this.list.reset(this.pages.len());
                    pdf
                }));
                cx.notify();
            });
        });

        cx.on_release(|this, cx| {
            for page in &mut this.pages {
                if let Some(image) = page.image.take() {
                    cx.drop_image(image, None);
                }
            }
        })
        .detach();

        Self {
            document: None,
            pages: Vec::new(),
            kept: 0..=KEPT_PAGES,
            list: ListState::new(0, ListAlignment::Top, OVERDRAW),
            _load: load,
        }
    }

    pub(super) fn page_count(&self) -> Option<usize> {
        matches!(self.document, Some(Ok(_))).then_some(self.pages.len())
    }

    fn page(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        // Pages far past the view are laid out to be measured, but not rendered.
        if self.kept.contains(&index) {
            self.render_page(index, cx);
        }
        let page = &self.pages[index];
        let rem = window.rem_size() / 16.;

        div()
            .w_full()
            .flex()
            .justify_center()
            .px_4()
            .pt_4()
            .when(index + 1 == self.pages.len(), |row| row.pb_4())
            .child(
                div()
                    .debug_selector(move || format!("response-pdf-page-{index}"))
                    .w_full()
                    .max_w(rem * page.size.width)
                    .aspect_ratio(page.size.width / page.size.height)
                    .bg(white())
                    .border_1()
                    .border_color(cx.theme().border)
                    .when_some(page.image.clone(), |page, image| {
                        page.child(img(image).size_full())
                    })
                    .when(page.failed, |page| {
                        page.flex()
                            .items_center()
                            .justify_center()
                            .text_sm()
                            .text_color(black().opacity(0.6))
                            .child("This page could not be displayed")
                    }),
            )
            .into_any_element()
    }

    /// Start rendering a page that is about to show.
    fn render_page(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(Ok(pdf)) = &self.document else {
            return;
        };
        let page = &mut self.pages[index];
        if page.image.is_some() || page.rendering.is_some() || page.failed {
            return;
        }

        let pdf = pdf.clone();
        let rendering = cx.background_spawn(async move {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| render(&pdf, index)))
                .ok()
                .flatten()
        });
        page.rendering = Some(cx.spawn(async move |this, cx| {
            let image = rendering.await;
            let _ = this.update(cx, |this, cx| {
                let page = &mut this.pages[index];
                page.rendering = None;
                page.failed = image.is_none();
                page.image = image.map(Arc::new);
                // Scrolling may have moved on while the page rendered.
                this.release_far_pages(None, cx);
                cx.notify();
            });
        }));
    }

    /// The pages in view and a few on each side, by the list's last layout.
    /// The list's state is borrowed while it lays out pages, so this must be
    /// read before.
    fn kept_pages(&self) -> RangeInclusive<usize> {
        let first = self.list.logical_scroll_top().item_ix;
        let last = (first..self.pages.len())
            .take_while(|&page| self.list.item_is_below_viewport(page) == Some(false))
            .last()
            .unwrap_or(first);

        first.saturating_sub(KEPT_PAGES)..=last + KEPT_PAGES
    }

    /// Release the bitmaps of pages far from the view, and stop rendering them.
    fn release_far_pages(&mut self, mut window: Option<&mut Window>, cx: &mut App) {
        for (index, page) in self.pages.iter_mut().enumerate() {
            if self.kept.contains(&index) {
                continue;
            }

            page.rendering = None;
            if let Some(image) = page.image.take() {
                cx.drop_image(image, window.as_deref_mut());
            }
        }
    }
}

/// A page's bitmap, white where the page draws nothing.
fn render(pdf: &Pdf, index: usize) -> Option<RenderImage> {
    let page = pdf.pages().get(index)?;
    let (width, height) = page.render_dimensions();
    let scale = SCALE.min(MAX_SIDE / width.max(height).max(1.));
    let pixmap = hayro::render(
        page,
        &RenderCache::new(),
        &InterpreterSettings::default(),
        &RenderSettings {
            x_scale: scale,
            y_scale: scale,
            bg_color: WHITE,
            ..Default::default()
        },
    );

    // The page is opaque, so premultiplied pixels are the same as straight
    // ones. GPUI takes BGRA.
    let mut pixels = pixmap.data_as_u8_slice().to_vec();
    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    let buffer = image::RgbaImage::from_raw(
        u32::from(pixmap.width()),
        u32::from(pixmap.height()),
        pixels,
    )?;

    Some(RenderImage::new(vec![image::Frame::new(buffer)]))
}

impl Render for PdfPreview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Scrolling the list renders this view again. A resized list measures
        // its new view after this render, so check once it has.
        self.kept = self.kept_pages();
        self.release_far_pages(Some(window), cx);
        cx.on_next_frame(window, |this, _, cx| {
            if this.kept_pages() != this.kept {
                cx.notify();
            }
        });

        let message = |text: SharedString| {
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(text)
        };

        match &self.document {
            None => message("Loading PDF…".into()).into_any_element(),
            Some(Err(error)) => message(error.clone()).into_any_element(),
            Some(Ok(_)) => div()
                .debug_selector(|| "response-pdf".into())
                .size_full()
                .bg(cx.theme().muted)
                .child(list(self.list.clone(), cx.processor(Self::page)).size_full())
                .into_any_element(),
        }
    }
}
