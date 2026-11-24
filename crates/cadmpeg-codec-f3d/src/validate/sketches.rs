// SPDX-License-Identifier: Apache-2.0
//! Validation of sketch placements, entity headers, relations, and geometry identities.

use super::{design_stream, push_constant_finding, Ctx};
use crate::{design, ids, records};
use cadmpeg_ir::report::check::{Check, Finding};
use cadmpeg_ir::report::Severity;
use std::collections::HashSet;

/// Validate sketch placement frames and their scope links.
pub(super) fn validate_sketch_placements(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let scopes_by_index = &ctx.scopes_by_index;
    let mut placement_records = HashSet::new();
    let mut placement_scopes = HashSet::new();
    let mut visibility_offsets = HashSet::new();
    let collections = native
        .design_types
        .iter()
        .filter(|registration| {
            registration
                .type_guid
                .as_str()
                .eq_ignore_ascii_case("5275CBA4-3D7D-40DC-A651-1DFF3FC8AFBF")
                && registration.version == 4
                && registration.module == "Geometry"
                && registration.base_type_guid.value().is_some_and(|base| {
                    base.as_str()
                        .eq_ignore_ascii_case("A7AEA631-985B-4DD1-8CE2-DE2C-14B54081")
                })
        })
        .flat_map(|registration| {
            registration
                .entities
                .values()
                .map(move |entity| (ids::design_segment(registration.id()), *entity))
        })
        .collect::<HashSet<_>>();
    for placement in &native.design_sketch_placements {
        let native_stream = design_stream(&placement.id);
        let unique_record = placement_records.insert((native_stream, placement.record_index));
        let unique_scope = match placement.scope_record_index {
            Some(index) => placement_scopes.insert((native_stream, index)),
            None => true,
        };
        let scope = placement
            .scope_record_index
            .and_then(|index| scopes_by_index.get(&(native_stream, index)));
        let visibility_valid = if let Some(visibility) = placement.visibility.as_ref() {
            let header_valid = ctx
                .entities_by_suffix
                .get(&(native_stream, placement.entity_id.suffix()))
                .is_some_and(|entity| visibility.stream_ordinal_offset() > entity.byte_offset);
            visibility
                .ordinal_collection
                .map_or(header_valid, |collection| {
                    let collection = collection.get();
                    if header_valid
                        && collections.contains(&(ids::design_segment(&placement.id), collection))
                    {
                        visibility_offsets.insert((native_stream, visibility.visible_offset()))
                    } else {
                        false
                    }
                })
        } else {
            true
        };
        let scope_valid = if placement.member_run_head() {
            scope.is_none_or(|scope| {
                design::design_feature_family(&scope.kind())
                    == Some(design::DesignFeatureFamily::Sketch)
            })
        } else {
            scope.is_some_and(|scope| {
                design::design_feature_family(&scope.kind())
                    == Some(design::DesignFeatureFamily::Sketch)
                    && scope
                        .sketch_entity()
                        .is_some_and(|binding| binding.entity_id == placement.entity_id)
            })
        };
        let valid = scope_valid && unique_record && unique_scope && visibility_valid;
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design sketch placement has an invalid frame or scope link",
                Some(placement.id.clone()),
            );
        }
    }
}

/// Validate design entity reference runs and suffix uniqueness.
pub(super) fn validate_entity_headers(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let mut entity_suffixes = HashSet::new();
    for header in &native.design_entity_headers {
        let native_stream = design_stream(&header.id);
        let references_resolve = header
            .reference_values()
            .all(|index| records_by_index.contains_key(&(native_stream, *index)));
        if !references_resolve {
            push_constant_finding(
                findings,
                Check::ReferentialIntegrity,
                "Fusion design entity has an invalid reference run",
                Some(header.entity_id.as_str().to_owned()),
            );
        }
        let key = (native_stream, header.entity_id.suffix());
        if !entity_suffixes.insert(key) {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design entity suffix is duplicated within its stream",
                Some(header.entity_id.as_str().to_owned()),
            );
        }
    }
}

