//! Scalar operand marker resolution over the link graph.

use super::selections::{
    operand_accepts_marker, operand_allows_compatible_ordinal_fallback,
    operand_uses_compatible_ordinal, unique_marker_candidate,
};
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
            resolve_operand_marker(entities.iter().copied(), operand.kind, operand.entity_index)
        })
        .collect::<Vec<_>>();
    if let ([first_operand, second_operand], [Some(first), Some(second)]) =
        (operands, resolved.as_slice())
    {
        if first.id == second.id && first_operand.entity_index != second_operand.entity_index {
            let alternatives = [
                resolve_operand_marker_excluding(
                    entities.iter().copied(),
                    first_operand.kind,
                    first_operand.entity_index,
                    &HashSet::from([second.id.clone()]),
                )
                .map(|alternative| [alternative, *second]),
                resolve_operand_marker_excluding(
                    entities.iter().copied(),
                    second_operand.kind,
                    second_operand.entity_index,
                    &HashSet::from([first.id.clone()]),
                )
                .map(|alternative| [*first, alternative]),
            ]
            .into_iter()
            .flatten()
            .filter(|[left, right]| left.id != right.id)
            .collect::<Vec<_>>();
            if let [alternative] = alternatives.as_slice() {
                resolved = alternative.iter().copied().map(Some).collect();
            }
        }
    }
    let resolved_siblings = resolved
        .iter()
        .flatten()
        .map(|entity| entity.id.clone())
        .collect::<HashSet<_>>();
    for (operand, target) in operands.iter().zip(&mut resolved) {
        if target.is_none() {
            *target = resolve_operand_marker_excluding(
                entities.iter().copied(),
                operand.kind,
                operand.entity_index,
                &resolved_siblings,
            );
        }
    }
    resolved
}

pub(super) fn resolve_operand_marker<'a>(
    entities: impl IntoIterator<Item = &'a SketchInputEntity>,
    kind: FeatureInputOperandKind,
    address: u16,
) -> Option<&'a SketchInputEntity> {
    resolve_operand_marker_excluding(entities, kind, address, &HashSet::new())
}

