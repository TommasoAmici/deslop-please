//! A `scip-swift` index and the package's files, turned into the same plain model the lints read for Rust.
//!
//! SwiftPM targets are the packages in the tree. Inside a target, directories
//! are the modules: Swift has no per-file module boundary, but a directory is
//! still the unit a reader lists and the place a feature is kept. References
//! between those directories form the cycle the Rust lints call a module cycle.

use std::collections::HashMap;

use crate::Error;
use crate::model::{
    File, FileId, Item, ItemId, ItemKind, Model, NodeId, NodeKind, Reference, Tree, Wrapper,
};
use crate::scip::{Index, SymbolInformation};
use crate::swift::Workspace;
use crate::swift::source::{Body, Encoding, Source};
use crate::swift::symbol::{Name, parse};

pub fn build(index: &Index, workspace: &Workspace) -> Result<Model, Error> {
    let mut builder = Builder::new(workspace)?;
    builder.read_symbols(index)?;
    builder.read_documents(index)?;
    Ok(builder.finish())
}

struct Named<'a> {
    information: &'a SymbolInformation<'a>,
    name: Name,
    file: FileId,
    module: NodeId,
}

struct Builder<'a> {
    workspace: &'a Workspace,
    tree: Tree,
    files: Vec<File>,
    items: Vec<Item>,
    references: Vec<Reference>,
    wrappers: HashMap<ItemId, Wrapper>,
    files_by_path: HashMap<String, FileId>,
    targets: HashMap<String, NodeId>,
    /// A target's type or free item, by the name Swift resolves.
    items_by_name: HashMap<(String, String), ItemId>,
    /// Every indexed symbol, including synthesized accessors and locals we do not keep.
    symbols: HashMap<String, ItemId>,
    kinds: HashMap<String, u64>,
}

