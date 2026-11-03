// SPDX-License-Identifier: Apache-2.0
//! Curve admission and retained curve-construction operands.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::sketches::{
    SketchConstraintDefinitionInput, SketchEntity, SketchGeometry, SketchGeometryDefinition,
    SketchLocus,
};

use super::{curve_ends, CurveEnds, PmDcSketchEntity, PmDcSketchEntityKind, SketchInventory};
use crate::loss::InventorLossCode;
use crate::rse::{RecordFrameState, RseInventory, SegmentBulkState};

pub(crate) fn is_non_geometric(entity: &PmDcSketchEntity) -> bool {
    matches!(
        entity.kind,
        PmDcSketchEntityKind::Polygon { .. }
            | PmDcSketchEntityKind::BlockInstance { .. }
            | PmDcSketchEntityKind::BlockMember { .. }
    )
}

pub(crate) fn relation_stays_native(
    constraint: &super::PmDcSketchConstraint,
    mut non_geometric: impl FnMut(crate::pmdc::PmDcReference) -> bool,
) -> bool {
    use super::PmDcSketchConstraintKind as Kind;
    match constraint.kind {
        Kind::PolygonRelation { .. } | Kind::BlockGroup { .. } => true,
        Kind::CircleCenter {
            entity: first,
            center: second,
        }
        | Kind::Midpoint {
            entity: first,
            point: second,
            ..
        }
        | Kind::SplinePoint {
            spline: first,
            point: second,
        }
        | Kind::Coincident { first, second }
        | Kind::Parallel { first, second, .. }
        | Kind::Perpendicular { first, second, .. }
        | Kind::Tangent { first, second, .. }
        | Kind::HorizontalDistance { first, second, .. }
        | Kind::VerticalDistance { first, second, .. }
        | Kind::EqualRadius { first, second }
        | Kind::EqualLength { first, second }
        | Kind::Distance { first, second, .. }
        | Kind::Angle { first, second, .. }
        | Kind::HorizontalAlignment { first, second }
        | Kind::VerticalAlignment { first, second }
        | Kind::Collinear { first, second, .. } => [first, second].into_iter().any(non_geometric),
        Kind::Horizontal { entity, .. }
        | Kind::Vertical { entity, .. }
        | Kind::Radius { entity, .. }
        | Kind::Diameter { entity, .. }
        | Kind::Fix { entity } => non_geometric(entity),
        Kind::Symmetric {
            first,
            second,
            axis,
        } => [first, second, axis].into_iter().any(non_geometric),
        Kind::EqualDistance { first, second } => first.into_iter().chain(second).any(non_geometric),
    }
}

pub(crate) fn closed_ellipse(entity: &PmDcSketchEntity) -> bool {
    entity.identity.type_id == crate::record_identity::RecordTypeId::from_bytes(super::ELLIPSE_TYPE)
        && matches!(&entity.kind, PmDcSketchEntityKind::Ellipse { points, state: 1, .. }
        if curve_ends(points) == Some(CurveEnds::Closed))
}

fn native_reason(
    entity: &PmDcSketchEntity,
    entities: &HashMap<(&str, u32), Option<&PmDcSketchEntity>>,
) -> Option<&'static str> {
    match &entity.kind {
        PmDcSketchEntityKind::Polygon { .. } => Some(
            "polygon control value and complete ordered boundary remain unresolved",
        ),
        PmDcSketchEntityKind::BlockInstance { .. } => Some(
            "block instance control value and group construction remain unresolved",
        ),
        PmDcSketchEntityKind::BlockMember { .. } => Some(
            "block member link retains instance membership; placed member curves have separate identities",
        ),
        PmDcSketchEntityKind::Spline { .. } => super::spline::native_reason(entity, entities),
        PmDcSketchEntityKind::Ellipse { .. } if !closed_ellipse(entity) => Some(
            "ellipse state and endpoints require a directed trim rule",
        ),
        PmDcSketchEntityKind::Point { .. }
        | PmDcSketchEntityKind::Line { .. }
        | PmDcSketchEntityKind::Circle { .. }
        | PmDcSketchEntityKind::Ellipse { .. } => None,
    }
}

