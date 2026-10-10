// SPDX-License-Identifier: Apache-2.0
//! Sketch profile assembly from persisted entity order and endpoint constraints.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{
    SketchConstraint, SketchConstraintDefinitionInput, SketchEntity, SketchEntityUse,
    SketchGeometryDefinition, SketchLocus,
};

pub(super) fn build_profiles(
    ctx: &DecodeContext<'_>,
    entities: &[SketchEntity],
    constraints: &[SketchConstraint],
) -> Result<Vec<Vec<SketchEntityUse>>, CodecError> {
    // Internal entities arrive in GeometryList order; appended external and built-in reference
    // entities are construction entries. Indices therefore preserve the persisted ordinal for
    // every eligible profile entity.
    let mut profile_storage = ctx.reserve_scoped(0, "FCStd profile entity ordinals")?;
    let mut profile_entities = BTreeSet::new();
    profile_storage.with_storage(|| {
        let mut source = entities.iter();
        while source.len() != 0 {
            let index = entities.len() - source.len();
            let Some(entity) = ctx.next_charged(&mut source, "FCStd profile entity scan")? else {
                break;
            };
            if !entity.construction {
                ctx.insert_btree_set(&mut profile_entities, index, "FCStd profile ordinals")?;
            }
        }
        Ok::<_, CodecError>(())
    })?;
    let mut unused_storage = ctx.reserve_scoped(0, "FCStd remaining profile ordinals")?;
    let mut unused = unused_storage.with_storage(|| {
        ctx.collect_btree_set(
            profile_entities.iter().copied(),
            "FCStd remaining profile ordinals",
        )
    })?;
    let (explicit_storage, explicit_relations);
    (explicit_relations, explicit_storage) =
        explicit_endpoint_relations(ctx, &profile_entities, entities, constraints)?;
    let (index_storage, index);
    (index, index_storage) = EndpointIndex::new(ctx, &profile_entities, entities)?;
    let mut ambiguous_storage = ctx.reserve_scoped(0, "FCStd ambiguous profile ordinals")?;
    let mut ambiguous = BTreeSet::new();
    let mut entity_sources = unused.iter();
    while entity_sources.len() != 0 {
        let Some(entity) = ctx.next_charged(&mut entity_sources, "FCStd profile ambiguity scan")?
        else {
            break;
        };
        for start in [true, false] {
            let (matches_storage, matches);
            (matches, matches_storage) = endpoint_candidates(
                ctx,
                EndpointLocus {
                    entity: *entity,
                    start,
                },
                &unused,
                &explicit_relations,
                entities,
                &index,
            )?;
            if matches.len() > 1 {
                ambiguous_storage.with_storage(|| {
                    ctx.insert_btree_set(
                        &mut ambiguous,
                        *entity,
                        "FCStd ambiguous profile ordinals",
                    )
                })?;
                let mut candidates = matches.iter();
                while candidates.len() != 0 {
                    let Some(candidate) =
                        ctx.next_charged(&mut candidates, "FCStd ambiguous profile matches")?
                    else {
                        break;
                    };
                    ambiguous_storage.with_storage(|| {
                        ctx.insert_btree_set(
                            &mut ambiguous,
                            candidate.entity,
                            "FCStd ambiguous profile ordinals",
                        )
                    })?;
                }
            }
            drop(matches);
            drop(matches_storage);
        }
    }
    let mut profiles = Vec::new();
    // FreeCAD persists no profile seed. CADIR selects the first remaining persisted ordinal.
    while let Some(first) = unused.first().copied() {
        unused_storage.with_storage(|| {
            ctx.remove_btree_set(&mut unused, &first, "FCStd remaining profile ordinals")
        })?;
        let mut chain_storage = ctx.reserve_scoped(0, "FCStd profile uses")?;
        let mut chain = VecDeque::new();
        chain_storage
            .with_storage(|| ctx.push_back(&mut chain, (first, false), "FCStd profile uses"))?;
        if ctx.contains_btree_set(&ambiguous, &first, "FCStd ambiguous profile lookup")? {
            ctx.push_vec(
                &mut profiles,
                finish_profile_chain(ctx, chain_storage, chain, entities)?,
                "FCStd profile chains",
            )?;
            continue;
        }
        if endpoints(&entities[first]).is_none() {
            ctx.push_vec(
                &mut profiles,
                finish_profile_chain(ctx, chain_storage, chain, entities)?,
                "FCStd profile chains",
            )?;
            continue;
        }
        let mut head = EndpointLocus {
            entity: first,
            start: true,
        };
        let mut tail = EndpointLocus {
            entity: first,
            start: false,
        };
        loop {
            let (_candidate_storage, mut candidates);
            (candidates, _candidate_storage) =
                endpoint_candidates(ctx, tail, &unused, &explicit_relations, entities, &index)?;
            ctx.retain_vec(
                &mut candidates,
                |candidate| {
                    ctx.contains_btree_set(
                        &ambiguous,
                        &candidate.entity,
                        "FCStd ambiguous profile lookup",
                    )
                    .map(|ambiguous| !ambiguous)
                },
                "FCStd unambiguous profile candidates",
            )?;
            let Some(candidate) = (candidates.len() == 1).then(|| candidates[0]) else {
                break;
            };
            let (reversed, next_tail) = if candidate.start {
                (
                    false,
                    EndpointLocus {
                        entity: candidate.entity,
                        start: false,
                    },
                )
            } else {
                (
                    true,
                    EndpointLocus {
                        entity: candidate.entity,
                        start: true,
                    },
                )
            };
            unused_storage.with_storage(|| {
                ctx.remove_btree_set(
                    &mut unused,
                    &candidate.entity,
                    "FCStd remaining profile ordinals",
                )
            })?;
            chain_storage.with_storage(|| {
                ctx.push_back(
                    &mut chain,
                    (candidate.entity, reversed),
                    "FCStd profile uses",
                )
            })?;
            tail = next_tail;
        }
        loop {
            let (_candidate_storage, mut candidates);
            (candidates, _candidate_storage) =
                endpoint_candidates(ctx, head, &unused, &explicit_relations, entities, &index)?;
            ctx.retain_vec(
                &mut candidates,
                |candidate| {
                    ctx.contains_btree_set(
                        &ambiguous,
                        &candidate.entity,
                        "FCStd ambiguous profile lookup",
                    )
                    .map(|ambiguous| !ambiguous)
                },
                "FCStd unambiguous profile candidates",
            )?;
            let Some(candidate) = (candidates.len() == 1).then(|| candidates[0]) else {
                break;
            };
            let (reversed, next_head) = if candidate.start {
                (
                    true,
                    EndpointLocus {
                        entity: candidate.entity,
                        start: false,
                    },
                )
            } else {
                (
                    false,
                    EndpointLocus {
                        entity: candidate.entity,
                        start: true,
                    },
                )
            };
            unused_storage.with_storage(|| {
                ctx.remove_btree_set(
                    &mut unused,
                    &candidate.entity,
                    "FCStd remaining profile ordinals",
                )
            })?;
            chain_storage.with_storage(|| {
                ctx.push_front(
                    &mut chain,
                    (candidate.entity, reversed),
                    "FCStd profile uses",
                )
            })?;
            head = next_head;
        }
        ctx.push_vec(
            &mut profiles,
            finish_profile_chain(ctx, chain_storage, chain, entities)?,
            "FCStd profile chains",
        )?;
    }
    drop(index);
    drop(index_storage);
    drop(explicit_relations);
    drop(explicit_storage);
    drop(ambiguous);
    drop(ambiguous_storage);
    drop(unused);
    drop(unused_storage);
    drop(profile_entities);
    drop(profile_storage);
    Ok(profiles)
}

