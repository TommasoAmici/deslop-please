//! Architectural linter for Rust and Swift: enforces deep modules and narrow interfaces, working like `cargo clippy`
//! or a Swift package's tests.

mod allow;
mod configuration;
mod format;
mod graph;
mod indexing;
mod lint;
mod model;
mod project;
mod scip;
mod swift;
mod workspace;

use std::fmt;
use std::path::PathBuf;
use std::process::ExitCode;

use allow::AllowList;
use configuration::Configuration;
use project::Project;

pub const TOOL: &str = env!("CARGO_BIN_NAME");

#[derive(Debug)]
pub struct Error(String);

impl Error {
    pub fn new(message: impl Into<String>) -> Error {
        Error(message.into())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputFormat {
    Text,
    Json,
    Sarif,
}

struct Options {
    scope: Option<String>,
    allow: Option<PathBuf>,
    update_allow: Option<PathBuf>,
    scip: Option<PathBuf>,
    format: OutputFormat,
    strict_allow: bool,
    configuration: Configuration,
}

fn usage() -> String {
    format!(
        "\
usage: {TOOL} [crate_or_mod] [--allow <path>] [--strict-allow] [--update-allow <path>]
       [--scip <index.scip>] [--target <triple>] [-F, --features <features>]
       [--all-features] [--no-default-features] [--json | --sarif]

Runs architectural lints (module-cycle, tangle, field-leak, shared-mutable-bag,
shallow-module, wide-type, wide-module, wide-struct, wide-enum, scattered-impl,
unused-pub, test-only-pub, over-public, single-impl-trait, pass-through,
reexport-sprawl, long-function, wide-signature, positional-trap, string-error,
duplicate-code, duplicate-struct, high-fan-out, god-hub, feature-envy,
layer-violation, unused-dep, wide-directory, long-file) on the Cargo workspace
or Swift package around the current directory. Rust uses `rust-analyzer scip`;
Swift uses `scip-swift`.

options:
  <crate_or_mod>               check only one crate, target or directory module
  --allow <path>               suppress listed existing debt; new errors still fail
  --strict-allow               fail if `--allow` contains stale (already-fixed) entries
  --update-allow <path>        write current diagnostics to <path>
  --scip <index.scip>          read an existing SCIP index instead of running the
                              language's indexer (the project is still discovered)
  --json                       write diagnostics as JSON to stdout
  --sarif                      write diagnostics as SARIF 2.1.0 to stdout
  -V, --version                print the version and exit
  -h, --help                   print this help and exit

configuration, as `cargo check --workspace` takes it:
  --target <triple>            check for the target triple, not the host's
  -F, --features <features>    space or comma separated list of features to
                               activate; may be given more than once
  --all-features               activate all features of every member
  --no-default-features        do not activate the `default` feature

A run checks one configuration: the host's target with default features,
unless these flags or cargo's own configuration (`build.target`, `RUSTFLAGS`)
choose another. Code behind a `cfg` that it leaves off, such as a
`#[cfg(windows)]` module on Linux, is not checked: run once for each
configuration that matters.

A flag's value is the next word, or follows `=`: `--target=<triple>`.
"
    )
}

/// The flags, and whether each takes a value. `-F` is `--features`.
const FLAGS: [(&str, bool); 10] = [
    ("--allow", true),
    ("--strict-allow", false),
    ("--update-allow", true),
    ("--scip", true),
    ("--target", true),
    ("--features", true),
    ("--all-features", false),
    ("--no-default-features", false),
    ("--json", false),
    ("--sarif", false),
];

/// Reads the command line as cargo reads its own: a flag's value is the next
/// word, or follows `=` in the flag's own (`--target=<triple>`, `-F=a,b`) or
/// `-F` (`-Fa,b`); only `--target` and `--features` may be given more than
/// once.
fn parse_arguments(arguments: &[String]) -> Result<Options, Error> {
    let mut options = Options {
        scope: None,
        allow: None,
        update_allow: None,
        scip: None,
        format: OutputFormat::Text,
        strict_allow: false,
        configuration: Configuration::default(),
    };
    let mut given = Vec::new();
    let mut words = arguments.iter().map(String::as_str).peekable();
    while let Some(word) = words.next() {
        if !word.starts_with('-') {
            if options.scope.replace(word.to_owned()).is_some() {
                return Err(Error::new(format!("unexpected argument '{word}'")));
            }
            continue;
        }
        let (flag, attached) = split_flag(word);
        let Some(&(flag, takes_value)) = FLAGS.iter().find(|(name, _)| *name == flag) else {
            return Err(Error::new(format!("unexpected argument '{word}'")));
        };
        if given.contains(&flag) && !matches!(flag, "--target" | "--features") {
            return Err(Error::new(format!(
                "'{flag}' cannot be given more than once"
            )));
        }
        given.push(flag);
        let value = match (takes_value, attached) {
            (true, Some(value)) => value,
            (true, None) => words
                .next_if(|next| !next.starts_with('-'))
                .ok_or_else(|| Error::new(format!("'{flag}' needs a value")))?,
            (false, Some(value)) => {
                return Err(Error::new(format!("'{flag}' takes no value: '{value}'")));
            }
            (false, None) => "",
        };
        let configuration = &mut options.configuration;
        match flag {
            "--allow" => {
                if value.is_empty() {
                    return Err(Error::new("'--allow' needs a path"));
                }
                options.allow = Some(PathBuf::from(value));
            }
            "--strict-allow" => options.strict_allow = true,
            "--update-allow" => {
                if value.is_empty() {
                    return Err(Error::new("'--update-allow' needs a path"));
                }
                options.update_allow = Some(PathBuf::from(value));
            }
            "--scip" => {
                if value.is_empty() {
                    return Err(Error::new("'--scip' needs a path"));
                }
                options.scip = Some(PathBuf::from(value));
            }
            "--target" => configuration.target(value)?,
            "--features" => configuration.features(value),
            "--all-features" => configuration.all_features(),
            "--no-default-features" => configuration.no_default_features(),
            "--json" => {
                if options.format == OutputFormat::Sarif {
                    return Err(Error::new("'--json' and '--sarif' cannot be given together"));
                }
                options.format = OutputFormat::Json;
            }
            "--sarif" => {
                if options.format == OutputFormat::Json {
                    return Err(Error::new("'--json' and '--sarif' cannot be given together"));
                }
                options.format = OutputFormat::Sarif;
            }
            _ => unreachable!("{flag} has no meaning"),
        }
    }
    if options.allow.is_some() && options.update_allow.is_some() {
        return Err(Error::new(
            "'--allow' and '--update-allow' cannot be given together",
        ));
    }
    if options.strict_allow && options.allow.is_none() {
        return Err(Error::new("'--strict-allow' requires '--allow <path>'"));
    }
    if options.update_allow.is_some() {
        match options.format {
            OutputFormat::Text => {}
            OutputFormat::Json => {
                return Err(Error::new("'--json' and '--update-allow' cannot be given together"));
            }
            OutputFormat::Sarif => {
                return Err(Error::new("'--sarif' and '--update-allow' cannot be given together"));
            }
        }
    }
    Ok(options)
}

/// The flag `word` names, and the value written in it: `--target=<triple>`,
/// and `-F<features>` or `-F=<features>`.
fn split_flag(word: &str) -> (&str, Option<&str>) {
    if let Some(value) = word.strip_prefix("-F") {
        let value = (!value.is_empty()).then(|| value.strip_prefix('=').unwrap_or(value));
        return ("--features", value);
    }
    match word.split_once('=') {
        Some((flag, value)) => (flag, Some(value)),
        None => (word, None),
    }
}

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if matches!(
        arguments.as_slice(),
        [help] if help == "-h" || help == "--help" || help == "help"
    ) {
        print!("{}", usage());
        return ExitCode::SUCCESS;
    }
    if matches!(
        arguments.as_slice(),
        [version] if version == "-V" || version == "--version" || version == "version"
    ) {
        println!("{TOOL} {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    let options = match parse_arguments(&arguments) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("{TOOL}: {message}");
            eprint!("{}", usage());
            return ExitCode::from(2);
        }
    };
    match run(options) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("{TOOL}: {error}");
            ExitCode::from(2)
        }
    }
}

