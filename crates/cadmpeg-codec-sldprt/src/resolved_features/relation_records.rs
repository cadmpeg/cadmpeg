//! Relation instance records and scalar roles.

use super::scalars::feature_object_name;
use super::SKETCH_POINT_TOLERANCE;
use crate::classification::{native_object_class, NativeClassKind};
use crate::history::classify::is_history_metadata_record;
use crate::layout::feature_input_shifted_scalar_trailer as shifted_trailer;
use crate::records::operand_tag::NativeOperandTag;
use crate::records::{
    FeatureInputClass, FeatureInputLane, FeatureInputName, FeatureInputOperand,
    FeatureInputOperandKind, FeatureInputRelationFamily, FeatureInputRelationInstance,
    FeatureInputScalar, FeatureInputScalarRole,
};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use std::collections::{HashMap, HashSet};

fn scalar_name_value<'a>(
    ctx: &DecodeContext<'_>,
    scalar: &FeatureInputScalar,
    names: &'a [FeatureInputName],
) -> Result<Option<&'a str>, CodecError> {
    for name in ctx.admit_iter(names, "find SLDPRT relation scalar name")? {
        if ctx.equal(
            name.id.as_str(),
            scalar.name.as_str(),
            "compare SLDPRT relation scalar names",
        )? {
            return Ok(Some(name.value.as_str()));
        }
    }
    Ok(None)
}

fn same_scalar_name(
    ctx: &DecodeContext<'_>,
    first: &FeatureInputScalar,
    second: &FeatureInputScalar,
    names: &[FeatureInputName],
) -> Result<bool, CodecError> {
    let Some(value) = scalar_name_value(ctx, first, names)? else {
        return Ok(false);
    };
    let Some(candidate) = scalar_name_value(ctx, second, names)? else {
        return Ok(false);
    };
    ctx.equal(value, candidate, "compare SLDPRT relation scalar names")
}

/// The offset interval each feature owns, in start order.
///
/// An interval's end is the next feature's start. The last feature has no
/// next start, so its end is `None`: the interval is open, and every offset at
/// or after its start is inside it.
pub(super) fn feature_intervals(
    ctx: &DecodeContext<'_>,
    histories: &[crate::records::FeatureHistory],
    lane: &FeatureInputLane,
) -> Result<Vec<(u64, Option<u64>, String)>, CodecError> {
    let mut starts_storage = ctx.reserve_scoped(0, "SLDPRT feature interval starts")?;
    let mut starts = Vec::<(u64, &str)>::new();
    for history in ctx.admit_iter(histories, "scan SLDPRT feature interval histories")? {
        for feature in ctx.admit_iter(&history.features, "scan SLDPRT feature intervals")? {
            if is_history_metadata_record(ctx, feature, &history.features)? {
                continue;
            }
            if let Some(name) = feature_object_name(feature, lane) {
                starts_storage.with_storage(|| {
                    ctx.push_vec(
                        &mut starts,
                        (name.offset, feature.id.as_str()),
                        "collect SLDPRT feature intervals",
                    )
                })?;
            }
        }
    }
    ctx.sort_unstable_by(
        &mut starts,
        |value| &value.0,
        Ord::cmp,
        "sort SLDPRT feature intervals",
    )?;
    ctx.dedup_by_key(
        &mut starts,
        |(offset, _)| Ok(*offset),
        "deduplicate SLDPRT feature intervals",
    )?;
    let mut intervals = Vec::new();
    for (index, (start, feature)) in ctx
        .admit_iter(&starts, "collect SLDPRT feature intervals")?
        .enumerate()
    {
        ctx.push_vec(
            &mut intervals,
            (
                *start,
                starts.get(index + 1).map(|(next, _)| *next),
                copy_relation_text(ctx, feature)?,
            ),
            "collect SLDPRT feature intervals",
        )?;
    }
    Ok(intervals)
}

/// The matching interval's feature name and exclusive end.
#[derive(Clone, Copy)]
struct FeatureIntervalMatch<'a> {
    feature_name: &'a str,
    end: Option<u64>,
}

/// The interval that contains `offset`, if one does.
fn feature_at_offset<'a>(
    ctx: &DecodeContext<'_>,
    offset: u64,
    intervals: &'a [(u64, Option<u64>, String)],
) -> Result<Option<FeatureIntervalMatch<'a>>, CodecError> {
    Ok(ctx
        .admit_iter(intervals, "find SLDPRT feature interval")?
        .find(|(start, end, _)| offset >= *start && end.is_none_or(|end| offset < end))
        .map(|(_, end, feature)| FeatureIntervalMatch {
            feature_name: feature.as_str(),
            end: *end,
        }))
}

/// The feature that owns `offset`, if one does.
fn feature_name_at_offset<'a>(
    ctx: &DecodeContext<'_>,
    offset: u64,
    intervals: &'a [(u64, Option<u64>, String)],
) -> Result<Option<&'a str>, CodecError> {
    Ok(feature_at_offset(ctx, offset, intervals)?.map(|interval| interval.feature_name))
}

/// Bytes a class with no feature interval may carry its relation over.
const UNKNOWN_FEATURE_SPAN: u64 = 128;

/// Where a relation's scope ends.
///
/// The three states are separate because neither of the last two is an offset.
/// The last feature interval is open, so a class inside it that no later
/// relation class of the same feature follows states a scope no offset bounds,
/// and every scalar after the class is in it. A class outside every interval
/// states no scope at all when no `u64` can name its unknown-feature span,
/// because reading its scalars unbounded would take every scalar after it.
enum RelationScope {
    /// The exclusive offset the scope ends at.
    Ends(u64),
    /// The class sits in the open last feature interval and no later relation
    /// class of that feature bounds it.
    Unbounded,
    /// The class states an offset whose unknown-feature span no `u64` can
    /// name. The class is refused: it declares no relation.
    Unstatable,
}

/// Where the relation's scope ends.
fn relation_scope_end(
    ctx: &DecodeContext<'_>,
    class: &FeatureInputClass,
    classes: &[FeatureInputClass],
    intervals: &[(u64, Option<u64>, String)],
) -> Result<RelationScope, CodecError> {
    let class_interval = feature_at_offset(ctx, class.offset, intervals)?;
    let class_feature = class_interval.map(|interval| interval.feature_name);
    let mut next_class = None;
    for candidate in ctx.admit_iter(classes, "find SLDPRT relation scope end")? {
        if candidate.offset <= class.offset || relation_family(&candidate.name).is_none() {
            continue;
        }
        let Some(feature) = class_feature else {
            continue;
        };
        let Some(candidate_feature) = feature_name_at_offset(ctx, candidate.offset, intervals)?
        else {
            continue;
        };
        if ctx.equal(
            feature,
            candidate_feature,
            "compare SLDPRT relation scope feature references",
        )? {
            next_class = Some(next_class.map_or(candidate.offset, |current: u64| {
                current.min(candidate.offset)
            }));
        }
    }
    // The interval the class sits in states its own end; a class in no
    // interval carries its relation over the unknown-feature span instead.
    let (feature_end, unknown_feature_limit) = match class_interval {
        Some(interval) => (interval.end, None),
        None => {
            let Some(limit) = class.offset.checked_add(UNKNOWN_FEATURE_SPAN) else {
                return Ok(RelationScope::Unstatable);
            };
            (None, Some(limit))
        }
    };
    Ok(
        match [next_class, feature_end, unknown_feature_limit]
            .into_iter()
            .flatten()
            .min()
        {
            Some(end) => RelationScope::Ends(end),
            None => RelationScope::Unbounded,
        },
    )
}

#[cfg(test)]
fn relation_declaration_candidates<'a>(
    ctx: &DecodeContext<'_>,
    classes: &'a [FeatureInputClass],
    scalars: &'a [FeatureInputScalar],
    intervals: &[(u64, Option<u64>, String)],
) -> Result<
    Vec<(
        &'a FeatureInputClass,
        &'a FeatureInputScalar,
        FeatureInputRelationFamily,
    )>,
    CodecError,
> {
    relation_declaration_candidates_impl(ctx, classes, scalars, intervals, false)
}

fn relation_declaration_candidates_impl<'a>(
    ctx: &DecodeContext<'_>,
    classes: &'a [FeatureInputClass],
    scalars: &'a [FeatureInputScalar],
    intervals: &[(u64, Option<u64>, String)],
    allow_dynamic: bool,
) -> Result<
    Vec<(
        &'a FeatureInputClass,
        &'a FeatureInputScalar,
        FeatureInputRelationFamily,
    )>,
    CodecError,
> {
    let mut candidates = Vec::new();
    for class in ctx.admit_iter(classes, "scan SLDPRT relation declarations")? {
        if let Some(candidate) =
            relation_declaration_candidate(ctx, class, classes, scalars, intervals, allow_dynamic)?
        {
            ctx.reserve_vec(&mut candidates, 1, "collect SLDPRT relation candidates")?;
            candidates.push(candidate);
        }
    }
    Ok(candidates)
}

fn relation_declaration_candidate<'a>(
    ctx: &DecodeContext<'_>,
    class: &'a FeatureInputClass,
    classes: &'a [FeatureInputClass],
    scalars: &'a [FeatureInputScalar],
    intervals: &[(u64, Option<u64>, String)],
    allow_dynamic: bool,
) -> Result<
    Option<(
        &'a FeatureInputClass,
        &'a FeatureInputScalar,
        FeatureInputRelationFamily,
    )>,
    CodecError,
> {
    let Some(family) = relation_family(&class.name) else {
        return Ok(None);
    };
    let class_feature = feature_name_at_offset(ctx, class.offset, intervals)?;
    let scope_end = match relation_scope_end(ctx, class, classes, intervals)? {
        RelationScope::Ends(end) => Some(end),
        RelationScope::Unbounded => None,
        RelationScope::Unstatable => return Ok(None),
    };
    let mut selected = None;
    for scalar in ctx.admit_iter(scalars, "match SLDPRT relation scalar declarations")? {
        if scalar.offset <= class.offset || scope_end.is_some_and(|end| scalar.offset >= end) {
            continue;
        }
        if let Some(feature) = class_feature {
            let Some(scalar_feature) = scalar.feature_ref.as_deref() else {
                continue;
            };
            if !ctx.equal(
                feature,
                scalar_feature,
                "compare SLDPRT relation declaration feature references",
            )? {
                continue;
            }
        }
        let signature = if allow_dynamic {
            relation_signature_for_declaration(family, scalar)
        } else {
            relation_signature(family, &scalar.operands)
        };
        if signature
            && selected.is_none_or(|current: &'a FeatureInputScalar| scalar.offset < current.offset)
        {
            selected = Some(scalar);
        }
    }
    Ok(selected.map(|scalar| (class, scalar, family)))
}

pub(super) fn unique_relation_declaration_candidates_charged<'a>(
    ctx: &DecodeContext<'_>,
    classes: &'a [FeatureInputClass],
    scalars: &'a [FeatureInputScalar],
    intervals: &[(u64, Option<u64>, String)],
) -> Result<
    Vec<(
        &'a FeatureInputClass,
        &'a FeatureInputScalar,
        FeatureInputRelationFamily,
    )>,
    CodecError,
> {
    let mut temporary_storage =
        ctx.reserve_scoped(0, "SLDPRT relation_records temporary storage")?;

    let candidates = relation_declaration_candidates_impl(ctx, classes, scalars, intervals, false)?;
    let mut counts = HashMap::<&str, usize>::new();
    for (_, scalar, _) in ctx.admit_iter(&candidates, "count SLDPRT relation candidates")? {
        if let Some(count) = ctx.get_mut_hash_map(
            &mut counts,
            scalar.id.as_str(),
            "lookup SLDPRT relation candidate count",
        )? {
            *count = count.checked_add(1).ok_or_else(|| {
                ctx.refuse_codec_limit("count SLDPRT relation candidates", u64::MAX - 1, u64::MAX)
            })?;
        } else {
            temporary_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut counts,
                    scalar.id.as_str(),
                    1,
                    "index SLDPRT relation candidates",
                )
            })?;
        }
    }
    let mut unique = Vec::new();
    for &candidate in ctx.admit_iter(&candidates, "select SLDPRT unique relations")? {
        if ctx.get_hash_map(
            &counts,
            candidate.1.id.as_str(),
            "lookup SLDPRT relation candidate count",
        )? == Some(&1)
        {
            ctx.reserve_vec(&mut unique, 1, "collect SLDPRT unique relations")?;
            unique.push(candidate);
        }
    }
    Ok(unique)
}

struct RelationGroup<'a> {
    feature_ref: &'a str,
    family: FeatureInputRelationFamily,
    class_ref: &'a str,
    operands: &'a [FeatureInputOperand],
    scalars: Vec<(usize, &'a FeatureInputScalar)>,
}