fn finish_profile_chain(
    ctx: &DecodeContext<'_>,
    _chain_storage: ScopedReservation<'_>,
    chain: VecDeque<(usize, bool)>,
    entities: &[SketchEntity],
) -> Result<Vec<SketchEntityUse>, CodecError> {
    let mut profile = ctx.vector_storage(chain.len(), "FCStd profile chain extraction")?;
    let mut source = chain.iter();
    while source.len() != 0 {
        let Some(&(index, reversed)) =
            ctx.next_charged(&mut source, "FCStd profile chain extraction")?
        else {
            break;
        };
        ctx.push_vec(
            &mut profile,
            SketchEntityUse {
                entity: entities[index]
                    .id()
                    .try_clone_for_decode(ctx, "FCStd profile use identity")?,
                reversed,
            },
            "FCStd profile chain extraction",
        )?;
    }
    drop(chain);
    Ok(profile)
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct EndpointLocus {
    entity: usize,
    start: bool,
}

impl cadmpeg_core::decode::cost::DecodeCost for EndpointLocus {
    const FIXED_BYTES: Option<u64> = Some(cadmpeg_core::decode::u64_from_index(
        std::mem::size_of::<usize>() + std::mem::size_of::<bool>(),
    ));
    fn decode_cost(
        &self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        _operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        Ok(cadmpeg_core::decode::u64_from_index(
            std::mem::size_of::<usize>() + std::mem::size_of::<bool>(),
        ))
    }
}

struct IndexedEndpoint {
    locus: EndpointLocus,
    point: Point2,
}

struct EndpointIndex {
    by_scale: BTreeMap<u64, Vec<IndexedEndpoint>>,
}

impl EndpointIndex {
    fn new<'ctx>(
        ctx: &'ctx DecodeContext<'_>,
        profile_entities: &BTreeSet<usize>,
        entities: &[SketchEntity],
    ) -> Result<(Self, ScopedReservation<'ctx>), CodecError> {
        let mut storage = ctx.reserve_scoped(0, "FCStd profile endpoint index")?;
        let by_scale = storage.with_storage(|| {
            let mut by_scale = BTreeMap::<u64, Vec<IndexedEndpoint>>::new();
            let mut source = profile_entities.iter();
            while source.len() != 0 {
                let Some(index) =
                    ctx.next_charged(&mut source, "FCStd profile endpoint extraction")?
                else {
                    break;
                };
                if let Some((start, end)) = endpoints(&entities[*index]) {
                    for (at_start, point) in [(true, start), (false, end)] {
                        let scale = endpoint_scale_bucket(point);
                        ctx.push_btree_group(
                            &mut by_scale,
                            scale,
                            IndexedEndpoint {
                                locus: EndpointLocus {
                                    entity: *index,
                                    start: at_start,
                                },
                                point,
                            },
                            "FCStd profile endpoint buckets",
                            "FCStd profile endpoint index",
                        )?;
                    }
                }
            }
            let mut buckets = by_scale.iter_mut();
            while buckets.len() != 0 {
                let Some((_, bucket)) =
                    ctx.next_charged(&mut buckets, "FCStd profile endpoint bucket sort")?
                else {
                    break;
                };
                ctx.stable_sort_by(
                    bucket,
                    |value| &value.point.u,
                    f64::total_cmp,
                    "FCStd profile index sort",
                )?;
            }
            Ok::<_, CodecError>(by_scale)
        })?;
        Ok((Self { by_scale }, storage))
    }
}