/// Validate sketch relation owners and byte frames.
pub(super) fn validate_sketch_relations(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let sketch_owner_ids = &ctx.sketch_owner_ids;
    for relation in &native.sketch_relations {
        let native_stream = design_stream(&relation.id);
        let owner_matches = match (
            sketch_owner_ids.get(&(native_stream, relation.owner_reference)),
            relation
                .owner_entity_id
                .as_ref()
                .map(cadmpeg_core::text::NonBlankString::as_str),
        ) {
            (Some(expected), Some(actual)) => *expected == actual,
            (None, None) => true,
            _ => false,
        };
        if !owner_matches {
            push_constant_finding(
                findings,
                Check::ReferentialIntegrity,
                "Fusion sketch relation has an invalid owner or byte frame",
                Some(relation.id.clone()),
            );
        }
    }
}

/// Validate sketch point and surface persistent identities and the typed
/// record of every sketch point, curve, and surface.
pub(super) fn validate_sketch_geometry_identities(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    // First holder of each point identity, by incident-curve count.
    let mut sketch_point_identities = std::collections::HashMap::new();
    let mut sketch_geometry_records = HashSet::new();
    // An unresolved owner is not one shared sketch. Enforce uniqueness only
    // when the owning sketch reference is known. The center point of a fillet
    // arc has one incident curve and repeats the identity of the filleted
    // corner point, which has other incident curves.
    for point in &native.sketch_points {
        let duplicate = if let (Some(persistent_id), Some(owner_reference)) =
            (point.persistent_id(), point.owner_reference)
        {
            let incident = point.companion().incident_curves.len();
            match sketch_point_identities.entry((
                design_stream(&point.id),
                owner_reference,
                persistent_id,
            )) {
                std::collections::hash_map::Entry::Vacant(slot) => {
                    slot.insert(incident);
                    false
                }
                std::collections::hash_map::Entry::Occupied(first) => {
                    (incident == 1) == (*first.get() == 1)
                }
            }
        } else {
            false
        };
        if duplicate {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion sketch point has an invalid persistent identity",
                Some(point.id.clone()),
            );
        }
        if !sketch_geometry_records.insert((design_stream(&point.id), point.record_index)) {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion sketch geometry aliases another typed indexed record",
                Some(point.id.clone()),
            );
        }
    }
    // Distinct curve records of one Sketch can carry one identity pair, so
    // the pair is not checked for uniqueness.
    for curve in &native.sketch_curve_identities {
        if !sketch_geometry_records.insert((design_stream(&curve.id), curve.record_index)) {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion sketch geometry aliases another typed indexed record",
                Some(curve.id.clone()),
            );
        }
    }
    let mut sketch_surface_identities = HashSet::new();
    for surface in &native.sketch_surfaces {
        let duplicate = if let Some(owner_reference) = surface.owner_reference {
            !sketch_surface_identities.insert((
                design_stream(&surface.id),
                owner_reference,
                surface.persistent_id.get(),
            ))
        } else {
            false
        };
        if duplicate {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion sketch surface has an invalid persistent identity",
                Some(surface.id.clone()),
            );
        }
        if !sketch_geometry_records.insert((design_stream(&surface.id), surface.record_index)) {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion sketch geometry aliases another typed indexed record",
                Some(surface.id.clone()),
            );
        }
    }
}

pub(super) fn emit_sketch_relation_finding(
    findings: &mut Vec<Finding>,
    entity: &str,
    message: &'static str,
) {
    let entity = {
        let mut copied = String::new();
        copied.push_str(entity);
        copied
    };
    findings.push(Finding {
        related: Vec::new(),
        check: Check::NativeLinks,
        rule: cadmpeg_ir::rule_name!("f3d/sketches.sketch-relation"),
        severity: Severity::Error,
        message: message.into(),
        entity: Some(entity),
    });
}