pub(crate) fn native_losses(entities: &[PmDcSketchEntity]) -> Vec<LossNote> {
    let sources = if entities
        .iter()
        .any(|entity| matches!(entity.kind, PmDcSketchEntityKind::Spline { .. }))
    {
        cadmpeg_core::decode::unique_index(entities.iter().map(|entity| {
            (
                (
                    entity.identity.segment_token.as_str(),
                    entity.identity.record_ordinal,
                ),
                entity,
            )
        }))
    } else {
        HashMap::new()
    };
    let mut counts = BTreeMap::new();
    for entity in entities {
        if let Some(reason) = native_reason(entity, &sources) {
            let code = match entity.kind {
                PmDcSketchEntityKind::Spline { .. } => InventorLossCode::SketchSplineCurveNative,
                PmDcSketchEntityKind::Ellipse { .. } => InventorLossCode::SketchEllipseCurveNative,
                PmDcSketchEntityKind::Point { .. }
                | PmDcSketchEntityKind::Line { .. }
                | PmDcSketchEntityKind::Circle { .. }
                | PmDcSketchEntityKind::Polygon { .. }
                | PmDcSketchEntityKind::BlockInstance { .. }
                | PmDcSketchEntityKind::BlockMember { .. } => InventorLossCode::SketchCurveNative,
            };
            *counts
                .entry((code, entity.identity.type_id.as_str(), reason))
                .or_insert(0usize) += 1;
        }
    }
    counts
        .into_iter()
        .map(|((code, record_type, reason), count)| {
            code.note(format!(
                "Retained {count} Inventor sketch entity record(s) of type {record_type}: {reason}."
            ))
        })
        .collect()
}

pub(crate) fn member_losses(
    inventory: &SketchInventory,
    document: &RseInventory<'_>,
) -> Vec<LossNote> {
    let known = inventory
        .entities
        .iter()
        .map(|entity| &entity.identity)
        .chain(
            inventory
                .constraints
                .iter()
                .map(|constraint| &constraint.identity),
        )
        .map(|identity| (identity.segment_token.as_str(), identity.record_ordinal))
        .collect::<HashSet<_>>();
    let unresolved = inventory
        .sketches
        .iter()
        .flat_map(|sketch| {
            sketch
                .entities
                .references()
                .iter()
                .filter_map(move |reference| {
                    Some((
                        sketch.identity.segment_token.as_str(),
                        reference.record_ordinal()?,
                    ))
                })
        })
        .filter(|key| !known.contains(key))
        .collect::<HashSet<_>>();
    if unresolved.is_empty() {
        return Vec::new();
    }
    member_loss_counts(
        &unresolved,
        document
            .segments
            .iter()
            .filter_map(|segment| {
                let SegmentBulkState::Framed(bulk) = &segment.bulk else {
                    return None;
                };
                let RecordFrameState::Framed(table) = &bulk.records else {
                    return None;
                };
                Some(table.records.iter().map(move |record| {
                    (
                        segment.pair.token.key().as_str(),
                        record.ordinal,
                        record.type_id,
                    )
                }))
            })
            .flatten(),
    )
}

fn member_loss_counts<'a>(
    unresolved: &HashSet<(&str, u32)>,
    records: impl IntoIterator<Item = (&'a str, u32, [u8; 16])>,
) -> Vec<LossNote> {
    let mut counts = BTreeMap::new();
    let mut seen = HashSet::new();
    for (token, ordinal, record_type) in records {
        if unresolved.contains(&(token, ordinal)) {
            seen.insert((token, ordinal));
            *counts.entry(record_type).or_insert(0usize) += 1;
        }
    }
    let mut losses = counts.into_iter().map(|(record_type, count)| {
        InventorLossCode::SketchMemberNative.note(format!(
            "Retained {count} Inventor sketch member record(s) of type {}: byte grammar and curve operands remain unresolved.",
            crate::pmdc::type_id_string(record_type),
        ))
    }).collect::<Vec<_>>();
    let missing = unresolved.iter().filter(|key| !seen.contains(*key)).count();
    if missing != 0 {
        losses.push(InventorLossCode::SketchMemberNative.note(format!(
            "Retained {missing} Inventor sketch member reference(s) whose source record is absent."
        )));
    }
    losses
}

