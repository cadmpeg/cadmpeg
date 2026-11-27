// SPDX-License-Identifier: Apache-2.0
//! The rules of one `Topology` on a resolved state (part 10, Coincidence:
//! every `Topology` of a document is checked at its `tolerances`). The
//! state's records are checked as the geometry of a document of their own, by
//! the checks that the current geometry runs.
//!
//! A finding is reported once along a chain, against the record whose delta
//! makes it: a finding that the base state also has is the base's, which the
//! base record reports, or the current-geometry checks do.

use std::collections::BTreeSet;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use super::state::{Applied, State};
use crate::document::Topology;
use crate::native::view::NativeView;
use crate::report::check::{Check, Finding};
use crate::validate::nested_finding;
use crate::validate::topology::nested;

/// The rules of one topology on the resolved states of one validation pass.
pub(super) struct StateRules<'c, 'd> {
    ctx: &'c DecodeContext<'d>,
    document: NativeView<'c>,
}

impl<'c, 'd> StateRules<'c, 'd> {
    pub(super) const fn new(ctx: &'c DecodeContext<'d>, document: NativeView<'c>) -> Self {
        Self { ctx, document }
    }

    fn findings(&self, topology: Topology, caches: bool) -> Result<Vec<Finding>, CodecError> {
        nested::check(
            self.ctx,
            self.document,
            topology,
            None,
            nested::NestRules::All { caches },
        )
    }

    /// Reports against `holder` each finding of the rules on `state` that the
    /// state of its base does not have. `applied` is what the record's delta
    /// changed. The rules read the records that the change reaches through
    /// references, in the base and in the state; where a rule reports that it
    /// stopped at its work limit, they read both states whole. With `caches`,
    /// the state's status is `Current` and its procedural curves are compared
    /// with their solved carriers (Caches, K2).
    pub(super) fn check(
        &self,
        state: &State<'_>,
        applied: &Applied<'_>,
        holder: &str,
        caches: bool,
        findings: &mut Vec<Finding>,
    ) -> Result<(), CodecError> {
        if applied.base.is_empty() {
            return Ok(());
        }
        let (base, now) = state.region_topologies(self.ctx, applied)?;
        let mut inherited = self.findings(base, caches)?;
        let mut found = self.findings(now, caches)?;
        let limited = |findings: &[Finding]| {
            findings.iter().any(|finding| {
                finding.rule
                    == crate::rule_name!("eval.model-carrier-evaluations-reached-local-work")
            })
        };
        if limited(&inherited) || limited(&found) {
            let (base, now) = state.whole_topologies(applied);
            inherited = self.findings(base, caches)?;
            found = self.findings(now, caches)?;
        }
        let inherited: BTreeSet<String> = inherited
            .iter()
            .map(|finding| format!("{finding:?}"))
            .collect();
        for finding in found {
            if !inherited.contains(&format!("{finding:?}")) {
                findings.push(nested_finding(
                    finding,
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
