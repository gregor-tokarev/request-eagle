use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui_kit::component::{
    button::*,
    input::{Input, InputEvent, InputState},
    spinner::Spinner,
    *,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use request::{Field, GrpcDefinition, GrpcError, ServiceDefinition};

use super::draft::GrpcDraft;
use super::methods::method_list;

/// Typing a server URL loads its methods once typing pauses.
const REFLECTION_DELAY: Duration = Duration::from_millis(700);

pub(crate) enum DefinitionState {
    /// Nothing is loaded for the current settings yet.
    Idle,
    Loading,
    Loaded(ServiceDefinition),
    Failed(GrpcError),
}

/// The request settings a definition was loaded from. Changing them makes
/// the loaded definition stale.
#[derive(Clone, PartialEq, Eq)]
pub(crate) enum DefinitionSource {
    Reflection {
        url: String,
        tls: bool,
        metadata: Vec<(String, String)>,
        verify_certificates: Option<bool>,
        server_name: String,
    },
    ProtoFile(GrpcDefinition),
}

impl GrpcDraft {
    /// None until there is a URL for reflection or a `.proto` path.
    pub(super) fn current_source(&self) -> Option<DefinitionSource> {
        match &self.request.definition {
            GrpcDefinition::Reflection if !self.request.url.trim().is_empty() => {
                Some(DefinitionSource::Reflection {
                    url: self.request.url.trim().to_owned(),
                    tls: self.request.uses_tls(),
                    metadata: Field::enabled(&self.request.metadata)
                        .map(|(key, value)| (key.to_owned(), value.to_owned()))
                        .collect(),
                    verify_certificates: self.request.settings.verify_certificates,
                    server_name: self.request.settings.server_name.clone(),
                })
            }
            GrpcDefinition::ProtoFile { path, .. } if !path.as_os_str().is_empty() => {
                Some(DefinitionSource::ProtoFile(self.request.definition.clone()))
            }
            _ => None,
        }
    }

    /// Whether the loaded definition matches the current settings.
    pub(super) fn definition_is_current(&self) -> bool {
        matches!(self.definition, DefinitionState::Loaded(_))
            && self.definition_source.is_some()
            && self.definition_source == self.current_source()
    }

    /// Reload server reflection after the URL, TLS, metadata or certificate
    /// settings change.
    pub(super) fn schedule_reflection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.request.definition.is_reflection()
            || (self.definition_source.is_some() && self.definition_source == self.current_source())
        {
            return;
        }

        self.definition = DefinitionState::Idle;
        self.definition_source = None;

        if self.current_source().is_none() {
            self.drop_pending_invoke(cx);
        }

        self.definition_task = self.current_source().map(|_| {
            cx.spawn_in(window, async move |this, cx| {
                cx.background_executor().timer(REFLECTION_DELAY).await;
                let _ = this.update_in(cx, |this, window, cx| {
                    this.definition_task = None;
                    this.load_definition(false, window, cx);
                    this.redraw(cx);
                });
            })
        });
        self.refresh_methods(window, cx);
        self.redraw(cx);
    }

    /// Load the services from the server or `.proto` file. Unless forced, a
    /// definition already loaded or loading for these settings is kept.
    pub(super) fn load_definition(
        &mut self,
        force: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(source) = self.current_source() else {
            self.drop_pending_invoke(cx);

            if !matches!(self.definition, DefinitionState::Idle) || self.definition_task.is_some() {
                self.definition = DefinitionState::Idle;
                self.definition_source = None;
                self.definition_task = None;
                self.refresh_methods(window, cx);
                self.redraw(cx);
            }
            return;
        };

        if !force
            && self.definition_source.as_ref() == Some(&source)
            && !matches!(self.definition, DefinitionState::Idle)
        {
            return;
        }

        let client = self.client(cx);
        let variables = self.variables.read(cx).request_variables(cx);
        let collection = self.collection_path();
        let load = client.load_definition(&self.request, &variables, collection.as_deref());
        self.reflected_target = reflected_target(&self.request, &variables);
        let task = cx.background_executor().spawn(load);

        self.definition = DefinitionState::Loading;
        self.definition_source = Some(source);
        self.definition_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.definition_task = None;
                this.definition = match result {
                    Ok(definition) => DefinitionState::Loaded(definition),
                    Err(error) => DefinitionState::Failed(error),
                };
                this.refresh_methods(window, cx);

                if std::mem::take(&mut this.invoke_when_loaded) {
                    match &this.definition {
                        DefinitionState::Loaded(_) => this.invoke(window, cx),
                        DefinitionState::Failed(error) => {
                            let message = format!("Could not load the service definition: {error}");
                            this.response
                                .update(cx, |response, cx| response.fail(message.into(), cx));
                        }
                        _ => {}
                    }
                }

                this.redraw(cx);
            });
        }));
        self.refresh_methods(window, cx);
        self.redraw(cx);
    }

    /// Keep a definition an invoke loaded for the call its Before invoke
    /// script prepared, so the method picker shows its methods. `source` is
    /// the settings it loaded from; it is dropped if they changed since.
    pub(super) fn keep_definition(
        &mut self,
        definition: ServiceDefinition,
        source: Option<DefinitionSource>,
        target: Option<Vec<String>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if source != self.current_source() {
            return;
        }

        self.definition = DefinitionState::Loaded(definition);
        self.definition_source = source;
        // It replaces a load for the draft's settings that is still running.
        self.definition_task = None;
        self.reflected_target = target;
        self.refresh_methods(window, cx);
        self.redraw(cx);
    }

    /// Forget an Invoke waiting for a definition that will no longer load,
    /// such as after the URL was cleared.
    fn drop_pending_invoke(&mut self, cx: &mut Context<Self>) {
        if std::mem::take(&mut self.invoke_when_loaded) {
            self.response
                .update(cx, |response, cx| response.cancel(false, cx));
        }
    }

    /// Show the loaded methods in the picker and select the request's method.
    pub(super) fn refresh_methods(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(methods) = self.methods.clone() else {
            return;
        };
        let services = match &self.definition {
            DefinitionState::Loaded(definition) => definition.services(),
            _ => Vec::new(),
        };
        let selected = self.request.method.trim().to_owned();

        methods.update(cx, |methods, cx| {
            methods.set_items(method_list(&services), window, cx);
            methods.set_selected_value(&selected, window, cx);
            cx.notify();
        });
    }

    /// Replace the message with an example of the method's input.
    pub(super) fn use_example_message(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let DefinitionState::Loaded(definition) = &self.definition else {
            return;
        };
        let Some(example) = definition.example_message(self.request.method.trim()) else {
            return;
        };
        let message = self.message_state(window, cx);

        message.update(cx, |message, cx| message.replace_all(example, window, cx));
        cx.notify();
    }

    /// Create the `.proto` path inputs when a proto file is the source.
    pub(super) fn definition_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let GrpcDefinition::ProtoFile { path, import_paths } = self.request.definition.clone()
        else {
            return;
        };

        if self.proto_path.is_none() {
            let input = cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("Path to a .proto file")
                    .default_value(path.to_string_lossy().into_owned())
            });
            self._subscriptions.push(cx.subscribe(
                &input,
                |this, input, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change)
                        && let GrpcDefinition::ProtoFile { path, .. } = &mut this.request.definition
                    {
                        *path = PathBuf::from(input.read(cx).value().trim());
                        cx.notify();
                    }
                },
            ));
            self.proto_path = Some(input);
        }

        while self.import_paths.len() < import_paths.len() {
            let path = &import_paths[self.import_paths.len()];
            self.push_import_path(path, window, cx);
        }
    }

    fn push_import_path(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Directory that imports resolve from")
                .default_value(path.to_string_lossy().into_owned())
        });
        self._subscriptions
            .push(cx.subscribe(&input, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.sync_import_paths(cx);
                }
            }));
        self.import_paths.push(input);
    }

    fn sync_import_paths(&mut self, cx: &mut Context<Self>) {
        let paths = self
            .import_paths
            .iter()
            .map(|input| PathBuf::from(input.read(cx).value().trim()))
            .collect();

        if let GrpcDefinition::ProtoFile { import_paths, .. } = &mut self.request.definition {
            *import_paths = paths;
        }

        self.redraw(cx);
    }

    /// Replace the definition's paths, such as with the relative paths a save
    /// stored. The loaded services stay current when they came from the
    /// same files.
    pub fn set_definition(
        &mut self,
        definition: GrpcDefinition,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.request.definition == definition {
            return;
        }

        let current = self.definition_is_current();
        self.request.definition = definition;
        self.proto_path = None;
        self.import_paths.clear();
        self.definition_inputs(window, cx);

        if current {
            self.definition_source = self.current_source();
        }

        self.redraw(cx);
    }

    fn use_reflection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.request.definition = GrpcDefinition::Reflection;
        self.proto_path = None;
        self.import_paths.clear();
        self.load_definition(false, window, cx);
    }

    fn use_proto_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.request.definition = GrpcDefinition::ProtoFile {
            path: PathBuf::new(),
            import_paths: Vec::new(),
        };
        self.definition_inputs(window, cx);
        self.load_definition(false, window, cx);
    }

    /// Store paths inside the collection relative to it, so the collection
    /// keeps working when it is shared or moved.
    fn stored_path(&self, path: PathBuf) -> PathBuf {
        self.collection_path()
            .and_then(|collection| path.strip_prefix(collection).ok().map(Path::to_path_buf))
            .unwrap_or(path)
    }

    fn choose_proto_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Import".into()),
        });

        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };

            let _ = this.update_in(cx, |this, window, cx| {
                let path = this.stored_path(path);

                if let Some(input) = &this.proto_path {
                    input.update(cx, |input, cx| {
                        input.set_value(path.to_string_lossy().into_owned(), window, cx)
                    });
                }
                if let GrpcDefinition::ProtoFile { path: stored, .. } = &mut this.request.definition
                {
                    *stored = path;
                }

                this.load_definition(true, window, cx);
                this.redraw(cx);
            });
        })
        .detach();
    }

    fn choose_import_path(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Add".into()),
        });

        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };

            let _ = this.update_in(cx, |this, window, cx| {
                let path = this.stored_path(path);

                if let Some(input) = this.import_paths.get(index) {
                    input.update(cx, |input, cx| {
                        input.set_value(path.to_string_lossy().into_owned(), window, cx)
                    });
                }
                this.sync_import_paths(cx);
                this.redraw(cx);
            });
        })
        .detach();
    }

    fn add_import_path(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.push_import_path(Path::new(""), window, cx);
        self.sync_import_paths(cx);

        let index = self.import_paths.len() - 1;
        self.import_paths[index].update(cx, |input, cx| input.focus(window, cx));
    }

    fn remove_import_path(&mut self, index: usize, cx: &mut Context<Self>) {
        self.import_paths.remove(index);
        self.sync_import_paths(cx);
    }

    pub(super) fn definition_tab(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.definition_inputs(window, cx);

        let content = if self.request.definition.is_reflection() {
            self.reflection_panel(cx).into_any_element()
        } else {
            self.proto_file_panel(cx).into_any_element()
        };

        v_flex()
            .debug_selector(|| "grpc-definition".into())
            .w_full()
            .max_w(rems(40.))
            .py_2()
            .gap_4()
            .child(
                div().text_color(cx.theme().muted_foreground).child(
                    "A service definition makes the client aware of the services and methods.",
                ),
            )
            .child(content)
            .into_any_element()
    }

    fn reflection_panel(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        v_flex()
            .gap_4()
            .child(self.definition_status(true, cx))
            .child(
                h_flex()
                    .gap_2()
                    .text_color(cx.theme().muted_foreground)
                    .child("OR")
                    .child(div().flex_1().h_px().bg(cx.theme().border)),
            )
            .child(
                v_flex()
                    .gap_2()
                    .items_start()
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Icon::default()
                                    .path("icons/file-code.svg")
                                    .text_color(cx.theme().muted_foreground),
                            )
                            .child("Load a .proto file from your computer."),
                    )
                    .child(
                        Button::new("grpc-import-proto")
                            .debug_selector(|| "grpc-import-proto".into())
                            .outline()
                            .label("Import .proto file")
                            .on_click(
                                cx.listener(|this, _, window, cx| this.use_proto_file(window, cx)),
                            ),
                    ),
            )
    }

    fn proto_file_panel(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let proto_path = self.proto_path.clone().unwrap();
        let has_path = matches!(
            &self.request.definition,
            GrpcDefinition::ProtoFile { path, .. } if !path.as_os_str().is_empty()
        );

        v_flex()
            .gap_4()
            .child(
                h_flex().child(
                    Button::new("grpc-use-reflection")
                        .debug_selector(|| "grpc-use-reflection".into())
                        .ghost()
                        .icon(IconName::ArrowLeft)
                        .label("Use server reflection")
                        .on_click(
                            cx.listener(|this, _, window, cx| this.use_reflection(window, cx)),
                        ),
                ),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("Import a .proto file"))
                    .child(
                        div()
                            .text_color(cx.theme().muted_foreground)
                            .child("Choose a .proto file from your computer. Paths inside the collection are saved relative to it."),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                div()
                                    .debug_selector(|| "grpc-proto-path".into())
                                    .flex_1()
                                    .min_w_0()
                                    .child(Input::new(&proto_path).aria_label(".proto file")),
                            )
                            .child(
                                Button::new("grpc-choose-proto")
                                    .debug_selector(|| "grpc-choose-proto".into())
                                    .outline()
                                    .label("Choose a File")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.choose_proto_file(window, cx)
                                    })),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("Import paths"))
                    .child(div().text_color(cx.theme().muted_foreground).child(
                        "Specify import paths to look for .proto files while resolving \"import\" directives. The file's own folder is searched last.",
                    ))
                    .children(self.import_paths.iter().enumerate().map(|(index, input)| {
                        h_flex()
                            .debug_selector(move || format!("grpc-import-path-{index}"))
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(Input::new(input).aria_label(format!("Import path {}", index + 1))),
                            )
                            .child(
                                Button::new(("grpc-choose-import-path", index))
                                    .ghost()
                                    .icon(IconName::FolderOpen)
                                    .accessibility_label("Choose a folder")
                                    .tooltip("Choose a folder")
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.choose_import_path(index, window, cx)
                                    })),
                            )
                            .child(
                                Button::new(("grpc-remove-import-path", index))
                                    .debug_selector(move || format!("grpc-remove-import-path-{index}"))
                                    .ghost()
                                    .icon(IconName::Close)
                                    .accessibility_label("Remove import path")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.remove_import_path(index, cx)
                                    })),
                            )
                    }))
                    .child(
                        h_flex().child(
                            Button::new("grpc-add-import-path")
                                .debug_selector(|| "grpc-add-import-path".into())
                                .ghost()
                                .icon(IconName::Plus)
                                .label("Add an import path")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.add_import_path(window, cx)
                                })),
                        ),
                    ),
            )
            .child(
                h_flex().child(
                    Button::new("grpc-load-proto")
                        .debug_selector(|| "grpc-load-proto".into())
                        .primary()
                        .label("Import")
                        .disabled(!has_path)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.load_definition(true, window, cx)
                        })),
                ),
            )
            .when(has_path, |panel| panel.child(self.definition_status(false, cx)))
    }

    /// Whether the definition is loading, loaded or failed, with a retry.
    fn definition_status(&self, reflection: bool, cx: &mut Context<Self>) -> AnyElement {
        let source = if reflection {
            "server reflection".to_owned()
        } else {
            match &self.request.definition {
                GrpcDefinition::ProtoFile { path, .. } => path.file_name().map_or_else(
                    || path.display().to_string(),
                    |name| name.to_string_lossy().into_owned(),
                ),
                GrpcDefinition::Reflection => String::new(),
            }
        };
        let stale =
            !self.definition_is_current() && matches!(self.definition, DefinitionState::Loaded(_));
        // Reflection waits for typing to pause before it loads.
        let loading = match self.definition {
            DefinitionState::Loading => true,
            DefinitionState::Idle => self.definition_task.is_some(),
            _ => false,
        };
        let retry = Button::new("grpc-reload-definition")
            .debug_selector(|| "grpc-reload-definition".into())
            .ghost()
            .xsmall()
            .icon(Icon::default().path("icons/refresh-cw.svg"))
            .accessibility_label("Load the service definition again")
            .tooltip("Load again")
            .on_click(cx.listener(|this, _, window, cx| this.load_definition(true, window, cx)));

        let (icon, title, detail): (AnyElement, SharedString, Option<SharedString>) =
            match &self.definition {
                _ if loading => (
                    Spinner::new()
                        .color(cx.theme().muted_foreground)
                        .into_any_element(),
                    format!("Loading {source}…").into(),
                    None,
                ),
                DefinitionState::Idle if reflection => (
                    Icon::new(IconName::Info)
                        .size_4()
                        .text_color(cx.theme().muted_foreground)
                        .into_any_element(),
                    "Using server reflection".into(),
                    Some("Enter the server URL to load its services.".into()),
                ),
                DefinitionState::Idle | DefinitionState::Loading => (
                    Icon::new(IconName::Info)
                        .size_4()
                        .text_color(cx.theme().muted_foreground)
                        .into_any_element(),
                    format!("{source} is not imported yet").into(),
                    None,
                ),
                DefinitionState::Loaded(definition) => {
                    let services = definition.services();
                    let methods: usize = services.iter().map(|service| service.methods.len()).sum();

                    (
                    Icon::new(IconName::CircleCheck)
                        .size_4()
                        .text_color(cx.theme().success)
                        .into_any_element(),
                    format!("Using {source}").into(),
                    Some(
                        if stale {
                            "Settings changed since the last load. Invoke or load again to update."
                                .to_owned()
                        } else {
                            format!(
                                "{} {}, {} {}",
                                services.len(),
                                if services.len() == 1 {
                                    "service"
                                } else {
                                    "services"
                                },
                                methods,
                                if methods == 1 { "method" } else { "methods" }
                            )
                        }
                        .into(),
                    ),
                )
                }
                DefinitionState::Failed(error) => (
                    Icon::default()
                        .path("icons/circle-alert.svg")
                        .size_4()
                        .text_color(cx.theme().danger)
                        .into_any_element(),
                    if reflection {
                        "Could not load server reflection".into()
                    } else {
                        format!("Could not import {source}").into()
                    },
                    Some(error.to_string().into()),
                ),
            };
        let failed = matches!(self.definition, DefinitionState::Failed(_));
        let needed_tls = match &self.definition {
            DefinitionState::Failed(GrpcError::TlsRequired) => Some(true),
            DefinitionState::Failed(GrpcError::TlsUnsupported) => Some(false),
            _ => None,
        }
        .filter(|_| lock_decides_tls(&self.request.url));

        // The detail and actions line up with the title, past the 1 rem icon.
        v_flex()
            .debug_selector(|| "grpc-definition-status".into())
            .gap_1()
            .p_3()
            .rounded(cx.theme().radius_tokens().lg)
            .bg(if failed {
                cx.theme().danger.opacity(0.1)
            } else {
                cx.theme().muted
            })
            .child(
                h_flex()
                    .gap_2()
                    .child(icon)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .font_weight(FontWeight::MEDIUM)
                            .child(title),
                    )
                    .when(!loading, |header| header.child(retry)),
            )
            .when_some(detail, |status, detail| {
                status.child(
                    div()
                        .debug_selector(|| "grpc-definition-detail".into())
                        .pl_6()
                        .text_color(if failed {
                            cx.theme().danger
                        } else {
                            cx.theme().muted_foreground
                        })
                        .when(failed, |detail| {
                            detail
                                .font_family(cx.theme().mono_font_family.clone())
                                .text_xs()
                        })
                        .child(detail),
                )
            })
            .when_some(needed_tls, |status, tls| {
                status.child(
                    h_flex().pl_6().pt_1().child(
                        Button::new("grpc-definition-set-tls")
                            .debug_selector(|| "grpc-definition-set-tls".into())
                            .outline()
                            .small()
                            .icon(Icon::default().path(if tls {
                                "icons/lock.svg"
                            } else {
                                "icons/lock-open.svg"
                            }))
                            .label(if tls { "Turn on TLS" } else { "Turn off TLS" })
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.set_tls(tls, window, cx)
                            })),
                    ),
                )
            })
            .into_any_element()
    }
}

/// What a reflection request connects to with these variable values.
pub(super) fn reflected_target(
    request: &request::GrpcRequest,
    variables: &request::RequestVariables,
) -> Option<Vec<String>> {
    if !request.definition.is_reflection() {
        return None;
    }

    variables.grpc_target_key(request)
}

/// Whether the lock decides how the request connects. A URL that starts
/// with a variable may supply its own scheme, which decides TLS instead.
pub(super) fn lock_decides_tls(url: &str) -> bool {
    let url = url.trim();
    // The lock rewrites a scheme written in the URL.
    let written_scheme = url
        .split_once("://")
        .is_some_and(|(scheme, _)| matches!(scheme, "grpc" | "grpcs" | "http" | "https"));

    written_scheme || !url.starts_with("{{")
}
