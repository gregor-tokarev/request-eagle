use request::{Authentication, FormBody, HttpRequest, Method, MultipartField};

use super::parser::{ImportedRequest, add_content_type, header, method, upload_path};

pub(super) fn parse(input: &str) -> Result<ImportedRequest, String> {
    let words = super::shell::words(input)?;

    if words.first().is_none_or(|word| word != "curl") {
        return Err(
            "Paste a command beginning with curl, or a Postman collection JSON file.".into(),
        );
    }

    let mut request = HttpRequest::default();
    let mut explicit_method = None;
    let mut body_parts = Vec::new();
    let mut form_fields = Vec::new();
    let mut json_body = false;
    let mut plain_body = false;
    let mut compressed = false;
    let mut get = false;
    let mut head = false;
    let mut positional = false;
    let mut user_agent = None;
    let mut referer = None;
    let mut index = 1;

    while index < words.len() {
        let word = &words[index];
        index += 1;

        if positional || !word.starts_with('-') {
            set_url(&mut request, word)?;
            continue;
        }

        if word == "--" {
            positional = true;
            continue;
        }

        let (option, inline) = split_option(word);
        let takes_value = matches!(
            option,
            "-X" | "--request"
                | "-H"
                | "--header"
                | "-d"
                | "--data"
                | "--data-raw"
                | "--data-binary"
                | "--data-ascii"
                | "--data-urlencode"
                | "--json"
                | "--url"
                | "-u"
                | "--user"
                | "-A"
                | "--user-agent"
                | "-e"
                | "--referer"
                | "-b"
                | "--cookie"
                | "--oauth2-bearer"
                | "-F"
                | "--form"
                | "--form-string"
        );

        let value = if takes_value {
            match inline {
                Some(value) => value,
                None => {
                    let value = words
                        .get(index)
                        .ok_or_else(|| format!("cURL option {option} needs a value."))?;
                    index += 1;
                    value
                }
            }
        } else {
            if inline.is_some() {
                return Err(format!("Unsupported cURL option {word}."));
            }

            ""
        };

        match option {
            "-X" | "--request" => explicit_method = Some(method(value)?),
            "--url" => set_url(&mut request, value)?,
            "-H" | "--header" => {
                if value.starts_with('@') {
                    return Err(
                        "Import cannot read a cURL header file. Paste the headers instead.".into(),
                    );
                }

                if let Some(name) = value.strip_suffix(';')
                    && !name.contains(':')
                    && !name.is_empty()
                {
                    request.headers.push((name.into(), String::new()));
                } else {
                    let pair = header(value)?;

                    if pair.1.is_empty() {
                        return Err("cURL header removal is not supported. Use 'Name;' to import an empty header.".into());
                    }

                    request.headers.push(pair);
                }
            }
            "-d" | "--data" | "--data-binary" | "--data-ascii" | "--json" => {
                reject_file(value)?;
                body_parts.push(value.to_owned());
                json_body |= option == "--json";
                plain_body |= option != "--json";
            }
            "--data-raw" => {
                body_parts.push(value.to_owned());
                plain_body = true;
            }
            "--data-urlencode" => {
                body_parts.push(encode_data(value)?);
                plain_body = true;
            }
            "-F" | "--form" | "--form-string" => {
                form_fields.push(form_field(value, option == "--form-string")?);
            }
            "-u" | "--user" => {
                let (username, password) = value.split_once(':').ok_or(
                    "cURL --user needs username:password. Interactive password prompts cannot be imported.",
                )?;
                request.authentication = Authentication::Basic {
                    username: username.into(),
                    password: password.into(),
                };
            }
            "--oauth2-bearer" => {
                request.authentication = Authentication::Bearer {
                    token: value.into(),
                };
            }
            "-A" | "--user-agent" => user_agent = Some(value.to_owned()),
            "-e" | "--referer" => {
                if value.ends_with(";auto") {
                    return Err("cURL's automatic Referer option cannot be imported.".into());
                }

                referer = Some(value.to_owned());
            }
            "-b" | "--cookie" => {
                if !value.contains('=') {
                    return Err(
                        "Import cannot read a cURL cookie file. Paste a Cookie header instead."
                            .into(),
                    );
                }

                request.headers.push(("Cookie".into(), value.into()));
            }
            "-G" | "--get" => get = true,
            "-I" | "--head" => head = true,
            // These only control terminal output, which import does not reproduce.
            "-s"
            | "--silent"
            | "-S"
            | "--show-error"
            | "-sS"
            | "-Ss"
            | "-v"
            | "--verbose"
            | "--no-progress-meter"
            | "-i"
            | "--include"
            | "-f"
            | "--fail"
            | "--fail-with-body"
            | "--globoff"
            | "-g" => {}
            "--compressed" => compressed = true,
            _ => {
                return Err(format!(
                    "Unsupported cURL option {option}. Remove it or configure that setting in the app."
                ));
            }
        }
    }

    if request.path.is_empty() {
        return Err("The cURL command has no URL.".into());
    }

    if json_body && plain_body {
        return Err(
            "Import cannot mix cURL --json with other data flags. Combine the body in one flag."
                .into(),
        );
    }

    if !form_fields.is_empty() && (!body_parts.is_empty() || head || get) {
        return Err(
            "cURL multipart fields cannot be mixed with raw data, --head, or --get.".into(),
        );
    }

    for (name, value) in [("User-Agent", user_agent), ("Referer", referer)] {
        if let Some(value) = value
            && !request
                .headers
                .iter()
                .any(|(key, _)| key.eq_ignore_ascii_case(name))
        {
            if value.is_empty() {
                return Err(format!("cURL removal of {name} cannot be imported."));
            }

            request.headers.push((name.into(), value));
        }
    }

    if compressed
        && !request
            .headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("accept-encoding"))
    {
        request
            .headers
            .push(("Accept-Encoding".into(), "gzip".into()));
    }

    if head && !body_parts.is_empty() && !get {
        return Err("cURL --head cannot be combined with request body data.".into());
    }

    request.method = explicit_method.unwrap_or(if head {
        Method::Head
    } else if get || (body_parts.is_empty() && form_fields.is_empty()) {
        Method::Get
    } else {
        Method::Post
    });

    if !form_fields.is_empty() {
        request.form = Some(FormBody::Multipart(form_fields));
    } else if !body_parts.is_empty() {
        let data = body_parts.join(if json_body { "" } else { "&" });

        if get {
            let (path, fragment) = request.path.split_once('#').unwrap_or((&request.path, ""));
            let separator = if path.contains('?') { '&' } else { '?' };
            request.path = format!(
                "{path}{separator}{data}{}",
                if fragment.is_empty() {
                    String::new()
                } else {
                    format!("#{fragment}")
                }
            );
        } else {
            request.body = Some(data.into_bytes());
            add_content_type(
                &mut request,
                if json_body {
                    "application/json"
                } else {
                    "application/x-www-form-urlencoded"
                },
            );
        }

        if json_body {
            add_content_type(&mut request, "application/json");
        }

        if json_body
            && !request
                .headers
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case("accept"))
        {
            request
                .headers
                .push(("Accept".into(), "application/json".into()));
        }
    }

    Ok(ImportedRequest {
        name: format!("{} {}", request.method.as_str(), request.path),
        folders: Vec::new(),
        request,
    })
}

