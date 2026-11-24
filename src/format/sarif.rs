//! SARIF v2.1.0 diagnostic formatting for CI and static analysis aggregators
//! (GitHub Code Scanning, SonarQube, reviewdog, etc.).

use std::fmt::Write as _;

use serde_json::{Value, json};

use crate::TOOL;
use crate::lint::{Diagnostic, Lint, Location, Picture};

const SCHEMA: &str = "https://json.schemastore.org/sarif-2.1.0.json";
const VERSION: &str = "2.1.0";

const ALL_LINTS: [Lint; 25] = [
    Lint::ModuleCycle,
    Lint::Tangle,
    Lint::FieldLeak,
    Lint::SharedMutableBag,
    Lint::ShallowModule,
    Lint::WideType,
    Lint::WideModule,
    Lint::WideStruct,
    Lint::WideEnum,
    Lint::ScatteredImpl,
    Lint::UnusedPub,
    Lint::TestOnlyPub,
    Lint::OverPublic,
    Lint::PassThrough,
    Lint::ReexportSprawl,
    Lint::LongFunction,
    Lint::WideSignature,
    Lint::StringError,
    Lint::DuplicateCode,
    Lint::HighFanOut,
    Lint::FeatureEnvy,
    Lint::LayerViolation,
    Lint::UnusedDep,
    Lint::WideDirectory,
    Lint::LongFile,
];
pub fn render(diagnostics: &[Diagnostic]) -> String {
    let rules: Vec<Value> = ALL_LINTS
        .iter()
        .map(|&lint| rule_descriptor(lint))
        .collect();
    let results: Vec<Value> = diagnostics.iter().map(result_json).collect();

    let out = json!({
        "$schema": SCHEMA,
        "version": VERSION,
        "runs": [
            {
                "tool": {
                    "driver": {
                        "name": TOOL,
                        "version": env!("CARGO_PKG_VERSION"),
                        "informationUri": env!("CARGO_PKG_REPOSITORY"),
                        "rules": rules,
                    }
                },
                "results": results,
            }
        ]
    });

    serde_json::to_string_pretty(&out).expect("diagnostics serialize to SARIF") + "\n"
}

fn rule_descriptor(lint: Lint) -> Value {
    json!({
        "id": lint.name(),
        "shortDescription": {
            "text": short_description(lint),
        },
        "defaultConfiguration": {
            "level": "error",
        },
    })
}

fn rule_index(lint: Lint) -> usize {
    ALL_LINTS
        .iter()
        .position(|&candidate| candidate == lint)
        .expect("every lint is listed in ALL_LINTS")
}

fn short_description(lint: Lint) -> &'static str {
    match lint {
        Lint::ModuleCycle => "Modules depend on each other in a cycle",
        Lint::Tangle => {
            "Items depend on each other in a cycle across modules or beyond what fits in mind"
        }
        Lint::FieldLeak => "A struct field is manipulated outside its owning type",
        Lint::SharedMutableBag => "A struct's fields are mutated from several other modules",
        Lint::ShallowModule => {
            "A module exposes too much of its interface relative to what it hides"
        }
        Lint::WideType => "A type shows more working functions than a reader can hold in mind",
        Lint::WideModule => "A leaf module shows more working functions than a reader can scan",
        Lint::WideStruct => "A struct has more fields than a reader can scan at once",
        Lint::WideEnum => "An enum has more variants than a reader can scan at once",
        Lint::ScatteredImpl => "A type's inherent `impl` blocks are spread across modules",
        Lint::UnusedPub => "A `pub` item has no non-test callers outside its library",
        Lint::TestOnlyPub => "A `pub(crate)`/`pub(super)` item is only referenced by tests",
        Lint::OverPublic => "A `pub` item is only used inside its own library",
        Lint::PassThrough => "A shim adds a second path to code without adding abstraction",
        Lint::ReexportSprawl => "A module re-exports another module's or crate's whole namespace",
        Lint::LongFunction => "A function is longer or branches more than a reader can hold in mind",
        Lint::WideSignature => "A function takes more parameters than a reader can hold in mind",
        Lint::StringError => "A function returns a stringly-typed `Result<_, String>`",
        Lint::DuplicateCode => "Two functions share an identical normalized body",
        Lint::HighFanOut => "A module references too many other modules",
        Lint::FeatureEnvy => "A method uses another type's members more than its own",
        Lint::LayerViolation => "A module depends on another in a lower layer",
        Lint::UnusedDep => "A declared dependency is never referenced in non-test code",
        Lint::WideDirectory => "A directory holds too many files and subdirectories",
        Lint::LongFile => "A source file exceeds the maximum line count",
    }
}

fn result_json(diagnostic: &Diagnostic) -> Value {
    let mut message = diagnostic.message.clone();
    for note in &diagnostic.notes {
        message.push('\n');
        message.push_str(note);
    }

    let mut out = json!({
        "ruleId": diagnostic.lint.name(),
        "ruleIndex": rule_index(diagnostic.lint),
        "level": "error",
        "message": {
            "text": message,
        },
        "partialFingerprints": {
            "deslop-please/v1": diagnostic.key,
        },
    });

    if let Some(location) = &diagnostic.location {
        out["locations"] = json!([location_json(location)]);
    }

    if let Picture::Circle { steps, .. } = &diagnostic.picture
        && !steps.is_empty()
    {
        let related: Vec<Value> = steps
            .iter()
            .enumerate()
            .map(|(index, step)| {
                let mut entry = location_json(&step.site);
                entry["id"] = json!(index + 1);
                entry["message"] = json!({
                    "text": format!("`{}`: {}", step.member, step.uses),
                });
                entry
            })
            .collect();
        out["relatedLocations"] = json!(related);
    }

    out
}

