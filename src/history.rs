//! The commits that touched the code, as `git log` lists them: what the design
//! has actually been changing, by the files each commit changed. Git is the
//! only witness of how a change falls across modules, so a workspace that is
//! not a git checkout, or a machine without git, has no history: `change-spread`
//! then has nothing to say.

use std::path::Path;
use std::process::Command;

use crate::model::Commit;

/// The most commits read. A design changes over its recent life; commits old
/// enough describe a system that is no longer here.
const MOST_COMMITS: usize = 2_000;

/// The last commits of the repository `root` is in, most recent first, each by
/// the files it changed below `root`, relative to it and `/`-separated. Empty
/// when git cannot say: not a repository, no git on `PATH`, or no commits yet.
pub fn read(root: &Path) -> Vec<Commit> {
    let Some(prefix) = prefix_of(root) else {
        return Vec::new();
    };
    let log = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["log", "--pretty=format:%x00", "--name-only"])
        .arg(format!("--max-count={MOST_COMMITS}"))
        .arg("--")
        .arg(".")
        .output();
    let Ok(log) = log else {
        return Vec::new();
    };
    if !log.status.success() {
        return Vec::new();
    }
    parse(&String::from_utf8_lossy(&log.stdout), &prefix)
}

/// Where `root` is inside the repository that holds it: `""` at its top, else
/// the directory's path from there, `/`-separated and with a trailing slash.
fn prefix_of(root: &Path) -> Option<String> {
    let prefix = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--show-prefix"])
        .output()
        .ok()?;
    if !prefix.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&prefix.stdout).trim().to_owned())
}

/// `log`, split at the NUL that starts each commit, by the paths it changed
/// below `prefix`. Commits that changed nothing there are left out.
fn parse(log: &str, prefix: &str) -> Vec<Commit> {
    log.split('\0')
        .map(|commit| Commit {
            paths: commit
                .lines()
                .filter_map(|path| path.trim().strip_prefix(prefix))
                .filter(|path| !path.is_empty())
                .map(str::to_owned)
                .collect(),
        })
        .filter(|commit| !commit.paths.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_commit_is_the_paths_it_changed_below_the_workspace() {
        let log = "\0\n\nsrc/kinds.rs\nREADME.md\n\n\0\n\nsrc/socket.rs\n";
        assert_eq!(
            parse(log, ""),
            [
                Commit {
                    paths: vec!["src/kinds.rs".into(), "README.md".into()]
                },
                Commit {
                    paths: vec!["src/socket.rs".into()]
                },
            ]
        );
    }

    #[test]
    fn a_workspace_below_the_repositorys_top_reads_its_own_paths() {
        let log = "\0\n\nworkspace/src/kinds.rs\nelsewhere/src/x.rs\n\n";
        assert_eq!(
            parse(log, "workspace/"),
            [Commit {
                paths: vec!["src/kinds.rs".into()]
            }]
        );
        // A commit that changed nothing here is no commit here.
        assert_eq!(parse("\0\n\nelsewhere/src/x.rs\n", "workspace/"), []);
    }

    #[test]
    fn a_repository_reads_the_history_of_its_own_commits() {
        let root = std::env::temp_dir().join(format!("deslop-history-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        assert_eq!(read(&root), [], "no repository, no history");
        let git = |arguments: &[&str]| {
            Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
                .args(arguments)
                .output()
                .unwrap()
        };
        assert!(git(&["init"]).status.success(), "git init");
        std::fs::write(root.join("a.rs"), "a\n").unwrap();
        assert!(git(&["add", "."]).status.success(), "git add");
        assert!(git(&["commit", "-m", "a"]).status.success(), "git commit");
        assert_eq!(
            read(&root),
            [Commit {
                paths: vec!["a.rs".into()]
            }]
        );
        std::fs::remove_dir_all(&root).unwrap();
    }
}
