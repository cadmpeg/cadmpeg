// SPDX-License-Identifier: Apache-2.0
//! Scope and linkage (§2.1, §2.2): the scope invariants, the identities of the
//! design records.

use std::collections::btree_map::Entry;
use std::collections::BTreeMap;

use super::records::DesignRecords;
use crate::document::{CadIr, ScopeViolation};
use crate::report::check::{Check, Finding};
use crate::report::Severity;
use crate::schema::ArenaRecord;

/// Part 10's scope rules (§2.1): one Error per [`ScopeViolation`].
pub(super) fn check_scope(ir: &CadIr, findings: &mut Vec<Finding>) {
    use crate::validate::record_finding;
    for violation in ir.scope_violations() {
        match violation {
            ScopeViolation::Unmet { .. } => record_finding(
                findings,
                Check::Design,
                Severity::Error,
                None,
                crate::rule!(
                    "design.scope-holds-class-without-required-class",
                    "{violation}"
                ),
            ),
            ScopeViolation::Populated { .. } => record_finding(
                findings,
                Check::Design,
                Severity::Error,
                None,
                crate::rule!("design.scope-excluded-arena-holds-records", "{violation}"),
            ),
            ScopeViolation::Referenced { .. } => record_finding(
                findings,
                Check::Design,
                Severity::Error,
                None,
                crate::rule!(
                    "design.scope-excluded-class-referenced-another-arena",
                    "{violation}"
                ),
            ),
        }
    }
}

pub(super) fn check_linkage(index: &DesignRecords<'_>, findings: &mut Vec<Finding>) {
    let ir = index.ir;
    check_scope(ir, findings);
    check_identities(ir, findings);
}

/// The class home of a record whose identity [`check_identities`] visits.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Home {
    Model,
    Design,
    Process,
}

macro_rules! visit_identities {
    (@arena $ir:ident, $visit:ident, $field:ident, model, []) => {
        for record in &$ir.model.$field {
            $visit(
                Home::Model,
                crate::document::arena_label!($field, model),
                ArenaRecord::record_identity(record),
            );
        }
    };
    (@arena $ir:ident, $visit:ident, scopes, design, [$($attribute:meta),*]) => {};
    (@arena $ir:ident, $visit:ident, $field:ident, design, [$($attribute:meta),*]) => {
        for record in &$ir.design.$field {
            $visit(
                Home::Design,
                crate::document::arena_label!($field, design),
                ArenaRecord::record_identity(record),
            );
        }
    };
    (@arena $ir:ident, $visit:ident, $field:ident, process, [$($attribute:meta),*]) => {
        for record in &$ir.process.$field {
            $visit(
                Home::Process,
                crate::document::arena_label!($field, process),
                ArenaRecord::record_identity(record),
            );
        }
    };
    (@arena $ir:ident, $visit:ident, $field:ident, $home:ident, [$($attribute:meta),*]) => {};
    ($($field:ident: $ty:ty, $home:ident, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {
        /// Visit the home, arena label, and identity of every record of the
        /// arenas outside `Topology` that the CADIR wire carries: the model
        /// arenas first, then the design and process arenas. A design
        /// scope's identity is the definition whose design it is, so the
        /// scopes are not visited.
        fn visit_identities<'a>(ir: &'a CadIr, mut visit: impl FnMut(Home, &'static str, &'a str)) {
            $(visit_identities!(@arena ir, visit, $field, $home, [$($attribute),*]);)*
        }
    };
}
crate::document::every_arena_registry!(visit_identities);

/// No two records of different arenas outside `Topology` share an id
/// (§2.2). Two model records that share one are the identity rules'
/// finding; the design records the model's in-memory arenas restate share
/// their ids and are not compared with them.
pub(super) fn check_identities(ir: &CadIr, findings: &mut Vec<Finding>) {
    let mut seen: BTreeMap<&str, (Home, &'static str)> = BTreeMap::new();
    visit_identities(ir, |home, arena, id| {
        let (first_home, first) = match seen.entry(id) {
            Entry::Vacant(entry) => {
                entry.insert((home, arena));
                return;
            }
            Entry::Occupied(entry) => *entry.get(),
        };
        match (first_home, home) {
            (Home::Model, Home::Model) => {}
            (Home::Design, Home::Design) => super::error(
                findings,
                id,
                crate::rule!(
                    "design.record-id-not-unique-among-design",
                    "{arena} record id is not unique among the design records"
                ),
            ),
            _ => super::error(
                findings,
                id,
                crate::rule!(
                    "design.record-id-shared-another-arena",
                    "{arena} record id is also the id of a {first} record"
                ),
            ),
        }
    });
}

#[cfg(test)]
mod tests;
