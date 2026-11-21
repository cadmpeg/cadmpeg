// SPDX-License-Identifier: Apache-2.0
//! The selection spellings of §5.8.
//!
//! The explicit items of one list under one propagation are one `Explicit`
//! term, and a rule whose only term is `Explicit`, with no propagation and no
//! exclusions, is `Named` (D43).

use crate::design::selection::{
    Keyed, MemberOrder, Members, Picked, Picks, RuleTerm, SelectionRule,
};

/// Puts a selection's picks into their one spelling.
pub(super) fn normalize<O: MemberOrder>(picks: &mut Picks<O>) {
    let Picks::Rule {
        terms,
        excluded,
        resolved,
    } = picks
    else {
        return;
    };
    merge_explicit_terms(terms);
    let [only] = terms.as_slice() else {
        return;
    };
    if !matches!(only.rule, SelectionRule::Explicit)
        || only.propagation.is_some()
        || !excluded.is_empty()
    {
        return;
    }
    let named = match resolved {
        None => Members::new(only.seeds.as_slice().to_vec()).ok(),
        Some(cached) => with_cache(only.seeds.as_slice(), cached.as_slice()),
    };
    if let Some(named) = named {
        *picks = Picks::Named(named);
    }
}

/// The seeds of an explicit rule with what its cached result states of each:
/// the cache lists the seeds, so each cached pick adds its witness and its
/// resolved entities where the seed states none. `None` where the cache
/// lists other picks, or states a witness or resolved entities that differ
/// from the seed's, which no list of picks can hold both of.
fn with_cache<O: MemberOrder>(seeds: &[Picked], cached: &[Picked]) -> Option<Members<Picked, O>> {
    if seeds.len() != cached.len() {
        return None;
    }
    let mut named = Vec::with_capacity(seeds.len());
    for (seed, cached) in seeds.iter().zip(cached) {
        if seed.key() != cached.key()
            || seed.domain != cached.domain
            || differ(seed.witness.as_ref(), cached.witness.as_ref())
            || differ(seed.resolved.as_ref(), cached.resolved.as_ref())
        {
            return None;
        }
        named.push(Picked {
            witness: seed.witness.as_ref().or(cached.witness.as_ref()).cloned(),
            resolved: seed.resolved.as_ref().or(cached.resolved.as_ref()).cloned(),
            ..seed.clone()
        });
    }
    Members::new(named).ok()
}

/// Whether two optional statements of one fact both state it, differently.
fn differ<T: PartialEq>(seed: Option<&T>, cached: Option<&T>) -> bool {
    matches!((seed, cached), (Some(seed), Some(cached)) if seed != cached)
}

/// Unites `Explicit` terms of equal propagation into the first of them. An
/// unordered selection unites every such pair; an ordered one, whose result
/// lists each term's result in turn, only neighbours, and only where their
/// seeds share no pick, so its result keeps its order.
fn merge_explicit_terms<O: MemberOrder>(terms: &mut Vec<RuleTerm<O>>) {
    let mut at = 0;
    while at < terms.len() {
        let mut next = at + 1;
        while next < terms.len() {
            if O::SORTED || next == at + 1 {
                if let Some(merged) = merged_seeds(&terms[at], &terms[next]) {
                    terms[at].seeds = merged;
                    terms.remove(next);
                    continue;
                }
            }
            if !O::SORTED {
                break;
            }
            next += 1;
        }
        at += 1;
    }
}

/// The seeds of two `Explicit` terms of equal propagation as one list, or
/// `None` where they are not such terms, where an ordered list would state a
/// pick twice, or where the two state one pick differently.
fn merged_seeds<O: MemberOrder>(
    first: &RuleTerm<O>,
    second: &RuleTerm<O>,
) -> Option<Members<Picked, O>> {
    if !matches!(first.rule, SelectionRule::Explicit)
        || !matches!(second.rule, SelectionRule::Explicit)
        || first.propagation != second.propagation
    {
        return None;
    }
    let mut seeds = first.seeds.as_slice().to_vec();
    for pick in second.seeds.as_slice() {
        match seeds.iter().find(|seed| seed.key() == pick.key()) {
            None => seeds.push(pick.clone()),
            Some(seed) if O::SORTED && seed == pick => {}
            Some(_) => return None,
        }
    }
    Members::new(seeds).ok()
}
