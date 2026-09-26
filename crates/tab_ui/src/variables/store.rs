use std::{collections::HashMap, path::PathBuf};

use environment::{Environment, VariableValues};
use gpui_kit::*;

pub(crate) struct VariableScope {
    pub path: Option<PathBuf>,
}

#[derive(Default)]
pub(crate) struct VariableStore {
    pub environments: HashMap<Option<PathBuf>, HashMap<String, String>>,
    pub environment_errors: HashMap<Option<PathBuf>, String>,
    environment_targets: HashMap<Option<PathBuf>, PathBuf>,
    pub secrets: HashMap<String, String>,
    pub secret_error: Option<String>,
    pub loading: bool,
    pub saving: bool,
    pub save_error: Option<String>,
    load_task: Option<Task<()>>,
    save_task: Option<Task<()>>,
}

struct SharedVariables(Entity<VariableStore>);
impl Global for SharedVariables {}

impl VariableStore {
    pub fn global(cx: &mut App) -> Entity<Self> {
        if let Some(store) = cx.try_global::<SharedVariables>() {
            return store.0.clone();
        }

        let store = cx.new(|_| Self::default());
        cx.set_global(SharedVariables(store.clone()));
        store
    }

    pub fn environment_path(scope: &Option<PathBuf>) -> Option<PathBuf> {
        scope.clone().or_else(|| {
            std::env::home_dir().map(|home| home.join(".request-eagle/environment.toml"))
        })
    }

    pub fn ensure_environment(&mut self, scope: &Option<PathBuf>, cx: &mut Context<Self>) {
        if self.environments.contains_key(scope) || self.environment_errors.contains_key(scope) {
            return;
        }

        let path = Self::environment_path(scope);
        if let Some(target) = path.as_ref().and_then(|path| path.canonicalize().ok()) {
            self.environment_targets.insert(scope.clone(), target);
        }
        let result = path.map(Environment::from_file);
        match result {
            Some(Ok(environment)) => {
                self.environments.insert(scope.clone(), environment.entries);
            }
            Some(Err(environment::EnvironmentLoadError::Read { source, .. }))
                if source.kind() == std::io::ErrorKind::NotFound =>
            {
                self.environments.insert(scope.clone(), HashMap::new());
            }
            Some(Err(error)) => {
                self.environment_errors
                    .insert(scope.clone(), error.to_string());
            }
            None => {
                self.environment_errors.insert(
                    scope.clone(),
                    "Could not locate the environment file.".into(),
                );
            }
        }
        cx.notify();
    }

    pub fn values(&self, scope: &Option<PathBuf>) -> Result<VariableValues, String> {
        if self.saving {
            return Err("Variables are still saving. Try sending again in a moment.".into());
        }

        Ok(VariableValues {
            environment: self.environments.get(scope).cloned().unwrap_or_default(),
            secrets: self.secrets.clone(),
        })
    }

    pub fn reload_environment(&mut self, scope: &Option<PathBuf>, cx: &mut Context<Self>) {
        self.save_error = None;
        let target = Self::environment_path(scope)
            .and_then(|path| path.canonicalize().ok())
            .or_else(|| self.environment_targets.get(scope).cloned());
        let mut scopes = vec![scope.clone()];
        if let Some(target) = target {
            for alias in self
                .environments
                .keys()
                .chain(self.environment_errors.keys())
            {
                if !scopes.contains(alias)
                    && Self::environment_path(alias)
                        .and_then(|path| path.canonicalize().ok())
                        .or_else(|| self.environment_targets.get(alias).cloned())
                        .is_some_and(|path| path == target)
                {
                    scopes.push(alias.clone());
                }
            }
        }
        for scope in scopes {
            self.environments.remove(&scope);
            self.environment_errors.remove(&scope);
            self.ensure_environment(&scope, cx);
        }
    }

    pub fn load_secrets(&mut self, cx: &mut Context<Self>) {
        if self.loading || self.saving {
            return;
        }

        self.loading = true;
        self.save_error = None;
        let task = preferences::read_request_secrets(cx);
        self.load_task = Some(cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.loading = false;
                match result {
                    Ok(values) => {
                        this.secrets = values;
                        this.secret_error = None;
                    }
                    Err(error) => {
                        this.secret_error = Some(error.to_string());
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub fn save_entry(
        &mut self,
        scope: Option<PathBuf>,
        secret: bool,
        previous_name: Option<String>,
        name: String,
        value: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if self.saving || (secret && (self.loading || self.secret_error.is_some())) {
            return;
        }
        self.saving = true;
        self.save_error = None;
        let task = if secret {
            preferences::update_request_secret(previous_name, name, value, cx)
        } else {
            let path = Self::environment_path(&scope);
            cx.background_executor().spawn(async move {
                let path =
                    path.ok_or_else(|| anyhow::anyhow!("Could not locate the environment file."))?;
                super::environment_file::save_entry(
                    &path,
                    previous_name.as_deref(),
                    &name,
                    value.as_deref(),
                )
            })
        };
        self.save_task = Some(cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.saving = false;
                match result {
                    Ok(values) if secret => {
                        this.secrets = values;
                    }
                    Ok(_) => this.reload_environment(&scope, cx),
                    Err(error) => {
                        if secret {
                            this.secret_error = Some(error.to_string());
                        } else {
                            this.reload_environment(&scope, cx);
                        }
                        this.save_error = Some(error.to_string());
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
}

pub fn load_variables(cx: &mut App) {
    VariableStore::global(cx).update(cx, |store, cx| store.load_secrets(cx));
}
