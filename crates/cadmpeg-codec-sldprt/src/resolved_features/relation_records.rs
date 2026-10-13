//! Relation instance records and scalar roles.

use super::scalars::ObjectNames;
use super::SKETCH_POINT_TOLERANCE;
use crate::classification::{native_object_class, NativeClassKind};
use crate::history::classify::is_history_metadata_record;
use crate::layout::feature_input_shifted_scalar_trailer as shifted_trailer;
use crate::records::operand_tag::NativeOperandTag;
use crate::records::{
    FeatureInputClass, FeatureInputLane, FeatureInputOperand, FeatureInputOperandKind,
    FeatureInputRelationFamily, FeatureInputRelationInstance, FeatureInputScalar,
    FeatureInputScalarRole,
};
use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use std::collections::{BTreeMap, HashMap, HashSet};

fn same_scalar_name(
    ctx: &DecodeContext<'_>,
    first: &FeatureInputScalar,
    second: &FeatureInputScalar,
    names: &HashMap<&str, &str>,
) -> Result<bool, CodecError> {
    const OPERATION: &str = "compare SLDPRT relation scalar names";
    let Some(value) = ctx
        .get_hash_map(names, first.name.as_str(), OPERATION)?
        .copied()
    else {
        return Ok(false);
    };
    let Some(candidate) = ctx
        .get_hash_map(names, second.name.as_str(), OPERATION)?
        .copied()
    else {
        return Ok(false);
    };
    ctx.equal(value, candidate, OPERATION)
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
    let names = ObjectNames::new(ctx, lane)?;
    let mut starts = Vec::<(u64, &str)>::new();
    for history in ctx.admit_iter(histories, "scan SLDPRT feature interval histories")? {
        for feature in ctx.admit_iter(&history.features, "scan SLDPRT feature intervals")? {
            if is_history_metadata_record(ctx, feature, &history.features)? {
                continue;
            }
            if let Some(name) = names.of(ctx, feature)? {
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

/// The interval that contains `offset`, if one does. Intervals are in
/// ascending start order and each ends where the next starts, so the containing
/// interval is the last one starting at or before `offset`.
fn feature_at_offset<'a>(
    ctx: &DecodeContext<'_>,
    offset: u64,
    intervals: &'a [(u64, Option<u64>, String)],
) -> Result<Option<FeatureIntervalMatch<'a>>, CodecError> {
    let after = ctx.partition_point(
        intervals,
        |(start, _, _)| Ok(*start <= offset),
        "find SLDPRT feature interval",
    )?;
    Ok(after
        .checked_sub(1)
        .and_then(|index| intervals.get(index))
        .filter(|(_, end, _)| end.is_none_or(|end| offset < end))
        .map(|(_, end, feature)| FeatureIntervalMatch {
            feature_name: feature.as_str(),
            end: *end,
        }))
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

/// The relation classes and scalars of one lane, ordered for scope searches.
struct DeclarationIndex<'a, 'i> {
    /// The offsets of the relation classes inside each feature interval, ascending.
    relation_class_offsets: HashMap<&'i str, Vec<u64>>,
    /// The scalars in offset order; equal offsets keep lane order.
    scalars: Vec<&'a FeatureInputScalar>,
}

impl<'a, 'i> DeclarationIndex<'a, 'i> {
    fn new(
        ctx: &DecodeContext<'_>,
        classes: &[FeatureInputClass],
        scalars: &'a [FeatureInputScalar],
        intervals: &'i [(u64, Option<u64>, String)],
    ) -> Result<Self, CodecError> {
        const OPERATION: &str = "index SLDPRT relation declarations";
        let mut relation_classes_storage = ctx.reserve_scoped(0, OPERATION)?;
        let mut relation_classes = Vec::new();
        for class in ctx.admit_iter(classes, OPERATION)? {
            if relation_family(&class.name).is_none() {
                continue;
            }
            if let Some(interval) = feature_at_offset(ctx, class.offset, intervals)? {
                relation_classes_storage.with_storage(|| {
                    ctx.push_vec(
                        &mut relation_classes,
                        (class.offset, interval.feature_name),
                        OPERATION,
                    )
                })?;
            }
        }
        ctx.sort_unstable_by_key(
            &mut relation_classes,
            |(offset, _)| *offset,
            Ord::cmp,
            OPERATION,
        )?;
        // Grouping in offset order leaves each feature's offsets ascending.
        let mut relation_class_offsets = HashMap::new();
        for (offset, feature) in ctx.admit_iter(relation_classes, OPERATION)? {
            ctx.push_hash_group(
                &mut relation_class_offsets,
                feature,
                offset,
                OPERATION,
                OPERATION,
            )?;
        }
        drop(relation_classes_storage);
        let mut ordered = ctx.collect_vec(scalars.iter(), OPERATION)?;
        ctx.stable_sort_by(&mut ordered, |scalar| &scalar.offset, Ord::cmp, OPERATION)?;
        Ok(Self {
            relation_class_offsets,
            scalars: ordered,
        })
    }

    /// Where the relation declared by `class` stops reading scalars.
    fn scope_end(
        &self,
        ctx: &DecodeContext<'_>,
        class: &FeatureInputClass,
        class_interval: Option<FeatureIntervalMatch<'_>>,
    ) -> Result<RelationScope, CodecError> {
        const OPERATION: &str = "find SLDPRT relation scope end";
        // The next relation class of the same feature.
        let mut next_class = None;
        if let Some(interval) = class_interval {
            if let Some(offsets) = ctx.get_hash_map(
                &self.relation_class_offsets,
                interval.feature_name,
                OPERATION,
            )? {
                let after =
                    ctx.partition_point(offsets, |offset| Ok(*offset <= class.offset), OPERATION)?;
                next_class = offsets.get(after).copied();
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

    /// The first scalar inside the class's scope whose operands fit the family.
    fn declaration(
        &self,
        ctx: &DecodeContext<'_>,
        class: &FeatureInputClass,
        class_interval: Option<FeatureIntervalMatch<'_>>,
        allow_dynamic: bool,
    ) -> Result<Option<(&'a FeatureInputScalar, FeatureInputRelationFamily)>, CodecError> {
        const OPERATION: &str = "match SLDPRT relation scalar declarations";
        let Some(family) = relation_family(&class.name) else {
            return Ok(None);
        };
        let class_feature = class_interval.map(|interval| interval.feature_name);
        let scope_end = match self.scope_end(ctx, class, class_interval)? {
            RelationScope::Ends(end) => Some(end),
            RelationScope::Unbounded => None,
            RelationScope::Unstatable => return Ok(None),
        };
        let first = ctx.partition_point(
            &self.scalars,
            |scalar| Ok(scalar.offset <= class.offset),
            OPERATION,
        )?;
        let in_scope = self.scalars.get(first..).unwrap_or_default();
        let selected = ctx.find_map(
            in_scope.iter().copied(),
            |scalar| {
                if scope_end.is_some_and(|end| scalar.offset >= end) {
                    // Past the scope: the search ends with no declaration.
                    return Ok(Some(None));
                }
                let signature = if allow_dynamic {
                    relation_signature_for_declaration(family, scalar)
                } else {
                    relation_signature(family, &scalar.operands)
                };
                if !signature {
                    return Ok(None);
                }
                if let Some(feature) = class_feature {
                    let Some(scalar_feature) = scalar.feature_ref.as_deref() else {
                        return Ok(None);
                    };
                    if !ctx.equal(
                        feature,
                        scalar_feature,
                        "compare SLDPRT relation declaration feature references",
                    )? {
                        return Ok(None);
                    }
                }
                Ok(Some(Some(scalar)))
            },
            OPERATION,
        )?;
        Ok(selected.flatten().map(|scalar| (scalar, family)))
    }
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
    let declaration_index = std::cell::OnceCell::new();
    let mut candidates = Vec::new();
    for class in ctx.admit_iter(classes, "scan SLDPRT relation declarations")? {
        if relation_family(&class.name).is_none() {
            continue;
        }
        let class_interval = feature_at_offset(ctx, class.offset, intervals)?;
        if class_interval.is_none() && class.offset.checked_add(UNKNOWN_FEATURE_SPAN).is_none() {
            continue;
        }
        let (index, _) = match declaration_index.get() {
            Some(index) => index,
            None => {
                let built = ctx
                    .with_scoped_storage("index SLDPRT relation declarations", || {
                        DeclarationIndex::new(ctx, classes, scalars, intervals)
                    })?;
                declaration_index.get_or_init(|| built)
            }
        };
        if let Some((scalar, family)) =
            index.declaration(ctx, class, class_interval, allow_dynamic)?
        {
            ctx.push_vec(
                &mut candidates,
                (class, scalar, family),
                "collect SLDPRT relation candidates",
            )?;
        }
    }
    Ok(candidates)
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

    let candidates_storage;
    let candidates;
    (candidates, candidates_storage) = ctx
        .with_scoped_storage("SLDPRT all relation declaration candidates", || {
            relation_declaration_candidates_impl(ctx, classes, scalars, intervals, false)
        })?;
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
    drop(candidates);
    drop(candidates_storage);
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
    if !ctx.all_by(
        left.iter().zip(right.iter()),
        |(left, right)| {
            if left.kind != right.kind || left.entity_index != right.entity_index {
                return Ok(false);
            }

            Ok(true)
        },
        operation,
    )? {
        return Ok(false);
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
    let mut groups_storage = ctx.reserve_scoped(0, "SLDPRT relation scalar groups")?;
    let groups = {
        let mut temporary_storage = ctx.reserve_scoped(0, "SLDPRT relation grouping indexes")?;
        let candidates_storage;
        let declaration_candidates;
        (declaration_candidates, candidates_storage) =
            ctx.with_scoped_storage("SLDPRT relation declaration candidates", || {
                let mut intervals_storage =
                    ctx.reserve_scoped(0, "SLDPRT relation feature intervals")?;
                let intervals =
                    intervals_storage.with_storage(|| feature_intervals(ctx, histories, lane))?;
                relation_declaration_candidates_impl(
                    ctx,
                    &lane.classes,
                    &lane.scalars,
                    &intervals,
                    true,
                )
            })?;
        // The offsets of every relation class, ascending, to find a class between
        // two scalars.
        let mut relation_class_offsets = Vec::new();
        for class in ctx.admit_iter(&lane.classes, "index SLDPRT relation group boundaries")? {
            if relation_family(&class.name).is_some() {
                temporary_storage.with_storage(|| {
                    ctx.push_vec(
                        &mut relation_class_offsets,
                        class.offset,
                        "index SLDPRT relation group boundaries",
                    )
                })?;
            }
        }
        ctx.sort_unstable_by(
            &mut relation_class_offsets,
            |offset| offset,
            Ord::cmp,
            "index SLDPRT relation group boundaries",
        )?;
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
                    ctx.refuse_codec_limit(
                        "count SLDPRT relation candidates",
                        u64::MAX - 1,
                        u64::MAX,
                    )
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
        drop(declaration_candidates);
        drop(candidates_storage);
        let scalar_names = std::cell::OnceCell::new();
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
                let same_scope_prefix = *last_index + 1 == scalar_index
                    && ctx.equal(
                        group.feature_ref,
                        feature_ref,
                        "compare SLDPRT relation group features",
                    )?;
                let has_intervening_class = if same_scope_prefix {
                    let after = ctx.partition_point(
                        &relation_class_offsets,
                        |offset| Ok(*offset <= last_scalar.offset),
                        "find SLDPRT relation group boundary",
                    )?;
                    relation_class_offsets
                        .get(after)
                        .is_some_and(|offset| *offset < scalar.offset)
                } else {
                    false
                };
                let same_scope = same_scope_prefix && !has_intervening_class;
                let mut repeated_circle_display = same_scope
                    && group.family == FeatureInputRelationFamily::CircleDiameter
                    && scalar.role == FeatureInputScalarRole::Display
                    && scalar.operands.len() == 1;
                if repeated_circle_display {
                    let (names, _) = match scalar_names.get() {
                        Some(names) => names,
                        None => {
                            const OPERATION: &str = "index SLDPRT relation scalar names";
                            let built = ctx.with_scoped_storage(OPERATION, || {
                                let mut names = HashMap::new();
                                for name in ctx.admit_iter(&lane.names, OPERATION)? {
                                    ctx.entry_hash_map(&mut names, name.id.as_str(), OPERATION)?
                                        .or_insert(name.value.as_str());
                                }
                                Ok::<_, CodecError>(names)
                            })?;
                            scalar_names.get_or_init(|| built)
                        }
                    };
                    repeated_circle_display = ctx.all_by(
                        &group.scalars,
                        |(_, candidate)| {
                            Ok(candidate.role == FeatureInputScalarRole::Display
                                && candidate.operands.len() == 1
                                && candidate.operands[0].kind == scalar.operands[0].kind
                                && candidate.operands[0].entity_index
                                    != scalar.operands[0].entity_index
                                && same_scalar_name(ctx, candidate, scalar, names)?)
                        },
                        "validate SLDPRT repeated circle display scalars",
                    )?;
                }
                if repeated_circle_display {
                    groups_storage.with_storage(|| {
                        ctx.reserve_vec(
                            &mut group.scalars,
                            1,
                            "collect SLDPRT relation scalar groups",
                        )
                    })?;
                    group.scalars.push((scalar_index, scalar));
                } else if same_scope
                    && scalar.role == FeatureInputScalarRole::Driving
                    && same_relation_operand_signature(
                        ctx,
                        group.operands,
                        &scalar.operands,
                        "compare SLDPRT relation operand signatures",
                    )?
                {
                    if group.scalars.len() == 1 {
                        groups_storage.with_storage(|| {
                            ctx.reserve_vec(
                                &mut group.scalars,
                                1,
                                "collect SLDPRT relation scalar groups",
                            )
                        })?;
                        group.scalars.push((scalar_index, scalar));
                    } else {
                        let next = RelationGroup {
                            feature_ref: group.feature_ref,
                            family: group.family,
                            class_ref: group.class_ref,
                            operands: &scalar.operands,
                            scalars: groups_storage.with_storage(|| {
                                ctx.alloc_filled(
                                    1,
                                    (scalar_index, scalar),
                                    "collect SLDPRT relation scalar groups",
                                )
                            })?,
                        };
                        groups_storage.with_storage(|| {
                            ctx.reserve_vec(&mut groups, 1, "collect SLDPRT relation groups")
                        })?;
                        groups.push(next);
                    }
                }
                continue;
            };
            // A display scalar can precede the declaration selected by its adjacent
            // driving scalar. The driving scalar's declaration is authoritative for
            // that pair; display-only scalars still stop at a class declaration.
            let promote_class = if let Some(group) = groups.last() {
                group.scalars.len() == 1
                    && group.scalars[0].1.role == FeatureInputScalarRole::Display
                    && scalar.role == FeatureInputScalarRole::Driving
                    && *class_offset > group.scalars[0].1.offset
                    && *class_offset < scalar.offset
                    && !ctx.equal(
                        group.class_ref,
                        class_ref,
                        "compare SLDPRT promoted relation classes",
                    )?
            } else {
                false
            };
            let append = if let Some(group) = groups.last() {
                (group.family == *family || promote_class)
                    && matches!(group.scalars.as_slice(), [(index, _)] if *index + 1 == scalar_index)
                    && ctx.equal(
                        group.feature_ref,
                        feature_ref,
                        "compare SLDPRT relation group features",
                    )?
                    && (promote_class
                        || ctx.equal(
                            group.class_ref,
                            class_ref,
                            "compare SLDPRT relation group classes",
                        )?)
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
                groups_storage.with_storage(|| {
                    ctx.reserve_vec(
                        &mut group.scalars,
                        1,
                        "collect SLDPRT relation scalar groups",
                    )
                })?;
                group.scalars.push((scalar_index, scalar));
            } else {
                groups_storage.with_storage(|| {
                    ctx.reserve_vec(&mut groups, 1, "collect SLDPRT relation groups")
                })?;
                groups.push(RelationGroup {
                    feature_ref,
                    family: *family,
                    class_ref,
                    operands: &scalar.operands,
                    scalars: groups_storage.with_storage(|| {
                        ctx.alloc_filled(
                            1,
                            (scalar_index, scalar),
                            "collect SLDPRT relation scalar groups",
                        )
                    })?,
                });
            }
        }
        groups
    };
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
    drop(groups);
    drop(groups_storage);
    // A class can declare more than one scalar relation. The ordinary
    // instance grouper joins a display scalar to its adjacent driving scalar
    // when their operand signatures agree; a second scalar with a different
    // signature is still a declared relation binding and must not disappear.
    // Promote only bindings whose scalar is not already claimed by an
    // instance. A scalar has one relation-instance owner even if malformed
    // input associates it with more than one class. Non-sketch bindings remain
    // native-only, matching the existing relation-instance scope.
    // The first scalar with each identity; built when a binding needs it.
    let mut scalars_by_id: Option<HashMap<&str, &FeatureInputScalar>> = None;
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
        if scalars_by_id.is_none() {
            scalars_by_id =
                Some(temporary_storage.with_storage(|| first_scalars_by_id(ctx, &lane.scalars))?);
        }
        let Some(scalar) = (match &scalars_by_id {
            Some(scalars) => ctx
                .get_hash_map(
                    scalars,
                    binding.scalar_ref.as_str(),
                    "find SLDPRT relation binding scalar",
                )?
                .copied(),
            None => None,
        }) else {
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
    for (ordinal, relation) in ctx
        .admit_iter(&mut instances, "number SLDPRT relation instances")?
        .enumerate()
    {
        relation.ordinal = u32::try_from(ordinal).map_err(|_| {
            ctx.refuse_codec_limit("number SLDPRT relation instances", u64::MAX - 1, u64::MAX)
        })?;
    }
    bind_detached_relation_drivers(ctx, &mut instances, lane)?;
    bind_circle_dimension_centers(ctx, &mut instances, lane)?;
    bind_relation_geometry_operands(ctx, &mut instances, lane)?;
    Ok(instances)
}

/// The first scalar of the lane with each identity.
fn first_scalars_by_id<'a>(
    ctx: &DecodeContext<'_>,
    scalars: &'a [FeatureInputScalar],
) -> Result<HashMap<&'a str, &'a FeatureInputScalar>, CodecError> {
    const OPERATION: &str = "index SLDPRT relation scalars";
    let mut index = HashMap::new();
    for scalar in ctx.admit_iter(scalars, OPERATION)? {
        ctx.entry_hash_map(&mut index, scalar.id.as_str(), OPERATION)?
            .or_insert(scalar);
    }
    Ok(index)
}

/// The last scalar of the lane with each identity.
fn last_scalars_by_id<'a>(
    ctx: &DecodeContext<'_>,
    scalars: &'a [FeatureInputScalar],
) -> Result<HashMap<&'a str, &'a FeatureInputScalar>, CodecError> {
    const OPERATION: &str = "index SLDPRT relation records";
    let mut index = HashMap::new();
    for scalar in ctx.admit_iter(scalars, OPERATION)? {
        ctx.insert_hash_map(&mut index, scalar.id.as_str(), scalar, OPERATION)?;
    }
    Ok(index)
}

/// The last name value of the lane with each identity.
fn last_name_values<'a>(
    ctx: &DecodeContext<'_>,
    lane: &'a FeatureInputLane,
) -> Result<HashMap<&'a str, &'a str>, CodecError> {
    const OPERATION: &str = "index SLDPRT relation records";
    let mut index = HashMap::new();
    for name in ctx.admit_iter(&lane.names, OPERATION)? {
        ctx.insert_hash_map(&mut index, name.id.as_str(), name.value.as_str(), OPERATION)?;
    }
    Ok(index)
}

/// The first position of each scalar identity in the lane.
fn scalar_positions<'a>(
    ctx: &DecodeContext<'_>,
    scalars: &'a [FeatureInputScalar],
) -> Result<HashMap<&'a str, usize>, CodecError> {
    const OPERATION: &str = "index SLDPRT relation scalar positions";
    let mut index = HashMap::new();
    for (position, scalar) in ctx.admit_iter(scalars, OPERATION)?.enumerate() {
        ctx.entry_hash_map(&mut index, scalar.id.as_str(), OPERATION)?
            .or_insert(position);
    }
    Ok(index)
}

fn copy_relation_text(ctx: &DecodeContext<'_>, text: &str) -> Result<String, CodecError> {
    ctx.copy_retained_text(text, "retain SLDPRT relation record identity")
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

    let mut scalars: Option<HashMap<&str, &FeatureInputScalar>> = None;
    let mut names: Option<HashMap<&str, &str>> = None;
    // Built when the first circle relation needs them: each scalar's lane
    // position, and the two-operand scalars grouped by feature, name and first
    // operand, in lane order.
    let mut positions: Option<HashMap<&str, usize>> = None;
    let mut center_groups: Option<CenterScalarGroups<'_>> = None;
    for relation in ctx.admit_iter(&mut *relations, "scan SLDPRT circle-diameter relations")? {
        if relation.family != FeatureInputRelationFamily::CircleDiameter
            || relation.operands.len() != 1
        {
            continue;
        }
        let Some(display_id) = relation.display_scalar_ref() else {
            continue;
        };
        let scalars = match &mut scalars {
            Some(index) => index,
            slot @ None => slot
                .insert(temporary_storage.with_storage(|| last_scalars_by_id(ctx, &lane.scalars))?),
        };
        let Some(display) = ctx
            .get_hash_map(scalars, display_id, "find SLDPRT display scalar")?
            .copied()
        else {
            continue;
        };
        let names = match &mut names {
            Some(index) => index,
            slot @ None => {
                slot.insert(temporary_storage.with_storage(|| last_name_values(ctx, lane))?)
            }
        };
        let Some(display_name) = ctx.get_hash_map(
            names,
            display.name.as_str(),
            "find SLDPRT display scalar name",
        )?
        else {
            continue;
        };
        if positions.is_none() {
            positions =
                Some(temporary_storage.with_storage(|| scalar_positions(ctx, &lane.scalars))?);
        }
        let Some(display_index) = (match &positions {
            Some(positions) => ctx
                .get_hash_map(
                    positions,
                    display.id.as_str(),
                    "match SLDPRT display scalar",
                )?
                .copied(),
            None => None,
        }) else {
            continue;
        };
        if center_groups.is_none() {
            center_groups = Some(
                temporary_storage
                    .with_storage(|| center_scalar_groups(ctx, &lane.scalars, names))?,
            );
        }
        let first = &relation.operands[0];
        let key = (
            display.feature_ref.as_deref(),
            *display_name,
            first.kind,
            first.entity_index,
        );
        let candidates = match &center_groups {
            Some(groups) => ctx
                .get_hash_map(groups, &key, "match SLDPRT circle center scalars")?
                .map_or(&[][..], Vec::as_slice),
            None => &[][..],
        };
        let first_center_index = display_index.checked_add(1);
        if candidates.first().map(|candidate| candidate.0) != first_center_index {
            continue;
        }
        let mut previous_index: Option<usize> = None;
        let contiguous = ctx.all_by(
            candidates,
            |(index, _)| {
                if previous_index.is_some_and(|previous| previous.checked_add(1) != Some(*index)) {
                    return Ok(false);
                }
                previous_index = Some(*index);
                Ok(true)
            },
            "check SLDPRT circle center scalar window",
        )?;
        if !contiguous {
            continue;
        }
        let mut center: Option<(FeatureInputOperandKind, u16, Option<&str>)> = None;
        let centers_match = ctx.all_by(
            candidates,
            |(_, scalar)| {
                let Some(operand) = scalar.operands.get(1) else {
                    return Ok(true);
                };
                let candidate = (
                    operand.kind,
                    operand.entity_index,
                    operand.entity_ref.as_deref(),
                );
                let same_center = if let Some((kind, entity_index, entity_ref)) = center {
                    if kind != candidate.0 || entity_index != candidate.1 {
                        return Ok(false);
                    }
                    match (entity_ref, candidate.2) {
                        (Some(left), Some(right)) => {
                            ctx.equal(left, right, "compare SLDPRT circle center references")?
                        }
                        (None, None) => true,
                        _ => false,
                    }
                } else {
                    center = Some(candidate);
                    true
                };
                Ok(same_center)
            },
            "check SLDPRT circle center operand consensus",
        )?;
        if !centers_match {
            continue;
        }
        let Some((_, source)) = candidates.first() else {
            continue;
        };
        relation.operands = copy_relation_operands(ctx, &source.operands)?;
        for (_, scalar) in ctx.admit_iter(candidates, "attach SLDPRT circle center scalars")? {
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

/// Two-operand scalars with a named display value, keyed by feature, name
/// value, and first operand kind and index, each group in lane order with the
/// scalars' lane positions.
type CenterScalarGroups<'a> = HashMap<
    (Option<&'a str>, &'a str, FeatureInputOperandKind, u16),
    Vec<(usize, &'a FeatureInputScalar)>,
>;

fn center_scalar_groups<'a>(
    ctx: &DecodeContext<'_>,
    scalars: &'a [FeatureInputScalar],
    names: &HashMap<&'a str, &'a str>,
) -> Result<CenterScalarGroups<'a>, CodecError> {
    const OPERATION: &str = "index SLDPRT circle center scalars";
    let mut groups = HashMap::new();
    for (index, scalar) in ctx.admit_iter(scalars, OPERATION)?.enumerate() {
        let [first, _] = scalar.operands.as_slice() else {
            continue;
        };
        let Some(name) = ctx
            .get_hash_map(names, scalar.name.as_str(), OPERATION)?
            .copied()
        else {
            continue;
        };
        ctx.push_hash_group(
            &mut groups,
            (
                scalar.feature_ref.as_deref(),
                name,
                first.kind,
                first.entity_index,
            ),
            (index, scalar),
            OPERATION,
            OPERATION,
        )?;
    }
    Ok(groups)
}

fn same_scalar_operands(
    ctx: &DecodeContext<'_>,
    left: &FeatureInputScalar,
    right: &FeatureInputScalar,
) -> Result<bool, CodecError> {
    if left.operands.len() != right.operands.len() {
        return Ok(false);
    }
    if !ctx.all_by(
        left.operands.iter().zip(right.operands.iter()),
        |(left, right)| {
            if left.kind != right.kind || left.entity_index != right.entity_index {
                return Ok(false);
            }
            let same_ref = match (left.entity_ref.as_deref(), right.entity_ref.as_deref()) {
                (Some(left), Some(right)) => {
                    ctx.equal(left, right, "compare SLDPRT relation scalar references")?
                }
                (None, None) => true,
                _ => false,
            };
            Ok(same_ref)
        },
        "compare SLDPRT relation scalar operands",
    )? {
        return Ok(false);
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
    if scalars.len() > 1 {
        ctx.sort_unstable_by(
            &mut scalars,
            |value| &value.offset,
            Ord::cmp,
            "sort SLDPRT relation scalars",
        )?;
    }
    let mut names: Option<HashMap<&str, &str>> = None;
    let Some(first_id) = relation.scalar_refs().first() else {
        return Ok(None);
    };
    let Some(first) = ctx
        .find_by(
            scalars.iter().copied(),
            |scalar| {
                ctx.equal(
                    scalar.id.as_str(),
                    first_id.as_str(),
                    "find SLDPRT relation display scalar",
                )
            },
            "find SLDPRT relation display scalar",
        )?
        .filter(|scalar| scalar.role == FeatureInputScalarRole::Display)
    else {
        return Ok(None);
    };
    // No following relation class states no upper bound on this relation.
    let mut next_relation_offset = None;
    let mut first_handle_offset = None;
    for class in ctx.admit_iter(&lane.classes, "find SLDPRT circle relation end")? {
        if class.offset > first.offset && relation_family(&class.name).is_some() {
            next_relation_offset = Some(
                next_relation_offset.map_or(class.offset, |limit: u64| limit.min(class.offset)),
            );
        }
        if class.offset > first.offset && class.name == "sgEntHandle"
        {
            first_handle_offset = Some(
                first_handle_offset.map_or(class.offset, |offset: u64| offset.min(class.offset)),
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
        // A handle class after the first scalar and before the display scalar.
        let declared_handle = first_handle_offset.is_some_and(|offset| offset < display.offset);
        let qualified_pair = declared_handle
            && display.role == FeatureInputScalarRole::Display
            && driving.role == FeatureInputScalarRole::Driving
            && first.offset < display.offset
            && next_relation_offset.is_none_or(|limit| driving.offset < limit)
            && match (
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
        if qualified_pair
            && same_scalar_operands(ctx, display, first)?
            && same_scalar_operands(ctx, driving, first)?
        {
            let names = match &mut names {
                Some(index) => index,
                slot @ None => {
                    slot.insert(temporary_storage.with_storage(|| last_name_values(ctx, lane))?)
                }
            };
            let left_name = ctx.get_hash_map(
                names,
                display.name.as_str(),
                "find SLDPRT circle display name",
            )?;
            let right_name = ctx.get_hash_map(
                names,
                driving.name.as_str(),
                "find SLDPRT circle driving name",
            )?;
            let same_name = match (left_name, right_name) {
                (Some(left), Some(right)) => {
                    ctx.equal(left, right, "compare SLDPRT circle scalar names")?
                }
                _ => false,
            };
            if same_name {
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
    let mut scalars: Option<HashMap<&str, &FeatureInputScalar>> = None;
    let mut names: Option<HashMap<&str, &str>> = None;
    let mut candidates = BTreeMap::<(String, String), Vec<usize>>::new();
    for (index, relation) in ctx
        .admit_iter(&*relations, "match SLDPRT detached relation candidates")?
        .enumerate()
    {
        if relation.parameter_scalar_ref().is_some() {
            continue;
        }
        let scalars = match &mut scalars {
            Some(index) => index,
            slot @ None => {
                slot.insert(temporary.with_storage(|| last_scalars_by_id(ctx, &lane.scalars))?)
            }
        };
        let mut first_name = None;
        let names_match = ctx.all_by(
            relation.scalar_refs(),
            |id| {
                let Some(scalar) = ctx.get_hash_map(scalars, id.as_str(), INDEX)? else {
                    return Ok(true);
                };
                if scalar.role != FeatureInputScalarRole::Display {
                    return Ok(true);
                }
                let names = match &mut names {
                    Some(index) => index,
                    slot @ None => {
                        slot.insert(temporary.with_storage(|| last_name_values(ctx, lane))?)
                    }
                };
                let Some(name) = ctx
                    .get_hash_map(names, scalar.name.as_str(), INDEX)?
                    .copied()
                else {
                    return Ok(true);
                };
                if let Some(first) = first_name {
                    if !ctx.equal(name, first, "compare SLDPRT detached relation names")? {
                        return Ok(false);
                    }
                } else {
                    first_name = Some(name);
                }
                Ok(true)
            },
            "compare SLDPRT detached relation names",
        )?;
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
            ctx.push_btree_group(
                &mut candidates,
                key,
                index,
                INDEX,
                "collect SLDPRT detached relation candidates",
            )
        })?;
    }
    let mut drivers: Option<HashMap<(&str, &str), Vec<&FeatureInputScalar>>> = None;
    for (key, relation_indices) in
        ctx.admit_iter(&candidates, "attach SLDPRT detached relation drivers")?
    {
        let [relation_index] = relation_indices.as_slice() else {
            continue;
        };
        let drivers = match &mut drivers {
            Some(index) => index,
            slot @ None => {
                let Some(names) = names.as_ref() else {
                    continue;
                };
                // Claim membership is needed only while building the driver
                // table. It must die before any relation membership changes.
                let mut claimed_storage = ctx.reserve_scoped(0, INDEX)?;
                let mut claimed = HashSet::new();
                for relation in ctx.admit_iter(&*relations, INDEX)? {
                    for id in ctx.admit_iter(relation.scalar_refs(), INDEX)? {
                        claimed_storage.with_storage(|| {
                            ctx.insert_hash_set(&mut claimed, id.as_str(), INDEX)
                        })?;
                    }
                }
                let mut built = HashMap::new();
                for scalar in
                    ctx.admit_iter(&lane.scalars, "match SLDPRT detached relation drivers")?
                {
                    if scalar.role != FeatureInputScalarRole::Driving
                        || !scalar.operands.is_empty()
                        || ctx.contains_hash_set(&claimed, scalar.id.as_str(), INDEX)?
                    {
                        continue;
                    }
                    let feature = scalar.feature_ref.as_deref();
                    let name = ctx
                        .get_hash_map(names, scalar.name.as_str(), INDEX)?
                        .copied();
                    let (Some(feature), Some(name)) = (feature, name) else {
                        continue;
                    };
                    temporary.with_storage(|| {
                        ctx.push_hash_group(
                            &mut built,
                            (feature, name),
                            scalar,
                            INDEX,
                            "collect SLDPRT detached relation drivers",
                        )
                    })?;
                }
                drop(claimed);
                drop(claimed_storage);
                slot.insert(built)
            }
        };
        let Some([driver]) = ctx
            .get_hash_map(drivers, &(key.0.as_str(), key.1.as_str()), INDEX)?
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

pub(super) fn relation_uses_solver_points(relation: &FeatureInputRelationInstance) -> bool {
    relation.family == FeatureInputRelationFamily::PointPointDistance
        && matches!(relation.operands.as_slice(), [first, second]
            if is_solver_point_operand(first.kind) && is_solver_point_operand(second.kind))
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
    scalars: &HashMap<&str, &FeatureInputScalar>,
) -> Result<Option<cadmpeg_ir::scalar::FiniteReal>, CodecError> {
    let scalar_id = relation
        .parameter_scalar_ref()
        .or(relation.display_scalar_ref());
    let Some(scalar_id) = scalar_id else {
        return Ok(None);
    };
    Ok(ctx
        .get_hash_map(scalars, scalar_id, "find SLDPRT relation target scalar")?
        .map(|scalar| scalar.value))
}

/// Each feature's markers of one lane, in offset and ordinal order.
type FeatureMarkers<'a> = HashMap<&'a str, Vec<&'a crate::records::SketchInputEntity>>;

fn feature_markers<'a>(
    ctx: &DecodeContext<'_>,
    lane: &'a FeatureInputLane,
) -> Result<FeatureMarkers<'a>, CodecError> {
    const OPERATION: &str = "index SLDPRT relation feature entities";
    // Group the markers in offset and ordinal order so each group is ordered.
    let _ordered_storage;
    let mut ordered;
    (ordered, _ordered_storage) = ctx.with_scoped_storage(OPERATION, || {
        ctx.collect_vec(lane.sketch_entities.iter(), OPERATION)
    })?;
    ctx.sort_unstable_by_key(
        &mut ordered,
        |value| (value.offset(), value.ordinal()),
        Ord::cmp,
        "sort SLDPRT relation feature entities",
    )?;
    let mut groups = HashMap::new();
    for entity in ctx.admit_iter(ordered, OPERATION)? {
        if let Some(feature) = entity.feature_ref.as_deref() {
            ctx.push_hash_group(&mut groups, feature, entity, OPERATION, OPERATION)?;
        }
    }
    Ok(groups)
}

fn is_finite_point(entity: &crate::records::SketchInputEntity) -> bool {
    entity.coordinates_m.is_some()
        && matches!(
            entity.kind(),
            crate::records::SketchInputKind::Point
                | crate::records::SketchInputKind::ConstrainedPoint
        )
}

fn push_point_candidate<'a>(
    ctx: &DecodeContext<'_>,
    candidates: &mut Vec<&'a crate::records::SketchInputEntity>,
    seen: &mut HashSet<&'a str>,
    seen_storage: &mut ScopedReservation<'_>,
    candidate: Option<&'a crate::records::SketchInputEntity>,
) -> Result<(), CodecError> {
    let Some(candidate) = candidate else {
        return Ok(());
    };
    if !is_finite_point(candidate) {
        return Ok(());
    }
    if !seen_storage.with_storage(|| {
        ctx.insert_hash_set(seen, candidate.id(), "index SLDPRT relation point identity")
    })? {
        return Ok(());
    }
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
            if !is_finite_point(entity)
                || !ctx.equal(
                    entity.id(),
                    entity_ref,
                    "compare SLDPRT explicit point references",
                )?
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
    let mut seen_storage = ctx.reserve_scoped(0, "SLDPRT dynamic point identity workspace")?;
    let mut seen = HashSet::new();
    push_point_candidate(
        ctx,
        &mut candidates,
        &mut seen,
        &mut seen_storage,
        entities.get(address).copied(),
    )?;
    let mut finite_index = 0usize;
    let coordinate_point = ctx.find_by(
        entities.iter().copied(),
        |entity| {
            if !is_finite_point(entity) {
                return Ok(false);
            }
            let selected = finite_index == address;
            if !selected {
                finite_index += 1;
            }
            Ok(selected)
        },
        "find SLDPRT coordinate point candidate",
    )?;
    push_point_candidate(
        ctx,
        &mut candidates,
        &mut seen,
        &mut seen_storage,
        coordinate_point,
    )?;
    for entity in ctx.admit_iter(entities, "match SLDPRT dynamic point identities")? {
        if entity.object_index() == Some(u32::from(operand.entity_index))
            || entity.local_id() == Some(u32::from(operand.entity_index))
        {
            push_point_candidate(
                ctx,
                &mut candidates,
                &mut seen,
                &mut seen_storage,
                Some(entity),
            )?;
        }
    }
    drop(seen);
    drop(seen_storage);
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
    let Some(_) = start.checked_add(1) else {
        return Ok(None);
    };
    let mut point_index = 0usize;
    let mut remaining = entities.iter().copied();
    let first = ctx.find_by(
        &mut remaining,
        |entity| {
            if !is_finite_point(entity) {
                return Ok(false);
            }
            let selected = point_index == start;
            if !selected {
                point_index += 1;
            }
            Ok(selected)
        },
        "select SLDPRT relation line points",
    )?;
    let second = if first.is_some() {
        ctx.find_by(
            &mut remaining,
            |entity| Ok(is_finite_point(entity)),
            "select SLDPRT relation line points",
        )?
    } else {
        None
    };
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
    for operand in ctx.admit_iter(&mut relation.operands, "clear SLDPRT relation operands")? {
        operand.entity_ref = None;
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
    ctx.any_by(
        entities,
        |entity| {
            Ok(matches!(
                entity.kind(),
                crate::records::SketchInputKind::LineOrCircle
                    | crate::records::SketchInputKind::Arc
                    | crate::records::SketchInputKind::Relation(_)
            ) && ctx.equal(
                entity.id(),
                entity_ref,
                "compare SLDPRT relation curve references",
            )?)
        },
        "find SLDPRT relation curve reference",
    )
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
    let coordinate_roster = std::cell::OnceCell::new();
    let coordinate_points = || {
        let (points, _) = match coordinate_roster.get() {
            Some(points) => points,
            None => {
                const OPERATION: &str = "collect SLDPRT relation coordinate points";
                let built = ctx.with_scoped_storage(OPERATION, || {
                    let mut points = Vec::new();
                    for entity in ctx.admit_iter(entities, OPERATION)? {
                        if is_finite_point(entity) {
                            ctx.push_vec(&mut points, *entity, OPERATION)?;
                        }
                    }
                    Ok::<_, CodecError>(points)
                })?;
                coordinate_roster.get_or_init(|| built)
            }
        };
        Ok::<_, CodecError>(points.as_slice())
    };
    let mut matches = temporary_storage.with_storage(|| {
        dynamic_point_matches(
            ctx,
            &first_candidates,
            &second_candidates,
            target,
            horizontal,
        )
    })?;
    if matches.is_empty() && (first.entity_ref.is_none() || second.entity_ref.is_none()) {
        let coordinate_points = coordinate_points()?;
        let first_fallback = if first.entity_ref.is_none() {
            coordinate_points
        } else {
            &first_candidates
        };
        let second_fallback = if second.entity_ref.is_none() {
            coordinate_points
        } else {
            &second_candidates
        };
        matches = temporary_storage.with_storage(|| {
            dynamic_point_matches(ctx, first_fallback, second_fallback, target, horizontal)
        })?;
    }
    if matches.is_empty() && (first.entity_ref.is_some() || second.entity_ref.is_some()) {
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
        matches = temporary_storage.with_storage(|| {
            dynamic_point_matches(
                ctx,
                first_relaxed.as_deref().unwrap_or(&first_candidates),
                second_relaxed.as_deref().unwrap_or(&second_candidates),
                target,
                horizontal,
            )
        })?;
    }
    if matches.is_empty() && (first.entity_ref.is_some() || second.entity_ref.is_some()) {
        let coordinate_points = coordinate_points()?;
        let first_relaxed = if first.entity_ref.is_some() {
            coordinate_points
        } else {
            &first_candidates
        };
        let second_relaxed = if second.entity_ref.is_some() {
            coordinate_points
        } else {
            &second_candidates
        };
        matches = temporary_storage.with_storage(|| {
            dynamic_point_matches(ctx, first_relaxed, second_relaxed, target, horizontal)
        })?;
    }
    drop(coordinate_roster);
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
    if first_operand.entity_index == second_operand.entity_index {
        clear_relation_operands(ctx, relation)?;
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

    // Built when the first relation needs them.
    let mut scalars_by_id: Option<HashMap<&str, &FeatureInputScalar>> = None;
    let mut markers_by_feature: Option<FeatureMarkers<'_>> = None;
    for relation in ctx.admit_iter(&mut *relations, "scan SLDPRT relation geometry operands")? {
        let dynamic = relation_uses_dynamic_operands(relation);
        let point_operands_unbound = if !dynamic
            && matches!(
                relation.family,
                FeatureInputRelationFamily::PointPointDistance
                    | FeatureInputRelationFamily::PointPointHorizontalDistance
                    | FeatureInputRelationFamily::PointPointVerticalDistance
            ) {
            ctx.all_by(
                &relation.operands,
                |operand| Ok(operand.entity_ref.is_none()),
                "check SLDPRT unbound point relation operands",
            )?
        } else {
            false
        };
        if !dynamic && !point_operands_unbound {
            continue;
        }
        if relation.family == FeatureInputRelationFamily::CircleDiameter {
            continue;
        }
        if scalars_by_id.is_none() {
            scalars_by_id =
                Some(temporary_storage.with_storage(|| first_scalars_by_id(ctx, &lane.scalars))?);
        }
        let target = match &scalars_by_id {
            Some(scalars) => relation_target_value(ctx, relation, scalars)?,
            None => None,
        };
        let Some(target) = target else {
            if dynamic {
                clear_relation_operands(ctx, relation)?;
            }
            continue;
        };
        if relation_uses_solver_points(relation) {
            continue;
        }
        if target.get() < 0.0 {
            if dynamic {
                clear_relation_operands(ctx, relation)?;
            }
            continue;
        }
        if markers_by_feature.is_none() {
            markers_by_feature =
                Some(temporary_storage.with_storage(|| feature_markers(ctx, lane))?);
        }
        let entities = match &markers_by_feature {
            Some(markers) => ctx
                .get_hash_map(
                    markers,
                    relation.feature_ref.as_str(),
                    "match SLDPRT relation feature entities",
                )?
                .map_or(&[][..], Vec::as_slice),
            None => &[][..],
        };
        match relation.family {
            FeatureInputRelationFamily::PointPointDistance => {
                bind_dynamic_point_relation(ctx, relation, entities, target.get(), None)?;
            }
            FeatureInputRelationFamily::PointPointHorizontalDistance => {
                bind_dynamic_point_relation(ctx, relation, entities, target.get(), Some(true))?;
            }
            FeatureInputRelationFamily::PointPointVerticalDistance => {
                bind_dynamic_point_relation(ctx, relation, entities, target.get(), Some(false))?;
            }
            FeatureInputRelationFamily::PointLineDistance => {
                bind_dynamic_point_line_relation(ctx, relation, entities, target.get())?;
            }
            FeatureInputRelationFamily::LineLineDistance => {
                bind_dynamic_line_relation(ctx, relation, entities, target.get(), false)?;
            }
            FeatureInputRelationFamily::Angle => {
                bind_dynamic_line_relation(ctx, relation, entities, target.get(), true)?;
            }
            FeatureInputRelationFamily::CircleDiameter => {}
        }
    }
    Ok(())
}

// The scalar trailer layouts are fixed-width records: every check below reads a
// constant number of bytes, so none takes a decode context.

pub(super) fn scalar_role(payload: &[u8], trailer_offset: usize) -> FeatureInputScalarRole {
    let shifted_layout = shifted_value_only_scalar_trailer(payload, trailer_offset);
    let fixed_layout = payload.get(trailer_offset..trailer_offset + 3) == Some(&[0, 0, 0])
        && payload.get(trailer_offset + 7..trailer_offset + 21) == Some(&[0; 14])
        && payload.get(trailer_offset + 24..trailer_offset + 29) == Some(&[0, 0, 0, 2, 0]);
    let role_offset = if shifted_layout {
        trailer_offset + shifted_trailer::ROLE
    } else if compact_scalar_layout(payload, trailer_offset) {
        trailer_offset + 27
    } else if fixed_layout {
        trailer_offset + 29
    } else if legacy_scalar_layout(payload, trailer_offset) {
        trailer_offset + 30
    } else {
        return FeatureInputScalarRole::Native;
    };
    match payload.get(role_offset) {
        Some(0) => FeatureInputScalarRole::Driving,
        Some(1) => FeatureInputScalarRole::Display,
        _ => FeatureInputScalarRole::Native,
    }
}

pub(super) fn shifted_value_only_scalar_trailer(payload: &[u8], trailer_offset: usize) -> bool {
    payload.get(
        trailer_offset + shifted_trailer::ZERO_PREFIX..trailer_offset + shifted_trailer::OBJECT_ID,
    ) == Some(&[0, 0, 0])
        && payload.get(
            trailer_offset + shifted_trailer::ZERO_OBJECT_TAIL
                ..trailer_offset + shifted_trailer::LAYOUT_MARKER,
        ) == Some(&[0; shifted_trailer::LAYOUT_MARKER - shifted_trailer::ZERO_OBJECT_TAIL])
        && payload.get(
            trailer_offset + shifted_trailer::LAYOUT_MARKER..trailer_offset + shifted_trailer::ROLE,
        ) == Some(&[1, 0, 0, 0, 2, 0])
        && payload
            .get(trailer_offset + shifted_trailer::ROLE)
            .is_some_and(|role| *role <= 1)
        && payload
            .get(trailer_offset + shifted_trailer::ZERO_TAIL..trailer_offset + shifted_trailer::LEN)
            == Some(&[0; shifted_trailer::LEN - shifted_trailer::ZERO_TAIL])
}

fn shifted_value_only_scalar_layout(payload: &[u8], trailer_offset: usize) -> bool {
    shifted_value_only_scalar_trailer(payload, trailer_offset)
        && payload
            .get(trailer_offset + 35..trailer_offset + 47)
            .is_some_and(|cell| {
                cell[0..2] != [0, 0]
                    && cell[0..2] != [0xff, 0xff]
                    && cell[4..8] == [0xff; 4]
                    && cell[8..12] == [0; 4]
            })
}

pub(super) fn compact_scalar_layout(payload: &[u8], trailer_offset: usize) -> bool {
    !shifted_value_only_scalar_layout(payload, trailer_offset)
        && payload.get(trailer_offset..trailer_offset + 3) == Some(&[0, 0, 0])
        && payload.get(trailer_offset + 7..trailer_offset + 21) == Some(&[0; 14])
        && payload.get(trailer_offset + 21..trailer_offset + 27) == Some(&[1, 0, 0, 0, 2, 0])
        && payload.get(trailer_offset + 28..trailer_offset + 35) == Some(&[0; 7])
        && payload.get(trailer_offset + 39..trailer_offset + 43) == Some(&[0xff; 4])
        && payload.get(trailer_offset + 47..trailer_offset + 51) == Some(&[0xff; 4])
}

pub(super) fn legacy_scalar_layout(payload: &[u8], trailer_offset: usize) -> bool {
    payload.get(trailer_offset..trailer_offset + 3) == Some(&[0, 0, 0])
        && payload.get(trailer_offset + 7..trailer_offset + 24) == Some(&[0; 17])
        && payload.get(trailer_offset + 24..trailer_offset + 30) == Some(&[0x0f, 0, 0, 0, 2, 0])
}

#[cfg(test)]
mod point_candidate_tests {
    use crate::records::{
        FeatureInputOperand, FeatureInputOperandKind, SketchInputEntity, SketchInputKind,
    };
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    fn fixture() -> ([SketchInputEntity; 2], FeatureInputOperand) {
        let mut first = SketchInputEntity::new("a", "lane", 0, 0, SketchInputKind::Point);
        first.coordinates_m = cadmpeg_ir::units::FiniteVector::new([0.0, 0.0]);
        let mut second = SketchInputEntity::new("b", "lane", 1, 1, SketchInputKind::Point)
            .with_test_identity(Some(0), None);
        second.coordinates_m = cadmpeg_ir::units::FiniteVector::new([1.0, 0.0]);
        (
            [first, second],
            FeatureInputOperand {
                offset: 0,
                reference_ref: "reference".into(),
                kind: FeatureInputOperandKind::D6,
                entity_index: 0,
                entity_ref: None,
            },
        )
    }

    #[test]
    fn point_identity_table_ends_before_returned_candidates() {
        assert_eq!(std::mem::size_of::<&str>(), 16);
        assert_eq!(std::mem::size_of::<&SketchInputEntity>(), 8);
        let (markers, operand) = fixture();
        let roster = [&markers[0], &markers[1]];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // Four &str buckets, fifteen alignment bytes and twenty control bytes.
        policy.limits.max_materialized_bytes = 99;
        // The returned Vec starts with four borrowed-pointer slots.
        policy.limits.max_retained_bytes = 32;
        policy.limits.max_collection_items = 4;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let candidates = super::dynamic_point_candidates(&ctx, &roster, &operand, true).unwrap();
        assert_eq!(candidates.len(), 2);
        assert!(std::ptr::eq(candidates[0], &raw const markers[0]));
        assert!(std::ptr::eq(candidates[1], &raw const markers[1]));
        let released = ctx
            .reserve_scoped(99, "released point identity table")
            .unwrap();
        drop(released);
        assert_eq!(ctx.resource_refusal(), None);
        drop(candidates);
    }

    #[test]
    fn scoped_point_candidates_release_their_identity_table_each_call() {
        let (markers, operand) = fixture();
        let roster = [&markers[0], &markers[1]];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 99 + 32;
        policy.limits.max_collection_items = 4 * 64;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        for _ in 0..64 {
            let storage;
            let candidates;
            (candidates, storage) = ctx
                .with_scoped_storage("point candidate test", || {
                    super::dynamic_point_candidates(&ctx, &roster, &operand, true)
                })
                .unwrap();
            assert_eq!(candidates.len(), 2);
            assert!(std::ptr::eq(candidates[0], &raw const markers[0]));
            assert!(std::ptr::eq(candidates[1], &raw const markers[1]));
            let released = ctx
                .reserve_scoped(99, "released point identity table")
                .unwrap();
            drop(released);
            drop(candidates);
            drop(storage);
            let released = ctx
                .reserve_scoped(131, "released point candidates")
                .unwrap();
            drop(released);
        }
        assert_eq!(ctx.resource_refusal(), None);
    }

    #[test]
    fn point_candidate_storage_refuses_before_growth_and_stays_fused() {
        let (markers, operand) = fixture();
        let roster = [&markers[0], &markers[1]];
        for (dimension, cap, used, additional, operation) in [
            (
                ResourceDimension::MaterializedBytes,
                98,
                0,
                99,
                "index SLDPRT relation point identity",
            ),
            (
                ResourceDimension::RetainedBytes,
                31,
                0,
                32,
                "collect SLDPRT dynamic point candidates",
            ),
            (
                ResourceDimension::CollectionItems,
                3,
                3,
                1,
                "collect SLDPRT dynamic point candidates",
            ),
        ] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = 99;
            policy.limits.max_retained_bytes = 32;
            policy.limits.max_collection_items = 4;
            match dimension {
                ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = cap,
                ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = cap,
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = cap,
                _ => panic!("point candidate storage dimension"),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let Err(CodecError::ResourceLimit(limit)) =
                super::dynamic_point_candidates(&ctx, &roster, &operand, true)
            else {
                panic!("point candidate storage refusal");
            };
            assert_eq!(limit.dimension, dimension);
            assert_eq!(limit.operation, operation);
            assert_eq!(limit.limit, cap);
            assert_eq!(limit.used, used);
            assert_eq!(limit.additional, additional);
            assert_eq!(ctx.resource_refusal(), Some(limit));
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky))
                if sticky == limit)
            );
        }
    }
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
mod tests;
