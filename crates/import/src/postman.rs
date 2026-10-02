//! Postman Collection v2.0 and v2.1.

use std::collections::HashMap;

use collection::{ImportedCollection, ImportedItem};
use request::{
    ApiKeyAuth, Auth, AuthLocation, AwsSignatureAuth, BearerAuth, Body, Field, FormPart,
    HttpRequest, HttpSettings, JwtAlgorithm, JwtAuth, Method, OAuth1Auth, OAuth1Signature,
    OAuth2Auth, OAuth2ClientAuthentication, OAuth2Grant, PasswordAuth, RawLanguage, Request,
    RequestScripts,
};
use serde_json::Value;

use crate::{
    CollectionImport, ImportError,
    body::set_content_type,
    document::{clean_name, text},
};

pub(crate) fn convert(document: &Value) -> Result<CollectionImport, ImportError> {
    let mut skipped = Vec::new();
    // The collection's scripts and authorization become its own, which its
    // requests run and inherit.
    let inherited = Inherited {
        auth: None,
        scripts: Scripts::default(),
        behavior: Behavior::default().then(document),
    };
    let items = items(document, &inherited, &mut skipped);
    let scripts = scripts(document);

    Ok(CollectionImport {
        collection: ImportedCollection {
            name: clean_name(document["info"]["name"].as_str(), "Postman Collection"),
            variables: variables(document),
            scripts: RequestScripts {
                pre_request: join_scripts(&scripts.pre_request),
                post_response: join_scripts(&scripts.post_response),
            },
            auth: own_auth(document).map_or(Auth::Inherit, auth),
            items,
        },
        skipped,
    })
}

/// What a folder passes to the items inside it.
pub(crate) struct Inherited<'a> {
    /// The authorization of the nearest folder that has one. Request Eagle
    /// has no folder authorization, so its requests take it as their own.
    pub(crate) auth: Option<&'a Value>,
    /// Request Eagle has no folder scripts, so each request runs its folders'
    /// scripts before its own, in the order Postman runs them.
    pub(crate) scripts: Scripts,
    pub(crate) behavior: Behavior,
}

/// The parts of Postman's protocol profile behavior that request settings
/// keep. Collections and folders set it for the requests inside them, and
/// the closest explicit value wins.
#[derive(Clone, Copy, Default)]
pub(crate) struct Behavior {
    follow_redirects: Option<bool>,
    verify_certificates: Option<bool>,
}

impl Behavior {
    /// This behavior with the item's own values applied over it.
    pub(crate) fn then(self, item: &Value) -> Self {
        let own = &item["protocolProfileBehavior"];

        Self {
            follow_redirects: own["followRedirects"].as_bool().or(self.follow_redirects),
            verify_certificates: own["strictSSL"].as_bool().or(self.verify_certificates),
        }
    }
}

/// Scripts in the order Postman runs them, each kept separate until they are
/// joined for a request.
#[derive(Clone, Default)]
pub(crate) struct Scripts {
    pub(crate) pre_request: Vec<String>,
    pub(crate) post_response: Vec<String>,
}

impl Scripts {
    pub(crate) fn then(&self, next: Scripts) -> Scripts {
        Scripts {
            pre_request: [self.pre_request.clone(), next.pre_request].concat(),
            post_response: [self.post_response.clone(), next.post_response].concat(),
        }
    }
}

fn items(parent: &Value, inherited: &Inherited, skipped: &mut Vec<String>) -> Vec<ImportedItem> {
    let Some(children) = parent.get("item").and_then(Value::as_array) else {
        return Vec::new();
    };

    children
        .iter()
        .filter_map(|item| {
            let name = clean_name(item["name"].as_str(), "Untitled");

            if item.get("item").is_some() {
                let inherited = Inherited {
                    auth: own_auth(item).or(inherited.auth),
                    scripts: inherited.scripts.then(scripts(item)),
                    behavior: inherited.behavior.then(item),
                };

                return Some(ImportedItem::Folder {
                    items: items(item, &inherited, skipped),
                    name,
                });
            }

            match request(item, inherited) {
                Some(request) => Some(ImportedItem::Request {
                    name,
                    request: Request::Http(request),
                }),
                None => {
                    skipped.push(name);
                    None
                }
            }
        })
        .collect()
}