fn endpoint_scale_bucket(point: Point2) -> u64 {
    point.u.abs().max(point.v.abs()).max(1.0).to_bits() >> 52
}

fn endpoint_candidates<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    endpoint: EndpointLocus,
    available: &BTreeSet<usize>,
    explicit_relations: &BTreeMap<EndpointLocus, BTreeSet<EndpointLocus>>,
    entities: &[SketchEntity],
    index: &EndpointIndex,
) -> Result<(Vec<EndpointLocus>, ScopedReservation<'ctx>), CodecError> {
    // Active explicit coincident loci override coordinates. Coordinate matching below is the
    // decoder-owned CADIR boundary, not a producer tolerance.
    let mut storage = ctx.reserve_scoped(0, "FCStd profile candidates")?;
    let matches = storage.with_storage(|| -> Result<Vec<EndpointLocus>, CodecError> {
        if let Some(explicit) = ctx.get_btree_map(
            explicit_relations,
            &endpoint,
            "FCStd explicit profile relation lookup",
        )? {
            let mut matches = Vec::new();
            let mut candidates = explicit.iter();
            while candidates.len() != 0 {
                let Some(candidate) =
                    ctx.next_charged(&mut candidates, "FCStd explicit profile matches")?
                else {
                    break;
                };
                if ctx.contains_btree_set(
                    available,
                    &candidate.entity,
                    "FCStd available explicit profile candidate",
                )? {
                    ctx.push_vec(&mut matches, *candidate, "FCStd profile candidates")?;
                }
            }
            return Ok(matches);
        }
        let Some(point) = endpoint_point(endpoint, entities) else {
            return Ok(Vec::new());
        };
        let mut matches = Vec::new();
        let scale = endpoint_scale_bucket(point);
        for bucket_number in (scale - 1)..=(scale + 1) {
            let Some(bucket) = ctx.get_btree_map(
                &index.by_scale,
                &bucket_number,
                "FCStd profile scale bucket lookup",
            )?
            else {
                continue;
            };
            let bucket_scale = f64::from_bits((bucket_number.max(scale) + 1) << 52).min(f64::MAX);
            let tolerance = SKETCH_ENDPOINT_ROUNDING_ULPS * f64::EPSILON * bucket_scale;
            let first = ctx.partition_point(
                bucket,
                |candidate| Ok(candidate.point.u < point.u - tolerance),
                "FCStd profile index search",
            )?;
            let _stop = ctx.find_map(
                &bucket[first..],
                |candidate| {
                    if candidate.point.u <= point.u + tolerance {
                        if candidate.locus.entity != endpoint.entity
                            && ctx.contains_btree_set(
                                available,
                                &candidate.locus.entity,
                                "FCStd available profile candidate",
                            )?
                            && !ctx.contains_key_btree_map(
                                explicit_relations,
                                &candidate.locus,
                                "FCStd explicit profile candidate lookup",
                            )?
                            && endpoints_match_by_roundoff(point, candidate.point)
                        {
                            ctx.push_vec(
                                &mut matches,
                                candidate.locus,
                                "FCStd profile candidates",
                            )?;
                        }
                        Ok(None)
                    } else {
                        Ok(Some(()))
                    }
                },
                "FCStd profile candidate comparison",
            )?;
        }
        ctx.sort_unstable_by(
            &mut matches,
            |value| value,
            Ord::cmp,
            "FCStd profile candidate order",
        )?;
        Ok(matches)
    })?;
    Ok((matches, storage))
}

