//! `change-spread`: two modules that the code's history always changes
//! together, and that name nothing in common. A module is the work one person
//! does because one decision changed, so two modules whose changes are never
//! apart hold a decision between them that neither hides: the boundary between
//! them falls through the middle of it. Where one uses the other, or both use
//! the same item, the commits that change one change the other for a reason the
//! code already shows. What no reference explains, only the commits show.

use std::collections::{BTreeMap, BTreeSet};

use super::{Diagnostic, GraphLint, Lint, Picture, counted, in_scope, module_location};
use crate::graph::Graph;
use crate::model::{ItemId, Model, NodeId};

/// The fewest commits that must have changed both modules. Fewer is a
/// coincidence, or a design that has changed once so far.
const LEAST_COMMITS: usize = 5;

/// The share of the commits that changed either that must have changed both,
/// in percent: three in four.
const SHARED: usize = 75;

pub fn check(model: &Model, graph: &Graph, scope: Option<NodeId>) -> Vec<Diagnostic> {
    let tree = &model.tree;
    // By path, the module the file is part of; test code changes on its own.
    let module_of: BTreeMap<&str, NodeId> = model
        .files
        .iter()
        .filter(|file| !file.test && !tree.is_test(file.module))
        .map(|file| (file.path.as_str(), file.module))
        .collect();
    // By module, the commits that changed it; by two of them, the commits that
    // changed both.
    let mut changed: BTreeMap<NodeId, usize> = BTreeMap::new();
    let mut together: BTreeMap<(NodeId, NodeId), usize> = BTreeMap::new();
    for commit in &model.history {
        let modules: BTreeSet<NodeId> = commit
            .paths
            .iter()
            .filter_map(|path| module_of.get(path.as_str()).copied())
            .collect();
        for &module in &modules {
            *changed.entry(module).or_default() += 1;
        }
        let modules: Vec<NodeId> = modules.into_iter().collect();
        for (at, &first) in modules.iter().enumerate() {
            for &second in &modules[at + 1..] {
                *together.entry((first, second)).or_default() += 1;
            }
        }
    }
    let used = module_uses(model, graph);
    let mut diagnostics = Vec::new();
    for (&(first, second), &both) in &together {
        let either = changed[&first] + changed[&second] - both;
        if both < LEAST_COMMITS || both * 100 < SHARED * either {
            continue;
        }
        // One inside the other changes together with itself, and two modules
        // that name each other, or that name what they both depend on, change
        // together for a reason the code gives.
        if tree.is_within(first, second) || tree.is_within(second, first) {
            continue;
        }
        if names(model, &used, first, second) || shares(&used, first, second) {
            continue;
        }
        if !in_scope(tree, scope, first) && !in_scope(tree, scope, second) {
            continue;
        }
        diagnostics.push(diagnostic(model, first, second, both, either));
    }
    diagnostics
}

/// By module, the items its code names, wherever they are written.
fn module_uses(model: &Model, graph: &Graph) -> BTreeMap<NodeId, BTreeSet<ItemId>> {
    let mut used: BTreeMap<NodeId, BTreeSet<ItemId>> = BTreeMap::new();
    for usage in &graph.uses {
        let Some(user) = usage.user else { continue };
        let from = graph.things[user.index()].home;
        if from != model.items[usage.item.index()].owner {
            used.entry(from).or_default().insert(usage.item);
        }
    }
    used
}

/// Whether `first`'s code names an item written in `second`, or the other way.
fn names(
    model: &Model,
    used: &BTreeMap<NodeId, BTreeSet<ItemId>>,
    first: NodeId,
    second: NodeId,
) -> bool {
    let writes = |from: NodeId, in_module: NodeId| {
        used.get(&from).is_some_and(|items| {
            items
                .iter()
                .any(|&item| model.items[item.index()].owner == in_module)
        })
    };
    writes(first, second) || writes(second, first)
}

/// Whether both name one item that a third module writes: what each changes
/// for.
fn shares(used: &BTreeMap<NodeId, BTreeSet<ItemId>>, first: NodeId, second: NodeId) -> bool {
    match (used.get(&first), used.get(&second)) {
        (Some(one), Some(other)) => one.intersection(other).next().is_some(),
        _ => false,
    }
}

