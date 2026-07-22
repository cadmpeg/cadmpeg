// SPDX-License-Identifier: Apache-2.0
//! Focused validation checks for identity order.
#![allow(clippy::wildcard_imports)] // Split checks share private orchestration context.

use super::*;
use crate::drawings::Drawing;
use crate::features::{DesignConfiguration, DesignParameter};
use crate::presentation::{PresentationDocument, ViewPresentation};
use crate::products::{AssemblyJoint, Component, Occurrence};
use crate::semantic_annotations::SemanticAnnotation;
use crate::sketches::{Sketch, SketchConstraint, SketchEntity, SpatialSketch, SpatialSketchEntity};
use crate::spreadsheets::Spreadsheet;
use crate::subd::SubdSurface;

pub(super) fn check_version(ir: &CadIr, findings: &mut Vec<Finding>) {
    if ir.ir_version != IR_VERSION {
        findings.push(Finding {
            check: Check::Version,
            severity: Severity::Error,
            message: format!(
                "unsupported ir_version {:?}; expected {IR_VERSION}",
                ir.ir_version
            ),
            entity: None,
        });
    }
}

fn valid_id(id: &str) -> bool {
    let Some((namespace, key)) = id.split_once('#') else {
        return false;
    };
    if key.is_empty() || key.contains('#') || id.chars().any(char::is_whitespace) {
        return false;
    }
    let mut components = namespace.split(':');
    components.next().is_some_and(|value| !value.is_empty())
        && components.next().is_some_and(|value| !value.is_empty())
        && components.next().is_some_and(|value| !value.is_empty())
        && components.next().is_none()
}

fn push_identity(seen: &mut HashSet<String>, findings: &mut Vec<Finding>, id: &str) {
    if !valid_id(id) {
        findings.push(Finding {
            check: Check::Identity,
            severity: Severity::Error,
            message: "entity id does not match `<format>:<scope>:<kind>#<key>`".into(),
            entity: Some(id.to_owned()),
        });
    }
    if !seen.insert(id.to_owned()) {
        findings.push(Finding {
            check: Check::Identity,
            severity: Severity::Error,
            message: "entity id is not globally unique".into(),
            entity: Some(id.to_owned()),
        });
    }
}

pub(super) fn check_order<'a>(
    arena: &str,
    ids: impl IntoIterator<Item = &'a str>,
    findings: &mut Vec<Finding>,
) {
    let mut previous: Option<&str> = None;
    for id in ids {
        if previous.is_some_and(|value| value >= id) {
            findings.push(Finding {
                check: Check::ArenaOrder,
                severity: Severity::Error,
                message: format!("arena `{arena}` is not strictly sorted by id"),
                entity: Some(id.to_owned()),
            });
            return;
        }
        previous = Some(id);
    }
}

macro_rules! define_model_identity_checks {
    ($( $field:ident: $element:ty, $doc:literal, [$($attribute:meta),*] => $key:expr; )*) => {
        fn check_model_identity_and_order(
            ir: &CadIr,
            seen: &mut HashSet<String>,
            findings: &mut Vec<Finding>,
        ) {
            $(
                let key: fn(&$element) -> String = $key;
                check_order(
                    stringify!($field),
                    ir.model.$field.iter().map(|entity| key(entity)).collect::<Vec<_>>().iter().map(String::as_str),
                    findings,
                );
                for entity in &ir.model.$field {
                    push_identity(seen, findings, &key(entity));
                }
            )*
        }
    };
}
crate::document::arena_registry!(define_model_identity_checks);

/// Run the identity and arena-order checks, returning the set of every entity
/// id in the document (model arenas, unknowns, and native records). Downstream
/// checks resolve annotation and link targets against this set instead of
/// re-enumerating the id universe.
pub(super) fn check_identity_and_order(ir: &CadIr, findings: &mut Vec<Finding>) -> HashSet<String> {
    let mut seen = HashSet::new();
    check_model_identity_and_order(ir, &mut seen, findings);
    let native_ids = collect_native_ids(ir);
    for (_, id) in &native_ids {
        push_identity(&mut seen, findings, id);
    }
    let mut by_arena: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (arena, id) in &native_ids {
        by_arena.entry(arena).or_default().push(id);
    }
    for (arena, ids) in by_arena {
        check_order(arena, ids, findings);
    }
    seen
}

pub(super) fn collect_native_ids(ir: &CadIr) -> Vec<(String, &str)> {
    ir.native
        .0
        .iter()
        .flat_map(|(format, namespace)| {
            namespace.arenas.iter().flat_map(move |(arena, records)| {
                records
                    .iter()
                    .map(move |record| (format!("native.{format}.{arena}"), record.id.as_str()))
            })
        })
        .collect()
}

macro_rules! define_registered_entity_counts {
    ($( $field:ident: $element:ty, $doc:literal, [$($attribute:meta),*] => $key:expr; )*) => {
        fn registered_entity_counts(ir: &CadIr) -> BTreeMap<String, usize> {
            BTreeMap::from([
                $((stringify!($field).into(), ir.model.$field.len())),*
            ])
        }
    };
}
crate::document::arena_registry!(define_registered_entity_counts);

pub(super) fn entity_counts(ir: &CadIr) -> BTreeMap<String, usize> {
    let mut counts = registered_entity_counts(ir);
    counts.insert(
        "surfaces_unknown_geometry".into(),
        ir.model
            .surfaces
            .iter()
            .filter(|surface| matches!(surface.geometry, SurfaceGeometry::Unknown { .. }))
            .count(),
    );
    for loss in ir.native.loss_counts() {
        counts.insert(format!("native.{}.{}", loss.format, loss.kind), loss.count);
    }
    counts
}
