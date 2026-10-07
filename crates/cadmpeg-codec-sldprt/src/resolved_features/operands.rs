//! Scalar operand marker resolution over the link graph.

use super::selections::{
    operand_accepts_marker, operand_allows_compatible_ordinal_fallback,
    operand_uses_compatible_ordinal,
};
use crate::records::operand_tag::NativeOperandTag;
use crate::records::{
    FeatureInputOperand, FeatureInputOperandKind, SketchInputEntity, SketchInputKind,
};
use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use std::collections::{HashMap, HashSet};

pub(crate) fn resolve_scalar_operand_markers<'a>(
    ctx: &DecodeContext<'_>,
    entities: &[&'a SketchInputEntity],
    operands: &[FeatureInputOperand],
) -> Result<Vec<Option<&'a SketchInputEntity>>, CodecError> {
    let mut temporary_storage = ctx.reserve_scoped(0, "SLDPRT operands temporary storage")?;

    let mut resolved = Vec::new();
    for operand in ctx.admit_iter(operands, "resolve SLDPRT scalar operands")? {
        let marker = resolve_operand_marker_excluding(
            ctx,
            entities,
            operand.kind,
            operand.entity_index,
            |_| Ok(false),
        )?;
        ctx.push_vec(
            &mut resolved,
            marker,
            "collect SLDPRT resolved scalar operands",
        )?;
    }
    if let ([first_operand, second_operand], [Some(first), Some(second)]) =
        (operands, resolved.as_slice())
    {
        if ctx.equal(
            first.id(),
            second.id(),
            "compare SLDPRT scalar operand markers",
        )? && first_operand.entity_index != second_operand.entity_index
        {
            let alternatives = [
                resolve_operand_marker_excluding(
                    ctx,
                    entities,
                    first_operand.kind,
                    first_operand.entity_index,
                    |id| ctx.equal(id, second.id(), "compare SLDPRT scalar operand markers"),
                )?
                .map(|alternative| [alternative, *second]),
                resolve_operand_marker_excluding(
                    ctx,
                    entities,
                    second_operand.kind,
                    second_operand.entity_index,
                    |id| ctx.equal(id, first.id(), "compare SLDPRT scalar operand markers"),
                )?
                .map(|alternative| [*first, alternative]),
            ];
            let first_alternative = match alternatives[0] {
                Some([left, right])
                    if !ctx.equal(
                        left.id(),
                        right.id(),
                        "compare SLDPRT scalar operand markers",
                    )? =>
                {
                    Some([left, right])
                }
                _ => None,
            };
            let second_alternative = match alternatives[1] {
                Some([left, right])
                    if !ctx.equal(
                        left.id(),
                        right.id(),
                        "compare SLDPRT scalar operand markers",
                    )? =>
                {
                    Some([left, right])
                }
                _ => None,
            };
            let alternative = match (first_alternative, second_alternative) {
                (Some(alternative), None) | (None, Some(alternative)) => Some(alternative),
                _ => None,
            };
            if let Some(alternative) = alternative {
                resolved[0] = Some(alternative[0]);
                resolved[1] = Some(alternative[1]);
            }
        }
    }
    let mut resolved_siblings = HashSet::new();
    for entity in ctx
        .admit_iter(&resolved, "index resolved SLDPRT scalar operand markers")?
        .flatten()
    {
        temporary_storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut resolved_siblings,
                entity.id(),
                "index SLDPRT scalar operand markers",
            )
        })?;
    }
    for index in ctx.admit_iter(
        &(0..operands.len().min(resolved.len())),
        "resolve remaining SLDPRT scalar operands",
    )? {
        let operand = &operands[index];
        let target = &mut resolved[index];
        if target.is_none() {
            *target = resolve_operand_marker_excluding(
                ctx,
                entities,
                operand.kind,
                operand.entity_index,
                |id| {
                    ctx.contains_hash_set(
                        &resolved_siblings,
                        id,
                        "exclude resolved SLDPRT scalar operand markers",
                    )
                },
            )?;
        }
    }
    Ok(resolved)
}

