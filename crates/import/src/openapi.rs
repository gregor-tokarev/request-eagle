//! OpenAPI 3.x and Swagger 2.0 specifications. Each operation becomes a
//! request in a folder named after its first tag.

use std::collections::HashMap;

use collection::{ImportedCollection, ImportedItem};
use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};
use request::{Body, Field, FormPart, HttpRequest, Method, RawLanguage, Request, RequestScripts};
use serde_json::{Map, Value};

use crate::{
    CollectionImport, ImportError,
    body::set_content_type,
    document::{clean_name, text},
};

/// The collection variable that holds the server URL.
const BASE_URL: &str = "base_url";

/// Deeper schemas are left out of generated bodies, which also ends
/// recursive references.
const MAX_SCHEMA_DEPTH: usize = 8;

/// Characters that would end or restructure a path segment.
const PATH_VALUE: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'/')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'`')
    .add(b'{')
    .add(b'}');

static NULL: Value = Value::Null;

pub(crate) fn convert(document: &Value) -> Result<CollectionImport, ImportError> {
    // Unquoted YAML versions are numbers.
    let openapi = text(document.get("openapi"));
    let swagger = text(document.get("swagger"));
    let swagger = if openapi.starts_with("3.") {
        false
    } else if openapi.is_empty() && swagger == "2.0" {
        true
    } else if openapi.is_empty() {
        return Err(ImportError::UnsupportedOpenApi(swagger));
    } else {
        return Err(ImportError::UnsupportedOpenApi(openapi));
    };
    let spec = Spec { document, swagger };

    let mut variables = HashMap::from([(BASE_URL.to_owned(), spec.base_url())]);
    let mut folders: Vec<(String, Vec<ImportedItem>)> = Vec::new();
    let mut requests = Vec::new();
    let mut skipped = Vec::new();

    for (path, path_item) in document["paths"].as_object().into_iter().flatten() {
        let path_item = spec.resolve(path_item);

        for (key, operation) in path_item.as_object().into_iter().flatten() {
            let method = match key.as_str() {
                "get" => Some(Method::Get),
                "put" => Some(Method::Put),
                "post" => Some(Method::Post),
                "delete" => Some(Method::Delete),
                "options" => Some(Method::Options),
                "head" => Some(Method::Head),
                "patch" => Some(Method::Patch),
                "trace" => None,
                _ => continue,
            };
            let name = clean_name(
                operation["summary"]
                    .as_str()
                    .or(operation["operationId"].as_str()),
                &format!("{} {path}", key.to_uppercase()),
            );
            let Some(method) = method else {
                skipped.push(name);
                continue;
            };

            let request = spec.request(method, path, path_item, operation, &mut variables);
            let item = ImportedItem::Request {
                name,
                request: Request::Http(request),
            };

            match operation["tags"][0].as_str() {
                Some(tag) => match folders.iter_mut().find(|(name, _)| name == tag) {
                    Some((_, items)) => items.push(item),
                    None => folders.push((tag.to_owned(), vec![item])),
                },
                None => requests.push(item),
            }
        }
    }

    let items = folders
        .into_iter()
        .map(|(name, items)| ImportedItem::Folder {
            name: clean_name(Some(&name), "Untitled"),
            items,
        })
        .chain(requests)
        .collect();

    Ok(CollectionImport {
        collection: ImportedCollection {
            name: clean_name(document["info"]["title"].as_str(), "OpenAPI"),
            variables,
            scripts: RequestScripts::default(),
            items,
        },
        skipped,
    })
}

struct Spec<'a> {
    document: &'a Value,
    /// Swagger 2.0 describes servers, bodies and security differently.
    swagger: bool,
}

impl<'a> Spec<'a> {
    /// The first server's URL with its variables' defaults, without a
    /// trailing slash, so request paths can follow it.
    fn base_url(&self) -> String {
        let document = self.document;
        let url = if self.swagger {
            let scheme = match document["schemes"].as_array() {
                Some(schemes)
                    if !schemes.is_empty() && !schemes.iter().any(|scheme| scheme == "https") =>
                {
                    text(schemes.first())
                }
                _ => "https".to_owned(),
            };
            let base_path = document["basePath"].as_str().unwrap_or_default();

            match document["host"].as_str() {
                Some(host) => format!("{scheme}://{host}{base_path}"),
                None => base_path.to_owned(),
            }
        } else {
            let server = &document["servers"][0];
            let mut url = text(server.get("url"));
            for (name, variable) in server["variables"].as_object().into_iter().flatten() {
                url = url.replace(&format!("{{{name}}}"), &text(variable.get("default")));
            }

            url
        };

        url.trim_end_matches('/').to_owned()
    }