pub(crate) fn midpoint_supported(
    constraint: &super::PmDcSketchConstraint,
    geometry: &SketchGeometry,
) -> bool {
    let super::PmDcSketchConstraintKind::Midpoint { value, .. } = constraint.kind else {
        return false;
    };
    let value_matches = if constraint.save_version_major < 20 {
        value.is_none()
    } else {
        value == Some(0)
    };
    value_matches
        && constraint
            .header
            .parameter
            .is_some_and(|parameter| parameter.index() == 0)
        && crate::pmdc::record_version_supported(constraint.save_version_major)
        && constraint.identity.type_id
            == crate::record_identity::RecordTypeId::from_bytes(super::MIDPOINT_TYPE)
        && super::constraint_maps_supported(constraint)
        && matches!(
            geometry.definition(),
            SketchGeometryDefinition::Line { .. } | SketchGeometryDefinition::Arc { .. }
        )
}

pub(super) fn midpoint_definition(
    constraint: &super::PmDcSketchConstraint,
    entity: crate::pmdc::PmDcReference,
    point: crate::pmdc::PmDcReference,
    point_entity: &SketchEntity,
    curve_entity: &SketchEntity,
) -> SketchConstraintDefinitionInput {
    if midpoint_supported(constraint, &curve_entity.geometry) {
        return SketchConstraintDefinitionInput::Midpoint {
            point: SketchLocus::Entity(point_entity.id().clone()),
            entity: curve_entity.id().clone(),
        };
    }
    SketchConstraintDefinitionInput::Native {
        native_kind: cadmpeg_core::nonblank_literal!("midpoint"),
        native_state: Some(u64::from(constraint.header.state.cast_unsigned())),
        native_flags: Some(u64::from(constraint.header.content.flags)),
        native_properties: BTreeMap::new(),
        entities: vec![curve_entity.id().clone(), point_entity.id().clone()],
        parameter: None,
        operands: vec![
            super::native_operand(
                constraint,
                cadmpeg_core::nonblank_literal!("entity"),
                entity,
            ),
            super::native_operand(constraint, cadmpeg_core::nonblank_literal!("point"), point),
        ],
    }
}

pub(crate) fn relation_loss(inventory: &SketchInventory) -> Option<LossNote> {
    if !inventory.constraints.iter().any(|constraint| {
        matches!(
            constraint.kind,
            super::PmDcSketchConstraintKind::Midpoint { .. }
        )
    }) {
        return None;
    }
    let sources = cadmpeg_core::decode::unique_index(inventory.entities.iter().map(|source| {
        (
            (
                source.identity.segment_token.as_str(),
                source.identity.record_ordinal,
            ),
            source,
        )
    }));
    let count = inventory
        .constraints
        .iter()
        .filter(|constraint| {
            let super::PmDcSketchConstraintKind::Midpoint { entity, .. } = constraint.kind else {
                return false;
            };
            let geometry = entity
                .record_ordinal()
                .and_then(|ordinal| {
                    sources.get(&(constraint.identity.segment_token.as_str(), ordinal))
                })
                .and_then(Option::as_ref)
                .and_then(|source| super::project_geometry(source, &sources));
            geometry
                .as_ref()
                .is_none_or(|geometry| !midpoint_supported(constraint, geometry))
        })
        .count();
    (count != 0).then(|| {
        InventorLossCode::SketchMidpointCurveNative.note(format!(
        "Retained {count} Inventor midpoint relation(s): control values or curve locus semantics remain unresolved."
    ))
    })
}

#[cfg(test)]
mod tests;
