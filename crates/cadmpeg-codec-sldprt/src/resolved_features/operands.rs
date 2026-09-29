//! Scalar operand marker resolution over the link graph.

use super::selections::{
    operand_accepts_marker, operand_allows_compatible_ordinal_fallback,
    operand_uses_compatible_ordinal,
};
use crate::records::operand_tag::NativeOperandTag;
use crate::records::{
    FeatureInputOperand, FeatureInputOperandKind, SketchInputEntity, SketchInputKind,
};
use std::collections::{HashMap, HashSet};

pub(crate) fn resolve_scalar_operand_markers<'a>(
    entities: impl IntoIterator<Item = &'a SketchInputEntity>,
    operands: &[FeatureInputOperand],
) -> Vec<Option<&'a SketchInputEntity>> {
    let entities = entities.into_iter().collect::<Vec<_>>();
    let mut resolved = operands
        .iter()
        .map(|operand| {
            resolve_operand_marker_excluding(&entities, operand.kind, operand.entity_index, |_| false)
        })
        .collect::<Vec<_>>();
    if let ([first_operand, second_operand], [Some(first), Some(second)]) =
        (operands, resolved.as_slice())
    {
        if first.id() == second.id() && first_operand.entity_index != second_operand.entity_index {
            let alternatives = [
                resolve_operand_marker_excluding(
                    &entities,
                    first_operand.kind,
                    first_operand.entity_index,
                    |id| id == second.id(),
                )
                .map(|alternative| [alternative, *second]),
                resolve_operand_marker_excluding(
                    &entities,
                    second_operand.kind,
                    second_operand.entity_index,
                    |id| id == first.id(),
                )
                .map(|alternative| [*first, alternative]),
            ]
            .into_iter()
            .flatten()
            .filter(|[left, right]| left.id() != right.id())
            .collect::<Vec<_>>();
            if let [alternative] = alternatives.as_slice() {
                resolved = alternative.iter().copied().map(Some).collect();
            }
        }
    }
    let resolved_siblings = resolved
        .iter()
        .flatten()
        .map(|entity| entity.id())
        .collect::<HashSet<_>>();
    for (operand, target) in operands.iter().zip(&mut resolved) {
        if target.is_none() {
            *target = resolve_operand_marker_excluding(
                &entities,
                operand.kind,
                operand.entity_index,
                |id| resolved_siblings.contains(id),
            );
        }
    }
    resolved
}

#[cfg(test)]
fn resolve_operand_marker<'a>(
    entities: impl IntoIterator<Item = &'a SketchInputEntity>,
    kind: FeatureInputOperandKind,
    address: u16,
) -> Option<&'a SketchInputEntity> {
    let entities = entities.into_iter().collect::<Vec<_>>();
    resolve_operand_marker_excluding(&entities, kind, address, |_| false)
}

fn unique_entity<'a>(mut candidates: impl Iterator<Item = &'a SketchInputEntity>) -> Option<&'a SketchInputEntity> {
    let first = candidates.next()?;
    candidates.next().is_none().then_some(first)
}

