use super::storage;
use crate::binding::{Binding, RegisteredBinding};

use std::{
    any::TypeId,
    collections::{BTreeMap, HashMap},
    path::PathBuf,
};

use gpui_kit::{Action, App, BorrowAppContext, Global, InvalidKeystrokeError};
use thiserror::Error;

#[derive(Default)]
pub struct KeybindingsService {
    bindings: HashMap<TypeId, RegisteredBinding>,
    commands: HashMap<&'static str, RegisteredCommand>,

    overrides: BTreeMap<String, Option<String>>,
    storage_path: Option<PathBuf>,
    storage_error: Option<String>,
}

impl Global for KeybindingsService {}

struct RegisteredCommand {
    action: Box<dyn Action>,
    label: &'static str,
    description: &'static str,
    category: &'static str,
    default_binding: Option<Binding>,
    context: Option<&'static str>,
    binding_error: Option<String>,
}

/// A command shown in Settings, including commands without a shortcut.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub category: &'static str,

    pub binding: Option<Binding>,
    pub default_binding: Option<Binding>,
    pub binding_error: Option<String>,
}

impl Command {
    pub fn is_modified(&self) -> bool {
        self.binding_error.is_some() || self.binding != self.default_binding
    }
}

pub fn init(cx: &mut App) {
    if !cx.has_global::<KeybindingsService>() {
        cx.set_global(KeybindingsService::default());
    }
}

/// Call before registering application commands. Tests can keep the service in
/// memory by omitting this or supply an isolated path.
pub fn load_overrides(path: PathBuf, cx: &mut App) -> anyhow::Result<()> {
    init(cx);

    let result = storage::load(&path);

    let service = cx.global_mut::<KeybindingsService>();
    service.storage_path = Some(path);

    match result {
        Ok(overrides) => {
            service.overrides = overrides;
            service.storage_error = None;

            Ok(())
        }
        Err(error) => {
            service.storage_error = Some(format!("Could not load saved key bindings: {error}"));

            Err(error)
        }
    }
}

pub fn storage_error(cx: &App) -> Option<&str> {
    cx.try_global::<KeybindingsService>()?
        .storage_error
        .as_deref()
}

/// Register each user-facing command once, with its built-in shortcut.
/// Register fixed bindings first. If a resolved shortcut conflicts with an
/// earlier binding, leave this command unassigned and report the error in
/// Settings. Keep the saved value until the user repairs or resets it.
pub fn register<A: Action>(
    action: A,
    label: &'static str,
    description: &'static str,
    category: &'static str,
    default_keystrokes: Option<&str>,
    context: Option<&'static str>,
    cx: &mut App,
) -> Result<(), KeybindingError> {
    let default_binding = default_keystrokes
        .map(|keys| Binding::new(keys, context))
        .transpose()?;

    init(cx);

    cx.update_global::<KeybindingsService, _>(|service, cx| {
        let id = A::name_for_type();

        if service.commands.contains_key(id) {
            return Ok(());
        }

        let mut binding = match service.overrides.get(id) {
            Some(keys) => keys
                .as_deref()
                .map(|keys| Binding::new(keys, context))
                .transpose()?,
            None => default_binding.clone(),
        };

        let binding_error = binding
            .as_ref()
            .and_then(|binding| service.check_conflict(id, binding).err())
            .map(|error| format!("Shortcut disabled: {error}"));

        if binding_error.is_some() {
            binding = None;
        }

        let command = RegisteredCommand {
            action: Box::new(action),
            label,
            description,
            category,
            default_binding,
            context,
            binding_error,
        };

        service.replace_binding(command.action.boxed_clone(), binding, cx)?;
        service.commands.insert(id, command);

        Ok(())
    })
}

pub fn commands(cx: &App) -> Vec<Command> {
    let Some(service) = cx.try_global::<KeybindingsService>() else {
        return Vec::new();
    };

    service
        .commands
        .iter()
        .map(|(&id, command)| Command {
            id,
            label: command.label,
            description: command.description,
            category: command.category,
            binding: service
                .bindings
                .get(&command.action.as_any().type_id())
                .map(|b| b.binding.clone()),
            default_binding: command.default_binding.clone(),
            binding_error: command.binding_error.clone(),
        })
        .collect()
}

/// Check a proposed shortcut without changing the keymap or saved settings.
pub fn validate_override(id: &str, keystrokes: &str, cx: &App) -> Result<(), KeybindingError> {
    let service = cx.global::<KeybindingsService>();
    let command = service
        .commands
        .get(id)
        .ok_or(KeybindingError::UnknownCommand)?;
    let binding = Binding::new(keystrokes, command.context)?;

    service.check_conflict(id, &binding)
}

/// Save first, then update the running keymap. A failed write leaves the active
/// shortcut unchanged. `None` explicitly removes a command's shortcut.
pub fn set_override(
    id: &str,
    keystrokes: Option<&str>,
    cx: &mut App,
) -> Result<(), KeybindingError> {
    cx.update_global::<KeybindingsService, _>(|service, cx| {
        let command = service
            .commands
            .get(id)
            .ok_or(KeybindingError::UnknownCommand)?;

        let binding = keystrokes
            .map(|keys| Binding::new(keys, command.context))
            .transpose()?;

        if let Some(binding) = &binding {
            service.check_conflict(id, binding)?;
        }

        let mut overrides = service.overrides.clone();
        if binding == command.default_binding {
            overrides.remove(id);
        } else {
            overrides.insert(
                id.to_owned(),
                binding.as_ref().map(|b| b.keystrokes.clone()),
            );
        }

        service.save(&overrides)?;
        service.apply(id, binding, cx)?;
        service.overrides = overrides;

        cx.refresh_windows();

        Ok(())
    })
}

