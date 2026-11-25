//! `call-order`: two free functions of one module, working on the same type,
//! where every caller that calls one calls the other first. A method's
//! receiver carries the order it is called in: what one call leaves, the next
//! reads. A free function carries nothing, so two that work on the same value
//! and are always called in one order keep a state their callers cannot see,
//! and the caller that gets the order wrong is not told. What both do to that
//! value is the type's business, and the type is where the sequence belongs.

use std::collections::BTreeMap;

use super::{
    Diagnostic, EncapsulationLint, Lint, Location, Picture, counted, in_scope, without_generics,
};
use crate::graph::{Graph, ThingId};
use crate::model::{ItemId, ItemKind, Model, NodeId};

/// The fewest callers that must keep the order. Fewer agrees by accident.
const LEAST_CALLERS: usize = 3;

/// Where a caller calls a function: its file and its line.
type Site = (String, u32);

pub fn check(model: &Model, graph: &Graph, scope: Option<NodeId>) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    // By the type a module's free functions work on, those of them that do.
    let mut working: BTreeMap<NodeId, BTreeMap<ItemId, Vec<ItemId>>> = BTreeMap::new();
    for (index, item) in model.items.iter().enumerate() {
        let id = ItemId::new(index);
        let Some(worked_on) = item.first_param_type else {
            continue;
        };
        if item.kind == ItemKind::Function
            && !item.takes_self
            && !item.trait_impl
            && !model.tree.is_test(item.owner)
            && item.file.is_none_or(|file| !model.files[file.index()].test)
            && !model.wrappers.contains_key(&id)
        {
            working
                .entry(item.owner)
                .or_default()
                .entry(worked_on)
                .or_default()
                .push(id);
        }
    }
    // By function, by the caller that calls it, where it calls it. Code of the
    // module itself keeps no order that its callers have to.
    let mut calls: BTreeMap<ItemId, BTreeMap<ThingId, Site>> = BTreeMap::new();
    for usage in &graph.uses {
        let Some(user) = usage.user else { continue };
        if graph.things[user.index()].home == model.items[usage.item.index()].owner {
            continue;
        }
        let reference = &model.references[usage.reference];
        let site = (
            model.files[reference.file.index()].path.clone(),
            reference.line,
        );
        let sites = calls.entry(usage.item).or_default();
        let first = sites.entry(user).or_insert_with(|| site.clone());
        if site < *first {
            *first = site;
        }
    }
    for (&module, worked_on) in &working {
        for (&ty, functions) in worked_on {
            for (at, &first) in functions.iter().enumerate() {
                for &second in &functions[at + 1..] {
                    let Some((callers, site)) = order(&calls, first, second) else {
                        continue;
                    };
                    if in_scope(&model.tree, scope, module) {
                        diagnostics.push(diagnostic(model, ty, first, second, (callers, site)));
                    }
                }
            }
        }
    }
    diagnostics
}

/// How `first` is called before `second`: by how many callers, and where the
/// earliest of them calls `first`. `None` unless every caller of `second` calls
/// `first` first: a caller that reaches `second` without `first`, or before it,
/// keeps no order of the module's. Fewer than [`LEAST_CALLERS`] is no order
/// either, since two callers can agree by accident.
fn order(
    calls: &BTreeMap<ItemId, BTreeMap<ThingId, Site>>,
    first: ItemId,
    second: ItemId,
) -> Option<(usize, Location)> {
    let of_first = calls.get(&first)?;
    let of_second = calls
        .get(&second)
        .filter(|sites| sites.len() >= LEAST_CALLERS)?;
    let mut earliest: Option<Location> = None;
    for (caller, at_second) in of_second {
        let at_first = of_first.get(caller)?;
        // Two calls on one line, one of them inside the other's arguments: the
        // code there does not say which happens first.
        if at_first == at_second || at_second < at_first {
            return None;
        }
        let here = Location {
            file: at_first.0.clone(),
            line: at_first.1,
        };
        if earliest.as_ref().is_none_or(|before| here < *before) {
            earliest = Some(here);
        }
    }
    Some((of_second.len(), earliest?))
}