fn same_relation_operand_signature(
    ctx: &DecodeContext<'_>,
    left: &[FeatureInputOperand],
    right: &[FeatureInputOperand],
    operation: &'static str,
) -> Result<bool, CodecError> {
    if left.len() != right.len() {
        return Ok(false);
    }
    for (left, right) in ctx
        .admit_iter(left, operation)?
        .zip(ctx.admit_iter(right, operation)?)
    {
        if left.kind != right.kind || left.entity_index != right.entity_index {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) fn relation_instances(
    ctx: &DecodeContext<'_>,
    histories: &[crate::records::FeatureHistory],
    lane: &FeatureInputLane,
) -> Result<Vec<FeatureInputRelationInstance>, CodecError> {
    let mut temporary_storage =
        ctx.reserve_scoped(0, "SLDPRT relation_records temporary storage")?;

    let mut sketch_features = HashSet::new();
    for history in ctx.admit_iter(histories, "index SLDPRT relation records")? {
        for feature in ctx.admit_iter(&history.features, "index SLDPRT relation records")? {
            if ctx.eq_ignore_ascii_case(
                feature.xml_tag.as_str(),
                "Sketch",
                "match SLDPRT sketch feature tags",
            )? {
                temporary_storage.with_storage(|| {
                    ctx.insert_hash_set(
                        &mut sketch_features,
                        feature.id.as_str(),
                        "index SLDPRT relation records",
                    )
                })?;
            }
        }
    }
    let intervals = temporary_storage.with_storage(|| feature_intervals(ctx, histories, lane))?;
    let declaration_candidates =
        relation_declaration_candidates_impl(ctx, &lane.classes, &lane.scalars, &intervals, true)?;
    let mut candidate_counts = HashMap::<&str, usize>::new();
    for (_, scalar, _) in
        ctx.admit_iter(&declaration_candidates, "count SLDPRT relation candidates")?
    {
        if let Some(count) = ctx.get_mut_hash_map(
            &mut candidate_counts,
            scalar.id.as_str(),
            "lookup SLDPRT relation candidate count",
        )? {
            *count = count.checked_add(1).ok_or_else(|| {
                ctx.refuse_codec_limit("count SLDPRT relation candidates", u64::MAX - 1, u64::MAX)
            })?;
        } else {
            temporary_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut candidate_counts,
                    scalar.id.as_str(),
                    1,
                    "index SLDPRT relation records",
                )
            })?;
        }
    }
    let mut declarations = HashMap::new();
    for (class, scalar, family) in ctx
        .admit_iter(
            &declaration_candidates,
            "index SLDPRT relation declarations",
        )?
        .copied()
    {
        if ctx.get_hash_map(
            &candidate_counts,
            scalar.id.as_str(),
            "lookup SLDPRT relation candidate count",
        )? == Some(&1)
        {
            temporary_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut declarations,
                    scalar.id.as_str(),
                    (class.offset, family, class.id.as_str()),
                    "index SLDPRT relation records",
                )
            })?;
        }
    }
    let mut groups = Vec::<RelationGroup<'_>>::new();
    for (scalar_index, scalar) in ctx
        .admit_iter(&lane.scalars, "group SLDPRT relation scalars")?
        .enumerate()
    {
        let Some(feature_ref) = scalar.feature_ref.as_deref() else {
            continue;
        };
        if !ctx.contains_hash_set(
            &sketch_features,
            feature_ref,
            "select SLDPRT sketch relation scalars",
        )? {
            continue;
        }
        let declaration = ctx.get_hash_map(
            &declarations,
            scalar.id.as_str(),
            "find SLDPRT relation declaration",
        )?;
        let Some((class_offset, family, class_ref)) = declaration else {
            if ctx
                .get_hash_map(
                    &candidate_counts,
                    scalar.id.as_str(),
                    "find SLDPRT relation candidate count",
                )?
                .is_some_and(|count| *count > 1)
            {
                continue;
            }
            let Some(group) = groups.last_mut() else {
                continue;
            };
            let Some((last_index, last_scalar)) = group.scalars.last() else {
                continue;
            };
            let same_scope_feature = ctx.equal(
                group.feature_ref,
                feature_ref,
                "compare SLDPRT relation group features",
            )?;
            let same_scope_prefix = same_scope_feature && *last_index + 1 == scalar_index;
            let has_intervening_class = if same_scope_prefix {
                ctx.admit_iter(&lane.classes, "find SLDPRT relation group boundary")?
                    .any(|class| {
                        class.offset > last_scalar.offset
                            && class.offset < scalar.offset
                            && relation_family(&class.name).is_some()
                    })
            } else {
                false
            };
            let same_scope = same_scope_prefix && !has_intervening_class;
            let same_operands = same_relation_operand_signature(
                ctx,
                group.operands,
                &scalar.operands,
                "compare SLDPRT relation operand signatures",
            )?;
            let mut repeated_circle_display = same_scope
                && group.family == FeatureInputRelationFamily::CircleDiameter
                && scalar.role == FeatureInputScalarRole::Display
                && scalar.operands.len() == 1;
            if repeated_circle_display {
                for (_, candidate) in ctx.admit_iter(
                    &group.scalars,
                    "validate SLDPRT repeated circle display scalars",
                )? {
                    if candidate.role != FeatureInputScalarRole::Display
                        || candidate.operands.len() != 1
                        || candidate.operands[0].kind != scalar.operands[0].kind
                        || candidate.operands[0].entity_index == scalar.operands[0].entity_index
                        || !same_scalar_name(ctx, candidate, scalar, &lane.names)?
                    {
                        repeated_circle_display = false;
                        break;
                    }
                }
            }
            if repeated_circle_display {
                ctx.reserve_vec(
                    &mut group.scalars,
                    1,
                    "collect SLDPRT relation scalar groups",
                )?;
                group.scalars.push((scalar_index, scalar));
            } else if same_scope && same_operands && scalar.role == FeatureInputScalarRole::Driving
            {
                if group.scalars.len() == 1 {
                    ctx.reserve_vec(
                        &mut group.scalars,
                        1,
                        "collect SLDPRT relation scalar groups",
                    )?;
                    group.scalars.push((scalar_index, scalar));
                } else {
                    let next = RelationGroup {
                        feature_ref: group.feature_ref,
                        family: group.family,
                        class_ref: group.class_ref,
                        operands: &scalar.operands,
                        scalars: ctx.alloc_filled(
                            1,
                            (scalar_index, scalar),
                            "collect SLDPRT relation scalar groups",
                        )?,
                    };
                    ctx.reserve_vec(&mut groups, 1, "collect SLDPRT relation groups")?;
                    groups.push(next);
                }
            }
            continue;
        };
        // A display scalar can precede the declaration selected by its adjacent
        // driving scalar. The driving scalar's declaration is authoritative for
        // that pair; display-only scalars still stop at a class declaration.
        let promote_class = if let Some(group) = groups.last() {
            !ctx.equal(
                group.class_ref,
                class_ref,
                "compare SLDPRT promoted relation classes",
            )? && group.scalars.len() == 1
                && group.scalars[0].1.role == FeatureInputScalarRole::Display
                && scalar.role == FeatureInputScalarRole::Driving
                && *class_offset > group.scalars[0].1.offset
                && *class_offset < scalar.offset
        } else {
            false
        };
        let append = if let Some(group) = groups.last() {
            ctx.equal(
                group.feature_ref,
                feature_ref,
                "compare SLDPRT relation group features",
            )? && (group.family == *family || promote_class)
                && (ctx.equal(
                    group.class_ref,
                    class_ref,
                    "compare SLDPRT relation group classes",
                )? || promote_class)
                && matches!(group.scalars.as_slice(), [(index, _)] if *index + 1 == scalar_index)
                && same_relation_operand_signature(
                    ctx,
                    group.operands,
                    &scalar.operands,
                    "compare SLDPRT relation operand signatures",
                )?
        } else {
            false
        };
        if append {
            let Some(group) = groups.last_mut() else {
                continue;
            };
            if promote_class {
                group.family = *family;
                group.class_ref = *class_ref;
            }
            ctx.reserve_vec(
                &mut group.scalars,
                1,
                "collect SLDPRT relation scalar groups",
            )?;
            group.scalars.push((scalar_index, scalar));
        } else {
            ctx.reserve_vec(&mut groups, 1, "collect SLDPRT relation groups")?;
            groups.push(RelationGroup {
                feature_ref,
                family: *family,
                class_ref,
                operands: &scalar.operands,
                scalars: ctx.alloc_filled(
                    1,
                    (scalar_index, scalar),
                    "collect SLDPRT relation scalar groups",
                )?,
            });
        }
    }
    let mut instances = Vec::new();
    let mut claimed_scalar_refs = HashSet::new();
    let lane_key = ctx
        .rsplit_once(&lane.id, "#", "split SLDPRT feature-input relation parent")?
        .map_or(lane.id.as_str(), |(_, key)| key);
    for (ordinal, group) in ctx
        .admit_iter(&groups, "collect SLDPRT relation instances")?
        .enumerate()
    {
        for (_, scalar) in
            ctx.admit_iter(&group.scalars, "index SLDPRT claimed relation scalars")?
        {
            temporary_storage.with_storage(|| {
                ctx.insert_hash_set(
                    &mut claimed_scalar_refs,
                    scalar.id.as_str(),
                    "index SLDPRT relation records",
                )
            })?;
        }
        let offset = group.scalars[0].1.offset;
        let ordinal = u32::try_from(ordinal).map_err(|_| {
            ctx.refuse_codec_limit("number SLDPRT relation instances", u64::MAX - 1, u64::MAX)
        })?;
        ctx.reserve_vec(&mut instances, 1, "collect SLDPRT relation instances")?;
        instances.push(FeatureInputRelationInstance {
            id: ctx.format_retained(
                format_args!("sldprt:feature-input:relation-instance#{lane_key}:{offset}"),
                "retain SLDPRT relation instance identity",
            )?,
            parent: copy_relation_text(ctx, &lane.id)?,
            ordinal,
            offset,
            family: group.family,
            class_ref: copy_relation_text(ctx, group.class_ref)?,
            feature_ref: copy_relation_text(ctx, group.feature_ref)?,
            scalars: crate::records::relation_scalars::RelationScalars::from_scalars(
                ctx,
                &group.scalars,
                |(_, scalar)| *scalar,
            )?,
            operands: copy_relation_operands(ctx, group.operands)?,
        });
    }
    // A class can declare more than one scalar relation. The ordinary
    // instance grouper joins a display scalar to its adjacent driving scalar
    // when their operand signatures agree; a second scalar with a different
    // signature is still a declared relation binding and must not disappear.
    // Promote only bindings whose scalar is not already claimed by an
    // instance. A scalar has one relation-instance owner even if malformed
    // input associates it with more than one class. Non-sketch bindings remain
    // native-only, matching the existing relation-instance scope.
    for binding in ctx.admit_iter(&lane.relation_bindings, "promote SLDPRT relation bindings")? {
        let Some(feature_ref) = binding.feature_ref.as_deref() else {
            continue;
        };
        if !ctx.contains_hash_set(
            &sketch_features,
            feature_ref,
            "select SLDPRT sketch relation bindings",
        )? {
            continue;
        }
        let Some(scalar_index) = ({
            let predicate = |scalar: &FeatureInputScalar| -> Result<bool, CodecError> {
                ctx.equal(
                    scalar.id.as_str(),
                    binding.scalar_ref.as_str(),
                    "find SLDPRT relation binding scalar",
                )
            };
            let mut found = None;
            for (index, value) in ctx
                .admit_iter(&lane.scalars, "find SLDPRT relation binding scalar")?
                .enumerate()
            {
                if predicate(value)? {
                    found = Some(index);
                    break;
                }
            }
            Ok::<_, CodecError>(found)
        })?
        else {
            continue;
        };
        let Some(scalar) = lane.scalars.get(scalar_index) else {
            continue;
        };
        if ctx.contains_hash_set(
            &claimed_scalar_refs,
            binding.scalar_ref.as_str(),
            "check claimed SLDPRT relation scalars",
        )? {
            continue;
        }
        temporary_storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut claimed_scalar_refs,
                binding.scalar_ref.as_str(),
                "index SLDPRT relation records",
            )
        })?;
        ctx.reserve_vec(&mut instances, 1, "collect SLDPRT relation instances")?;
        instances.push(FeatureInputRelationInstance {
            id: ctx.format_retained(
                format_args!(
                    "sldprt:feature-input:relation-instance#{lane_key}:{}",
                    scalar.offset
                ),
                "retain SLDPRT relation instance identity",
            )?,
            parent: copy_relation_text(ctx, &lane.id)?,
            ordinal: u32::try_from(instances.len()).map_err(|_| {
                ctx.refuse_codec_limit("number SLDPRT relation instances", u64::MAX - 1, u64::MAX)
            })?,
            offset: scalar.offset,
            family: binding.family,
            class_ref: copy_relation_text(ctx, &binding.class_ref)?,
            feature_ref: copy_relation_text(ctx, feature_ref)?,
            scalars: crate::records::relation_scalars::RelationScalars::from_scalars(
                ctx,
                &[scalar],
                |scalar| *scalar,
            )?,
            operands: copy_relation_operands(ctx, &scalar.operands)?,
        });
    }
    ctx.sort_unstable_by_key(
        &mut instances,
        |value| (value.offset, value.ordinal),
        Ord::cmp,
        "sort SLDPRT relation instances",
    )?;
    for ordinal in ctx.admit_iter(&(0..instances.len()), "number SLDPRT relation instances")? {
        let relation = &mut instances[ordinal];
        relation.ordinal = u32::try_from(ordinal).map_err(|_| {
            ctx.refuse_codec_limit("number SLDPRT relation instances", u64::MAX - 1, u64::MAX)
        })?;
    }
    bind_detached_relation_drivers(ctx, &mut instances, lane)?;
    bind_circle_dimension_centers(ctx, &mut instances, lane)?;
    bind_relation_geometry_operands(ctx, &mut instances, lane)?;
    Ok(instances)
}

fn copy_relation_text(ctx: &DecodeContext<'_>, text: &str) -> Result<String, CodecError> {
    let copy_work = cadmpeg_core::decode::u64_from_index(text.len())
        .checked_mul(4)
        .ok_or_else(|| {
            ctx.refuse_codec_limit(
                "retain SLDPRT relation record identity",
                u64::MAX - 1,
                u64::MAX,
            )
        })?;
    ctx.charge_work(copy_work, "retain SLDPRT relation record identity")?;
    ctx.format_retained(
        format_args!("{text}"),
        "retain SLDPRT relation record identity",
    )
}

fn copy_relation_operands(
    ctx: &DecodeContext<'_>,
    operands: &[FeatureInputOperand],
) -> Result<Vec<FeatureInputOperand>, CodecError> {
    let mut copy = Vec::new();
    for operand in ctx.admit_iter(operands, "copy SLDPRT relation operands")? {
        ctx.push_vec(
            &mut copy,
            FeatureInputOperand {
                offset: operand.offset,
                reference_ref: copy_relation_text(ctx, &operand.reference_ref)?,
                kind: operand.kind,
                entity_index: operand.entity_index,
                entity_ref: operand
                    .entity_ref
                    .as_deref()
                    .map(|id| copy_relation_text(ctx, id))
                    .transpose()?,
            },
            "collect SLDPRT relation operands",
        )?;
    }
    Ok(copy)
}

pub(super) fn bind_circle_dimension_centers(
    ctx: &DecodeContext<'_>,
    relations: &mut [FeatureInputRelationInstance],
    lane: &FeatureInputLane,
) -> Result<(), CodecError> {
    let mut temporary_storage =
        ctx.reserve_scoped(0, "SLDPRT relation_records temporary storage")?;

    let mut scalars = HashMap::new();
    for scalar in ctx.admit_iter(&lane.scalars, "index SLDPRT relation records")? {
        temporary_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut scalars,
                scalar.id.as_str(),
                scalar,
                "index SLDPRT relation records",
            )
        })?;
    }
    let mut names = HashMap::new();
    for name in ctx.admit_iter(&lane.names, "index SLDPRT relation records")? {
        temporary_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut names,
                name.id.as_str(),
                name.value.as_str(),
                "index SLDPRT relation records",
            )
        })?;
    }
    for relation_index in ctx.admit_iter(
        &(0..relations.len()),
        "scan SLDPRT circle-diameter relations",
    )? {
        let relation = &mut relations[relation_index];
        if relation.family != FeatureInputRelationFamily::CircleDiameter
            || relation.operands.len() != 1
        {
            continue;
        }
        let Some(display_id) = relation.display_scalar_ref() else {
            continue;
        };
        let Some(display) = ctx
            .get_hash_map(&scalars, display_id, "find SLDPRT display scalar")?
            .copied()
        else {
            continue;
        };
        let Some(display_name) = ctx.get_hash_map(
            &names,
            display.name.as_str(),
            "find SLDPRT display scalar name",
        )?
        else {
            continue;
        };
        let Some(display_index) = ({
            let predicate = |scalar: &FeatureInputScalar| -> Result<bool, CodecError> {
                ctx.equal(
                    scalar.id.as_str(),
                    display.id.as_str(),
                    "match SLDPRT display scalar",
                )
            };
            let mut found = None;
            for (index, value) in ctx
                .admit_iter(&lane.scalars, "match SLDPRT display scalar")?
                .enumerate()
            {
                if predicate(value)? {
                    found = Some(index);
                    break;
                }
            }
            Ok::<_, CodecError>(found)
        })?
        else {
            continue;
        };
        let first = &relation.operands[0];
        let mut candidates = Vec::new();
        for (index, scalar) in ctx
            .admit_iter(&lane.scalars, "match SLDPRT circle center scalars")?
            .enumerate()
        {
            let feature_matches = match (
                scalar.feature_ref.as_deref(),
                display.feature_ref.as_deref(),
            ) {
                (Some(left), Some(right)) => {
                    ctx.equal(left, right, "compare SLDPRT circle center features")?
                }
                (None, None) => true,
                _ => false,
            };
            if !feature_matches {
                continue;
            }
            let Some(name) = ctx.get_hash_map(
                &names,
                scalar.name.as_str(),
                "find SLDPRT circle center scalar name",
            )?
            else {
                continue;
            };
            if !ctx.equal(name, display_name, "compare SLDPRT circle center names")? {
                continue;
            }
            if !matches!(scalar.operands.as_slice(), [candidate, _]
                if candidate.kind == first.kind
                    && candidate.entity_index == first.entity_index)
            {
                continue;
            }
            temporary_storage.with_storage(|| {
                ctx.push_vec(
                    &mut candidates,
                    (index, scalar),
                    "collect SLDPRT circle center scalars",
                )
            })?;
        }
        let first_center_index = display_index.checked_add(1);
        if candidates.first().map(|candidate| candidate.0) != first_center_index {
            continue;
        }
        let mut previous_index: Option<usize> = None;
        let mut contiguous = true;
        for (index, _) in ctx.admit_iter(&candidates, "check SLDPRT circle center scalar window")? {
            if previous_index.is_some_and(|previous| previous.checked_add(1) != Some(*index)) {
                contiguous = false;
                break;
            }
            previous_index = Some(*index);
        }
        if !contiguous {
            continue;
        }
        let mut center: Option<(FeatureInputOperandKind, u16, Option<&str>)> = None;
        let mut centers_match = true;
        for (_, scalar) in
            ctx.admit_iter(&candidates, "check SLDPRT circle center operand consensus")?
        {
            let Some(operand) = scalar.operands.get(1) else {
                continue;
            };
            let candidate = (
                operand.kind,
                operand.entity_index,
                operand.entity_ref.as_deref(),
            );
            let same_center = if let Some((kind, entity_index, entity_ref)) = center {
                let same_ref = match (entity_ref, candidate.2) {
                    (Some(left), Some(right)) => {
                        ctx.equal(left, right, "compare SLDPRT circle center references")?
                    }
                    (None, None) => true,
                    _ => false,
                };
                kind == candidate.0 && entity_index == candidate.1 && same_ref
            } else {
                center = Some(candidate);
                true
            };
            if !same_center {
                centers_match = false;
                break;
            }
        }
        let Some(center) = center else {
            continue;
        };
        if !centers_match {
            continue;
        }
        let Some((_, source)) = ({
            let predicate =
                |(_, scalar): &(usize, &FeatureInputScalar)| -> Result<bool, CodecError> {
                    let Some(operand) = scalar.operands.get(1) else {
                        return Ok(false);
                    };
                    let same_ref = match (center.2, operand.entity_ref.as_deref()) {
                        (Some(left), Some(right)) => {
                            ctx.equal(left, right, "compare SLDPRT circle center references")?
                        }
                        (None, None) => true,
                        _ => false,
                    };
                    Ok(operand.kind == center.0 && operand.entity_index == center.1 && same_ref)
                };
            let mut found = None;
            for value in ctx.admit_iter(&candidates, "find SLDPRT circle center source")? {
                if predicate(value)? {
                    found = Some(value);
                    break;
                }
            }
            Ok::<_, CodecError>(found)
        })?
        else {
            continue;
        };
        relation.operands = copy_relation_operands(ctx, &source.operands)?;
        for (_, scalar) in ctx.admit_iter(&candidates, "attach SLDPRT circle center scalars")? {
            if !ctx.contains(
                relation.scalar_refs(),
                &scalar.id,
                "check SLDPRT circle center scalar references",
            )? {
                relation.scalars.push(ctx, &scalar.id)?;
            }
        }
    }
    Ok(())
}