    fn request(
        &self,
        method: Method,
        path: &str,
        path_item: &'a Value,
        operation: &'a Value,
        variables: &mut HashMap<String, String>,
    ) -> HttpRequest {
        let mut path_values = HashMap::new();
        let mut headers = Vec::new();
        let mut query = Vec::new();
        let mut form = Vec::new();
        let mut body_schema = None;

        for parameter in self.parameters(path_item, operation) {
            let Some(name) = parameter["name"].as_str() else {
                continue;
            };
            let required = parameter["required"].as_bool() == Some(true);
            let value = || {
                self.simple_value(parameter)
                    .unwrap_or_else(|| variable(name))
            };

            match parameter["in"].as_str() {
                Some("path") => {
                    let value = match self.simple_value(parameter) {
                        Some(value) => utf8_percent_encode(&value, PATH_VALUE).to_string(),
                        None => variable(name),
                    };
                    path_values.insert(name, value);
                }
                // Optional parameters change what the server does, so only
                // required ones are sent.
                Some("query") if required => query.extend(
                    self.query_pairs(name, parameter)
                        .into_iter()
                        .map(Field::from),
                ),
                Some("header") if required => headers.push(Field::new(name, value())),
                Some("body") => body_schema = Some(&parameter["schema"]),
                Some("formData") if parameter["type"] != "file" => {
                    form.push((name.to_owned(), value()));
                }
                _ => {}
            }
        }

        self.authorize(operation, &mut headers, &mut query, variables);

        let body = if self.swagger {
            let consumes = operation["consumes"]
                .as_array()
                .or(self.document["consumes"].as_array())
                .and_then(|types| types.first())
                .and_then(Value::as_str);

            if let Some(schema) = body_schema {
                let value = self.sample(schema, 0);
                encode(consumes.unwrap_or("application/json"), &value, &mut headers)
            } else if form.is_empty() {
                None
            } else if consumes.is_some_and(|media_type| media_type.starts_with("multipart/")) {
                Some(multipart(form))
            } else {
                Some(Body::UrlEncoded { fields: form })
            }
        } else {
            self.request_body(operation, &mut headers)
        };

        HttpRequest {
            method,
            path: format!("{{{{{BASE_URL}}}}}{}", fill_path(path, &path_values)),
            headers,
            body,
            query,
            path_variables: Vec::new(),
            scripts: RequestScripts::default(),
            settings: Default::default(),
        }
    }

    /// The path's parameters, replaced by the operation's own where both
    /// describe the same one.
    fn parameters(&self, path_item: &'a Value, operation: &'a Value) -> Vec<&'a Value> {
        let mut parameters: Vec<&Value> = Vec::new();

        for parameter in [path_item, operation]
            .into_iter()
            .flat_map(|item| item["parameters"].as_array().into_iter().flatten())
        {
            let parameter = self.resolve(parameter);
            parameters.retain(|existing| {
                existing["name"] != parameter["name"] || existing["in"] != parameter["in"]
            });
            parameters.push(parameter);
        }

