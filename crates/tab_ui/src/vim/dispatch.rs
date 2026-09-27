use std::collections::HashMap;

use gpui_kit::{component::input::EditorState, *};

use super::Vim;

struct Dispatch {
    editors: HashMap<ElementId, WeakEntity<Vim>>,
    active: HashMap<WindowId, WeakEntity<Vim>>,
    _keys: Subscription,
}

impl Global for Dispatch {}

pub(super) fn register(editor: &Entity<EditorState>, cx: &mut Context<Vim>) -> Subscription {
    if !cx.has_global::<Dispatch>() {
        let keys = cx.intercept_keystrokes(|event, window, cx| {
            if !cx
                .try_global::<preferences::Preferences>()
                .is_some_and(|preferences| preferences.vim_mode)
            {
                return;
            }

            let Some(focus) = window.focused(cx) else {
                return;
            };
            let window_id = window.window_handle().window_id();
            let dispatch = cx.global_mut::<Dispatch>();
            let editor = dispatch.editors.get(&ElementId::from(&focus)).cloned();

            // Search controls are children of the last active Vim editor.
            // Retain only a weak reference and verify its focus before dispatch.
            let target = if let Some(editor) = editor {
                dispatch.active.insert(window_id, editor.clone());
                Some(editor)
            } else {
                dispatch.active.get(&window_id).cloned()
            };

            if let Some(target) = target {
                let _ = target.update(cx, |vim, cx| vim.dispatch(event, window, cx));
            }
        });
        cx.set_global(Dispatch {
            editors: HashMap::new(),
            active: HashMap::new(),
            _keys: keys,
        });
    }

    let focus = ElementId::from(&editor.focus_handle(cx));
    let weak = cx.weak_entity();
    cx.global_mut::<Dispatch>()
        .editors
        .insert(focus.clone(), weak.clone());

    cx.on_release(move |_, cx| {
        if cx.has_global::<Dispatch>() {
            let dispatch = cx.global_mut::<Dispatch>();
            dispatch.editors.remove(&focus);
            dispatch.active.retain(|_, editor| editor != &weak);
        }
    })
}