impl<'a> Builder<'a> {
    fn new(workspace: &'a Workspace) -> Result<Builder<'a>, Error> {
        let mut tree = Tree::new();
        let mut targets = HashMap::new();
        let mut files = Vec::new();
        let mut files_by_path = HashMap::new();
        for target in &workspace.targets {
            let package = tree.child(tree.root(), NodeKind::Package, &target.name);
            if target.test {
                tree.node_mut(package).test = true;
            }
            targets.insert(target.name.clone(), package);
            for source in &target.sources {
                let path = join(&target.directory, source);
                let module = directory_module(&mut tree, package, parent(source));
                let text = std::fs::read_to_string(workspace.root.join(&path)).map_err(|e| {
                    Error::new(format!("{}: {e}", workspace.root.join(&path).display()))
                })?;
                let file = FileId::new(files.len());
                files_by_path.insert(path.clone(), file);
                files.push(File {
                    path,
                    module,
                    test: target.test,
                    lines: text.lines().count(),
                });
            }
        }
        Ok(Builder {
            workspace,
            tree,
            files,
            items: Vec::new(),
            references: Vec::new(),
            wrappers: HashMap::new(),
            files_by_path,
            targets,
            items_by_name: HashMap::new(),
            symbols: HashMap::new(),
            kinds: HashMap::new(),
        })
    }

    fn read_symbols(&mut self, index: &Index) -> Result<(), Error> {
        let mut named = Vec::new();
        for document in &index.documents {
            let Some(&file) = self.files_by_path.get(&*document.path) else {
                continue;
            };
            let module = self.files[file.index()].module;
            for information in &document.symbols {
                let Some(name) = parse(information.symbol, information.display_name) else {
                    continue;
                };
                if !self.targets.contains_key(&name.module) {
                    continue;
                }
                named.push(Named {
                    information,
                    name,
                    file,
                    module,
                });
            }
        }
        // A nested type is placed after its outer type, whichever document
        // either was declared in.
        named.sort_by_key(|named| {
            (
                !is_type(named.information.kind),
                named.name.path.len(),
                named.name.module.clone(),
                named.name.path.clone(),
            )
        });
        for named in &named {
            if is_type(named.information.kind) {
                self.add_type(named);
            }
        }
        for named in &named {
            if !is_type(named.information.kind) && !is_accessor(named.information.kind) {
                self.add_item(named);
            }
        }
        for named in &named {
            if is_accessor(named.information.kind) {
                self.add_accessor(named);
            }
        }
        Ok(())
    }

    fn add_type(&mut self, named: &Named) {
        let parent = named
            .name
            .parent()
            .and_then(|parent| self.item(&named.name.module, &parent));
        let id = self.push(named, ItemKind::Type, parent, false);
        self.items_by_name
            .insert((named.name.module.clone(), named.name.item()), id);
        self.symbols.insert(named.information.symbol.to_owned(), id);
        self.kinds
            .insert(named.information.symbol.to_owned(), named.information.kind);
    }

    fn add_item(&mut self, named: &Named) {
        let parent = named
            .name
            .parent()
            .and_then(|parent| self.item(&named.name.module, &parent));
        let kind = item_kind(named.information.kind);
        if kind == ItemKind::Other {
            return;
        }
        // A member of a type the package does not define is outside this model.
        if named.name.path.len() > 1 && parent.is_none() {
            return;
        }
        let takes_self = kind == ItemKind::Function
            && parent.is_some()
            && !named.name.is_static
            && !matches!(
                named.information.kind,
                KIND_CONSTRUCTOR | KIND_STATIC_METHOD
            );
        let id = self.push(named, kind, parent, takes_self);
        self.items_by_name
            .insert((named.name.module.clone(), named.name.item()), id);
        self.symbols.insert(named.information.symbol.to_owned(), id);
        self.kinds
            .insert(named.information.symbol.to_owned(), named.information.kind);
    }

    fn add_accessor(&mut self, named: &Named) {
        let Some(property) = self.item(&named.name.module, &named.name.item()) else {
            return;
        };
        if self.items[property.index()].kind != ItemKind::Field {
            return;
        }
        let parent = self.items[property.index()].parent;
        let takes_self = parent.is_some() && !named.name.is_static;
        let accessor = if named.information.kind == KIND_SETTER {
            "setter"
        } else {
            "getter"
        };
        let mut named = Named {
            information: named.information,
            name: named.name.clone(),
            file: named.file,
            module: named.module,
        };
        named.name.path.push(accessor.to_owned());
        let id = self.push(&named, ItemKind::Function, parent, takes_self);
        self.symbols.insert(named.information.symbol.to_owned(), id);
        self.kinds
            .insert(named.information.symbol.to_owned(), named.information.kind);
        self.wrappers.insert(
            id,
            Wrapper::Accessor {
                lent: vec![property],
                copied: Vec::new(),
                to_read: named.information.kind == KIND_GETTER,
            },
        );
    }

    fn push(
        &mut self,
        named: &Named,
        kind: ItemKind,
        parent: Option<ItemId>,
        takes_self: bool,
    ) -> ItemId {
        let trait_impl = named
            .information
            .relationships
            .iter()
            .any(|relationship| relationship.implementation);
        self.items.push(Item {
            name: named.name.item(),
            owner: named.module,
            module: None,
            file: Some(named.file),
            kind,
            parent,
            trait_impl,
            takes_self,
            holds_nothing: false,
        });
        ItemId::new(self.items.len() - 1)
    }

    fn item(&self, module: &str, name: &str) -> Option<ItemId> {
        self.items_by_name
            .get(&(module.to_owned(), name.to_owned()))
            .copied()
    }

    fn read_documents(&mut self, index: &Index) -> Result<(), Error> {
        for document in &index.documents {
            let Some(&file) = self.files_by_path.get(&*document.path) else {
                continue;
            };
            let text = std::fs::read_to_string(self.workspace.root.join(&*document.path)).map_err(
                |e| {
                    Error::new(format!(
                        "{}: {e}",
                        self.workspace.root.join(&*document.path).display()
                    ))
                },
            )?;
            let source =
                Source::with_encoding(text, Encoding::from_scip(document.position_encoding));
            let mut definitions: Vec<(
                usize,
                ItemId,
                crate::scip::Range,
                Option<crate::scip::Range>,
            )> = document
                .occurrences
                .iter()
                .filter(|occurrence| occurrence.definition)
                .filter_map(|occurrence| {
                    let item = *self.symbols.get(occurrence.symbol)?;
                    let start = source.offset(occurrence.range.start)?;
                    Some((start, item, occurrence.range, occurrence.enclosing))
                })
                .collect();
            definitions.sort_by_key(|(start, _, _, _)| *start);
            let mut bodies = Vec::new();
            for (index, (_, item, range, enclosing)) in definitions.iter().enumerate() {
                let model_item = &self.items[item.index()];
                if !matches!(model_item.kind, ItemKind::Type | ItemKind::Function) {
                    continue;
                }
                let limit = definitions
                    .get(index + 1)
                    .map(|(start, _, _, _)| *start)
                    .unwrap_or(source.len());
                let body = enclosing
                    .and_then(|enclosing| source.range(enclosing))
                    .map(|range| Body { range })
                    .or_else(|| source.body_after(*range, limit));
                if let Some(body) = body {
                    bodies.push((body, item.index()));
                }
            }
            let module = self.files[file.index()].module;
            for occurrence in &document.occurrences {
                if occurrence.definition {
                    continue;
                }
                let Some(&item) = self.symbols.get(occurrence.symbol) else {
                    continue;
                };
                let Some(range) = source.range(occurrence.range) else {
                    continue;
                };
                let kind = self.kinds.get(occurrence.symbol).copied().unwrap_or(0);
                let copies = !occurrence.write
                    && matches!(kind, KIND_GETTER | KIND_PROPERTY | KIND_STATIC_PROPERTY)
                    && source.only_reads(occurrence.range);
                let test = self.files[file.index()].test || occurrence.test;
                self.references.push(Reference {
                    file,
                    line: occurrence.range.start.line + 1,
                    from: module,
                    holder: source.holder(&bodies, range.start).map(ItemId::new),
                    item,
                    copies,
                    test,
                });
            }
        }
        Ok(())
    }

    fn finish(self) -> Model {
        Model {
            tree: self.tree,
            files: self.files,
            items: self.items,
            references: self.references,
            wrappers: self.wrappers,
        }
    }
}

fn directory_module(tree: &mut Tree, package: NodeId, directory: &str) -> NodeId {
    let mut node = package;
    for part in directory.split('/').filter(|part| !part.is_empty()) {
        node = tree.child(node, NodeKind::Module, part);
    }
    node
}

fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
}

