use gpui_kit::component::{ActiveTheme as _, text::TextView};
use gpui_kit::*;

/// An HTML page rendered as rich text, without running scripts or styles or
/// loading anything. Its images are removed off the UI thread first.
pub(super) struct HtmlPreview {
    page: Option<SharedString>,
    _strip: Task<()>,
}

impl HtmlPreview {
    pub(super) fn new(html: SharedString, cx: &mut Context<Self>) -> Self {
        let stripping = cx.background_spawn(async move { without_images(&html) });
        let strip = cx.spawn(async move |this, cx| {
            let page = stripping.await;
            let _ = this.update(cx, |this, cx| {
                this.page = Some(page.into());
                cx.notify();
            });
        });

        Self {
            page: None,
            _strip: strip,
        }
    }
}

impl Render for HtmlPreview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let preview = div()
            .debug_selector(|| "response-html".into())
            .size_full()
            .p_4();

        match &self.page {
            Some(page) => preview.child(
                TextView::html("response-html", page.clone())
                    .size_full()
                    .selectable(true)
                    .scrollable(true),
            ),
            None => preview
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child("Loading preview…"),
        }
    }
}

/// A page without its images. A previewed page loads nothing: GPUI Kit's rich
/// text fetches images while measuring them, outside the request's proxy
/// settings.
pub(super) fn without_images(html: &str) -> String {
    use html5ever::{ParseOpts, local_name, parse_document, serialize, tendril::TendrilSink};
    use markup5ever_rcdom::{Handle, NodeData, RcDom, SerializableHandle};

    fn remove(node: &Handle) {
        node.children.borrow_mut().retain(|child| {
            !matches!(&child.data, NodeData::Element { name, .. } if name.local == local_name!("img"))
        });

        for child in node.children.borrow().iter() {
            remove(child);
        }
    }

    // Remove the images a parser finds, so text that only looks like an image
    // tag, as in a textarea, stays as received.
    let document = parse_document(RcDom::default(), ParseOpts::default())
        .one(html)
        .document;
    remove(&document);

    let mut serialized = Vec::with_capacity(html.len());
    if serialize(
        &mut serialized,
        &SerializableHandle::from(document),
        Default::default(),
    )
    .is_err()
    {
        return String::new();
    }
    let serialized = String::from_utf8(serialized).unwrap_or_default();

    // Serialized text is escaped, except in elements such as `style`, whose
    // text a later parse can read as markup. No image tag survives as text.
    let lower = serialized.to_ascii_lowercase();
    let mut page = String::with_capacity(serialized.len());
    let mut copied = 0;

    for (start, _) in lower.match_indices('<') {
        // HTML parsers read an `image` tag as `img`.
        let Some(name) = ["img", "image"].into_iter().find(|name| {
            lower[start + 1..].starts_with(name)
                && lower[start + 1 + name.len()..].starts_with(|next: char| {
                    next.is_ascii_whitespace() || next == '/' || next == '>'
                })
        }) else {
            continue;
        };

        page.push_str(&serialized[copied..start]);
        page.push_str("<wbr");
        copied = start + 1 + name.len();
    }

    page.push_str(&serialized[copied..]);
    page
}
