use std::{
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

    // The workspace's `collections` folder may be chosen itself.
    let collections = [workspace.join("collections"), path.to_owned()]
        .into_iter()
        .flat_map(|folder| children(&folder))
        .filter(|folder| folder.is_dir() && postman_v3::is_collection(folder).unwrap_or(false));
    let environments = children(&workspace.join("environments"))
        .into_iter()
        .filter(|file| {
            let name = file_name(file).unwrap_or_default();
            file.is_file() && ENVIRONMENT_EXTENSIONS.iter().any(|end| name.ends_with(end))
        });

    let sources: Vec<_> = collections.chain(environments).collect();
    if sources.is_empty() {
        vec![path.to_owned()]
    } else {
        sources
    }
}

/// The folder's entries in name order, without hidden ones. A folder that
/// cannot be read has none.
fn children(folder: &Path) -> Vec<PathBuf> {
    let mut children: Vec<_> = fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| file_name(path).is_some_and(|name| !name.starts_with('.')))
        .collect();
    children.sort();

    children
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

    parse(&source)
}

/// Converts a Postman collection or environment, or an OpenAPI specification,
/// recognized by its content.
pub fn parse(source: &str) -> Result<Import, ImportError> {
    let source = source.trim_start_matches('\u{feff}');

    // YAML also reads JSON, but JSON's errors are clearer for JSON files.
    let document: Value = if source.trim_start().starts_with(['{', '[']) {
        serde_json::from_str(source).map_err(|error| ImportError::Syntax(error.to_string()))?
    } else {
        serde_saphyr::from_str(source).map_err(|error| ImportError::Syntax(error.to_string()))?
    };

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
