use std::time::Instant;

use collection::Method;
use gpui_kit::{
    AppContext as _, Keystroke, TestAppContext,
    component::{Root, input::EditorState},
    px, size,
};

use super::{draft::RequestSection, tests::new_draft};

// Measures dispatch, effects, CPU drawing and cleanup without software GPU
// presentation competing for CPU. Check the native FPS monitor separately.
// cargo test -p tab_ui --release vim_cursor_benchmark -- --ignored --nocapture --test-threads=1
#[gpui_kit::test]
#[ignore = "manual Vim cursor interaction performance comparison"]
fn vim_cursor_benchmark(cx: &mut TestAppContext) {
    #[expect(
        clippy::assertions_on_constants,
        reason = "Reject debug runs of this manual benchmark, not ordinary test builds."
    )]
    {
        assert!(!cfg!(debug_assertions), "run with --release");
    }

    let enabled = std::env::var("REQUEST_EAGLE_BENCH_VIM").as_deref() != Ok("0");
    let samples = 1000;
    let retained_count: usize = std::env::var("REQUEST_EAGLE_BENCH_EDITORS")
        .ok()
        .map(|value| {
            value
                .parse()
                .expect("REQUEST_EAGLE_BENCH_EDITORS must be a count")
        })
        .unwrap_or(0);

    cx.update(|cx| {
        gpui_kit::init(cx);
        preferences::update(cx, |p| p.vim_mode = enabled).unwrap();
        request_eagle_theme::init(cx);
        cx.set_reduce_motion(true);
    });

    for scripts in [false, true] {
        let text = if scripts {
            (0..400)
                .map(|i| {
                    format!(
                        "const item{i} = {{ id: {i}, name: \"performance fixture\", enabled: true }};\n"
                    )
                })
                .collect::<String>()
        } else {
            let rows = (0..400)
                .map(|i| {
                    format!(
                        "  \"item{i}\": {{ \"id\": {i}, \"name\": \"performance fixture\", \"enabled\": true }}"
                    )
                })
                .collect::<Vec<_>>()
                .join(",\n");

            format!("{{\n{rows}\n}}")
        };
        let mut editor = None;
        let mut retained = Vec::with_capacity(retained_count);
        let (_, cx) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| {
                let mut draft = new_draft(cx);
                draft.request.method = Method::Post;
                draft.section = if scripts {
                    RequestSection::Scripts
                } else {
                    RequestSection::Body
                };

                if scripts {
                    draft.request.scripts.pre_request = text.clone();
                } else {
                    draft.request.body = Some(text.as_bytes().to_vec());
                }

                draft.prepare(window, cx);
                let state = if scripts {
                    draft.script_state(window, cx)
                } else {
                    draft.body_state(window, cx)
                };
                state.update(cx, |state, cx| {
                    state.set_selected_range(10..10, cx);
                    state.focus(window, cx);
                });
                editor = Some(state);

                draft
            });

            for _ in 0..retained_count {
                let editor =
                    cx.new(|cx| EditorState::new(window, cx).default_value("inactive editor"));
                retained.push(cx.new(|cx| crate::vim::Vim::new(editor, cx)));
            }

            Root::new(view, window, cx)
        });
        cx.simulate_resize(size(px(1440.), px(900.)));
        cx.run_until_parked();

        let editor = editor.unwrap();
        let down = Keystroke::parse("down").unwrap();
        let up = Keystroke::parse("up").unwrap();
        let mut durations = Vec::with_capacity(samples);
        let mut allocations = Vec::with_capacity(samples);

        for i in 0..samples + 120 {
            let before = cx.read(|cx| editor.read(cx).cursor());
            let started = Instant::now();
            let allocated = crate::test_allocator::allocated_by(|| {
                cx.update(|window, cx| {
                    assert!(window.dispatch_keystroke(
                        if i % 60 < 30 {
                            down.clone()
                        } else {
                            up.clone()
                        },
                        cx
                    ));
                });
            });
            let duration = started.elapsed().as_secs_f64() * 1000.;

            if i >= 120 {
                durations.push(duration);
                allocations.push(allocated);
            }

            assert_ne!(before, cx.read(|cx| editor.read(cx).cursor()));
        }

        assert_eq!(text, cx.read(|cx| editor.read(cx).value().to_string()));
        durations.sort_by(f64::total_cmp);
        allocations.sort();

        eprintln!(
            "{} vim={enabled} retained={retained_count} n={samples} mean={:.3} p95={:.3} p99={:.3} max={:.3} allocation_p99={} over8={}",
            if scripts { "scripts" } else { "body" },
            durations.iter().sum::<f64>() / samples as f64,
            durations[(samples * 95).div_ceil(100) - 1],
            durations[(samples * 99).div_ceil(100) - 1],
            durations[samples - 1],
            allocations[(samples * 99).div_ceil(100) - 1],
            durations.iter().filter(|&&x| x > 1000. / 120.).count()
        );
    }
}
