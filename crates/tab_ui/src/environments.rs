use std::path::PathBuf;

use environment::GlobalEnvironments;
use gpui_kit::*;

const NEW_ENVIRONMENT: &str = "New Environment";

pub enum EnvironmentsEvent {
    Renamed {
        from: SharedString,
        to: SharedString,
    },
    Deleted(SharedString),
}

/// The workspace's global environments and the one whose variables requests
/// use. Every tab, the sidebar, and the environment picker share this entity.
pub struct Environments {
    catalog: GlobalEnvironments,
    names: Vec<SharedString>,
    active: Option<SharedString>,
    error: Option<String>,
}

impl EventEmitter<EnvironmentsEvent> for Environments {}

impl Environments {
    pub fn new(catalog: GlobalEnvironments, active: Option<String>) -> Self {
        let mut environments = Self {
            catalog,
            names: Vec::new(),
            active: None,
            error: None,
        };
        environments.reload();
        environments.active = active
            .map(SharedString::from)
            .filter(|name| environments.names.contains(name));

        environments
    }

    pub fn names(&self) -> &[SharedString] {
        &self.names
    }

    pub fn active(&self) -> Option<&SharedString> {
        self.active.as_ref()
    }

    /// The most recent failure to list, create, rename, or delete an environment.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.catalog.path(name)
    }

    /// The directory of environment files.
    pub fn catalog(&self) -> &GlobalEnvironments {
        &self.catalog
    }

    /// Lists the environments again after files were added to the directory.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.reload();
        cx.notify();
    }

    pub(crate) fn active_path(&self) -> Option<PathBuf> {
        self.active.as_deref().map(|name| self.catalog.path(name))
    }

    pub fn set_active(&mut self, name: Option<SharedString>, cx: &mut Context<Self>) {
        if self.active == name {
            return;
        }

        self.active = name;
        self.persist_active(cx);
        cx.notify();
    }

    pub fn create(&mut self, cx: &mut Context<Self>) -> Option<SharedString> {
        let result = self.catalog.create(NEW_ENVIRONMENT);
        self.reload();

        let name = match result {
            Ok(name) => Some(SharedString::from(name)),
            Err(error) => {
                self.error = Some(format!("Could not create environment: {error}"));
                None
            }
        };

        cx.notify();
        name
    }

    pub fn rename(
        &mut self,
        from: &SharedString,
        to: &str,
        cx: &mut Context<Self>,
    ) -> Result<SharedString, String> {
        let to: SharedString = self
            .catalog
            .rename(from, to)
            .map_err(|error| error.to_string())?
            .into();
        self.reload();

        if self.active.as_ref() == Some(from) {
            self.active = Some(to.clone());
            self.persist_active(cx);
        }

        if &to != from {
            cx.emit(EnvironmentsEvent::Renamed {
                from: from.clone(),
                to: to.clone(),
            });
        }

        cx.notify();
        Ok(to)
    }

    pub fn delete(&mut self, name: &SharedString, cx: &mut Context<Self>) {
        let result = self.catalog.delete(name);
        self.reload();

        match result {
            Ok(()) => {
                if self.active.as_ref() == Some(name) {
                    self.active = None;
                    self.persist_active(cx);
                }

                cx.emit(EnvironmentsEvent::Deleted(name.clone()));
            }
            Err(error) => self.error = Some(format!("Could not delete environment: {error}")),
        }

        cx.notify();
    }

    fn reload(&mut self) {
        match self.catalog.names() {
            Ok(names) => {
                self.names = names.into_iter().map(SharedString::from).collect();
                self.error = None;
            }
            Err(error) => self.error = Some(format!("Could not read environments: {error}")),
        }
    }

    fn persist_active(&mut self, cx: &mut Context<Self>) {
        let active = self.active.as_ref().map(ToString::to_string);

        if let Err(error) = preferences::update(cx, |preferences| {
            preferences.active_environment = active;
        }) {
            self.error = Some(format!(
                "Could not remember the active environment: {error}"
            ));
        }
    }
}
