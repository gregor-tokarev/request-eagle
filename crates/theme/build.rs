use std::{env, error::Error, fs, path::PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    let manifest_directory = PathBuf::from(
        env::var_os("CARGO_MANIFEST_DIR")
            .ok_or("CARGO_MANIFEST_DIR should be set while compiling the theme crate")?,
    );
    let themes_directory = manifest_directory.join("themes");

    println!("cargo:rerun-if-changed={}", themes_directory.display());

    let mut theme_files = fs::read_dir(&themes_directory)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?;
    theme_files.retain(|path| {
        path.is_file()
            && path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
    });
    theme_files.sort();

    let mut generated = String::from("const EMBEDDED_THEME_SETS: &[&str] = &[\n");
    for path in theme_files {
        println!("cargo:rerun-if-changed={}", path.display());

        // Embed the contents so generated source is identical across worktrees.
        let contents = fs::read_to_string(&path)?;
        generated.push_str(&format!("    {contents:?},\n"));
    }
    generated.push_str("];\n");

    let output_directory = PathBuf::from(
        env::var_os("OUT_DIR").ok_or("OUT_DIR should be set while compiling the theme crate")?,
    );
    fs::write(output_directory.join("embedded_theme_sets.rs"), generated)?;

    Ok(())
}