fn explicit_endpoint_relations<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    profile_entities: &BTreeSet<usize>,
    entities: &[SketchEntity],
    constraints: &[SketchConstraint],
) -> Result<
    (
        BTreeMap<EndpointLocus, BTreeSet<EndpointLocus>>,
        ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    let mut storage = ctx.reserve_scoped(0, "FCStd explicit profile relations")?;
    let relations = storage.with_storage(|| {
        let mut entity_lookup = None;
        let mut relations = BTreeMap::new();
        let mut source = constraints.iter();
        while source.len() != 0 {
            let Some(constraint) =
                ctx.next_charged(&mut source, "FCStd profile constraint scan")?
            else {
                break;
            };
            if constraint.active == Some(false) {
                continue;
            }
            let SketchConstraintDefinitionInput::CoincidentLoci { loci } =
                constraint.definition.kind()
            else {
                continue;
            };
            let (entity_indices, _entity_index_storage) = match &mut entity_lookup {
                Some(lookup) => lookup,
                slot @ None => slot.insert(ctx.with_scoped_storage(
                    "FCStd profile entity lookup storage",
                    || {
                        ctx.collect_hash_map(
                            entities
                                .iter()
                                .enumerate()
                                .map(|(index, entity)| (entity.id().as_str(), index)),
                            "FCStd profile entity lookup",
                        )
                    },
                )?),
            };
            let mut endpoint_storage = ctx.reserve_scoped(0, "FCStd explicit profile loci")?;
            let mut endpoints = BTreeSet::new();
            endpoint_storage.with_storage(|| {
                let mut loci_source = loci.iter();
                while loci_source.len() != 0 {
                    let Some(locus) =
                        ctx.next_charged(&mut loci_source, "FCStd explicit profile loci")?
                    else {
                        break;
                    };
                    let (entity, start) = match locus {
                        SketchLocus::Start(entity) => (entity, true),
                        SketchLocus::End(entity) => (entity, false),
                        _ => continue,
                    };
                    let Some(index) = ctx
                        .get_hash_map(
                            entity_indices,
                            entity.as_str(),
                            "FCStd profile entity index",
                        )?
                        .copied()
                    else {
                        continue;
                    };
                    if ctx.contains_btree_set(
                        profile_entities,
                        &index,
                        "FCStd eligible profile entity lookup",
                    )? {
                        ctx.insert_btree_set(
                            &mut endpoints,
                            EndpointLocus {
                                entity: index,
                                start,
                            },
                            "FCStd explicit profile loci",
                        )?;
                    }
                }
                Ok::<_, CodecError>(())
            })?;
            let mut sources = endpoints.iter();
            while sources.len() != 0 {
                let Some(first) =
                    ctx.next_charged(&mut sources, "FCStd explicit profile relation sources")?
                else {
                    break;
                };
                let mut targets = endpoints.iter();
                while targets.len() != 0 {
                    let Some(candidate) =
                        ctx.next_charged(&mut targets, "FCStd explicit profile relation targets")?
                    else {
                        break;
                    };
                    if first == candidate {
                        continue;
                    }
                    ctx.insert_btree_group_set(
                        &mut relations,
                        *first,
                        *candidate,
                        "FCStd explicit profile relations",
                        "FCStd explicit profile relations",
                    )?;
                }
            }
            drop(endpoints);
            drop(endpoint_storage);
        }
        Ok::<_, CodecError>(relations)
    })?;
    Ok((relations, storage))
}