fn join(directory: &str, source: &str) -> String {
    if directory.is_empty() {
        source.to_owned()
    } else {
        format!("{directory}/{source}")
    }
}

const KIND_CLASS: u64 = 7;
const KIND_CONSTRUCTOR: u64 = 9;
const KIND_ENUM: u64 = 11;
const KIND_ENUM_MEMBER: u64 = 12;
const KIND_FIELD: u64 = 15;
const KIND_FUNCTION: u64 = 17;
const KIND_GETTER: u64 = 18;
const KIND_INTERFACE: u64 = 21;
const KIND_METHOD: u64 = 26;
const KIND_PROPERTY: u64 = 41;
const KIND_PROTOCOL: u64 = 42;
const KIND_SUBSCRIPT: u64 = 47;
const KIND_SETTER: u64 = 45;
const KIND_STRUCT: u64 = 49;
const KIND_TRAIT: u64 = 53;
const KIND_TYPE_ALIAS: u64 = 55;
const KIND_ABSTRACT_METHOD: u64 = 66;
const KIND_TRAIT_METHOD: u64 = 70;
const KIND_ACCESSOR: u64 = 72;
const KIND_STATIC_FIELD: u64 = 79;
const KIND_STATIC_METHOD: u64 = 80;
const KIND_STATIC_PROPERTY: u64 = 81;
const KIND_STATIC_VARIABLE: u64 = 82;
const KIND_VARIABLE: u64 = 61;

fn is_type(kind: u64) -> bool {
    matches!(
        kind,
        KIND_CLASS
            | KIND_ENUM
            | KIND_INTERFACE
            | KIND_STRUCT
            | KIND_TRAIT
            | KIND_PROTOCOL
            | KIND_TYPE_ALIAS
    )
}

fn is_accessor(kind: u64) -> bool {
    matches!(kind, KIND_GETTER | KIND_SETTER | KIND_ACCESSOR)
}

