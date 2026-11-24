//! The Swift package around the current directory, as `swift package describe` resolves it.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

use crate::Error;

pub struct Workspace {
    pub root: PathBuf,
    pub build_directory: PathBuf,
    pub targets: Vec<Target>,
}

pub struct Target {
    pub name: String,
    /// Relative to the package root, `/`-separated.
    pub directory: String,
    /// Relative to the package root, `/`-separated.
    pub sources: Vec<String>,
    pub test: bool,
}

impl Workspace {
    pub fn discover_in(dir: &Path) -> Result<Workspace, Error> {
        let root = package_root(dir).ok_or_else(|| {
            Error::new(format!(
                "not inside a Cargo workspace or Swift package: no Cargo.toml or Package.swift in {} or its parents",
                dir.display()
            ))
        })?;
        let output = Command::new("swift")
            .args(["package", "describe", "--type", "json"])
            .current_dir(&root)
            .output()
            .map_err(|e| Error::new(format!("cannot run swift package describe: {e}")))?;
        if !output.status.success() {
            return Err(Error::new(format!(
                "swift package describe failed ({}):\n{}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        let description: Value = serde_json::from_slice(&output.stdout)
            .map_err(|e| Error::new(format!("swift package describe's output is not JSON: {e}")))?;
        Workspace::from_description(root, &description)
    }

    fn from_description(root: PathBuf, description: &Value) -> Result<Workspace, Error> {
        let targets = list(description, "targets")?
            .iter()
            .map(Target::from_description)
            .collect::<Result<_, _>>()?;
        Ok(Workspace {
            build_directory: root.join(".build"),
            root,
            targets,
        })
    }

    /// Every file whose change can change the index: the manifest and its
    /// resolution, and every source SwiftPM puts in a target.
    pub fn source_files(&self) -> Result<Vec<String>, Error> {
        let mut files = vec!["Package.swift".to_owned(), "Package.resolved".to_owned()];
        for target in &self.targets {
            for source in &target.sources {
                files.push(join(&target.directory, source));
            }
        }
        files.sort();
        files.dedup();
        Ok(files)
    }
}

impl Target {
    fn from_description(target: &Value) -> Result<Target, Error> {
        let name = text(target, "name")?.to_owned();
        let directory = relative(Path::new(text(target, "path")?))?;
        let sources = list(target, "sources")?
            .iter()
            .map(|source| {
                let source = source.as_str().ok_or_else(|| missing("sources"))?;
                relative(Path::new(source))
            })
            .collect::<Result<_, _>>()?;
        Ok(Target {
            name,
            directory,
            sources,
            test: text(target, "type")? == "test",
        })
    }
}

fn package_root(dir: &Path) -> Option<PathBuf> {
    std::iter::successors(Some(dir), |path| path.parent())
        .find(|path| path.join("Package.swift").is_file())
        .map(Path::to_path_buf)
}

fn join(directory: &str, source: &str) -> String {
    if directory.is_empty() {
        source.to_owned()
    } else {
        format!("{directory}/{source}")
    }
}

fn relative(path: &Path) -> Result<String, Error> {
    let parts: Vec<_> = path
        .components()
        .map(|part| part.as_os_str().to_string_lossy())
        .collect();
    Ok(parts.join("/"))
}

fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, Error> {
    value[key].as_str().ok_or_else(|| missing(key))
}

fn list<'a>(value: &'a Value, key: &str) -> Result<&'a Vec<Value>, Error> {
    value[key].as_array().ok_or_else(|| missing(key))
}

fn missing(key: &str) -> Error {
    Error::new(format!("swift package describe's output has no {key:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_targets_their_sources_and_which_are_tests() {
        let workspace = Workspace::from_description(
            PathBuf::from("/w"),
            &json!({
                "targets": [{
                    "name": "App",
                    "path": "Sources/App",
                    "sources": ["Model/Queue.swift", "main.swift"],
                    "type": "executable"
                }, {
                    "name": "AppTests",
                    "path": "Tests/AppTests",
                    "sources": ["QueueTests.swift"],
                    "type": "test"
                }]
            }),
        )
        .unwrap();
        assert_eq!(workspace.root, PathBuf::from("/w"));
        assert_eq!(workspace.build_directory, PathBuf::from("/w/.build"));
        assert_eq!(workspace.targets[0].name, "App");
        assert_eq!(
            workspace.targets[0].sources,
            ["Model/Queue.swift", "main.swift"]
        );
        assert!(!workspace.targets[0].test);
        assert!(workspace.targets[1].test);
        assert_eq!(
            workspace.source_files().unwrap(),
            [
                "Package.resolved",
                "Package.swift",
                "Sources/App/Model/Queue.swift",
                "Sources/App/main.swift",
                "Tests/AppTests/QueueTests.swift",
            ]
        );
    }
}
