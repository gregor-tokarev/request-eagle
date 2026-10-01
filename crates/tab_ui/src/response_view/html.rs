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

/// A page that no parse can read an image from. GPUI Kit's rich text fetches
/// images while measuring them, outside the request's proxy settings. It also
/// parses a page twice, writing some elements' text back unescaped in
/// between, so markup that only looks like text could become an image.
///
/// The page is rebuilt from a parse instead: images, scripts and styles are
/// left out, `<` in text and attribute values is escaped, and one in text
/// that could start a tag is followed by a zero-width space, which keeps it
/// text even when written back unescaped.
pub(super) fn without_images(html: &str) -> String {
    use html5ever::{ParseOpts, parse_document, tendril::TendrilSink};
    use markup5ever_rcdom::{Handle, NodeData, RcDom};

    const VOID: [&str; 13] = [
        "area", "base", "br", "col", "embed", "hr", "input", "keygen", "link", "meta", "param",
        "source", "track",
    ];

    let is_name = |name: &str| {
        !name.is_empty()
            && name.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '-' | '_')
            })
    };

    fn write(node: &Handle, page: &mut String, is_name: &dyn Fn(&str) -> bool) {
        let children = |page: &mut String| {
            for child in node.children.borrow().iter() {
                write(child, page, is_name);
            }
        };

        match &node.data {
            NodeData::Document => children(page),
            NodeData::Text { contents } => {
                let text = contents.borrow();
                let mut characters = text.chars().peekable();

                while let Some(character) = characters.next() {
                    match character {
                        '&' => page.push_str("&amp;"),
                        '>' => page.push_str("&gt;"),
                        '<' => {
                            page.push_str("&lt;");
                            if characters.peek().is_some_and(|next| {
                                next.is_ascii_alphabetic() || matches!(next, '/' | '!' | '?')
                            }) {
                                page.push('\u{200b}');
                            }
                        }
                        character => page.push(character),
                    }
                }
            }
            NodeData::Element { name, attrs, .. } => {
                let name = name.local.as_ref();
                if matches!(name, "img" | "image" | "script" | "style" | "template") {
                    return;
                }
                // A name the tokenizer could split leaves out only its tags.
                if !is_name(name) {
                    return children(page);
                }

                page.push('<');
                page.push_str(name);
                for attribute in attrs.borrow().iter() {
                    let attribute_name = attribute.name.local.as_ref();
                    if !is_name(attribute_name) {
                        continue;
                    }

                    page.push(' ');
                    page.push_str(attribute_name);
                    page.push_str("=\"");
                    for character in attribute.value.chars() {
                        match character {
                            '&' => page.push_str("&amp;"),
                            '"' => page.push_str("&quot;"),
                            '<' => page.push_str("&lt;"),
                            '>' => page.push_str("&gt;"),
                            character => page.push(character),
                        }
                    }
                    page.push('"');
                }
                page.push('>');

                if !VOID.contains(&name) {
                    children(page);
                    page.push_str("</");
                    page.push_str(name);
                    page.push('>');
                }
            }
            // Doctypes, comments and processing instructions show nothing.
            _ => {}
        }
    }

    let document = parse_document(RcDom::default(), ParseOpts::default())
        .one(html)
        .document;
    let mut page = String::with_capacity(html.len());
    write(&document, &mut page, &is_name);

    page
}
