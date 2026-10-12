// SPDX-License-Identifier: Apache-2.0
//! Resolve design dependencies and preserve source ordinals during ordering.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{DesignParameter, DistinctMembers, FeatureId, ParameterId};

use crate::native::{ObjectRecord, PropertyRecord};

use super::{feature_id, is_body, is_design_object};

pub(super) struct FeatureOrdering<'ctx, 'a> {
    pub(super) ordinals: HashMap<&'a str, u64>,
    pub(super) cycle_affected: BTreeSet<String>,
    pub(super) storage: ScopedReservation<'ctx>,
    pub(super) cycle_storage: ScopedReservation<'ctx>,
}

pub(super) fn feature_ordinals<'ctx, 'a>(
    ctx: &'ctx DecodeContext<'_>,
    objects: &'a [ObjectRecord],
    properties_by_owner: &BTreeMap<&'a str, Vec<&'a PropertyRecord>>,
    parent_by_member: &HashMap<&'a str, FeatureId>,
) -> Result<FeatureOrdering<'ctx, 'a>, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "fcstd design ordinal working storage")?;
    let mut design_objects = Vec::new();
    let mut source = objects.iter();
    while source.len() != 0 {
        let Some(object) = ctx.next_charged(&mut source, "fcstd design ordered objects")? else {
            break;
        };
        if is_design_object(&object.type_name) {
            storage.with_storage(|| {
                ctx.push_vec(&mut design_objects, object, "fcstd design ordered objects")
            })?;
        }
    }
    let count = design_objects.len();
    let mut object_by_id = HashMap::new();
    let mut object_by_name = HashMap::new();
    let mut object_by_feature = HashMap::new();
    let mut source_ordinals =
        storage.with_storage(|| ctx.vector_storage(count, "fcstd design source ordinals"))?;
    let mut source = design_objects.iter();
    while source.len() != 0 {
        let Some(object) = ctx.next_charged(&mut source, "fcstd design source indexes")? else {
            break;
        };
        storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut object_by_id,
                object.id().as_str(),
                *object,
                "fcstd design id index",
            )?;
            ctx.insert_hash_map(
                &mut object_by_name,
                object.name().as_str(),
                *object,
                "fcstd design name index",
            )?;
            ctx.insert_hash_map(
                &mut object_by_feature,
                feature_id(ctx, object)?,
                object.id().as_str(),
                "fcstd design feature index",
            )?;
            ctx.push_vec(
                &mut source_ordinals,
                cadmpeg_core::decode::u64_from_index(object.order),
                "fcstd design source ordinals",
            )
        })?;
    }
    ctx.sort_unstable_by(
        &mut source_ordinals,
        |value| value,
        Ord::cmp,
        "fcstd design source ordinals sort",
    )?;
    let mut emitted = BTreeSet::new();
    let mut ordinal_storage = ctx.reserve_scoped(0, "fcstd design ordinal result storage")?;
    let mut ordinals = HashMap::new();
    let mut cycle_storage = ctx.reserve_scoped(0, "fcstd design cycle object storage")?;
    let mut cycle_affected = BTreeSet::new();

    let dependencies = storage.with_storage(|| {
        let mut dependencies =
            ctx.vector_storage(design_objects.len(), "fcstd design dependency lists")?;
        let mut source = design_objects.iter();
        while source.len() != 0 {
            let Some(object) = ctx.next_charged(&mut source, "fcstd design dependency sources")?
            else {
                break;
            };
            let mut required = BTreeSet::new();
            if let Some(parent) = ctx.get_hash_map(
                parent_by_member,
                object.id().as_str(),
                "fcstd design membership parent lookup",
            )? {
                if let Some(parent) = ctx.get_hash_map(
                    &object_by_feature,
                    parent,
                    "fcstd design feature parent lookup",
                )? {
                    ctx.insert_btree_set(&mut required, *parent, "fcstd design required parent")?;
                }
            }
            if !is_body(&object.type_name) {
                let mut source = object.dependencies.iter();
                while source.len() != 0 {
                    let Some(dependency) =
                        ctx.next_charged(&mut source, "fcstd declared design dependencies")?
                    else {
                        break;
                    };
                    if ctx.contains_key_hash_map(
                        &object_by_id,
                        dependency.as_str(),
                        "fcstd declared dependency lookup",
                    )? {
                        ctx.insert_btree_set(
                            &mut required,
                            dependency.as_str(),
                            "fcstd design required declaration",
                        )?;
                    }
                }
            }
            if let Some(properties) = ctx.get_btree_map(
                properties_by_owner,
                object.id().as_str(),
                "fcstd design dependency properties",
            )? {
                let mut source = properties.iter();
                while source.len() != 0 {
                    let Some(property) =
                        ctx.next_charged(&mut source, "fcstd design dependency properties")?
                    else {
                        break;
                    };
                    let mut source = property.values().iter();
                    while source.len() != 0 {
                        let Some(value) =
                            ctx.next_charged(&mut source, "fcstd design expression values")?
                        else {
                            break;
                        };
                        if let Some(expression) = ctx.get_btree_map(
                            &value.attributes,
                            "expression",
                            "fcstd design expression attribute",
                        )? {
                            expression_identifiers_until(
                                ctx,
                                expression,
                                "fcstd design expression identifiers",
                                |identifier| {
                                    if let Some(index) = ctx.position_by(
                                        identifier.as_bytes(),
                                        |byte| Ok(*byte == b'.'),
                                        "fcstd design expression owner separator",
                                    )? {
                                        let owner = &identifier[..index];
                                        if let Some(dependency) = ctx.get_hash_map(
                                            &object_by_name,
                                            owner,
                                            "fcstd design expression owner lookup",
                                        )? {
                                            if !ctx.equal(
                                                dependency.id(),
                                                object.id(),
                                                "fcstd design expression self identity",
                                            )? {
                                                ctx.insert_btree_set(
                                                    &mut required,
                                                    dependency.id().as_str(),
                                                    "fcstd design required expression",
                                                )?;
                                            }
                                        }
                                    }
                                    Ok(true)
                                },
                            )?;
                        }
                    }
                    if !is_body(&object.type_name) {
                        let mut source = property.links().iter();
                        while source.len() != 0 {
                            let Some(link) =
                                ctx.next_charged(&mut source, "fcstd design property links")?
                            else {
                                break;
                            };
                            let Some(dependency_id) =
                                link.as_ref().and_then(crate::native::LinkTarget::object)
                            else {
                                continue;
                            };
                            let Some(dependency) = ctx.get_hash_map(
                                &object_by_id,
                                dependency_id,
                                "fcstd design linked dependency lookup",
                            )?
                            else {
                                continue;
                            };
                            if matches!(
                                property.name.as_str(),
                                "Base"
                                    | "BaseFeature"
                                    | "Originals"
                                    | "Path"
                                    | "Profile"
                                    | "Sketch"
                                    | "Sections"
                                    | "Source"
                                    | "Spine"
                            ) || dependency.order < object.order
                            {
                                ctx.insert_btree_set(
                                    &mut required,
                                    dependency_id,
                                    "fcstd design required link",
                                )?;
                            }
                        }
                    }
                }
            }
            ctx.push_vec(&mut dependencies, required, "fcstd design dependency lists")?;
        }
        Ok::<_, CodecError>(dependencies)
    })?;
    let mut source = source_ordinals.iter();
    while source.len() != 0 {
        let Some(ordinal) =
            ctx.next_charged(&mut source, "fcstd design dependency ordering passes")?
        else {
            break;
        };
        let mut next: Option<&ObjectRecord> = None;
        let mut source = design_objects.iter();
        while source.len() != 0 {
            let index = design_objects.len() - source.len();
            let Some(object) = ctx.next_charged(&mut source, "fcstd design dependency ordering")?
            else {
                break;
            };
            if ctx.contains_btree_set(
                &emitted,
                object.id().as_str(),
                "fcstd design emitted object lookup",
            )? {
                continue;
            }
            if !ctx.all_by(
                &dependencies[index],
                |dependency| {
                    ctx.contains_btree_set(
                        &emitted,
                        *dependency,
                        "fcstd design emitted dependency lookup",
                    )
                },
                "fcstd design dependency readiness",
            )? {
                continue;
            }
            if next.is_none_or(|current| object.order < current.order) {
                next = Some(object);
            }
        }
        let next = match next {
            Some(next) => next,
            None => {
                let mut next: Option<&ObjectRecord> = None;
                let mut source = design_objects.iter();
                while source.len() != 0 {
                    let Some(object) =
                        ctx.next_charged(&mut source, "fcstd design cycle objects")?
                    else {
                        break;
                    };
                    if ctx.contains_btree_set(
                        &emitted,
                        object.id().as_str(),
                        "fcstd design emitted cycle object lookup",
                    )? {
                        continue;
                    }
                    if !ctx.contains_btree_set(
                        &cycle_affected,
                        object.id().as_str(),
                        "fcstd design cycle affected object lookup",
                    )? {
                        cycle_storage.with_storage(|| {
                            ctx.insert_btree_set(
                                &mut cycle_affected,
                                ctx.copy_retained_text(object.id(), "fcstd design cycle object")?,
                                "fcstd design cycle affected objects",
                            )
                        })?;
                    }
                    if next.is_none_or(|current| object.order < current.order) {
                        next = Some(object);
                    }
                }
                next.ok_or_else(|| {
                    CodecError::malformed(
                        "design object ordering lost an un-emitted object while resolving a cycle",
                    )
                })?
            }
        };
        storage.with_storage(|| {
            ctx.insert_btree_set(
                &mut emitted,
                next.id().as_str(),
                "fcstd design emitted objects",
            )
        })?;
        ordinal_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut ordinals,
                next.id().as_str(),
                *ordinal,
                "fcstd design ordinals",
            )
        })?;
    }
    Ok(FeatureOrdering {
        ordinals,
        cycle_affected,
        storage: ordinal_storage,
        cycle_storage,
    })
}

