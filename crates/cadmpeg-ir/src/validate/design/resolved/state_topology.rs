// SPDX-License-Identifier: Apache-2.0
//! The rules of one `Topology` on a resolved state (part 10, Coincidence:
//! every `Topology` of a document is checked at its `tolerances`). The
//! state's records are checked as the geometry of a document of their own, by
//! the checks that the current geometry runs.
//!
//! A finding is reported once along a chain, against the record whose delta
//! makes it: a finding that the base state also has is the base's, which the
//! base record reports, or the current-geometry checks do.

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use super::super::records::PositionKey;
use super::ResolvedState;
use crate::native::view::NativeView;
use crate::report::check::{Check, Finding};
use crate::validate::nested_finding;
use crate::validate::topology::nested;

/// The state that a resolved state is: the base a record's content names, or
/// the record itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum StateKey<'a> {
    /// Nothing.
    Empty,
    /// The extent of a scope's current state at a position.
    Current(&'a str, PositionKey<'a>),
    /// An evaluation, the first record of its id.
    Evaluation(&'a str),
    /// A process state, the first record of its id.
    Process(&'a str),
}

/// The findings of the rules on one resolved state.
struct Checked {
    findings: Vec<Finding>,
    /// The `Debug` form of each finding, which identifies the breach.
    breaches: BTreeSet<String>,
}

/// The rules of one topology on the resolved states of one validation pass.
/// The findings of each keyed state are kept for the states based on it.
pub(super) struct StateRules<'a, 'c, 'd> {
    ctx: &'c DecodeContext<'d>,
    document: NativeView<'c>,
    /// The findings of each keyed state, by whether the caches were compared.
    checked: BTreeMap<(StateKey<'a>, bool), Rc<Checked>>,
}

impl<'a, 'c, 'd> StateRules<'a, 'c, 'd> {
    pub(super) fn new(ctx: &'c DecodeContext<'d>, document: NativeView<'c>) -> Self {
        Self {
            ctx,
            document,
            checked: BTreeMap::new(),
        }
    }

    fn checked(
        &mut self,
        key: Option<StateKey<'a>>,
        state: &ResolvedState<'_>,
        caches: bool,
    ) -> Result<Rc<Checked>, CodecError> {
        if let Some(checked) = key.and_then(|key| self.checked.get(&(key, caches))) {
            return Ok(Rc::clone(checked));
        }
        let findings = nested::check(
            self.ctx,
            self.document,
            state.topology(),
            None,
            nested::NestRules::All { caches },
        )?;
        let breaches = findings
            .iter()
            .map(|finding| format!("{finding:?}"))
            .collect();
        let checked = Rc::new(Checked { findings, breaches });
        if let Some(key) = key {
            self.checked.insert((key, caches), Rc::clone(&checked));
        }
        Ok(checked)
    }

    /// Reports against `holder` each finding of the rules on the `state`
    /// that its `base` does not have. With `caches`, the state's status is
    /// `Current` and its procedural curves are compared with their solved
    /// carriers (Caches, K2).
    pub(super) fn check(
        &mut self,
        base: (StateKey<'a>, &ResolvedState<'_>),
        state: (Option<StateKey<'a>>, &ResolvedState<'_>),
        holder: &str,
        caches: bool,
        findings: &mut Vec<Finding>,
    ) -> Result<(), CodecError> {
        let inherited = self.checked(Some(base.0), base.1, caches)?;
        let checked = self.checked(state.0, state.1, caches)?;
        for finding in &checked.findings {
            if !inherited.breaches.contains(&format!("{finding:?}")) {
                findings.push(nested_finding(
                    finding.clone(),
                    Check::Design,
                    holder,
                    "resolved state",
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