/// The request, or `None` when Request Eagle cannot send its method.
pub(crate) fn request(item: &Value, inherited: &Inherited) -> Option<HttpRequest> {
    let request = &item["request"];
    let method = match request["method"]
        .as_str()
        .unwrap_or("GET")
        .to_uppercase()
        .as_str()
    {
        "GET" => Method::Get,
        "POST" => Method::Post,
        "PUT" => Method::Put,
        "PATCH" => Method::Patch,
        "HEAD" => Method::Head,
        "OPTIONS" => Method::Options,
        "DELETE" => Method::Delete,
        _ => return None,
    };

    // A request may be written as just its URL.
    let (path, path_variables) = match request {
        Value::String(url) => (url.clone(), Vec::new()),
        _ => url(&request["url"]),
    };

    let mut headers = fields(&request["header"]);
    // The URL holds the parameters that are sent; those switched off stay
    // beside it.
    let query: Vec<_> = fields(&request["url"]["query"])
        .into_iter()
        .filter(|field| !field.enabled)
        .collect();
    let scripts = inherited.scripts.then(scripts(item));
    let body = body(&request["body"], &mut headers);

    // The request's Settings tab in Postman.
    let behavior = inherited.behavior.then(item);

    Some(HttpRequest {
        method,
        path,
        headers,
        body,
        query,
        path_variables,
        auth: request_auth(own_auth(request), inherited),
        scripts: RequestScripts {
            pre_request: join_scripts(&scripts.pre_request),
            post_response: join_scripts(&scripts.post_response),
        },
        settings: HttpSettings {
            timeout_ms: None,
            follow_redirects: behavior.follow_redirects,
            verify_certificates: behavior.verify_certificates,
        },
    })
}

/// The URL as Postman shows it, and the values of its `:name` path
/// variables. Its query stays in the URL as written, so encoded values are
/// sent unchanged.
fn url(url: &Value) -> (String, Vec<(String, String)>) {
    let raw = match url {
        Value::String(raw) => return (raw.clone(), Vec::new()),
        Value::Object(_) => match url["raw"].as_str() {
            Some(raw) => raw.to_owned(),
            None => assemble_url(url),
        },
        _ => return (String::new(), Vec::new()),
    };

    let names: Vec<&str> = request::path_variables(&raw)
        .map(|(_, name)| name)
        .collect();
    let variables = pairs(&url["variable"])
        .into_iter()
        .filter(|(key, value)| !value.is_empty() && names.contains(&key.as_str()))
        .collect();

    (raw, variables)
}

/// A URL from the parts Postman stores beside, or instead of, the raw URL.
fn assemble_url(url: &Value) -> String {
    let join = |parts: &Value, separator: &str| match parts {
        Value::Array(parts) => parts
            .iter()
            .map(|part| text(Some(part.get("value").unwrap_or(part))))
            .collect::<Vec<_>>()
            .join(separator),
        part => text(Some(part)),
    };

    let mut assembled = String::new();
    if let Some(protocol) = url["protocol"].as_str() {
        assembled.push_str(&format!("{protocol}://"));
    }
    assembled.push_str(&join(&url["host"], "."));
    if !url["port"].is_null() {
        assembled.push_str(&format!(":{}", text(Some(&url["port"]))));
    }

    let path = join(&url["path"], "/");
    if !path.is_empty() && !path.starts_with('/') {
        assembled.push('/');
    }
    assembled.push_str(&path);

    let query = pairs(&url["query"]);
    if !query.is_empty() {
        let query: Vec<_> = query
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect();
        assembled.push_str(&format!("?{}", query.join("&")));
    }

    assembled
}