pub(super) fn bind_parameter_dependencies<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    parameters: &mut Vec<DesignParameter>,
    objects: &[ObjectRecord],
    cycle_affected_features: &BTreeSet<FeatureId>,
) -> Result<(BTreeSet<FeatureId>, ScopedReservation<'ctx>), CodecError> {
    let mut dependency_storage =
        ctx.reserve_scoped(0, "fcstd parameter dependency result storage")?;
    if parameters.is_empty() {
        return Ok((BTreeSet::new(), dependency_storage));
    }
    let mut needs_local = false;
    let mut needs_qualified = false;
    let mut consumers = parameters.iter();
    while consumers.len() != 0 && !(needs_local && needs_qualified) {
        let Some(parameter) = ctx.next_charged(&mut consumers, "fcstd dependency consumers")?
        else {
            break;
        };
        if let Some(owner) = parameter.owner.as_ref() {
            if ctx.contains_btree_set(
                cycle_affected_features,
                owner,
                "fcstd dependency consumer cycle",
            )? {
                continue;
            }
        }
        expression_identifiers_until(
            ctx,
            &parameter.expression,
            "fcstd dependency consumer identifiers",
            |identifier| {
                needs_local |= parameter.owner.is_some();
                if !needs_qualified {
                    needs_qualified = ctx
                        .position_by(
                            identifier.bytes(),
                            |byte| Ok(byte == b'.'),
                            "fcstd qualified dependency demand",
                        )?
                        .is_some();
                }
                Ok(!(needs_local && needs_qualified))
            },
        )?;
    }
    let (candidate_storage, dependencies);
    (dependencies, candidate_storage) =
        ctx.with_scoped_storage("fcstd parameter dependency candidates", || {
            let mut qualified_key_storage =
                ctx.reserve_scoped(0, "fcstd qualified candidate key storage")?;
            let mut object_names = None;
            let mut local = HashMap::<(&FeatureId, &str), Option<&ParameterId>>::new();
            let mut qualified = HashMap::<String, Option<&ParameterId>>::new();
            let mut source = parameters.iter();
            while (needs_local || needs_qualified) && source.len() != 0 {
                let Some(parameter) =
                    ctx.next_charged(&mut source, "fcstd dependency parameters")?
                else {
                    break;
                };
                let Some(owner) = parameter.owner.as_ref() else {
                    continue;
                };
                let object_names = if needs_qualified {
                    Some(match &mut object_names {
                        Some(names) => names,
                        slot @ None => {
                            let mut names = HashMap::new();
                            let mut source = objects.iter();
                            while source.len() != 0 {
                                let Some(object) = ctx.next_charged(
                                    &mut source,
                                    "fcstd parameter dependency objects",
                                )?
                                else {
                                    break;
                                };
                                ctx.insert_hash_map(
                                    &mut names,
                                    feature_id(ctx, object)?,
                                    object.name().as_str(),
                                    "fcstd parameter dependency object names",
                                )?;
                            }
                            slot.insert(names)
                        }
                    })
                } else {
                    None
                };
                let source_name = match ctx.get_btree_map(
                    &parameter.properties,
                    "source_name",
                    "fcstd parameter source name",
                )? {
                    Some(name)
                        if !ctx.equal(name, &parameter.name, "fcstd parameter source name")? =>
                    {
                        Some(name.as_str())
                    }
                    _ => None,
                };
                for name in [Some(parameter.name.as_str()), source_name]
                    .into_iter()
                    .flatten()
                {
                    if needs_local {
                        let key = (owner, name);
                        if let Some(candidate) =
                            ctx.get_mut_hash_map(&mut local, &key, "fcstd unique local candidates")?
                        {
                            *candidate = None;
                        } else {
                            ctx.insert_hash_map(
                                &mut local,
                                key,
                                Some(&parameter.id),
                                "fcstd unique local candidates",
                            )?;
                        }
                    }
                    if let Some(object_names) = object_names.as_ref() {
                        if let Some(object) = ctx.get_hash_map(
                            object_names,
                            owner,
                            "fcstd qualified candidate owner",
                        )? {
                            let (mut key_storage, key);
                            (key, key_storage) = ctx.format_scoped(
                                format_args!("{object}.{name}"),
                                "fcstd qualified candidate name",
                            )?;
                            if let Some(candidate) = ctx.get_mut_hash_map(
                                &mut qualified,
                                key.as_str(),
                                "fcstd unique qualified candidates",
                            )? {
                                *candidate = None;
                            } else {
                                qualified_key_storage.absorb(&mut key_storage)?;
                                ctx.insert_hash_map(
                                    &mut qualified,
                                    key,
                                    Some(&parameter.id),
                                    "fcstd unique qualified candidates",
                                )?;
                            }
                        }
                    }
                }
            }
            dependency_storage.with_storage(|| {
                let mut dependencies =
                    ctx.vector_storage(parameters.len(), "fcstd parameter dependency results")?;
                let mut source = parameters.iter();
                while source.len() != 0 {
                    let Some(parameter) =
                        ctx.next_charged(&mut source, "fcstd parameter dependency scan")?
                    else {
                        break;
                    };
                    let owner_cycle = match parameter.owner.as_ref() {
                        Some(owner) => ctx.contains_btree_set(
                            cycle_affected_features,
                            owner,
                            "fcstd parameter dependency cycle owner",
                        )?,
                        None => false,
                    };
                    if owner_cycle {
                        ctx.push_vec(
                            &mut dependencies,
                            None,
                            "fcstd parameter dependency results",
                        )?;
                        continue;
                    }
                    let mut found = BTreeSet::new();
                    expression_identifiers_until(
                        ctx,
                        &parameter.expression,
                        "fcstd parameter expression identifiers",
                        |identifier| {
                            let qualified_dependency = ctx
                                .get_hash_map(
                                    &qualified,
                                    identifier,
                                    "fcstd qualified dependency lookup",
                                )?
                                .and_then(Option::as_ref)
                                .copied();
                            let dependency = if qualified_dependency.is_some() {
                                qualified_dependency
                            } else if let Some(owner) = parameter.owner.as_ref() {
                                let key = (owner, identifier);
                                ctx.get_hash_map(&local, &key, "fcstd local dependency lookup")?
                                    .and_then(Option::as_ref)
                                    .copied()
                            } else {
                                None
                            };
                            if let Some(dependency) = dependency {
                                if !ctx.equal(
                                    dependency,
                                    &parameter.id,
                                    "fcstd parameter self dependency check",
                                )? && !ctx.contains_btree_set(
                                    &found,
                                    dependency,
                                    "fcstd parameter dependency duplicate lookup",
                                )? {
                                    ctx.insert_btree_set(
                                        &mut found,
                                        dependency.try_clone_for_decode(
                                            ctx,
                                            "fcstd scratch dependency identity",
                                        )?,
                                        "fcstd parameter dependencies",
                                    )?;
                                }
                            }
                            Ok(true)
                        },
                    )?;
                    ctx.push_vec(
                        &mut dependencies,
                        Some(found),
                        "fcstd parameter dependency results",
                    )?;
                }
                Ok::<_, CodecError>(dependencies)
            })
        })?;
    drop(candidate_storage);
    let mut source = 0..parameters.len();
    while !source.is_empty() {
        let Some(index) =
            ctx.next_charged(&mut source, "fcstd parameter dependency materialization")?
        else {
            break;
        };
        let parameter = &mut parameters[index];
        parameter.dependencies = match &dependencies[index] {
            None => DistinctMembers::default(),
            Some(dependencies) => {
                let mut members =
                    ctx.vector_storage(dependencies.len(), "fcstd parameter dependency members")?;
                let mut source = dependencies.iter();
                while source.len() != 0 {
                    let Some(dependency) =
                        ctx.next_charged(&mut source, "fcstd parameter dependency members")?
                    else {
                        break;
                    };
                    ctx.push_vec(
                        &mut members,
                        dependency
                            .try_clone_for_decode(ctx, "fcstd parameter dependency identity")?,
                        "fcstd parameter dependency members",
                    )?;
                }
                DistinctMembers::try_from(members, ctx).map_err(CodecError::from)?
            }
        };
    }
    drop(dependencies);
    drop(dependency_storage);

    let mut owner_ordinal_storage =
        ctx.reserve_scoped(0, "fcstd owner ordinal grouping storage")?;
    let mut owner_ordinals = BTreeMap::<Option<FeatureId>, Vec<u32>>::new();
    let mut source = parameters.iter();
    while source.len() != 0 {
        let Some(parameter) = ctx.next_charged(&mut source, "fcstd ordinal owner groups")? else {
            break;
        };
        if let Some(ordinals) = ctx.get_mut_btree_map(
            &mut owner_ordinals,
            &parameter.owner,
            "fcstd ordinal owner groups",
        )? {
            owner_ordinal_storage.with_storage(|| {
                ctx.push_vec(ordinals, parameter.ordinal, "fcstd owner ordinals")
            })?;
        } else {
            owner_ordinal_storage.with_storage(|| {
                let owner = parameter
                    .owner
                    .as_ref()
                    .map(|owner| owner.try_clone_for_decode(ctx, "fcstd ordinal owner identity"))
                    .transpose()?;
                ctx.push_btree_group(
                    &mut owner_ordinals,
                    owner,
                    parameter.ordinal,
                    "fcstd ordinal owner groups",
                    "fcstd owner ordinals",
                )
            })?;
        }
    }
    let mut source = owner_ordinals.iter_mut();
    while source.len() != 0 {
        let Some((_, ordinals)) = ctx.next_charged(&mut source, "fcstd owner ordinal sorting")?
        else {
            break;
        };
        ctx.stable_sort_by(
            ordinals,
            |value| value,
            Ord::cmp,
            "fcstd owner ordinals sort",
        )?;
    }
    let (parameter_cycle_storage, parameter_cycle_features);
    (parameter_cycle_features, parameter_cycle_storage) =
        order_parameters_by_dependencies(ctx, parameters)?;
    let mut source = 0..parameters.len();
    while !source.is_empty() {
        let Some(index) =
            ctx.next_charged(&mut source, "fcstd parameter dependency cycle clearing")?
        else {
            break;
        };
        let has_cycle = match parameters[index].owner.as_ref() {
            Some(owner) => ctx.contains_btree_set(
                &parameter_cycle_features,
                owner,
                "fcstd parameter dependency cycle check",
            )?,
            None => false,
        };
        if has_cycle {
            // The native property record retains the expression. A neutral
            // parameter edge would create an invented evaluation order for
            // a history that FreeCAD itself could not topologically sort.
            parameters[index].dependencies.clear();
        }
    }
    let mut next_ordinal_storage = ctx.reserve_scoped(0, "fcstd next ordinal storage")?;
    let mut next_ordinal = HashMap::<Option<&FeatureId>, usize>::new();
    let mut source = parameters.iter_mut();
    while source.len() != 0 {
        let Some(parameter) =
            ctx.next_charged(&mut source, "fcstd parameter ordinal assignment")?
        else {
            break;
        };
        next_ordinal_storage.with_storage(|| {
            let owner = parameter.owner.as_ref();
            let index = ctx
                .get_hash_map(&next_ordinal, &owner, "fcstd next owner ordinal")?
                .copied()
                .unwrap_or(0);
            let owner_ordinals = ctx
                .get_btree_map(
                    &owner_ordinals,
                    &parameter.owner,
                    "fcstd owner ordinal values",
                )?
                .ok_or_else(|| {
                    CodecError::malformed("parameter owner lost its source ordinal list")
                })?;
            parameter.ordinal = owner_ordinals.get(index).copied().ok_or_else(|| {
                CodecError::malformed("parameter source ordinal list ended early")
            })?;
            let next_index = index.checked_add(1).ok_or_else(|| {
                ctx.refuse_codec_limit("fcstd next owner ordinal", u64::MAX, u64::MAX)
            })?;
            ctx.insert_hash_map(
                &mut next_ordinal,
                owner,
                next_index,
                "fcstd next ordinal owners",
            )
        })?;
    }
    Ok((parameter_cycle_features, parameter_cycle_storage))
}