fn endpoint_point(endpoint: EndpointLocus, entities: &[SketchEntity]) -> Option<Point2> {
    endpoints(&entities[endpoint.entity])
        .map(|points| if endpoint.start { points.0 } else { points.1 })
}

fn endpoints(entity: &SketchEntity) -> Option<(Point2, Point2)> {
    match *entity.geometry.definition() {
        SketchGeometryDefinition::Line { start, end } => Some((start.get(), end.get())),
        SketchGeometryDefinition::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => Some((
            Point2::new(
                center.u + radius.get() * start_angle.get().cos(),
                center.v + radius.get() * start_angle.get().sin(),
            ),
            Point2::new(
                center.u + radius.get() * end_angle.get().cos(),
                center.v + radius.get() * end_angle.get().sin(),
            ),
        )),
        SketchGeometryDefinition::Ellipse {
            center,
            major_angle,
            radii,
            bounds: Some([start, end]),
        } => {
            let major = Point2::new(major_angle.get().cos(), major_angle.get().sin());
            let minor = Point2::new(-major.v, major.u);
            let point = |parameter: f64| {
                let (along_major, along_minor) = (parameter.cos(), parameter.sin());
                Point2::new(
                    center.u
                        + radii.major().get() * along_major * major.u
                        + radii.minor().get() * along_minor * minor.u,
                    center.v
                        + radii.major().get() * along_major * major.v
                        + radii.minor().get() * along_minor * minor.v,
                )
            };
            Some((point(start.get()), point(end.get())))
        }
        _ => None,
    }
}

const SKETCH_ENDPOINT_ROUNDING_ULPS: f64 = 64.0;

fn endpoints_match_by_roundoff(a: Point2, b: Point2) -> bool {
    let scale =
        a.u.abs()
            .max(a.v.abs())
            .max(b.u.abs())
            .max(b.v.abs())
            .max(1.0);
    (a.u - b.u).hypot(a.v - b.v) <= SKETCH_ENDPOINT_ROUNDING_ULPS * f64::EPSILON * scale
}

#[cfg(test)]
mod tests;
