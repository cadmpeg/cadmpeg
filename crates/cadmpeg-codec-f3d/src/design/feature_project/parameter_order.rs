// SPDX-License-Identifier: Apache-2.0
//! Parameter dependency ordering and cycle detection.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

pub(super) fn normalize_parameter_ordinals(
    ctx: &DecodeContext<'_>,
    parameters: &mut [cadmpeg_ir::features::DesignParameter],
    owners: &HashMap<cadmpeg_ir::features::ParameterId, Option<cadmpeg_ir::features::FeatureId>>,
) -> Result<(), CodecError> {
    let mut storage = ctx.reserve_scoped(0, "f3d parameter ordering storage")?;
    storage.with_storage(|| {
        use cadmpeg_ir::features::FeatureId;

        let mut groups = BTreeMap::<Option<FeatureId>, Vec<usize>>::new();
        for (index, parameter) in ctx
            .admit_iter(&*parameters, "f3d parameter owner scan")?
            .enumerate()
        {
            if let Some(indices) =
                ctx.get_mut_btree_map(&mut groups, &parameter.owner, "f3d parameter owner lookup")?
            {
                ctx.push_vec(indices, index, "f3d parameter owner member")?;
            } else {
                let owner = parameter
                    .owner
                    .as_ref()
                    .map(|id| (id).try_clone_for_decode(ctx, "f3d parameter owner ID"))
                    .transpose()?;
                let mut indices = Vec::new();
                ctx.push_vec(&mut indices, index, "f3d parameter owner member")?;
                // discarded-value: a new owner group has no previous member list.
                let _ =
                    ctx.insert_btree_map(&mut groups, owner, indices, "f3d parameter owner group")?;
            }
        }
        for (owner, indices) in ctx.admit_iter(groups, "f3d parameter owner group scan")? {
            let mut ordinals = Vec::new();
            for index in ctx.admit_iter(&indices, "f3d parameter group member scan")? {
                ctx.push_vec(
                    &mut ordinals,
                    parameters[*index].ordinal,
                    "f3d parameter group ordinal",
                )?;
            }
            ctx.sort_unstable_by(
                &mut ordinals,
                |value| value,
                Ord::cmp,
                "f3d parameter group ordinal sort",
            )?;
            let mut unresolved = BTreeSet::new();
            for index in ctx.admit_iter(indices, "f3d parameter unresolved scan")? {
                // discarded-value: each parameter index occurs once in its owner group.
                let _ =
                    ctx.insert_btree_set(&mut unresolved, index, "f3d parameter unresolved index")?;
            }
            let mut resolved = HashSet::new();
            let mut order = Vec::new();
            while !unresolved.is_empty() {
                let mut ready = Vec::new();
                for index in ctx.admit_iter(&unresolved, "f3d parameter readiness")? {
                    if ctx.all_by(
                        parameters[*index].dependencies.as_slice(),
                        |dependency| {
                            let same_owner = match ctx.get_hash_map(
                                owners,
                                dependency,
                                "f3d parameter dependency owner",
                            )? {
                                Some(dependency_owner) => ctx.equal(
                                    dependency_owner,
                                    &owner,
                                    "f3d parameter dependency owner comparison",
                                )?,
                                None => false,
                            };
                            Ok(!same_owner
                                || ctx.contains_hash_set(
                                    &resolved,
                                    dependency,
                                    "f3d parameter resolved lookup",
                                )?)
                        },
                        "f3d parameter dependency scan",
                    )? {
                        ctx.push_vec(&mut ready, *index, "f3d parameter ready index")?;
                    }
                }
                ctx.stable_sort_by_key(
                    &mut ready[..],
                    |index| (parameters[*index].ordinal, parameters[*index].id.as_str()),
                    Ord::cmp,
                    "sort f3d design feature_project 8",
                )?;
                if ready.is_empty() {
                    // Every blocked parameter has an unresolved dependency, so the
                    // remaining graph contains a cycle. Break its lowest-ordinal
                    // member, then resume ordering before selecting another cycle.
                    let cycles = cyclic_parameter_components(ctx, parameters, &unresolved)?;
                    let mut selected: Option<(usize, usize)> = None;
                    for (component, (first, remaining)) in ctx
                        .admit_iter(&cycles, "f3d parameter cycle selection scan")?
                        .enumerate()
                    {
                        let breaker = ctx.fold(
                            remaining,
                            *first,
                            |best, &index| {
                                Ok(
                                    if ctx.compare(
                                        &(parameters[index].ordinal, parameters[index].id.as_str()),
                                        &(parameters[best].ordinal, parameters[best].id.as_str()),
                                        "f3d parameter cycle breaker comparison",
                                    )? == std::cmp::Ordering::Less
                                    {
                                        index
                                    } else {
                                        best
                                    },
                                )
                            },
                            "f3d parameter cycle breaker scan",
                        )?;
                        if match selected {
                            None => true,
                            Some((_, best)) => {
                                ctx.compare(
                                    &(parameters[breaker].ordinal, parameters[breaker].id.as_str()),
                                    &(parameters[best].ordinal, parameters[best].id.as_str()),
                                    "f3d parameter cycle selection comparison",
                                )? == std::cmp::Ordering::Less
                            }
                        } {
                            selected = Some((component, breaker));
                        }
                    }
                    if let Some((component, breaker)) = selected {
                        let (first, remaining) = &cycles[component];
                        let (before, selected) = parameters.split_at_mut(breaker);
                        let [parameter, after @ ..] = selected else {
                            return Err(CodecError::malformed(
                                "parameter cycle breaker is outside its group",
                            ));
                        };
                        let mut members = HashSet::new();
                        let mut indices = std::iter::once(*first).chain(remaining.iter().copied());
                        while let Some(index) =
                            ctx.next_charged(&mut indices, "f3d parameter cycle member scan")?
                        {
                            let id = match index.cmp(&breaker) {
                                std::cmp::Ordering::Less => &before[index].id,
                                std::cmp::Ordering::Equal => &parameter.id,
                                std::cmp::Ordering::Greater => &after[index - breaker - 1].id,
                            };
                            // discarded-value: component indices are distinct.
                            let _ = ctx.insert_hash_set(
                                &mut members,
                                id,
                                "f3d parameter cycle member",
                            )?;
                        }
                        parameter
                            .dependencies
                            .retain(|dependency| !members.contains(dependency));
                    }
                    continue;
                }
                for index in ctx.admit_iter(ready, "f3d parameter ready scan")? {
                    ctx.remove_btree_set(
                        &mut unresolved,
                        &index,
                        "f3d parameter resolved removal",
                    )?;
                    if let Some((id, _)) = ctx.get_key_value_hash_map(
                        owners,
                        &parameters[index].id,
                        "f3d parameter resolved owner lookup",
                    )? {
                        // discarded-value: a parameter is resolved once.
                        let _ =
                            ctx.insert_hash_set(&mut resolved, id, "f3d parameter resolved index")?;
                    }
                    ctx.push_vec(&mut order, index, "f3d parameter sorted order")?;
                }
            }
            for (index, ordinal) in ctx
                .admit_iter(order, "f3d parameter ordinal assignment")?
                .zip(ordinals)
            {
                parameters[index].ordinal = ordinal;
            }
        }
        Ok(())
    })
}

