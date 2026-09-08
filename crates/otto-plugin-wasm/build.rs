use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

// Canonical-absent package builds still need a precise sanity check, so keep
// the vendored file set and normalized-content digests pinned here.
const EXPECTED_VENDORED_WIT: &[(&str, u64)] = &[
    ("plugin-interactive.wit", 0xc5cec69227c8b970),
    ("plugin-provider.wit", 0xf62b9a6b8ae413dc),
    ("plugin-static.wit", 0x0f0960c5e8fc564b),
    ("shared.wit", 0xa07f91c5fb481db3),
    ("spp.wit", 0x114f2936ffaac15e),
];

fn main() {
    println!("cargo::rerun-if-changed=wit");

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
    if canonical_exists {
        println!("cargo::rerun-if-changed=../otto-plugin-wit/wit");
    }
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

    if !canonical_exists {
        validate_expected_vendored_tree(&vendored_dir, &vendored_files);
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

fn validate_expected_vendored_tree(vendored_dir: &Path, vendored_files: &BTreeSet<PathBuf>) {
    let expected_files: BTreeSet<_> = EXPECTED_VENDORED_WIT
        .iter()
        .map(|(file, _)| PathBuf::from(file))
        .collect();
    if vendored_files != &expected_files {
        panic!(
            "vendored WIT directory `{}` does not match the expected file set.\nexpected: {:?}\nactual: {:?}\nresync `crates/otto-plugin-wasm/wit/` from `crates/otto-plugin-wit/wit/` and update `EXPECTED_VENDORED_WIT` if the canonical contract changed.",
            vendored_dir.display(),
            expected_files,
            vendored_files,
        );
    }

    let expected_digests: BTreeMap<_, _> = EXPECTED_VENDORED_WIT.iter().copied().collect();
    for relative_path in vendored_files {
        let path = vendored_dir.join(relative_path);
        let bytes = fs::read(&path).unwrap_or_else(|error| {
            panic!(
                "failed to read vendored WIT file `{}`: {error}",
                path.display()
            )
        });
        let actual_digest = fnv1a64(&bytes);
        let expected_digest = expected_digests[relative_path.to_string_lossy().as_ref()];

        if actual_digest != expected_digest {
            panic!(
                "vendored WIT file `{}` has digest {:016x}, expected {:016x}.\nresync `crates/otto-plugin-wasm/wit/` from `crates/otto-plugin-wit/wit/` and update `EXPECTED_VENDORED_WIT` if the canonical contract changed.",
                path.display(),
                actual_digest,
                expected_digest,
            );
        }
    }
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in normalize_newlines(bytes) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn normalize_newlines(bytes: &[u8]) -> Vec<u8> {
    let mut normalized = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\r' && bytes.get(index + 1) == Some(&b'\n') {
            normalized.push(b'\n');
            index += 2;
            continue;
        }
        normalized.push(bytes[index]);
        index += 1;
    }
    normalized
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
