use gpui_kit::{AppContext as _, Entity, Modifiers, TestAppContext, VisualTestContext};
use request::Method;

use super::{RequestDraft, draft::RequestSection};
use crate::variables::VariableStore;

fn setup(cx: &mut TestAppContext) -> (Entity<RequestDraft>, &mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        preferences::init(cx);
        request_eagle_theme::init(cx);
        VariableStore::global(cx).update(cx, |store, _| {
            store.environments.insert(
                None,
                [
                    ("base_url".into(), "https://example.com".into()),
                    ("message".into(), "hello".into()),
                ]
                .into(),
            );
            store
                .secrets
                .insert("token".into(), "never-display-this".into());
        });
    });
    let mut draft = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| {
            let mut draft = RequestDraft::new();
            draft.prepare(window, cx);
            draft
        });
        draft = Some(view.clone());
        gpui_kit::component::Root::new(view, window, cx)
    });
    (draft.unwrap(), cx)
}

fn click(cx: &mut VisualTestContext, selector: &'static str) {
    cx.update(|window, _| window.refresh());
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    cx.simulate_click(bounds.center(), Modifiers::default());
}

fn popup(cx: &mut VisualTestContext) -> bool {
    cx.update(|window, _| window.refresh());
    cx.debug_bounds("variable-completions").is_some()
}

#[gpui_kit::test]
fn variable_completion_filters_accepts_dismisses_and_supports_undo(cx: &mut TestAppContext) {
    let (draft, cx) = setup(cx);
    click(cx, "request-url");
    cx.simulate_input("{");
    assert!(!popup(cx));
    cx.simulate_input("{");
    assert!(popup(cx));
    cx.simulate_keystrokes("down enter");
    cx.read(|cx| assert_eq!(draft.read(cx).request.path, "{{message}}"));
    assert!(!popup(cx));
    cx.simulate_keystrokes("secondary-z");
    cx.read(|cx| assert_eq!(draft.read(cx).request.path, "{{"));
    cx.simulate_input("base");
    assert!(popup(cx));
    cx.simulate_keystrokes("tab");
    cx.read(|cx| assert_eq!(draft.read(cx).request.path, "{{base_url}}"));

    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input("{{does_not_exist");
    assert!(popup(cx));
    cx.simulate_keystrokes("escape");
    assert!(!popup(cx));
    cx.read(|cx| assert_eq!(draft.read(cx).request.path, "{{does_not_exist"));
}

#[gpui_kit::test]
fn variable_completion_works_in_params_headers_and_json(cx: &mut TestAppContext) {
    let (draft, cx) = setup(cx);
    click(cx, "request-section-Params");
    click(cx, "params-key-0");
    cx.simulate_input("{{mess");
    cx.simulate_keystrokes("enter");
    click(cx, "params-value-0");
    cx.simulate_input("{{$guid");
    assert!(popup(cx));
    click(cx, "variable-suggestion-0");
    cx.read(|cx| {
        assert_eq!(
            draft.read(cx).request.query.as_ref().unwrap()[0],
            ("{{message}}".into(), "{{$guid}}".into())
        )
    });

    click(cx, "request-section-Headers");
    click(cx, "headers-key-0");
    cx.simulate_input("X-{{mess");
    cx.simulate_keystrokes("tab");
    click(cx, "headers-value-0");
    cx.simulate_input("Bearer {{vault:");
    cx.simulate_keystrokes("enter");
    cx.read(|cx| {
        assert_eq!(
            draft.read(cx).request.headers[0],
            ("X-{{message}}".into(), "Bearer {{vault:token}}".into())
        )
    });

    cx.update(|window, cx| {
        draft.update(cx, |draft, cx| {
            draft.set_method(Method::Post, cx);
            draft.section = RequestSection::Body;
            draft.prepare(window, cx);
            draft.body.as_ref().unwrap().update(cx, |body, cx| {
                body.replace_all("{\"time\":\"{{$iso}}\"}", window, cx);
                body.set_selected_range(15..15, cx);
                body.focus(window, cx);
            });
        })
    });
    assert!(popup(cx));
    cx.simulate_keystrokes("enter");
    cx.read(|cx| {
        assert_eq!(
            draft.read(cx).request.body.as_deref().unwrap(),
            br#"{"time":"{{$isoTimestamp}}"}"#
        )
    });
}

#[gpui_kit::test]
fn variable_completion_handles_unicode_blur_and_window_edges_at_all_scales(
    cx: &mut TestAppContext,
) {
    use gpui_kit::{px, size};

    let (draft, cx) = setup(cx);
    for theme in ["Ayu Light", "Ayu Dark"] {
        for font_size in [12., 16., 24.] {
            cx.update(|window, cx| {
                request_eagle_theme::apply(theme, cx);
                gpui_kit::component::Theme::global_mut(cx).font_size = px(font_size);
                window.set_rem_size(px(font_size));
                window.refresh();
            });
            cx.simulate_resize(size(px(1024.), px(800.)));
            let text = format!(
                "https://example.com/{}/🦅/{{{{base}}}}/tail",
                "long-path/".repeat(20)
            );
            let caret = text.find("base}}").unwrap() + 4;
            cx.update(|window, cx| {
                draft.read(cx).url.clone().unwrap().update(cx, |input, cx| {
                    input.set_value(text.clone(), window, cx);
                    input.set_selected_range(caret..caret, cx);
                    input.focus(window, cx);
                });
            });
            assert!(popup(cx));
            let bounds = cx.debug_bounds("variable-completions").unwrap();
            assert!(bounds.left() >= px(0.) && bounds.right() <= px(1024.));
            assert!(bounds.top() >= px(0.) && bounds.bottom() <= px(800.));
            cx.simulate_keystrokes("enter");
            cx.read(|cx| {
                assert_eq!(
                    draft.read(cx).request.path,
                    text.replace("{{base}}", "{{base_url}}")
                )
            });

            cx.simulate_keystrokes("secondary-a");
            cx.simulate_input("{{");
            assert!(popup(cx));
            click(cx, "headers-key-0");
            assert!(!popup(cx));
        }
    }
}

#[gpui_kit::test]
fn unresolved_variables_block_send_and_collection_scope_changes_with_the_request(
    cx: &mut TestAppContext,
) {
    let (draft, cx) = setup(cx);
    cx.update(|window, cx| {
        draft.update(cx, |draft, cx| {
            draft.request.path = "http://127.0.0.1:1/{{missing}}".into();
            draft.send(window, cx);
            assert!(
                draft.task.is_none(),
                "resolution must fail before dispatching HTTP"
            );

            draft.set_variable_environment(
                std::path::Path::new("/tmp/variable-test/Collection/Folder/request.toml"),
                1,
                cx,
            );
            assert_eq!(
                draft.variables(cx).read(cx).path.as_deref(),
                Some(std::path::Path::new(
                    "/tmp/variable-test/Collection/environment.toml"
                ))
            );
            let path = draft.variables(cx).read(cx).path.clone();
            assert!(
                VariableStore::global(cx)
                    .read(cx)
                    .values(&path)
                    .unwrap()
                    .environment
                    .is_empty()
            );
        });
    });
}
