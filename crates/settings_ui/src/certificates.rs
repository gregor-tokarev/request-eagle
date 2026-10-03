use std::path::{Path, PathBuf};

use gpui_kit::component::{
    button::*,
    input::{Input, InputEvent, InputState},
    radio::RadioGroup,
    *,
};
use gpui_kit::{prelude::*, *};
use preferences::{CertificateFiles, ClientCertificate, Preferences};

use crate::layout::{self, row, section};

/// Certificate authorities to trust, and client certificates for mutual TLS.
pub(crate) struct CertificateSettings {
    ca_certificates: Entity<InputState>,
    form: Option<CertificateForm>,
    error: Option<String>,
    _subscriptions: Vec<Subscription>,
}

/// A client certificate being added.
struct CertificateForm {
    host: Entity<InputState>,
    pkcs12: bool,
    certificate: Entity<InputState>,
    key: Entity<InputState>,
    pkcs12_file: Entity<InputState>,
    passphrase: Entity<InputState>,
    /// Its files are being checked and it is being saved.
    adding: bool,
    error: Option<String>,
}

/// An input that a file chooser can fill.
#[derive(Clone, Copy)]
enum FileInput {
    CaCertificates,
    Certificate,
    Key,
    Pkcs12,
}

impl CertificateSettings {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let path = cx.global::<Preferences>().request.ca_certificates.clone();
        let ca_certificates = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("PEM file")
                .default_value(path.as_deref().map(display).unwrap_or_default())
        });
        let subscriptions = vec![
            cx.subscribe(&ca_certificates, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.save_ca_certificates(cx);
                }
            }),
            // Passphrases arrive from the credential store after startup.
            cx.observe_global::<Preferences>(|_, cx| cx.notify()),
        ];

        Self {
            ca_certificates,
            form: None,
            error: None,
            _subscriptions: subscriptions,
        }
    }

    fn save_ca_certificates(&mut self, cx: &mut Context<Self>) {
        let path = self.ca_certificates.read(cx).value().trim().to_owned();

        self.error = preferences::update(cx, |preferences| {
            preferences.request.ca_certificates = (!path.is_empty()).then(|| PathBuf::from(path));
        })
        .err()
        .map(|error| format!("Could not save CA certificates: {error}"));

        cx.notify();
    }

    fn input(&self, input: FileInput) -> Option<&Entity<InputState>> {
        let form = self.form.as_ref();

        match input {
            FileInput::CaCertificates => Some(&self.ca_certificates),
            FileInput::Certificate => form.map(|form| &form.certificate),
            FileInput::Key => form.map(|form| &form.key),
            FileInput::Pkcs12 => form.map(|form| &form.pkcs12_file),
        }
    }

    fn choose_file(&mut self, input: FileInput, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose".into()),
        });

        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };

            let _ = this.update_in(cx, |this, window, cx| {
                if let Some(field) = this.input(input) {
                    field.update(cx, |field, cx| field.set_value(display(&path), window, cx));
                }

                // Setting a value does not report a change.
                if matches!(input, FileInput::CaCertificates) {
                    this.save_ca_certificates(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn open_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = |placeholder: &'static str, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| InputState::new(window, cx).placeholder(placeholder))
        };
        let host = input("api.example.com or *.example.com:8443", window, cx);
        host.update(cx, |host, cx| host.focus(window, cx));

        self.form = Some(CertificateForm {
            host,
            pkcs12: false,
            certificate: input("PEM file", window, cx),
            key: input("PEM file", window, cx),
            pkcs12_file: input(".p12 or .pfx file", window, cx),
            passphrase: cx.new(|cx| InputState::new(window, cx).masked(true)),
            adding: false,
            error: None,
        });
        cx.notify();
    }

    fn add(&mut self, cx: &mut Context<Self>) {
        let Some(form) = &mut self.form else {
            return;
        };
        let path = |input: &Entity<InputState>| PathBuf::from(input.read(cx).value().trim());
        let key = path(&form.key);
        let files = if form.pkcs12 {
            CertificateFiles::Pkcs12 {
                path: path(&form.pkcs12_file),
            }
        } else {
            CertificateFiles::Pem {
                certificate: path(&form.certificate),
                key: (!key.as_os_str().is_empty()).then_some(key),
            }
        };
        let certificate = ClientCertificate {
            id: String::new(),
            host: form.host.read(cx).value().trim().to_owned(),
            files,
            has_passphrase: false,
            passphrase: form.passphrase.read(cx).value().to_string(),
            passphrase_unavailable: false,
        };

        form.adding = true;
        form.error = None;
        // The form can be cancelled and another opened while this one saves.
        let submitted = form.host.entity_id();
        let save = preferences::add_client_certificate(certificate, cx);

        cx.spawn(async move |this, cx| {
            let result = save.await;

            let _ = this.update(cx, |this, cx| {
                if this
                    .form
                    .as_ref()
                    .is_none_or(|form| form.host.entity_id() != submitted)
                {
                    return;
                }

                match result {
                    Ok(()) => this.form = None,
                    Err(error) => {
                        if let Some(form) = &mut this.form {
                            form.adding = false;
                            form.error = Some(format!("Could not add the certificate: {error}"));
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn remove(&mut self, id: &str, cx: &mut Context<Self>) {
        self.error = preferences::remove_client_certificate(id, cx)
            .err()
            .map(|error| format!("Could not remove the certificate: {error}"));

        cx.notify();
    }

    /// A file path input with a button that chooses the file.
    fn file_field(
        &self,
        label: &'static str,
        hint: Option<&'static str>,
        input: FileInput,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let field = self.input(input).unwrap();

        v_flex()
            .gap_2()
            .child(label)
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(Input::new(field).aria_label(label).w_full()),
                    )
                    .child(
                        Button::new(SharedString::from(format!("choose-{label}")))
                            .outline()
                            .label("Choose…")
                            .accessibility_label(format!("Choose {label}"))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.choose_file(input, window, cx)
                            })),
                    ),
            )
            .when_some(hint, |this, hint| {
                this.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(hint),
                )
            })
    }

    fn certificate_form(
        &self,
        form: &CertificateForm,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let pkcs12 = form.pkcs12;

        v_flex()
            .debug_selector(|| "client-certificate-form".into())
            .w_full()
            .py_4()
            .gap_4()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(div().font_weight(FontWeight::MEDIUM).child("Add a client certificate"))
            .child(
                v_flex()
                    .gap_2()
                    .child("Host")
                    .child(Input::new(&form.host).aria_label("Host").w_full())
                    .child(
                        div().text_sm().text_color(cx.theme().muted_foreground).child(
                            "Without a port, the certificate is sent on any port. *.example.com matches its subdomains.",
                        ),
                    ),
            )
            .child(
                RadioGroup::horizontal("client-certificate-format")
                    .children(["PEM files", "PKCS #12 file"])
                    .selected_index(Some(usize::from(pkcs12)))
                    .on_click(cx.listener(|this, index: &usize, _, cx| {
                        if let Some(form) = &mut this.form {
                            form.pkcs12 = *index == 1;
                        }
                        cx.notify();
                    })),
            )
            .map(|this| {
                if pkcs12 {
                    this.child(self.file_field("PKCS #12 file", None, FileInput::Pkcs12, cx))
                } else {
                    this.child(self.file_field("Certificate", None, FileInput::Certificate, cx))
                        .child(self.file_field(
                            "Private key",
                            Some("Leave empty when the certificate file includes the key."),
                            FileInput::Key,
                            cx,
                        ))
                }
            })
            .child(
                v_flex()
                    .gap_2()
                    .child("Passphrase")
                    .child(
                        Input::new(&form.passphrase)
                            .aria_label("Passphrase")
                            .mask_toggle()
                            .w_full(),
                    )
                    .child(
                        div().text_sm().text_color(cx.theme().muted_foreground).child(
                            if cfg!(target_os = "linux") {
                                "Only for an encrypted key or PKCS #12 file. It is saved in your desktop keyring."
                            } else if cfg!(windows) {
                                "Only for an encrypted key or PKCS #12 file. It is saved in Windows Credential Manager."
                            } else {
                                "Only for an encrypted key or PKCS #12 file. It is saved in macOS Keychain."
                            },
                        ),
                    ),
            )
            .when_some(form.error.clone(), |this, error| {
                this.child(div().text_color(cx.theme().danger).child(error))
            })
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("save-client-certificate")
                            .primary()
                            .label(if form.adding { "Checking…" } else { "Add certificate" })
                            .disabled(form.adding)
                            .on_click(cx.listener(|this, _, _, cx| this.add(cx))),
                    )
                    .child(
                        Button::new("cancel-client-certificate")
                            .ghost()
                            .label("Cancel")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.form = None;
                                cx.notify();
                            })),
                    ),
            )
    }
}