pub(super) fn resolve_operand_marker_excluding<'a>(
    entities: impl IntoIterator<Item = &'a SketchInputEntity>,
    kind: FeatureInputOperandKind,
    address: u16,
    excluded: &HashSet<String>,
) -> Option<&'a SketchInputEntity> {
    let entities = entities.into_iter().collect::<Vec<_>>();
    if kind == FeatureInputOperandKind::Native(0x81dd) {
        let mut points = entities
            .iter()
            .copied()
            .filter(|entity| {
                matches!(
                    entity.kind,
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                ) && entity
                    .coordinates_m
                    .is_some_and(|coordinates| coordinates.into_iter().all(f64::is_finite))
            })
            .collect::<Vec<_>>();
        points.sort_unstable_by_key(|entity| entity.offset);
        return points
            .get(usize::from(address))
            .copied()
            .filter(|entity| !excluded.contains(&entity.id));
    }
    if kind == FeatureInputOperandKind::Native(0x81e7) {
        // In scalar relations, 81e7 addresses the solver-line roster formed
        // from coordinate points; it does not directly resolve a line marker.
        return None;
    }
    if kind == FeatureInputOperandKind::Native(0x810f) {
        // An 810f cell belongs to the declared line-distance family. Its
        // address is an object index when that index is present, or a local
        // identifier otherwise. A coordinate-bearing point can share either
        // address with a line handle, but it is not a line operand. Keep only
        // line/arc markers, relation handles, and coordinate-less point
        // proxies; reject every ambiguous candidate set.
        let accepts = |entity: &SketchInputEntity| {
            matches!(
                entity.kind,
                SketchInputKind::LineOrCircle | SketchInputKind::Arc | SketchInputKind::Relation(_)
            ) || (matches!(
                entity.kind,
                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
            ) && entity.coordinates_m.is_none())
        };
        let indexed = entities
            .iter()
            .filter(|entity| entity.object_index == Some(u32::from(address)))
            .filter(|entity| accepts(entity))
            .filter(|entity| !excluded.contains(&entity.id))
            .collect::<Vec<_>>();
        if entities
            .iter()
            .any(|entity| entity.object_index == Some(u32::from(address)) && accepts(entity))
        {
            return match indexed.as_slice() {
                [entity] => Some(*entity),
                _ => None,
            };
        }
        let local = entities
            .iter()
            .filter(|entity| entity.local_id == Some(u32::from(address)))
            .filter(|entity| accepts(entity))
            .filter(|entity| !excluded.contains(&entity.id))
            .collect::<Vec<_>>();
        return match local.as_slice() {
            [entity] => Some(*entity),
            _ => None,
        };
    }
    if kind == FeatureInputOperandKind::Native(0xbc7c) {
        let indexed = entities
            .iter()
            .copied()
            .filter(|entity| entity.object_index == Some(u32::from(address)))
            .filter(|entity| entity.coordinates_m.is_some())
            .filter(|entity| {
                matches!(
                    entity.kind,
                    SketchInputKind::Point
                        | SketchInputKind::ConstrainedPoint
                        | SketchInputKind::LineOrCircle
                        | SketchInputKind::Arc
                )
            })
            .filter(|entity| !excluded.contains(&entity.id))
            .collect::<Vec<_>>();
        if let [entity] = indexed.as_slice() {
            return Some(*entity);
        }
    }
    if kind == FeatureInputOperandKind::Native(0xbc87) {
        let indexed = entities
            .iter()
            .copied()
            .filter(|entity| entity.object_index == Some(u32::from(address)))
            .filter(|entity| entity.coordinates_m.is_some())
            .filter(|entity| {
                matches!(
                    entity.kind,
                    SketchInputKind::LineOrCircle | SketchInputKind::Arc
                )
            })
            .filter(|entity| !excluded.contains(&entity.id))
            .collect::<Vec<_>>();
        if let [entity] = indexed.as_slice() {
            return Some(*entity);
        }
    }
    if kind == FeatureInputOperandKind::Native(0x814c) {
        let indexed = entities
            .iter()
            .copied()
            .filter(|entity| entity.object_index == Some(u32::from(address)))
            .filter(|entity| entity.coordinates_m.is_some())
            .filter(|entity| {
                matches!(
                    entity.kind,
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
            })
            .filter(|entity| !excluded.contains(&entity.id))
            .collect::<Vec<_>>();
        return match indexed.as_slice() {
            [entity] => Some(*entity),
            _ => None,
        };
    }
    if matches!(
        kind,
        FeatureInputOperandKind::Native(0x80cc | 0x8152 | 0x8ab6 | 0x8dcb | 0x929d | 0xbd69,)
    ) {
        let indexed = entities
            .iter()
            .copied()
            .filter(|entity| entity.object_index == Some(u32::from(address)))
            .filter(|entity| entity.coordinates_m.is_some())
            .filter(|entity| operand_accepts_marker(kind, entity.kind))
            .filter(|entity| !excluded.contains(&entity.id))
            .collect::<Vec<_>>();
        if let [entity] = indexed.as_slice() {
            return Some(*entity);
        }
    }
    if matches!(
        kind,
        FeatureInputOperandKind::Native(0x80ac | 0x80d5 | 0x8138)
    ) {
        // These class-scoped relation cells use the same address precedence
        // as point references, but may also name a relation handle whose
        // links resolve to a point locus. A line or arc sharing the address
        // is not a candidate for this operand family.
        let indexed = entities
            .iter()
            .copied()
            .filter(|entity| entity.object_index == Some(u32::from(address)))
            .filter(|entity| operand_accepts_marker(kind, entity.kind))
            .filter(|entity| !excluded.contains(&entity.id))
            .collect::<Vec<_>>();
        if entities.iter().any(|entity| {
            entity.object_index == Some(u32::from(address))
                && operand_accepts_marker(kind, entity.kind)
        }) {
            return match indexed.as_slice() {
                [entity] => Some(*entity),
                _ => None,
            };
        }
        let local = entities
            .iter()
            .copied()
            .filter(|entity| entity.local_id == Some(u32::from(address)))
            .filter(|entity| operand_accepts_marker(kind, entity.kind))
            .filter(|entity| !excluded.contains(&entity.id))
            .collect::<Vec<_>>();
        if let [entity] = local.as_slice() {
            return Some(*entity);
        }
    }
    if matches!(
        kind,
        FeatureInputOperandKind::E1 | FeatureInputOperandKind::Native(0x8386)
    ) {
        let indexed = entities
            .iter()
            .copied()
            .filter(|entity| entity.object_index == Some(u32::from(address)))
            .filter(|entity| operand_accepts_marker(kind, entity.kind))
            .filter(|entity| !excluded.contains(&entity.id))
            .collect::<Vec<_>>();
        if let [entity] = indexed.as_slice() {
            return Some(*entity);
        }
        if kind == FeatureInputOperandKind::Native(0x8386) {
            let entities_by_id = entities
                .iter()
                .map(|entity| (entity.id.as_str(), *entity))
                .collect::<HashMap<_, _>>();
            let indexed_line_handles = entities
                .iter()
                .copied()
                .filter(|entity| entity.object_index == Some(u32::from(address)))
                .filter(|entity| !excluded.contains(&entity.id))
                .filter(|entity| {
                    linked_coordinate_line_endpoints(entity, &entities_by_id).is_some()
                })
                .collect::<Vec<_>>();
            if let [entity] = indexed_line_handles.as_slice() {
                return Some(*entity);
            }
        }
    }
    let mut compatible = entities
        .iter()
        .copied()
        .filter(|entity| operand_accepts_marker(kind, entity.kind))
        .collect::<Vec<_>>();
    compatible.sort_unstable_by_key(|entity| entity.offset);
    let mut ordinal_link_graph = false;
    if operand_uses_compatible_ordinal(kind) {
        if let Some(entity) = compatible
            .get(usize::from(address))
            .filter(|entity| !excluded.contains(&entity.id))
        {
            return Some(*entity);
        }
        if !point_operand_uses_link_graph(kind) {
            return None;
        }
        ordinal_link_graph = true;
    }
    let exact = if ordinal_link_graph {
        Vec::new()
    } else {
        compatible
            .iter()
            .copied()
            .filter(|entity| entity.local_id == Some(u32::from(address)))
            .filter(|entity| !excluded.contains(&entity.id))
            .collect::<Vec<_>>()
    };
    match exact.as_slice() {
        [entity] => Some(*entity),
        [] => {
            if kind == FeatureInputOperandKind::Native(0x8386) {
                let entities_by_id = entities
                    .iter()
                    .map(|entity| (entity.id.as_str(), *entity))
                    .collect::<HashMap<_, _>>();
                let linked_line_handles = entities
                    .iter()
                    .copied()
                    .filter(|entity| entity.local_id == Some(u32::from(address)))
                    .filter(|entity| !excluded.contains(&entity.id))
                    .filter(|entity| {
                        linked_coordinate_line_endpoints(entity, &entities_by_id).is_some()
                    })
                    .collect::<Vec<_>>();
                if let [entity] = linked_line_handles.as_slice() {
                    return Some(*entity);
                }
            }
            let mut indirect = if point_operand_uses_link_graph(kind) {
                linked_point_markers(&entities, address, kind, excluded)
            } else if operand_accepts_link_indirection(kind) {
                entities
                    .iter()
                    .copied()
                    .filter(|entity| entity.local_id == Some(u32::from(address)))
                    .flat_map(|entity| &entity.links)
                    .filter_map(|link| {
                        entities
                            .iter()
                            .copied()
                            .find(|entity| entity.id == link.entity_ref)
                    })
                    .filter(|entity| operand_accepts_marker(kind, entity.kind))
                    .filter(|entity| !excluded.contains(&entity.id))
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            indirect.sort_unstable_by_key(|entity| entity.id.as_str());
            indirect.dedup_by_key(|entity| entity.id.as_str());
            match indirect.as_slice() {
                [entity] => Some(*entity),
                [] if point_operand_uses_link_graph(kind) && !excluded.is_empty() && {
                    let linked = linked_point_markers(&entities, address, kind, &HashSet::new());
                    !linked.is_empty() && linked.iter().all(|entity| excluded.contains(&entity.id))
                } =>
                {
                    let remaining = compatible
                        .iter()
                        .copied()
                        .filter(|entity| !excluded.contains(&entity.id))
                        .collect::<Vec<_>>();
                    let [entity] = remaining.as_slice() else {
                        return None;
                    };
                    Some(*entity)
                }
                [] if operand_allows_compatible_ordinal_fallback(kind) => {
                    compatible.get(usize::from(address)).copied().or_else(|| {
                        (kind == FeatureInputOperandKind::Native(0xbc7c))
                            .then(|| {
                                entities
                                    .iter()
                                    .copied()
                                    .filter(|entity| {
                                        matches!(
                                            entity.kind,
                                            SketchInputKind::LineOrCircle | SketchInputKind::Arc
                                        ) && entity.local_id == Some(u32::from(address))
                                            && !excluded.contains(&entity.id)
                                    })
                                    .collect::<Vec<_>>()
                            })
                            .and_then(|candidates| {
                                let [candidate] = candidates.as_slice() else {
                                    return None;
                                };
                                Some(*candidate)
                            })
                    })
                }
                _ => None,
            }
        }
        _ => unique_marker_candidate(
            &exact
                .iter()
                .map(|entity| (entity.id.clone(), entity.coordinates_m.is_some()))
                .collect::<Vec<_>>(),
        )
        .and_then(|id| exact.iter().copied().find(|entity| entity.id == id)),
    }
}

fn point_operand_uses_link_graph(kind: FeatureInputOperandKind) -> bool {
    matches!(kind, FeatureInputOperandKind::D6)
}

fn linked_point_markers<'a>(
    entities: &[&'a SketchInputEntity],
    address: u16,
    kind: FeatureInputOperandKind,
    excluded: &HashSet<String>,
) -> Vec<&'a SketchInputEntity> {
    let by_id = entities
        .iter()
        .map(|entity| (entity.id.as_str(), *entity))
        .collect::<HashMap<_, _>>();
    let mut pending = entities
        .iter()
        .copied()
        .filter(|entity| entity.local_id == Some(u32::from(address)))
        .filter(|entity| !operand_accepts_marker(kind, entity.kind))
        .map(|entity| entity.id.as_str())
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
        if operand_accepts_marker(kind, entity.kind) && !excluded.contains(&entity.id) {
            compatible.push(entity);
            continue;
        }
        pending.extend(entity.links.iter().map(|link| link.entity_ref.as_str()));
    }
    compatible
}