pub fn reset_command(id: &str, cx: &mut App) -> Result<(), KeybindingError> {
    let keys = cx
        .global::<KeybindingsService>()
        .commands
        .get(id)
        .ok_or(KeybindingError::UnknownCommand)?
        .default_binding
        .as_ref()
        .map(|b| b.keystrokes.clone());

    set_override(id, keys.as_deref(), cx)
}

pub fn reset_all(cx: &mut App) -> Result<(), KeybindingError> {
    cx.update_global::<KeybindingsService, _>(|service, cx| {
        service.save(&BTreeMap::new())?;

        let defaults = service
            .commands
            .iter()
            .map(|(&id, c)| (id, c.default_binding.clone()))
            .collect::<Vec<_>>();

        for (id, binding) in defaults {
            service.apply(id, binding, cx)?;
        }

        service.overrides.clear();

        cx.refresh_windows();

        Ok(())
    })
}

/// Sets the active binding for an action. Calling this again replaces the
/// previous binding immediately without clearing bindings owned by GPUI or
/// other services.
pub fn set_binding<A: Action>(
    keystrokes: &str,
    action: A,
    context: Option<&str>,
    cx: &mut App,
) -> Result<(), KeybindingError> {
    let binding = Binding::new(keystrokes, context)?;

    init(cx);

    cx.update_global::<KeybindingsService, _>(|service, cx| {
        service.replace_binding(Box::new(action), Some(binding), cx)
    })
}

pub fn binding_for<A: Action>(cx: &App) -> Option<&Binding> {
    cx.try_global::<KeybindingsService>()?
        .bindings
        .get(&TypeId::of::<A>())
        .map(|registered| &registered.binding)
}

impl KeybindingsService {
    fn apply(
        &mut self,
        id: &str,
        binding: Option<Binding>,
        cx: &mut App,
    ) -> Result<(), KeybindingError> {
        let action = self
            .commands
            .get(id)
            .ok_or(KeybindingError::UnknownCommand)?
            .action
            .boxed_clone();

        self.replace_binding(action, binding, cx)?;
        self.commands.get_mut(id).unwrap().binding_error = None;

        Ok(())
    }

    fn replace_binding(
        &mut self,
        action: Box<dyn Action>,
        binding: Option<Binding>,
        cx: &mut App,
    ) -> Result<(), KeybindingError> {
        let action_type = action.as_any().type_id();
        let action_name = action.name();

        if self.bindings.get(&action_type).map(|entry| &entry.binding) == binding.as_ref() {
            return Ok(());
        }

        let gpui_binding = binding
            .as_ref()
            .map(|binding| binding.to_gpui(action))
            .transpose()?;

        let mut changes = Vec::with_capacity(2);
        if let Some(previous) = self.bindings.remove(&action_type) {
            changes.push(previous.unbind());
        }

        if let (Some(binding), Some(gpui_binding)) = (binding, gpui_binding) {
            self.bindings.insert(
                action_type,
                RegisteredBinding {
                    action_name,
                    binding,
                },
            );
            changes.push(gpui_binding);
        }

        cx.bind_keys(changes);

        Ok(())
    }

    fn check_conflict(&self, id: &str, binding: &Binding) -> Result<(), KeybindingError> {
        for other in self.bindings.values() {
            if other.action_name == id {
                continue;
            }

            let contexts_overlap = binding.context.is_none()
                || other.binding.context.is_none()
                || binding.context == other.binding.context;
            let keys = &binding.keystrokes;
            let other_keys = &other.binding.keystrokes;

            if contexts_overlap
                && (keys == other_keys
                    || keys.starts_with(&format!("{other_keys} "))
                    || other_keys.starts_with(&format!("{keys} ")))
            {
                let label = self
                    .commands
                    .get(other.action_name)
                    .map(|command| command.label)
                    .unwrap_or_else(|| {
                        other
                            .action_name
                            .rsplit("::")
                            .next()
                            .unwrap_or(other.action_name)
                    });

                return Err(KeybindingError::Conflict(label));
            }
        }

        Ok(())
    }

    fn save(&self, overrides: &BTreeMap<String, Option<String>>) -> Result<(), KeybindingError> {
        if let Some(error) = &self.storage_error {
            return Err(KeybindingError::Storage(format!(
                "{error}. Fix the key bindings file and restart to save changes."
            )));
        }

        let Some(path) = &self.storage_path else {
            return Ok(());
        };

        storage::save(path, overrides).map_err(|error| {
            KeybindingError::Storage(format!("Could not save key bindings: {error}"))
        })
    }
}

#[derive(Debug, Error)]
pub enum KeybindingError {
    #[error("This command is no longer available.")]
    UnknownCommand,

    #[error("This shortcut is already used by {0}. Choose another combination.")]
    Conflict(&'static str),

    #[error("{0}")]
    Storage(String),

    #[error("a keybinding must contain at least one keystroke")]
    EmptyKeystrokes,

    #[error(transparent)]
    InvalidKeystroke(#[from] InvalidKeystrokeError),

    #[error("invalid keybinding context {context:?}: {source}")]
    InvalidContext {
        context: String,
        #[source]
        source: anyhow::Error,
    },
}
