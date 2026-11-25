//! The names in a `scip-swift` symbol: the Swift module, the type path and the member.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Name {
    pub module: String,
    /// `AppModel`, or `AppModel::CachedCoverThumbnail`, or `AppModel::save`.
    pub path: Vec<String>,
    pub is_static: bool,
}

impl Name {
    pub fn item(&self) -> String {
        self.path.join("::")
    }

    pub fn parent(&self) -> Option<String> {
        (self.path.len() > 1).then(|| self.path[..self.path.len() - 1].join("::"))
    }
}

pub fn parse(symbol: &str, display_name: &str) -> Option<Name> {
    let (module, written) = written(symbol)?;
    let mut name = Name {
        module: module.to_owned(),
        path: match display(display_name, module) {
            Some(path) => path,
            None if written.contains('`') => return None,
            None => written_path(written),
        },
        is_static: display_name.trim_start().starts_with("static "),
    };
    if name.path.is_empty() {
        return None;
    }
    for part in &mut name.path {
        *part = part.trim_matches('`').to_owned();
        if part.is_empty() || part == "local" {
            return None;
        }
    }
    Some(name)
}

fn written(symbol: &str) -> Option<(&str, &str)> {
    let rest = symbol.strip_prefix("scip-swift ")?;
    let (_build, rest) = rest.split_once(' ')?;
    let (module, written) = rest.split_once(" . ")?;
    let written = written.strip_suffix('.').unwrap_or(written);
    (!module.is_empty() && !written.is_empty()).then_some((module, written))
}

fn written_path(written: &str) -> Vec<String> {
    written
        .split('#')
        .filter(|part| !part.is_empty())
        .map(clean_part)
        .collect()
}

fn display(display: &str, module: &str) -> Option<Vec<String>> {
    let display = display.trim();
    let display = display.strip_prefix("static ").unwrap_or(display);
    let display = display.strip_prefix(&format!("{module}."))?;
    let display = display.split(" : ").next()?;
    let display = display.split(" -> ").next()?;
    let display = without_private_names(display);
    let display = display.split('(').next()?;
    let mut path: Vec<String> = display
        .split('.')
        .filter(|part| !part.is_empty())
        .map(|part| part.trim_matches('`').to_owned())
        .collect();
    if matches!(path.last().map(String::as_str), Some("getter" | "setter")) {
        path.pop();
    }
    (!path.is_empty()).then_some(path)
}

/// The type a declaration's first parameter is written as, spelled the way
/// `scip-swift` spells it: `App.Queue` for `_ queue: Queue`, `App.State` for
/// `inout State`, `Swift.Int` for `count: Int`. `None` for a parameter that is
/// not one name — an array, a tuple, a closure — and for a declaration that
/// takes none.
pub fn first_parameter_type(display_name: &str) -> Option<String> {
    let display = without_private_names(display_name.trim());
    let open = display.find('(')?;
    let parameters = &display[open + 1..];
    let first = first_argument(&parameters[..matching_paren(parameters)?]);
    // A labeled parameter is `count: Swift.Int`; one with no external label is
    // written as the type alone, `App.admit(App.Queue)`.
    let written = first.split_once(':').map_or(first, |(_, written)| written);
    bare_type(written.split('=').next()?)
}

/// The text of a parameter list before the comma that ends its first parameter.
fn first_argument(parameters: &str) -> &str {
    let mut depth = 0i32;
    for (at, byte) in parameters.bytes().enumerate() {
        match byte {
            b'(' | b'[' | b'<' | b'{' => depth += 1,
            b')' | b']' | b'>' | b'}' => depth -= 1,
            b',' if depth == 0 => return &parameters[..at],
            _ => {}
        }
    }
    parameters
}

/// The text before the `)` that closes the parameter list it opens.
fn matching_paren(parameters: &str) -> Option<usize> {
    let mut depth = 0i32;
    for (at, byte) in parameters.bytes().enumerate() {
        match byte {
            b'(' => depth += 1,
            b')' if depth == 0 => return Some(at),
            b')' => depth -= 1,
            _ => {}
        }
    }
    None
}

/// A written parameter type without what does not name the type: its ownership
/// modifier, its attribute, its generic arguments and its optional marker.
fn bare_type(written: &str) -> Option<String> {
    let mut text = written.trim();
    while let Some((word, rest)) = text.split_once(' ') {
        if !word.starts_with('@') && !is_ownership(word) {
            break;
        }
        text = rest.trim_start();
    }
    for wrapper in ["Swift.Optional<", "Swift.ImplicitlyUnwrappedOptional<"] {
        if let Some(inner) = text.strip_prefix(wrapper) {
            return bare_type(&inner[..inner.rfind('>')?]);
        }
    }
    let base = text
        .split('<')
        .next()?
        .trim_end_matches(['?', '!', '.', ' ']);
    is_type_name(base).then(|| base.trim_matches('`').to_owned())
}

fn is_ownership(word: &str) -> bool {
    matches!(
        word,
        "inout" | "borrowing" | "consuming" | "isolated" | "some" | "any" | "__owned" | "__shared"
    )
}