fn item_kind(kind: u64) -> ItemKind {
    match kind {
        KIND_FIELD | KIND_PROPERTY | KIND_STATIC_FIELD | KIND_STATIC_PROPERTY => ItemKind::Field,
        KIND_ENUM_MEMBER => ItemKind::Variant,
        KIND_FUNCTION | KIND_METHOD | KIND_CONSTRUCTOR | KIND_ABSTRACT_METHOD
        | KIND_TRAIT_METHOD | KIND_GETTER | KIND_SETTER | KIND_ACCESSOR | KIND_STATIC_METHOD
        | KIND_SUBSCRIPT => ItemKind::Function,
        8 | KIND_STATIC_VARIABLE | KIND_VARIABLE => ItemKind::Constant,
        _ => ItemKind::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lint;
    use crate::scip::{Document, Occurrence, Position};
    use crate::swift::workspace::Target;
    use std::borrow::Cow;

    const QUEUE: &str = "struct Queue {\n    var items: [Int] = []\n    let limit: Int\n    func push(_ value: Int) { items.append(value) }\n    func count() -> Int { items.count }\n}\n";
    const POLICY: &str = "func admit(_ queue: Queue) {\n    queue.items.append(1)\n}\n";

    fn at(text: &str, needle: &str) -> crate::scip::Range {
        let start = text.find(needle).unwrap();
        let line = text[..start].bytes().filter(|&byte| byte == b'\n').count() as u32;
        let column = (start - text[..start].rfind('\n').map_or(0, |at| at + 1)) as u32;
        crate::scip::Range {
            start: Position { line, column },
            end: Position {
                line,
                column: column + needle.len() as u32,
            },
        }
    }

    fn at_nth(text: &str, needle: &str, nth: usize) -> crate::scip::Range {
        let start = text.match_indices(needle).nth(nth).unwrap().0;
        let line = text[..start].bytes().filter(|&byte| byte == b'\n').count() as u32;
        let column = (start - text[..start].rfind('\n').map_or(0, |at| at + 1)) as u32;
        crate::scip::Range {
            start: Position { line, column },
            end: Position {
                line,
                column: column + needle.len() as u32,
            },
        }
    }

    fn symbol(
        symbol: &'static str,
        display: &'static str,
        kind: u64,
    ) -> SymbolInformation<'static> {
        SymbolInformation {
            symbol,
            kind,
            signature: "",
            display_name: display,
            enclosing_symbol: "",
            relationships: Vec::new(),
        }
    }

    fn occurrence(
        symbol: &'static str,
        range: crate::scip::Range,
        definition: bool,
    ) -> Occurrence<'static> {
        Occurrence {
            symbol,
            range,
            definition,
            write: false,
            test: false,
            enclosing: None,
        }
    }

    fn queue_document(policy: &str) -> Index<'static> {
        let items = "scip-swift swiftpm App . Queue#items.";
        let items_getter = "scip-swift swiftpm App . Queue#items().";
        let limit = "scip-swift swiftpm App . Queue#limit.";
        let push = "scip-swift swiftpm App . Queue#push().";
        let count = "scip-swift swiftpm App . Queue#count().";
        let admit = "scip-swift swiftpm App . admit().";
        let queue = Document {
            path: Cow::Borrowed("Sources/App/Model/Queue.swift"),
            position_encoding: 1,
            symbols: vec![
                symbol("scip-swift swiftpm App . Queue#", "App.Queue", KIND_STRUCT),
                symbol(items, "App.Queue.items : [Swift.Int]", KIND_PROPERTY),
                symbol(limit, "App.Queue.limit : Swift.Int", KIND_PROPERTY),
                symbol(push, "App.Queue.push(_: Swift.Int) -> ()", KIND_METHOD),
                symbol(count, "App.Queue.count() -> Swift.Int", KIND_METHOD),
                symbol(
                    items_getter,
                    "App.Queue.items.getter : [Swift.Int]",
                    KIND_GETTER,
                ),
            ],
            occurrences: vec![
                occurrence("scip-swift swiftpm App . Queue#", at(QUEUE, "Queue"), true),
                occurrence(items, at(QUEUE, "items"), true),
                occurrence(limit, at(QUEUE, "limit"), true),
                occurrence(push, at(QUEUE, "push"), true),
                occurrence(count, at(QUEUE, "count"), true),
                occurrence(items_getter, at_nth(QUEUE, "items", 1), false),
                occurrence(items_getter, at_nth(QUEUE, "items", 2), false),
            ],
        };
        let policy = Document {
            path: Cow::Borrowed("Sources/App/Policy/Admit.swift"),
            position_encoding: 1,
            symbols: vec![symbol(
                admit,
                "App.admit(_: App.Queue) -> ()",
                KIND_FUNCTION,
            )],
            occurrences: vec![
                occurrence(admit, at(policy, "admit"), true),
                occurrence(items_getter, at(policy, "queue.items"), false),
            ],
        };
        Index {
            documents: vec![queue, policy],
        }
    }

    fn workspace(test: &str) -> (Workspace, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!("deslop-swift-{}-{test}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Sources/App/Model")).unwrap();
        std::fs::create_dir_all(root.join("Sources/App/Policy")).unwrap();
        std::fs::write(root.join("Sources/App/Model/Queue.swift"), QUEUE).unwrap();
        std::fs::write(root.join("Sources/App/Policy/Admit.swift"), POLICY).unwrap();
        let workspace = Workspace {
            root: root.clone(),
            build_directory: root.join(".build"),
            targets: vec![Target {
                name: "App".to_owned(),
                directory: "Sources/App".to_owned(),
                sources: vec![
                    "Model/Queue.swift".to_owned(),
                    "Policy/Admit.swift".to_owned(),
                ],
                test: false,
            }],
        };
        (workspace, root)
    }

    fn keys(model: &Model) -> Vec<String> {
        lint::check(model, Some(model.tree.root()))
            .into_iter()
            .map(|diagnostic| format!("{} {}", diagnostic.lint.name(), diagnostic.key))
            .collect()
    }

    #[test]
    fn a_swift_property_reached_through_its_getter_leaks_but_a_read_does_not() {
        let (workspace, root) = workspace("leak");
        let model = build(&queue_document(POLICY), &workspace).unwrap();
        assert_eq!(
            keys(&model),
            ["field-leak App::Model::Queue::items"],
            "push and count work on the type's own field; admit reaches through it"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn moving_the_work_into_the_type_removes_the_leak() {
        let policy = "func admit(_ queue: Queue) {\n    queue.push(1)\n}\n";
        let (workspace, root) = workspace("fixed");
        std::fs::write(root.join("Sources/App/Policy/Admit.swift"), policy).unwrap();
        let mut index = queue_document(POLICY);
        let push = "scip-swift swiftpm App . Queue#push().";
        let last = index.documents[1].occurrences.last_mut().unwrap();
        last.symbol = push;
        last.range = at(policy, "queue.push");
        let model = build(&index, &workspace).unwrap();
        assert_eq!(keys(&model), Vec::<String>::new());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn references_between_swift_directories_form_the_module_cycle() {
        let left = "class Left { var right: Right? }\n";
        let right = "class Right { var left: Left? }\n";
        let root = std::env::temp_dir().join(format!("deslop-swift-{}-cycle", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Sources/App/Left")).unwrap();
        std::fs::create_dir_all(root.join("Sources/App/Right")).unwrap();
        std::fs::write(root.join("Sources/App/Left/Left.swift"), left).unwrap();
        std::fs::write(root.join("Sources/App/Right/Right.swift"), right).unwrap();
        let workspace = Workspace {
            root: root.clone(),
            build_directory: root.join(".build"),
            targets: vec![Target {
                name: "App".to_owned(),
                directory: "Sources/App".to_owned(),
                sources: vec!["Left/Left.swift".to_owned(), "Right/Right.swift".to_owned()],
                test: false,
            }],
        };
        let left_type = "scip-swift swiftpm App . Left#";
        let right_type = "scip-swift swiftpm App . Right#";
        let index = Index {
            documents: vec![
                Document {
                    path: Cow::Borrowed("Sources/App/Left/Left.swift"),
                    position_encoding: 1,
                    symbols: vec![symbol(left_type, "App.Left", KIND_CLASS)],
                    occurrences: vec![
                        occurrence(left_type, at(left, "Left"), true),
                        occurrence(right_type, at(left, "Right"), false),
                    ],
                },
                Document {
                    path: Cow::Borrowed("Sources/App/Right/Right.swift"),
                    position_encoding: 1,
                    symbols: vec![symbol(right_type, "App.Right", KIND_CLASS)],
                    occurrences: vec![
                        occurrence(right_type, at(right, "Right"), true),
                        occurrence(left_type, at(right, "Left"), false),
                    ],
                },
            ],
        };
        let model = build(&index, &workspace).unwrap();
        let keys = keys(&model);
        assert!(
            keys.contains(&"module-cycle App::Left,App::Right".to_owned()),
            "{keys:?}"
        );
        assert!(
            keys.contains(&"tangle App::Left::Left,App::Right::Right".to_owned()),
            "{keys:?}"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