pub(super) fn order_parameters_by_dependencies<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    parameters: &mut Vec<DesignParameter>,
) -> Result<(BTreeSet<FeatureId>, ScopedReservation<'ctx>), CodecError> {
    let mut known_storage = ctx.reserve_scoped(0, "fcstd known parameter storage")?;
    let mut known = BTreeSet::new();
    let mut source = parameters.iter();
    while source.len() != 0 {
        let Some(parameter) = ctx.next_charged(&mut source, "fcstd known parameter scan")? else {
            break;
        };
        if !ctx.contains_btree_set(&known, &parameter.id, "fcstd known parameter lookup")? {
            known_storage.with_storage(|| {
                ctx.insert_btree_set(
                    &mut known,
                    parameter
                        .id
                        .try_clone_for_decode(ctx, "fcstd known parameter identity")?,
                    "fcstd known parameter identities",
                )
            })?;
        }
    }
    let mut remaining = std::mem::take(parameters);
    let mut emitted_storage = ctx.reserve_scoped(0, "fcstd emitted parameter storage")?;
    let mut emitted = BTreeSet::new();
    let mut cycle_storage = ctx.reserve_scoped(0, "fcstd parameter cycle feature storage")?;
    let mut cycle_features = BTreeSet::new();
    while !remaining.is_empty() {
        let Some(index) = ctx.position_by(
            &remaining,
            |parameter| {
                ctx.all_by(
                    &parameter.dependencies,
                    |dependency| {
                        Ok(!ctx.contains_btree_set(
                            &known,
                            dependency,
                            "fcstd known dependency lookup",
                        )? || ctx.contains_btree_set(
                            &emitted,
                            dependency,
                            "fcstd emitted dependency lookup",
                        )?)
                    },
                    "fcstd parameter dependency members",
                )
            },
            "fcstd parameter dependency ordering",
        )?
        else {
            let mut source = remaining.iter();
            while source.len() != 0 {
                let Some(parameter) =
                    ctx.next_charged(&mut source, "fcstd parameter cycle owners")?
                else {
                    break;
                };
                let Some(owner) = parameter.owner.as_ref() else {
                    continue;
                };
                if !ctx.contains_btree_set(
                    &cycle_features,
                    owner,
                    "fcstd parameter cycle owner lookup",
                )? {
                    cycle_storage.with_storage(|| {
                        ctx.insert_btree_set(
                            &mut cycle_features,
                            owner.try_clone_for_decode(
                                ctx,
                                "fcstd parameter cycle owner identity",
                            )?,
                            "fcstd parameter cycle owners",
                        )
                    })?;
                }
            }
            ctx.append_vec(parameters, &mut remaining, "fcstd reordered parameters")?;
            break;
        };
        if remaining.len() - index > 1 {
            ctx.rotate_left(
                &mut remaining[index..],
                1,
                "fcstd parameter dependency extraction",
            )?;
        }
        let parameter = remaining
            .pop()
            .ok_or_else(|| CodecError::malformed("ready parameter disappeared"))?;
        emitted_storage.with_storage(|| {
            ctx.insert_btree_set(
                &mut emitted,
                parameter
                    .id
                    .try_clone_for_decode(ctx, "fcstd emitted parameter identity")?,
                "fcstd emitted parameter identities",
            )
        })?;
        ctx.push_vec(parameters, parameter, "fcstd reordered parameters")?;
    }
    Ok((cycle_features, cycle_storage))
}

fn expression_identifiers_until(
    ctx: &DecodeContext<'_>,
    expression: &str,
    operation: &'static str,
    mut visit: impl FnMut(&str) -> Result<bool, CodecError>,
) -> Result<bool, CodecError> {
    let mut identifier_start = None;
    let mut offset = 0_usize;
    let mut characters = expression.chars();
    while !characters.as_str().is_empty() {
        let Some(character) = ctx.next_charged(&mut characters, operation)? else {
            break;
        };
        if character.is_ascii_alphanumeric() || matches!(character, '_' | '.') {
            identifier_start.get_or_insert(offset);
        } else if let Some(start) = identifier_start.take() {
            if !visit(&expression[start..offset])? {
                return Ok(false);
            }
        }
        offset += character.len_utf8();
    }
    if let Some(start) = identifier_start {
        return visit(&expression[start..]);
    }
    Ok(true)
}
