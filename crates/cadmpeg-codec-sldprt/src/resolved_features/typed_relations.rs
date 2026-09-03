//! Typed marker relation definitions and geometry predicates.

use super::curves::compact_bounded_curve_tangent;
use super::endpoints::{
    compact_indexed_curve_endpoint_indices, compact_indexed_curve_record_end,
    compact_legacy_code_one_line_endpoint_indices, extended_compact_indexed_curve_endpoint_indices,
    extended_direct_object_line_endpoint_ids, extended_shifted_construction_line_endpoint_indices,
    legacy_marker104_arc_center, legacy_terminal_profile_endpoint_offset,
    marker_profile_curve_role, roster_curve_endpoint_markers, wide_indexed_curve_endpoint_indices,
    wide_indexed_curve_record_is_complete, CompactIndexedCurveRecordEnd,
};
use super::markers::{
    compact_legacy_142_profile_curve_endpoints, finite_coordinate_pair, inline_arc_coordinates,
    legacy_extended_profile_curve_kind, marker_is_geometry_locus, marker_native_code,
    sketch_marker_prefix_at,
};
use super::relation_loci::{
    canonical_profile_loci, line_line_distance, linked_midpoint_operands, linked_single_arc_entity,
    linked_single_ellipse_entity, linked_single_entities, marker_point_locus,
    point_line_distance_value, profile_locus_point, relation_operand_loci, same_dimension_angle,
    same_dimension_length,
};
use super::scalars::operand_kind;
use super::selections::operand_accepts_marker;
use super::transforms::{locus_entity, locus_key, marker_entities, sketch_entity_loci};
use super::{
    LEGACY_EXTENDED_SKETCH_MARKER, LEGACY_SKETCH_MARKER, SKETCH_MARKER, SKETCH_POINT_TOLERANCE,
};
use crate::records::{SketchInputEntity, SketchInputKind, SketchInputLink};
use cadmpeg_core::decode::View;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{
    SketchConstraintDefinition, SketchEntity, SketchEntityId, SketchGeometry, SketchId,
    SketchLocus, SketchNativeOperand,
};
use std::collections::{HashMap, HashSet};

const EPS_TYPED_RELATIONS_TYPED_MARKER_RELATION_DEFINITION_IN_SKETCH_E12: f64 = 1.0e-12;
const EPS_TYPED_RELATIONS_SKETCH_ENTITY_MIDPOINT_E12: f64 = 1.0e-12;
const EPS_TYPED_RELATIONS_SKETCH_ENTITY_CONTAINS_POINT_E12: f64 = 1.0e-12;
const EPS_TYPED_RELATIONS_SKETCH_ENTITY_CONTAINS_POINT_E9: f64 = 1.0e-9;