fn same_scalar_operands(
    ctx: &DecodeContext<'_>,
    left: &FeatureInputScalar,
    right: &FeatureInputScalar,
) -> Result<bool, CodecError> {
    if left.operands.len() != right.operands.len() {
        return Ok(false);
    }
    for (left, right) in ctx
        .admit_iter(&left.operands, "compare SLDPRT relation scalar operands")?
        .zip(ctx.admit_iter(&right.operands, "compare SLDPRT relation scalar operands")?)
    {
        let same_ref = match (left.entity_ref.as_deref(), right.entity_ref.as_deref()) {
            (Some(left), Some(right)) => {
                ctx.equal(left, right, "compare SLDPRT relation scalar references")?
            }
            (None, None) => true,
            _ => false,
        };
        if left.kind != right.kind || left.entity_index != right.entity_index || !same_ref {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) fn circle_dimension_handle_driver<'a>(
    ctx: &DecodeContext<'_>,
    relation: &FeatureInputRelationInstance,
    lane: &'a FeatureInputLane,
) -> Result<Option<&'a FeatureInputScalar>, CodecError> {
    let mut temporary_storage =
        ctx.reserve_scoped(0, "SLDPRT relation_records temporary storage")?;

    if relation.family != FeatureInputRelationFamily::CircleDiameter
        || relation.parameter_scalar_ref().is_some()
        || relation.scalar_refs().len() != 1
    {
        return Ok(None);
    }
    let mut scalars = Vec::new();
    for scalar in ctx.admit_iter(&lane.scalars, "collect SLDPRT relation scalars")? {
        temporary_storage.with_storage(|| {
            ctx.push_vec(&mut scalars, scalar, "collect SLDPRT relation scalars")
        })?;
    }
    ctx.sort_unstable_by(
        &mut scalars,
        |value| &value.offset,
        Ord::cmp,
        "sort SLDPRT relation scalars",
    )?;
    let mut names = HashMap::new();
    for name in ctx.admit_iter(&lane.names, "index SLDPRT relation records")? {
        temporary_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut names,
                name.id.as_str(),
                name.value.as_str(),
                "index SLDPRT relation records",
            )
        })?;
    }
    let Some(first_id) = relation.scalar_refs().first() else {
        return Ok(None);
    };
    let Some(first) = ({
        let predicate = |scalar: &&FeatureInputScalar| -> Result<bool, CodecError> {
            ctx.equal(
                scalar.id.as_str(),
                first_id.as_str(),
                "find SLDPRT relation display scalar",
            )
        };
        let mut found = None;
        for value in ctx.admit_iter(&scalars, "find SLDPRT relation display scalar")? {
            if predicate(value)? {
                found = Some(value);
                break;
            }
        }
        Ok::<_, CodecError>(found)
    })?
    .copied()
    .filter(|scalar| scalar.role == FeatureInputScalarRole::Display) else {
        return Ok(None);
    };
    // No following relation class states no upper bound on this relation.
    let mut next_relation_offset = None;
    for class in ctx.admit_iter(&lane.classes, "find SLDPRT circle relation end")? {
        if class.offset > first.offset && relation_family(&class.name).is_some() {
            next_relation_offset = Some(
                next_relation_offset.map_or(class.offset, |limit: u64| limit.min(class.offset)),
            );
        }
    }
    let mut previous = None;
    let mut candidate = None;
    let mut ambiguous = false;
    for driving in ctx.admit_iter(&scalars, "match SLDPRT circle handle driver")? {
        let Some(display) = previous else {
            previous = Some(*driving);
            continue;
        };
        let declared_handle = ({
            let predicate = |class: &FeatureInputClass| -> Result<bool, CodecError> {
                Ok(class.offset > first.offset
                    && class.offset < display.offset
                    && ctx.equal(
                        class.name.as_str(),
                        "sgEntHandle",
                        "match SLDPRT circle handle class",
                    )?)
            };
            let mut found = false;
            for value in ctx.admit_iter(&lane.classes, "find SLDPRT circle handle class")? {
                if predicate(value)? {
                    found = true;
                    break;
                }
            }
            Ok::<_, CodecError>(found)
        })?;
        let same_feature = match (
            display.feature_ref.as_deref(),
            first.feature_ref.as_deref(),
            driving.feature_ref.as_deref(),
        ) {
            (Some(display), Some(first), Some(driving)) => {
                ctx.equal(display, first, "compare SLDPRT circle features")?
                    && ctx.equal(driving, first, "compare SLDPRT circle features")?
            }
            (None, None, None) => true,
            _ => false,
        };
        if declared_handle
            && same_feature
            && same_scalar_operands(ctx, display, first)?
            && same_scalar_operands(ctx, driving, first)?
            && display.role == FeatureInputScalarRole::Display
            && driving.role == FeatureInputScalarRole::Driving
        {
            let left_name = ctx.get_hash_map(
                &names,
                display.name.as_str(),
                "find SLDPRT circle display name",
            )?;
            let right_name = ctx.get_hash_map(
                &names,
                driving.name.as_str(),
                "find SLDPRT circle driving name",
            )?;
            let same_name = match (left_name, right_name) {
                (Some(left), Some(right)) => {
                    ctx.equal(left, right, "compare SLDPRT circle scalar names")?
                }
                _ => false,
            };
            let in_relation = first.offset < display.offset
                && next_relation_offset.is_none_or(|limit| driving.offset < limit);
            if same_name && in_relation {
                if candidate.is_some() {
                    ambiguous = true;
                } else {
                    candidate = Some(*driving);
                }
            }
        }
        previous = Some(*driving);
    }
    Ok(if ambiguous { None } else { candidate })
}

pub(super) fn bind_detached_relation_drivers(
    ctx: &DecodeContext<'_>,
    relations: &mut [FeatureInputRelationInstance],
    lane: &FeatureInputLane,
) -> Result<(), CodecError> {
    const INDEX: &str = "index SLDPRT relation records";
    let mut temporary = ctx.reserve_scoped(0, INDEX)?;
    let mut scalars = HashMap::new();
    for scalar in ctx.admit_iter(&lane.scalars, INDEX)? {
        temporary.with_storage(|| {
            ctx.insert_hash_map(&mut scalars, scalar.id.as_str(), scalar, INDEX)
        })?;
    }
    let mut names = HashMap::new();
    for name in ctx.admit_iter(&lane.names, INDEX)? {
        temporary.with_storage(|| {
            ctx.insert_hash_map(&mut names, name.id.as_str(), name.value.as_str(), INDEX)
        })?;
    }
    let mut claimed = HashSet::new();
    for relation in ctx.admit_iter(&*relations, INDEX)? {
        for id in ctx.admit_iter(relation.scalar_refs(), INDEX)? {
            temporary.with_storage(|| ctx.insert_hash_set(&mut claimed, id.as_str(), INDEX))?;
        }
    }
    let mut drivers = HashMap::<(&str, &str), Vec<&FeatureInputScalar>>::new();
    for scalar in ctx.admit_iter(&lane.scalars, "match SLDPRT detached relation drivers")? {
        if scalar.role != FeatureInputScalarRole::Driving
            || !scalar.operands.is_empty()
            || ctx.contains_hash_set(&claimed, scalar.id.as_str(), INDEX)?
        {
            continue;
        }
        let feature = scalar.feature_ref.as_deref();
        let name = ctx
            .get_hash_map(&names, scalar.name.as_str(), INDEX)?
            .copied();
        let (Some(feature), Some(name)) = (feature, name) else {
            continue;
        };
        temporary.with_storage(|| {
            ctx.push_hash_group(
                &mut drivers,
                (feature, name),
                scalar,
                INDEX,
                "collect SLDPRT detached relation drivers",
            )
        })?;
    }
    let mut candidates = HashMap::<(String, String), Vec<usize>>::new();
    for (index, relation) in ctx
        .admit_iter(&*relations, "match SLDPRT detached relation candidates")?
        .enumerate()
    {
        if relation.parameter_scalar_ref().is_some() {
            continue;
        }
        let mut first_name = None;
        let mut names_match = true;
        for id in ctx.admit_iter(
            relation.scalar_refs(),
            "compare SLDPRT detached relation names",
        )? {
            let Some(scalar) = ctx.get_hash_map(&scalars, id.as_str(), INDEX)? else {
                continue;
            };
            if scalar.role != FeatureInputScalarRole::Display {
                continue;
            }
            let Some(name) = ctx
                .get_hash_map(&names, scalar.name.as_str(), INDEX)?
                .copied()
            else {
                continue;
            };
            if let Some(first) = first_name {
                if !ctx.equal(name, first, "compare SLDPRT detached relation names")? {
                    names_match = false;
                    break;
                }
            } else {
                first_name = Some(name);
            }
        }
        let Some(name) = first_name else {
            continue;
        };
        if !names_match {
            continue;
        }
        let key = temporary.with_storage(|| {
            Ok::<_, CodecError>((
                copy_relation_text(ctx, &relation.feature_ref)?,
                copy_relation_text(ctx, name)?,
            ))
        })?;
        temporary.with_storage(|| {
            ctx.push_hash_group(
                &mut candidates,
                key,
                index,
                INDEX,
                "collect SLDPRT detached relation candidates",
            )
        })?;
    }
    for (key, relation_indices) in
        ctx.admit_iter(&candidates, "attach SLDPRT detached relation drivers")?
    {
        let [relation_index] = relation_indices.as_slice() else {
            continue;
        };
        let Some([driver]) = ctx
            .get_hash_map(&drivers, &(key.0.as_str(), key.1.as_str()), INDEX)?
            .map(Vec::as_slice)
        else {
            continue;
        };
        let relation = &mut relations[*relation_index];
        relation.scalars.push_parameter(ctx, &driver.id)?;
    }
    Ok(())
}

fn relation_family(name: &str) -> Option<FeatureInputRelationFamily> {
    match native_object_class(name) {
        NativeClassKind::SketchRelation(family) => Some(family),
        _ => None,
    }
}

fn relation_signature(
    family: FeatureInputRelationFamily,
    operands: &[FeatureInputOperand],
) -> bool {
    // `80d5`, `8138`, `80ac`, and `810f` are class-scoped relation cells. Keep
    // them behind the declared family instead of treating them as global
    // marker kinds.
    use FeatureInputOperandKind::{Native, D6, E1};
    use FeatureInputRelationFamily::{
        Angle, CircleDiameter, LineLineDistance, PointLineDistance, PointPointDistance,
        PointPointHorizontalDistance, PointPointVerticalDistance,
    };
    match (family, operands) {
        (CircleDiameter, [operand]) => matches!(operand.kind, Native(_)),
        (PointPointDistance, [first, second]) => {
            (first.kind == D6 && second.kind == D6)
                || (first.kind == Native(NativeOperandTag::TAG_81B2)
                    && second.kind == Native(NativeOperandTag::TAG_81B2))
                || (first.kind == Native(NativeOperandTag::TAG_8152)
                    && second.kind == Native(NativeOperandTag::TAG_8152))
                || (first.kind == Native(NativeOperandTag::TAG_80D5)
                    && second.kind == Native(NativeOperandTag::TAG_80D5))
                || (first.kind == Native(NativeOperandTag::TAG_8138)
                    && second.kind == Native(NativeOperandTag::TAG_8138))
                || (first.kind == Native(NativeOperandTag::TAG_80AC)
                    && second.kind == Native(NativeOperandTag::TAG_80AC))
                || (first.kind == Native(NativeOperandTag::TAG_837B)
                    && second.kind == Native(NativeOperandTag::TAG_837B))
                || (first.kind == Native(NativeOperandTag::TAG_BC7C)
                    && second.kind == Native(NativeOperandTag::TAG_BC7C))
                || (first.kind == Native(NativeOperandTag::TAG_81DD)
                    && second.kind == Native(NativeOperandTag::TAG_81DD))
                || (first.kind == Native(NativeOperandTag::TAG_8100)
                    && second.kind == Native(NativeOperandTag::TAG_8100))
                || (first.kind == Native(NativeOperandTag::TAG_820F)
                    && second.kind == Native(NativeOperandTag::TAG_820F))
        }
        (LineLineDistance, [first, second]) => {
            (first.kind == E1 && second.kind == E1)
                || (first.kind == Native(NativeOperandTag::TAG_8386)
                    && second.kind == Native(NativeOperandTag::TAG_8386))
                || (first.kind == Native(NativeOperandTag::TAG_810F)
                    && second.kind == Native(NativeOperandTag::TAG_810F))
                || (first.kind == Native(NativeOperandTag::TAG_BC87)
                    && second.kind == Native(NativeOperandTag::TAG_BC87))
                || (first.kind == Native(NativeOperandTag::TAG_81E7)
                    && second.kind == Native(NativeOperandTag::TAG_81E7))
        }
        (PointLineDistance, [first, second]) => {
            (first.kind == D6 && second.kind == E1)
                || (first.kind == Native(NativeOperandTag::TAG_837B)
                    && second.kind == Native(NativeOperandTag::TAG_8386))
                || (first.kind == Native(NativeOperandTag::TAG_BC7C)
                    && second.kind == Native(NativeOperandTag::TAG_BC87))
                || (first.kind == Native(NativeOperandTag::TAG_81DD)
                    && second.kind == Native(NativeOperandTag::TAG_81E7))
        }
        (PointPointHorizontalDistance | PointPointVerticalDistance, [first, second]) => {
            (first.kind == Native(NativeOperandTag::TAG_8152)
                && second.kind == Native(NativeOperandTag::TAG_8152))
                || (first.kind == Native(NativeOperandTag::TAG_80D5)
                    && second.kind == Native(NativeOperandTag::TAG_80D5))
                || (first.kind == Native(NativeOperandTag::TAG_8DCB)
                    && second.kind == Native(NativeOperandTag::TAG_8DCB))
        }
        (Angle, [first, second]) => {
            (first.kind == Native(NativeOperandTag::TAG_8DDA)
                && second.kind == Native(NativeOperandTag::TAG_8DDA))
                || (first.kind == Native(NativeOperandTag::TAG_80D5)
                    && second.kind == Native(NativeOperandTag::TAG_80D5))
        }
        _ => false,
    }
}

