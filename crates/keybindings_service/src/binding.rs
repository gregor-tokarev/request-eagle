use std::rc::Rc;

use gpui_kit::{
    Action, App, InvalidKeystrokeError, KeyBinding, KeyBindingContextPredicate, Keystroke,
    OperatingSystem, SharedString, Unbind,
};

use crate::KeybindingError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    pub keystrokes: String,
    pub context: Option<String>,
}

pub(crate) struct RegisteredBinding {
    pub(crate) action_name: &'static str,
    pub(crate) binding: Binding,
}

impl Binding {
    pub(crate) fn new(
        keystrokes: &str,
        context: Option<&str>,
        os: OperatingSystem,
    ) -> Result<Self, KeybindingError> {
        let keystrokes = keystrokes.trim();
        if keystrokes.is_empty() {
            return Err(KeybindingError::EmptyKeystrokes);
        }

        if let Some(context) = context {
            KeyBindingContextPredicate::parse(context).map_err(|source| {
                KeybindingError::InvalidContext {
                    context: context.to_owned(),
                    source,
                }
            })?;
        }

        Ok(Self {
            keystrokes: keystrokes
                .split_whitespace()
                .map(|stroke| Keystroke::parse_with_os(stroke, os))
                .map(|stroke| stroke.map(|stroke| stroke.unparse(os)))
                .collect::<Result<Vec<_>, _>>()?
                .join(" "),
            context: context.map(str::to_owned),
        })
    }

    pub(crate) fn to_gpui(
        &self,
        action: Box<dyn Action>,
        cx: &App,
    ) -> Result<KeyBinding, InvalidKeystrokeError> {
        let predicate = self
            .context
            .as_deref()
            .map(KeyBindingContextPredicate::parse)
            .transpose()
            .expect("binding context was validated when the binding was created")
            .map(Rc::new);

        KeyBinding::load(&self.keystrokes, action, predicate, false, None, cx)
    }
}

impl RegisteredBinding {
    pub(crate) fn unbind(self, cx: &App) -> KeyBinding {
        self.binding
            .to_gpui(Box::new(Unbind(SharedString::from(self.action_name))), cx)
            .expect("registered binding was already validated")
    }
}