fn body(body: &Value, headers: &mut Vec<Field>) -> Option<Body> {
    if body["disabled"].as_bool() == Some(true) {
        return None;
    }

    match body["mode"].as_str()? {
        "raw" => {
            let text = body["raw"]
                .as_str()
                .filter(|raw| !raw.is_empty())?
                .to_owned();
            // Other languages are sent as text of their own type.
            let language = match body["options"]["raw"]["language"].as_str() {
                Some("json") => RawLanguage::Json,
                Some("xml") => RawLanguage::Xml,
                Some("html") => {
                    set_content_type(headers, "text/html");
                    RawLanguage::Text
                }
                Some("javascript") => {
                    set_content_type(headers, "application/javascript");
                    RawLanguage::Text
                }
                _ => RawLanguage::Text,
            };

            Some(Body::Raw { language, text })
        }
        "urlencoded" => {
            let fields = pairs(&body["urlencoded"]);
            (!fields.is_empty()).then_some(Body::UrlEncoded { fields })
        }
        "formdata" => {
            let mut parts = Vec::new();
            let fields = body["formdata"].as_array().into_iter().flatten();
            for field in fields.filter(|field| field["disabled"].as_bool() != Some(true)) {
                let Some(name) = field["key"].as_str() else {
                    continue;
                };

                if field["type"].as_str() != Some("file") {
                    parts.push(FormPart {
                        name: name.to_owned(),
                        value: text(field.get("value")),
                        file: false,
                    });
                    continue;
                }

                // A field can send several files, or none chosen yet.
                let files = match &field["src"] {
                    Value::Array(files) => files.iter().filter_map(Value::as_str).collect(),
                    Value::String(file) => vec![file.as_str()],
                    _ => Vec::new(),
                };
                for file in if files.is_empty() { vec![""] } else { files } {
                    parts.push(FormPart {
                        name: name.to_owned(),
                        value: file.to_owned(),
                        file: true,
                    });
                }
            }

            (!parts.is_empty()).then_some(Body::Multipart { parts })
        }
        "file" => {
            let file = body["file"]["src"]
                .as_str()
                .filter(|file| !file.is_empty())?;
            Some(Body::Binary { file: file.into() })
        }
        "graphql" => {
            let graphql = &body["graphql"];
            let query = serde_json::to_string(&text(graphql.get("query"))).ok()?;
            // Variables are JSON once their `{{variables}}` are filled in, so
            // they are kept as written.
            let variables = match &graphql["variables"] {
                Value::String(variables) => variables.trim().to_owned(),
                Value::Null => String::new(),
                variables => serde_json::to_string_pretty(variables).ok()?,
            };

            Some(Body::json(if variables.is_empty() {
                format!("{{\n  \"query\": {query}\n}}")
            } else {
                format!("{{\n  \"query\": {query},\n  \"variables\": {variables}\n}}")
            }))
        }
        _ => None,
    }
}

/// The item's own authorization, which `noauth` sets to none. Without one,
/// the item inherits its parent's.
pub(crate) fn own_auth(item: &Value) -> Option<&Value> {
    item.get("auth")
        .filter(|auth| !auth.is_null() && auth["type"].as_str() != Some("inherit"))
}

/// The authorization a request sends: its own, or else its folder's. Without
/// either, it inherits the collection's.
pub(crate) fn request_auth(own: Option<&Value>, inherited: &Inherited) -> Auth {
    own.or(inherited.auth).map_or(Auth::Inherit, auth)
}

/// A Postman authorization as Request Eagle's. Kinds it does not have, such
/// as NTLM and Hawk, send nothing.
pub(crate) fn auth(auth: &Value) -> Auth {
    let kind = auth["type"].as_str().unwrap_or_default();
    let value = |key: &str| auth_value(auth, kind, key);
    let flag = |key: &str| value(key) == "true";
    let password = || PasswordAuth {
        username: value("username"),
        password: value("password"),
    };
    // Postman's own defaults, for parameters an export leaves out.
    let or = |key: &str, default: &str| match value(key) {
        value if value.is_empty() => default.to_owned(),
        value => value,
    };

    match kind {
        "apikey" => Auth::ApiKey(ApiKeyAuth {
            key: value("key"),
            value: value("value"),
            add_to: if value("in") == "query" {
                AuthLocation::Query
            } else {
                AuthLocation::Header
            },
        }),
        "bearer" => Auth::Bearer(BearerAuth {
            token: value("token"),
        }),
        "basic" => Auth::Basic(password()),
        "digest" => Auth::Digest(password()),
        "oauth1" => Auth::OAuth1(OAuth1Auth {
            signature_method: OAuth1Signature::ALL
                .into_iter()
                .find(|method| method.label() == value("signatureMethod"))
                .unwrap_or_default(),
            consumer_key: value("consumerKey"),
            consumer_secret: value("consumerSecret"),
            access_token: value("token"),
            token_secret: value("tokenSecret"),
            private_key: value("privateKey"),
            callback_url: value("callback"),
            verifier: value("verifier"),
            realm: value("realm"),
            add_to: if value("addParamsToHeader") == "false" {
                AuthLocation::Query
            } else {
                AuthLocation::Header
            },
        }),
        "oauth2" => {
            let grant = value("grant_type");
            let callback = value("redirect_uri");

            Auth::OAuth2(OAuth2Auth {
                access_token: value("accessToken"),
                header_prefix: or("headerPrefix", "Bearer"),
                add_to: if value("addTokenTo") == "queryParams" {
                    AuthLocation::Query
                } else {
                    AuthLocation::Header
                },
                grant_type: match grant.as_str() {
                    "client_credentials" => OAuth2Grant::ClientCredentials,
                    "password_credentials" => OAuth2Grant::Password,
                    _ => OAuth2Grant::AuthorizationCode,
                },
                auth_url: value("authUrl"),
                token_url: value("accessTokenUrl"),
                // Postman's own callback page returns to Postman, not here.
                callback_url: if callback.is_empty() || callback.contains("oauth.pstmn.io") {
                    OAuth2Auth::default().callback_url
                } else {
                    callback
                },
                client_id: value("clientId"),
                client_secret: value("clientSecret"),
                scope: value("scope"),
                username: value("username"),
                password: value("password"),
                pkce: grant == "authorization_code_with_pkce",
                client_authentication: if value("client_authentication") == "body" {
                    OAuth2ClientAuthentication::Body
                } else {
                    OAuth2ClientAuthentication::Header
                },
            })
        }
        "jwt" => Auth::Jwt(JwtAuth {
            algorithm: JwtAlgorithm::ALL
                .into_iter()
                .find(|algorithm| algorithm.label() == value("algorithm"))
                .unwrap_or_default(),
            secret: value("secret"),
            secret_base64: flag("isSecretBase64Encoded"),
            private_key: value("privateKey"),
            payload: or("payload", "{}"),
            headers: match value("header").trim() {
                "{}" => String::new(),
                headers => headers.to_owned(),
            },
            add_to: if value("addTokenTo") == "queryParam" {
                AuthLocation::Query
            } else {
                AuthLocation::Header
            },
            header_prefix: or("headerPrefix", "Bearer"),
            query_param: or("queryParamKey", "token"),
        }),
        "awsv4" => Auth::AwsSignature(AwsSignatureAuth {
            access_key: value("accessKey"),
            secret_key: value("secretKey"),
            session_token: value("sessionToken"),
            region: value("region"),
            service: value("service"),
            add_to: if flag("addAuthDataToQuery") {
                AuthLocation::Query
            } else {
                AuthLocation::Header
            },
        }),
        _ => Auth::None,
    }
}