fn is_solver_point_operand(kind: FeatureInputOperandKind) -> bool {
    matches!(
        kind,
        FeatureInputOperandKind::Native(NativeOperandTag::TAG_8100 | NativeOperandTag::TAG_820F)
    )
}

pub(super) fn relation_uses_solver_points(
    ctx: &DecodeContext<'_>,
    relation: &FeatureInputRelationInstance,
) -> Result<bool, CodecError> {
    if relation.family != FeatureInputRelationFamily::PointPointDistance
        || relation.operands.len() != 2
    {
        return Ok(false);
    }
    Ok(ctx
        .admit_iter(&relation.operands, "check SLDPRT relation solver points")?
        .all(|operand| is_solver_point_operand(operand.kind)))
}

fn relation_signature_for_declaration(
    family: FeatureInputRelationFamily,
    scalar: &FeatureInputScalar,
) -> bool {
    relation_signature(family, &scalar.operands)
        || (matches!(
            scalar.role,
            FeatureInputScalarRole::Display | FeatureInputScalarRole::Driving
        ) && dynamic_relation_signature(family, &scalar.operands))
}

fn dynamic_relation_signature(
    family: FeatureInputRelationFamily,
    operands: &[FeatureInputOperand],
) -> bool {
    match family {
        FeatureInputRelationFamily::CircleDiameter => false,
        _ => matches!(
            operands,
            [
                FeatureInputOperand {
                    kind: FeatureInputOperandKind::Native(_),
                    ..
                },
                FeatureInputOperand {
                    kind: FeatureInputOperandKind::Native(_),
                    ..
                }
            ]
        ),
    }
}

/// Returns whether a relation uses a lane-local operand tag whose meaning is
/// supplied by the declared relation family and operand position.
pub(super) fn relation_uses_dynamic_operands(relation: &FeatureInputRelationInstance) -> bool {
    match relation.family {
        // `80d5` is a point carrier in point-distance records but occurs in the
        // line-role cells of angular records. The family therefore owns its
        // meaning in this case even though the static signature recognizes it.
        FeatureInputRelationFamily::Angle => {
            dynamic_relation_signature(relation.family, &relation.operands)
                && !matches!(
                    relation.operands.as_slice(),
                    [
                        FeatureInputOperand {
                            kind: FeatureInputOperandKind::Native(NativeOperandTag::TAG_8DDA),
                            ..
                        },
                        FeatureInputOperand {
                            kind: FeatureInputOperandKind::Native(NativeOperandTag::TAG_8DDA),
                            ..
                        }
                    ]
                )
        }
        FeatureInputRelationFamily::CircleDiameter => false,
        family => {
            !relation_signature(family, &relation.operands)
                && dynamic_relation_signature(family, &relation.operands)
        }
    }
}

fn relation_target_value(
    ctx: &DecodeContext<'_>,
    relation: &FeatureInputRelationInstance,
    lane: &FeatureInputLane,
) -> Result<Option<cadmpeg_ir::scalar::FiniteReal>, CodecError> {
    let scalar_id = relation
        .parameter_scalar_ref()
        .or(relation.display_scalar_ref());
    let Some(scalar_id) = scalar_id else {
        return Ok(None);
    };
    for scalar in ctx.admit_iter(&lane.scalars, "find SLDPRT relation target scalar")? {
        if ctx.equal(
            scalar.id.as_str(),
            scalar_id,
            "compare SLDPRT relation target scalar references",
        )? {
            return Ok(Some(scalar.value));
        }
    }
    Ok(None)
}

fn feature_entities<'a>(
    ctx: &DecodeContext<'_>,
    lane: &'a FeatureInputLane,
    feature: &str,
) -> Result<Vec<&'a crate::records::SketchInputEntity>, CodecError> {
    let mut entities = Vec::new();
    for entity in ctx.admit_iter(
        &lane.sketch_entities,
        "scan SLDPRT relation feature entities",
    )? {
        if let Some(entity_feature) = entity.feature_ref.as_deref() {
            if ctx.equal(
                entity_feature,
                feature,
                "match SLDPRT relation feature entities",
            )? {
                ctx.push_vec(
                    &mut entities,
                    entity,
                    "collect SLDPRT relation feature entities",
                )?;
            }
        }
    }
    ctx.sort_unstable_by_key(
        &mut entities,
        |value| (value.offset(), value.ordinal()),
        Ord::cmp,
        "sort SLDPRT relation feature entities",
    )?;
    Ok(entities)
}

fn is_finite_point(
    ctx: &DecodeContext<'_>,
    entity: &crate::records::SketchInputEntity,
) -> Result<bool, CodecError> {
    let is_point = matches!(
        entity.kind(),
        crate::records::SketchInputKind::Point | crate::records::SketchInputKind::ConstrainedPoint
    );
    let Some(coordinates) = entity
        .coordinates_m
        .map(cadmpeg_ir::units::FiniteVector::get)
    else {
        return Ok(false);
    };
    Ok(is_point
        && ctx
            .admit_iter(&coordinates, "validate SLDPRT relation point coordinates")?
            .copied()
            .all(f64::is_finite))
}

fn push_point_candidate<'a>(
    ctx: &DecodeContext<'_>,
    candidates: &mut Vec<&'a crate::records::SketchInputEntity>,
    seen: &mut HashSet<&'a str>,
    candidate: Option<&'a crate::records::SketchInputEntity>,
) -> Result<(), CodecError> {
    let Some(candidate) = candidate else {
        return Ok(());
    };
    if !is_finite_point(ctx, candidate)? {
        return Ok(());
    }
    if ctx.contains_hash_set(seen, candidate.id(), "check SLDPRT relation point identity")? {
        return Ok(());
    }
    ctx.insert_hash_set(seen, candidate.id(), "index SLDPRT relation point identity")?;
    ctx.push_vec(
        candidates,
        candidate,
        "collect SLDPRT dynamic point candidates",
    )?;
    Ok(())
}

fn dynamic_point_candidates<'a>(
    ctx: &DecodeContext<'_>,
    entities: &[&'a crate::records::SketchInputEntity],
    operand: &FeatureInputOperand,
    use_explicit: bool,
) -> Result<Vec<&'a crate::records::SketchInputEntity>, CodecError> {
    let address = usize::from(operand.entity_index);
    if let Some(entity_ref) = operand.entity_ref.as_deref().filter(|_| use_explicit) {
        let mut candidates = Vec::new();
        for entity in ctx.admit_iter(entities, "find SLDPRT explicit point references")? {
            if !ctx.equal(
                entity.id(),
                entity_ref,
                "compare SLDPRT explicit point references",
            )? || !is_finite_point(ctx, entity)?
            {
                continue;
            }
            ctx.push_vec(
                &mut candidates,
                *entity,
                "collect SLDPRT explicit point references",
            )?;
        }
        return Ok(candidates);
    }
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();
    push_point_candidate(
        ctx,
        &mut candidates,
        &mut seen,
        entities.get(address).copied(),
    )?;
    let mut finite_index = 0usize;
    let mut coordinate_point = None;
    for entity in ctx.admit_iter(entities, "find SLDPRT coordinate point candidate")? {
        if !is_finite_point(ctx, entity)? {
            continue;
        }
        if finite_index == address {
            coordinate_point = Some(*entity);
            break;
        }
        finite_index += 1;
    }
    push_point_candidate(ctx, &mut candidates, &mut seen, coordinate_point)?;
    for entity in ctx.admit_iter(entities, "match SLDPRT dynamic point identities")? {
        if entity.object_index() == Some(u32::from(operand.entity_index))
            || entity.local_id() == Some(u32::from(operand.entity_index))
        {
            push_point_candidate(ctx, &mut candidates, &mut seen, Some(entity))?;
        }
    }
    ctx.sort_unstable_by(
        &mut candidates,
        |value| value.id(),
        Ord::cmp,
        "sort SLDPRT dynamic point candidates",
    )?;
    Ok(candidates)
}

fn dynamic_solver_line<'a>(
    ctx: &DecodeContext<'_>,
    entities: &[&'a crate::records::SketchInputEntity],
    index: u16,
) -> Result<Option<[&'a crate::records::SketchInputEntity; 2]>, CodecError> {
    let Some(start) = usize::from(index).checked_mul(2) else {
        return Ok(None);
    };
    let Some(second_index) = start.checked_add(1) else {
        return Ok(None);
    };
    let mut first = None;
    let mut second = None;
    let mut point_index = 0usize;
    for entity in ctx.admit_iter(entities, "select SLDPRT relation line points")? {
        if !is_finite_point(ctx, entity)? {
            continue;
        }
        if point_index == start {
            first = Some(*entity);
        } else if point_index == second_index {
            second = Some(*entity);
            break;
        }
        point_index += 1;
    }
    let (Some(first), Some(second)) = (first, second) else {
        return Ok(None);
    };
    let same_coordinates = first
        .coordinates_m
        .zip(second.coordinates_m)
        .is_some_and(|(first, second)| first.get() == second.get());
    Ok((!same_coordinates).then_some([first, second]))
}

fn point_distance(first: [f64; 2], second: [f64; 2]) -> f64 {
    (second[0] - first[0]).hypot(second[1] - first[1])
}

fn axis_distance(first: [f64; 2], second: [f64; 2], horizontal: bool) -> f64 {
    if horizontal {
        (second[0] - first[0]).abs()
    } else {
        (second[1] - first[1]).abs()
    }
}

fn line_direction(line: [[f64; 2]; 2]) -> [f64; 2] {
    [line[1][0] - line[0][0], line[1][1] - line[0][1]]
}

pub(super) fn line_line_distance(first: [[f64; 2]; 2], second: [[f64; 2]; 2]) -> Option<f64> {
    let first_direction = line_direction(first);
    let second_direction = line_direction(second);
    let unit = |direction: [f64; 2]| {
        let v = cadmpeg_ir::math::Vector3::new(direction[0], direction[1], 0.0);
        (v.norm() > SKETCH_POINT_TOLERANCE)
            .then(|| {
                cadmpeg_ir::features::FiniteVector3::new(v)
                    .and_then(cadmpeg_ir::features::FiniteVector3::unit_nonzero)
            })
            .flatten()
    };
    let a = unit(first_direction)?;
    let b = unit(second_direction)?;
    if (a.x * b.y - a.y * b.x).abs() > SKETCH_POINT_TOLERANCE {
        return None;
    }
    let distance = ((second[0][0] - first[0][0]) * a.y - (second[0][1] - first[0][1]) * a.x).abs();
    distance.is_finite().then_some(distance)
}

pub(super) fn line_line_angle(first: [[f64; 2]; 2], second: [[f64; 2]; 2]) -> Option<f64> {
    let first = line_direction(first);
    let second = line_direction(second);
    let first = cadmpeg_ir::math::Vector3::new(first[0], first[1], 0.0);
    let second = cadmpeg_ir::math::Vector3::new(second[0], second[1], 0.0);
    if first.norm() <= SKETCH_POINT_TOLERANCE || second.norm() <= SKETCH_POINT_TOLERANCE {
        return None;
    }
    let first = cadmpeg_ir::features::FiniteVector3::new(first)?.unit_nonzero()?;
    let second = cadmpeg_ir::features::FiniteVector3::new(second)?.unit_nonzero()?;
    Some(first.cross(second).norm().atan2(first.dot(second)))
}

fn dynamic_line_line_angle(first: [[f64; 2]; 2], second: [[f64; 2]; 2]) -> Option<f64> {
    let angle = line_line_angle(first, second)?;
    Some(angle.min(std::f64::consts::PI - angle))
}

fn same_relation_dimension(left: f64, right: f64) -> bool {
    (left - right).abs() <= SKETCH_POINT_TOLERANCE * left.abs().max(right.abs()).max(1.0)
}

fn clear_relation_operands(
    ctx: &DecodeContext<'_>,
    relation: &mut FeatureInputRelationInstance,
) -> Result<(), CodecError> {
    for operand_index in ctx.admit_iter(
        &(0..relation.operands.len()),
        "clear SLDPRT relation operands",
    )? {
        relation.operands[operand_index].entity_ref = None;
    }
    Ok(())
}