        parameters
    }

    /// A parameter's example or default.
    fn parameter_example(&self, parameter: &'a Value) -> Option<&'a Value> {
        let schema = self.resolve(&parameter["schema"]);

        [
            &parameter["example"],
            &parameter["x-example"],
            &parameter["default"],
            &schema["example"],
            &schema["default"],
        ]
        .into_iter()
        .find(|value| !value.is_null())
    }

    /// A header, path or form parameter's example in the `simple` style that
    /// OpenAPI uses for headers and paths, or in Swagger's `collectionFormat`.
    fn simple_value(&self, parameter: &'a Value) -> Option<String> {
        let value = self.parameter_example(parameter)?;
        let separator = match parameter["collectionFormat"].as_str() {
            Some("ssv") => " ",
            Some("tsv") => "\t",
            Some("pipes") => "|",
            _ => ",",
        };

        Some(join_value(
            value,
            separator,
            parameter["explode"].as_bool() == Some(true),
        ))
    }

    /// A query parameter's pairs in its `style`, OpenAPI's `form` by default,
    /// or in Swagger's `collectionFormat`. An exploded array repeats the
    /// parameter for each item.
    fn query_pairs(&self, name: &str, parameter: &'a Value) -> Vec<(String, String)> {
        let Some(value) = self.parameter_example(parameter) else {
            return vec![(name.to_owned(), variable(name))];
        };
        let style = parameter["style"].as_str().unwrap_or("form");
        let (explode, separator) = if self.swagger {
            match parameter["collectionFormat"].as_str() {
                Some("multi") => (true, ","),
                Some("ssv") => (false, " "),
                Some("tsv") => (false, "\t"),
                Some("pipes") => (false, "|"),
                _ => (false, ","),
            }
        } else {
            let separator = match style {
                "spaceDelimited" => " ",
                "pipeDelimited" => "|",
                _ => ",",
            };
            (
                parameter["explode"].as_bool().unwrap_or(style == "form"),
                separator,
            )
        };

        match value {
            Value::Array(items) if explode => items
                .iter()
                .map(|item| (name.to_owned(), text(Some(item))))
                .collect(),
            Value::Object(properties) if style == "deepObject" => properties
                .iter()
                .map(|(key, value)| (format!("{name}[{key}]"), text(Some(value))))
                .collect(),
            Value::Object(properties) if explode => properties
                .iter()
                .map(|(key, value)| (key.clone(), text(Some(value))))
                .collect(),
            value => vec![(name.to_owned(), join_value(value, separator, false))],
        }
    }

    /// An OpenAPI 3 request body, preferring JSON.
    fn request_body(&self, operation: &'a Value, headers: &mut Vec<Field>) -> Option<Body> {
        let content = self.resolve(&operation["requestBody"])["content"].as_object()?;
        let (media_type, media) = content
            .iter()
            .find(|(media_type, _)| media_type.contains("json"))
            .or_else(|| {
                content.iter().find(|(media_type, _)| {
                    media_type.as_str() == "application/x-www-form-urlencoded"
                })
            })
            .or_else(|| {
                content
                    .iter()
                    .find(|(media_type, _)| media_type.starts_with("multipart/form-data"))
            })
            .or_else(|| content.iter().next())?;

        let example = media["examples"]
            .as_object()
            .and_then(|examples| examples.values().next())
            .map(|example| &self.resolve(example)["value"]);
        let value = match media.get("example").or(example) {
            Some(example) => example.clone(),
            None => self.sample(&media["schema"], 0),
        };

        encode(media_type, &value, headers)
    }

    /// Adds the first security requirement's credentials as variables, which
    /// are added to the collection for the user to fill in.
    fn authorize(
        &self,
        operation: &Value,
        headers: &mut Vec<Field>,
        query: &mut Vec<Field>,
        variables: &mut HashMap<String, String>,
    ) {
        let requirements = operation
            .get("security")
            .unwrap_or(&self.document["security"]);
        let Some(requirement) = requirements[0].as_object() else {
            return;
        };

        for scheme_name in requirement.keys() {
            let scheme = if self.swagger {
                &self.document["securityDefinitions"][scheme_name]
            } else {
                self.resolve(&self.document["components"]["securitySchemes"][scheme_name])
            };
            let credential = format!("{{{{{scheme_name}}}}}");
            let bearer = match scheme["type"].as_str() {
                Some("http") => scheme["scheme"]
                    .as_str()
                    .is_some_and(|scheme| scheme.eq_ignore_ascii_case("bearer")),
                Some("oauth2" | "openIdConnect") => true,
                _ => false,
            };

            if bearer {
                headers.push(Field::new("Authorization", format!("Bearer {credential}")));
            } else if scheme["type"] == "apiKey"
                && let Some(name) = scheme["name"].as_str()
            {
                match scheme["in"].as_str() {
                    Some("header") => headers.push(Field::new(name, credential)),
                    Some("query") => query.push(Field::new(name, credential)),
                    _ => continue,
                }
            } else {
                continue;
            }

            variables.entry(scheme_name.clone()).or_default();
        }
    }

    /// Follows local `$ref`s. References to other files resolve to nothing.
    fn resolve(&self, mut value: &'a Value) -> &'a Value {
        for _ in 0..MAX_SCHEMA_DEPTH {
            let Some(reference) = value["$ref"].as_str() else {
                return value;
            };
            value = reference
                .strip_prefix('#')
                .and_then(|pointer| self.document.pointer(pointer))
                .unwrap_or(&NULL);
        }

        value
    }

    /// An example value for a schema, built from its examples, defaults and
    /// types.
    fn sample(&self, schema: &'a Value, depth: usize) -> Value {
        if depth > MAX_SCHEMA_DEPTH {
            return Value::Null;
        }
        let schema = self.resolve(schema);

        if let Some(example) = schema
            .get("example")
            .or(schema.get("default"))
            .or(schema["examples"].get(0))
            .or(schema["enum"].get(0))
        {
            return example.clone();
        }

        if let Some(schemas) = schema["allOf"].as_array() {
            let mut merged = Map::new();
            for composed in schemas {
                match self.sample(composed, depth + 1) {
                    Value::Object(properties) => merged.extend(properties),
                    value if schemas.len() == 1 && schema.get("properties").is_none() => {
                        return value;
                    }
                    _ => {}
                }
            }
            // Properties beside `allOf` apply as well.
            merged.extend(self.properties_sample(schema, depth));

            return Value::Object(merged);
        }

        if let Some(schema) = schema["oneOf"].get(0).or(schema["anyOf"].get(0)) {
            return self.sample(schema, depth + 1);
        }

        // OpenAPI 3.1 lists types, including "null".
        let kind = match &schema["type"] {
            Value::Array(kinds) => kinds
                .iter()
                .filter_map(Value::as_str)
                .find(|kind| *kind != "null"),
            kind => kind.as_str(),
        };

        match kind {
            Some("object") | None if schema.get("properties").is_some() => {
                Value::Object(self.properties_sample(schema, depth))
            }
            Some("object") => Value::Object(Map::new()),
            Some("array") => Value::Array(vec![self.sample(&schema["items"], depth + 1)]),
            Some("string") => Value::from(match schema["format"].as_str() {
                Some("date-time") => "2024-01-01T00:00:00Z",
                Some("date") => "2024-01-01",
                Some("email") => "user@example.com",
                Some("uuid") => "00000000-0000-0000-0000-000000000000",
                Some("uri" | "url") => "https://example.com",
                _ => "string",
            }),
            Some("integer" | "number") => Value::from(0),
            Some("boolean") => Value::from(true),
            _ => Value::Null,
        }
    }

    fn properties_sample(&self, schema: &'a Value, depth: usize) -> Map<String, Value> {
        schema["properties"]
            .as_object()
            .into_iter()
            .flatten()
            // Servers set read-only properties, so requests leave them out.
            .filter(|(_, property)| self.resolve(property)["readOnly"] != true)
            .map(|(name, property)| (name.clone(), self.sample(property, depth + 1)))
            .collect()
    }
}

/// A variable named after a parameter, for the user to fill in.
fn variable(name: &str) -> String {
    format!("{{{{{name}}}}}")
}

/// Array items, or object keys and values, joined by `separator`. Exploded
/// objects join `key=value` pairs instead.
fn join_value(value: &Value, separator: &str, explode: bool) -> String {
    let parts: Vec<_> = match value {
        Value::Array(items) => items.iter().map(|item| text(Some(item))).collect(),
        Value::Object(properties) if explode => properties
            .iter()
            .map(|(key, value)| format!("{key}={}", text(Some(value))))
            .collect(),
        Value::Object(properties) => properties
            .iter()
            .flat_map(|(key, value)| [key.clone(), text(Some(value))])
            .collect(),
        value => return text(Some(value)),
    };

    parts.join(separator)
}

/// The path with each `{parameter}` replaced by its value. Parameters the
/// specification leaves undescribed become variables.
fn fill_path(path: &str, values: &HashMap<&str, String>) -> String {
    let mut filled = String::new();
    let mut rest = path;

    while let Some(start) = rest.find('{')
        && let Some(length) = rest[start..].find('}')
    {
        let name = &rest[start + 1..start + length];
        filled.push_str(&rest[..start]);
        match values.get(name) {
            Some(value) => filled.push_str(value),
            None => filled.push_str(&variable(name)),
        }
        rest = &rest[start + length + 1..];
    }
    filled.push_str(rest);

    filled
}

/// The body that sends an example value as its media type. A media type
/// other than the body's own becomes the request's `Content-Type`.
fn encode(media_type: &str, value: &Value, headers: &mut Vec<Field>) -> Option<Body> {
    if value.is_null() {
        return None;
    }

    let fields = || -> Vec<(String, String)> {
        value
            .as_object()
            .into_iter()
            .flatten()
            .map(|(name, value)| (name.clone(), text(Some(value))))
            .collect()
    };

    if media_type == "application/x-www-form-urlencoded" {
        Some(Body::UrlEncoded { fields: fields() })
    } else if media_type.starts_with("multipart/form-data") {
        Some(multipart(fields()))
    } else {
        let (language, text) = if media_type.contains("json") {
            (RawLanguage::Json, serde_json::to_string_pretty(value).ok()?)
        } else if let Value::String(text) = value {
            let language = if media_type.contains("xml") {
                RawLanguage::Xml
            } else {
                RawLanguage::Text
            };
            (language, text.clone())
        } else {
            return None;
        };

        if media_type != language.content_type() {
            set_content_type(headers, media_type);
        }
        Some(Body::Raw { language, text })
    }
}

/// A multipart form of text fields.
fn multipart(fields: Vec<(String, String)>) -> Body {
    Body::Multipart {
        parts: fields
            .into_iter()
            .map(|(name, value)| FormPart {
                name,
                value,
                file: false,
            })
            .collect(),
    }
}