/// An authorization parameter: a key–value list in v2.1, an object in v2.0.
fn auth_value(auth: &Value, kind: &str, key: &str) -> String {
    match &auth[kind] {
        Value::Array(parameters) => text(
            parameters
                .iter()
                .find(|parameter| parameter["key"].as_str() == Some(key))
                .and_then(|parameter| parameter.get("value")),
        ),
        parameters => text(parameters.get(key)),
    }
}

/// Header or metadata rows, including those switched off, with their
/// descriptions.
pub(crate) fn fields(fields: &Value) -> Vec<Field> {
    fields
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|field| {
            let description = &field["description"];

            Some(Field {
                key: field["key"].as_str()?.to_owned(),
                value: text(field.get("value")),
                enabled: field["disabled"].as_bool() != Some(true),
                // A description is text, or an object with its text as content.
                description: text(description.get("content").or(Some(description))),
            })
        })
        .collect()
}

/// Enabled key–value pairs, such as form fields.
pub(crate) fn pairs(pairs: &Value) -> Vec<(String, String)> {
    pairs
        .as_array()
        .into_iter()
        .flatten()
        .filter(|pair| pair["disabled"].as_bool() != Some(true))
        .filter_map(|pair| Some((pair["key"].as_str()?.to_owned(), text(pair.get("value")))))
        .collect()
}

fn variables(document: &Value) -> HashMap<String, String> {
    pairs(&document["variable"]).into_iter().collect()
}

/// The item's own scripts.
pub(crate) fn scripts(item: &Value) -> Scripts {
    let mut scripts = Scripts::default();

    for event in item["event"].as_array().into_iter().flatten() {
        if event["disabled"].as_bool() == Some(true) {
            continue;
        }

        let source = match &event["script"]["exec"] {
            Value::Array(lines) => lines
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join("\n"),
            Value::String(source) => source.clone(),
            _ => continue,
        };
        if source.trim().is_empty() {
            continue;
        }

        match event["listen"].as_str() {
            Some("prerequest") => scripts.pre_request.push(source),
            Some("test") => scripts.post_response.push(source),
            _ => {}
        }
    }

    scripts
}

/// Joins scripts that Postman runs one after another. Each keeps its own
/// block, so declarations with the same name in two of them do not collide.
pub(crate) fn join_scripts(scripts: &[String]) -> String {
    match scripts {
        [script] => script.clone(),
        scripts => scripts
            .iter()
            .map(|script| format!("{{\n{script}\n}}"))
            .collect::<Vec<_>>()
            .join("\n\n"),
    }
}
