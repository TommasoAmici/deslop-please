//! The little Swift source reading the model needs: which body holds a name, and whether a property access only reads
//! its value. Symbol resolution belongs to the index; this only reads the text around the ranges it gives.

use crate::scip::{Position, Range};

pub struct Source {
    text: String,
    /// Comments and string bodies replaced by spaces, with newlines and byte offsets kept.
    code: Vec<u8>,
    line_starts: Vec<usize>,
    encoding: Encoding,
}

#[derive(Clone, Copy)]
pub enum Encoding {
    Utf8,
    Utf16,
    Utf32,
}

impl Encoding {
    pub fn from_scip(value: u64) -> Encoding {
        match value {
            2 => Encoding::Utf16,
            3 => Encoding::Utf32,
            _ => Encoding::Utf8,
        }
    }
}

pub struct Body {
    pub range: std::ops::Range<usize>,
}

impl Source {
    pub fn with_encoding(text: String, encoding: Encoding) -> Source {
        let code = mask_comments_and_strings(&text);
        let line_starts = std::iter::once(0)
            .chain(text.match_indices('\n').map(|(at, _)| at + 1))
            .collect();
        Source {
            text,
            code,
            line_starts,
            encoding,
        }
    }

    pub fn len(&self) -> usize {
        self.text.len()
    }

    pub fn offset(&self, position: Position) -> Option<usize> {
        let line = *self.line_starts.get(position.line as usize)?;
        let end = self
            .line_starts
            .get(position.line as usize + 1)
            .copied()
            .unwrap_or(self.text.len());
        let text = self.text.get(line..end)?;
        let column = match self.encoding {
            Encoding::Utf8 => position.column as usize,
            Encoding::Utf16 => {
                let mut units = 0usize;
                let mut at = 0usize;
                for character in text.chars() {
                    if units >= position.column as usize {
                        break;
                    }
                    units += character.len_utf16();
                    at += character.len_utf8();
                }
                if units != position.column as usize {
                    return None;
                }
                at
            }
            Encoding::Utf32 => {
                let mut count = 0usize;
                let mut at = 0usize;
                for character in text.chars() {
                    if count >= position.column as usize {
                        break;
                    }
                    count += 1;
                    at += character.len_utf8();
                }
                if count != position.column as usize {
                    return None;
                }
                at
            }
        };
        let at = line.checked_add(column)?;
        (at <= end).then_some(at)
    }

    pub fn range(&self, range: Range) -> Option<std::ops::Range<usize>> {
        Some(self.offset(range.start)?..self.offset(range.end)?)
    }

    /// The body after a declaration's name, before the next declaration at
    /// `limit`. A body is braces, or a single expression after `=`.
    pub fn body_after(&self, declaration: Range, limit: usize) -> Option<Body> {
        let mut at = self.offset(declaration.end)?.min(limit);
        let mut round = 0usize;
        let mut square = 0usize;
        while at < limit {
            match self.code[at] {
                b'(' => round += 1,
                b')' => round = round.saturating_sub(1),
                b'[' => square += 1,
                b']' => square = square.saturating_sub(1),
                b'{' if round == 0 && square == 0 => {
                    return matching_brace(&self.code, at).map(|end| Body { range: at..end });
                }
                b'=' if round == 0 && square == 0 => {
                    if self.code.get(at + 1) == Some(&b'=') {
                        at += 2;
                        continue;
                    }
                    let expression = self.next_code(at + 1, limit)?;
                    if self.code[expression] == b'{' {
                        return matching_brace(&self.code, expression).map(|end| Body {
                            range: expression..end,
                        });
                    }
                    return Some(Body {
                        range: expression..self.line_end(limit),
                    });
                }
                b';' if round == 0 && square == 0 => return None,
                _ => {}
            }
            at += 1;
        }
        None
    }

    /// The smallest of `bodies` that holds `at`.
    pub fn holder<'a>(&'a self, bodies: &'a [(Body, usize)], at: usize) -> Option<usize> {
        bodies
            .iter()
            .filter(|(body, _)| body.range.start <= at && at < body.range.end)
            .min_by_key(|(body, _)| body.range.len())
            .map(|(_, item)| *item)
    }

    /// Whether a definition range spells the declaration's source name.
    /// Compiler-generated members can point at the macro or owning type instead.
    pub fn spells(&self, range: Range, name: &str) -> bool {
        let Some(bytes) = self.range(range) else {
            return false;
        };
        let Some(written) = self.text.get(bytes) else {
            return false;
        };
        written.trim_matches('`') == name
    }

    /// Whether a use range spells the member's source name, including `queue.items`.
    pub fn spells_use(&self, range: Range, name: &str) -> bool {
        self.spells(range, name)
            || self.range(range).is_some_and(|bytes| {
                self.text
                    .get(bytes)
                    .is_some_and(|written| written.trim_matches('`').ends_with(&format!(".{name}")))
            })
    }

    /// A getter's value is only read when the code does not immediately reach
    /// through it: `queue.count` reads a number, `queue.items.append` works on
    /// the collection, and `queue.items[0]` indexes it.
    pub fn only_reads(&self, range: Range) -> bool {
        let Some(end) = self.offset(range.end) else {
            return false;
        };
        let Some(next) = self.next_code(end, self.code.len()) else {
            return true;
        };
        !matches!(self.code[next], b'.' | b'[' | b'(' | b'?' | b'!')
    }

    fn next_code(&self, mut at: usize, limit: usize) -> Option<usize> {
        while at < limit {
            if !self.code[at].is_ascii_whitespace() {
                return Some(at);
            }
            at += 1;
        }
        None
    }

    fn line_end(&self, limit: usize) -> usize {
        self.code[limit..]
            .iter()
            .position(|&byte| byte == b'\n')
            .map_or(limit, |at| limit + at)
    }
}

