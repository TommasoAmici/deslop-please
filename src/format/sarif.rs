//! SARIF v2.1.0 diagnostic formatting for CI and static analysis aggregators
//! (GitHub Code Scanning, SonarQube, reviewdog, etc.).

use std::fmt::Write as _;

use serde_json::{Value, json};

use crate::TOOL;
use crate::lint::{
    Diagnostic, EncapsulationLint, GraphLint, HygieneLint, Lint, Location, Picture, SizeLint,
    SurfaceLint,
};

const SCHEMA: &str = "https://json.schemastore.org/sarif-2.1.0.json";
const VERSION: &str = "2.1.0";

const ALL_LINTS: [Lint; 31] = [
    Lint::Graph(GraphLint::ModuleCycle),
    Lint::Graph(GraphLint::Tangle),
    Lint::Graph(GraphLint::HighFanOut),
    Lint::Graph(GraphLint::GodHub),
    Lint::Graph(GraphLint::LayerViolation),
    Lint::Graph(GraphLint::UnusedDep),
    Lint::Graph(GraphLint::ChangeSpread),
    Lint::Encapsulation(EncapsulationLint::FieldLeak),
    Lint::Encapsulation(EncapsulationLint::SharedMutableBag),
    Lint::Encapsulation(EncapsulationLint::ShallowModule),
    Lint::Encapsulation(EncapsulationLint::ScatteredImpl),
    Lint::Encapsulation(EncapsulationLint::FeatureEnvy),
    Lint::Encapsulation(EncapsulationLint::PassThrough),
    Lint::Encapsulation(EncapsulationLint::ReexportSprawl),
    Lint::Encapsulation(EncapsulationLint::CallOrder),
    Lint::Size(SizeLint::WideType),
    Lint::Size(SizeLint::WideModule),
    Lint::Size(SizeLint::WideStruct),
    Lint::Size(SizeLint::WideEnum),
    Lint::Size(SizeLint::LongFunction),
    Lint::Size(SizeLint::WideSignature),
    Lint::Size(SizeLint::PositionalTrap),
    Lint::Size(SizeLint::WideDirectory),
    Lint::Size(SizeLint::LongFile),
    Lint::Surface(SurfaceLint::UnusedPub),
    Lint::Surface(SurfaceLint::TestOnlyPub),
    Lint::Surface(SurfaceLint::OverPublic),
    Lint::Surface(SurfaceLint::SingleImplTrait),
    Lint::Hygiene(HygieneLint::StringError),
    Lint::Hygiene(HygieneLint::DuplicateCode),
    Lint::Hygiene(HygieneLint::DuplicateStruct),
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
        Lint::Graph(GraphLint::ModuleCycle) => "Modules depend on each other in a cycle",
        Lint::Graph(GraphLint::Tangle) => {
            "Items depend on each other in a cycle across modules or beyond what fits in mind"
        }
        Lint::Graph(GraphLint::HighFanOut) => "A module references too many other modules",
        Lint::Graph(GraphLint::GodHub) => "A module both many modules use and that uses many",
        Lint::Graph(GraphLint::LayerViolation) => "A module depends on another in a lower layer",
        Lint::Graph(GraphLint::UnusedDep) => "A declared dependency is never referenced in non-test code",
        Lint::Graph(GraphLint::ChangeSpread) => {
            "Two modules the history always changes together, that name nothing in common"
        }
        Lint::Encapsulation(EncapsulationLint::FieldLeak) => {
            "A struct field is manipulated outside its owning type"
        }
        Lint::Encapsulation(EncapsulationLint::SharedMutableBag) => {
            "A struct's fields are mutated from several other modules"
        }
        Lint::Encapsulation(EncapsulationLint::ShallowModule) => {
            "A module exposes too much of its interface relative to what it hides"
        }
        Lint::Encapsulation(EncapsulationLint::ScatteredImpl) => {
            "A type's inherent `impl` blocks are spread across modules"
        }
        Lint::Encapsulation(EncapsulationLint::FeatureEnvy) => {
            "A method uses another type's members more than its own"
        }
        Lint::Encapsulation(EncapsulationLint::PassThrough) => {
            "A shim adds a second path to code without adding abstraction"
        }
        Lint::Encapsulation(EncapsulationLint::ReexportSprawl) => {
            "A module re-exports another module's or crate's whole namespace"
        }
        Lint::Encapsulation(EncapsulationLint::CallOrder) => {
            "Two free functions are always called in one order on the same value"
        }
        Lint::Size(SizeLint::WideType) => "A type shows more working functions than a reader can hold in mind",
        Lint::Size(SizeLint::WideModule) => "A leaf module shows more working functions than a reader can scan",
        Lint::Size(SizeLint::WideStruct) => "A struct has more fields than a reader can scan at once",
        Lint::Size(SizeLint::WideEnum) => "An enum has more variants than a reader can scan at once",
        Lint::Size(SizeLint::LongFunction) => {
            "A function is longer or branches more than a reader can hold in mind"
        }
        Lint::Size(SizeLint::WideSignature) => {
            "A function takes more parameters than a reader can hold in mind"
        }
        Lint::Size(SizeLint::PositionalTrap) => {
            "A function takes several parameters whose type and order decide their meaning"
        }
        Lint::Size(SizeLint::WideDirectory) => "A directory holds too many files and subdirectories",
        Lint::Size(SizeLint::LongFile) => "A source file exceeds the maximum line count",
        Lint::Surface(SurfaceLint::UnusedPub) => "A `pub` item has no non-test callers outside its library",
        Lint::Surface(SurfaceLint::TestOnlyPub) => "A restricted item is only referenced by tests",
        Lint::Surface(SurfaceLint::OverPublic) => "A `pub` item is only used inside its own library",
        Lint::Surface(SurfaceLint::SingleImplTrait) => "A trait has one implementation or fewer",
        Lint::Hygiene(HygieneLint::StringError) => "A function returns a stringly-typed `Result<_, String>`",
        Lint::Hygiene(HygieneLint::DuplicateCode) => "Two functions share an identical normalized body",
        Lint::Hygiene(HygieneLint::DuplicateStruct) => "Two structs have the same fields",
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
            lint: Lint::Graph(GraphLint::ModuleCycle),
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
            lint: Lint::Encapsulation(EncapsulationLint::ShallowModule),
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
        assert_eq!(result["ruleIndex"], rule_index(Lint::Encapsulation(EncapsulationLint::ShallowModule)));
    }
}
