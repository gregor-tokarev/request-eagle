use std::sync::Arc;

use gpui_kit::component::*;
use gpui_kit::*;

/// An image body, decoded off the UI thread. It shows at its own size, or
/// smaller to fit. Its texture is released with the view.
pub(super) struct ImagePreview {
    pub(super) decoded: Option<Result<Arc<RenderImage>, SharedString>>,
    /// Bitmap pixels per image pixel. GPUI renders SVG larger, for smooth edges.
    scale: f32,
    _decode: Task<()>,
}

impl ImagePreview {
    pub(super) fn new(image: Arc<Image>, cx: &mut Context<Self>) -> Self {
        let scale = if image.format == ImageFormat::Svg {
            SMOOTH_SVG_SCALE_FACTOR
        } else {
            1.
        };
        let renderer = cx.svg_renderer();
        let decoding = cx.background_spawn(async move { image.to_image_data(renderer) });
        let decode = cx.spawn(async move |this, cx| {
            let decoded = decoding.await.map_err(|error| error.to_string().into());
            let _ = this.update(cx, |this, cx| {
                this.decoded = Some(decoded);
                cx.notify();
            });
        });

        cx.on_release(|this, cx| {
            if let Some(Ok(image)) = this.decoded.take() {
                cx.drop_image(image, None);
            }
        })
        .detach();

        Self {
            decoded: None,
            scale,
            _decode: decode,
        }
    }
}

impl Render for ImagePreview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let preview = v_flex()
            .debug_selector(|| "response-image".into())
            .size_full()
            .min_h_0()
            .p_4()
            .gap_2()
            .items_center()
            .justify_center()
            .text_sm()
            .text_color(cx.theme().muted_foreground);

        match &self.decoded {
            None => preview.child("Loading image…"),
            Some(Err(error)) => preview
                .child(
                    div()
                        .text_color(cx.theme().foreground)
                        .child("This image could not be displayed"),
                )
                .child(div().text_xs().child(error.clone())),
            Some(Ok(image)) => {
                let pixels = image.size(0);
                let width = (pixels.width.0 as f32 / self.scale).round();
                let height = (pixels.height.0 as f32 / self.scale).round();

                preview
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .w_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                img(image.clone())
                                    .debug_selector(|| "response-image-content".into())
                                    .size_full()
                                    .max_w(px(width))
                                    .max_h(px(height))
                                    .object_fit(ObjectFit::Contain),
                            ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_xs()
                            .child(format!("{width} × {height}")),
                    )
            }
        }
    }
}