#[cfg(test)]
pub(super) fn typed_marker_relation_definition(
    marker: &SketchInputEntity,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Option<SketchConstraintDefinition> {
    typed_marker_relation_definition_in_sketch(
        marker,
        &SketchId(String::new()),
        &[],
        markers_by_id,
        loci_by_marker,
    )
}

fn unique_entity_from_link_intersection(
    marker: &SketchInputEntity,
    sketch: &SketchId,
    sketch_entities: &[SketchEntity],
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Option<SketchEntityId> {
    let links = marker
        .links
        .iter()
        .filter(|link| !relation_link_identifies_owner(marker, link))
        .collect::<Vec<_>>();
    let first = links.first()?;
    let mut candidates = marker_entities(&first.entity_ref, markers_by_id, loci_by_marker);
    candidates.retain(|entity| {
        !entity.0.contains("sketch-entity#relation-point:")
            && sketch_entities
                .iter()
                .any(|candidate| candidate.id == *entity && candidate.sketch == *sketch)
            && links.iter().skip(1).all(|link| {
                marker_entities(&link.entity_ref, markers_by_id, loci_by_marker).contains(entity)
            })
    });
    candidates.sort();
    candidates.dedup();
    match candidates.as_slice() {
        [entity] => Some(entity.clone()),
        _ => None,
    }
}

pub(super) fn typed_marker_relation_definition_in_sketch(
    marker: &SketchInputEntity,
    sketch: &SketchId,
    sketch_entities: &[SketchEntity],
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Option<SketchConstraintDefinition> {
    use crate::records::SketchRelationKind::{
        ArcAngle180, ArcAngle270, ArcAngle90, AtIntersection, Coincident, Collinear, Concentric,
        Coradial, EllipseAngle180, EllipseAngle270, EllipseAngle90, Equal, Fixed, Horizontal,
        HorizontalPoints, MergePoints, Midpoint, Parallel, Perpendicular, Symmetric, Tangent,
        Vertical, VerticalPoints,
    };
    let kind = match marker.kind {
        SketchInputKind::Relation(kind) => Some(kind),
        SketchInputKind::Native(_) => None,
        _ => return None,
    };
    if !marker_owns_constraint(marker, markers_by_id) {
        return None;
    }
    let native = || {
        let mut entities = marker
            .links
            .iter()
            .filter(|link| !relation_link_identifies_owner(marker, link))
            .flat_map(|link| marker_entities(&link.entity_ref, markers_by_id, loci_by_marker))
            .collect::<Vec<_>>();
        entities.sort_by(|left, right| left.0.cmp(&right.0));
        entities.dedup();
        let owners = relation_owner_markers(marker, markers_by_id);
        entities.extend(
            owners
                .iter()
                .flat_map(|owner| marker_entities(&owner.id, markers_by_id, loci_by_marker)),
        );
        entities.sort_by(|left, right| left.0.cmp(&right.0));
        entities.dedup();
        let mut operands = marker
            .links
            .iter()
            .map(|link| SketchNativeOperand {
                native_kind: "sldprt:marker-local-id".into(),
                native_field: None,
                native_role: None,
                object_index: u32::from(link.local_id),
                native_ref: Some(link.entity_ref.clone()),
            })
            .collect::<Vec<_>>();
        operands.extend(owners.into_iter().map(|owner| SketchNativeOperand {
            native_kind: "sldprt:marker-constraint-owner".into(),
            native_field: None,
            native_role: None,
            object_index: owner.object_index.or(owner.local_id).unwrap_or(u32::MAX),
            native_ref: Some(owner.id.clone()),
        }));
        SketchConstraintDefinition::Native {
            native_kind: match marker.kind {
                SketchInputKind::Relation(kind) => {
                    format!("sldprt:marker-relation:{}", kind.native_code())
                }
                SketchInputKind::Native(code) => format!("sldprt:marker-relation:{code}"),
                _ => unreachable!("non-relation markers were rejected"),
            },
            native_state: None,
            native_flags: None,
            native_properties: std::collections::BTreeMap::new(),
            entities,
            parameter: None,
            operands,
        }
    };
    let Some(kind) = kind else {
        return Some(native());
    };
    if kind == Fixed {
        if let Some(entity) = unique_entity_from_link_intersection(
            marker,
            sketch,
            sketch_entities,
            markers_by_id,
            loci_by_marker,
        ) {
            return Some(SketchConstraintDefinition::Fixed { entity });
        }
    }
    if matches!(kind, Horizontal | Vertical) {
        // Point targets disambiguate these operands when a local/object index
        // happens to collide with the relation handle's index.
        // Forward point links are explicit operands. Reverse incidences are
        // ownership metadata and must not suppress those operands.
        let point_links = marker
            .links
            .iter()
            .filter(|link| {
                link.entity_ref != marker.id
                    && !matches!(
                        markers_by_id
                            .get(link.entity_ref.as_str())
                            .map(|linked| linked.kind),
                        Some(SketchInputKind::Relation(_))
                    )
            })
            .collect::<Vec<_>>();
        if let [first_link, second_link] = point_links.as_slice() {
            let point_locus = |link: &SketchInputLink| {
                let linked = markers_by_id.get(link.entity_ref.as_str())?;
                if !matches!(
                    linked.kind,
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                ) {
                    return None;
                }
                let mut candidates = sketch_entities.iter().filter(|entity| {
                    entity.sketch == *sketch
                        && entity.native_ref.as_deref() == Some(link.entity_ref.as_str())
                        && matches!(entity.geometry, SketchGeometry::Point { .. })
                });
                match (candidates.next(), candidates.next()) {
                    (Some(entity), None) => Some(SketchLocus::Entity(entity.id.clone())),
                    (None, None) => {
                        let locus =
                            marker_point_locus(&link.entity_ref, markers_by_id, loci_by_marker)?;
                        sketch_entities
                            .iter()
                            .any(|entity| {
                                entity.sketch == *sketch && entity.id == locus_entity(&locus)
                            })
                            .then_some(locus)
                    }
                    _ => None,
                }
            };
            if let (Some(first), Some(second)) = (point_locus(first_link), point_locus(second_link))
            {
                if first != second {
                    return Some(if kind == Horizontal {
                        SketchConstraintDefinition::HorizontalPoints { first, second }
                    } else {
                        SketchConstraintDefinition::VerticalPoints { first, second }
                    });
                }
            }
        }
    }
    Some(match kind {
        Horizontal | Vertical | Fixed => {
            if matches!(kind, Horizontal | Vertical) {
                if let Some([first, second]) = axis_relation_point_loci(
                    marker,
                    sketch,
                    sketch_entities,
                    markers_by_id,
                    loci_by_marker,
                ) {
                    return Some(if kind == Horizontal {
                        SketchConstraintDefinition::HorizontalPoints { first, second }
                    } else {
                        SketchConstraintDefinition::VerticalPoints { first, second }
                    });
                }
            }
            if matches!(kind, Horizontal | Vertical) {
                let point_links = marker
                    .links
                    .iter()
                    .filter(|link| !relation_link_identifies_owner(marker, link))
                    .collect::<Vec<_>>();
                if let [first_link, second_link] = point_links.as_slice() {
                    let point_links = [first_link, second_link];
                    if point_links.into_iter().all(|link| {
                        matches!(
                            markers_by_id
                                .get(link.entity_ref.as_str())
                                .map(|linked| linked.kind),
                            Some(SketchInputKind::Point | SketchInputKind::ConstrainedPoint)
                        )
                    }) {
                        if let Some(loci) =
                            relation_operand_loci(marker, markers_by_id, loci_by_marker)
                        {
                            if let [first, second] = loci.as_slice() {
                                return Some(if kind == Horizontal {
                                    SketchConstraintDefinition::HorizontalPoints {
                                        first: first.clone(),
                                        second: second.clone(),
                                    }
                                } else {
                                    SketchConstraintDefinition::VerticalPoints {
                                        first: first.clone(),
                                        second: second.clone(),
                                    }
                                });
                            }
                        }
                    }
                }
            }
            let inferred_entities =
                marker_entities(marker.id.as_str(), markers_by_id, loci_by_marker);
            let mut exact_entities = marker
                .links
                .iter()
                .filter(|link| !relation_link_identifies_owner(marker, link))
                .flat_map(|link| {
                    let Some(linked) = markers_by_id.get(link.entity_ref.as_str()) else {
                        return Vec::new();
                    };
                    if kind == Fixed
                        && matches!(
                            linked.kind,
                            SketchInputKind::Point
                                | SketchInputKind::ConstrainedPoint
                                | SketchInputKind::LineOrCircle
                                | SketchInputKind::Arc
                        )
                    {
                        return marker_entities(&link.entity_ref, markers_by_id, loci_by_marker);
                    }
                    if !matches!(
                        linked.kind,
                        SketchInputKind::LineOrCircle | SketchInputKind::Arc
                    ) {
                        return Vec::new();
                    }
                    let mut matching = sketch_entities.iter().filter(|entity| {
                        entity.native_ref.as_deref() == Some(link.entity_ref.as_str())
                    });
                    let Some(entity) = matching.next() else {
                        return Vec::new();
                    };
                    if matching.next().is_some() {
                        return Vec::new();
                    }
                    vec![entity.id.clone()]
                })
                .collect::<Vec<_>>();
            exact_entities.sort();
            exact_entities.dedup();
            let direct_entities = if exact_entities.len() == 1 {
                exact_entities
            } else {
                inferred_entities
            };
            let relation_owners = relation_owner_markers(marker, markers_by_id);
            let point_owner_pair = matches!(relation_owners.as_slice(), [first, second]
                if matches!(first.kind, SketchInputKind::Point | SketchInputKind::ConstrainedPoint)
                    && matches!(second.kind, SketchInputKind::Point | SketchInputKind::ConstrainedPoint));
            let owner_entities =
                relation_owner_curve_entities(marker, markers_by_id, loci_by_marker);
            let entities = if point_owner_pair && matches!(kind, Horizontal | Vertical) {
                Vec::new()
            } else {
                match owner_entities.as_slice() {
                    [owner]
                        if direct_entities.iter().all(|entity| {
                            entity == owner || entity.0.contains("sketch-entity#relation-point:")
                        }) =>
                    {
                        owner_entities
                    }
                    _ => direct_entities,
                }
            };
            if let [entity] = entities.as_slice() {
                if matches!(kind, Horizontal | Vertical)
                    && sketch_entities.is_empty()
                    && entity.0.contains("sketch-entity#relation-point:")
                {
                    return Some(native());
                }
                match kind {
                    Horizontal => SketchConstraintDefinition::Horizontal {
                        entity: entity.clone(),
                    },
                    Vertical => SketchConstraintDefinition::Vertical {
                        entity: entity.clone(),
                    },
                    Fixed => SketchConstraintDefinition::Fixed {
                        entity: entity.clone(),
                    },
                    _ => unreachable!("relation kind was filtered above"),
                }
            } else if matches!(kind, Horizontal | Vertical) {
                let loci =
                    relation_operand_loci(marker, markers_by_id, loci_by_marker).or_else(|| {
                        unique_axis_aligned_linked_loci(
                            marker,
                            sketch,
                            sketch_entities,
                            markers_by_id,
                            loci_by_marker,
                            kind == Horizontal,
                        )
                    });
                let Some(loci) = loci else {
                    return Some(native());
                };
                let [first, second] = loci.as_slice() else {
                    return Some(native());
                };
                if kind == Horizontal {
                    SketchConstraintDefinition::HorizontalPoints {
                        first: first.clone(),
                        second: second.clone(),
                    }
                } else {
                    SketchConstraintDefinition::VerticalPoints {
                        first: first.clone(),
                        second: second.clone(),
                    }
                }
            } else {
                return Some(native());
            }
        }
        ArcAngle90 | ArcAngle180 | ArcAngle270 => {
            let Some(entity) = linked_single_arc_entity(marker, markers_by_id, loci_by_marker)
            else {
                return Some(native());
            };
            let angle = match kind {
                ArcAngle90 => std::f64::consts::FRAC_PI_2,
                ArcAngle180 => std::f64::consts::PI,
                ArcAngle270 => 3.0 * std::f64::consts::FRAC_PI_2,
                _ => unreachable!("relation kind was filtered above"),
            };
            if !sketch_entities.is_empty() {
                let Some(SketchEntity {
                    geometry:
                        SketchGeometry::Arc {
                            start_angle,
                            end_angle,
                            ..
                        },
                    ..
                }) = sketch_entities
                    .iter()
                    .find(|candidate| candidate.id == entity)
                else {
                    return Some(native());
                };
                let raw = end_angle.0 - start_angle.0;
                let mut sweep = raw.rem_euclid(std::f64::consts::TAU);
                if sweep <= EPS_TYPED_RELATIONS_TYPED_MARKER_RELATION_DEFINITION_IN_SKETCH_E12
                    && raw.abs()
                        > EPS_TYPED_RELATIONS_TYPED_MARKER_RELATION_DEFINITION_IN_SKETCH_E12
                {
                    sweep = std::f64::consts::TAU;
                }
                if !same_dimension_angle(sweep, angle) {
                    return Some(native());
                }
            }
            SketchConstraintDefinition::ArcAngle {
                entity,
                angle: cadmpeg_ir::features::Angle(angle),
            }
        }
        EllipseAngle90 | EllipseAngle180 | EllipseAngle270 => {
            let Some(entity) = linked_single_ellipse_entity(
                marker,
                markers_by_id,
                loci_by_marker,
                sketch_entities,
            ) else {
                return Some(native());
            };
            let angle = match kind {
                EllipseAngle90 => std::f64::consts::FRAC_PI_2,
                EllipseAngle180 => std::f64::consts::PI,
                EllipseAngle270 => 3.0 * std::f64::consts::FRAC_PI_2,
                _ => unreachable!("relation kind was filtered above"),
            };
            let Some(SketchEntity {
                geometry:
                    SketchGeometry::Ellipse {
                        bounds: Some([start, end]),
                        ..
                    },
                ..
            }) = sketch_entities
                .iter()
                .find(|candidate| candidate.id == entity)
            else {
                return Some(native());
            };
            let raw = end.0 - start.0;
            let mut sweep = raw.rem_euclid(std::f64::consts::TAU);
            if sweep <= EPS_TYPED_RELATIONS_TYPED_MARKER_RELATION_DEFINITION_IN_SKETCH_E12
                && raw.abs() > EPS_TYPED_RELATIONS_TYPED_MARKER_RELATION_DEFINITION_IN_SKETCH_E12
            {
                sweep = std::f64::consts::TAU;
            }
            if !same_dimension_angle(sweep, angle) {
                return Some(native());
            }
            SketchConstraintDefinition::EllipseAngle {
                entity,
                angle: cadmpeg_ir::features::Angle(angle),
            }
        }
        Parallel | Perpendicular | Tangent | Equal | Collinear | Concentric | Coradial => {
            let owner_entities =
                relation_owner_curve_entities(marker, markers_by_id, loci_by_marker);
            let forward_entities = marker
                .links
                .iter()
                .filter(|link| !relation_link_identifies_owner(marker, link))
                .flat_map(|link| marker_entities(&link.entity_ref, markers_by_id, loci_by_marker))
                .filter(|entity| !entity.0.contains("sketch-entity#relation-point:"))
                .collect::<Vec<_>>();
            let geometry_pair = if owner_entities.is_empty() && !sketch_entities.is_empty() {
                let links = marker
                    .links
                    .iter()
                    .filter(|link| !relation_link_identifies_owner(marker, link))
                    .collect::<Vec<_>>();
                if let [first_link, second_link] = links.as_slice() {
                    let candidates = [first_link, second_link].map(|link| {
                        marker_entities(&link.entity_ref, markers_by_id, loci_by_marker)
                            .into_iter()
                            .filter(|entity| {
                                sketch_entities.iter().any(|candidate| {
                                    candidate.id == *entity && candidate.sketch == *sketch
                                })
                            })
                            .collect::<Vec<_>>()
                    });
                    let mut matches = candidates[0]
                        .iter()
                        .flat_map(|first| {
                            candidates[1].iter().filter_map(move |second| {
                                (first != second).then_some((first, second))
                            })
                        })
                        .filter(|(first, second)| {
                            let Some(first_entity) =
                                sketch_entities.iter().find(|entity| entity.id == **first)
                            else {
                                return false;
                            };
                            let Some(second_entity) =
                                sketch_entities.iter().find(|entity| entity.id == **second)
                            else {
                                return false;
                            };
                            binary_relation_matches_evaluated_geometry(
                                kind,
                                first_entity,
                                second_entity,
                            )
                        })
                        .map(|(first, second)| (first.clone(), second.clone()))
                        .collect::<Vec<_>>();
                    matches.sort();
                    matches.dedup();
                    match matches.as_slice() {
                        [pair] => Some(pair.clone()),
                        _ => None,
                    }
                } else {
                    None
                }
            } else {
                None
            };
            let entities = if owner_entities.len() == 2
                && forward_entities
                    .iter()
                    .all(|entity| owner_entities.contains(entity))
            {
                owner_entities
            } else if let Some((first, second)) = geometry_pair {
                vec![first, second]
            } else {
                let Some(entities) = linked_single_entities(marker, markers_by_id, loci_by_marker)
                else {
                    return Some(native());
                };
                entities
            };
            let [first, second] = entities.as_slice() else {
                return Some(native());
            };
            if !sketch_entities.is_empty() {
                let Some(_first_entity) = sketch_entities
                    .iter()
                    .find(|candidate| candidate.id == *first)
                else {
                    return Some(native());
                };
                let Some(_second_entity) = sketch_entities
                    .iter()
                    .find(|candidate| candidate.id == *second)
                else {
                    return Some(native());
                };
            }
            match kind {
                Parallel => SketchConstraintDefinition::Parallel {
                    first: first.clone(),
                    second: second.clone(),
                },
                Perpendicular => SketchConstraintDefinition::Perpendicular {
                    first: first.clone(),
                    second: second.clone(),
                },
                Tangent => SketchConstraintDefinition::Tangent {
                    first: first.clone(),
                    second: second.clone(),
                },
                Equal => SketchConstraintDefinition::Equal {
                    first: first.clone(),
                    second: second.clone(),
                },
                Collinear => SketchConstraintDefinition::Collinear {
                    first: first.clone(),
                    second: second.clone(),
                },
                Concentric => SketchConstraintDefinition::Concentric {
                    first: first.clone(),
                    second: second.clone(),
                },
                Coradial => SketchConstraintDefinition::Coradial {
                    first: first.clone(),
                    second: second.clone(),
                },
                _ => unreachable!("relation kind was filtered above"),
            }
        }
        Coincident | MergePoints => {
            let Some(loci) = relation_operand_loci(marker, markers_by_id, loci_by_marker) else {
                return Some(native());
            };
            if loci.len() < 2 {
                return Some(native());
            }
            if !sketch_entities.is_empty()
                && loci
                    .iter()
                    .any(|locus| profile_locus_point(locus, sketch_entities).is_none())
            {
                return Some(native());
            }
            SketchConstraintDefinition::CoincidentLoci { loci }
        }
        HorizontalPoints | VerticalPoints => {
            let Some(loci) = relation_operand_loci(marker, markers_by_id, loci_by_marker) else {
                return Some(native());
            };
            let [first, second] = loci.as_slice() else {
                return Some(native());
            };
            match kind {
                HorizontalPoints => SketchConstraintDefinition::HorizontalPoints {
                    first: first.clone(),
                    second: second.clone(),
                },
                VerticalPoints => SketchConstraintDefinition::VerticalPoints {
                    first: first.clone(),
                    second: second.clone(),
                },
                _ => unreachable!("relation kind was filtered above"),
            }
        }
        AtIntersection => {
            if sketch_entities.is_empty() {
                return Some(native());
            }
            let Some(loci) = relation_operand_loci(marker, markers_by_id, loci_by_marker) else {
                return Some(native());
            };
            let mut point = None;
            let mut entities = Vec::new();
            for locus in loci {
                let Some(entity) = sketch_entities
                    .iter()
                    .find(|candidate| candidate.id == locus_entity(&locus))
                else {
                    return Some(native());
                };
                let entity_locus = matches!(locus, SketchLocus::Entity(_));
                if entity_locus
                    && !matches!(
                        entity.geometry,
                        SketchGeometry::Point { .. } | SketchGeometry::Native { .. }
                    )
                {
                    entities.push(entity.id.clone());
                } else if point.replace(locus).is_some() {
                    return Some(native());
                }
            }
            let (Some(point), [first, second]) = (point, entities.as_slice()) else {
                return Some(native());
            };
            if first == second {
                return Some(native());
            }
            let Some(position) = profile_locus_point(&point, sketch_entities) else {
                return Some(native());
            };
            if [first, second].into_iter().any(|id| {
                sketch_entities
                    .iter()
                    .find(|entity| entity.id == *id)
                    .is_none_or(|entity| !sketch_entity_contains_point(entity, position))
            }) {
                return Some(native());
            }
            SketchConstraintDefinition::AtIntersection {
                point,
                first: first.clone(),
                second: second.clone(),
            }
        }
        Symmetric => {
            if sketch_entities.is_empty() {
                return Some(native());
            }
            let Some(loci) = relation_operand_loci(marker, markers_by_id, loci_by_marker) else {
                return Some(native());
            };
            let mut axis = None;
            let mut points = Vec::new();
            for locus in loci {
                let entity = sketch_entities
                    .iter()
                    .find(|candidate| candidate.id == locus_entity(&locus));
                if matches!(locus, SketchLocus::Entity(_))
                    && entity.is_some_and(|entity| {
                        matches!(entity.geometry, SketchGeometry::Line { .. })
                    })
                {
                    if axis.replace(locus_entity(&locus)).is_some() {
                        return Some(native());
                    }
                } else {
                    points.push(locus);
                }
            }
            let (Some(axis), [first, second]) = (axis, points.as_slice()) else {
                return Some(native());
            };
            if first == second {
                return Some(native());
            }
            let Some(first_point) = profile_locus_point(first, sketch_entities) else {
                return Some(native());
            };
            let Some(second_point) = profile_locus_point(second, sketch_entities) else {
                return Some(native());
            };
            let Some(axis_entity) = sketch_entities.iter().find(|entity| entity.id == axis) else {
                return Some(native());
            };
            if symmetric_loci_match_axis(first_point, second_point, axis_entity) != Some(true) {
                return Some(native());
            }
            SketchConstraintDefinition::Symmetric {
                first: first.clone(),
                second: second.clone(),
                axis,
            }
        }
        Midpoint => {
            let Some((point, entity)) =
                linked_midpoint_operands(marker, markers_by_id, loci_by_marker)
            else {
                return Some(native());
            };
            if !sketch_entities.is_empty() {
                let Some(point_position) = profile_locus_point(&point, sketch_entities) else {
                    return Some(native());
                };
                let Some(midpoint) = sketch_entities
                    .iter()
                    .find(|candidate| candidate.id == entity)
                    .and_then(sketch_entity_midpoint)
                else {
                    return Some(native());
                };
                if !same_dimension_length(point_position.u, midpoint.u)
                    || !same_dimension_length(point_position.v, midpoint.v)
                {
                    return Some(native());
                }
            }
            SketchConstraintDefinition::Midpoint { point, entity }
        }
        crate::records::SketchRelationKind::Distance
        | crate::records::SketchRelationKind::Angle
        | crate::records::SketchRelationKind::Radius
        | crate::records::SketchRelationKind::Diameter => return None,
        _ => native(),
    })
}

fn sketch_entity_midpoint(entity: &SketchEntity) -> Option<Point2> {
    match &entity.geometry {
        SketchGeometry::Line { start, end } => Some(Point2::new(
            (start.u + end.u) * 0.5,
            (start.v + end.v) * 0.5,
        )),
        SketchGeometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => {
            let raw = end_angle.0 - start_angle.0;
            let mut sweep = raw.rem_euclid(std::f64::consts::TAU);
            if sweep <= EPS_TYPED_RELATIONS_SKETCH_ENTITY_MIDPOINT_E12
                && raw.abs() > EPS_TYPED_RELATIONS_SKETCH_ENTITY_MIDPOINT_E12
            {
                sweep = std::f64::consts::TAU;
            }
            let angle = start_angle.0 + sweep * 0.5;
            Some(Point2::new(
                center.u + radius.0 * angle.cos(),
                center.v + radius.0 * angle.sin(),
            ))
        }
        _ => None,
    }
}

pub(super) fn sketch_entity_contains_point(entity: &SketchEntity, point: Point2) -> bool {
    match &entity.geometry {
        SketchGeometry::Line { start, end } => {
            let du = end.u - start.u;
            let dv = end.v - start.v;
            let length_squared = du * du + dv * dv;
            if length_squared <= SKETCH_POINT_TOLERANCE * SKETCH_POINT_TOLERANCE {
                return false;
            }
            let parameter = ((point.u - start.u) * du + (point.v - start.v) * dv) / length_squared;
            let distance =
                ((point.u - start.u) * dv - (point.v - start.v) * du).abs() / length_squared.sqrt();
            distance <= SKETCH_POINT_TOLERANCE
                && (-SKETCH_POINT_TOLERANCE..=1.0 + SKETCH_POINT_TOLERANCE).contains(&parameter)
        }
        SketchGeometry::ReferenceLine { origin, direction } => {
            let length = direction.u.hypot(direction.v);
            length > SKETCH_POINT_TOLERANCE
                && ((point.u - origin.u) * direction.v - (point.v - origin.v) * direction.u).abs()
                    <= SKETCH_POINT_TOLERANCE * length
        }
        SketchGeometry::Circle { center, radius } => {
            same_dimension_length((point.u - center.u).hypot(point.v - center.v), radius.0)
        }
        SketchGeometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => {
            if !same_dimension_length((point.u - center.u).hypot(point.v - center.v), radius.0) {
                return false;
            }
            let raw = end_angle.0 - start_angle.0;
            let mut sweep = raw.rem_euclid(std::f64::consts::TAU);
            if sweep <= EPS_TYPED_RELATIONS_SKETCH_ENTITY_CONTAINS_POINT_E12
                && raw.abs() > EPS_TYPED_RELATIONS_SKETCH_ENTITY_CONTAINS_POINT_E12
            {
                sweep = std::f64::consts::TAU;
            }
            let parameter = ((point.v - center.v).atan2(point.u - center.u) - start_angle.0)
                .rem_euclid(std::f64::consts::TAU);
            parameter <= sweep + EPS_TYPED_RELATIONS_SKETCH_ENTITY_CONTAINS_POINT_E9
        }
        SketchGeometry::Ellipse {
            center,
            major_angle,
            major_radius,
            minor_radius,
            bounds,
        } => {
            let cosine = major_angle.0.cos();
            let sine = major_angle.0.sin();
            let du = point.u - center.u;
            let dv = point.v - center.v;
            let x = du * cosine + dv * sine;
            let y = -du * sine + dv * cosine;
            let equation = (x / major_radius.0).powi(2) + (y / minor_radius.0).powi(2);
            if (equation - 1.0).abs() > EPS_TYPED_RELATIONS_SKETCH_ENTITY_CONTAINS_POINT_E9 {
                return false;
            }
            match bounds {
                Some([start, end]) => {
                    let parameter = ((y / minor_radius.0).atan2(x / major_radius.0) - start.0)
                        .rem_euclid(std::f64::consts::TAU);
                    let raw = end.0 - start.0;
                    let mut sweep = raw.rem_euclid(std::f64::consts::TAU);
                    if sweep <= EPS_TYPED_RELATIONS_SKETCH_ENTITY_CONTAINS_POINT_E12
                        && raw.abs() > EPS_TYPED_RELATIONS_SKETCH_ENTITY_CONTAINS_POINT_E12
                    {
                        sweep = std::f64::consts::TAU;
                    }
                    parameter <= sweep + EPS_TYPED_RELATIONS_SKETCH_ENTITY_CONTAINS_POINT_E9
                }
                None => true,
            }
        }
        SketchGeometry::Hyperbola {
            center,
            major_angle,
            major_radius,
            minor_radius,
            bounds,
        } => {
            let cosine = major_angle.0.cos();
            let sine = major_angle.0.sin();
            let du = point.u - center.u;
            let dv = point.v - center.v;
            let x = du * cosine + dv * sine;
            let y = -du * sine + dv * cosine;
            let parameter = (y / minor_radius.0).asinh();
            let on_curve = (x - major_radius.0 * parameter.cosh()).abs()
                <= SKETCH_POINT_TOLERANCE * (1.0 + x.abs());
            on_curve
                && bounds.as_ref().is_none_or(|[start, end]| {
                    ((*start).min(*end) - SKETCH_POINT_TOLERANCE
                        ..=(*start).max(*end) + SKETCH_POINT_TOLERANCE)
                        .contains(&parameter)
                })
        }
        SketchGeometry::Parabola {
            vertex,
            axis_angle,
            focal_length,
            bounds,
        } => {
            let cosine = axis_angle.0.cos();
            let sine = axis_angle.0.sin();
            let du = point.u - vertex.u;
            let dv = point.v - vertex.v;
            let x = du * cosine + dv * sine;
            let parameter = -du * sine + dv * cosine;
            let on_curve = (x - parameter * parameter / (4.0 * focal_length.0)).abs()
                <= SKETCH_POINT_TOLERANCE * (1.0 + x.abs());
            on_curve
                && bounds.as_ref().is_none_or(|[start, end]| {
                    ((*start).min(*end) - SKETCH_POINT_TOLERANCE
                        ..=(*start).max(*end) + SKETCH_POINT_TOLERANCE)
                        .contains(&parameter)
                })
        }
        SketchGeometry::Point { .. }
        | SketchGeometry::Text { .. }
        | SketchGeometry::Nurbs { .. }
        | SketchGeometry::ExternalReference { .. }
        | SketchGeometry::Native { .. } => false,
    }
}

pub(super) fn symmetric_loci_match_axis(
    first: Point2,
    second: Point2,
    axis: &SketchEntity,
) -> Option<bool> {
    let SketchGeometry::Line { start, end } = axis.geometry else {
        return None;
    };
    let du = end.u - start.u;
    let dv = end.v - start.v;
    let length = du.hypot(dv);
    if length <= SKETCH_POINT_TOLERANCE {
        return None;
    }
    let coordinates = |point: Point2| {
        (
            ((point.u - start.u) * du + (point.v - start.v) * dv) / length,
            ((point.u - start.u) * dv - (point.v - start.v) * du) / length,
        )
    };
    let (first_along, first_across) = coordinates(first);
    let (second_along, second_across) = coordinates(second);
    Some(
        same_dimension_length(first_along, second_along)
            && same_dimension_length(first_across, -second_across),
    )
}

pub(super) fn binary_relation_matches_evaluated_geometry(
    kind: crate::records::SketchRelationKind,
    first: &SketchEntity,
    second: &SketchEntity,
) -> bool {
    use crate::records::SketchRelationKind::{
        Collinear, Concentric, Coradial, Equal, Parallel, Perpendicular, Tangent,
    };
    match kind {
        Parallel => line_relation_value(first, second, |cross, _dot, lengths| {
            cross.abs() <= SKETCH_POINT_TOLERANCE * lengths
        }),
        Perpendicular => line_relation_value(first, second, |_cross, dot, lengths| {
            dot.abs() <= SKETCH_POINT_TOLERANCE * lengths
        }),
        Collinear => line_line_distance(first, second)
            .is_some_and(|distance| same_dimension_length(distance, 0.0)),
        Concentric => centered_geometry(first)
            .zip(centered_geometry(second))
            .is_some_and(|(first, second)| {
                same_dimension_length(first.u, second.u) && same_dimension_length(first.v, second.v)
            }),
        Coradial => centered_geometry(first)
            .zip(circular_radius(first))
            .zip(centered_geometry(second).zip(circular_radius(second)))
            .is_some_and(
                |((first_center, first_radius), (second_center, second_radius))| {
                    same_dimension_length(first_center.u, second_center.u)
                        && same_dimension_length(first_center.v, second_center.v)
                        && same_dimension_length(first_radius, second_radius)
                },
            ),
        Equal => equal_geometry_size(first, second),
        Tangent => tangent_geometry(first, second),
        _ => false,
    }
}

fn line_relation_value(
    first: &SketchEntity,
    second: &SketchEntity,
    predicate: impl FnOnce(f64, f64, f64) -> bool,
) -> bool {
    let Some((first_u, first_v, first_length)) = line_direction(first) else {
        return false;
    };
    let Some((second_u, second_v, second_length)) = line_direction(second) else {
        return false;
    };
    predicate(
        first_u * second_v - first_v * second_u,
        first_u * second_u + first_v * second_v,
        first_length * second_length,
    )
}

fn line_direction(entity: &SketchEntity) -> Option<(f64, f64, f64)> {
    let SketchGeometry::Line { start, end } = &entity.geometry else {
        return None;
    };
    let u = end.u - start.u;
    let v = end.v - start.v;
    let length = u.hypot(v);
    (length > SKETCH_POINT_TOLERANCE).then_some((u, v, length))
}

fn centered_geometry(entity: &SketchEntity) -> Option<Point2> {
    match &entity.geometry {
        SketchGeometry::Circle { center, .. }
        | SketchGeometry::Arc { center, .. }
        | SketchGeometry::Ellipse { center, .. } => Some(*center),
        _ => None,
    }
}

fn circular_radius(entity: &SketchEntity) -> Option<f64> {
    match &entity.geometry {
        SketchGeometry::Circle { radius, .. } | SketchGeometry::Arc { radius, .. } => {
            Some(radius.0)
        }
        _ => None,
    }
}

fn equal_geometry_size(first: &SketchEntity, second: &SketchEntity) -> bool {
    match (&first.geometry, &second.geometry) {
        (
            SketchGeometry::Line {
                start: first_start,
                end: first_end,
            },
            SketchGeometry::Line {
                start: second_start,
                end: second_end,
            },
        ) => same_dimension_length(
            (first_end.u - first_start.u).hypot(first_end.v - first_start.v),
            (second_end.u - second_start.u).hypot(second_end.v - second_start.v),
        ),
        (
            SketchGeometry::Circle {
                radius: first_radius,
                ..
            }
            | SketchGeometry::Arc {
                radius: first_radius,
                ..
            },
            SketchGeometry::Circle {
                radius: second_radius,
                ..
            }
            | SketchGeometry::Arc {
                radius: second_radius,
                ..
            },
        ) => same_dimension_length(first_radius.0, second_radius.0),
        (
            SketchGeometry::Ellipse {
                major_radius: first_major,
                minor_radius: first_minor,
                ..
            },
            SketchGeometry::Ellipse {
                major_radius: second_major,
                minor_radius: second_minor,
                ..
            },
        ) => {
            same_dimension_length(first_major.0, second_major.0)
                && same_dimension_length(first_minor.0, second_minor.0)
        }
        _ => false,
    }
}

fn tangent_geometry(first: &SketchEntity, second: &SketchEntity) -> bool {
    let line_circle = |line: &SketchEntity, circle: &SketchEntity| {
        if let SketchGeometry::Ellipse {
            center,
            major_angle,
            major_radius,
            minor_radius,
            ..
        } = &circle.geometry
        {
            let Some((du, dv, length)) = line_direction(line) else {
                return false;
            };
            let normal = [-dv / length, du / length];
            let major = [major_angle.0.cos(), major_angle.0.sin()];
            let minor = [-major[1], major[0]];
            let support = ((major_radius.0 * (normal[0] * major[0] + normal[1] * major[1]))
                .powi(2)
                + (minor_radius.0 * (normal[0] * minor[0] + normal[1] * minor[1])).powi(2))
            .sqrt();
            return point_line_distance_value(*center, line)
                .is_some_and(|distance| same_dimension_length(distance, support));
        }
        centered_geometry(circle)
            .zip(circular_radius(circle))
            .and_then(|(center, radius)| {
                point_line_distance_value(center, line).map(|distance| (distance, radius))
            })
            .is_some_and(|(distance, radius)| same_dimension_length(distance, radius))
    };
    if matches!(first.geometry, SketchGeometry::Line { .. }) {
        return line_circle(first, second);
    }
    if matches!(second.geometry, SketchGeometry::Line { .. }) {
        return line_circle(second, first);
    }
    centered_geometry(first)
        .zip(circular_radius(first))
        .zip(centered_geometry(second).zip(circular_radius(second)))
        .is_some_and(
            |((first_center, first_radius), (second_center, second_radius))| {
                let center_distance =
                    (second_center.u - first_center.u).hypot(second_center.v - first_center.v);
                same_dimension_length(center_distance, first_radius + second_radius)
                    || same_dimension_length(center_distance, (first_radius - second_radius).abs())
            },
        )
}

pub(super) fn unique_axis_aligned_linked_loci(
    marker: &SketchInputEntity,
    sketch: &SketchId,
    sketch_entities: &[SketchEntity],
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
    horizontal: bool,
) -> Option<Vec<SketchLocus>> {
    let links = marker
        .links
        .iter()
        .filter(|link| relation_link_is_geometric_operand(marker, link, markers_by_id))
        .collect::<Vec<_>>();
    let [first_link, second_link] = links.as_slice() else {
        return None;
    };
    let first = marker_point_locus(&first_link.entity_ref, markers_by_id, loci_by_marker);
    let second = marker_point_locus(&second_link.entity_ref, markers_by_id, loci_by_marker);
    let (known, known_is_first) = match (first, second) {
        (Some(known), None) => (known, true),
        (None, Some(known)) => (known, false),
        _ => return None,
    };
    let point = |locus: &SketchLocus| {
        let entity = sketch_entities
            .iter()
            .find(|entity| entity.id == locus_entity(locus))?;
        sketch_entity_loci(entity)
            .into_iter()
            .find_map(|(point, candidate)| (candidate == *locus).then_some(point))
    };
    let known_point = point(&known)?;
    let mut candidates = canonical_profile_loci(sketch, sketch_entities)
        .into_iter()
        .filter_map(|(candidate_point, candidate)| {
            let aligned = if horizontal {
                same_dimension_length(candidate_point.v, known_point.v)
            } else {
                same_dimension_length(candidate_point.u, known_point.u)
            };
            (candidate != known && aligned).then_some(candidate)
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| locus_key(left).cmp(&locus_key(right)));
    candidates.dedup();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(if known_is_first {
        vec![known, candidate.clone()]
    } else {
        vec![candidate.clone(), known]
    })
}

fn axis_relation_point_loci(
    relation: &SketchInputEntity,
    sketch: &SketchId,
    sketch_entities: &[SketchEntity],
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Option<[SketchLocus; 2]> {
    if !relation.links.iter().any(|link| {
        !relation_link_identifies_owner(relation, link)
            && matches!(
                markers_by_id
                    .get(link.entity_ref.as_str())
                    .map(|marker| marker.kind),
                Some(SketchInputKind::Relation(_))
            )
    }) {
        return None;
    }
    let mut loci = Vec::new();
    // Forward relation links are the solver handle's explicit operands. Reverse
    // incidences describe owners and are only a fallback for incomplete links.
    collect_axis_relation_point_loci(
        relation,
        sketch,
        sketch_entities,
        markers_by_id,
        loci_by_marker,
        &mut HashSet::new(),
        &mut loci,
        false,
    );
    loci.sort_by(|left, right| locus_key(left).cmp(&locus_key(right)));
    loci.dedup();
    if loci.len() == 2 {
        return loci.try_into().ok();
    }
    if loci.len() > 2 {
        return None;
    }
    loci.clear();
    collect_axis_relation_point_loci(
        relation,
        sketch,
        sketch_entities,
        markers_by_id,
        loci_by_marker,
        &mut HashSet::new(),
        &mut loci,
        true,
    );
    loci.sort_by(|left, right| locus_key(left).cmp(&locus_key(right)));
    loci.dedup();
    loci.try_into().ok()
}

// The collector keeps the sketch, marker indexes, and locus indexes separate
// because each lookup has a distinct ownership boundary.
#[allow(clippy::too_many_arguments)]
fn collect_axis_relation_point_loci(
    relation: &SketchInputEntity,
    sketch: &SketchId,
    sketch_entities: &[SketchEntity],
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
    visited: &mut HashSet<String>,
    loci: &mut Vec<SketchLocus>,
    include_reverse_owners: bool,
) {
    if !visited.insert(relation.id.clone()) {
        return;
    }
    for link in relation
        .links
        .iter()
        .filter(|link| !relation_link_identifies_owner(relation, link))
    {
        let Some(linked) = markers_by_id.get(link.entity_ref.as_str()) else {
            continue;
        };
        match linked.kind {
            SketchInputKind::Point | SketchInputKind::ConstrainedPoint => {
                append_axis_relation_point_locus(
                    &linked.id,
                    sketch,
                    sketch_entities,
                    markers_by_id,
                    loci_by_marker,
                    loci,
                );
            }
            SketchInputKind::Relation(_) => collect_axis_relation_point_loci(
                linked,
                sketch,
                sketch_entities,
                markers_by_id,
                loci_by_marker,
                visited,
                loci,
                include_reverse_owners,
            ),
            _ => {}
        }
    }
    if include_reverse_owners {
        for owner in relation_owner_markers(relation, markers_by_id) {
            if matches!(
                owner.kind,
                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
            ) {
                append_axis_relation_point_locus(
                    &owner.id,
                    sketch,
                    sketch_entities,
                    markers_by_id,
                    loci_by_marker,
                    loci,
                );
            }
        }
    }
}

fn append_axis_relation_point_locus(
    marker_id: &str,
    sketch: &SketchId,
    sketch_entities: &[SketchEntity],
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
    loci: &mut Vec<SketchLocus>,
) {
    if let Some(locus) = marker_point_locus(marker_id, markers_by_id, loci_by_marker) {
        if !loci.contains(&locus) {
            loci.push(locus);
        }
        return;
    }
    let candidates = sketch_entities
        .iter()
        .filter(|entity| entity.sketch == *sketch)
        .filter(|entity| entity.native_ref.as_deref() == Some(marker_id))
        .filter(|entity| matches!(entity.geometry, SketchGeometry::Point { .. }))
        .map(|entity| SketchLocus::Entity(entity.id.clone()))
        .collect::<Vec<_>>();
    if let [locus] = candidates.as_slice() {
        if !loci.contains(locus) {
            loci.push(locus.clone());
        }
    }
}

pub(super) fn relation_owner_markers<'a>(
    relation: &SketchInputEntity,
    markers_by_id: &'a HashMap<&str, &SketchInputEntity>,
) -> Vec<&'a SketchInputEntity> {
    let mut owners = markers_by_id
        .values()
        .copied()
        .filter(|marker| marker.feature_ref == relation.feature_ref)
        .filter(|marker| {
            matches!(
                marker.kind,
                SketchInputKind::Point
                    | SketchInputKind::LineOrCircle
                    | SketchInputKind::Arc
                    | SketchInputKind::ConstrainedPoint
            )
        })
        .filter(|marker| {
            marker
                .links
                .iter()
                .any(|link| link.entity_ref == relation.id)
        })
        .collect::<Vec<_>>();
    owners.sort_unstable_by_key(|marker| marker.offset);
    owners
}

pub(crate) fn marker_owns_constraint(
    marker: &SketchInputEntity,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
) -> bool {
    let mut axis_point_links = marker
        .links
        .iter()
        .filter(|link| link.entity_ref != marker.id)
        .filter(|link| {
            !matches!(
                markers_by_id
                    .get(link.entity_ref.as_str())
                    .map(|linked| linked.kind),
                Some(SketchInputKind::Relation(_))
            )
        });
    let axis_point_pair = matches!(
        marker.kind,
        SketchInputKind::Relation(
            crate::records::SketchRelationKind::Horizontal
                | crate::records::SketchRelationKind::Vertical
        )
    ) && relation_owner_markers(marker, markers_by_id).is_empty()
        && axis_point_links.clone().count() == 2
        && axis_point_links.all(|link| {
            matches!(
                markers_by_id
                    .get(link.entity_ref.as_str())
                    .map(|linked| linked.kind),
                Some(SketchInputKind::Point | SketchInputKind::ConstrainedPoint)
            )
        });
    marker.kind.owns_constraint()
        && (axis_point_pair
            || marker
                .links
                .iter()
                .any(|link| !relation_link_identifies_owner(marker, link))
            || !relation_owner_markers(marker, markers_by_id).is_empty())
}

pub(super) fn relation_link_identifies_owner(
    relation: &SketchInputEntity,
    link: &crate::records::SketchInputLink,
) -> bool {
    link.entity_ref == relation.id || relation.local_id == Some(u32::from(link.local_id))
}

pub(super) fn relation_link_is_geometric_operand(
    relation: &SketchInputEntity,
    link: &crate::records::SketchInputLink,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
) -> bool {
    // Relation markers are solver handles; geometric operands come from direct
    // links or reverse incidence, never from a relation-to-relation chain.
    !relation_link_identifies_owner(relation, link)
        && !matches!(
            markers_by_id
                .get(link.entity_ref.as_str())
                .map(|marker| marker.kind),
            Some(SketchInputKind::Relation(_))
        )
}

fn typed_axis_relation_is_inactive(
    definition: &SketchConstraintDefinition,
    sketch_entities: &[SketchEntity],
) -> Option<bool> {
    let entity = |id: &SketchEntityId| sketch_entities.iter().find(|entity| entity.id == *id);
    match definition {
        SketchConstraintDefinition::Horizontal { entity: id }
        | SketchConstraintDefinition::Vertical { entity: id } => {
            let SketchGeometry::Line { start, end } = &entity(id)?.geometry else {
                return Some(true);
            };
            Some(
                if matches!(definition, SketchConstraintDefinition::Horizontal { .. }) {
                    !same_dimension_length(start.v, end.v)
                } else {
                    !same_dimension_length(start.u, end.u)
                },
            )
        }
        SketchConstraintDefinition::HorizontalPoints { first, second }
        | SketchConstraintDefinition::VerticalPoints { first, second } => {
            let first = profile_locus_point(first, sketch_entities)?;
            let second = profile_locus_point(second, sketch_entities)?;
            Some(
                if matches!(
                    definition,
                    SketchConstraintDefinition::HorizontalPoints { .. }
                ) {
                    !same_dimension_length(first.v, second.v)
                } else {
                    !same_dimension_length(first.u, second.u)
                },
            )
        }
        _ => None,
    }
}

fn typed_binary_relation_is_inactive(
    kind: crate::records::SketchRelationKind,
    definition: &SketchConstraintDefinition,
    sketch_entities: &[SketchEntity],
) -> Option<bool> {
    use crate::records::SketchRelationKind::{
        Collinear, Concentric, Coradial, Equal, Parallel, Perpendicular, Tangent,
    };
    let ((Parallel, SketchConstraintDefinition::Parallel { first, second })
    | (Perpendicular, SketchConstraintDefinition::Perpendicular { first, second })
    | (Tangent, SketchConstraintDefinition::Tangent { first, second })
    | (Equal, SketchConstraintDefinition::Equal { first, second })
    | (Collinear, SketchConstraintDefinition::Collinear { first, second })
    | (Concentric, SketchConstraintDefinition::Concentric { first, second })
    | (Coradial, SketchConstraintDefinition::Coradial { first, second })) = (kind, definition)
    else {
        return None;
    };
    let first = sketch_entities.iter().find(|entity| entity.id == *first)?;
    let second = sketch_entities.iter().find(|entity| entity.id == *second)?;
    Some(!binary_relation_matches_evaluated_geometry(
        kind, first, second,
    ))
}

pub(super) fn marker_relation_is_inactive(
    marker: &SketchInputEntity,
    definition: &SketchConstraintDefinition,
    sketch_entities: &[SketchEntity],
) -> bool {
    use crate::records::SketchRelationKind::{
        ArcAngle180, ArcAngle270, ArcAngle90, Collinear, Concentric, Coradial, EllipseAngle180,
        EllipseAngle270, EllipseAngle90, Equal, Horizontal, MergePoints, Parallel, Perpendicular,
        Tangent, Vertical,
    };

    let SketchInputKind::Relation(kind) = marker.kind else {
        return false;
    };
    if let Some(inactive) = typed_axis_relation_is_inactive(definition, sketch_entities) {
        return inactive;
    }
    if let Some(inactive) = typed_binary_relation_is_inactive(kind, definition, sketch_entities) {
        return inactive;
    }
    if let SketchConstraintDefinition::CoincidentLoci { loci } = definition {
        let Some(points) = loci
            .iter()
            .map(|locus| profile_locus_point(locus, sketch_entities))
            .collect::<Option<Vec<_>>>()
        else {
            return false;
        };
        return points.iter().skip(1).any(|point| {
            !same_dimension_length(point.u, points[0].u)
                || !same_dimension_length(point.v, points[0].v)
        });
    }
    let SketchConstraintDefinition::Native {
        entities, operands, ..
    } = definition
    else {
        return false;
    };
    let repeated_single_operand = operands.len() >= 2
        && operands[0].native_ref.is_some()
        && operands
            .iter()
            .all(|operand| operand.native_ref == operands[0].native_ref);
    if repeated_single_operand
        && matches!(
            kind,
            Horizontal
                | Vertical
                | Parallel
                | Perpendicular
                | Tangent
                | Equal
                | Collinear
                | Concentric
                | Coradial
        )
    {
        return true;
    }
    if entities.is_empty() || sketch_entities.is_empty() {
        return false;
    }
    let resolved = entities
        .iter()
        .filter_map(|id| sketch_entities.iter().find(|entity| entity.id == *id))
        .collect::<Vec<_>>();
    if resolved.len() != entities.len() {
        return false;
    }
    match kind {
        ArcAngle90 | ArcAngle180 | ArcAngle270 => !matches!(
            resolved.as_slice(),
            [SketchEntity {
                geometry: SketchGeometry::Arc { .. },
                ..
            }]
        ),
        EllipseAngle90 | EllipseAngle180 | EllipseAngle270 => !matches!(
            resolved.as_slice(),
            [SketchEntity {
                geometry: SketchGeometry::Ellipse { .. },
                ..
            }]
        ),
        Horizontal | Vertical => !matches!(
            resolved.as_slice(),
            [SketchEntity {
                geometry: SketchGeometry::Line { .. },
                ..
            }] | [
                SketchEntity {
                    geometry: SketchGeometry::Point { .. },
                    ..
                },
                SketchEntity {
                    geometry: SketchGeometry::Point { .. },
                    ..
                }
            ]
        ),
        Parallel | Perpendicular | Tangent | Equal | Collinear | Concentric | Coradial => {
            resolved.len() != 2
                || resolved.iter().any(|entity| {
                    matches!(
                        entity.geometry,
                        SketchGeometry::Point { .. } | SketchGeometry::Native { .. }
                    )
                })
        }
        crate::records::SketchRelationKind::Coincident | MergePoints => {
            let [SketchEntity {
                geometry: SketchGeometry::Point { position: first },
                ..
            }, SketchEntity {
                geometry: SketchGeometry::Point { position: second },
                ..
            }] = resolved.as_slice()
            else {
                return false;
            };
            !same_dimension_length(first.u, second.u) || !same_dimension_length(first.v, second.v)
        }
        _ => false,
    }
}

fn relation_owner_curve_entities(
    relation: &SketchInputEntity,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Vec<SketchEntityId> {
    let mut entities = relation_owner_markers(relation, markers_by_id)
        .into_iter()
        .filter(|owner| {
            matches!(
                owner.kind,
                SketchInputKind::LineOrCircle | SketchInputKind::Arc
            )
        })
        .flat_map(|owner| marker_entities(&owner.id, markers_by_id, loci_by_marker))
        .collect::<Vec<_>>();
    entities.sort_by(|left, right| left.0.cmp(&right.0));
    entities.dedup();
    entities
}

pub(super) fn line_endpoint_markers<'a>(
    line: &SketchInputEntity,
    markers_by_id: &HashMap<&str, &'a SketchInputEntity>,
) -> Vec<&'a SketchInputEntity> {
    let mut endpoints = line
        .links
        .iter()
        .filter_map(|link| markers_by_id.get(link.entity_ref.as_str()).copied())
        .chain(markers_by_id.values().copied().filter(|candidate| {
            candidate
                .links
                .iter()
                .any(|link| link.entity_ref == line.id)
        }))
        .filter(|endpoint| {
            endpoint.feature_ref == line.feature_ref
                && endpoint.coordinates_m.is_some()
                && matches!(
                    endpoint.kind,
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
        })
        .collect::<Vec<_>>();
    endpoints.sort_unstable_by_key(|endpoint| endpoint.offset);
    endpoints.dedup_by_key(|endpoint| endpoint.id.as_str());
    endpoints
}

pub(super) fn marker_curve_endpoint_markers<'a>(
    payload: &[u8],
    curve: &'a SketchInputEntity,
    markers_by_id: &HashMap<&str, &'a SketchInputEntity>,
    markers: &[&'a SketchInputEntity],
) -> Vec<&'a SketchInputEntity> {
    if let Some(endpoints) = extended_direct_object_line_endpoints(payload, curve, markers) {
        return endpoints.to_vec();
    }
    let endpoints = line_endpoint_markers(curve, markers_by_id);
    if endpoints.len() == 2 {
        return endpoints;
    }
    let shifted = usize::try_from(curve.offset).ok().and_then(|offset| {
        extended_shifted_construction_line_endpoint_indices(payload, offset)
            .map(|indices| (offset, indices))
    });
    if let Some((offset, indices)) = shifted {
        let endpoints = if payload.get(offset + 72..offset + 76) == Some(&[0; 4]) {
            let mut owned = markers
                .iter()
                .copied()
                .filter(|marker| marker.feature_ref == curve.feature_ref)
                .collect::<Vec<_>>();
            owned.sort_unstable_by_key(|marker| marker.offset);
            indices
                .into_iter()
                .filter_map(|index| {
                    let index = usize::try_from(index).ok()?.checked_sub(1)?;
                    owned.get(index).copied().filter(|marker| {
                        marker.coordinates_m.is_some()
                            && matches!(
                                marker.kind,
                                SketchInputKind::Point
                                    | SketchInputKind::ConstrainedPoint
                                    | SketchInputKind::LineOrCircle
                                    | SketchInputKind::Arc
                            )
                    })
                })
                .collect::<Vec<_>>()
        } else {
            super::endpoints::coordinate_roster_curve_endpoint_markers_at(
                payload,
                curve,
                markers,
                Some(56),
            )
        };
        if endpoints.len() == 2 {
            return endpoints;
        }
    }
    if let Some(endpoints) = compact_legacy_object_line_endpoints(payload, curve, markers) {
        return endpoints.to_vec();
    }
    if let Some(endpoints) = extended_wide_selected_axis_endpoints(payload, curve, markers) {
        return endpoints.to_vec();
    }
    if let Some(endpoints) = inline_arc_endpoint_markers(payload, curve, markers) {
        return endpoints.to_vec();
    }
    if let Some(endpoints) =
        compact_legacy_142_profile_curve_endpoint_markers(payload, curve, markers)
    {
        return endpoints.to_vec();
    }
    if let Some(endpoints) = one_based_point_roster_line_endpoint_markers(payload, curve, markers) {
        return endpoints.to_vec();
    }
    if let Some(endpoints) = legacy_point_roster_line_endpoint_markers(payload, curve, markers) {
        return endpoints.to_vec();
    }
    let endpoints = roster_curve_endpoint_markers(payload, curve, markers);
    if endpoints.len() == 2 {
        if let Some(direct) = legacy_marker104_arc_endpoints(payload, curve, markers) {
            let roster = [endpoints[0], endpoints[1]];
            if legacy_marker104_arc_center(payload, curve, markers, roster).is_none()
                && legacy_marker104_arc_center(payload, curve, markers, direct).is_some()
            {
                return direct.to_vec();
            }
        }
        return endpoints;
    }
    if let Some(endpoints) = legacy_marker104_arc_endpoints(payload, curve, markers) {
        return endpoints.to_vec();
    }
    if let Some(endpoints) = current_coordinate_linked_line_endpoints(payload, curve, markers) {
        return endpoints.to_vec();
    }
    if let Some(endpoints) = coordinate_centered_line_endpoints(payload, curve, markers) {
        return endpoints.to_vec();
    }
    if let Some(endpoints) = legacy_terminal_profile_indexed_endpoints(payload, curve, markers) {
        return endpoints.to_vec();
    }
    let endpoints = consecutive_legacy_profile_line_endpoints(payload, curve, markers);
    if endpoints.len() == 2 {
        return endpoints;
    }
    coordinate_profile_line_endpoints(payload, curve, markers_by_id)
        .map(|endpoints| endpoints.to_vec())
        .unwrap_or(endpoints)
}

fn coordinate_profile_line_endpoints<'a>(
    payload: &[u8],
    curve: &'a SketchInputEntity,
    markers_by_id: &HashMap<&str, &'a SketchInputEntity>,
) -> Option<[&'a SketchInputEntity; 2]> {
    let offset = usize::try_from(curve.offset).ok()?;
    if curve.kind != SketchInputKind::LineOrCircle
        || curve.coordinates_m.is_none()
        || !matches!(
            payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len()),
            Some(prefix) if prefix == SKETCH_MARKER || prefix == LEGACY_EXTENDED_SKETCH_MARKER
        )
        || marker_native_code(payload, offset) != Some(1)
        || !marker_is_geometry_locus(payload, offset)
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 64..offset + 66) != Some(&[0x1e, 0x00])
    {
        return None;
    }
    let mut point = None;
    for link in curve
        .links
        .iter()
        .filter(|link| link.entity_ref != curve.id)
    {
        let linked = markers_by_id.get(link.entity_ref.as_str()).copied()?;
        if linked.feature_ref != curve.feature_ref {
            return None;
        }
        match linked.kind {
            SketchInputKind::Point | SketchInputKind::ConstrainedPoint => {
                if linked.coordinates_m.is_none() || point.replace(linked).is_some() {
                    return None;
                }
            }
            SketchInputKind::Relation(_) => {}
            SketchInputKind::LineOrCircle | SketchInputKind::Arc | SketchInputKind::Native(_) => {
                return None
            }
        }
    }
    let point = point?;
    (curve.coordinates_m? != point.coordinates_m?).then_some([curve, point])
}

