//! The project being checked: a Cargo workspace or a Swift package. Both end at the same SCIP reader and model lints.

use std::path::Path;

use crate::configuration::Configuration;
use crate::model::Model;
use crate::{Error, indexing, scip, swift, workspace};

pub enum Project {
    Rust(workspace::Workspace),
    Swift(swift::Workspace),
}

impl Project {
    pub fn discover() -> Result<Project, Error> {
        let cwd = std::env::current_dir()
            .map_err(|e| Error::new(format!("cannot read current directory: {e}")))?;
        Project::discover_in(&cwd)
    }

    pub fn discover_in(dir: &Path) -> Result<Project, Error> {
        let cargo = nearest_manifest(dir, "Cargo.toml");
        let swift = nearest_manifest(dir, "Package.swift");
        match (cargo, swift) {
            (Some(cargo), Some(swift))
                if swift.components().count() > cargo.components().count() =>
            {
                swift::Workspace::discover_in(&swift).map(Project::Swift)
            }
            (Some(cargo), _) => workspace::Workspace::discover_in(&cargo).map(Project::Rust),
            (_, Some(swift)) => swift::Workspace::discover_in(&swift).map(Project::Swift),
            (None, None) => Err(Error::new(format!(
                "not inside a Cargo workspace or Swift package: no Cargo.toml or Package.swift in {} or its parents",
                dir.display()
            ))),
        }
    }

    pub fn is_swift(&self) -> bool {
        matches!(self, Project::Swift(_))
    }

    pub fn load_model(
        &self,
        configuration: &Configuration,
        external_index: Option<&Path>,
    ) -> Result<Model, Error> {
        let bytes = match (self, external_index) {
            (_, Some(path)) => {
                std::fs::read(path).map_err(|e| Error::new(format!("{}: {e}", path.display())))?
            }
            (Project::Rust(workspace), None) => indexing::index(workspace, configuration)?,
            (Project::Swift(workspace), None) => swift::indexing::index(workspace)?,
        };
        let index = scip::read(&bytes).map_err(|e| Error::new(format!("the SCIP index: {e}")))?;
        match self {
            Project::Rust(workspace) => crate::model::build(&index, workspace),
            Project::Swift(workspace) => swift::model::build(&index, workspace),
        }
    }
}

fn nearest_manifest(dir: &Path, name: &str) -> Option<std::path::PathBuf> {
    std::iter::successors(Some(dir), |path| path.parent())
        .find(|path| path.join(name).is_file())
        .map(Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_nearest_manifest_wins_when_a_swift_package_sits_inside_a_cargo_checkout() {
        let root = std::env::temp_dir().join(format!("deslop-manifests-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let package = root.join("swift");
        std::fs::create_dir_all(package.join("Sources/App")).unwrap();
        std::fs::write(root.join("Cargo.toml"), "[workspace]\n").unwrap();
        std::fs::write(
            package.join("Package.swift"),
            "// swift-tools-version: 6.0\n",
        )
        .unwrap();
        let nested = package.join("Sources/App");
        assert_eq!(nearest_manifest(&nested, "Cargo.toml"), Some(root.clone()));
        assert_eq!(
            nearest_manifest(&nested, "Package.swift"),
            Some(package.clone())
        );
        assert!(
            package.components().count() > root.components().count(),
            "the package is nearer than the Cargo checkout"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