fn operand_accepts_link_indirection(kind: FeatureInputOperandKind) -> bool {
    matches!(
        kind,
        FeatureInputOperandKind::E1
            | FeatureInputOperandKind::Native(0x8386 | 0x83fe | 0x8dda | 0xbc87)
    )
}

pub(super) fn linked_coordinate_line_endpoints<'a>(
    marker: &SketchInputEntity,
    markers_by_id: &HashMap<&str, &'a SketchInputEntity>,
) -> Option<[&'a SketchInputEntity; 2]> {
    let links = marker
        .links
        .iter()
        .filter(|link| link.entity_ref != marker.id)
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
    if matches!(marker.kind, SketchInputKind::Relation(_))
        && ![first, second].into_iter().all(|endpoint| {
            matches!(
                endpoint.kind,
                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
            )
        })
    {
        return None;
    }
    (first.id != second.id).then_some([first, second])
}

pub(super) fn coordinate_line_endpoints_with_linked_point<'a>(
    marker: &'a SketchInputEntity,
    markers_by_id: &HashMap<&str, &'a SketchInputEntity>,
) -> Option<[&'a SketchInputEntity; 2]> {
    if !matches!(
        marker.kind,
        SketchInputKind::LineOrCircle | SketchInputKind::Arc
    ) || marker.coordinates_m.is_none()
    {
        return None;
    }
    let mut endpoints = marker
        .links
        .iter()
        .filter(|link| link.entity_ref != marker.id)
        .filter_map(|link| markers_by_id.get(link.entity_ref.as_str()).copied())
        .filter(|endpoint| {
            endpoint.feature_ref == marker.feature_ref
                && endpoint.coordinates_m.is_some()
                && matches!(
                    endpoint.kind,
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
        })
        .collect::<Vec<_>>();
    endpoints.sort_unstable_by_key(|endpoint| endpoint.offset);
    endpoints.dedup_by_key(|endpoint| endpoint.id.as_str());
    let [endpoint] = endpoints.as_slice() else {
        return None;
    };
    Some([marker, *endpoint])
}

#[cfg(test)]
mod operands_tests;
