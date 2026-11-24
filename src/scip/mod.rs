//! A SCIP index (github.com/sourcegraph/scip, `scip.proto`): the documents, and
//! in each every name written there with the symbol it resolves to. Only the
//! fields this tool reads are decoded; the rest are skipped.

mod protobuf;
mod symbol;

pub use protobuf::Malformed;
pub use symbol::{Impl, Kind, Place, Symbol};

use std::borrow::Cow;

use protobuf::{Fields, Value};

pub struct Index<'a> {
    pub documents: Vec<Document<'a>>,
}

pub struct Document<'a> {
    /// Relative to the indexed workspace's root, `/`-separated on every
    /// platform: see [`with_slashes`].
    pub path: Cow<'a, str>,
    /// `scip.proto`'s `PositionEncoding`; zero is treated as UTF-8.
    pub position_encoding: u64,
    pub occurrences: Vec<Occurrence<'a>>,
    /// What the document says about each symbol it defines.
    pub symbols: Vec<SymbolInformation<'a>>,
}

pub struct SymbolInformation<'a> {
    pub symbol: &'a str,
    /// `scip.proto`'s `SymbolInformation.Kind`: what the symbol is (struct, field, method...).
    pub kind: u64,
    /// The declaration as written, body left out: `pub(crate) fn serve(&self)`, `pub mod log`.
    pub signature: &'a str,
    /// The resolved name an indexer gives the symbol, when it has one.
    pub display_name: &'a str,
    /// The symbol that lexically contains this one, when the indexer records it.
    pub enclosing_symbol: &'a str,
    pub relationships: Vec<Relationship<'a>>,
}

#[derive(Debug)]
pub struct Relationship<'a> {
    pub symbol: &'a str,
    pub implementation: bool,
}

pub struct Occurrence<'a> {
    pub range: Range,
    pub symbol: &'a str,
    pub definition: bool,
    pub write: bool,
    pub test: bool,
    /// For a definition: the whole item, body included.
    pub enclosing: Option<Range>,
}

/// Zero-based lines and columns; `end` is exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Position {
    pub line: u32,
    pub column: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

impl Range {
    pub fn holds(self, other: Range) -> bool {
        self.start <= other.start && other.end <= self.end
    }
}

// scip.proto's field numbers.
const INDEX_DOCUMENTS: u32 = 2;
const DOCUMENT_RELATIVE_PATH: u32 = 1;
const DOCUMENT_OCCURRENCES: u32 = 2;
const DOCUMENT_SYMBOLS: u32 = 3;
const DOCUMENT_TEXT: u32 = 5;
const DOCUMENT_POSITION_ENCODING: u32 = 6;
const SYMBOL_INFORMATION_SYMBOL: u32 = 1;
const SYMBOL_INFORMATION_KIND: u32 = 5;
const SYMBOL_INFORMATION_DISPLAY_NAME: u32 = 6;
const SYMBOL_INFORMATION_SIGNATURE: u32 = 7;
const SYMBOL_INFORMATION_ENCLOSING_SYMBOL: u32 = 8;
const SYMBOL_INFORMATION_RELATIONSHIPS: u32 = 4;
const RELATIONSHIP_SYMBOL: u32 = 1;
const RELATIONSHIP_IMPLEMENTATION: u32 = 3;
const OCCURRENCE_RANGE: u32 = 1;
const OCCURRENCE_SYMBOL: u32 = 2;
const OCCURRENCE_SYMBOL_ROLES: u32 = 3;
const OCCURRENCE_ENCLOSING_RANGE: u32 = 7;
const OCCURRENCE_SINGLE_LINE_RANGE: u32 = 8;
const OCCURRENCE_MULTI_LINE_RANGE: u32 = 9;
const OCCURRENCE_SINGLE_LINE_ENCLOSING_RANGE: u32 = 10;
const OCCURRENCE_MULTI_LINE_ENCLOSING_RANGE: u32 = 11;
const ROLE_DEFINITION: u64 = 1;
const ROLE_WRITE: u64 = 4;
const ROLE_TEST: u64 = 32;