fn display(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

impl Render for CertificateSettings {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let certificates = cx
            .global::<Preferences>()
            .request
            .client_certificates
            .clone();

        v_flex()
            .w_full()
            .max_w(layout::PAGE_WIDTH)
            .gap_6()
            .child(
                v_flex()
                    .gap_3()
                    .child(
                        div()
                            .text_xl()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Certificates"),
                    )
                    .child(div().text_color(cx.theme().muted_foreground).child(
                        "Trust private certificate authorities and identify yourself to servers that require a client certificate. Changes apply to new requests.",
                    )),
            )
            .child(
                section("Certificate authorities").child(
                    v_flex()
                        .w_full()
                        .py_4()
                        .gap_2()
                        .border_t_1()
                        .border_color(cx.theme().border)
                        .child(div().font_weight(FontWeight::MEDIUM).child("Custom CA certificates"))
                        .child(div().text_color(cx.theme().muted_foreground).child(
                            "Trust servers whose certificates are signed by the authorities in this PEM file, in addition to the ones your system trusts.",
                        ))
                        .child(
                            h_flex()
                                .debug_selector(|| "ca-certificates".into())
                                .gap_2()
                                .child(
                                    div().flex_1().min_w_0().child(
                                        Input::new(&self.ca_certificates)
                                            .aria_label("CA certificates file")
                                            .cleanable(true)
                                            .w_full(),
                                    ),
                                )
                                .child(
                                    Button::new("choose-ca-certificates")
                                        .outline()
                                        .label("Choose…")
                                        .accessibility_label("Choose CA certificates file")
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.choose_file(FileInput::CaCertificates, window, cx)
                                        })),
                                ),
                        ),
                ),
            )
            .child(
                section("Client certificates")
                    .child(
                        div()
                            .pb_4()
                            .text_color(cx.theme().muted_foreground)
                            .child("Sent to servers that ask for a certificate (mutual TLS). Each certificate is sent only to its host."),
                    )
                    .children(certificates.into_iter().enumerate().map(|(index, certificate)| {
                        let files = match &certificate.files {
                            CertificateFiles::Pem { certificate, key } => {
                                let mut files = vec![format!("Certificate: {}", display(certificate))];
                                files.extend(key.as_deref().map(|key| format!("Key: {}", display(key))));
                                files
                            }
                            CertificateFiles::Pkcs12 { path } => {
                                vec![format!("PKCS #12: {}", display(path))]
                            }
                        };
                        let id = certificate.id.clone();

                        row(
                            certificate.host.clone(),
                            v_flex()
                                .children(files)
                                .when(certificate.passphrase_unavailable, |this| {
                                    this.child(div().text_color(cx.theme().danger).child(
                                        "The passphrase is unavailable. Unlock your keyring, then restart Request Eagle.",
                                    ))
                                }),
                            Button::new(("remove-client-certificate", index))
                                .ghost()
                                .label("Remove")
                                .accessibility_label(format!("Remove the certificate for {}", certificate.host))
                                .on_click(cx.listener(move |this, _, _, cx| this.remove(&id, cx))),
                            cx,
                        )
                        .debug_selector(move || format!("client-certificate-{index}"))
                    }))
                    .map(|this| match &self.form {
                        Some(form) => this.child(self.certificate_form(form, cx)),
                        None => this.child(
                            h_flex().pt_4().border_t_1().border_color(cx.theme().border).child(
                                Button::new("add-client-certificate")
                                    .outline()
                                    .icon(IconName::Plus)
                                    .label("Add certificate")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.open_form(window, cx)
                                    })),
                            ),
                        ),
                    }),
            )
            .when_some(self.error.clone(), |this, error| {
                this.child(div().text_color(cx.theme().danger).child(error))
            })
    }
}