pub(super) fn extended_direct_object_line_endpoints<'a>(
    payload: &[u8],
    curve: &SketchInputEntity,
    markers: &[&'a SketchInputEntity],
) -> Option<[&'a SketchInputEntity; 2]> {
    if curve.kind != SketchInputKind::LineOrCircle {
        return None;
    }
    let offset = usize::try_from(curve.offset).ok()?;
    let endpoint_ids = extended_direct_object_line_endpoint_ids(payload, offset)?;
    let resolve = |id| {
        let mut candidates = markers.iter().copied().filter(|marker| {
            marker.feature_ref == curve.feature_ref
                && marker.coordinates_m.is_some()
                && matches!(
                    marker.kind,
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
                && if id == 0 {
                    marker.object_index.is_none()
                } else {
                    marker.object_index == Some(id)
                }
        });
        let marker = candidates.next()?;
        candidates.next().is_none().then_some(marker)
    };
    let endpoints = [resolve(endpoint_ids[0])?, resolve(endpoint_ids[1])?];
    (endpoints[0].id != endpoints[1].id && endpoints[0].coordinates_m != endpoints[1].coordinates_m)
        .then_some(endpoints)
}

pub(super) fn compact_legacy_object_line_endpoints<'a>(
    payload: &[u8],
    curve: &SketchInputEntity,
    markers: &[&'a SketchInputEntity],
) -> Option<[&'a SketchInputEntity; 2]> {
    let offset = usize::try_from(curve.offset).ok()?;
    let endpoint_ids = compact_legacy_code_one_line_endpoint_indices(payload, offset)?;
    let resolve = |id| {
        let mut candidates = markers.iter().copied().filter(|marker| {
            marker.feature_ref == curve.feature_ref
                && marker.object_index == Some(id)
                && marker.coordinates_m.is_some()
                && matches!(
                    marker.kind,
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
        });
        let marker = candidates.next()?;
        candidates.next().is_none().then_some(marker)
    };
    let endpoints = [resolve(endpoint_ids[0])?, resolve(endpoint_ids[1])?];
    (endpoints[0].id != endpoints[1].id && endpoints[0].coordinates_m != endpoints[1].coordinates_m)
        .then_some(endpoints)
}

pub(super) fn extended_wide_selected_axis_endpoints<'a>(
    payload: &[u8],
    curve: &SketchInputEntity,
    markers: &[&'a SketchInputEntity],
) -> Option<[&'a SketchInputEntity; 2]> {
    let offset = usize::try_from(curve.offset).ok()?;
    if curve.kind != SketchInputKind::LineOrCircle
        || payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len())
            != Some(LEGACY_EXTENDED_SKETCH_MARKER)
        || payload.get(offset + 5..offset + 13) != Some(&[0xff; 8])
        || payload.get(offset + 13..offset + 17) != Some(&[0x00, 0x00, 0x80, 0xbf])
        || marker_native_code(payload, offset) != Some(2)
        || payload.get(offset + 23..offset + 31)
            != Some(&[0x04, 0x00, 0x02, 0x00, 0x02, 0x00, 0x00, 0x00])
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00])
        || payload.get(offset + 39..offset + 48) != Some(&[0; 9])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 56..offset + 64) != Some(&[0; 8])
        || payload.get(offset + 68..offset + 72) != Some(&[0; 4])
        || payload.get(offset + 72..offset + 80) != Some(&(-1.0f64).to_le_bytes())
        || payload.get(offset + 80..offset + 84) != Some(&[0x00, 0x00, 0x02, 0x00])
        || payload.get(offset + 84..offset + 88) != Some(&[0; 4])
        || !sketch_marker_prefix_at(payload, offset.checked_add(92)?)
    {
        return None;
    }
    let endpoint = |relative| Some(u32::from(View::u16_le_at(payload, offset + relative)?));
    let encoded = [endpoint(64)?, endpoint(66)?];
    if encoded[0] == encoded[1] || encoded.contains(&u32::from(u16::MAX)) {
        return None;
    }
    let resolve_object = |index| {
        let mut candidates = markers.iter().copied().filter(|marker| {
            marker.feature_ref == curve.feature_ref
                && marker.object_index == Some(index)
                && marker.coordinates_m.is_some()
                && matches!(
                    marker.kind,
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
        });
        let candidate = candidates.next()?;
        candidates.next().is_none().then_some(candidate)
    };
    let object_endpoints = encoded.map(|index| resolve_object(index + 1));
    if let [Some(first), Some(second)] = object_endpoints {
        if first.id != second.id && first.coordinates_m != second.coordinates_m {
            return Some([first, second]);
        }
    }
    let indices = encoded.map(|index| usize::try_from(index).ok()?.checked_sub(1));
    let [Some(first_index), Some(second_index)] = indices else {
        return None;
    };
    let mut points = markers
        .iter()
        .copied()
        .filter(|marker| {
            marker.feature_ref == curve.feature_ref
                && marker.coordinates_m.is_some()
                && matches!(
                    marker.kind,
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
        })
        .collect::<Vec<_>>();
    points.sort_unstable_by_key(|marker| marker.offset);
    let endpoints = [*points.get(first_index)?, *points.get(second_index)?];
    (endpoints[0].id != endpoints[1].id && endpoints[0].coordinates_m != endpoints[1].coordinates_m)
        .then_some(endpoints)
}

pub(super) fn legacy_marker104_arc_endpoints<'a>(
    payload: &[u8],
    curve: &SketchInputEntity,
    markers: &[&'a SketchInputEntity],
) -> Option<[&'a SketchInputEntity; 2]> {
    let offset = usize::try_from(curve.offset).ok()?;
    if curve.kind != SketchInputKind::Arc
        || payload.get(offset..offset + LEGACY_SKETCH_MARKER.len()) != Some(LEGACY_SKETCH_MARKER)
        || marker_native_code(payload, offset) != Some(2)
        || payload.get(offset + 23..offset + 27) != Some(&[0x05, 0x00, 0x01, 0x00])
        || marker_profile_curve_role(payload, offset) != Some(1)
        || compact_indexed_curve_record_end(payload, offset)
            != Some(CompactIndexedCurveRecordEnd::Marker104)
    {
        return None;
    }
    let endpoint_id = |relative| {
        let id = View::u16_le_at(payload, offset + relative)?;
        (!matches!(id, 0 | u16::MAX)).then_some(u32::from(id))
    };
    let endpoint_ids = [endpoint_id(56)?, endpoint_id(58)?];
    if endpoint_ids[0] == endpoint_ids[1] {
        return None;
    }
    let resolve = |id| {
        let mut candidates = markers.iter().copied().filter(|marker| {
            marker.feature_ref == curve.feature_ref
                && marker.object_index == Some(id)
                && marker.coordinates_m.is_some()
                && matches!(
                    marker.kind,
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
        });
        let candidate = candidates.next()?;
        candidates.next().is_none().then_some(candidate)
    };
    let endpoints = [resolve(endpoint_ids[0])?, resolve(endpoint_ids[1])?];
    (endpoints[0].coordinates_m != endpoints[1].coordinates_m).then_some(endpoints)
}

pub(super) fn one_based_point_roster_line_endpoint_markers<'a>(
    payload: &[u8],
    curve: &SketchInputEntity,
    markers: &[&'a SketchInputEntity],
) -> Option<[&'a SketchInputEntity; 2]> {
    let offset = usize::try_from(curve.offset).ok()?;
    if payload.get(offset..offset + SKETCH_MARKER.len()) != Some(SKETCH_MARKER)
        || payload.get(offset + 5..offset + 13) != Some(&[0xff; 8])
        || payload.get(offset + 13..offset + 17) != Some(&[0x00, 0x00, 0x80, 0xbf])
        || marker_native_code(payload, offset) != Some(1)
        || payload.get(offset + 23..offset + 27) != Some(&[0x04, 0x00, 0x02, 0x00])
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 29..offset + 31) != Some(&1u16.to_le_bytes())
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        || payload.get(offset + 40..offset + 48) != Some(&[0; 8])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 60..offset + 64) != Some(&1u32.to_le_bytes())
        || payload.get(offset + 64..offset + 72) != Some(&(-1.0f64).to_le_bytes())
        || !sketch_marker_prefix_at(payload, offset.checked_add(84)?)
    {
        return None;
    }
    let endpoint_index =
        |relative| usize::from(View::u16_le_at(payload, offset + relative)?).checked_sub(1);
    let indices = [endpoint_index(56)?, endpoint_index(58)?];
    if indices[0] == indices[1] {
        return None;
    }
    if markers.iter().any(|marker| {
        marker.feature_ref == curve.feature_ref && marker.kind == SketchInputKind::Arc
    }) {
        return None;
    }
    let mut points = markers
        .iter()
        .copied()
        .filter(|marker| {
            marker.feature_ref == curve.feature_ref
                && marker.coordinates_m.is_some()
                && matches!(
                    marker.kind,
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
        })
        .collect::<Vec<_>>();
    points.sort_unstable_by_key(|marker| marker.offset);
    let endpoints = [*points.get(indices[0])?, *points.get(indices[1])?];
    (endpoints[0].id != endpoints[1].id).then_some(endpoints)
}

pub(super) fn legacy_point_roster_line_endpoint_markers<'a>(
    payload: &[u8],
    curve: &SketchInputEntity,
    markers: &[&'a SketchInputEntity],
) -> Option<[&'a SketchInputEntity; 2]> {
    let offset = usize::try_from(curve.offset).ok()?;
    if curve.kind != SketchInputKind::LineOrCircle
        || payload.get(offset..offset + LEGACY_SKETCH_MARKER.len()) != Some(LEGACY_SKETCH_MARKER)
        || payload.get(offset + 5..offset + 13) != Some(&[0xff; 8])
        || payload.get(offset + 13..offset + 17) != Some(&[0x00, 0x00, 0x80, 0xbf])
        || marker_native_code(payload, offset) != Some(0)
        || !marker_is_geometry_locus(payload, offset)
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 29..offset + 31) != Some(&1u16.to_le_bytes())
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x05, 0x00])
        || payload.get(offset + 39..offset + 48) != Some(&[0; 9])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 60..offset + 64) != Some(&1u32.to_le_bytes())
        || payload.get(offset + 64..offset + 72) != Some(&(-1.0f64).to_le_bytes())
        || payload.get(offset + 72..offset + 76) != Some(&[0; 4])
        || payload
            .get(offset + 76..offset + 80)
            .is_none_or(|identity| identity == [0; 4] || identity == [0xff; 4])
        || payload
            .get(offset + 80..offset + 84)
            .is_none_or(|identity| identity == [0; 4] || identity == [0xff; 4])
        || !sketch_marker_prefix_at(payload, offset.checked_add(84)?)
    {
        return None;
    }
    let index = |relative| Some(usize::from(View::u16_le_at(payload, offset + relative)?));
    let indices = [index(56)?, index(58)?];
    if indices[0] == indices[1] || indices.contains(&usize::from(u16::MAX)) {
        return None;
    }
    let mut points = markers
        .iter()
        .copied()
        .filter(|marker| {
            marker.feature_ref == curve.feature_ref
                && marker.coordinates_m.is_some()
                && matches!(
                    marker.kind,
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
        })
        .collect::<Vec<_>>();
    points.sort_unstable_by_key(|marker| marker.offset);
    let endpoints = [*points.get(indices[0])?, *points.get(indices[1])?];
    (endpoints[0].id != endpoints[1].id && endpoints[0].coordinates_m != endpoints[1].coordinates_m)
        .then_some(endpoints)
}

