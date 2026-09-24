use std::fs;

use gpui_kit::{
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, TestAppContext,
    VisualTestContext, point, px,
};

use super::editing_tests::{Fixture, sidebar};
use collection::MovePlacement;

fn drag_to(
    cx: &mut VisualTestContext,
    source: &'static str,
    target: Point<Pixels>,
    intermediate_motion: bool,
) {
    let start = cx.debug_bounds(source).unwrap().center();
    cx.simulate_event(MouseDownEvent {
        button: MouseButton::Left,
        position: start,
        click_count: 1,
        ..Default::default()
    });
    if intermediate_motion {
        cx.simulate_event(MouseMoveEvent {
            position: start + point(px(10.), px(0.)),
            pressed_button: Some(MouseButton::Left),
            ..Default::default()
        });
        cx.run_until_parked();
    }
    cx.simulate_event(MouseMoveEvent {
        position: target,
        pressed_button: Some(MouseButton::Left),
        ..Default::default()
    });
    cx.run_until_parked();
    cx.simulate_event(MouseUpEvent {
        button: MouseButton::Left,
        position: target,
        click_count: 1,
        ..Default::default()
    });
    cx.run_until_parked();
}

#[gpui_kit::test]
fn dragging_moves_a_request_into_a_collection_and_back_into_a_folder(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (sidebar, cx) = sidebar(&fixture, cx);
    let other = cx.debug_bounds("collection-row-3").unwrap().center();
    drag_to(cx, "collection-row-2", other, false);
    assert!(fixture.0.join("Other/list.toml").is_file());
    assert!(!fixture.0.join("API/Users/list.toml").exists());
    cx.read(|cx| {
        let sidebar = sidebar.read(cx);
        assert!(sidebar.error.is_none());
        assert_eq!(
            sidebar.tree.items[sidebar.selected.unwrap()].path,
            fixture.0.join("Other/list.toml")
        );
    });

    let folder = cx.debug_bounds("collection-row-1").unwrap().center();
    drag_to(cx, "collection-row-3", folder, true);
    assert!(fixture.0.join("API/Users/list.toml").is_file());
    assert!(!fixture.0.join("Other/list.toml").exists());
}

#[gpui_kit::test]
fn dragging_near_a_row_edge_reorders_and_invalid_drop_keeps_the_tree(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let second = fixture.0.join("API/Users/second.toml");
    fs::write(&second, "id = 'second'\nname = 'Second'\nschema_version = 1\n[request]\ntype = 'http'\nmethod = 'GET'\npath = '/second'\n").unwrap();
    let (sidebar, cx) = sidebar(&fixture, cx);
    let first = cx.debug_bounds("collection-row-2").unwrap();
    drag_to(
        cx,
        "collection-row-3",
        point(first.center().x, first.top() + px(2.)),
        false,
    );
    cx.read(|cx| assert_eq!(sidebar.read(cx).tree.items[2].path, second));
    let child = cx.debug_bounds("collection-row-2").unwrap().center();
    drag_to(cx, "collection-row-1", child, true);
    assert!(fixture.0.join("API/Users/list.toml").is_file());
    assert!(second.is_file());
    cx.read(|cx| {
        assert_eq!(
            sidebar.read(cx).tree.items[1].path,
            fixture.0.join("API/Users")
        )
    });
}

#[gpui_kit::test]
fn moving_a_collapsed_folder_preserves_collapse_and_reveals_its_new_parent(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new();
    let (sidebar, cx) = sidebar(&fixture, cx);
    cx.update(|window, cx| {
        sidebar.update(cx, |sidebar, cx| {
            sidebar.collapsed.insert(1);
            sidebar.collapsed.insert(3);
            sidebar.move_item(
                &fixture.0.join("API/Users"),
                3,
                MovePlacement::Inside,
                window,
                cx,
            );
            assert!(sidebar.error.is_none());
            let selected = sidebar.selected.unwrap();
            assert_eq!(
                sidebar.tree.items[selected].path,
                fixture.0.join("Other/Users")
            );
            assert!(sidebar.collapsed.contains(&selected));
            assert!(
                !sidebar
                    .collapsed
                    .contains(&sidebar.tree.items[selected].parent.unwrap())
            );
        })
    });
}
