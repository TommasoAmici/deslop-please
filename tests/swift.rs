//! The Swift path through a real SwiftPM build and `scip-swift`. This runs only on macOS, where the indexer exists.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const PROGRAM: &str = env!("CARGO_BIN_EXE_deslop-please");

fn copy_directory(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let target = to.join(path.file_name().unwrap());
        if path.is_dir() {
            copy_directory(&path, &target);
        } else {
            std::fs::copy(&path, &target).unwrap();
        }
    }
}

fn run(directory: &Path, arguments: &[&str]) -> Output {
    let output = Command::new(PROGRAM)
        .args(arguments)
        .current_dir(directory)
        .env("CARGO_TERM_COLOR", "never")
        .output()
        .unwrap();
    eprintln!(
        "$ deslop-please {}\n{}{}",
        arguments.join(" "),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn find_index(directory: &Path) -> Option<PathBuf> {
    for entry in std::fs::read_dir(directory).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_file() && path.file_name().is_some_and(|name| name == "index.scip") {
            return Some(path);
        }
        if path.is_dir()
            && let Some(index) = find_index(&path)
        {
            return Some(index);
        }
    }
    None
}

#[test]
fn a_swift_package_is_indexed_and_an_external_scip_index_gives_the_same_report() {
    let indexer = Command::new("scip-swift").arg("--version").output();
    assert!(
        indexer.is_ok_and(|output| output.status.success()),
        "this test needs scip-swift on PATH (the macOS CI job installs it)"
    );
    let workspace = Path::new(env!("CARGO_TARGET_TMPDIR")).join("swift-package");
    if workspace.exists() {
        std::fs::remove_dir_all(&workspace).unwrap();
    }
    copy_directory(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/swift"),
        &workspace,
    );
    let allow = workspace.with_extension("allow");

    let managed = run(&workspace, &["--update-allow", allow.to_str().unwrap()]);
    assert!(managed.status.success());
    let keys = std::fs::read_to_string(&allow).unwrap();
    assert!(
        keys.contains("field-leak App::Model::Queue::items"),
        "{keys}"
    );
    assert!(
        keys.contains("call-order App::Policy::admit,next"),
        "a free function's first parameter names the type it works on: {keys}"
    );
    assert!(
        !keys.contains("field-leak App::Observed::value")
            && !keys.contains("field-leak App::Observed::isSet")
            && !keys.contains("field-leak App::Counter::value"),
        "source accessors and Observation macro expansions are not outside field uses: {keys}"
    );
    assert!(
        keys.contains("field-leak App::Model::Guarded::other")
            && !keys.contains("field-leak App::Model::Guarded::value")
            && !keys.contains("field-leak App::Model::Guarded::doubled"),
        "extension methods and property accessors are part of their type, even in another directory: {keys}"
    );

    let index = find_index(&workspace.join(".build/deslop-please")).unwrap();
    let external_allow = workspace.with_extension("external.allow");
    let external = run(
        &workspace,
        &[
            "--scip",
            index.to_str().unwrap(),
            "--update-allow",
            external_allow.to_str().unwrap(),
        ],
    );
    assert!(external.status.success());
    assert_eq!(keys, std::fs::read_to_string(external_allow).unwrap());
}