fn operand_marker_index<'a>(
    ctx: &DecodeContext<'_>,
    entities: &[&'a SketchInputEntity],
) -> Result<HashMap<&'a str, &'a SketchInputEntity>, CodecError> {
    let mut result = HashMap::new();
    for entity in ctx.admit_iter(entities, "index SLDPRT scalar operand markers")? {
        ctx.insert_hash_map(
            &mut result,
            entity.id(),
            *entity,
            "index SLDPRT scalar operand markers",
        )?;
    }
    Ok(result)
}

#[cfg(test)]
fn resolve_operand_marker<'a>(
    entities: impl IntoIterator<Item = &'a SketchInputEntity>,
    kind: FeatureInputOperandKind,
    address: u16,
) -> Option<&'a SketchInputEntity> {
    let entities = entities.into_iter().collect::<Vec<_>>();
    resolve_operand_marker_excluding(
        &cadmpeg_test_support::service_decode_context(),
        &entities,
        kind,
        address,
        |_| Ok(false),
    )
    .unwrap()
}

/// The one candidate that matches, charging only the candidates visited
/// until a second match proves it ambiguous.
fn unique_entity<'a>(
    ctx: &DecodeContext<'_>,
    candidates: &[&'a SketchInputEntity],
    mut matches: impl FnMut(&SketchInputEntity) -> Result<bool, CodecError>,
    operation: &'static str,
) -> Result<Option<&'a SketchInputEntity>, CodecError> {
    let mut remaining = candidates.iter();
    let Some(selected) = ctx.find_by(&mut remaining, |entity| matches(entity), operation)? else {
        return Ok(None);
    };
    if ctx.any_by(&mut remaining, |entity| matches(entity), operation)? {
        return Ok(None);
    }
    Ok(Some(*selected))
}