fn dynamic_curve_reference_is_valid(
    ctx: &DecodeContext<'_>,
    entities: &[&crate::records::SketchInputEntity],
    entity_ref: Option<&str>,
) -> Result<bool, CodecError> {
    let Some(entity_ref) = entity_ref else {
        return Ok(false);
    };
    for entity in ctx.admit_iter(entities, "find SLDPRT relation curve reference")? {
        if ctx.equal(
            entity.id(),
            entity_ref,
            "compare SLDPRT relation curve references",
        )? && matches!(
            entity.kind(),
            crate::records::SketchInputKind::LineOrCircle
                | crate::records::SketchInputKind::Arc
                | crate::records::SketchInputKind::Relation(_)
        ) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn dynamic_point_matches<'a>(
    ctx: &DecodeContext<'_>,
    first_candidates: &[&'a crate::records::SketchInputEntity],
    second_candidates: &[&'a crate::records::SketchInputEntity],
    target: f64,
    horizontal: Option<bool>,
) -> Result<Vec<(&'a str, &'a str)>, CodecError> {
    let mut matches = Vec::<(&str, &str)>::new();
    for first in ctx.admit_iter(first_candidates, "match SLDPRT dynamic relation points")? {
        let Some(first_coordinates) = first
            .coordinates_m
            .map(cadmpeg_ir::units::FiniteVector::get)
        else {
            continue;
        };
        for second in ctx.admit_iter(second_candidates, "match SLDPRT dynamic relation points")? {
            if ctx.equal(
                first.id(),
                second.id(),
                "compare SLDPRT dynamic relation point identities",
            )? {
                continue;
            }
            let Some(second_coordinates) = second
                .coordinates_m
                .map(cadmpeg_ir::units::FiniteVector::get)
            else {
                continue;
            };
            let measured = horizontal.map_or_else(
                || point_distance(first_coordinates, second_coordinates),
                |horizontal| axis_distance(first_coordinates, second_coordinates, horizontal),
            );
            if same_relation_dimension(measured, target) {
                ctx.reserve_vec(&mut matches, 1, "collect SLDPRT dynamic point matches")?;
                matches.push((first.id(), second.id()));
            }
        }
    }
    Ok(matches)
}

fn bind_dynamic_point_relation(
    ctx: &DecodeContext<'_>,
    relation: &mut FeatureInputRelationInstance,
    entities: &[&crate::records::SketchInputEntity],
    target: f64,
    horizontal: Option<bool>,
) -> Result<(), CodecError> {
    let mut temporary_storage =
        ctx.reserve_scoped(0, "SLDPRT relation_records temporary storage")?;

    let [first, second] = relation.operands.as_slice() else {
        clear_relation_operands(ctx, relation)?;
        return Ok(());
    };
    let first_candidates =
        temporary_storage.with_storage(|| dynamic_point_candidates(ctx, entities, first, true))?;
    let second_candidates =
        temporary_storage.with_storage(|| dynamic_point_candidates(ctx, entities, second, true))?;
    let mut coordinate_points = Vec::new();
    for entity in ctx.admit_iter(entities, "collect SLDPRT relation coordinate points")? {
        if is_finite_point(ctx, entity)? {
            temporary_storage.with_storage(|| {
                ctx.push_vec(
                    &mut coordinate_points,
                    *entity,
                    "collect SLDPRT relation coordinate points",
                )
            })?;
        }
    }
    let mut matches = dynamic_point_matches(
        ctx,
        &first_candidates,
        &second_candidates,
        target,
        horizontal,
    )?;
    if matches.is_empty() {
        let first_fallback = if first.entity_ref.is_none() {
            &coordinate_points
        } else {
            &first_candidates
        };
        let second_fallback = if second.entity_ref.is_none() {
            &coordinate_points
        } else {
            &second_candidates
        };
        matches = dynamic_point_matches(ctx, first_fallback, second_fallback, target, horizontal)?;
    }
    if matches.is_empty() {
        let first_relaxed = if first.entity_ref.is_some() {
            Some(
                temporary_storage
                    .with_storage(|| dynamic_point_candidates(ctx, entities, first, false))?,
            )
        } else {
            None
        };
        let second_relaxed = if second.entity_ref.is_some() {
            Some(
                temporary_storage
                    .with_storage(|| dynamic_point_candidates(ctx, entities, second, false))?,
            )
        } else {
            None
        };
        matches = dynamic_point_matches(
            ctx,
            first_relaxed.as_deref().unwrap_or(&first_candidates),
            second_relaxed.as_deref().unwrap_or(&second_candidates),
            target,
            horizontal,
        )?;
    }
    if matches.is_empty() {
        let first_relaxed = if first.entity_ref.is_some() {
            &coordinate_points
        } else {
            &first_candidates
        };
        let second_relaxed = if second.entity_ref.is_some() {
            &coordinate_points
        } else {
            &second_candidates
        };
        matches = dynamic_point_matches(ctx, first_relaxed, second_relaxed, target, horizontal)?;
    }
    ctx.sort_unstable_by(
        &mut matches,
        |value| value,
        Ord::cmp,
        "sort SLDPRT dynamic point matches",
    )?;
    ctx.dedup_vec(&mut matches, "deduplicate SLDPRT dynamic point matches")?;
    if let [(first, second)] = matches.as_slice() {
        relation.operands[0].entity_ref = Some(copy_relation_text(ctx, first)?);
        relation.operands[1].entity_ref = Some(copy_relation_text(ctx, second)?);
    } else {
        clear_relation_operands(ctx, relation)?;
    }
    Ok(())
}

fn bind_dynamic_point_line_relation(
    ctx: &DecodeContext<'_>,
    relation: &mut FeatureInputRelationInstance,
    entities: &[&crate::records::SketchInputEntity],
    target: f64,
) -> Result<(), CodecError> {
    let mut temporary_storage =
        ctx.reserve_scoped(0, "SLDPRT relation_records temporary storage")?;

    let Ok([point_operand, line_operand]) =
        <&mut [FeatureInputOperand; 2]>::try_from(relation.operands.as_mut_slice())
    else {
        clear_relation_operands(ctx, relation)?;
        return Ok(());
    };
    if dynamic_curve_reference_is_valid(ctx, entities, line_operand.entity_ref.as_deref())? {
        return Ok(());
    }
    line_operand.entity_ref = None;
    let line_index = line_operand.entity_index;
    let point_candidates = temporary_storage
        .with_storage(|| dynamic_point_candidates(ctx, entities, point_operand, true))?;
    let Some(line_markers) = dynamic_solver_line(ctx, entities, line_index)? else {
        clear_relation_operands(ctx, relation)?;
        return Ok(());
    };
    let [Some(first), Some(second)] = line_markers.map(|marker| marker.coordinates_m) else {
        clear_relation_operands(ctx, relation)?;
        return Ok(());
    };
    let direction = [second[0] - first[0], second[1] - first[1]];
    let length = direction[0].hypot(direction[1]);
    if length <= SKETCH_POINT_TOLERANCE {
        clear_relation_operands(ctx, relation)?;
        return Ok(());
    }
    let mut matches = Vec::new();
    for point in ctx.admit_iter(
        &point_candidates,
        "match SLDPRT dynamic point-line candidates",
    )? {
        let Some(coordinates) = point
            .coordinates_m
            .map(cadmpeg_ir::units::FiniteVector::get)
        else {
            continue;
        };
        let measured = ((coordinates[0] - first[0]) * direction[1]
            - (coordinates[1] - first[1]) * direction[0])
            .abs()
            / length;
        if same_relation_dimension(measured, target) {
            temporary_storage.with_storage(|| {
                ctx.push_vec(
                    &mut matches,
                    point.id(),
                    "collect SLDPRT dynamic point-line matches",
                )
            })?;
        }
    }
    ctx.sort_unstable_by(
        &mut matches,
        |value| value,
        Ord::cmp,
        "sort SLDPRT dynamic point-line matches",
    )?;
    ctx.dedup_vec(
        &mut matches,
        "deduplicate SLDPRT dynamic point-line matches",
    )?;
    if let [point] = matches.as_slice() {
        relation.operands[0].entity_ref = Some(copy_relation_text(ctx, point)?);
        relation.operands[1].entity_ref = None;
    } else {
        clear_relation_operands(ctx, relation)?;
    }
    Ok(())
}

fn bind_dynamic_line_relation(
    ctx: &DecodeContext<'_>,
    relation: &mut FeatureInputRelationInstance,
    entities: &[&crate::records::SketchInputEntity],
    target: f64,
    angle: bool,
) -> Result<(), CodecError> {
    let Ok([first_operand, second_operand]) =
        <&mut [FeatureInputOperand; 2]>::try_from(relation.operands.as_mut_slice())
    else {
        clear_relation_operands(ctx, relation)?;
        return Ok(());
    };
    let first_valid =
        dynamic_curve_reference_is_valid(ctx, entities, first_operand.entity_ref.as_deref())?;
    let second_valid =
        dynamic_curve_reference_is_valid(ctx, entities, second_operand.entity_ref.as_deref())?;
    if !first_valid {
        first_operand.entity_ref = None;
    }
    if !second_valid {
        second_operand.entity_ref = None;
    }
    if first_valid || second_valid {
        return Ok(());
    }
    let (first_index, second_index) = (first_operand.entity_index, second_operand.entity_index);
    let Some(first_markers) = dynamic_solver_line(ctx, entities, first_index)? else {
        clear_relation_operands(ctx, relation)?;
        return Ok(());
    };
    let Some(second_markers) = dynamic_solver_line(ctx, entities, second_index)? else {
        clear_relation_operands(ctx, relation)?;
        return Ok(());
    };
    if first_operand.entity_index == second_operand.entity_index {
        clear_relation_operands(ctx, relation)?;
        return Ok(());
    }
    let [Some(first_line_first), Some(first_line_second)] = first_markers.map(|marker| {
        marker
            .coordinates_m
            .map(cadmpeg_ir::units::FiniteVector::get)
    }) else {
        clear_relation_operands(ctx, relation)?;
        return Ok(());
    };
    let [Some(second_line_first), Some(second_line_second)] = second_markers.map(|marker| {
        marker
            .coordinates_m
            .map(cadmpeg_ir::units::FiniteVector::get)
    }) else {
        clear_relation_operands(ctx, relation)?;
        return Ok(());
    };
    let measured = if angle {
        dynamic_line_line_angle(
            [first_line_first, first_line_second],
            [second_line_first, second_line_second],
        )
    } else {
        line_line_distance(
            [first_line_first, first_line_second],
            [second_line_first, second_line_second],
        )
    };
    if measured.is_some_and(|measured| same_relation_dimension(measured, target)) {
        relation.operands[0].entity_ref = None;
        relation.operands[1].entity_ref = None;
    } else {
        clear_relation_operands(ctx, relation)?;
    }
    Ok(())
}

fn bind_relation_geometry_operands(
    ctx: &DecodeContext<'_>,
    relations: &mut [FeatureInputRelationInstance],
    lane: &FeatureInputLane,
) -> Result<(), CodecError> {
    let mut temporary_storage =
        ctx.reserve_scoped(0, "SLDPRT relation_records temporary storage")?;

    for relation_index in ctx.admit_iter(
        &(0..relations.len()),
        "scan SLDPRT relation geometry operands",
    )? {
        let relation = &mut relations[relation_index];
        let dynamic = relation_uses_dynamic_operands(relation);
        let point_operands_unbound = if !dynamic
            && matches!(
                relation.family,
                FeatureInputRelationFamily::PointPointDistance
                    | FeatureInputRelationFamily::PointPointHorizontalDistance
                    | FeatureInputRelationFamily::PointPointVerticalDistance
            ) {
            ctx.admit_iter(
                &relation.operands,
                "check SLDPRT unbound point relation operands",
            )?
            .all(|operand| operand.entity_ref.is_none())
        } else {
            false
        };
        if !dynamic && !point_operands_unbound {
            continue;
        }
        if relation.family == FeatureInputRelationFamily::CircleDiameter {
            continue;
        }
        let Some(target) = relation_target_value(ctx, relation, lane)? else {
            if dynamic {
                clear_relation_operands(ctx, relation)?;
            }
            continue;
        };
        if relation_uses_solver_points(ctx, relation)? {
            continue;
        }
        if target.get() < 0.0 {
            if dynamic {
                clear_relation_operands(ctx, relation)?;
            }
            continue;
        }
        let entities = temporary_storage
            .with_storage(|| feature_entities(ctx, lane, relation.feature_ref.as_str()))?;
        match relation.family {
            FeatureInputRelationFamily::PointPointDistance => {
                bind_dynamic_point_relation(ctx, relation, &entities, target.get(), None)?;
            }
            FeatureInputRelationFamily::PointPointHorizontalDistance => {
                bind_dynamic_point_relation(ctx, relation, &entities, target.get(), Some(true))?;
            }
            FeatureInputRelationFamily::PointPointVerticalDistance => {
                bind_dynamic_point_relation(ctx, relation, &entities, target.get(), Some(false))?;
            }
            FeatureInputRelationFamily::PointLineDistance => {
                bind_dynamic_point_line_relation(ctx, relation, &entities, target.get())?;
            }
            FeatureInputRelationFamily::LineLineDistance => {
                bind_dynamic_line_relation(ctx, relation, &entities, target.get(), false)?;
            }
            FeatureInputRelationFamily::Angle => {
                bind_dynamic_line_relation(ctx, relation, &entities, target.get(), true)?;
            }
            FeatureInputRelationFamily::CircleDiameter => {}
        }
    }
    Ok(())
}

pub(super) fn scalar_role(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    trailer_offset: usize,
) -> Result<FeatureInputScalarRole, CodecError> {
    let shifted_layout = shifted_value_only_scalar_trailer(ctx, payload, trailer_offset)?;
    let fixed_prefix = payload.get(trailer_offset..trailer_offset + 3) == Some(&[0, 0, 0]);
    let fixed_zero_fields = if fixed_prefix {
        payload
            .get(trailer_offset + 7..trailer_offset + 21)
            .map(|bytes| -> Result<bool, CodecError> {
                Ok(ctx
                    .admit_iter(bytes, "validate SLDPRT scalar layout")?
                    .all(|byte| *byte == 0))
            })
            .transpose()?
            .unwrap_or(false)
    } else {
        false
    };
    let fixed_layout = fixed_prefix
        && fixed_zero_fields
        && payload.get(trailer_offset + 24..trailer_offset + 29) == Some(&[0, 0, 0, 2, 0]);
    let role_offset = if shifted_layout {
        trailer_offset + shifted_trailer::ROLE
    } else if compact_scalar_layout(ctx, payload, trailer_offset)? {
        trailer_offset + 27
    } else if fixed_layout {
        trailer_offset + 29
    } else if legacy_scalar_layout(ctx, payload, trailer_offset)? {
        trailer_offset + 30
    } else {
        return Ok(FeatureInputScalarRole::Native);
    };
    Ok(match payload.get(role_offset) {
        Some(0) => FeatureInputScalarRole::Driving,
        Some(1) => FeatureInputScalarRole::Display,
        _ => FeatureInputScalarRole::Native,
    })
}

pub(super) fn shifted_value_only_scalar_trailer(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    trailer_offset: usize,
) -> Result<bool, CodecError> {
    if payload.get(
        trailer_offset + shifted_trailer::ZERO_PREFIX..trailer_offset + shifted_trailer::OBJECT_ID,
    ) != Some(&[0, 0, 0])
    {
        return Ok(false);
    }
    let Some(zero_object_tail) = payload.get(
        trailer_offset + shifted_trailer::ZERO_OBJECT_TAIL
            ..trailer_offset + shifted_trailer::LAYOUT_MARKER,
    ) else {
        return Ok(false);
    };
    if !ctx
        .admit_iter(zero_object_tail, "validate SLDPRT shifted scalar trailer")?
        .all(|byte| *byte == 0)
        || payload.get(
            trailer_offset + shifted_trailer::LAYOUT_MARKER..trailer_offset + shifted_trailer::ROLE,
        ) != Some(&[1, 0, 0, 0, 2, 0])
        || payload
            .get(trailer_offset + shifted_trailer::ROLE)
            .is_none_or(|role| *role > 1)
    {
        return Ok(false);
    }
    let Some(zero_tail) = payload
        .get(trailer_offset + shifted_trailer::ZERO_TAIL..trailer_offset + shifted_trailer::LEN)
    else {
        return Ok(false);
    };
    Ok(ctx
        .admit_iter(zero_tail, "validate SLDPRT shifted scalar trailer")?
        .all(|byte| *byte == 0))
}

fn shifted_value_only_scalar_layout(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    trailer_offset: usize,
) -> Result<bool, CodecError> {
    if !shifted_value_only_scalar_trailer(ctx, payload, trailer_offset)? {
        return Ok(false);
    }
    Ok(payload
        .get(trailer_offset + 35..trailer_offset + 47)
        .is_some_and(|cell| {
            cell[0..2] != [0, 0]
                && cell[0..2] != [0xff, 0xff]
                && cell[4..8] == [0xff; 4]
                && cell[8..12] == [0; 4]
        }))
}

pub(super) fn compact_scalar_layout(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    trailer_offset: usize,
) -> Result<bool, CodecError> {
    if shifted_value_only_scalar_layout(ctx, payload, trailer_offset)?
        || payload.get(trailer_offset..trailer_offset + 3) != Some(&[0, 0, 0])
    {
        return Ok(false);
    }
    let Some(first_zero_field) = payload.get(trailer_offset + 7..trailer_offset + 21) else {
        return Ok(false);
    };
    if !ctx
        .admit_iter(first_zero_field, "validate SLDPRT compact scalar layout")?
        .all(|byte| *byte == 0)
        || payload.get(trailer_offset + 21..trailer_offset + 27) != Some(&[1, 0, 0, 0, 2, 0])
    {
        return Ok(false);
    }
    let Some(second_zero_field) = payload.get(trailer_offset + 28..trailer_offset + 35) else {
        return Ok(false);
    };
    Ok(ctx
        .admit_iter(second_zero_field, "validate SLDPRT compact scalar layout")?
        .all(|byte| *byte == 0)
        && payload.get(trailer_offset + 39..trailer_offset + 43) == Some(&[0xff; 4])
        && payload.get(trailer_offset + 47..trailer_offset + 51) == Some(&[0xff; 4]))
}

pub(super) fn legacy_scalar_layout(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    trailer_offset: usize,
) -> Result<bool, CodecError> {
    if payload.get(trailer_offset..trailer_offset + 3) != Some(&[0, 0, 0]) {
        return Ok(false);
    }
    let Some(zero_field) = payload.get(trailer_offset + 7..trailer_offset + 24) else {
        return Ok(false);
    };
    Ok(ctx
        .admit_iter(zero_field, "validate SLDPRT legacy scalar layout")?
        .all(|byte| *byte == 0)
        && payload.get(trailer_offset + 24..trailer_offset + 30) == Some(&[0x0f, 0, 0, 0, 2, 0]))
}

#[cfg(test)]
mod binary_relation_operand_tests {
    #[test]
    fn large_perpendicular_lines_have_no_parallel_distance() {
        assert_eq!(
            super::line_line_distance([[0., 0.], [1e200, 0.]], [[0., 1.], [0., 1e200]]),
            None
        );
        assert_eq!(
            super::line_line_distance([[0., 0.], [1e200, 0.]], [[0., 1.], [1e200, 1.]]),
            Some(1.)
        );
    }

    use super::{
        bind_dynamic_line_relation, bind_dynamic_point_line_relation, FeatureInputOperand,
        FeatureInputOperandKind, FeatureInputRelationFamily, FeatureInputRelationInstance,
    };

    fn operand(entity_index: u16) -> FeatureInputOperand {
        FeatureInputOperand {
            offset: u64::from(entity_index),
            reference_ref: format!("reference#{entity_index}"),
            kind: FeatureInputOperandKind::D6,
            entity_index,
            entity_ref: Some(format!("entity#{entity_index}")),
        }
    }

    fn relation(operands: Vec<FeatureInputOperand>) -> FeatureInputRelationInstance {
        FeatureInputRelationInstance {
            id: "relation#0".to_string(),
            parent: "lane#0".to_string(),
            ordinal: 0,
            offset: 0,
            family: FeatureInputRelationFamily::PointPointDistance,
            class_ref: "class#0".to_string(),
            feature_ref: "feature#0".to_string(),
            scalars: crate::records::relation_scalars::RelationScalars::from_refs(
                vec!["scalar#0".into()],
                None,
                None,
            )
            .unwrap(),
            operands,
        }
    }

    #[test]
    fn a_relation_that_is_not_binary_is_cleared_rather_than_indexed() {
        for operands in [
            vec![],
            vec![operand(0)],
            vec![operand(0), operand(1), operand(2)],
        ] {
            let mut point_line = relation(operands.clone());
            bind_dynamic_point_line_relation(
                &cadmpeg_test_support::service_decode_context(),
                &mut point_line,
                &[],
                1.0,
            )
            .unwrap();
            assert!(point_line
                .operands
                .iter()
                .all(|operand| operand.entity_ref.is_none()));

            let mut line = relation(operands);
            bind_dynamic_line_relation(
                &cadmpeg_test_support::service_decode_context(),
                &mut line,
                &[],
                1.0,
                false,
            )
            .unwrap();
            assert!(line
                .operands
                .iter()
                .all(|operand| operand.entity_ref.is_none()));
        }
    }
}

#[cfg(test)]
mod relation_records_tests {
    #[test]
    fn line_angles_retain_shallow_and_near_opposite_angles() {
        const ANGLE: f64 = 1e-8;
        let first = [[0.0, 0.0], [1.0, 0.0]];
        let second = [[0.0, 0.0], [ANGLE.cos(), ANGLE.sin()]];
        assert!(
            (super::line_line_angle(first, second).unwrap() - ANGLE).abs() <= f64::EPSILON * ANGLE
        );
        let opposite = [[0.0, 0.0], [-ANGLE.cos(), ANGLE.sin()]];
        assert!(
            (super::line_line_angle(first, opposite).unwrap() - (std::f64::consts::PI - ANGLE))
                .abs()
                <= f64::EPSILON * std::f64::consts::PI
        );
    }

    #[test]
    fn numerical_audit_line_angles_are_finite_for_large_directions() {
        let x = [[0.0, 0.0], [1.0e200, 0.0]];
        let y = [[0.0, 0.0], [0.0, 1.0e200]];
        assert_eq!(super::line_line_angle(x, x), Some(0.0));
        assert_eq!(
            super::line_line_angle(x, y),
            Some(std::f64::consts::FRAC_PI_2)
        );
        assert_eq!(super::dynamic_line_line_angle(x, x), Some(0.0));
    }

    use super::{
        circle_dimension_handle_driver, feature_intervals, is_solver_point_operand,
        relation_declaration_candidates, relation_instances, relation_signature,
        relation_uses_dynamic_operands, UNKNOWN_FEATURE_SPAN,
    };
    use crate::records::operand_tag::NativeOperandTag;
    use crate::records::FeatureInputOperand;
    use crate::records::FeatureInputOperandKind;
    use crate::records::FeatureInputRelationFamily;
    use crate::records::FeatureInputScalar;
    use crate::records::FeatureInputScalarRole;
    use crate::records::FeatureSource;
    use crate::records::{
        Feature, FeatureHistory, FeatureInputClass, FeatureInputLane, FeatureInputName,
        FeatureInputRelationBinding,
    };
    use std::collections::BTreeMap;

    fn class(offset: u64, name: &str) -> FeatureInputClass {
        FeatureInputClass {
            id: format!("class-{offset}"),
            parent: "lane".into(),
            ordinal: 0,
            offset,
            name: name.into(),
        }
    }

    fn scalar(offset: u64, role: FeatureInputScalarRole) -> FeatureInputScalar {
        let operands = [0_u16, 1]
            .into_iter()
            .enumerate()
            .map(|(ordinal, entity_index)| FeatureInputOperand {
                offset: offset + cadmpeg_core::decode::u64_from_index(ordinal),
                reference_ref: format!("reference-{offset}-{ordinal}"),
                kind: FeatureInputOperandKind::Native(NativeOperandTag::TAG_8152),
                entity_index,
                entity_ref: None,
            })
            .collect();
        FeatureInputScalar {
            id: format!("scalar-{offset}"),
            parent: "lane".into(),
            feature_ref: Some("sketch".into()),
            ordinal: 0,
            offset,
            object_id: 1,
            name: "dimension".into(),
            value: cadmpeg_ir::scalar::FiniteReal::new(1.0).expect("finite test scalar"),
            role,

            operands,
        }
    }

    fn circle_scalar(
        offset: u64,
        name: &str,
        role: FeatureInputScalarRole,
        entity_ref: Option<&str>,
    ) -> FeatureInputScalar {
        FeatureInputScalar {
            id: format!("scalar-{offset}"),
            parent: "lane".into(),
            feature_ref: Some("sketch".into()),
            ordinal: 0,
            offset,
            object_id: 1,
            name: name.into(),
            value: cadmpeg_ir::scalar::FiniteReal::new(1.0).expect("finite test scalar"),
            role,

            operands: vec![FeatureInputOperand {
                offset: offset + 1,
                reference_ref: format!("reference-{offset}"),
                kind: FeatureInputOperandKind::Native(NativeOperandTag::try_from(0x1234).unwrap()),
                entity_index: 0,
                entity_ref: entity_ref.map(str::to_owned),
            }],
        }
    }

    fn lane(classes: Vec<FeatureInputClass>, scalars: Vec<FeatureInputScalar>) -> FeatureInputLane {
        FeatureInputLane {
            id: "lane".into(),
            configuration: None,
            native_payload: Vec::new(),
            classes,
            names: Vec::new(),
            scalars,
            relation_bindings: Vec::new(),
            relation_instances: Vec::new(),
            body_selections: Vec::new(),
            edge_selections: Vec::new(),
            surface_selections: Vec::new(),
            generated_surface_identities: Vec::new(),
            references: Vec::new(),
            sketch_entities: Vec::new(),
        }
    }

    fn sketch_history() -> Vec<FeatureHistory> {
        vec![FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![Feature {
                id: "sketch".into(),
                parent: "history".into(),
                xml_tag: "Sketch".into(),
                tree_parent: None,
                source_id: None,
                ordinal: 0,
                name: "Sketch".into(),
                kind: "Sketch".into(),
                input_class: None,
                suppressed: false,
                parameters: BTreeMap::new(),
                dimension_properties: BTreeMap::new(),
                properties: BTreeMap::new(),
                text: None,
                content: Vec::new(),
            }],
        }]
    }

    #[test]
    fn native_relation_tags_are_scoped_to_the_declared_family() {
        let operand_pair = |kind| {
            [0_u16, 1]
                .into_iter()
                .enumerate()
                .map(|(ordinal, entity_index)| FeatureInputOperand {
                    offset: cadmpeg_core::decode::u64_from_index(ordinal),
                    reference_ref: format!("reference-{ordinal}"),
                    kind,
                    entity_index,
                    entity_ref: None,
                })
                .collect::<Vec<_>>()
        };
        let point_tagged =
            operand_pair(FeatureInputOperandKind::Native(NativeOperandTag::TAG_80D5));
        let line_tagged = operand_pair(FeatureInputOperandKind::Native(NativeOperandTag::TAG_810F));
        let roster_point_tagged =
            operand_pair(FeatureInputOperandKind::Native(NativeOperandTag::TAG_81DD));
        let roster_line_tagged =
            operand_pair(FeatureInputOperandKind::Native(NativeOperandTag::TAG_81E7));

        assert!(relation_signature(
            FeatureInputRelationFamily::PointPointDistance,
            &point_tagged
        ));
        assert!(relation_signature(
            FeatureInputRelationFamily::PointPointHorizontalDistance,
            &point_tagged
        ));
        assert!(relation_signature(
            FeatureInputRelationFamily::PointPointVerticalDistance,
            &point_tagged
        ));
        assert!(relation_signature(
            FeatureInputRelationFamily::Angle,
            &point_tagged
        ));
        assert!(relation_signature(
            FeatureInputRelationFamily::LineLineDistance,
            &line_tagged
        ));
        assert!(relation_signature(
            FeatureInputRelationFamily::PointPointDistance,
            &roster_point_tagged
        ));
        assert!(relation_signature(
            FeatureInputRelationFamily::LineLineDistance,
            &roster_line_tagged
        ));
        assert!(relation_signature(
            FeatureInputRelationFamily::PointLineDistance,
            &[
                FeatureInputOperand {
                    kind: FeatureInputOperandKind::Native(NativeOperandTag::TAG_81DD),
                    ..roster_point_tagged[0].clone()
                },
                FeatureInputOperand {
                    kind: FeatureInputOperandKind::Native(NativeOperandTag::TAG_81E7),
                    ..roster_line_tagged[1].clone()
                },
            ]
        ));

        for tag in [0x8138, 0x80ac] {
            let point_distance_tagged =
                operand_pair(FeatureInputOperandKind::Native(tag.try_into().unwrap()));
            assert!(relation_signature(
                FeatureInputRelationFamily::PointPointDistance,
                &point_distance_tagged
            ));
            assert!(!relation_signature(
                FeatureInputRelationFamily::PointPointHorizontalDistance,
                &point_distance_tagged
            ));
            assert!(!relation_signature(
                FeatureInputRelationFamily::PointPointVerticalDistance,
                &point_distance_tagged
            ));
            assert!(!relation_signature(
                FeatureInputRelationFamily::Angle,
                &point_distance_tagged
            ));
        }

        assert!(!relation_signature(
            FeatureInputRelationFamily::PointPointDistance,
            &line_tagged
        ));
        assert!(!relation_signature(
            FeatureInputRelationFamily::LineLineDistance,
            &point_tagged
        ));
        assert!(!relation_signature(
            FeatureInputRelationFamily::PointLineDistance,
            &point_tagged
        ));

        for tag in [0x8100, 0x820f] {
            let solver_point_tagged =
                operand_pair(FeatureInputOperandKind::Native(tag.try_into().unwrap()));
            assert!(relation_signature(
                FeatureInputRelationFamily::PointPointDistance,
                &solver_point_tagged
            ));
            assert!(!relation_signature(
                FeatureInputRelationFamily::PointPointHorizontalDistance,
                &solver_point_tagged
            ));
            assert!(solver_point_tagged
                .iter()
                .all(|operand| is_solver_point_operand(operand.kind)));
        }
    }

    /// A class whose unknown-feature span no `u64` can name states no scope,
    /// so it declares no relation. The control states the same shape at an
    /// offset the span can name and declares one.
    #[test]
    fn a_class_whose_unknown_feature_span_is_unstatable_declares_no_relation() {
        let unstatable = class(u64::MAX - 100, "sgPntPntHorDist");
        let following = scalar(u64::MAX - 50, FeatureInputScalarRole::Driving);
        assert!(relation_declaration_candidates(
            &cadmpeg_test_support::service_decode_context(),
            std::slice::from_ref(&unstatable),
            std::slice::from_ref(&following),
            &[],
        )
        .unwrap()
        .is_empty());

        let stated = class(1_000, "sgPntPntHorDist");
        let within = scalar(1_050, FeatureInputScalarRole::Driving);
        assert_eq!(
            relation_declaration_candidates(
                &cadmpeg_test_support::service_decode_context(),
                std::slice::from_ref(&stated),
                std::slice::from_ref(&within),
                &[],
            )
            .unwrap()
            .len(),
            1
        );
    }

    #[test]
    fn declaration_skips_nearer_incompatible_scalar() {
        let relation_class = class(10, "sgPntPntHorDist");
        let mut native = scalar(20, FeatureInputScalarRole::Native);
        native.operands.clear();

        let driving = scalar(40, FeatureInputScalarRole::Driving);
        let lane = lane(vec![relation_class], vec![native, driving.clone()]);

        let instances = relation_instances(
            &cadmpeg_test_support::service_decode_context(),
            &sketch_history(),
            &lane,
        )
        .unwrap();
        let [relation] = instances.as_slice() else {
            panic!("one relation instance");
        };
        assert_eq!(
            relation.parameter_scalar_ref().map(str::to_owned),
            Some(driving.id.clone())
        );
        assert_eq!(relation.scalar_refs(), vec![driving.id]);
    }

    #[test]
    fn declaration_reaches_compatible_scalar_after_auxiliary_records() {
        let relation_class = class(10, "sgPntPntHorDist");
        let driving = scalar(200, FeatureInputScalarRole::Driving);
        let mut lane = lane(vec![relation_class], vec![driving.clone()]);
        lane.names.push(FeatureInputName {
            id: "name-sketch".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            object_id: None,
            value: "Sketch".into(),
        });

        let instances = relation_instances(
            &cadmpeg_test_support::service_decode_context(),
            &sketch_history(),
            &lane,
        )
        .unwrap();
        let [relation] = instances.as_slice() else {
            panic!("one relation instance");
        };
        assert_eq!(
            relation.parameter_scalar_ref().map(str::to_owned),
            Some(driving.id.clone())
        );
        assert_eq!(relation.scalar_refs(), vec![driving.id]);
    }

    #[test]
    fn declaration_stops_before_the_next_relation_class() {
        let first = class(10, "sgPntPntHorDist");
        let second = class(100, "sgPntPntVertDist");
        let driving = scalar(200, FeatureInputScalarRole::Driving);
        let mut lane = lane(vec![first, second.clone()], vec![driving]);
        lane.names.push(FeatureInputName {
            id: "name-sketch".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            object_id: None,
            value: "Sketch".into(),
        });

        let instances = relation_instances(
            &cadmpeg_test_support::service_decode_context(),
            &sketch_history(),
            &lane,
        )
        .unwrap();
        let [relation] = instances.as_slice() else {
            panic!("one relation instance");
        };
        assert_eq!(relation.class_ref, second.id);
        assert_eq!(
            relation.family,
            FeatureInputRelationFamily::PointPointVerticalDistance
        );
    }

    #[test]
    fn display_scalar_joins_driving_scalar_after_class_declaration() {
        let vertical = class(10, "sgPntPntVertDist");
        let horizontal = class(30, "sgPntPntHorDist");
        let display = scalar(20, FeatureInputScalarRole::Display);
        let driving = scalar(40, FeatureInputScalarRole::Driving);
        let lane = lane(
            vec![vertical, horizontal.clone()],
            vec![display.clone(), driving.clone()],
        );

        let history = sketch_history();
        let instances = relation_instances(
            &cadmpeg_test_support::service_decode_context(),
            &history,
            &lane,
        )
        .unwrap();
        let [relation] = instances.as_slice() else {
            panic!("one relation instance");
        };
        assert_eq!(
            relation.family,
            FeatureInputRelationFamily::PointPointHorizontalDistance
        );
        assert_eq!(relation.class_ref, horizontal.id);
        assert_eq!(
            relation.scalar_refs(),
            vec![display.id.clone(), driving.id.clone()]
        );
        assert_eq!(
            relation.display_scalar_ref().map(str::to_owned),
            Some(display.id)
        );
        assert_eq!(
            relation.parameter_scalar_ref().map(str::to_owned),
            Some(driving.id)
        );
    }

    #[test]
    fn display_scalars_do_not_cross_a_class_declaration() {
        let vertical = class(10, "sgPntPntVertDist");
        let horizontal = class(30, "sgPntPntHorDist");
        let first = scalar(20, FeatureInputScalarRole::Display);
        let second = scalar(40, FeatureInputScalarRole::Display);
        let lane = lane(vec![vertical, horizontal], vec![first, second]);

        let relations = relation_instances(
            &cadmpeg_test_support::service_decode_context(),
            &sketch_history(),
            &lane,
        )
        .unwrap();
        assert_eq!(relations.len(), 2);
        assert_eq!(
            relations
                .iter()
                .map(|relation| relation.family)
                .collect::<Vec<_>>(),
            vec![
                FeatureInputRelationFamily::PointPointVerticalDistance,
                FeatureInputRelationFamily::PointPointHorizontalDistance,
            ]
        );
    }

    #[test]
    fn ambiguous_relation_declarations_leave_scalar_unbound() {
        let distance = class(10, "sgPntPntDist");
        let vertical = class(20, "sgPntPntVertDist");
        let lane = lane(
            vec![distance, vertical],
            vec![scalar(30, FeatureInputScalarRole::Driving)],
        );

        assert!(relation_instances(
            &cadmpeg_test_support::service_decode_context(),
            &sketch_history(),
            &lane
        )
        .unwrap()
        .is_empty());
    }

    /// Two features, so the first interval ends at the second's start. A class
    /// inside the first interval reaches a scalar past its own
    /// unknown-feature span but inside the interval: the scope the class takes
    /// is the interval's end, not the span.
    #[test]
    fn a_class_inside_an_interval_takes_the_interval_end_not_the_unknown_span() {
        let mut history = sketch_history();
        history[0].features[0].id = "first".into();
        history[0].features[0].name = "First".into();
        let mut second = history[0].features[0].clone();
        second.id = "second".into();
        second.name = "Second".into();
        second.ordinal = 1;
        history[0].features.push(second);

        let mut relation_scalar = scalar(150, FeatureInputScalarRole::Driving);
        relation_scalar.feature_ref = Some("first".into());
        let mut lane = lane(
            vec![class(10, "sgPntPntHorDist")],
            vec![relation_scalar.clone()],
        );
        lane.names = vec![
            FeatureInputName {
                id: "name-first".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: 0,
                object_id: None,
                value: "First".into(),
            },
            FeatureInputName {
                id: "name-second".into(),
                parent: "lane".into(),
                ordinal: 1,
                offset: 200,
                object_id: None,
                value: "Second".into(),
            },
        ];

        let intervals = feature_intervals(
            &cadmpeg_test_support::service_decode_context(),
            &history,
            &lane,
        )
        .unwrap();
        assert_eq!(
            intervals,
            vec![
                (0, Some(200), "first".to_owned()),
                (200, None, "second".to_owned()),
            ]
        );
        // 150 is past 10 + UNKNOWN_FEATURE_SPAN and inside the interval.
        assert!(relation_scalar.offset > 10 + UNKNOWN_FEATURE_SPAN);
        assert_eq!(
            relation_declaration_candidates(
                &cadmpeg_test_support::service_decode_context(),
                &lane.classes,
                &lane.scalars,
                &intervals
            )
            .unwrap()
            .len(),
            1
        );
    }

    /// The last feature interval is open, so a class inside it is bounded by
    /// nothing and reaches a scalar at the highest offset a `u64` states.
    #[test]
    fn a_class_in_the_open_last_interval_reaches_the_highest_scalar_offset() {
        let history = sketch_history();
        let mut relation_scalar = scalar(10, FeatureInputScalarRole::Driving);
        relation_scalar.offset = u64::MAX;
        let mut lane = lane(
            vec![class(u64::MAX - 1, "sgPntPntHorDist")],
            vec![relation_scalar],
        );
        lane.names = vec![FeatureInputName {
            id: "name-sketch".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            object_id: None,
            value: "Sketch".into(),
        }];

        let intervals = feature_intervals(
            &cadmpeg_test_support::service_decode_context(),
            &history,
            &lane,
        )
        .unwrap();
        assert_eq!(
            relation_declaration_candidates(
                &cadmpeg_test_support::service_decode_context(),
                &lane.classes,
                &lane.scalars,
                &intervals
            )
            .unwrap()
            .len(),
            1,
            "the open last interval bounds no scalar offset"
        );
        assert_eq!(intervals, vec![(0, None, "sketch".to_owned())]);
    }

    #[test]
    fn relation_declarations_do_not_cross_feature_intervals() {
        let mut history = sketch_history();
        history[0].features[0].id = "first".into();
        history[0].features[0].name = "First".into();
        let mut second = history[0].features[0].clone();
        second.id = "second".into();
        second.name = "Second".into();
        second.ordinal = 1;
        history[0].features.push(second);

        let mut relation_scalar = scalar(120, FeatureInputScalarRole::Driving);
        relation_scalar.feature_ref = Some("second".into());
        let mut lane = lane(vec![class(10, "sgPntPntDist")], vec![relation_scalar]);
        lane.names = vec![
            FeatureInputName {
                id: "name-first".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: 0,
                object_id: None,
                value: "First".into(),
            },
            FeatureInputName {
                id: "name-second".into(),
                parent: "lane".into(),
                ordinal: 1,
                offset: 100,
                object_id: None,
                value: "Second".into(),
            },
        ];

        assert!(relation_instances(
            &cadmpeg_test_support::service_decode_context(),
            &history,
            &lane
        )
        .unwrap()
        .is_empty());
    }

    #[test]
    fn metadata_records_do_not_split_relation_feature_intervals() {
        let mut history = sketch_history();
        let mut metadata = history[0].features[0].clone();
        metadata.id = "attribute-definition".into();
        metadata.xml_tag = "Feature".into();
        metadata.source_id = Some(FeatureSource::Reserved);
        metadata.ordinal = 1;
        metadata.name = "Attribute-Definition".into();
        metadata.kind = "Attribute-Definition".into();
        metadata.input_class = None;
        history[0].features.push(metadata);

        let mut relation_scalar = scalar(120, FeatureInputScalarRole::Driving);
        relation_scalar.feature_ref = Some("sketch".into());
        let mut lane = lane(vec![class(110, "sgPntPntDist")], vec![relation_scalar]);
        lane.names = vec![
            FeatureInputName {
                id: "name-sketch".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: 0,
                object_id: None,
                value: "Sketch".into(),
            },
            FeatureInputName {
                id: "name-attribute".into(),
                parent: "lane".into(),
                ordinal: 1,
                offset: 100,
                object_id: None,
                value: "Attribute-Definition".into(),
            },
        ];

        let relations = relation_instances(
            &cadmpeg_test_support::service_decode_context(),
            &history,
            &lane,
        )
        .unwrap();
        let [relation] = relations.as_slice() else {
            panic!("metadata must not terminate the sketch interval");
        };
        assert_eq!(relation.feature_ref, "sketch");
    }

    #[test]
    fn repeated_driving_scalar_starts_another_anchored_relation() {
        let lane = lane(
            vec![class(10, "sgPntPntDist")],
            vec![
                scalar(20, FeatureInputScalarRole::Display),
                scalar(30, FeatureInputScalarRole::Driving),
                scalar(40, FeatureInputScalarRole::Driving),
            ],
        );

        let relations = relation_instances(
            &cadmpeg_test_support::service_decode_context(),
            &sketch_history(),
            &lane,
        )
        .unwrap();
        assert_eq!(relations.len(), 2);
        assert_eq!(
            relations
                .iter()
                .map(|relation| relation.scalar_refs().len())
                .collect::<Vec<_>>(),
            vec![2, 1]
        );
        assert!(relations.iter().all(|relation| {
            relation.family == FeatureInputRelationFamily::PointPointDistance
                && relation.class_ref == "class-10"
        }));
    }

    #[test]
    fn unclaimed_relation_binding_scalar_becomes_an_instance() {
        let dynamic = dynamic_scalar(
            20,
            FeatureInputOperandKind::Native(NativeOperandTag::try_from(0x812a).unwrap()),
            &[1, 2],
            2.0,
        );
        let mut bound = dynamic_scalar(30, FeatureInputOperandKind::E1, &[3, 4], 3.0);
        bound.role = FeatureInputScalarRole::Display;
        let mut lane = lane(vec![class(10, "sgLLDist")], vec![dynamic, bound.clone()]);
        lane.relation_bindings = vec![FeatureInputRelationBinding {
            id: "binding-10".into(),
            parent: lane.id.clone(),
            ordinal: 0,
            offset: 10,
            class_ref: "class-10".into(),
            family: FeatureInputRelationFamily::LineLineDistance,
            scalar_ref: bound.id,
            feature_ref: Some("sketch".into()),
        }];
        lane.relation_bindings.push(FeatureInputRelationBinding {
            id: "binding-11".into(),
            parent: lane.id.clone(),
            ordinal: 1,
            offset: 11,
            class_ref: "class-11".into(),
            family: FeatureInputRelationFamily::LineLineDistance,
            scalar_ref: "scalar-30".into(),
            feature_ref: Some("sketch".into()),
        });

        let instances = relation_instances(
            &cadmpeg_test_support::service_decode_context(),
            &sketch_history(),
            &lane,
        )
        .unwrap();
        assert_eq!(instances.len(), 2);
        assert_eq!(instances[1].offset, 30);
        assert_eq!(
            instances[1].family,
            FeatureInputRelationFamily::LineLineDistance
        );
        assert_eq!(instances[1].scalar_refs(), vec!["scalar-30"]);
        assert_eq!(instances[1].display_scalar_ref(), Some("scalar-30"));
    }

    fn detached_driver_with_duplicate_index_key(
        duplicate_scalar: bool,
    ) -> (
        FeatureInputLane,
        crate::records::FeatureInputRelationInstance,
    ) {
        let mut display = scalar(20, FeatureInputScalarRole::Display);
        display.name = "display-name".into();
        let mut driver = scalar(40, FeatureInputScalarRole::Driving);
        driver.name = "driver-name".into();
        driver.operands.clear();
        let mut last_display = display.clone();
        last_display.name = "last-display-name".into();
        let mut input = lane(
            Vec::new(),
            if duplicate_scalar {
                vec![display.clone(), last_display, driver]
            } else {
                vec![display.clone(), driver]
            },
        );
        let name = |id: &str, value: &str| FeatureInputName {
            id: id.into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            object_id: None,
            value: value.into(),
        };
        input.names = if duplicate_scalar {
            vec![
                name("display-name", "First"),
                name("last-display-name", "Last"),
                name("driver-name", "Last"),
            ]
        } else {
            vec![
                name("display-name", "First"),
                name("display-name", "Last"),
                name("driver-name", "Last"),
            ]
        };
        let relation = crate::records::FeatureInputRelationInstance {
            id: "relation".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            family: FeatureInputRelationFamily::CircleDiameter,
            class_ref: "class".into(),
            feature_ref: "sketch".into(),
            scalars: crate::records::relation_scalars::RelationScalars::from_refs(
                vec![display.id.clone()],
                None,
                Some(display.id),
            )
            .unwrap(),
            operands: Vec::new(),
        };
        (input, relation)
    }

    #[test]
    fn detached_relation_uses_last_duplicate_scalar_index_entry() {
        let (input, relation) = detached_driver_with_duplicate_index_key(true);
        let mut relations = [relation];
        super::bind_detached_relation_drivers(
            &cadmpeg_test_support::service_decode_context(),
            &mut relations,
            &input,
        )
        .unwrap();
        assert_eq!(relations[0].parameter_scalar_ref(), Some("scalar-40"));
    }

    #[test]
    fn detached_relation_uses_last_duplicate_name_index_entry() {
        let (input, relation) = detached_driver_with_duplicate_index_key(false);
        let mut relations = [relation];
        super::bind_detached_relation_drivers(
            &cadmpeg_test_support::service_decode_context(),
            &mut relations,
            &input,
        )
        .unwrap();
        assert_eq!(relations[0].parameter_scalar_ref(), Some("scalar-40"));
    }

    #[test]
    fn circle_dimension_binds_driver_inside_declared_entity_handle() {
        let first = circle_scalar(20, "name-first", FeatureInputScalarRole::Display, None);
        let nested_display =
            circle_scalar(40, "name-nested", FeatureInputScalarRole::Display, None);
        let nested_driver = circle_scalar(50, "name-nested", FeatureInputScalarRole::Driving, None);
        let mut lane = lane(
            vec![class(10, "sgCircleDim"), class(31, "sgEntHandle")],
            vec![first.clone(), nested_display.clone(), nested_driver.clone()],
        );
        lane.names = vec![
            FeatureInputName {
                id: "name-first".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: 20,
                object_id: None,
                value: "D1".into(),
            },
            FeatureInputName {
                id: "name-nested".into(),
                parent: "lane".into(),
                ordinal: 1,
                offset: 40,
                object_id: None,
                value: "D4".into(),
            },
        ];

        let instances = relation_instances(
            &cadmpeg_test_support::service_decode_context(),
            &sketch_history(),
            &lane,
        )
        .unwrap();
        let [relation] = instances.as_slice() else {
            panic!("one circle relation");
        };
        assert_eq!(relation.scalar_refs(), vec![first.id]);
        assert_eq!(relation.display_scalar_ref(), Some("scalar-20"));
        assert!(relation.parameter_scalar_ref().is_none());
        assert_eq!(
            circle_dimension_handle_driver(
                &cadmpeg_test_support::service_decode_context(),
                relation,
                &lane
            )
            .unwrap()
            .map(|scalar| scalar.id.as_str()),
            Some("scalar-50")
        );
    }

    #[test]
    fn circle_dimension_keeps_ambiguous_handle_drivers_unbound() {
        let first = circle_scalar(20, "name-first", FeatureInputScalarRole::Display, None);
        let d4_display = circle_scalar(40, "name-d4", FeatureInputScalarRole::Display, None);
        let d4_driver = circle_scalar(50, "name-d4", FeatureInputScalarRole::Driving, None);
        let d5_display = circle_scalar(60, "name-d5", FeatureInputScalarRole::Display, None);
        let d5_driver = circle_scalar(70, "name-d5", FeatureInputScalarRole::Driving, None);
        let mut lane = lane(
            vec![class(10, "sgCircleDim"), class(31, "sgEntHandle")],
            vec![first, d4_display, d4_driver, d5_display, d5_driver],
        );
        lane.names = [("name-first", "D1"), ("name-d4", "D4"), ("name-d5", "D5")]
            .into_iter()
            .enumerate()
            .map(|(ordinal, (id, value))| FeatureInputName {
                id: id.into(),
                parent: "lane".into(),
                ordinal: u32::try_from(ordinal).expect("test index fits u32"),
                offset: 20 + cadmpeg_core::decode::u64_from_index(ordinal) * 10,
                object_id: None,
                value: value.into(),
            })
            .collect();

        let instances = relation_instances(
            &cadmpeg_test_support::service_decode_context(),
            &sketch_history(),
            &lane,
        )
        .unwrap();
        let [relation] = instances.as_slice() else {
            panic!("one circle relation");
        };
        assert_eq!(relation.scalar_refs().len(), 1);
        assert!(relation.parameter_scalar_ref().is_none());
        assert!(circle_dimension_handle_driver(
            &cadmpeg_test_support::service_decode_context(),
            relation,
            &lane
        )
        .unwrap()
        .is_none());
    }

    #[test]
    fn circle_dimension_groups_repeated_display_scalars_by_name_and_entity() {
        let mut first = circle_scalar(20, "name-d1", FeatureInputScalarRole::Display, None);
        first.operands[0].entity_index = 0;
        let mut second = circle_scalar(30, "name-d1", FeatureInputScalarRole::Display, None);
        second.operands[0].entity_index = 1;
        let mut third = circle_scalar(40, "name-d1", FeatureInputScalarRole::Display, None);
        third.operands[0].entity_index = 2;
        let mut different_name =
            circle_scalar(50, "name-d2", FeatureInputScalarRole::Display, None);
        different_name.operands[0].entity_index = 3;
        let mut lane = lane(
            vec![class(10, "sgCircleDim")],
            vec![first.clone(), second, third, different_name],
        );
        lane.names = vec![
            FeatureInputName {
                id: "name-d1".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: 20,
                object_id: None,
                value: "D1".into(),
            },
            FeatureInputName {
                id: "name-d2".into(),
                parent: "lane".into(),
                ordinal: 1,
                offset: 50,
                object_id: None,
                value: "D2".into(),
            },
        ];

        let instances = relation_instances(
            &cadmpeg_test_support::service_decode_context(),
            &sketch_history(),
            &lane,
        )
        .unwrap();
        let [relation] = instances.as_slice() else {
            panic!("one repeated circle relation");
        };
        assert_eq!(
            relation.scalar_refs(),
            vec!["scalar-20", "scalar-30", "scalar-40"]
        );
        assert!(relation.parameter_scalar_ref().is_none());
        assert!(relation.display_scalar_ref().is_none());
        assert_eq!(relation.operands, first.operands);
    }

    fn dynamic_scalar(
        offset: u64,
        kind: FeatureInputOperandKind,
        indices: &[u16],
        value: f64,
    ) -> FeatureInputScalar {
        let mut scalar = scalar(offset, FeatureInputScalarRole::Driving);
        scalar.value = cadmpeg_ir::scalar::FiniteReal::new(value).expect("finite test scalar");

        scalar.operands = indices
            .iter()
            .enumerate()
            .map(|(ordinal, entity_index)| FeatureInputOperand {
                offset: offset + cadmpeg_core::decode::u64_from_index(ordinal),
                reference_ref: format!("reference-{offset}-{ordinal}"),
                kind,
                entity_index: *entity_index,
                entity_ref: None,
            })
            .collect();
        scalar
    }

    fn sketch_marker(
        id: &str,
        ordinal: u32,
        offset: u64,
        kind: crate::records::SketchInputKind,
        coordinates_m: Option<[f64; 2]>,
    ) -> crate::records::SketchInputEntity {
        let mut marker = crate::records::SketchInputEntity::new(id, "lane", ordinal, offset, kind);
        marker.feature_ref = Some("sketch".into());
        marker.coordinates_m = coordinates_m.and_then(cadmpeg_ir::units::FiniteVector::new);
        marker
    }

    fn dynamic_point_markers() -> Vec<crate::records::SketchInputEntity> {
        vec![
            sketch_marker(
                "relation",
                0,
                1,
                crate::records::SketchInputKind::Relation(
                    crate::records::SketchRelationKind::Distance,
                ),
                None,
            ),
            sketch_marker(
                "p0",
                1,
                10,
                crate::records::SketchInputKind::Point,
                Some([0.0, 0.0]),
            ),
            sketch_marker(
                "p1",
                2,
                20,
                crate::records::SketchInputKind::Point,
                Some([1.0, 0.0]),
            ),
            sketch_marker(
                "p2",
                3,
                30,
                crate::records::SketchInputKind::Point,
                Some([3.0, 0.0]),
            ),
        ]
    }

    #[test]
    fn dynamic_relation_tags_are_admitted_by_family_scope() {
        let lane = lane(
            vec![class(10, "sgPntPntDist")],
            vec![dynamic_scalar(
                40,
                FeatureInputOperandKind::Native(NativeOperandTag::try_from(0x812a).unwrap()),
                &[0, 1],
                2.0,
            )],
        );

        let instances = relation_instances(
            &cadmpeg_test_support::service_decode_context(),
            &sketch_history(),
            &lane,
        )
        .unwrap();
        let [relation] = instances.as_slice() else {
            panic!("one dynamically tagged relation");
        };
        assert_eq!(
            relation.family,
            FeatureInputRelationFamily::PointPointDistance
        );
        assert!(relation_uses_dynamic_operands(relation));
    }

    #[test]
    fn dynamic_point_relation_uses_unique_geometry_match_across_address_tiers() {
        let mut lane = lane(
            vec![class(10, "sgPntPntDist")],
            vec![dynamic_scalar(
                40,
                FeatureInputOperandKind::Native(NativeOperandTag::try_from(0x812a).unwrap()),
                &[1, 2],
                2.0,
            )],
        );
        lane.sketch_entities = dynamic_point_markers();

        let instances = relation_instances(
            &cadmpeg_test_support::service_decode_context(),
            &sketch_history(),
            &lane,
        )
        .unwrap();
        let [relation] = instances.as_slice() else {
            panic!("one dynamically tagged relation");
        };
        assert_eq!(
            relation
                .operands
                .iter()
                .map(|operand| operand.entity_ref.as_deref())
                .collect::<Vec<_>>(),
            vec![Some("p1"), Some("p2")]
        );
    }

    #[test]
    fn dynamic_point_relation_preserves_explicit_driving_operand_reference() {
        let mut driving = dynamic_scalar(
            40,
            FeatureInputOperandKind::Native(NativeOperandTag::try_from(0x812a).unwrap()),
            &[1, 2],
            2.0,
        );
        driving.operands[0].entity_ref = Some("p1".into());
        let mut lane = lane(vec![class(10, "sgPntPntDist")], vec![driving]);
        lane.sketch_entities = dynamic_point_markers();

        let instances = relation_instances(
            &cadmpeg_test_support::service_decode_context(),
            &sketch_history(),
            &lane,
        )
        .unwrap();
        let [relation] = instances.as_slice() else {
            panic!("one dynamically tagged relation");
        };
        assert_eq!(
            relation
                .operands
                .iter()
                .map(|operand| operand.entity_ref.as_deref())
                .collect::<Vec<_>>(),
            vec![Some("p1"), Some("p2")]
        );
    }

    #[test]
    fn dynamic_point_relation_falls_back_to_unique_geometry_after_address_miss() {
        let mut driving = dynamic_scalar(
            40,
            FeatureInputOperandKind::Native(NativeOperandTag::try_from(0x812a).unwrap()),
            &[3, 1],
            1.0,
        );
        driving.operands[0].entity_ref = Some("p0".into());
        let mut lane = lane(vec![class(10, "sgPntPntDist")], vec![driving]);
        lane.sketch_entities = vec![
            sketch_marker(
                "relation",
                0,
                1,
                crate::records::SketchInputKind::Relation(
                    crate::records::SketchRelationKind::Distance,
                ),
                None,
            ),
            sketch_marker(
                "p0",
                1,
                10,
                crate::records::SketchInputKind::Point,
                Some([0.0, 0.0]),
            ),
            sketch_marker(
                "p1",
                2,
                20,
                crate::records::SketchInputKind::Point,
                Some([1.0, 1.0]),
            ),
            sketch_marker(
                "p2",
                3,
                30,
                crate::records::SketchInputKind::Point,
                Some([1.0, 0.0]),
            ),
        ];

        let instances = relation_instances(
            &cadmpeg_test_support::service_decode_context(),
            &sketch_history(),
            &lane,
        )
        .unwrap();
        let [relation] = instances.as_slice() else {
            panic!("one dynamically tagged relation");
        };
        assert_eq!(
            relation
                .operands
                .iter()
                .map(|operand| operand.entity_ref.as_deref())
                .collect::<Vec<_>>(),
            vec![Some("p0"), Some("p2")]
        );
    }

    #[test]
    fn relation_instance_keeps_first_scalar_operands_when_grouping_display_and_driver() {
        let mut display = dynamic_scalar(
            20,
            FeatureInputOperandKind::Native(NativeOperandTag::try_from(0x812a).unwrap()),
            &[1, 2],
            2.0,
        );
        display.role = FeatureInputScalarRole::Display;
        display.operands[0].entity_ref = Some("p1".into());
        let mut driving = dynamic_scalar(
            30,
            FeatureInputOperandKind::Native(NativeOperandTag::try_from(0x812a).unwrap()),
            &[1, 2],
            2.0,
        );
        driving.operands[0].entity_ref = Some("p0".into());
        let mut lane = lane(
            vec![class(10, "sgPntPntDist")],
            vec![display, driving.clone()],
        );
        lane.sketch_entities = dynamic_point_markers();

        let instances = relation_instances(
            &cadmpeg_test_support::service_decode_context(),
            &sketch_history(),
            &lane,
        )
        .unwrap();
        let [relation] = instances.as_slice() else {
            panic!("one grouped relation instance");
        };
        assert_eq!(
            relation.parameter_scalar_ref().map(str::to_owned),
            Some(driving.id)
        );
        assert_eq!(
            relation
                .operands
                .iter()
                .map(|operand| operand.entity_ref.as_deref())
                .collect::<Vec<_>>(),
            vec![Some("p1"), Some("p2")]
        );
    }

    #[test]
    fn dynamic_display_relation_uses_display_value_without_driver() {
        let mut display = dynamic_scalar(
            40,
            FeatureInputOperandKind::Native(NativeOperandTag::try_from(0x812a).unwrap()),
            &[1, 2],
            2.0,
        );
        display.role = FeatureInputScalarRole::Display;
        let mut lane = lane(vec![class(10, "sgPntPntDist")], vec![display]);
        lane.sketch_entities = dynamic_point_markers();

        let instances = relation_instances(
            &cadmpeg_test_support::service_decode_context(),
            &sketch_history(),
            &lane,
        )
        .unwrap();
        let [relation] = instances.as_slice() else {
            panic!("one dynamically tagged relation");
        };
        assert!(relation.parameter_scalar_ref().is_none());
        assert_eq!(
            relation
                .operands
                .iter()
                .map(|operand| operand.entity_ref.as_deref())
                .collect::<Vec<_>>(),
            vec![Some("p1"), Some("p2")]
        );
    }

    #[test]
    fn dynamic_point_relation_with_ambiguous_geometry_stays_unbound() {
        let mut lane = lane(
            vec![class(10, "sgPntPntDist")],
            vec![dynamic_scalar(
                40,
                FeatureInputOperandKind::Native(NativeOperandTag::try_from(0x812a).unwrap()),
                &[1, 2],
                1.0,
            )],
        );
        lane.sketch_entities = dynamic_point_markers();
        lane.sketch_entities[3].coordinates_m = cadmpeg_ir::units::FiniteVector::new([1.0, 0.0]);

        let instances = relation_instances(
            &cadmpeg_test_support::service_decode_context(),
            &sketch_history(),
            &lane,
        )
        .unwrap();
        let [relation] = instances.as_slice() else {
            panic!("one dynamically tagged relation");
        };
        assert!(relation
            .operands
            .iter()
            .all(|operand| operand.entity_ref.is_none()));
    }

    #[test]
    fn dynamic_point_line_relation_resolves_solver_line_by_exact_distance() {
        let mut driving = dynamic_scalar(
            40,
            FeatureInputOperandKind::Native(NativeOperandTag::try_from(0x812a).unwrap()),
            &[0, 1],
            1.0,
        );
        driving.operands[1].entity_ref = Some("p0".into());
        let mut lane = lane(vec![class(10, "sgPntLineDist")], vec![driving]);
        lane.sketch_entities = vec![
            sketch_marker(
                "relation",
                0,
                1,
                crate::records::SketchInputKind::Relation(
                    crate::records::SketchRelationKind::Distance,
                ),
                None,
            ),
            sketch_marker(
                "p0",
                1,
                10,
                crate::records::SketchInputKind::Point,
                Some([0.0, 0.0]),
            ),
            sketch_marker(
                "p1",
                2,
                20,
                crate::records::SketchInputKind::Point,
                Some([1.0, 0.0]),
            ),
            sketch_marker(
                "p2",
                3,
                30,
                crate::records::SketchInputKind::Point,
                Some([0.0, 1.0]),
            ),
            sketch_marker(
                "p3",
                4,
                40,
                crate::records::SketchInputKind::Point,
                Some([1.0, 1.0]),
            ),
        ];

        let instances = relation_instances(
            &cadmpeg_test_support::service_decode_context(),
            &sketch_history(),
            &lane,
        )
        .unwrap();
        let [relation] = instances.as_slice() else {
            panic!("one dynamically tagged relation");
        };
        assert_eq!(relation.operands[0].entity_ref.as_deref(), Some("p0"));
        assert!(relation.operands[1].entity_ref.is_none());
    }

    #[test]
    fn dynamic_line_relation_validates_solver_line_pair() {
        let mut driving = dynamic_scalar(
            40,
            FeatureInputOperandKind::Native(NativeOperandTag::try_from(0x812a).unwrap()),
            &[0, 1],
            1.0,
        );
        driving.operands[0].entity_ref = Some("p0".into());
        let mut lane = lane(vec![class(10, "sgLLDist")], vec![driving]);
        lane.sketch_entities = vec![
            sketch_marker(
                "p0",
                0,
                10,
                crate::records::SketchInputKind::Point,
                Some([0.0, 0.0]),
            ),
            sketch_marker(
                "p1",
                1,
                20,
                crate::records::SketchInputKind::Point,
                Some([1.0, 0.0]),
            ),
            sketch_marker(
                "p2",
                2,
                30,
                crate::records::SketchInputKind::Point,
                Some([0.0, 1.0]),
            ),
            sketch_marker(
                "p3",
                3,
                40,
                crate::records::SketchInputKind::Point,
                Some([1.0, 1.0]),
            ),
        ];

        let instances = relation_instances(
            &cadmpeg_test_support::service_decode_context(),
            &sketch_history(),
            &lane,
        )
        .unwrap();
        let [relation] = instances.as_slice() else {
            panic!("one dynamically tagged relation");
        };
        assert!(relation
            .operands
            .iter()
            .all(|operand| operand.entity_ref.is_none()));
    }

    #[test]
    fn dynamic_angle_accepts_reversed_solver_line_direction() {
        let driving = dynamic_scalar(
            40,
            FeatureInputOperandKind::Native(NativeOperandTag::try_from(0x812a).unwrap()),
            &[0, 1],
            std::f64::consts::FRAC_PI_4,
        );
        let mut lane = lane(vec![class(10, "sgAnglDim")], vec![driving]);
        lane.sketch_entities = vec![
            sketch_marker(
                "first-start",
                0,
                10,
                crate::records::SketchInputKind::Point,
                Some([0.0, 0.0]),
            ),
            sketch_marker(
                "first-end",
                1,
                20,
                crate::records::SketchInputKind::Point,
                Some([1.0, 0.0]),
            ),
            sketch_marker(
                "second-start",
                2,
                30,
                crate::records::SketchInputKind::Point,
                Some([0.0, 0.0]),
            ),
            sketch_marker(
                "second-end",
                3,
                40,
                crate::records::SketchInputKind::Point,
                Some([-1.0, 1.0]),
            ),
        ];

        let instances = relation_instances(
            &cadmpeg_test_support::service_decode_context(),
            &sketch_history(),
            &lane,
        )
        .unwrap();
        let [relation] = instances.as_slice() else {
            panic!("one dynamically tagged angle relation");
        };
        assert!(relation
            .operands
            .iter()
            .all(|operand| operand.entity_ref.is_none()));
    }
}
