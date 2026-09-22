use std::time::{Duration, Instant};

use gpui_kit::{
    AppContext as _, InputEvent as _, Modifiers, ScrollDelta, ScrollWheelEvent, TestAppContext,
    TouchPhase,
    component::{Root, input::EditorState},
    point, px, size,
};
use request::{Execution, HeaderMap, HttpResponse, Response, StatusCode, Version};

use super::{ResponseContent, ResponseView, body::ResponseBodyEditor};

// Use a saved response so network changes cannot alter measurements mid-run.
// REQUEST_EAGLE_HTML_FIXTURE=/tmp/page.html cargo test -p workspace --release
// standard_html_editor_benchmark -- --ignored --nocapture --test-threads=1
#[gpui_kit::test]
#[ignore = "manual standard editor HTML performance and allocation comparison"]
fn standard_html_editor_benchmark(cx: &mut TestAppContext) {
    assert!(!cfg!(debug_assertions), "run with --release");
    let body = std::env::var("REQUEST_EAGLE_HTML_FIXTURE")
        .map(|path| std::fs::read(path).expect("read HTML fixture"))
        .unwrap_or_else(|_| {
            format!(
                "<!doctype html><html><body>{}</body></html>",
                "<article class=\"card\"><h2>Example response</h2><p>Searchable text</p></article>"
                    .repeat(1200)
            )
            .into_bytes()
        });
    let mut headers = HeaderMap::new();
    headers.insert("content-type", "text/html".parse().unwrap());
    let content = ResponseContent::new(Execution {
        elapsed: Duration::from_millis(1),
        response: Response::Http(HttpResponse {
            status: StatusCode::OK,
            version: Version::HTTP_11,
            headers,
            body,
            metrics: Default::default(),
        }),
    });
    let text = content.raw.clone();
    let longest = text.split('\n').map(str::len).max().unwrap_or(0);
    let mut offset = 0;
    let mut longest_offset = 0;
    for line in text.split_inclusive('\n') {
        if line.trim_end_matches('\n').len() == longest {
            longest_offset = offset;
            break;
        }
        offset += line.len();
    }
    let bytes = text.len();
    cx.update(|cx| {
        gpui_kit::init(cx);
        request_eagle_theme::init(cx);
        cx.set_reduce_motion(true);
    });
    let mut editor = None;
    let load = Instant::now();
    let (_, cx) = cx.add_window_view(|window, cx| {
        let response = cx.new(|cx| {
            let mut view = ResponseView::new(cx);
            view.finish(Ok(content), window, cx);
            // Deliberately bypass the production fallback for this comparison.
            let state = view.editor.clone().unwrap_or_else(|| {
                cx.new(|cx| {
                    EditorState::new(window, cx)
                        .language("html")
                        .line_number(true)
                        .soft_wrap(true)
                        .searchable(true)
                        .replaceable(false)
                        .default_value(text)
                })
            });
            view.virtual_body = None;
            view.editor_view = Some(cx.new(|_| ResponseBodyEditor(state.clone())));
            view.editor = Some(state.clone());
            editor = Some(state);
            view
        });
        Root::new(response, window, cx)
    });
    cx.run_until_parked();
    eprintln!(
        "Standard HTML editor: {bytes} bytes, longest line {longest} bytes, initial window {:.2} ms",
        load.elapsed().as_secs_f64() * 1000.
    );
    let editor = editor.unwrap();

    for width in [1024., 640.] {
        cx.simulate_resize(size(px(width), px(768.)));
        cx.run_until_parked();
        for (region, anchor) in [("top", 0), ("longest line", longest_offset)] {
            cx.update(|_, cx| {
                editor.update(cx, |editor, cx| {
                    editor.set_selected_range(anchor..anchor, cx)
                })
            });
            cx.run_until_parked();
            if anchor > 0 {
                assert!(cx.read(|cx| editor.read(cx).scroll_offset().y) < px(0.));
            }
            let body = cx.debug_bounds("response-body").unwrap();
            let mut samples = Vec::new();
            let mut maximum_allocated = 0;
            for index in 0..140 {
                let before = cx.read(|cx| editor.read(cx).scroll_offset());
                let started = Instant::now();
                let allocated = crate::test_allocator::allocated_by(|| {
                    cx.update(|window, cx| {
                        window.dispatch_event(
                            ScrollWheelEvent {
                                position: body.center(),
                                delta: ScrollDelta::Pixels(point(
                                    px(0.),
                                    px(if index % 40 < 20 { -24. } else { 24. }),
                                )),
                                modifiers: Modifiers::default(),
                                touch_phase: TouchPhase::Moved,
                            }
                            .to_platform_input(),
                            cx,
                        )
                    });
                    cx.run_until_parked();
                });
                if index == 0 {
                    assert_ne!(
                        cx.read(|cx| editor.read(cx).scroll_offset()),
                        before,
                        "must actually scroll"
                    );
                }
                if index >= 20 {
                    samples.push(started.elapsed().as_secs_f64() * 1000.);
                    maximum_allocated = maximum_allocated.max(allocated);
                }
            }
            samples.sort_by(f64::total_cmp);
            eprintln!(
                "Standard HTML {width}px {region} scroll: p95 {:.2} ms, p99 {:.2} ms, max {:.2} ms, max allocations {maximum_allocated} bytes",
                samples[113], samples[118], samples[119]
            );

            for query in ["html", "e"] {
                cx.update(|_, cx| {
                    editor.update(cx, |editor, cx| {
                        editor.set_selected_range(anchor..anchor, cx)
                    })
                });
                cx.run_until_parked();
                let started = Instant::now();
                cx.update(|_, cx| {
                    editor.update(cx, |editor, cx| editor.set_search_query(query, true, cx))
                });
                cx.run_until_parked();
                let count = cx.read(|cx| editor.read(cx).search_session().matcher.len());
                eprintln!(
                    "Standard HTML {width}px {region} search {query:?}: {count} matches, {:.2} ms",
                    started.elapsed().as_secs_f64() * 1000.
                );
                let started = Instant::now();
                for _ in 0..8 {
                    cx.update(|_, cx| editor.update(cx, |editor, cx| editor.next_search_match(cx)));
                    cx.run_until_parked();
                }
                eprintln!(
                    "Standard HTML {width}px {region} 8 next matches: {:.2} ms",
                    started.elapsed().as_secs_f64() * 1000.
                );
                cx.update(|_, cx| {
                    editor.update(cx, |editor, cx| {
                        editor.close_search(cx);
                        editor.set_scroll_offset(point(px(0.), px(0.)), cx);
                        editor.set_selected_range(0..0, cx);
                    })
                });
            }
        }
    }
}