fn resolve_operand_marker_excluding<'a>(
    ctx: &DecodeContext<'_>,
    entities: &[&'a SketchInputEntity],
    kind: FeatureInputOperandKind,
    address: u16,
    excluded: impl Fn(&str) -> Result<bool, CodecError>,
) -> Result<Option<&'a SketchInputEntity>, CodecError> {
    let mut temporary_storage = ctx.reserve_scoped(0, "SLDPRT operands temporary storage")?;

    let excluded = &excluded;
    if kind == FeatureInputOperandKind::Native(NativeOperandTag::TAG_81DD) {
        let mut points = Vec::new();
        for entity in ctx.admit_iter(entities, "scan SLDPRT scalar operand points")? {
            if !matches!(
                entity.kind(),
                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
            ) {
                continue;
            }
            if entity.coordinates_m.is_none() {
                continue;
            }
            temporary_storage.with_storage(|| {
                ctx.push_vec(&mut points, *entity, "collect SLDPRT scalar operand points")
            })?;
        }
        ctx.sort_unstable_by_key(
            &mut points,
            |value| value.offset(),
            Ord::cmp,
            "sort SLDPRT scalar operand points",
        )?;
        let Some(entity) = points.get(usize::from(address)).copied() else {
            return Ok(None);
        };
        return if excluded(entity.id())? {
            Ok(None)
        } else {
            Ok(Some(entity))
        };
    }
    if kind == FeatureInputOperandKind::Native(NativeOperandTag::TAG_81E7) {
        // In scalar relations, 81e7 addresses the solver-line roster formed
        // from coordinate points; it does not directly resolve a line marker.
        return Ok(None);
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
        if ctx.any_by(
            entities,
            |entity| Ok(entity.object_index() == Some(u32::from(address)) && accepts(entity)),
            "check indexed SLDPRT line operand address",
        )? {
            return unique_entity(
                ctx,
                entities,
                |entity| {
                    Ok(entity.object_index() == Some(u32::from(address))
                        && accepts(entity)
                        && !excluded(entity.id())?)
                },
                "select unique indexed SLDPRT line operand",
            );
        }
        return unique_entity(
            ctx,
            entities,
            |entity| {
                Ok(entity.local_id() == Some(u32::from(address))
                    && accepts(entity)
                    && !excluded(entity.id())?)
            },
            "select unique local SLDPRT line operand",
        );
    }
    if kind == FeatureInputOperandKind::Native(NativeOperandTag::TAG_BC7C) {
        if let Some(entity) = unique_entity(
            ctx,
            entities,
            |entity| {
                Ok(entity.object_index() == Some(u32::from(address))
                    && entity.coordinates_m.is_some()
                    && {
                        matches!(
                            entity.kind(),
                            SketchInputKind::Point
                                | SketchInputKind::ConstrainedPoint
                                | SketchInputKind::LineOrCircle
                                | SketchInputKind::Arc
                        )
                    }
                    && !excluded(entity.id())?)
            },
            "select unique indexed SLDPRT point operand",
        )? {
            return Ok(Some(entity));
        }
    }
    if kind == FeatureInputOperandKind::Native(NativeOperandTag::TAG_BC87) {
        if let Some(entity) = unique_entity(
            ctx,
            entities,
            |entity| {
                Ok(entity.object_index() == Some(u32::from(address))
                    && entity.coordinates_m.is_some()
                    && matches!(
                        entity.kind(),
                        SketchInputKind::LineOrCircle | SketchInputKind::Arc
                    )
                    && !excluded(entity.id())?)
            },
            "select unique indexed SLDPRT line operand",
        )? {
            return Ok(Some(entity));
        }
    }
    if kind == FeatureInputOperandKind::Native(NativeOperandTag::TAG_814C) {
        return unique_entity(
            ctx,
            entities,
            |entity| {
                Ok(entity.object_index() == Some(u32::from(address))
                    && entity.coordinates_m.is_some()
                    && matches!(
                        entity.kind(),
                        SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                    )
                    && !excluded(entity.id())?)
            },
            "select unique indexed SLDPRT point operand",
        );
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
        if let Some(entity) = unique_entity(
            ctx,
            entities,
            |entity| {
                Ok(entity.object_index() == Some(u32::from(address))
                    && entity.coordinates_m.is_some()
                    && operand_accepts_marker(kind, entity.kind())
                    && !excluded(entity.id())?)
            },
            "select unique indexed SLDPRT compatible operand",
        )? {
            return Ok(Some(entity));
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
        if ctx.any_by(
            entities,
            |entity| {
                Ok(entity.object_index() == Some(u32::from(address))
                    && operand_accepts_marker(kind, entity.kind()))
            },
            "check indexed SLDPRT point operand address",
        )? {
            return unique_entity(
                ctx,
                entities,
                |entity| {
                    Ok(entity.object_index() == Some(u32::from(address))
                        && operand_accepts_marker(kind, entity.kind())
                        && !excluded(entity.id())?)
                },
                "select unique indexed SLDPRT point operand",
            );
        }
        if let Some(entity) = unique_entity(
            ctx,
            entities,
            |entity| {
                Ok(entity.local_id() == Some(u32::from(address))
                    && operand_accepts_marker(kind, entity.kind())
                    && !excluded(entity.id())?)
            },
            "select unique local SLDPRT point operand",
        )? {
            return Ok(Some(entity));
        }
    }
    if matches!(
        kind,
        FeatureInputOperandKind::E1 | FeatureInputOperandKind::Native(NativeOperandTag::TAG_8386)
    ) {
        if let Some(entity) = unique_entity(
            ctx,
            entities,
            |entity| {
                Ok(entity.object_index() == Some(u32::from(address))
                    && operand_accepts_marker(kind, entity.kind())
                    && !excluded(entity.id())?)
            },
            "select unique indexed SLDPRT operand",
        )? {
            return Ok(Some(entity));
        }
        if kind == FeatureInputOperandKind::Native(NativeOperandTag::TAG_8386) {
            let entities_by_id =
                temporary_storage.with_storage(|| operand_marker_index(ctx, entities))?;
            if let Some(entity) = unique_entity(
                ctx,
                entities,
                |entity| {
                    Ok(entity.object_index() == Some(u32::from(address))
                        && !excluded(entity.id())?
                        && linked_coordinate_line_endpoints(ctx, entity, &entities_by_id)?
                            .is_some())
                },
                "select unique indexed SLDPRT line handle",
            )? {
                return Ok(Some(entity));
            }
        }
    }
    let mut compatible = Vec::new();
    for entity in ctx.admit_iter(entities, "scan SLDPRT compatible operand markers")? {
        if operand_accepts_marker(kind, entity.kind()) {
            temporary_storage.with_storage(|| {
                ctx.push_vec(
                    &mut compatible,
                    *entity,
                    "collect SLDPRT compatible operand markers",
                )
            })?;
        }
    }
    ctx.sort_unstable_by_key(
        &mut compatible,
        |value| value.offset(),
        Ord::cmp,
        "sort SLDPRT compatible operand markers",
    )?;
    let mut ordinal_link_graph = false;
    if operand_uses_compatible_ordinal(kind) {
        if let Some(entity) = compatible.get(usize::from(address)).copied() {
            if excluded(entity.id())? {
                return Ok(None);
            }
            return Ok(Some(entity));
        }
        if !point_operand_uses_link_graph(kind) {
            return Ok(None);
        }
        ordinal_link_graph = true;
    }
    let mut exact = compatible.iter();
    let exact_match = |entity: &&&SketchInputEntity| {
        Ok(!ordinal_link_graph
            && entity.local_id() == Some(u32::from(address))
            && !excluded(entity.id())?)
    };
    let exact_first = ctx
        .find_by(&mut exact, exact_match, "scan exact SLDPRT operand markers")?
        .copied();
    let exact_second = exact_first.is_some()
        && ctx.any_by(
            &mut exact,
            |entity| exact_match(&entity),
            "scan exact SLDPRT operand markers",
        )?;
    Ok(match (exact_first, exact_second) {
        (Some(entity), false) => Some(entity),
        (None, _) => {
            if kind == FeatureInputOperandKind::Native(NativeOperandTag::TAG_8386) {
                let entities_by_id =
                    temporary_storage.with_storage(|| operand_marker_index(ctx, entities))?;
                if let Some(entity) = unique_entity(
                    ctx,
                    entities,
                    |entity| {
                        Ok(entity.local_id() == Some(u32::from(address))
                            && !excluded(entity.id())?
                            && linked_coordinate_line_endpoints(ctx, entity, &entities_by_id)?
                                .is_some())
                    },
                    "select unique local SLDPRT line handle",
                )? {
                    return Ok(Some(entity));
                }
            }
            let mut indirect = if point_operand_uses_link_graph(kind) {
                linked_point_markers(
                    ctx,
                    &mut temporary_storage,
                    entities,
                    address,
                    kind,
                    excluded,
                )?
            } else if operand_accepts_link_indirection(kind) {
                let mut indirect = Vec::new();
                let mut first_by_id = HashMap::new();
                for entity in ctx.admit_iter(entities, "index SLDPRT linked operand targets")? {
                    temporary_storage.with_storage(|| {
                        ctx.entry_hash_map(
                            &mut first_by_id,
                            entity.id(),
                            "index SLDPRT linked operand targets",
                        )
                        .map(|slot| {
                            slot.or_insert(*entity);
                        })
                    })?;
                }
                for entity in ctx.admit_iter(entities, "scan SLDPRT linked operand handles")? {
                    if entity.local_id() != Some(u32::from(address)) {
                        continue;
                    }
                    for link in ctx.admit_iter(entity.links(), "scan SLDPRT linked operands")? {
                        let Some(&target) = ctx.get_hash_map(
                            &first_by_id,
                            link.entity_ref.as_str(),
                            "resolve SLDPRT linked operand target",
                        )?
                        else {
                            continue;
                        };
                        if !operand_accepts_marker(kind, target.kind()) || excluded(target.id())? {
                            continue;
                        }
                        temporary_storage.with_storage(|| {
                            ctx.push_vec(
                                &mut indirect,
                                target,
                                "collect SLDPRT indirect operand markers",
                            )
                        })?;
                    }
                }
                indirect
            } else {
                Vec::new()
            };
            ctx.sort_unstable_by(
                &mut indirect,
                |value| value.id(),
                Ord::cmp,
                "sort SLDPRT indirect operand markers",
            )?;
            ctx.dedup_by_key(
                &mut indirect,
                |entity| Ok(entity.id()),
                "deduplicate SLDPRT indirect operand markers",
            )?;
            match indirect.as_slice() {
                [entity] => Some(*entity),
                [] if point_operand_uses_link_graph(kind) && {
                    let linked = linked_point_markers(
                        ctx,
                        &mut temporary_storage,
                        entities,
                        address,
                        kind,
                        |_| Ok(false),
                    )?;
                    !linked.is_empty()
                        && ctx.all_by(
                            &linked,
                            |entity| excluded(entity.id()),
                            "check excluded SLDPRT linked point markers",
                        )?
                } =>
                {
                    unique_entity(
                        ctx,
                        &compatible,
                        |entity| Ok(!excluded(entity.id())?),
                        "select unique remaining SLDPRT operand",
                    )?
                }
                [] if operand_allows_compatible_ordinal_fallback(kind) => {
                    if let Some(entity) = compatible.get(usize::from(address)).copied() {
                        Some(entity)
                    } else if kind == FeatureInputOperandKind::Native(NativeOperandTag::TAG_BC7C) {
                        unique_entity(
                            ctx,
                            entities,
                            |entity| {
                                Ok(matches!(
                                    entity.kind(),
                                    SketchInputKind::LineOrCircle | SketchInputKind::Arc
                                ) && entity.local_id() == Some(u32::from(address))
                                    && !excluded(entity.id())?)
                            },
                            "select unique local SLDPRT line operand",
                        )?
                    } else {
                        None
                    }
                }
                _ => None,
            }
        }
        (Some(_), true) => unique_entity(
            ctx,
            &compatible,
            |entity| {
                Ok(!ordinal_link_graph
                    && entity.local_id() == Some(u32::from(address))
                    && entity.coordinates_m.is_some()
                    && !excluded(entity.id())?)
            },
            "select unique coordinate SLDPRT operand markers",
        )?,
    })
}