pub(super) fn legacy_terminal_profile_indexed_endpoints<'a>(
    payload: &[u8],
    curve: &SketchInputEntity,
    markers: &[&'a SketchInputEntity],
) -> Option<[&'a SketchInputEntity; 2]> {
    let offset = usize::try_from(curve.offset).ok()?;
    let endpoint_offset = legacy_terminal_profile_endpoint_offset(payload, offset)?;
    let endpoint = |relative| Some(u32::from(View::u16_le_at(payload, offset + relative)?));
    let endpoint_ids = [endpoint(endpoint_offset)?, endpoint(endpoint_offset + 2)?];
    if endpoint_ids[0].checked_add(1) != Some(endpoint_ids[1]) {
        return None;
    }
    let resolve = |id| {
        let mut candidates = markers.iter().copied().filter(|marker| {
            marker.feature_ref == curve.feature_ref
                && matches!(
                    marker.kind,
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
                && marker.coordinates_m.is_some()
                && (marker.local_id == Some(id)
                    || marker.object_index.and_then(|index| index.checked_add(1)) == Some(id))
        });
        let candidate = candidates.next()?;
        candidates.next().is_none().then_some(candidate)
    };
    let resolved = endpoint_ids.map(resolve);
    let [Some(first), Some(second)] = resolved else {
        return None;
    };
    (first.id != second.id).then_some([first, second])
}

fn inline_arc_endpoint_markers<'a>(
    payload: &[u8],
    arc: &SketchInputEntity,
    markers: &[&'a SketchInputEntity],
) -> Option<[&'a SketchInputEntity; 2]> {
    let offset = usize::try_from(arc.offset).ok()?;
    let [_, start, end] = inline_arc_coordinates(payload, offset)?;
    let endpoint = |coordinates: [f64; 2]| {
        let mut candidates = markers.iter().copied().filter(|marker| {
            marker.feature_ref == arc.feature_ref
                && matches!(
                    marker.kind,
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
                && marker.coordinates_m.is_some_and(|point| {
                    same_dimension_length(point[0], coordinates[0])
                        && same_dimension_length(point[1], coordinates[1])
                })
        });
        let candidate = candidates.next()?;
        candidates.next().is_none().then_some(candidate)
    };
    let endpoints = [endpoint(start)?, endpoint(end)?];
    (endpoints[0].id != endpoints[1].id).then_some(endpoints)
}

fn compact_legacy_142_profile_curve_endpoint_markers<'a>(
    payload: &[u8],
    curve: &SketchInputEntity,
    markers: &[&'a SketchInputEntity],
) -> Option<[&'a SketchInputEntity; 2]> {
    if curve.kind != SketchInputKind::LineOrCircle || curve.coordinates_m.is_some() {
        return None;
    }
    let offset = usize::try_from(curve.offset).ok()?;
    let [start, end] = compact_legacy_142_profile_curve_endpoints(payload, offset)?;
    let resolve = |coordinates: [f64; 2]| {
        let mut candidates = markers.iter().copied().filter(|marker| {
            marker.feature_ref == curve.feature_ref
                && matches!(
                    marker.kind,
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
                && marker.coordinates_m.is_some_and(|point| {
                    same_dimension_length(point[0], coordinates[0])
                        && same_dimension_length(point[1], coordinates[1])
                })
        });
        let candidate = candidates.next()?;
        candidates.next().is_none().then_some(candidate)
    };
    let endpoints = [resolve(start)?, resolve(end)?];
    (endpoints[0].id != endpoints[1].id).then_some(endpoints)
}

pub(super) fn current_undetailed_bounded_curve_is_line(payload: &[u8], offset: usize) -> bool {
    let supported_prefix = matches!(
        payload.get(offset..offset + SKETCH_MARKER.len()),
        Some(marker) if marker == SKETCH_MARKER || marker == LEGACY_EXTENDED_SKETCH_MARKER
    );
    let profile_locus = payload.get(offset + 23..offset + 27) == Some(&[0x04, 0x00, 0x02, 0x00]);
    let extended_geometry_locus = payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len())
        == Some(LEGACY_EXTENDED_SKETCH_MARKER)
        && marker_is_geometry_locus(payload, offset);
    let distinct = |endpoints: [u32; 2]| endpoints[0] != endpoints[1];
    let compact_indexed_record = compact_indexed_curve_endpoint_indices(payload, offset)
        .is_some_and(distinct)
        || extended_compact_indexed_curve_endpoint_indices(payload, offset).is_some_and(distinct);
    let complete_indexed_record = wide_indexed_curve_endpoint_indices(payload, offset).is_some()
        && wide_indexed_curve_record_is_complete(payload, offset)
        || compact_indexed_record
            && matches!(
                compact_indexed_curve_record_end(payload, offset),
                Some(
                    CompactIndexedCurveRecordEnd::Marker84
                        | CompactIndexedCurveRecordEnd::Marker96
                        | CompactIndexedCurveRecordEnd::Marker104
                )
            );
    supported_prefix
        && (profile_locus || extended_geometry_locus)
        && complete_indexed_record
        && compact_bounded_curve_tangent(payload, offset).is_none()
}

pub(super) fn current_coordinate_linked_line_endpoints<'a>(
    payload: &[u8],
    line: &'a SketchInputEntity,
    markers: &[&'a SketchInputEntity],
) -> Option<[&'a SketchInputEntity; 2]> {
    let offset = usize::try_from(line.offset).ok()?;
    let cell = payload.get(offset + 86..offset + 98)?;
    let kind = operand_kind(cell[..2].try_into().ok()?)?;
    if payload.get(offset..offset + SKETCH_MARKER.len()) != Some(SKETCH_MARKER)
        || payload.get(offset + 5..offset + 13) != Some(&[0xff; 8])
        || payload.get(offset + 13..offset + 17) != Some(&[0x00, 0x00, 0x80, 0xbf])
        || marker_native_code(payload, offset) != Some(1)
        || !marker_is_geometry_locus(payload, offset)
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 29..offset + 31) != Some(&0u16.to_le_bytes())
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 64..offset + 66) != Some(&[0x1e, 0x00])
        || payload.get(offset + 82..offset + 86) != Some(&[0x00, 0x00, 0x01, 0x00])
        || !operand_accepts_marker(kind, SketchInputKind::LineOrCircle)
        || !operand_accepts_marker(kind, SketchInputKind::Arc)
        || cell[4..8] != [0xff; 4]
        || cell[8..12] != [0; 4]
        || payload.get(offset + 98..offset + 102) != Some(&[0; 4])
        || payload.get(offset + 102..offset + 106) != Some(&(-2i32).to_le_bytes())
        || payload.get(offset + 106..offset + 148) != Some(&[0; 42])
        || !sketch_marker_prefix_at(payload, offset.checked_add(152)?)
    {
        return None;
    }
    let local_id = u32::from(View::u16_le_at(cell, 2)?);
    // A local-link endpoint can select a coordinate-bearing curve marker before
    // the binding pass promotes it to a profile vertex. Keep that candidate in
    // the graph; the binding pass retains it as a curve only when it resolves
    // its own two endpoints.
    let mut endpoints = markers.iter().copied().filter(|marker| {
        marker.feature_ref == line.feature_ref
            && marker.id != line.id
            && marker.local_id == Some(local_id)
            && marker.coordinates_m.is_some()
            && matches!(
                marker.kind,
                SketchInputKind::Point
                    | SketchInputKind::ConstrainedPoint
                    | SketchInputKind::LineOrCircle
                    | SketchInputKind::Arc
            )
    });
    let endpoint = endpoints.next()?;
    endpoints.next().is_none().then_some([line, endpoint])
}