/// A name the model can resolve: `Queue`, `App.Queue`, `App.Model.Nested`.
fn is_type_name(text: &str) -> bool {
    text.chars()
        .next()
        .is_some_and(|first| first.is_alphabetic() || first == '`')
        && text
            .chars()
            .all(|character| character.is_alphanumeric() || matches!(character, '_' | '`' | '.'))
}

fn clean_part(part: &str) -> String {
    let part = part.strip_suffix("()").unwrap_or(part);
    let part = part.strip_suffix('.').unwrap_or(part);
    if let Some((base, _)) = part.split_once("(+") {
        base.to_owned()
    } else {
        part.to_owned()
    }
}

/// `AppModel.(save in 7D4A…)` names the same member as `AppModel.save`.
fn without_private_names(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('(') {
        let after = &rest[at + 1..];
        let Some(end) = after.find(')') else {
            out.push_str(rest);
            return out;
        };
        let inside = &after[..end];
        if let Some((name, _)) = inside.split_once(" in ") {
            out.push_str(&rest[..at]);
            out.push_str(name);
            rest = &after[end + 1..];
        } else {
            out.push_str(&rest[..=at]);
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_types_members_nested_types_and_free_functions() {
        let cases = [
            (
                "scip-swift swiftpm ClapStudio . AppModel#",
                "ClapStudio.AppModel",
                vec!["AppModel"],
            ),
            (
                "scip-swift swiftpm ClapStudio . AppModel#save().",
                "ClapStudio.AppModel.save() -> ()",
                vec!["AppModel", "save"],
            ),
            (
                "scip-swift swiftpm ClapStudio . AppModel#CachedCoverThumbnail#image.",
                "ClapStudio.AppModel.CachedCoverThumbnail.image : __C.NSImage",
                vec!["AppModel", "CachedCoverThumbnail", "image"],
            ),
            (
                "scip-swift swiftpm ClapCore . NameSet#V#",
                "ClapCore.NameSet.V",
                vec!["NameSet", "V"],
            ),
            (
                "scip-swift swiftpm ClapBench . benchmarkBounce().",
                "ClapBench.benchmarkBounce() -> ()",
                vec!["benchmarkBounce"],
            ),
        ];
        for (symbol, display, path) in cases {
            let name = parse(symbol, display).unwrap();
            assert_eq!(name.path, path, "{symbol}");
        }
    }

    #[test]
    fn a_private_disambiguation_and_accessor_suffix_are_not_part_of_the_name() {
        let name = parse(
            "scip-swift swiftpm ClapStudio . AppModel#appVersion().",
            "ClapStudio.AppModel.(appVersion in 756981) : Swift.String",
        )
        .unwrap();
        assert_eq!(name.item(), "AppModel::appVersion");
        assert_eq!(name.parent().as_deref(), Some("AppModel"));
    }

    #[test]
    fn unique_suffixes_and_static_members_are_read() {
        let name = parse(
            "scip-swift swiftpm ClapBench . Filter(+1).",
            "ClapBench.FilterBenchEngine.columns.getter : ClapCore.LibraryColumns",
        )
        .unwrap();
        assert_eq!(name.item(), "FilterBenchEngine::columns");
        let name = parse(
            "scip-swift swiftpm ClapCore . FrontEndConfig#clap.",
            "static ClapCore.FrontEndConfig.clap : ClapCore.FrontEndConfig",
        )
        .unwrap();
        assert!(name.is_static);
    }

    #[test]
    fn locals_and_raw_usr_fallbacks_are_not_items() {
        assert!(parse("local count", "count").is_none());
        assert!(parse("scip-swift swiftpm ClapCore . `s:SiySiSdcfc`.", "Swift...").is_none());
    }

    #[test]
    fn the_first_parameter_names_the_type_a_function_works_on() {
        let cases = [
            ("App.admit(_: App.Queue) -> ()", Some("App.Queue")),
            ("App.admit(App.Queue) -> ()", Some("App.Queue")),
            (
                "App.admit(App.Queue, count: Swift.Int) throws -> ()",
                Some("App.Queue"),
            ),
            (
                "App.admit(queue: App.Queue, item: Swift.Int) -> ()",
                Some("App.Queue"),
            ),
            ("App.clear(_: inout App.State) -> ()", Some("App.State")),
            ("App.count(of: App.Queue?) -> Swift.Int", Some("App.Queue")),
            (
                "App.take(_: Swift.Optional<App.Queue>) -> ()",
                Some("App.Queue"),
            ),
            (
                "App.(writeNoise in _5ACDF8EF5915523861A3FABDACAADF13)(_: App.Queue, rate: Swift.Double) throws -> ()",
                Some("App.Queue"),
            ),
            (
                "App.Model.Nested.save(_: App.Model.Nested) -> ()",
                Some("App.Model.Nested"),
            ),
            ("App.fill(_: [Swift.Float], from: Swift.Int) -> ()", None),
            ("App.run(_: @escaping (Swift.Int) -> Swift.Int) -> ()", None),
            ("App.save() -> ()", None),
            ("App.Queue.items.getter : [Swift.Int]", None),
        ];
        for (display, expected) in cases {
            assert_eq!(
                first_parameter_type(display).as_deref(),
                expected,
                "{display}"
            );
        }
    }
}