fn point_operand_uses_link_graph(kind: FeatureInputOperandKind) -> bool {
    matches!(kind, FeatureInputOperandKind::D6)
}

fn linked_point_markers<'a>(
    ctx: &DecodeContext<'_>,
    temporary_storage: &mut ScopedReservation<'_>,
    entities: &[&'a SketchInputEntity],
    address: u16,
    kind: FeatureInputOperandKind,
    excluded: impl Fn(&str) -> Result<bool, CodecError>,
) -> Result<Vec<&'a SketchInputEntity>, CodecError> {
    let by_id = temporary_storage.with_storage(|| operand_marker_index(ctx, entities))?;
    let mut pending = Vec::new();
    for entity in ctx.admit_iter(entities, "collect SLDPRT scalar operand link roots")? {
        if entity.local_id() == Some(u32::from(address))
            && !operand_accepts_marker(kind, entity.kind())
        {
            temporary_storage.with_storage(|| {
                ctx.push_vec(
                    &mut pending,
                    entity.id(),
                    "collect SLDPRT scalar operand link roots",
                )
            })?;
        }
    }
    let mut visited = HashSet::new();
    let mut compatible = Vec::new();
    while let Some(id) = pending.pop() {
        ctx.charge_work(1, "walk SLDPRT scalar operand links")?;
        if ctx.contains_hash_set(&visited, id, "check visited SLDPRT scalar operand links")? {
            continue;
        }
        temporary_storage.with_storage(|| {
            ctx.insert_hash_set(&mut visited, id, "index SLDPRT scalar operand markers")
        })?;
        let Some(entity) = ctx
            .get_hash_map(&by_id, id, "resolve SLDPRT scalar operand link")?
            .copied()
        else {
            continue;
        };
        if operand_accepts_marker(kind, entity.kind()) && !excluded(entity.id())? {
            temporary_storage.with_storage(|| {
                ctx.push_vec(
                    &mut compatible,
                    entity,
                    "collect SLDPRT scalar operand markers",
                )
            })?;
            continue;
        }
        for link in ctx.admit_iter(entity.links(), "scan SLDPRT scalar operand links")? {
            temporary_storage.with_storage(|| {
                ctx.push_vec(
                    &mut pending,
                    link.entity_ref.as_str(),
                    "collect SLDPRT scalar operand links",
                )
            })?;
        }
    }
    Ok(compatible)
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
    ctx: &DecodeContext<'_>,
    marker: &SketchInputEntity,
    markers_by_id: &HashMap<&str, &'a SketchInputEntity>,
) -> Result<Option<[&'a SketchInputEntity; 2]>, CodecError> {
    let mut links = [None, None];
    for link in ctx.admit_iter(marker.links(), "resolve SLDPRT linked line endpoints")? {
        if ctx.equal(
            link.entity_ref.as_str(),
            marker.id(),
            "compare SLDPRT linked line identities",
        )? {
            continue;
        }
        match links {
            [None, _] => links[0] = Some(link),
            [Some(_), None] => links[1] = Some(link),
            [Some(_), Some(_)] => return Ok(None),
        }
    }
    let [Some(first_link), Some(second_link)] = links else {
        return Ok(None);
    };
    let Some(first) = ctx
        .get_hash_map(
            markers_by_id,
            first_link.entity_ref.as_str(),
            "resolve SLDPRT first linked line endpoint",
        )?
        .copied()
    else {
        return Ok(None);
    };
    let Some(second) = ctx
        .get_hash_map(
            markers_by_id,
            second_link.entity_ref.as_str(),
            "resolve SLDPRT second linked line endpoint",
        )?
        .copied()
    else {
        return Ok(None);
    };
    if !ctx.equal(
        &first.feature_ref.as_deref(),
        &marker.feature_ref.as_deref(),
        "compare SLDPRT linked line features",
    )? || first.coordinates_m.is_none()
    {
        return Ok(None);
    }
    if !ctx.equal(
        &second.feature_ref.as_deref(),
        &marker.feature_ref.as_deref(),
        "compare SLDPRT linked line features",
    )? || second.coordinates_m.is_none()
    {
        return Ok(None);
    }
    if matches!(marker.kind(), SketchInputKind::Relation(_))
        && ![first, second].into_iter().all(|endpoint| {
            matches!(
                endpoint.kind(),
                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
            )
        })
    {
        return Ok(None);
    }
    if ctx.equal(
        first.id(),
        second.id(),
        "compare SLDPRT linked line endpoints",
    )? {
        Ok(None)
    } else {
        Ok(Some([first, second]))
    }
}

