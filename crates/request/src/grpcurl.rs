//! gRPC calls written as grpcurl commands, in the form of the cURL code
//! snippets: one option per line, with the server and method last.

use std::collections::HashMap;
use std::path::Path;

use environment::VariableResolver;

use crate::curl::{keep_unknown, quote};
use crate::grpc::Target;
use crate::{Field, GrpcDefinition, GrpcRequest};

impl GrpcRequest {
    /// The grpcurl command that makes this call as Request Eagle does.
    /// `{{variables}}` that `values` defines are filled in as invoking fills
    /// them; others, and generated ones such as `{{$guid}}`, stay as written.
    /// Relative `.proto` paths resolve from `collection`. Without a method,
    /// the command lists the server's services.
    pub fn grpcurl_command(
        &self,
        values: &HashMap<String, String>,
        collection: Option<&Path>,
    ) -> String {
        // A reference without its closing braces cannot be filled in, so the
        // text is written as it is.
        let fill = |text: &str| {
            VariableResolver::new(values)
                .resolve(&keep_unknown(text, values))
                .unwrap_or_else(|_| text.to_owned())
        };
        let request = GrpcRequest {
            url: fill(&self.url),
            ..self.clone()
        };
        let settings = &request.settings;

        // Without a port, invoking connects to 443. Unfilled variables could
        // hold the port, so such an address stays as written.
        let target = Target::parse(&request.url, request.tls)
            .ok()
            .filter(|_| !request.url.contains("{{"));
        let tls = target
            .as_ref()
            .map_or_else(|| request.uses_tls(), |target| target.tls);
        let address = match &target {
            Some(target) => format!("{}:{}", target.host, target.port),
            None => {
                let url = request.url.trim();
                let authority = url.split_once("://").map_or(url, |(_, rest)| rest);
                authority.strip_suffix('/').unwrap_or(authority).to_owned()
            }
        };

        let mut command = String::from("grpcurl");
        if !tls {
            command.push_str(" -plaintext");
        } else if settings.verify_certificates == Some(false) {
            command.push_str(" -insecure");
        }
        // Request Eagle shows the fields that have their default value
        // unless the request's settings leave them out.
        if settings.include_default_fields {
            command.push_str(" -emit-defaults");
        }

        let mut options = Vec::new();
        let server_name = settings.server_name.trim();
        if tls && settings.verify_certificates != Some(false) && !server_name.is_empty() {
            options.push(("-servername", server_name.to_owned()));
        }
        if let Some(megabytes) = settings.max_response_message_mb {
            // A message's length prefix cannot exceed `u32::MAX`, so that size
            // stands for no limit, which grpcurl has no setting for.
            let largest = u64::from(u32::MAX);
            let bytes = match megabytes {
                0 => largest,
                megabytes => megabytes.saturating_mul(1024 * 1024).min(largest),
            };
            options.push(("-max-msg-sz", bytes.to_string()));
        }

        let definition = match collection {
            Some(collection) => request.definition.resolved_from(collection),
            None => request.definition.clone(),
        };
        if let GrpcDefinition::ProtoFile { path, import_paths } = definition {
            // Imports resolve from the import paths, then from the file's
            // directory, and the first of them that contains the file names it.
            let mut includes = import_paths;
            if let Some(directory) = path.parent()
                && !directory.as_os_str().is_empty()
                && !includes.iter().any(|include| include == directory)
            {
                includes.push(directory.to_path_buf());
            }
            let name = includes
                .iter()
                .find_map(|include| path.strip_prefix(include).ok())
                .unwrap_or(&path);

            for include in &includes {
                options.push(("-import-path", include.display().to_string()));
            }
            options.push(("-proto", name.display().to_string()));
        }

        let metadata: Vec<Field> = Field::enabled(&request.metadata)
            .map(|(name, value)| Field::new(fill(name), fill(value)))
            .collect();
        // The authorization's credentials, as invoking adds them. One that
        // cannot be made, such as a JWT without its key, is left out.
        let mut auth = request.auth.sending();
        auth.resolve_with(|text| Ok::<_, ()>(fill(text))).ok();
        let credentials = crate::grpc::auth_metadata(&metadata, &auth).unwrap_or_default();
        let metadata = Field::pairs(&metadata);

        for (name, value) in metadata.iter().chain(&credentials) {
            let name = name.trim();
            if name.is_empty() {
                continue;
            }

            options.push(("-H", format!("{name}: {value}").trim_end().to_owned()));
        }

        // Invoking finds the method with or without a leading slash.
        let method = request.method.trim().trim_start_matches('/');
        let message = fill(&request.message);
        // Without data, grpcurl sends an empty message, as invoking does.
        if !method.is_empty() && !message.trim().is_empty() {
            options.push(("-d", message));
        }

        for (option, value) in &options {
            command.push_str(" \\\n");
            command.push_str(option);
            command.push(' ');
            command.push_str(&shell_word(value));
        }

        command.push_str(if options.is_empty() { " " } else { " \\\n" });
        // grpcurl would read an address that starts with `-` as an option.
        if address.starts_with('-') {
            command.push_str("-- ");
        }
        command.push_str(&shell_word(&address));
        command.push(' ');
        if method.is_empty() {
            command.push_str("list");
        } else {
            command.push_str(&shell_word(method));
        }

        command
    }
}

/// Text as one word for a POSIX shell, quoted unless it needs no quotes.
fn shell_word(text: &str) -> String {
    let plain = !text.is_empty()
        && text
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || "-_.,:/@%+=".contains(ch));

    if plain { text.to_owned() } else { quote(text) }
}