pub(super) fn validate_sketch_relation_owners(ctx: &Ctx, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let owners_by_index = &ctx.owners_by_index;
    let companions_by_index = &ctx.companions_by_index;
    let placements_by_scope = &ctx.placements_by_scope;
    let sketch_owner_ids = &ctx.sketch_owner_ids;
    let owned_sketch_records = native
        .sketch_points
        .iter()
        .map(|point| (&point.id, point.record_index, point.owner_reference))
        .chain(
            native
                .sketch_curve_identities
                .iter()
                .map(|curve| (&curve.id, curve.record_index, curve.owner_reference)),
        )
        .chain(
            native
                .sketch_surfaces
                .iter()
                .map(|surface| (&surface.id, surface.record_index, surface.owner_reference)),
        )
        .filter(|(_, _, owner_reference)| owner_reference.is_some())
        .map(|(id, record_index, _)| (design_stream(id), record_index))
        .collect::<std::collections::HashSet<_>>();
    let sketch_operands = native
        .sketch_points
        .iter()
        .map(|point| {
            (
                (design_stream(&point.id), point.record_index),
                records::sketch_relations::SketchRelationOperand::Point {
                    record_index: point.record_index,
                    persistent_id: point.persistent_id(),
                },
            )
        })
        .chain(native.sketch_curve_identities.iter().map(|curve| {
            (
                (design_stream(&curve.id), curve.record_index),
                records::sketch_relations::SketchRelationOperand::Curve {
                    record_index: curve.record_index,
                    primary_id: curve.primary_id.get(),
                    secondary_id: curve.secondary_id,
                },
            )
        }))
        .chain(native.sketch_surfaces.iter().map(|surface| {
            (
                (design_stream(&surface.id), surface.record_index),
                records::sketch_relations::SketchRelationOperand::Surface {
                    record_index: surface.record_index,
                    persistent_id: surface.persistent_id.get(),
                },
            )
        }))
        .collect::<std::collections::HashMap<_, _>>();
    let mut relation_owners = std::collections::HashMap::new();
    for (id, record_index, owner_reference) in native
        .sketch_points
        .iter()
        .map(|point| (&point.id, point.record_index, point.owner_reference))
        .chain(
            native
                .sketch_curve_identities
                .iter()
                .map(|curve| (&curve.id, curve.record_index, curve.owner_reference)),
        )
        .chain(
            native
                .sketch_surfaces
                .iter()
                .map(|surface| (&surface.id, surface.record_index, surface.owner_reference)),
        )
    {
        let Some(owner_reference) = owner_reference else {
            continue;
        };
        let native_stream = design_stream(id);
        if sketch_owner_ids.contains_key(&(native_stream, owner_reference)) {
            relation_owners.insert((native_stream, record_index), owner_reference);
        }
    }
    for relation in &native.sketch_relations {
        let native_stream = design_stream(&relation.id);
        let agrees = |reference: &records::sketch_relations::SketchRelationReference| {
            let record_index = reference.record_index();
            match sketch_operands.get(&(native_stream, record_index)) {
                Some(expected) => reference.resolved() == Some(expected),
                None => {
                    reference.resolved()
                        == Some(&records::sketch_relations::SketchRelationOperand::Record {
                            record_index,
                        })
                }
            }
        };
        if !relation
            .members()
            .iter()
            .all(|member| agrees(&member.reference))
            || !relation
                .return_members()
                .iter()
                .all(|member| agrees(&member.reference))
        {
            emit_sketch_relation_finding(
                findings,
                &relation.id,
                "Fusion sketch relation typed operands disagree with its indexed references",
            );
        }
        if !sketch_owner_ids.contains_key(&(native_stream, relation.owner_reference)) {
            continue;
        }
        for member in relation.all_member_indices() {
            if !owned_sketch_records.contains(&(native_stream, member)) {
                continue;
            }
            if relation_owners
                .insert((native_stream, member), relation.owner_reference)
                .is_some_and(|owner| owner != relation.owner_reference)
            {
                emit_sketch_relation_finding(
                    findings,
                    &relation.id,
                    "Fusion sketch member belongs to multiple sketch owners",
                );
            }
        }
    }
    for entity in native
        .design_entity_headers
        .iter()
        .filter(|entity| entity.in_sketch_module())
    {
        let native_stream = design_stream(&entity.id);
        let Ok(owner) = u32::try_from(entity.entity_id.suffix()) else {
            continue;
        };
        for member in entity.member_values() {
            if !owned_sketch_records.contains(&(native_stream, *member)) {
                continue;
            }
            if relation_owners
                .insert((native_stream, *member), owner)
                .is_some_and(|existing| existing != owner)
            {
                emit_sketch_relation_finding(
                    findings,
                    &entity.id,
                    "Fusion sketch member belongs to multiple sketch owners",
                );
            }
        }
    }
    for pair in &native.design_dimension_locus_pairs {
        let native_stream = design_stream(&pair.id);
        let owner = companions_by_index
            .get(&(native_stream, pair.governing_companion_record_index))
            .and_then(|companion| {
                owners_by_index.get(&(native_stream, companion.owner_record_index()))
            })
            .and_then(|parameter_owner| {
                placements_by_scope.get(&(native_stream, parameter_owner.scope_record_index()))
            })
            .and_then(|placement| u32::try_from(placement.entity_id.suffix()).ok());
        let Some(owner) = owner else {
            continue;
        };
        for member in [
            pair.loci()[0].geometry_index(),
            pair.loci()[1].geometry_index(),
        ] {
            if relation_owners
                .insert((native_stream, member), owner)
                .is_some_and(|existing| existing != owner)
            {
                emit_sketch_relation_finding(
                    findings,
                    &pair.id,
                    "Fusion sketch member belongs to multiple sketch owners",
                );
            }
        }
    }
    for group in &native.design_dimension_locus_groups {
        let native_stream = design_stream(&group.id);
        for member in group
            .loci
            .iter()
            .map(|locus| locus.geometry_record_index)
            .chain(group.loci.iter().map(|locus| locus.returned.value))
        {
            if relation_owners
                .insert((native_stream, member), group.owner_reference)
                .is_some_and(|existing| existing != group.owner_reference)
            {
                emit_sketch_relation_finding(
                    findings,
                    &group.id,
                    "Fusion sketch member belongs to multiple sketch owners",
                );
            }
        }
    }
    for pair in &native.design_dimension_null_locus_pairs {
        let native_stream = design_stream(&pair.id);
        let owner = companions_by_index
            .get(&(native_stream, pair.governing_companion_record_index))
            .and_then(|companion| {
                owners_by_index.get(&(native_stream, companion.owner_record_index()))
            })
            .and_then(|parameter_owner| {
                placements_by_scope.get(&(native_stream, parameter_owner.scope_record_index()))
            })
            .and_then(|placement| u32::try_from(placement.entity_id.suffix()).ok());
        let Some(owner) = owner else {
            continue;
        };
        if relation_owners
            .insert((native_stream, pair.loci()[1].geometry_index()), owner)
            .is_some_and(|existing| existing != owner)
        {
            emit_sketch_relation_finding(
                findings,
                &pair.id,
                "Fusion sketch member belongs to multiple sketch owners",
            );
        }
    }
    for (id, record_index, owner_reference) in native
        .sketch_points
        .iter()
        .map(|point| (&point.id, point.record_index, point.owner_reference))
        .chain(
            native
                .sketch_curve_identities
                .iter()
                .map(|curve| (&curve.id, curve.record_index, curve.owner_reference)),
        )
    {
        if relation_owners
            .get(&(design_stream(id), record_index))
            .copied()
            != owner_reference
        {
            emit_sketch_relation_finding(
                findings,
                id,
                "Fusion sketch geometry owner disagrees with its relation graph",
            );
        }
    }
}

pub(super) fn validate_profile_curve_selection(
    selection: &records::topology::entity_selection::DesignProfileCurveSelection,
    findings: &mut Vec<Finding>,
) {
    if !selection.valid_layout() {
        push_constant_finding(
            findings,
            Check::NativeLinks,
            "Fusion profile curve selection has an invalid region layout",
            Some(selection.id.clone()),
        );
    }
}