fn resolve_operand_marker_excluding<'a>(
    entities: &[&'a SketchInputEntity],
    kind: FeatureInputOperandKind,
    address: u16,
    excluded: impl Fn(&str) -> bool,
) -> Option<&'a SketchInputEntity> {
    let excluded = &excluded;
    if kind == FeatureInputOperandKind::Native(NativeOperandTag::TAG_81DD) {
        let mut points = entities
            .iter()
            .copied()
            .filter(|entity| {
                matches!(
                    entity.kind(),
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                ) && entity
                    .coordinates_m
                    .is_some_and(|coordinates| coordinates.into_iter().all(f64::is_finite))
            })
            .collect::<Vec<_>>();
        points.sort_unstable_by_key(|entity| entity.offset());
        return points
            .get(usize::from(address))
            .copied()
            .filter(|entity| !excluded(entity.id()));
    }
    if kind == FeatureInputOperandKind::Native(NativeOperandTag::TAG_81E7) {
        // In scalar relations, 81e7 addresses the solver-line roster formed
        // from coordinate points; it does not directly resolve a line marker.
        return None;
    }
    if kind == FeatureInputOperandKind::Native(NativeOperandTag::TAG_810F) {
        // An 810f cell belongs to the declared line-distance family. Its
        // address is an object index when that index is present, or a local
        // identifier otherwise. A coordinate-bearing point can share either
        // address with a line handle, but it is not a line operand. Keep only
        // line/arc markers, relation handles, and coordinate-less point
        // proxies; reject every ambiguous candidate set.
        let accepts = |entity: &SketchInputEntity| {
            matches!(
                entity.kind(),
                SketchInputKind::LineOrCircle | SketchInputKind::Arc | SketchInputKind::Relation(_)
            ) || (matches!(
                entity.kind(),
                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
            ) && entity.coordinates_m.is_none())
        };
        let indexed = entities
            .iter()
            .copied()
            .filter(|entity| entity.object_index() == Some(u32::from(address)))
            .filter(|entity| accepts(entity))
            .filter(|entity| !excluded(entity.id()));
        if entities
            .iter()
            .any(|entity| entity.object_index() == Some(u32::from(address)) && accepts(entity))
        {
            return unique_entity(indexed);
        }
        let local = entities
            .iter()
            .copied()
            .filter(|entity| entity.local_id() == Some(u32::from(address)))
            .filter(|entity| accepts(entity))
            .filter(|entity| !excluded(entity.id()));
        return unique_entity(local);
    }
    if kind == FeatureInputOperandKind::Native(NativeOperandTag::TAG_BC7C) {
        let indexed = entities
            .iter()
            .copied()
            .filter(|entity| entity.object_index() == Some(u32::from(address)))
            .filter(|entity| entity.coordinates_m.is_some())
            .filter(|entity| {
                matches!(
                    entity.kind(),
                    SketchInputKind::Point
                        | SketchInputKind::ConstrainedPoint
                        | SketchInputKind::LineOrCircle
                        | SketchInputKind::Arc
                )
            })
            .filter(|entity| !excluded(entity.id()));
        if let Some(entity) = unique_entity(indexed) {
            return Some(entity);
        }
    }
    if kind == FeatureInputOperandKind::Native(NativeOperandTag::TAG_BC87) {
        let indexed = entities
            .iter()
            .copied()
            .filter(|entity| entity.object_index() == Some(u32::from(address)))
            .filter(|entity| entity.coordinates_m.is_some())
            .filter(|entity| {
                matches!(
                    entity.kind(),
                    SketchInputKind::LineOrCircle | SketchInputKind::Arc
                )
            })
            .filter(|entity| !excluded(entity.id()));
        if let Some(entity) = unique_entity(indexed) {
            return Some(entity);
        }
    }
    if kind == FeatureInputOperandKind::Native(NativeOperandTag::TAG_814C) {
        let indexed = entities
            .iter()
            .copied()
            .filter(|entity| entity.object_index() == Some(u32::from(address)))
            .filter(|entity| entity.coordinates_m.is_some())
            .filter(|entity| {
                matches!(
                    entity.kind(),
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
            })
            .filter(|entity| !excluded(entity.id()));
        return unique_entity(indexed);
    }
    if matches!(
        kind,
        FeatureInputOperandKind::Native(
            NativeOperandTag::TAG_80CC
                | NativeOperandTag::TAG_8152
                | NativeOperandTag::TAG_8AB6
                | NativeOperandTag::TAG_8DCB
                | NativeOperandTag::TAG_929D
                | NativeOperandTag::TAG_BD69
        )
    ) {
        let indexed = entities
            .iter()
            .copied()
            .filter(|entity| entity.object_index() == Some(u32::from(address)))
            .filter(|entity| entity.coordinates_m.is_some())
            .filter(|entity| operand_accepts_marker(kind, entity.kind()))
            .filter(|entity| !excluded(entity.id()));
        if let Some(entity) = unique_entity(indexed) {
            return Some(entity);
        }
    }
    if matches!(
        kind,
        FeatureInputOperandKind::Native(
            NativeOperandTag::TAG_80AC | NativeOperandTag::TAG_80D5 | NativeOperandTag::TAG_8138
        )
    ) {
        // These class-scoped relation cells use the same address precedence
        // as point references, but may also name a relation handle whose
        // links resolve to a point locus. A line or arc sharing the address
        // is not a candidate for this operand family.
        let indexed = entities
            .iter()
            .copied()
            .filter(|entity| entity.object_index() == Some(u32::from(address)))
            .filter(|entity| operand_accepts_marker(kind, entity.kind()))
            .filter(|entity| !excluded(entity.id()));
        if entities.iter().any(|entity| {
            entity.object_index() == Some(u32::from(address))
                && operand_accepts_marker(kind, entity.kind())
        }) {
            return unique_entity(indexed);
        }
        let local = entities
            .iter()
            .copied()
            .filter(|entity| entity.local_id() == Some(u32::from(address)))
            .filter(|entity| operand_accepts_marker(kind, entity.kind()))
            .filter(|entity| !excluded(entity.id()));
        if let Some(entity) = unique_entity(local) {
            return Some(entity);
        }
    }
    if matches!(
        kind,
        FeatureInputOperandKind::E1 | FeatureInputOperandKind::Native(NativeOperandTag::TAG_8386)
    ) {
        let indexed = entities
            .iter()
            .copied()
            .filter(|entity| entity.object_index() == Some(u32::from(address)))
            .filter(|entity| operand_accepts_marker(kind, entity.kind()))
            .filter(|entity| !excluded(entity.id()));
        if let Some(entity) = unique_entity(indexed) {
            return Some(entity);
        }
        if kind == FeatureInputOperandKind::Native(NativeOperandTag::TAG_8386) {
            let entities_by_id = entities
                .iter()
                .map(|entity| (entity.id(), *entity))
                .collect::<HashMap<_, _>>();
            let indexed_line_handles = entities
                .iter()
                .copied()
                .filter(|entity| entity.object_index() == Some(u32::from(address)))
                .filter(|entity| !excluded(entity.id()))
                .filter(|entity| {
                    linked_coordinate_line_endpoints(entity, &entities_by_id).is_some()
                });
            if let Some(entity) = unique_entity(indexed_line_handles) {
                return Some(entity);
            }
        }
    }
    let mut compatible = entities
        .iter()
        .copied()
        .filter(|entity| operand_accepts_marker(kind, entity.kind()))
        .collect::<Vec<_>>();
    compatible.sort_unstable_by_key(|entity| entity.offset());
    let mut ordinal_link_graph = false;
    if operand_uses_compatible_ordinal(kind) {
        if let Some(entity) = compatible
            .get(usize::from(address))
            .filter(|entity| !excluded(entity.id()))
        {
            return Some(*entity);
        }
        if !point_operand_uses_link_graph(kind) {
            return None;
        }
        ordinal_link_graph = true;
    }
    let exact_candidates = || {
        compatible.iter().copied().filter(|entity| {
            !ordinal_link_graph
                && entity.local_id() == Some(u32::from(address))
                && !excluded(entity.id())
        })
    };
    let mut exact = exact_candidates();
    match (exact.next(), exact.next()) {
        (Some(entity), None) => Some(entity),
        (None, _) => {
            if kind == FeatureInputOperandKind::Native(NativeOperandTag::TAG_8386) {
                let entities_by_id = entities
                    .iter()
                    .map(|entity| (entity.id(), *entity))
                    .collect::<HashMap<_, _>>();
                let linked_line_handles = entities
                    .iter()
                    .copied()
                    .filter(|entity| entity.local_id() == Some(u32::from(address)))
                    .filter(|entity| !excluded(entity.id()))
                    .filter(|entity| {
                        linked_coordinate_line_endpoints(entity, &entities_by_id).is_some()
                    });
                if let Some(entity) = unique_entity(linked_line_handles) {
                    return Some(entity);
                }
            }
            let mut indirect = if point_operand_uses_link_graph(kind) {
                linked_point_markers(&entities, address, kind, excluded)
            } else if operand_accepts_link_indirection(kind) {
                entities
                    .iter()
                    .copied()
                    .filter(|entity| entity.local_id() == Some(u32::from(address)))
                    .flat_map(crate::records::SketchInputEntity::links)
                    .filter_map(|link| {
                        entities
                            .iter()
                            .copied()
                            .find(|entity| entity.id() == link.entity_ref)
                    })
                    .filter(|entity| operand_accepts_marker(kind, entity.kind()))
                    .filter(|entity| !excluded(entity.id()))
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            indirect.sort_unstable_by_key(|entity| entity.id());
            indirect.dedup_by_key(|entity| entity.id());
            match indirect.as_slice() {
                [entity] => Some(*entity),
                [] if point_operand_uses_link_graph(kind) && {
                    let linked = linked_point_markers(&entities, address, kind, |_| false);
                    !linked.is_empty() && linked.iter().all(|entity| excluded(entity.id()))
                } =>
                {
                    let remaining = compatible
                        .iter()
                        .copied()
                        .filter(|entity| !excluded(entity.id()));
                    unique_entity(remaining)
                }
                [] if operand_allows_compatible_ordinal_fallback(kind) => {
                    compatible.get(usize::from(address)).copied().or_else(|| {
                        (kind == FeatureInputOperandKind::Native(NativeOperandTag::TAG_BC7C))
                            .then(|| {
                                unique_entity(entities.iter().copied().filter(|entity| {
                                    matches!(
                                        entity.kind(),
                                        SketchInputKind::LineOrCircle | SketchInputKind::Arc
                                    ) && entity.local_id() == Some(u32::from(address))
                                        && !excluded(entity.id())
                                }))
                            })
                            .flatten()
                    })
                }
                _ => None,
            }
        }
        (Some(_), Some(_)) => {
            unique_entity(exact_candidates().filter(|entity| entity.coordinates_m.is_some()))
        }
    }
}

fn point_operand_uses_link_graph(kind: FeatureInputOperandKind) -> bool {
    matches!(kind, FeatureInputOperandKind::D6)
}

fn linked_point_markers<'a>(
    entities: &[&'a SketchInputEntity],
    address: u16,
    kind: FeatureInputOperandKind,
    excluded: impl Fn(&str) -> bool,
) -> Vec<&'a SketchInputEntity> {
    let by_id = entities
        .iter()
        .map(|entity| (entity.id(), *entity))
        .collect::<HashMap<_, _>>();
    let mut pending = entities
        .iter()
        .copied()
        .filter(|entity| entity.local_id() == Some(u32::from(address)))
        .filter(|entity| !operand_accepts_marker(kind, entity.kind()))
        .map(super::super::records::SketchInputEntity::id)
        .collect::<Vec<_>>();
    let mut visited = HashSet::new();
    let mut compatible = Vec::new();
    while let Some(id) = pending.pop() {
        if !visited.insert(id) {
            continue;
        }
        let Some(entity) = by_id.get(id).copied() else {
            continue;
        };
        if operand_accepts_marker(kind, entity.kind()) && !excluded(entity.id()) {
            compatible.push(entity);
            continue;
        }
        pending.extend(entity.links().iter().map(|link| link.entity_ref.as_str()));
    }
    compatible
}