pub(super) fn coordinate_centered_line_endpoints<'a>(
    payload: &[u8],
    line: &SketchInputEntity,
    markers: &[&'a SketchInputEntity],
) -> Option<[&'a SketchInputEntity; 2]> {
    let offset = usize::try_from(line.offset).ok()?;
    let [center_u, center_v] = coordinate_centered_line_center(payload, offset)?;
    let mut coordinates = markers
        .iter()
        .copied()
        .filter(|marker| {
            marker.feature_ref == line.feature_ref
                && marker.offset > line.offset
                && marker.coordinates_m.is_some()
        })
        .collect::<Vec<_>>();
    coordinates.sort_unstable_by_key(|marker| marker.offset);
    let [first, second, ..] = coordinates.as_slice() else {
        return None;
    };
    let [first_u, first_v] = first.coordinates_m?;
    let [second_u, second_v] = second.coordinates_m?;
    let centered = same_dimension_length((first_u + second_u) * 0.5, center_u)
        && same_dimension_length((first_v + second_v) * 0.5, center_v);
    (centered && (first_u != second_u || first_v != second_v)).then_some([first, second])
}

fn coordinate_centered_line_center(payload: &[u8], offset: usize) -> Option<[f64; 2]> {
    if payload.get(offset + 5..offset + 13) != Some(&[0xff; 8])
        || payload.get(offset + 13..offset + 17) != Some(&[0x00, 0x00, 0x80, 0xbf])
        || marker_native_code(payload, offset) != Some(2)
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 29..offset + 31) != Some(&0u16.to_le_bytes())
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
    {
        return None;
    }
    if payload.get(offset..offset + SKETCH_MARKER.len()) == Some(SKETCH_MARKER)
        && marker_is_geometry_locus(payload, offset)
        && payload.get(offset + 64..offset + 66) == Some(&[0x1e, 0x00])
        && payload.get(offset + 82..offset + 86) == Some(&1u32.to_le_bytes())
        && payload.get(offset + 86..offset + 92) == Some(&[0; 6])
        && payload.get(offset + 92..offset + 96) == Some(&(-2i32).to_le_bytes())
        && payload.get(offset + 96..offset + 138) == Some(&[0; 42])
        && sketch_marker_prefix_at(payload, offset.checked_add(142)?)
    {
        return finite_coordinate_pair(payload, offset + 66);
    }
    let direct = View::u16_le_at(payload, offset + 74)?;
    let count = View::u16_le_at(payload, offset + 76)?;
    let tagged = View::u16_le_at(payload, offset + 82)?;
    if payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len())
        == Some(LEGACY_EXTENDED_SKETCH_MARKER)
        && payload.get(offset + 23..offset + 27) == Some(&[0x04, 0x00, 0x02, 0x00])
        && payload.get(offset + 56..offset + 58) == Some(&[0x1e, 0x00])
        && ((direct == 1 && count == 0 && tagged == 0)
            || (direct == 0 && (1..=3).contains(&count) && tagged <= 1))
        && payload.get(offset + 78..offset + 82) == Some(&[0; 4])
        && payload.get(offset + 84..offset + 88) == Some(&(-2i32).to_le_bytes())
        && payload.get(offset + 88..offset + 130) == Some(&[0; 42])
        && sketch_marker_prefix_at(payload, offset.checked_add(134)?)
    {
        return finite_coordinate_pair(payload, offset + 58);
    }
    None
}