pub(super) fn coordinate_line_endpoints_with_linked_point<'a>(
    ctx: &DecodeContext<'_>,
    marker: &'a SketchInputEntity,
    markers_by_id: &HashMap<&str, &'a SketchInputEntity>,
) -> Result<Option<[&'a SketchInputEntity; 2]>, CodecError> {
    if !matches!(
        marker.kind(),
        SketchInputKind::LineOrCircle | SketchInputKind::Arc
    ) || marker.coordinates_m.is_none()
    {
        return Ok(None);
    }
    let mut selected: Option<&'a SketchInputEntity> = None;
    for link in ctx.admit_iter(marker.links(), "scan SLDPRT linked line endpoints")? {
        if ctx.equal(
            link.entity_ref.as_str(),
            marker.id(),
            "compare SLDPRT linked line identities",
        )? {
            continue;
        }
        let Some(endpoint) = ctx
            .get_hash_map(
                markers_by_id,
                link.entity_ref.as_str(),
                "resolve SLDPRT linked line endpoint",
            )?
            .copied()
        else {
            continue;
        };
        if !ctx.equal(
            &endpoint.feature_ref.as_deref(),
            &marker.feature_ref.as_deref(),
            "compare SLDPRT linked line features",
        )? || endpoint.coordinates_m.is_none()
            || !matches!(
                endpoint.kind(),
                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
            )
        {
            continue;
        }
        if let Some(first) = selected {
            if !ctx.equal(
                first.id(),
                endpoint.id(),
                "compare SLDPRT linked line endpoints",
            )? {
                return Ok(None);
            }
        } else {
            selected = Some(endpoint);
        }
    }
    Ok(selected.map(|endpoint| [marker, endpoint]))
}

#[cfg(test)]
mod operands_tests;