fn operand_accepts_link_indirection(kind: FeatureInputOperandKind) -> bool {
    matches!(
        kind,
        FeatureInputOperandKind::E1
            | FeatureInputOperandKind::Native(
                NativeOperandTag::TAG_8386
                    | NativeOperandTag::TAG_83FE
                    | NativeOperandTag::TAG_8DDA
                    | NativeOperandTag::TAG_BC87
            )
    )
}

pub(super) fn linked_coordinate_line_endpoints<'a>(
    marker: &SketchInputEntity,
    markers_by_id: &HashMap<&str, &'a SketchInputEntity>,
) -> Option<[&'a SketchInputEntity; 2]> {
    let links = marker
        .links()
        .iter()
        .filter(|link| link.entity_ref != marker.id())
        .collect::<Vec<_>>();
    let [first, second] = links.as_slice() else {
        return None;
    };
    let endpoints = [first, second].map(|link| {
        markers_by_id
            .get(link.entity_ref.as_str())
            .copied()
            .filter(|endpoint| {
                endpoint.feature_ref == marker.feature_ref && endpoint.coordinates_m.is_some()
            })
    });
    let [Some(first), Some(second)] = endpoints else {
        return None;
    };
    if matches!(marker.kind(), SketchInputKind::Relation(_))
        && ![first, second].into_iter().all(|endpoint| {
            matches!(
                endpoint.kind(),
                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
            )
        })
    {
        return None;
    }
    (first.id() != second.id()).then_some([first, second])
}

pub(super) fn coordinate_line_endpoints_with_linked_point<'a>(
    marker: &'a SketchInputEntity,
    markers_by_id: &HashMap<&str, &'a SketchInputEntity>,
) -> Option<[&'a SketchInputEntity; 2]> {
    if !matches!(
        marker.kind(),
        SketchInputKind::LineOrCircle | SketchInputKind::Arc
    ) || marker.coordinates_m.is_none()
    {
        return None;
    }
    let mut endpoints = marker
        .links()
        .iter()
        .filter(|link| link.entity_ref != marker.id())
        .filter_map(|link| markers_by_id.get(link.entity_ref.as_str()).copied())
        .filter(|endpoint| {
            endpoint.feature_ref == marker.feature_ref
                && endpoint.coordinates_m.is_some()
                && matches!(
                    endpoint.kind(),
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
        })
        .collect::<Vec<_>>();
    endpoints.sort_unstable_by_key(|endpoint| endpoint.offset());
    endpoints.dedup_by_key(|endpoint| endpoint.id());
    let [endpoint] = endpoints.as_slice() else {
        return None;
    };
    Some([marker, *endpoint])
}

#[cfg(test)]
mod operands_tests;