fn diagnostic(
    model: &Model,
    worked_on: ItemId,
    first: ItemId,
    second: ItemId,
    (callers, site): (usize, Location),
) -> Diagnostic {
    let name = |item: ItemId| without_generics(&model.items[item.index()].name);
    let (a, b) = (name(first), name(second));
    let ty = name(worked_on);
    let at = model.tree.display(model.items[first.index()].owner);
    Diagnostic {
        lint: Lint::Encapsulation(EncapsulationLint::CallOrder),
        key: format!("{at}::{a},{b}"),
        message: format!(
            "`{a}` then `{b}`, in that order in {}, all on a `{ty}`: the sequence belongs in `{ty}`",
            counted(callers, "caller")
        ),
        location: Some(site),
        notes: vec![
            format!(
                "every caller that calls `{b}` calls `{a}` first: none calls `{b}` alone, or \
                 before it"
            ),
            format!("as methods of `{ty}`, what they do to it between them is the type's to keep"),
        ],
        picture: Picture::Arrow {
            from: format!("{at}::{a}"),
            to: b,
            detail: format!("called first by all {callers} callers, each on a `{ty}`"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::Graph;
    use crate::model::tests::Sample;

    fn keys(sample: &Sample) -> Vec<String> {
        check(&sample.model, &Graph::new(&sample.model), None)
            .into_iter()
            .map(|d| format!("{} {}", d.lint.name(), d.key))
            .collect()
    }

    /// A module `queue` that keeps a `Queue` and the two functions over it, and
    /// `callers` functions of `app`, each calling `admit` and then `next`.
    fn with_callers(callers: usize) -> (Sample, ItemId, ItemId) {
        let mut sample = Sample::new();
        let queue = sample.module("queue");
        let queue_type = sample.item(queue, "Queue", ItemKind::Type);
        let admit = sample.item(queue, "admit", ItemKind::Function);
        let next = sample.item(queue, "next", ItemKind::Function);
        for function in [admit, next] {
            sample.model.items[function.index()].first_param_type = Some(queue_type);
        }
        let app = sample.module("app");
        for at in 0..callers {
            let caller = sample.item(app, &format!("run{at}"), ItemKind::Function);
            sample.refer(caller, admit);
            sample.refer(caller, next);
        }
        (sample, admit, next)
    }

    /// One more function of `app`, calling what `calls` names.
    fn another_caller(sample: &mut Sample, name: &str, calls: &[ItemId]) -> ItemId {
        let app = sample.module("app");
        let caller = sample.item(app, name, ItemKind::Function);
        for &item in calls {
            sample.refer(caller, item);
        }
        caller
    }

    #[test]
    fn two_functions_every_caller_calls_in_one_order_are_the_types_to_keep() {
        let (sample, _, _) = with_callers(3);
        assert_eq!(keys(&sample), ["call-order p::queue::admit,next"]);
        let diagnostic = &check(&sample.model, &Graph::new(&sample.model), None)[0];
        assert_eq!(
            diagnostic.message,
            "`admit` then `next`, in that order in 3 callers, all on a `Queue`: the sequence \
             belongs in `Queue`"
        );
        assert_eq!(
            diagnostic.picture,
            Picture::Arrow {
                from: "p::queue::admit".into(),
                to: "next".into(),
                detail: "called first by all 3 callers, each on a `Queue`".into(),
            }
        );
    }

    #[test]
    fn two_callers_can_agree_by_accident_where_three_are_an_order() {
        assert_eq!(keys(&with_callers(2).0), Vec::<String>::new());
    }

    #[test]
    fn a_caller_that_gets_to_the_second_without_the_first_keeps_no_order() {
        let (mut sample, _, next) = with_callers(3);
        another_caller(&mut sample, "alone", &[next]);
        assert_eq!(keys(&sample), Vec::<String>::new());

        let (mut sample, admit, next) = with_callers(3);
        another_caller(&mut sample, "backwards", &[next, admit]);
        assert_eq!(keys(&sample), Vec::<String>::new());
    }

    #[test]
    fn a_method_carries_its_own_order_and_functions_that_work_on_nothing_carry_none() {
        let (mut sample, admit, next) = with_callers(3);
        for function in [admit, next] {
            sample.model.items[function.index()].takes_self = true;
        }
        assert_eq!(keys(&sample), Vec::<String>::new());

        let (mut sample, _, next) = with_callers(3);
        let queue = sample.module("queue");
        let other = sample.item(queue, "Other", ItemKind::Type);
        sample.model.items[next.index()].first_param_type = Some(other);
        assert_eq!(keys(&sample), Vec::<String>::new());
    }
}