fn matching_brace(code: &[u8], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (at, &byte) in code.iter().enumerate().skip(open) {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(at);
                }
            }
            _ => {}
        }
    }
    None
}

/// Keep every byte's place, but hide comments and string contents from the
/// brace and delimiter scan. Nested block comments are legal Swift.
fn mask_comments_and_strings(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut code = bytes.to_vec();
    let mut at = 0usize;
    while at < bytes.len() {
        if bytes[at..].starts_with(b"//") {
            while at < bytes.len() && bytes[at] != b'\n' {
                hide(&mut code, at);
                at += 1;
            }
        } else if bytes[at..].starts_with(b"/*") {
            let mut depth = 1usize;
            hide(&mut code, at);
            hide(&mut code, at + 1);
            at += 2;
            while at < bytes.len() && depth > 0 {
                if bytes[at..].starts_with(b"/*") {
                    depth += 1;
                    hide(&mut code, at);
                    hide(&mut code, at + 1);
                    at += 2;
                } else if bytes[at..].starts_with(b"*/") {
                    depth -= 1;
                    hide(&mut code, at);
                    hide(&mut code, at + 1);
                    at += 2;
                } else {
                    hide(&mut code, at);
                    at += 1;
                }
            }
        } else if bytes[at] == b'"' {
            let triple = bytes[at..].starts_with(b"\"\"\"");
            let quote_len = if triple { 3 } else { 1 };
            for offset in 0..quote_len {
                hide(&mut code, at + offset);
            }
            at += quote_len;
            while at < bytes.len() {
                if bytes[at..].starts_with(if triple { b"\"\"\"" } else { b"\"" }) {
                    for offset in 0..quote_len {
                        hide(&mut code, at + offset);
                    }
                    at += quote_len;
                    break;
                }
                if bytes[at] == b'\\' && at + 1 < bytes.len() {
                    hide(&mut code, at);
                    hide(&mut code, at + 1);
                    at += 2;
                } else {
                    hide(&mut code, at);
                    at += 1;
                }
            }
        } else {
            at += 1;
        }
    }
    code
}

fn hide(code: &mut [u8], at: usize) {
    if code[at] != b'\n' {
        code[at] = b' ';
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(text: &str) -> Source {
        Source::with_encoding(text.to_owned(), Encoding::Utf8)
    }

    fn range(text: &str, needle: &str) -> Range {
        let at = text.find(needle).unwrap();
        let line = text[..at].bytes().filter(|&byte| byte == b'\n').count() as u32;
        let column = (at - text[..at].rfind('\n').map_or(0, |at| at + 1)) as u32;
        Range {
            start: Position { line, column },
            end: Position {
                line,
                column: column + needle.len() as u32,
            },
        }
    }

    #[test]
    fn finds_a_method_body_past_a_multiline_signature_and_ignores_braces_in_strings() {
        let text = "struct Q {\n  func save(\n    _ value: Int\n  ) -> Bool {\n    let x = \"}\"\n    return true\n  }\n}\n";
        let source = source(text);
        let body = source
            .body_after(
                range(text, "save"),
                text.find("struct Q").unwrap() + text.len(),
            )
            .unwrap();
        let held = &source.text[body.range.clone()];
        assert!(held.starts_with('{'));
        assert!(held.trim_end().ends_with("return true"));
    }

    #[test]
    fn comments_and_nested_block_comments_do_not_open_bodies() {
        let text = "/* { /* } */ } */\nfunc f() { /* } */ }\n";
        let source = source(text);
        let body = source.body_after(range(text, "f"), text.len()).unwrap();
        assert_eq!(&source.text[body.range], "{ /* } */ ");
    }

    #[test]
    fn positions_use_the_encoding_the_index_declares() {
        let text = "let 🚀 = queue.items\n";
        let utf16 = Source::with_encoding(text.to_owned(), Encoding::Utf16);
        let utf32 = Source::with_encoding(text.to_owned(), Encoding::Utf32);
        assert_eq!(
            utf16.offset(Position {
                line: 0,
                column: 15
            }),
            Some(text.find("items").unwrap())
        );
        assert_eq!(
            utf32.offset(Position {
                line: 0,
                column: 14
            }),
            Some(text.find("items").unwrap())
        );
    }

    #[test]
    fn a_definition_spells_only_the_name_written_at_its_range() {
        let text = "@Observable final class Model { var value = 0 }\n";
        let source = Source::with_encoding(text.to_owned(), Encoding::Utf8);
        assert!(source.spells(range(text, "value"), "value"));
        assert!(!source.spells(range(text, "Model"), "_value"));
        assert!(!source.spells(range(text, "@Observable"), "_value"));
    }

    #[test]
    fn a_property_read_is_a_copy_until_the_code_reaches_through_it() {
        for (text, read) in [
            ("let n = queue.count\n", true),
            ("queue.items.append(1)\n", false),
            ("let n = queue.items[0]\n", false),
            ("_ = queue.count()\n", false),
        ] {
            let source = source(text);
            let name = if text.contains("items") {
                "items"
            } else {
                "count"
            };
            assert_eq!(source.only_reads(range(text, name)), read, "{text}");
        }
    }
}