/// Strongly connected components of the unresolved parameter graph that contain a cycle.
/// Each component retains one member separately from its remaining members.
pub(super) fn cyclic_parameter_components(
    ctx: &DecodeContext<'_>,
    parameters: &[cadmpeg_ir::features::DesignParameter],
    unresolved: &BTreeSet<usize>,
) -> Result<Vec<(usize, Vec<usize>)>, CodecError> {
    enum Visit {
        Enter(usize),
        Leave(usize),
    }

    let mut scratch = ctx.reserve_scoped(0, "f3d parameter cycle scratch")?;
    let mut indices = Vec::new();
    for index in ctx.admit_iter(unresolved, "f3d parameter cycle unresolved scan")? {
        ctx.push_scoped_vec(
            &mut scratch,
            &mut indices,
            *index,
            "f3d parameter cycle indices",
        )?;
    }
    let mut local_by_id = HashMap::new();
    for (local, index) in ctx
        .admit_iter(&indices, "f3d parameter cycle index scan")?
        .enumerate()
    {
        // discarded-value: distinct parameter IDs retain one local index.
        let _ = scratch.with_storage(|| {
            ctx.insert_hash_map(
                &mut local_by_id,
                &parameters[*index].id,
                local,
                "f3d parameter cycle local index",
            )
        })?;
    }
    let mut edges = Vec::new();
    for index in ctx.admit_iter(&indices, "f3d parameter group member scan")? {
        let mut dependencies = Vec::new();
        for dependency in ctx.admit_iter(
            parameters[*index].dependencies.as_slice(),
            "f3d parameter cycle dependency scan",
        )? {
            if let Some(local) = ctx.get_hash_map(
                &local_by_id,
                dependency,
                "f3d parameter cycle dependency lookup",
            )? {
                ctx.push_scoped_vec(
                    &mut scratch,
                    &mut dependencies,
                    *local,
                    "f3d parameter cycle dependency edge",
                )?;
            }
        }
        ctx.push_scoped_vec(
            &mut scratch,
            &mut edges,
            dependencies,
            "f3d parameter cycle edge list",
        )?;
    }
    let mut incoming = Vec::new();
    for _ in ctx.admit_iter(&indices, "f3d parameter cycle incoming initialization")? {
        ctx.push_scoped_vec(
            &mut scratch,
            &mut incoming,
            Vec::new(),
            "f3d parameter cycle incoming list",
        )?;
    }
    for (source, dependencies) in ctx
        .admit_iter(&edges, "f3d parameter cycle edge scan")?
        .enumerate()
    {
        for &target in ctx.admit_iter(dependencies, "f3d parameter cycle target scan")? {
            ctx.push_scoped_vec(
                &mut scratch,
                &mut incoming[target],
                source,
                "f3d parameter cycle incoming edge",
            )?;
        }
    }

    // Iterative graph walks keep long source dependency chains off the call stack.
    let mut visited = HashSet::new();
    let mut finished = Vec::new();
    for root in ctx.admit_iter(0..indices.len(), "f3d parameter cycle root scan")? {
        if ctx.contains_hash_set(&visited, &root, "f3d parameter cycle root membership")? {
            continue;
        }
        let mut pending = Vec::new();
        ctx.push_scoped_vec(
            &mut scratch,
            &mut pending,
            Visit::Enter(root),
            "f3d parameter cycle pending visit",
        )?;
        while let Some(visit) = ctx.next_charged(
            &mut std::iter::from_fn(|| pending.pop()),
            "f3d parameter cycle visit",
        )? {
            match visit {
                Visit::Enter(node) => {
                    if !scratch.with_storage(|| {
                        ctx.insert_hash_set(&mut visited, node, "f3d parameter cycle visited node")
                    })? {
                        continue;
                    }
                    ctx.push_scoped_vec(
                        &mut scratch,
                        &mut pending,
                        Visit::Leave(node),
                        "f3d parameter cycle pending visit",
                    )?;
                    for dependency in
                        ctx.admit_iter(&edges[node], "f3d parameter cycle outgoing scan")?
                    {
                        ctx.push_scoped_vec(
                            &mut scratch,
                            &mut pending,
                            Visit::Enter(*dependency),
                            "f3d parameter cycle pending visit",
                        )?;
                    }
                }
                Visit::Leave(node) => {
                    ctx.push_scoped_vec(
                        &mut scratch,
                        &mut finished,
                        node,
                        "f3d parameter cycle finish order",
                    )?;
                }
            }
        }
    }

    let mut assigned = HashSet::new();
    let mut cycles = Vec::new();
    for root in ctx
        .admit_iter(finished, "f3d parameter cycle finished scan")?
        .rev()
    {
        if !scratch.with_storage(|| {
            ctx.insert_hash_set(&mut assigned, root, "f3d parameter cycle assigned node")
        })? {
            continue;
        }
        let mut remaining_members = Vec::new();
        let mut pending = Vec::new();
        ctx.push_scoped_vec(
            &mut scratch,
            &mut pending,
            root,
            "f3d parameter cycle reverse pending",
        )?;
        while let Some(node) = ctx.next_charged(
            &mut std::iter::from_fn(|| pending.pop()),
            "f3d parameter cycle reverse visit",
        )? {
            for &source in ctx.admit_iter(&incoming[node], "f3d parameter cycle incoming scan")? {
                if scratch.with_storage(|| {
                    ctx.insert_hash_set(&mut assigned, source, "f3d parameter cycle assigned node")
                })? {
                    ctx.push_vec(
                        &mut remaining_members,
                        indices[source],
                        "f3d parameter cycle component member",
                    )?;
                    ctx.push_scoped_vec(
                        &mut scratch,
                        &mut pending,
                        source,
                        "f3d parameter cycle reverse pending",
                    )?;
                }
            }
        }
        if !remaining_members.is_empty()
            || !edges[root].is_empty()
                && ctx.any_by(
                    &edges[root],
                    |node| Ok(*node == root),
                    "visit F3D parameter cycle self-edge membership",
                )?
        {
            ctx.push_vec(
                &mut cycles,
                (indices[root], remaining_members),
                "f3d parameter cycle component",
            )?;
        }
    }
    Ok(cycles)
}

#[cfg(test)]
mod tests;
