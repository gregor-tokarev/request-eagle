use std::{
    collections::HashSet,
    ffi::OsStr,
    fs, io,
    path::{Path, PathBuf},
};

use collection::ImportedCollection;
use serde_json::Value;
use thiserror::Error;

use crate::{
    openapi, postman,
    postman_environment::{self, ImportedEnvironment},
    postman_v3,
};

/// A converted collection or environment.
pub enum Import {
    Collection(CollectionImport),
    Environment(ImportedEnvironment),
}

/// A converted collection and what could not be converted.
pub struct CollectionImport {
    pub collection: ImportedCollection,
    /// Requests left out because Request Eagle cannot send their protocol or
    /// HTTP method.
    pub skipped: Vec<String>,
}

#[derive(Debug, Error)]
pub enum ImportError {
    #[error("Could not read {}: {source}", path.display())]
    Read { path: PathBuf, source: io::Error },
    #[error("The file is not valid JSON or YAML: {0}")]
    Syntax(String),
    #[error("The file is not a Postman collection or environment, or an OpenAPI specification.")]
    UnknownFormat,
    #[error("Postman Collection v1 is not supported. Export the collection as v2.1 and try again.")]
    PostmanV1,
    #[error("The file is part of a Postman collection folder. Import the folder instead.")]
    PostmanV3File,
    #[error(
        "The folder is not a Postman workspace or collection. Choose a Git repository connected \
         to Postman, or one of its collection folders."
    )]
    NotPostmanFolder,
    #[error("OpenAPI {0} is not supported. Use OpenAPI 3 or Swagger 2.0.")]
    UnsupportedOpenApi(String),
}

/// The files Postman writes for each environment of a workspace.
const ENVIRONMENT_EXTENSIONS: [&str; 3] =
    [".environment.yaml", ".environment.yml", ".environment.json"];

/// What importing `path` reads: each collection and environment of a Postman
/// workspace folder, or `path` itself.
pub fn sources(path: &Path) -> Vec<PathBuf> {
    if !path.is_dir() || postman_v3::is_collection(path).unwrap_or(false) {
        return vec![path.to_owned()];
    }

    // A Git repository connected to Postman keeps its workspace in `postman`.
    let workspace = match path.join("postman") {
        folder if folder.is_dir() => folder,
        _ => path.to_owned(),
    };
    let collections = workspace.join("collections");
    let environments = workspace.join("environments");

    // A folder that cannot be listed is read itself, which explains why.
    let mut sources = Vec::new();
    match children(&collections) {
        Ok(folders) => {
            sources.extend(folders.into_iter().filter(|folder| {
                folder.is_dir() && postman_v3::is_collection(folder).unwrap_or(true)
            }))
        }
        Err(_) => sources.push(collections),
    }
    // The workspace's `collections` folder may be chosen itself.
    sources.extend(
        children(path)
            .unwrap_or_default()
            .into_iter()
            .filter(|folder| folder.is_dir() && postman_v3::is_collection(folder).unwrap_or(false)),
    );
    match children(&environments) {
        Ok(files) => sources.extend(
            files
                .into_iter()
                .filter(|file| file.is_file() && is_environment_file(file)),
        ),
        Err(_) => sources.push(environments),
    }

    let repository = match file_name(path) {
        Some("postman") => path.parent().unwrap_or(path),
        _ => path,
    };
    sources.extend(listed(repository));

    let mut seen = HashSet::new();
    sources.retain(|source| seen.insert(fs::canonicalize(source).unwrap_or(source.clone())));

    if sources.is_empty() {
        vec![path.to_owned()]
    } else {
        sources
    }
}

