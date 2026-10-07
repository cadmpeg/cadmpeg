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

/// Candidate rosters in source order, with ordinal rosters in offset order.
/// The five compatibility families partition the operand acceptance rules.
struct OperandCandidates<'a, 'ctx> {
    by_object: HashMap<u32, Vec<&'a SketchInputEntity>>,
    by_local: HashMap<u32, Vec<&'a SketchInputEntity>>,
    by_id: HashMap<&'a str, &'a SketchInputEntity>,
    first_by_id: HashMap<&'a str, &'a SketchInputEntity>,
    compatible: [Vec<&'a SketchInputEntity>; 5],
    coordinate_points: Vec<&'a SketchInputEntity>,
    _storage: ScopedReservation<'ctx>,
}

fn compatibility_family(kind: FeatureInputOperandKind) -> usize {
    // These representatives have the five distinct marker acceptance sets.
    match kind {
        FeatureInputOperandKind::D6
        | FeatureInputOperandKind::Native(
            NativeOperandTag::TAG_80CC
            | NativeOperandTag::TAG_8152
            | NativeOperandTag::TAG_81B2
            | NativeOperandTag::TAG_8AB6
            | NativeOperandTag::TAG_8DCB
            | NativeOperandTag::TAG_929D
            | NativeOperandTag::TAG_BC7C
            | NativeOperandTag::TAG_BD69
            | NativeOperandTag::TAG_81DD,
        ) => 0,
        FeatureInputOperandKind::E1
        | FeatureInputOperandKind::Native(
            NativeOperandTag::TAG_8386
            | NativeOperandTag::TAG_83FE
            | NativeOperandTag::TAG_8DDA
            | NativeOperandTag::TAG_BC87
            | NativeOperandTag::TAG_81E7,
        ) => 1,
        FeatureInputOperandKind::Native(NativeOperandTag::TAG_837B) => 2,
        FeatureInputOperandKind::Native(
            NativeOperandTag::TAG_80AC | NativeOperandTag::TAG_80D5 | NativeOperandTag::TAG_8138,
        ) => 3,
        FeatureInputOperandKind::Native(_) => 4,
    }
}

impl<'a, 'ctx> OperandCandidates<'a, 'ctx> {
    fn new(
        ctx: &'ctx DecodeContext<'_>,
        entities: &[&'a SketchInputEntity],
        kinds: impl IntoIterator<Item = FeatureInputOperandKind>,
    ) -> Result<Self, CodecError> {
        const OPERATION: &str = "index SLDPRT scalar operand candidates";
        let mut needed = [false; 5];
        let mut needs_coordinate_points = false;
        let mut needs_object = false;
        let mut needs_local = false;
        let mut needs_last_id = false;
        let mut needs_first_id = false;
        let mut kinds = kinds.into_iter();
        while let Some(kind) = ctx.next_charged(&mut kinds, OPERATION)? {
            if kind == FeatureInputOperandKind::Native(NativeOperandTag::TAG_81E7) {
                continue;
            }
            needs_coordinate_points |=
                kind == FeatureInputOperandKind::Native(NativeOperandTag::TAG_81DD);
            if kind == FeatureInputOperandKind::Native(NativeOperandTag::TAG_81DD) {
                continue;
            }
            needs_object = true;
            if kind == FeatureInputOperandKind::Native(NativeOperandTag::TAG_814C) {
                continue;
            }
            needs_local = true;
            if kind == FeatureInputOperandKind::Native(NativeOperandTag::TAG_810F) {
                continue;
            }
            needs_first_id |= operand_accepts_link_indirection(kind);
            needs_last_id |= point_operand_uses_link_graph(kind)
                || kind == FeatureInputOperandKind::Native(NativeOperandTag::TAG_8386);
            needed[compatibility_family(kind)] = true;
        }
        let mut result = Self {
            by_object: HashMap::new(),
            by_local: HashMap::new(),
            by_id: HashMap::new(),
            first_by_id: HashMap::new(),
            compatible: std::array::from_fn(|_| Vec::new()),
            coordinate_points: Vec::new(),
            _storage: ctx.reserve_scoped(0, OPERATION)?,
        };
        if !needs_object && !needs_coordinate_points {
            return Ok(result);
        }
        for &entity in ctx.admit_iter(entities, OPERATION)? {
            result._storage.with_storage(|| {
                if let Some(index) = entity.object_index().filter(|_| needs_object) {
                    ctx.push_hash_group(
                        &mut result.by_object,
                        index,
                        entity,
                        OPERATION,
                        OPERATION,
                    )?;
                }
                if let Some(index) = entity.local_id().filter(|_| needs_local) {
                    ctx.push_hash_group(&mut result.by_local, index, entity, OPERATION, OPERATION)?;
                }
                if needs_last_id {
                    ctx.insert_hash_map(&mut result.by_id, entity.id(), entity, OPERATION)?;
                }
                if needs_first_id
                    && !ctx.contains_key_hash_map(&result.first_by_id, entity.id(), OPERATION)?
                {
                    ctx.insert_hash_map(&mut result.first_by_id, entity.id(), entity, OPERATION)?;
                }
                for (family, representative) in [
                    FeatureInputOperandKind::D6,
                    FeatureInputOperandKind::E1,
                    FeatureInputOperandKind::Native(NativeOperandTag::TAG_837B),
                    FeatureInputOperandKind::Native(NativeOperandTag::TAG_80AC),
                    FeatureInputOperandKind::Native(NativeOperandTag::TAG_8100),
                ]
                .into_iter()
                .enumerate()
                {
                    if needed[family] && operand_accepts_marker(representative, entity.kind()) {
                        ctx.push_vec(&mut result.compatible[family], entity, OPERATION)?;
                    }
                }
                if needs_coordinate_points
                    && entity.coordinates_m.is_some()
                    && matches!(
                        entity.kind(),
                        SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                    )
                {
                    ctx.push_vec(&mut result.coordinate_points, entity, OPERATION)?;
                }
                Ok::<_, CodecError>(())
            })?;
        }
        for roster in &mut result.compatible {
            ctx.sort_unstable_by_key(
                roster,
                |entity| entity.offset(),
                Ord::cmp,
                "sort SLDPRT compatible operand markers",
            )?;
        }
        ctx.sort_unstable_by_key(
            &mut result.coordinate_points,
            |entity| entity.offset(),
            Ord::cmp,
            "sort SLDPRT scalar operand points",
        )?;
        Ok(result)
    }
}

pub(crate) fn resolve_scalar_operand_markers<'a>(
    ctx: &DecodeContext<'_>,
    entities: &[&'a SketchInputEntity],
    operands: &[FeatureInputOperand],
) -> Result<Vec<Option<&'a SketchInputEntity>>, CodecError> {
    let mut temporary_storage = ctx.reserve_scoped(0, "SLDPRT operands temporary storage")?;
    if operands.is_empty() {
        return Ok(Vec::new());
    }
    let candidates =
        OperandCandidates::new(ctx, entities, operands.iter().map(|operand| operand.kind))?;
    let mut resolved = Vec::new();
    for operand in ctx.admit_iter(operands, "resolve SLDPRT scalar operands")? {
        let marker = resolve_indexed_operand_marker(
            ctx,
            &candidates,
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
                resolve_indexed_operand_marker(
                    ctx,
                    &candidates,
                    first_operand.kind,
                    first_operand.entity_index,
                    |id| ctx.equal(id, second.id(), "compare SLDPRT scalar operand markers"),
                )?
                .map(|alternative| [alternative, *second]),
                resolve_indexed_operand_marker(
                    ctx,
                    &candidates,
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
            *target = resolve_indexed_operand_marker(
                ctx,
                &candidates,
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

#[cfg(test)]
fn resolve_operand_marker_excluding<'a>(
    ctx: &DecodeContext<'_>,
    entities: &[&'a SketchInputEntity],
    kind: FeatureInputOperandKind,
    address: u16,
    excluded: impl Fn(&str) -> Result<bool, CodecError>,
) -> Result<Option<&'a SketchInputEntity>, CodecError> {
    let candidates = OperandCandidates::new(ctx, entities, [kind])?;
    resolve_indexed_operand_marker(ctx, &candidates, kind, address, excluded)
}

fn resolve_indexed_operand_marker<'a>(
    ctx: &DecodeContext<'_>,
    candidates: &OperandCandidates<'a, '_>,
    kind: FeatureInputOperandKind,
    address: u16,
    excluded: impl Fn(&str) -> Result<bool, CodecError>,
) -> Result<Option<&'a SketchInputEntity>, CodecError> {
    let mut temporary_storage = ctx.reserve_scoped(0, "SLDPRT operands temporary storage")?;

    let excluded = &excluded;
    if kind == FeatureInputOperandKind::Native(NativeOperandTag::TAG_81DD) {
        let Some(entity) = candidates
            .coordinate_points
            .get(usize::from(address))
            .copied()
        else {
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
    let indexed = ctx
        .get_hash_map(
            &candidates.by_object,
            &u32::from(address),
            "find indexed SLDPRT operand candidates",
        )?
        .map_or(&[][..], Vec::as_slice);
    let local = if kind == FeatureInputOperandKind::Native(NativeOperandTag::TAG_814C) {
        &[][..]
    } else {
        ctx.get_hash_map(
            &candidates.by_local,
            &u32::from(address),
            "find local SLDPRT operand candidates",
        )?
        .map_or(&[][..], Vec::as_slice)
    };
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
            indexed,
            |entity| Ok(entity.object_index() == Some(u32::from(address)) && accepts(entity)),
            "check indexed SLDPRT line operand address",
        )? {
            return unique_entity(
                ctx,
                indexed,
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
            local,
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
            indexed,
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
            indexed,
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
            indexed,
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
            indexed,
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
            indexed,
            |entity| {
                Ok(entity.object_index() == Some(u32::from(address))
                    && operand_accepts_marker(kind, entity.kind()))
            },
            "check indexed SLDPRT point operand address",
        )? {
            return unique_entity(
                ctx,
                indexed,
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
            local,
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
            indexed,
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
            if let Some(entity) = unique_entity(
                ctx,
                indexed,
                |entity| {
                    Ok(entity.object_index() == Some(u32::from(address))
                        && !excluded(entity.id())?
                        && linked_coordinate_line_endpoints(ctx, entity, &candidates.by_id)?
                            .is_some())
                },
                "select unique indexed SLDPRT line handle",
            )? {
                return Ok(Some(entity));
            }
        }
    }
    let compatible = &candidates.compatible[compatibility_family(kind)];
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
    let mut exact = local.iter();
    let exact_match = |entity: &&&SketchInputEntity| {
        Ok(!ordinal_link_graph
            && operand_accepts_marker(kind, entity.kind())
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
                if let Some(entity) = unique_entity(
                    ctx,
                    local,
                    |entity| {
                        Ok(entity.local_id() == Some(u32::from(address))
                            && !excluded(entity.id())?
                            && linked_coordinate_line_endpoints(ctx, entity, &candidates.by_id)?
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
                    candidates,
                    address,
                    kind,
                    excluded,
                )?
            } else if operand_accepts_link_indirection(kind) {
                let mut indirect = Vec::new();
                for entity in ctx.admit_iter(local, "scan SLDPRT linked operand handles")? {
                    if entity.local_id() != Some(u32::from(address)) {
                        continue;
                    }
                    for link in ctx.admit_iter(entity.links(), "scan SLDPRT linked operands")? {
                        let Some(&target) = ctx.get_hash_map(
                            &candidates.first_by_id,
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
                        candidates,
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
                        compatible,
                        |entity| excluded(entity.id()).map(|excluded| !excluded),
                        "select unique remaining SLDPRT operand",
                    )?
                }
                [] if operand_allows_compatible_ordinal_fallback(kind) => {
                    if let Some(entity) = compatible.get(usize::from(address)).copied() {
                        Some(entity)
                    } else if kind == FeatureInputOperandKind::Native(NativeOperandTag::TAG_BC7C) {
                        unique_entity(
                            ctx,
                            local,
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
            local,
            |entity| {
                Ok(operand_accepts_marker(kind, entity.kind())
                    && !ordinal_link_graph
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
    candidates: &OperandCandidates<'a, '_>,
    address: u16,
    kind: FeatureInputOperandKind,
    excluded: impl Fn(&str) -> Result<bool, CodecError>,
) -> Result<Vec<&'a SketchInputEntity>, CodecError> {
    let roots = ctx
        .get_hash_map(
            &candidates.by_local,
            &u32::from(address),
            "find SLDPRT scalar operand link roots",
        )?
        .map_or(&[][..], Vec::as_slice);
    let mut pending = Vec::new();
    for entity in ctx.admit_iter(roots, "collect SLDPRT scalar operand link roots")? {
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
            .get_hash_map(&candidates.by_id, id, "resolve SLDPRT scalar operand link")?
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