pub(super) fn consecutive_legacy_profile_line_endpoints<'a>(
    payload: &[u8],
    line: &'a SketchInputEntity,
    markers: &[&'a SketchInputEntity],
) -> Vec<&'a SketchInputEntity> {
    let Some(offset) = usize::try_from(line.offset).ok() else {
        return Vec::new();
    };
    if line.kind != SketchInputKind::LineOrCircle
        || line.coordinates_m.is_none()
        || payload.get(offset..offset + LEGACY_SKETCH_MARKER.len()) != Some(LEGACY_SKETCH_MARKER)
        || marker_native_code(payload, offset) != Some(1)
        || payload.get(offset + 23..offset + 27) != Some(&[0x04, 0x00, 0x02, 0x00])
        || marker_profile_curve_role(payload, offset) != Some(1)
    {
        return Vec::new();
    }
    let Some(next) = markers
        .iter()
        .copied()
        .filter(|marker| marker.feature_ref == line.feature_ref && marker.offset > line.offset)
        .min_by_key(|marker| marker.offset)
    else {
        return Vec::new();
    };
    if next.coordinates_m.is_none()
        || !matches!(
            next.kind,
            SketchInputKind::Point
                | SketchInputKind::ConstrainedPoint
                | SketchInputKind::LineOrCircle
                | SketchInputKind::Arc
        )
        || usize::try_from(next.offset)
            .ok()
            .is_none_or(|next_offset| !sketch_marker_prefix_at(payload, next_offset))
    {
        return Vec::new();
    }
    vec![line, next]
}

