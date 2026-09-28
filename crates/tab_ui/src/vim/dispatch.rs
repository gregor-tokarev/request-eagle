use std::collections::HashMap;

use gpui_kit::{component::input::EditorState, *};

use super::Vim;

struct Dispatch {
    editors: HashMap<ElementId, WeakEntity<Vim>>,
    search: Option<(WeakFocusHandle, Option<WeakEntity<Vim>>)>,
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
            let dispatch = cx.global::<Dispatch>();
            let target = if let Some(editor) = dispatch.editors.get(&ElementId::from(&focus)) {
                Some(editor.clone())
            } else if event
                .context_stack
                .iter()
                .any(|context| context.contains("SearchPanel"))
            {
                if let Some((_, target)) =
                    dispatch.search.as_ref().filter(|(last, _)| last == &focus)
                {
                    target.clone()
                } else {
                    // Native search fields expose no owner/focus-handle API.
                    // Resolve their containing Vim view once per focused field,
                    // caching misses too so unrelated search typing stays cheap.
                    let target = dispatch.editors.values().find_map(|editor| {
                        editor
                            .upgrade()?
                            .focus_handle(cx)
                            .contains_focused(window, cx)
                            .then(|| editor.clone())
                    });
                    cx.global_mut::<Dispatch>().search = Some((focus.downgrade(), target.clone()));
                    target
                }
            } else {
                None
            };

            if let Some(target) = target {
                let _ = target.update(cx, |vim, cx| vim.dispatch(event, window, cx));
            }
        });
        cx.set_global(Dispatch {
            editors: HashMap::new(),
            search: None,
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
            if dispatch
                .search
                .as_ref()
                .is_some_and(|(_, target)| target.as_ref() == Some(&weak))
            {
                dispatch.search = None;
            }
        }
    })
}
