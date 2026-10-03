//! Embed the release notes that the release workflow generates with
//! `scripts/release-notes.py`. Other builds embed an empty list.

use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo::rerun-if-env-changed=REQUEST_EAGLE_RELEASE_NOTES");

    let notes = match env::var_os("REQUEST_EAGLE_RELEASE_NOTES") {
        Some(path) => {
            let path = PathBuf::from(path);
            println!("cargo::rerun-if-changed={}", path.display());

            let notes = fs::read_to_string(&path).unwrap_or_else(|error| {
                panic!(
                    "Could not read release notes from {}: {error}",
                    path.display()
                )
            });

            // A broken file fails the release build rather than the app.
            match serde_json::from_str::<serde_json::Value>(&notes) {
                Ok(serde_json::Value::Array(_)) => notes,
                _ => panic!("{} is not a JSON list of releases", path.display()),
            }
        }
        None => "[]".to_owned(),
    };

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR"));
    fs::write(out_dir.join("release-notes.json"), notes).expect("OUT_DIR is writable");
}