pub fn read(bytes: &[u8]) -> Result<Index<'_>, Malformed> {
    let mut documents = Vec::new();
    for field in Fields::new(bytes) {
        if let (INDEX_DOCUMENTS, value) = field? {
            documents.push(document(protobuf::bytes(value)?)?);
        }
    }
    Ok(Index { documents })
}

fn document(bytes: &[u8]) -> Result<Document<'_>, Malformed> {
    let mut path = "";
    let mut position_encoding = 0;
    let mut occurrences = Vec::new();
    let mut symbols = Vec::new();
    for field in Fields::new(bytes) {
        match field? {
            (DOCUMENT_RELATIVE_PATH, value) => path = protobuf::string(value)?,
            (DOCUMENT_OCCURRENCES, value) => occurrences.push(occurrence(protobuf::bytes(value)?)?),
            (DOCUMENT_SYMBOLS, value) => {
                symbols.push(symbol_information(protobuf::bytes(value)?)?);
            }
            (DOCUMENT_POSITION_ENCODING, Value::Varint(encoding)) => position_encoding = encoding,
            _ => {}
        }
    }
    Ok(Document {
        path: with_slashes(path, std::path::MAIN_SEPARATOR),
        position_encoding,
        occurrences,
        symbols,
    })
}

/// `path`, written with the platform's `separator`, with `/` in its place.
/// `scip.proto` asks for `/` "including on Windows", but rust-analyzer writes
/// `\` there. Only the platform's separator is replaced: elsewhere `\` is a
/// character a file's name can hold.
pub fn with_slashes(path: &str, separator: char) -> Cow<'_, str> {
    if separator != '/' && path.contains(separator) {
        Cow::Owned(path.replace(separator, "/"))
    } else {
        Cow::Borrowed(path)
    }
}

fn symbol_information(bytes: &[u8]) -> Result<SymbolInformation<'_>, Malformed> {
    let mut information = SymbolInformation {
        symbol: "",
        kind: 0,
        signature: "",
        display_name: "",
        enclosing_symbol: "",
        relationships: Vec::new(),
    };
    for field in Fields::new(bytes) {
        match field? {
            (SYMBOL_INFORMATION_SYMBOL, value) => information.symbol = protobuf::string(value)?,
            (SYMBOL_INFORMATION_KIND, Value::Varint(kind)) => information.kind = kind,
            (SYMBOL_INFORMATION_DISPLAY_NAME, value) => {
                information.display_name = protobuf::string(value)?;
            }
            (SYMBOL_INFORMATION_ENCLOSING_SYMBOL, value) => {
                information.enclosing_symbol = protobuf::string(value)?;
            }
            (SYMBOL_INFORMATION_RELATIONSHIPS, value) => {
                information
                    .relationships
                    .push(relationship(protobuf::bytes(value)?)?);
            }
            (SYMBOL_INFORMATION_SIGNATURE, value) => {
                for field in Fields::new(protobuf::bytes(value)?) {
                    if let (DOCUMENT_TEXT, text) = field? {
                        information.signature = protobuf::string(text)?;
                    }
                }
            }
            _ => {}
        }
    }
    Ok(information)
}

fn relationship(bytes: &[u8]) -> Result<Relationship<'_>, Malformed> {
    let mut relationship = Relationship {
        symbol: "",
        implementation: false,
    };
    for field in Fields::new(bytes) {
        match field? {
            (RELATIONSHIP_SYMBOL, value) => relationship.symbol = protobuf::string(value)?,
            (RELATIONSHIP_IMPLEMENTATION, Value::Varint(value)) => {
                relationship.implementation = value != 0;
            }
            _ => {}
        }
    }
    Ok(relationship)
}