fn location_json(location: &Location) -> Value {
    json!({
        "physicalLocation": {
            "artifactLocation": {
                "uri": artifact_uri(&location.file),
            },
            "region": {
                "startLine": location.line.max(1),
                "startColumn": 1,
            },
        }
    })
}

fn artifact_uri(file: &str) -> String {
    let mut uri = String::with_capacity(file.len());
    for byte in file.bytes() {
        match byte {
            b'/' | b'\\' => uri.push('/'),
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                uri.push(char::from(byte));
            }
            _ => write!(uri, "%{byte:02X}").expect("writing to a String"),
        }
    }
    uri
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lint::Step;

    #[test]
    fn artifact_paths_are_relative_uris() {
        for (file, expected) in [
            ("src/plain.rs", "src/plain.rs"),
            ("src/course notes.rs", "src/course%20notes.rs"),
            ("src/course#notes.rs", "src/course%23notes.rs"),
            ("src/already%20.rs", "src/already%2520.rs"),
            ("src\\caf\u{e9}.rs", "src/caf%C3%A9.rs"),
        ] {
            let value = location_json(&Location {
                file: file.into(),
                line: 1,
            });
            assert_eq!(
                value["physicalLocation"]["artifactLocation"]["uri"], expected,
                "{file}",
            );
        }
    }

    #[test]
    fn empty_diagnostics_emit_valid_sarif_envelope_with_all_rules() {
        let rendered = render(&[]);
        let value: Value = serde_json::from_str(&rendered).unwrap();
        assert_eq!(value["$schema"], SCHEMA);
        assert_eq!(value["version"], VERSION);
        let runs = value["runs"].as_array().unwrap();
        assert_eq!(runs.len(), 1);
        let driver = &runs[0]["tool"]["driver"];
        assert_eq!(driver["name"], TOOL);
        assert_eq!(driver["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(driver["informationUri"], env!("CARGO_PKG_REPOSITORY"));
        assert_eq!(driver["rules"].as_array().unwrap().len(), ALL_LINTS.len());
        assert_eq!(runs[0]["results"], json!([]));
    }

    #[test]
    fn diagnostic_maps_rule_location_fingerprint_notes_and_related_locations() {
        let diag = Diagnostic {
            lint: Lint::ModuleCycle,
            key: "net::kinds,net::socket".into(),
            message: "2 modules in `net` use each other in a circle".into(),
            location: Some(Location {
                file: "src\\kinds.rs".into(),
                line: 10,
            }),
            notes: vec!["src/kinds.rs:10:1: `kinds` uses `socket` by 1 item: LIMIT".into()],
            picture: Picture::Circle {
                steps: vec![
                    Step {
                        member: "kinds".into(),
                        site: Location {
                            file: "src/kinds.rs".into(),
                            line: 10,
                        },
                        uses: "Stream::new uses LIMIT".into(),
                    },
                    Step {
                        member: "socket".into(),
                        site: Location {
                            file: "src/socket.rs".into(),
                            line: 5,
                        },
                        uses: "listen uses Stream".into(),
                    },
                ],
                rest: None,
            },
        };

        let value: Value = serde_json::from_str(&render(&[diag])).unwrap();
        let result = &value["runs"][0]["results"][0];
        assert_eq!(result["ruleId"], "module-cycle");
        assert_eq!(result["ruleIndex"], 0);
        assert_eq!(result["level"], "error");
        assert_eq!(
            result["message"]["text"],
            "2 modules in `net` use each other in a circle\n\
             src/kinds.rs:10:1: `kinds` uses `socket` by 1 item: LIMIT"
        );
        assert_eq!(
            result["partialFingerprints"]["deslop-please/v1"],
            "net::kinds,net::socket"
        );
        assert_eq!(
            result["locations"][0]["physicalLocation"]["artifactLocation"]["uri"],
            "src/kinds.rs"
        );
        assert_eq!(
            result["locations"][0]["physicalLocation"]["region"]["startLine"],
            10
        );
        let related = result["relatedLocations"].as_array().unwrap();
        assert_eq!(related.len(), 2);
        assert_eq!(related[0]["id"], 1);
        assert_eq!(
            related[1]["physicalLocation"]["artifactLocation"]["uri"],
            "src/socket.rs"
        );
    }

    #[test]
    fn diagnostic_without_location_omits_locations_field() {
        let diag = Diagnostic {
            lint: Lint::ShallowModule,
            key: "app::store".into(),
            message: "`app::store` is shallow".into(),
            location: None,
            notes: vec![],
            picture: Picture::Lists {
                shown: 9,
                hidden: 2,
                lists: vec![],
            },
        };

        let value: Value = serde_json::from_str(&render(&[diag])).unwrap();
        let result = &value["runs"][0]["results"][0];
        assert!(result.get("locations").is_none());
        assert!(result.get("relatedLocations").is_none());
        assert_eq!(result["ruleIndex"], rule_index(Lint::ShallowModule));
    }
}