/// The collections and environments a repository's manifest lists, which
/// may be kept outside `postman`.
fn listed(repository: &Path) -> Vec<PathBuf> {
    let manifest = repository.join(".postman");
    let Ok(source) = fs::read_to_string(manifest.join("resources.yaml")) else {
        return Vec::new();
    };
    let Ok(document) = serde_saphyr::from_str::<Value>(&source) else {
        return Vec::new();
    };

    // Paths are relative to `.postman`. Local resources are listed, and the
    // ones already in the cloud map to their IDs.
    let mut paths = Vec::new();
    for kind in ["collections", "environments"] {
        if let Value::Array(listed) = &document["localResources"][kind] {
            paths.extend(listed.iter().filter_map(Value::as_str));
        }
        if let Value::Object(mapped) = &document["cloudResources"][kind] {
            paths.extend(mapped.keys().map(String::as_str));
        }
    }

    paths.into_iter().map(|path| manifest.join(path)).collect()
}

/// The folder's entries in name order. A missing folder has none.
fn children(folder: &Path) -> io::Result<Vec<PathBuf>> {
    let entries = match fs::read_dir(folder) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };

    let mut children = entries
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<io::Result<Vec<_>>>()?;
    children.sort();

    Ok(children)
}

/// Whether Postman would read the file as one of a workspace's environments.
fn is_environment_file(path: &Path) -> bool {
    let name = file_name(path).unwrap_or_default().to_lowercase();
    ENVIRONMENT_EXTENSIONS
        .iter()
        .any(|extension| name.ends_with(extension))
}

fn file_name(path: &Path) -> Option<&str> {
    path.file_name().and_then(OsStr::to_str)
}

/// Converts a file, as [`parse`] does, or a Postman collection folder.
pub fn read(path: &Path) -> Result<Import, ImportError> {
    if path.is_dir() {
        return postman_v3::convert(path).map(Import::Collection);
    }

    let source = fs::read_to_string(path).map_err(|source| ImportError::Read {
        path: path.to_owned(),
        source,
    })?;

    // An environment file may leave out its variables, which content alone
    // would not show to be an environment.
    if is_environment_file(path) {
        let document = document(&source)?;
        if document["name"].is_string() {
            return Ok(Import::Environment(postman_environment::convert(&document)));
        }
    }

    parse(&source)
}

/// Converts a Postman collection or environment, or an OpenAPI specification,
/// recognized by its content.
pub fn parse(source: &str) -> Result<Import, ImportError> {
    let document = document(source)?;

    if document.get("openapi").is_some() || document.get("swagger").is_some() {
        openapi::convert(&document).map(Import::Collection)
    } else if document.get("info").is_some() && document.get("item").is_some() {
        postman::convert(&document).map(Import::Collection)
    } else if document.get("requests").is_some() && document.get("order").is_some() {
        Err(ImportError::PostmanV1)
    } else if postman_environment::is_environment(&document) {
        Ok(Import::Environment(postman_environment::convert(&document)))
    } else if document.get("$kind").is_some() {
        Err(ImportError::PostmanV3File)
    } else {
        Err(ImportError::UnknownFormat)
    }
}

/// The JSON or YAML document in `source`.
fn document(source: &str) -> Result<Value, ImportError> {
    let source = source.trim_start_matches('\u{feff}');

    // YAML also reads JSON, but JSON's errors are clearer for JSON files.
    if source.trim_start().starts_with(['{', '[']) {
        serde_json::from_str(source).map_err(|error| ImportError::Syntax(error.to_string()))
    } else {
        serde_saphyr::from_str(source).map_err(|error| ImportError::Syntax(error.to_string()))
    }
}

/// A single-line name, or `fallback` when there is none.
pub(crate) fn clean_name(name: Option<&str>, fallback: &str) -> String {
    let name = name
        .unwrap_or_default()
        .split(char::is_control)
        .collect::<Vec<_>>()
        .join(" ");
    let name = name.trim();

    if name.is_empty() {
        fallback.to_owned()
    } else {
        name.to_owned()
    }
}

/// A scalar as text; numbers and booleans are written as they appear in JSON.
pub(crate) fn text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        None | Some(Value::Null) => String::new(),
        Some(value) => value.to_string(),
    }
}