fn occurrence(bytes: &[u8]) -> Result<Occurrence<'_>, Malformed> {
    let mut range = Vec::new();
    let mut enclosing = Vec::new();
    let mut written_range = None;
    let mut written_enclosing = None;
    let mut symbol = "";
    let mut roles = 0;
    for field in Fields::new(bytes) {
        match field? {
            (OCCURRENCE_RANGE, value) => protobuf::integers(value, &mut range)?,
            (OCCURRENCE_SYMBOL, value) => symbol = protobuf::string(value)?,
            (OCCURRENCE_SYMBOL_ROLES, Value::Varint(value)) => roles = value,
            (OCCURRENCE_ENCLOSING_RANGE, value) => protobuf::integers(value, &mut enclosing)?,
            (OCCURRENCE_SINGLE_LINE_RANGE, value) => {
                written_range = typed_range(protobuf::bytes(value)?, false);
            }
            (OCCURRENCE_MULTI_LINE_RANGE, value) => {
                written_range = typed_range(protobuf::bytes(value)?, true);
            }
            (OCCURRENCE_SINGLE_LINE_ENCLOSING_RANGE, value) => {
                written_enclosing = typed_range(protobuf::bytes(value)?, false);
            }
            (OCCURRENCE_MULTI_LINE_ENCLOSING_RANGE, value) => {
                written_enclosing = typed_range(protobuf::bytes(value)?, true);
            }
            _ => {}
        }
    }
    Ok(Occurrence {
        range: written_range
            .or_else(|| to_range(&range))
            .ok_or(Malformed("an occurrence's range"))?,
        symbol,
        definition: roles & ROLE_DEFINITION != 0,
        write: roles & ROLE_WRITE != 0,
        test: roles & ROLE_TEST != 0,
        enclosing: written_enclosing.or_else(|| to_range(&enclosing)),
    })
}

fn typed_range(bytes: &[u8], multiline: bool) -> Option<Range> {
    let mut numbers = [0u32; 4];
    for field in Fields::new(bytes) {
        if let (number, Value::Varint(value)) = field.ok()? {
            let slot = match (multiline, number) {
                (false, 1) => 0,
                (false, 2) => 1,
                (false, 3) => 2,
                (true, 1) => 0,
                (true, 2) => 1,
                (true, 3) => 2,
                (true, 4) => 3,
                _ => continue,
            };
            numbers[slot] = u32::try_from(value).ok()?;
        }
    }
    let at = |line, column| Position { line, column };
    if multiline {
        Some(Range {
            start: at(numbers[0], numbers[1]),
            end: at(numbers[2], numbers[3]),
        })
    } else {
        Some(Range {
            start: at(numbers[0], numbers[1]),
            end: at(numbers[0], numbers[2]),
        })
    }
}

/// SCIP writes `[line, start, end]` for a range on one line, `[start line, start, end line, end]` otherwise.
fn to_range(numbers: &[u64]) -> Option<Range> {
    let numbers: Vec<u32> = numbers
        .iter()
        .map(|&n| u32::try_from(n).ok())
        .collect::<Option<_>>()?;
    let at = |line, column| Position { line, column };
    match numbers[..] {
        [line, start, end] => Some(Range {
            start: at(line, start),
            end: at(line, end),
        }),
        [start_line, start, end_line, end] => Some(Range {
            start: at(start_line, start),
            end: at(end_line, end),
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_has_slashes_whichever_separator_its_platform_writes() {
        assert_eq!(with_slashes(r"src\lint\a.rs", '\\'), "src/lint/a.rs");
        assert_eq!(with_slashes("src/lint/a.rs", '\\'), "src/lint/a.rs");
        assert_eq!(with_slashes("src/lint/a.rs", '/'), "src/lint/a.rs");
        // A Unix file's name can hold a backslash.
        assert_eq!(with_slashes(r"src/a\b.rs", '/'), r"src/a\b.rs");
        assert!(matches!(
            with_slashes("src/lib.rs", '\\'),
            Cow::Borrowed("src/lib.rs")
        ));
    }

    #[test]
    fn typed_ranges_default_the_fields_protobuf_omits_at_zero() {
        assert_eq!(
            typed_range(&[0x10, 7, 0x18, 15], false),
            Some(Range {
                start: Position { line: 0, column: 7 },
                end: Position {
                    line: 0,
                    column: 15
                },
            })
        );
        assert_eq!(
            typed_range(&[0x08, 1, 0x10, 2, 0x18, 3, 0x20, 4], true),
            Some(Range {
                start: Position { line: 1, column: 2 },
                end: Position { line: 3, column: 4 },
            })
        );
        assert_eq!(
            typed_range(&[0x18, 15], false),
            Some(Range {
                start: Position { line: 0, column: 0 },
                end: Position {
                    line: 0,
                    column: 15
                },
            })
        );
    }
}