fn run(options: Options) -> Result<bool, Error> {
    let project = Project::discover()?;
    if options.scip.is_some() && !options.configuration.is_default() {
        return Err(Error::new(
            "--target and --features choose a configuration while indexing; they cannot be used with --scip",
        ));
    }
    if project.is_swift() && !options.configuration.is_default() {
        return Err(Error::new(
            "--target and --features choose a Cargo configuration; Swift packages do not take them",
        ));
    }
    let model = project.load_model(&options.configuration, options.scip.as_deref())?;
    let scope = match &options.scope {
        Some(query) => Some(
            lint::resolve_node(&model, query)
                .ok_or_else(|| Error::new(format!("{query}: no such crate or module")))?,
        ),
        None => None,
    };
    let all = lint::check(&model, scope);

    if let Some(path) = options.update_allow {
        let allow = AllowList::from_diagnostics(&all);
        allow.write(&path)?;
        eprintln!("{TOOL}: wrote {} entries to {}", all.len(), path.display());
        return Ok(true);
    }

    let (reported, stale_allowed, allow_shown) = match &options.allow {
        Some(path) => {
            let allow = AllowList::read(path)?;
            let stale = allow.stale(&all);
            let active: Vec<_> = all.into_iter().filter(|d| !allow.is_allowed(d)).collect();
            (active, stale, Some(path.display().to_string()))
        }
        None => (all, Vec::new(), None),
    };

    match options.format {
        OutputFormat::Text => print!(
            "{}",
            format::render(
                &reported,
                &stale_allowed,
                allow_shown.as_deref(),
                options.strict_allow,
            )
        ),
        OutputFormat::Json => print!(
            "{}",
            format::render_json(&reported, stale_allowed.len(), allow_shown.as_deref())
        ),
        OutputFormat::Sarif => print!("{}", format::render_sarif(&reported)),
    }
    Ok(reported.is_empty() && (!options.strict_allow || stale_allowed.is_empty()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(words: &[&str]) -> Result<Options, Error> {
        let arguments: Vec<String> = words.iter().map(|&word| word.to_owned()).collect();
        parse_arguments(&arguments)
    }

    fn configuration(words: &[&str]) -> Configuration {
        parsed(words).unwrap().configuration
    }

    #[test]
    fn no_flags_choose_the_default_configuration() {
        let options = parsed(&["my-crate"]).unwrap();
        assert_eq!(options.scope.as_deref(), Some("my-crate"));
        assert_eq!((options.allow, options.update_allow), (None, None));
        assert_eq!(options.format, OutputFormat::Text);
        assert!(options.configuration.is_default());
    }

    #[test]
    fn a_value_follows_its_flag_or_an_equals_sign_in_it() {
        let spaced = [
            "my-crate",
            "--allow",
            "a.allow",
            "--target",
            "aarch64-apple-darwin",
        ];
        let joined = [
            "--allow=a.allow",
            "--target=aarch64-apple-darwin",
            "my-crate",
        ];
        for words in [&spaced[..], &joined[..]] {
            let options = parsed(words).unwrap();
            assert_eq!(options.scope.as_deref(), Some("my-crate"));
            assert_eq!(options.allow, Some(PathBuf::from("a.allow")));
            assert_eq!(
                options.configuration.to_string(),
                "--target aarch64-apple-darwin"
            );
        }
        for words in [
            &["--update-allow", "b.allow"][..],
            &["--update-allow=b.allow"],
        ] {
            let options = parsed(words).unwrap();
            assert_eq!(options.update_allow, Some(PathBuf::from("b.allow")));
        }
    }

    #[test]
    fn features_are_written_as_cargo_takes_them() {
        let expected = configuration(&["--features", "a,b"]);
        for words in [
            &["--features", "a b"][..],
            &["--features= a , b "],
            &["--features", "a", "--features", "b"],
            &["-F", "a", "-Fb"],
            &["-F=a,b"],
        ] {
            assert_eq!(configuration(words), expected, "{words:?}");
        }
        assert!(configuration(&["--features="]).is_default());
        assert_eq!(
            configuration(&["--no-default-features", "--all-features"]).to_string(),
            "--all-features --no-default-features"
        );
    }

    #[test]
    fn json_and_sarif_flags_are_parsed() {
        let options = parsed(&["--json", "my-crate"]).unwrap();
        assert_eq!(options.format, OutputFormat::Json);
        assert_eq!(options.scope.as_deref(), Some("my-crate"));
        assert!(parsed(&["--json", "--json"]).is_err());

        let options = parsed(&["--sarif", "my-crate"]).unwrap();
        assert_eq!(options.format, OutputFormat::Sarif);
        assert_eq!(options.scope.as_deref(), Some("my-crate"));
        assert!(parsed(&["--sarif", "--sarif"]).is_err());
        assert!(parsed(&["--json", "--sarif"]).is_err());
        assert!(parsed(&["--sarif", "--json"]).is_err());
    }

    #[test]
    fn two_targets_are_an_error_since_rust_analyzer_indexes_one() {
        let two = [
            "--target",
            "aarch64-apple-darwin",
            "--target=x86_64-pc-windows-msvc",
        ];
        let error = parsed(&two).err().unwrap().to_string();
        assert!(error.contains("one target per run"), "{error}");
        let same = [
            "--target",
            "aarch64-apple-darwin",
            "--target=aarch64-apple-darwin",
        ];
        assert_eq!(
            configuration(&same).to_string(),
            "--target aarch64-apple-darwin"
        );
    }

    #[test]
    fn an_external_index_is_read_instead_of_running_the_indexer() {
        let options = parsed(&["App::Model", "--scip=index.scip"]).unwrap();
        assert_eq!(options.scope.as_deref(), Some("App::Model"));
        assert_eq!(options.scip, Some(PathBuf::from("index.scip")));
        assert!(options.configuration.is_default());
        assert!(parsed(&["--scip"]).is_err());
        assert!(parsed(&["--scip=a", "--scip=b"]).is_err());
    }

    #[test]
    fn what_cargo_would_reject_is_an_error() {
        for words in [
            &["--target"][..],
            &["--target="],
            &["--features"],
            &["--target", "--features", "a"],
            &["--all-features", "--all-features"],
            &["--no-default-features=true"],
            &["--allow", "a", "--allow", "b"],
            &["--allow", "a", "--update-allow", "b"],
            &["--allow="],
            &["--update-allow="],
            &["--scip="],
            &["--json", "--update-allow", "a"],
            &["--sarif", "--update-allow", "a"],
            &["--sarif=true"],
            &["--strict-allow"],
            &["--strict-allow=true"],
            &["--release"],
            &["-p", "my-crate"],
            &["my-crate", "other-crate"],
        ] {
            assert!(parsed(words).is_err(), "{words:?}");
        }
    }
}
