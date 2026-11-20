// SPDX-License-Identifier: Apache-2.0
//! Scope and linkage (§2.1, §2.2): the scope invariants, the identities of the
//! design records, and their native links.

use std::collections::BTreeSet;

use super::records::DesignRecords;
use crate::design::feature::{FeatureOperation, StoredOrigin};
use crate::document::CadIr;
use crate::index::ModelIndex;
use crate::report::check::{Check, Finding};
use crate::report::Severity;
use crate::schema::ArenaRecord;

pub(super) fn check_linkage(
    index: &DesignRecords<'_>,
    ids: &ModelIndex<'_>,
    findings: &mut Vec<Finding>,
) {
    let ir = index.ir;
    if let Err(violation) = ir.check_scope() {
        crate::validate::record_finding(
            findings,
            Check::Design,
            Severity::Error,
            None,
            format_args!("the document breaks the scope invariants: {violation}"),
        );
    }
    check_identities(ir, findings);
    check_native_refs(index, ids, findings);
}

macro_rules! visit_design_identities {
    (@arena $ir:ident, $visit:ident, $field:ident, design) => {
        for record in &$ir.design.$field {
            $visit(
                crate::document::arena_label!($field, design),
                ArenaRecord::record_identity(record),
            );
        }
    };
    (@arena $ir:ident, $visit:ident, $field:ident, $home:ident) => {};
    ($($field:ident: $ty:ty, $home:ident, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {
        /// Visit the arena label and identity of every design record. A
        /// design scope's identity is the definition whose design it is.
        fn visit_design_identities<'a>(ir: &'a CadIr, mut visit: impl FnMut(&'static str, &'a str)) {
            $(visit_design_identities!(@arena ir, visit, $field, $home);)*
        }
    };
}
crate::document::every_arena_registry!(visit_design_identities);

/// Design identities are unique among the design records. The design records
/// form their own identity namespace while the model holds the records they
/// restate.
fn check_identities(ir: &CadIr, findings: &mut Vec<Finding>) {
    let mut seen = BTreeSet::new();
    visit_design_identities(ir, |arena, id| {
        if !seen.insert(id) {
            super::error(
                findings,
                id,
                format_args!("{arena} record id is not unique among the design records"),
            );
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
                format_args!("native_ref `{target}` does not resolve"),
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
    for (holder, definition) in index.feature_definitions() {
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
    for program in &design.programs {
        check(program.id.as_str(), program.native_ref.as_ref());
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
