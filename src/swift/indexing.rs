//! `scip-swift`'s SCIP index of a Swift package, cached under `.build/deslop-please/`.
//!
//! `scip-swift` builds the package with SwiftPM's index store enabled, then
//! reads the same IndexStoreDB records Xcode and SourceKit-LSP use. It keeps its
//! own build cache so a run never depends on an Xcode project or DerivedData.

use std::hash::{DefaultHasher, Hasher};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::swift::Workspace;
use crate::{Error, TOOL};

pub fn index(workspace: &Workspace) -> Result<Vec<u8>, Error> {
    let directory = cache_dir(workspace)?;
    let index = directory.join("index.scip");
    let stamp_path = directory.join("index.stamp");
    let stamp = stamp(workspace)?;
    let last_stamp = std::fs::read_to_string(&stamp_path).ok();
    if last_stamp.as_deref() != Some(stamp.as_str()) || !index.exists() {
        eprintln!("{TOOL}: indexing Swift package...");
        std::fs::create_dir_all(&directory).map_err(|e| io_error(&directory, e))?;
        remove_if_present(&stamp_path)?;
        run_scip_swift(workspace, &directory, &index)?;
        std::fs::write(&stamp_path, stamp).map_err(|e| io_error(&stamp_path, e))?;
    }
    std::fs::read(&index).map_err(|e| io_error(&index, e))
}

fn cache_dir(workspace: &Workspace) -> Result<PathBuf, Error> {
    let mut hasher = DefaultHasher::new();
    hasher.write(workspace.root.to_string_lossy().as_bytes());
    Ok(workspace
        .build_directory
        .join(TOOL)
        .join("swift")
        .join(format!("{:016x}", hasher.finish())))
}

fn stamp(workspace: &Workspace) -> Result<String, Error> {
    let mut stamp = version("scip-swift")?;
    stamp.push_str(&version("swift")?);
    for file in workspace.source_files()? {
        let hash = match std::fs::read(workspace.root.join(&file)) {
            Ok(bytes) => {
                let mut hasher = DefaultHasher::new();
                hasher.write(&bytes);
                format!("{:016x}", hasher.finish())
            }
            Err(e) if e.kind() == ErrorKind::NotFound => "missing".to_owned(),
            Err(e) => return Err(io_error(&workspace.root.join(&file), e)),
        };
        stamp += &format!("{hash} {file}\n");
    }
    Ok(stamp)
}

fn version(program: &str) -> Result<String, Error> {
    let output = Command::new(program)
        .arg("--version")
        .output()
        .map_err(|e| {
            if e.kind() == ErrorKind::NotFound {
                Error::new(format!(
                    "{program} is not on PATH: Swift support reads the index that `scip-swift` writes"
                ))
            } else {
                Error::new(format!("cannot run {program}: {e}"))
            }
        })?;
    if !output.status.success() {
        return Err(Error::new(format!(
            "{program} --version failed ({})",
            output.status
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn run_scip_swift(workspace: &Workspace, directory: &Path, index: &Path) -> Result<(), Error> {
    let cache = directory.join("scip-swift");
    let mut output = scip_swift(workspace, &cache, index, false)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // SwiftPM 6.4 moved the index store to `out/Products/<configuration>/`.
        // scip-swift 0.3 still looks in `<scratch>/<triple>/<configuration>/`;
        // lend it the store the build did produce and let it convert that.
        if stderr.contains("no IndexStore")
            && let Some(store) = find_index_store(&cache.join("build-scratch"), 0)?
        {
            link_index_store(&cache, &store)?;
            output = scip_swift(workspace, &cache, index, true)?;
        }
    }
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let last_lines: Vec<&str> = stderr.lines().rev().take(20).collect();
    let last_lines: Vec<&str> = last_lines.into_iter().rev().collect();
    Err(Error::new(format!(
        "scip-swift failed ({}):\n{}",
        output.status,
        last_lines.join("\n")
    )))
}

fn scip_swift(
    workspace: &Workspace,
    cache: &Path,
    index: &Path,
    index_only: bool,
) -> Result<std::process::Output, Error> {
    let mut command = Command::new("scip-swift");
    command
        .arg("index")
        .arg(&workspace.root)
        .arg("--output")
        .arg(index)
        .arg("--cache-dir")
        .arg(cache);
    if index_only {
        command.arg("--index-only");
    }
    command
        .output()
        .map_err(|e| Error::new(format!("cannot run scip-swift: {e}")))
}

fn find_index_store(directory: &Path, depth: usize) -> Result<Option<PathBuf>, Error> {
    if depth > 8 {
        return Ok(None);
    }
    let entries = std::fs::read_dir(directory).map_err(|e| io_error(directory, e))?;
    for entry in entries {
        let entry = entry.map_err(|e| io_error(directory, e))?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|e| io_error(&path, e))?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            if path.file_name().is_some_and(|name| name == "store")
                && path
                    .parent()
                    .is_some_and(|parent| parent.ends_with("index"))
            {
                return Ok(Some(path));
            }
            if let Some(store) = find_index_store(&path, depth + 1)? {
                return Ok(Some(store));
            }
        }
    }
    Ok(None)
}

#[cfg(unix)]
fn link_index_store(cache: &Path, store: &Path) -> Result<(), Error> {
    let link = cache
        .join("build-scratch")
        .join("deslop-please")
        .join("debug")
        .join("index")
        .join("store");
    if let Some(parent) = link.parent() {
        std::fs::create_dir_all(parent).map_err(|e| io_error(parent, e))?;
    }
    remove_if_present(&link)?;
    std::os::unix::fs::symlink(store, &link).map_err(|e| io_error(&link, e))
}

#[cfg(not(unix))]
fn link_index_store(_cache: &Path, _store: &Path) -> Result<(), Error> {
    Err(Error::new(
        "managed Swift indexing uses scip-swift, which runs on macOS",
    ))
}

fn remove_if_present(path: &Path) -> Result<(), Error> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != ErrorKind::NotFound => Err(io_error(path, e)),
        _ => Ok(()),
    }
}

fn io_error(path: &Path, error: std::io::Error) -> Error {
    Error::new(format!("{}: {error}", path.display()))
}
