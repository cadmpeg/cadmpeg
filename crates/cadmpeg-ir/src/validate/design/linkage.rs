// SPDX-License-Identifier: Apache-2.0
//! Scope and linkage (§2.1, §2.2): the scope invariants, the identities of the
//! design records, and their native links.

use std::collections::btree_map::Entry;
use std::collections::BTreeMap;

use super::records::DesignRecords;
use crate::design::feature::{FeatureOperation, StoredOrigin};
use crate::document::{CadIr, ScopeViolation};
use crate::index::ModelIndex;
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

pub(super) fn check_linkage(
    index: &DesignRecords<'_>,
    ids: &ModelIndex<'_>,
    findings: &mut Vec<Finding>,
) {
    let ir = index.ir;
    check_scope(ir, findings);
    check_identities(ir, findings);
    check_native_refs(index, ids, findings);
}

/// The class home of a record whose identity [`check_identities`] visits.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Home {
    Model,
    Design,
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
    (@arena $ir:ident, $visit:ident, $field:ident, $home:ident, [$($attribute:meta),*]) => {};
    ($($field:ident: $ty:ty, $home:ident, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {
        /// Visit the home, arena label, and identity of every record of the
        /// arenas outside `Topology` that the CADIR wire carries: the model
        /// arenas first, then the design arenas. A design
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

/// Every `native_ref` of a design record resolves.
fn check_native_refs(index: &DesignRecords<'_>, ids: &ModelIndex<'_>, findings: &mut Vec<Finding>) {
    let ir = index.ir;
    let mut check = |owner: &str, native_ref: Option<&String>| {
        let Some(target) = native_ref else { return };
        let resolves = crate::validate::annotations_native::native_link_resolves(
            ir,
            |target| ids.native_record_owns(target),
            target,
        );
        if !resolves {
            super::error(
                findings,
                owner,
                crate::rule!(
                    "design.native-ref-does-not-resolve",
                    "native_ref `{target}` does not resolve"
                ),
            );
        }
    };
    let design = &ir.design;
    for scope in &design.scopes {
        for current in &scope.current {
            check(scope.definition.as_str(), current.native_ref.as_ref());
        }
    }
    for feature in &design.features {
        check(feature.id.as_str(), feature.native_ref.as_ref());
    }
    for (holder, _, definition) in index.feature_definitions() {
        let link = match &definition.operation {
            FeatureOperation::Stored {
                origin: StoredOrigin::Linked(link),
                ..
            } => Some(&**link),
            FeatureOperation::Datum(datum) => datum.link.as_ref(),
            _ => None,
        };
        if let Some(link) = link {
            check(holder, link.native_ref.as_ref());
        }
    }
    for sketch in &design.sketches {
        check(sketch.id.as_str(), sketch.native_ref.as_ref());
    }
    for entity in &design.sketch_entities {
        check(entity.id.as_str(), entity.native_ref.as_ref());
    }
    for constraint in &design.sketch_constraints {
        check(constraint.id.as_str(), constraint.native_ref.as_ref());
    }
    for sketch in &design.spatial_sketches {
        check(sketch.id.as_str(), sketch.native_ref.as_ref());
    }
    for entity in &design.spatial_sketch_entities {
        check(entity.id.as_str(), entity.native_ref.as_ref());
    }
    for constraint in &design.spatial_sketch_constraints {
        check(constraint.id.as_str(), constraint.native_ref.as_ref());
    }
    for parameter in &design.parameters {
        check(parameter.id.as_str(), parameter.native_ref.as_ref());
    }
    for spreadsheet in &design.spreadsheets {
        check(spreadsheet.id.as_str(), spreadsheet.native_ref.as_ref());
    }
    for rule in &design.sheet_metal_rules {
        check(rule.id.as_str(), rule.native_ref.as_ref());
    }
    for configuration in &design.configurations {
        check(configuration.id.as_str(), configuration.native_ref.as_ref());
    }
    for arrangement in &design.arrangements {
        check(arrangement.id.as_str(), arrangement.native_ref.as_ref());
    }
    for evaluation in &design.evaluations {
        check(evaluation.id.as_str(), evaluation.native_ref.as_ref());
    }
}

#[cfg(test)]
mod tests;