pub(super) fn legacy_terminal_indexed_profile_line(
    payload: &[u8],
    curve: &SketchInputEntity,
    markers: &[&SketchInputEntity],
) -> bool {
    let Some(offset) = usize::try_from(curve.offset).ok() else {
        return false;
    };
    if payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len())
        != Some(LEGACY_EXTENDED_SKETCH_MARKER)
        || marker_native_code(payload, offset) != Some(0)
        || !(marker_is_geometry_locus(payload, offset)
            || payload.get(offset + 23..offset + 27) == Some(&[0x04, 0x00, 0x02, 0x00]))
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 60..offset + 64) != Some(&1u32.to_le_bytes())
        || payload.get(offset + 64..offset + 72) != Some(&(-1.0f64).to_le_bytes())
        || sketch_marker_prefix_at(payload, offset.saturating_add(84))
    {
        return false;
    }
    markers.iter().copied().any(|sibling| {
        let Some(sibling_offset) = usize::try_from(sibling.offset).ok() else {
            return false;
        };
        sibling.feature_ref == curve.feature_ref
            && sibling.offset < curve.offset
            && sibling.kind == SketchInputKind::LineOrCircle
            && marker_native_code(payload, sibling_offset) == Some(0)
            && legacy_extended_profile_curve_kind(payload, sibling_offset)
                == Some(SketchInputKind::LineOrCircle)
    })
}

#[cfg(test)]
mod typed_relations_tests;