fn split_option(word: &str) -> (&str, Option<&str>) {
    if word.starts_with("--") {
        word.split_once('=')
            .map_or((word, None), |(key, value)| (key, Some(value)))
    } else if word.len() > 2
        && matches!(
            word.get(..2),
            Some("-X" | "-H" | "-d" | "-u" | "-A" | "-e" | "-b" | "-F")
        )
    {
        (&word[..2], Some(&word[2..]))
    } else {
        (word, None)
    }
}

fn form_field(value: &str, literal: bool) -> Result<MultipartField, String> {
    let (name, value) = value
        .split_once('=')
        .ok_or("A cURL multipart field needs name=value.")?;

    if !literal && value.contains(';') {
        return Err("Custom cURL multipart attributes are not supported. Use --form-string for literal semicolons.".into());
    }

    if !literal && let Some(path) = value.strip_prefix('@') {
        if path.contains([',', '"']) {
            return Err("Import cURL uploads using one absolute file path per --form flag.".into());
        }

        return Ok(MultipartField::File {
            name: name.into(),
            path: upload_path(path)?,
        });
    }

    if !literal && value.starts_with('<') {
        return Err("cURL file-backed text fields cannot be imported. Paste the field value with --form-string.".into());
    }

    Ok(MultipartField::Text {
        name: name.into(),
        value: value.into(),
    })
}

fn set_url(request: &mut HttpRequest, value: &str) -> Result<(), String> {
    if !request.path.is_empty() {
        return Err("Import one cURL URL at a time.".into());
    }

    if value.is_empty() {
        return Err("The cURL URL is empty.".into());
    }

    request.path = value.into();
    Ok(())
}

fn reject_file(value: &str) -> Result<(), String> {
    if value.starts_with('@') {
        Err("Import cannot read cURL data files. Paste the body with --data-raw instead.".into())
    } else {
        Ok(())
    }
}

fn encode_data(value: &str) -> Result<String, String> {
    if let Some((name, value)) = value.split_once('=') {
        let encoded = url::form_urlencoded::byte_serialize(value.as_bytes()).collect::<String>();

        return Ok(if name.is_empty() {
            encoded
        } else {
            format!("{name}={encoded}")
        });
    }

    if value.contains('@') {
        return Err(
            "Import cannot read cURL --data-urlencode files. Paste name=value instead.".into(),
        );
    }

    Ok(url::form_urlencoded::byte_serialize(value.as_bytes()).collect())
}