fn diagnostic(
    model: &Model,
    first: NodeId,
    second: NodeId,
    both: usize,
    either: usize,
) -> Diagnostic {
    let tree = &model.tree;
    let (a, b) = (tree.display(first), tree.display(second));
    Diagnostic {
        lint: Lint::Graph(GraphLint::ChangeSpread),
        key: format!("{a},{b}"),
        message: format!(
            "`{a}` and `{b}` change together: {both} of the {} that change either change both, and \
             neither uses the other",
            counted(either, "commit")
        ),
        location: module_location(model, first),
        notes: vec![
            "no reference either way: the decision they change for has no module of its own"
                .to_owned(),
            format!("the change wants one module, or the line between `{a}` and `{b}` is wrong"),
        ],
        picture: Picture::Bar {
            label: "together".to_owned(),
            length: both,
            most: either,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::Graph;
    use crate::model::Commit;
    use crate::model::ItemKind;
    use crate::model::tests::Sample;

    fn keys(sample: &Sample) -> Vec<String> {
        names(check(&sample.model, &Graph::new(&sample.model), None))
    }

    fn names(diagnostics: Vec<Diagnostic>) -> Vec<String> {
        diagnostics
            .into_iter()
            .map(|d| format!("{} {}", d.lint.name(), d.key))
            .collect()
    }

    /// One commit that changed `paths`.
    fn commit(paths: &[&str]) -> Commit {
        Commit {
            paths: paths.iter().map(|path| (*path).to_owned()).collect(),
        }
    }

    /// `count` commits, each of them changing both of `alpha` and `beta`.
    fn together(count: usize) -> Vec<Commit> {
        vec![commit(&["src/alpha.rs", "src/beta.rs"]); count]
    }

    /// Two sibling modules of one crate, `alpha` and `beta`.
    fn siblings() -> Sample {
        let mut sample = Sample::new();
        sample.module("alpha");
        sample.module("beta");
        sample
    }

    #[test]
    fn two_modules_the_commits_never_change_apart_change_for_one_decision() {
        let mut sample = siblings();
        sample.model.history = together(5);
        let diagnostics = check(&sample.model, &Graph::new(&sample.model), None);
        assert_eq!(
            names(diagnostics.clone()),
            ["change-spread p::alpha,p::beta"]
        );
        assert_eq!(
            diagnostics[0].message,
            "`p::alpha` and `p::beta` change together: 5 of the 5 commits that change either \
             change both, and neither uses the other"
        );
        assert_eq!(
            diagnostics[0].picture,
            Picture::Bar {
                label: "together".into(),
                length: 5,
                most: 5
            }
        );
    }

    #[test]
    fn four_commits_are_a_coincidence_where_five_are_a_design() {
        let mut sample = siblings();
        sample.model.history = together(4);
        assert_eq!(keys(&sample), Vec::<String>::new());
    }

    #[test]
    fn changes_that_are_mostly_apart_are_not_changes_that_are_never_apart() {
        let mut sample = siblings();
        // Five of the eleven commits that change either change both.
        let mut history = together(5);
        history.extend(vec![commit(&["src/alpha.rs"]); 6]);
        sample.model.history = history;
        assert_eq!(keys(&sample), Vec::<String>::new());
    }

    #[test]
    fn two_modules_that_name_each_other_change_together_because_one_depends() {
        let mut sample = siblings();
        let alpha = sample.module("alpha");
        let beta = sample.module("beta");
        let caller = sample.item(alpha, "run", ItemKind::Function);
        let called = sample.item(beta, "send", ItemKind::Function);
        sample.refer(caller, called);
        sample.model.history = together(5);
        assert_eq!(keys(&sample), Vec::<String>::new());
    }

    #[test]
    fn two_modules_that_name_what_they_share_change_together_for_that() {
        let mut sample = siblings();
        let (alpha, beta) = (sample.module("alpha"), sample.module("beta"));
        let shared = sample.module("shared");
        let helper = sample.item(shared, "helper", ItemKind::Function);
        let one = sample.item(alpha, "one", ItemKind::Function);
        let other = sample.item(beta, "two", ItemKind::Function);
        sample.refer(one, helper);
        sample.refer(other, helper);
        sample.model.history = together(5);
        assert_eq!(keys(&sample), Vec::<String>::new());
    }

    #[test]
    fn a_module_and_what_it_holds_change_together_by_being_one_module() {
        let mut sample = Sample::new();
        sample.module("alpha");
        sample.module("alpha::inner");
        sample.model.history = vec![commit(&["src/alpha.rs", "src/alpha/inner.rs"]); 5];
        assert_eq!(keys(&sample), Vec::<String>::new());
    }

    #[test]
    fn test_code_changes_on_its_own() {
        let mut sample = siblings();
        let beta = sample.module("beta");
        sample.file(beta, "tests/beta.rs", 1);
        sample.model.files.last_mut().expect("just added").test = true;
        sample.model.history = vec![commit(&["src/alpha.rs", "tests/beta.rs"]); 5];
        assert_eq!(keys(&sample), Vec::<String>::new());
    }

    #[test]
    fn a_report_is_made_where_one_of_the_two_is_in_scope() {
        let mut sample = siblings();
        sample.model.history = together(5);
        let gamma = sample.module("gamma");
        let beta = sample.module("beta");
        let in_scope = |scope: NodeId| {
            names(check(
                &sample.model,
                &Graph::new(&sample.model),
                Some(scope),
            ))
        };
        assert_eq!(in_scope(gamma), Vec::<String>::new());
        assert_eq!(in_scope(beta), ["change-spread p::alpha,p::beta"]);
    }
}
