use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo::rerun-if-changed=wit");
    println!("cargo::rerun-if-changed=../otto-plugin-wit/wit");

    let manifest_dir = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR")
            .expect("Cargo sets CARGO_MANIFEST_DIR for build scripts"),
    );
    let vendored_dir = manifest_dir.join("wit");
    let canonical_dir = manifest_dir.join("../otto-plugin-wit/wit");

    let vendored_exists = vendored_dir
        .try_exists()
        .expect("checking vendored WIT directory should succeed");
    if !vendored_exists {
        panic!(
            "vendored WIT directory `{}` is missing.\nrestore `crates/otto-plugin-wasm/wit/` before building.",
            vendored_dir.display()
        );
    }

    let canonical_exists = canonical_dir
        .try_exists()
        .expect("checking canonical WIT directory should succeed");
    if !canonical_exists {
        let vendored_files = collect_wit_files(&vendored_dir)
            .map_err(|error| {
                format!(
                    "failed to read vendored WIT dir `{}`: {error}",
                    vendored_dir.display()
                )
            })
            .unwrap_or_else(|error| panic!("{error}"));
        if vendored_files.is_empty() {
            panic!(
                "vendored WIT directory `{}` does not contain any `.wit` files.\nrestore `crates/otto-plugin-wasm/wit/` before building.",
                vendored_dir.display()
            );
        }
        return;
    }

    if let Err(error) = verify_wit_sync(&vendored_dir, &canonical_dir) {
        panic!("{error}");
    }
}

fn verify_wit_sync(vendored_dir: &Path, canonical_dir: &Path) -> Result<(), String> {
    let vendored_files = collect_wit_files(vendored_dir).map_err(|error| {
        format!(
            "failed to read vendored WIT dir `{}`: {error}",
            vendored_dir.display()
        )
    })?;
    let canonical_files = collect_wit_files(canonical_dir).map_err(|error| {
        format!(
            "failed to read canonical WIT dir `{}`: {error}",
            canonical_dir.display()
        )
    })?;

    if vendored_files != canonical_files {
        return Err(format!(
            "vendored WIT files in `{}` are out of sync with canonical `{}`.\nvendored: {:?}\ncanonical: {:?}\nresync `crates/otto-plugin-wasm/wit/` from `crates/otto-plugin-wit/wit/`.",
            vendored_dir.display(),
            canonical_dir.display(),
            vendored_files,
            canonical_files,
        ));
    }

    for relative_path in vendored_files {
        let vendored_path = vendored_dir.join(&relative_path);
        let canonical_path = canonical_dir.join(&relative_path);
        let vendored_bytes = fs::read(&vendored_path).map_err(|error| {
            format!(
                "failed to read vendored WIT file `{}`: {error}",
                vendored_path.display()
            )
        })?;
        let canonical_bytes = fs::read(&canonical_path).map_err(|error| {
            format!(
                "failed to read canonical WIT file `{}`: {error}",
                canonical_path.display()
            )
        })?;

        if vendored_bytes != canonical_bytes {
            return Err(format!(
                "vendored WIT file `{}` differs from canonical `{}`.\nresync `crates/otto-plugin-wasm/wit/` from `crates/otto-plugin-wit/wit/`.",
                vendored_path.display(),
                canonical_path.display(),
            ));
        }
    }

    Ok(())
}

fn collect_wit_files(root: &Path) -> std::io::Result<BTreeSet<PathBuf>> {
    let mut files = BTreeSet::new();
    collect_wit_files_recursive(root, root, &mut files)?;
    Ok(files)
}

fn collect_wit_files_recursive(
    root: &Path,
    current: &Path,
    files: &mut BTreeSet<PathBuf>,
) -> std::io::Result<()> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;

        if file_type.is_dir() {
            collect_wit_files_recursive(root, &path, files)?;
            continue;
        }

        if path.extension().is_some_and(|extension| extension == "wit") {
            let relative_path = path
                .strip_prefix(root)
                .expect("walked path stays under root");
            files.insert(relative_path.to_path_buf());
        }
    }

    Ok(())
}
