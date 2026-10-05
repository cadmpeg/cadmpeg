//! History feature class binding.

use super::is_class_token;
use super::operations::repeated_class_token;
use super::scalars::ObjectNames;
use crate::classification::{native_object_class, NativeClassKind};
use crate::records::{
    Feature, FeatureHistory, FeatureInputClass, FeatureInputClassRole, FeatureInputLane,
};
use cadmpeg_core::decode::{u64_from_index, DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use std::collections::{BTreeMap, HashMap, HashSet};

use crate::records::ObjectId;

/// The offset of the object name that directly follows a class declaration.
fn direct_name_offset(class: &FeatureInputClass) -> Option<u64> {
    class
        .offset
        .checked_add(6)?
        .checked_add(u64_from_index(class.name.len()))
}

/// Recognize the source-less legacy plane/origin/sketch/extrusion prefix.
fn idless_legacy_startup_shape(
    ctx: &DecodeContext<'_>,
    records: &[Feature],
) -> Result<bool, CodecError> {
    let [front, top, right, origin, sketch, extrusion] = records else {
        return Ok(false);
    };
    let records = [front, top, right, origin, sketch, extrusion];
    for record in records {
        if record.input_class.is_some()
            || record.source_id.is_some()
            || record.tree_parent.is_some()
            || !ctx.eq_ignore_ascii_case(
                &record.xml_tag,
                "Feature",
                "check SLDPRT idless startup tag",
            )?
            || !record.properties.is_empty()
        {
            return Ok(false);
        }
    }
    if [front, top, right, origin]
        .into_iter()
        .any(|record| !record.parameters.is_empty())
        || extrusion.parameters.is_empty()
        || front.kind.is_empty()
        || !ctx.equal(&front.kind, &top.kind, "compare SLDPRT startup kinds")?
        || !ctx.equal(&front.kind, &right.kind, "compare SLDPRT startup kinds")?
        || ctx.equal(&origin.kind, &front.kind, "compare SLDPRT startup kinds")?
        || ctx.equal(&sketch.kind, &origin.kind, "compare SLDPRT startup kinds")?
        || ctx.equal(
            &extrusion.kind,
            &sketch.kind,
            "compare SLDPRT startup kinds",
        )?
    {
        return Ok(false);
    }
    Ok(records
        .windows(2)
        .all(|pair| pair[1].ordinal == pair[0].ordinal + 1))
}

/// Whether each history feature, in history order, carries a nonempty name
/// that no other feature repeats.
fn unique_feature_names(
    ctx: &DecodeContext<'_>,
    temporary: &mut ScopedReservation<'_>,
    histories: &[FeatureHistory],
) -> Result<Vec<bool>, CodecError> {
    let mut once = HashMap::<&str, bool>::new();
    for history in ctx.admit_iter(histories, "count SLDPRT history feature names")? {
        for feature in ctx.admit_iter(&history.features, "count SLDPRT history feature names")? {
            if feature.name.is_empty() {
                continue;
            }
            if let Some(single) = ctx.get_mut_hash_map(
                &mut once,
                feature.name.as_str(),
                "count SLDPRT history feature names",
            )? {
                *single = false;
                continue;
            }
            temporary.with_storage(|| {
                ctx.insert_hash_map(
                    &mut once,
                    feature.name.as_str(),
                    true,
                    "count SLDPRT history feature names",
                )
            })?;
        }
    }
    let mut unique = Vec::new();
    for history in ctx.admit_iter(histories, "mark SLDPRT unique history feature names")? {
        for feature in ctx.admit_iter(
            &history.features,
            "mark SLDPRT unique history feature names",
        )? {
            let single = !feature.name.is_empty()
                && ctx.get_hash_map(
                    &once,
                    feature.name.as_str(),
                    "find SLDPRT history feature-name count",
                )? == Some(&true);
            temporary.with_storage(|| {
                ctx.push_vec(
                    &mut unique,
                    single,
                    "mark SLDPRT unique history feature names",
                )
            })?;
        }
    }
    Ok(unique)
}

/// Lane names that carry a repeated class token, indexed by the object id
/// and by the text that bind a history feature to them.
struct TokenNames<'lanes> {
    by_object: HashMap<u32, Vec<(&'lanes str, u16)>>,
    by_text: HashMap<&'lanes str, Vec<(&'lanes str, u16)>>,
    /// Whether exactly one lane name with an object id carries each text.
    single_object_text: HashMap<&'lanes str, Option<()>>,
}

impl<'lanes> TokenNames<'lanes> {
    fn new(
        ctx: &DecodeContext<'_>,
        temporary: &mut ScopedReservation<'_>,
        lanes: &'lanes [FeatureInputLane],
    ) -> Result<Self, CodecError> {
        let mut direct_name_offsets = HashSet::new();
        for lane in ctx.admit_iter(lanes, "scan SLDPRT direct class lanes")? {
            for class in ctx.admit_iter(&lane.classes, "index SLDPRT direct class names")? {
                let Some(offset) = direct_name_offset(class) else {
                    continue;
                };
                temporary.with_storage(|| {
                    ctx.insert_hash_set(
                        &mut direct_name_offsets,
                        (lane.id.as_str(), offset),
                        "index SLDPRT direct class names",
                    )
                })?;
            }
        }
        let mut by_object = HashMap::new();
        let mut by_text = HashMap::new();
        let mut single_object_text = HashMap::<&str, Option<()>>::new();
        for lane in ctx.admit_iter(lanes, "scan SLDPRT token-binding lanes")? {
            for name in ctx.admit_iter(&lane.names, "index SLDPRT token-binding names")? {
                let object_id = name.object_id.and_then(ObjectId::value);
                if object_id.is_some() {
                    if let Some(single) = ctx.get_mut_hash_map(
                        &mut single_object_text,
                        name.value.as_str(),
                        "index SLDPRT object name texts",
                    )? {
                        *single = None;
                    } else {
                        temporary.with_storage(|| {
                            ctx.insert_hash_map(
                                &mut single_object_text,
                                name.value.as_str(),
                                Some(()),
                                "index SLDPRT object name texts",
                            )
                        })?;
                    }
                }
                if ctx.contains_hash_set(
                    &direct_name_offsets,
                    &(lane.id.as_str(), name.offset),
                    "find SLDPRT direct name offset",
                )? {
                    continue;
                }
                let Some(token) = usize::try_from(name.offset)
                    .ok()
                    .and_then(|offset| repeated_class_token(&lane.native_payload, offset))
                else {
                    continue;
                };
                let entry = (lane.id.as_str(), token);
                if let Some(object_id) = object_id {
                    temporary.with_storage(|| {
                        ctx.push_hash_group(
                            &mut by_object,
                            object_id,
                            entry,
                            "index SLDPRT token-binding names",
                            "index SLDPRT token-binding names",
                        )
                    })?;
                }
                temporary.with_storage(|| {
                    ctx.push_hash_group(
                        &mut by_text,
                        name.value.as_str(),
                        entry,
                        "index SLDPRT token-binding names",
                        "index SLDPRT token-binding names",
                    )
                })?;
            }
        }
        Ok(Self {
            by_object,
            by_text,
            single_object_text,
        })
    }

    /// The token-carrying names that bind `feature`: those with its object id,
    /// or, for a feature without one whose name is unique among the history
    /// features and among the lane names with an object id, those with its text.
    fn of(
        &self,
        ctx: &DecodeContext<'_>,
        feature: &Feature,
        name_is_unique: bool,
    ) -> Result<Option<&[(&'lanes str, u16)]>, CodecError> {
        let group = match feature.source_value() {
            Some(object_id) => ctx.get_hash_map(
                &self.by_object,
                &object_id,
                "find SLDPRT token-binding object names",
            )?,
            None => {
                if !name_is_unique
                    || ctx
                        .get_hash_map(
                            &self.single_object_text,
                            feature.name.as_str(),
                            "find SLDPRT object name text",
                        )?
                        .and_then(Option::as_ref)
                        .is_none()
                {
                    return Ok(None);
                }
                ctx.get_hash_map(
                    &self.by_text,
                    feature.name.as_str(),
                    "find SLDPRT token-binding text names",
                )?
            }
        };
        Ok(group.map(Vec::as_slice))
    }
}

/// Bind Keywords history records to their serialized feature-input object classes.
pub(crate) fn bind_history_classes(
    ctx: &DecodeContext<'_>,
    histories: &mut [FeatureHistory],
    lanes: &[FeatureInputLane],
) -> Result<(), CodecError> {
    let mut temporary = ctx.reserve_scoped(0, "SLDPRT history class workspace")?;
    for history in ctx.admit_iter(&mut *histories, "clear SLDPRT history feature classes")? {
        for feature in ctx.admit_iter(
            &mut history.features,
            "clear SLDPRT history feature classes",
        )? {
            feature.input_class = None;
        }
    }

    let mut classes_by_object = HashMap::<u32, Vec<&str>>::new();
    let mut direct_classes_by_name = BTreeMap::<&str, Vec<&str>>::new();
    for lane in ctx.admit_iter(lanes, "scan SLDPRT input class lanes")? {
        let names_by_offset = temporary.with_storage(|| {
            ctx.collect_hash_map(
                lane.names.iter().map(|name| (name.offset, name)),
                "index SLDPRT input class names",
            )
        })?;
        for class in ctx.admit_iter(&lane.classes, "scan SLDPRT lane classes")? {
            let Some(name_offset) = direct_name_offset(class) else {
                continue;
            };
            let Some(name) =
                ctx.get_hash_map(&names_by_offset, &name_offset, "find SLDPRT class name")?
            else {
                continue;
            };
            if let Some(object_id) = name.object_id.and_then(ObjectId::value) {
                temporary.with_storage(|| {
                    ctx.push_hash_group(
                        &mut classes_by_object,
                        object_id,
                        class.name.as_str(),
                        "bind SLDPRT history classes",
                        "bind SLDPRT history classes",
                    )
                })?;
            }
            if class.role() != FeatureInputClassRole::Native {
                temporary.with_storage(|| {
                    ctx.push_btree_group(
                        &mut direct_classes_by_name,
                        name.value.as_str(),
                        class.name.as_str(),
                        "bind SLDPRT history classes",
                        "bind SLDPRT history classes",
                    )
                })?;
            }
        }
    }

    for history in ctx.admit_iter(
        &mut *histories,
        "bind SLDPRT object classes to history features",
    )? {
        for feature in ctx.admit_iter(
            &mut history.features,
            "bind SLDPRT object classes to history features",
        )? {
            let Some(object_id) = feature.source_value() else {
                continue;
            };
            let Some(classes) = ctx.get_hash_map(
                &classes_by_object,
                &object_id,
                "find SLDPRT feature object classes",
            )?
            else {
                continue;
            };
            let Some((&first, rest)) = classes.split_first() else {
                continue;
            };
            if ctx.all_by(
                rest,
                |class| ctx.equal(*class, first, "compare SLDPRT object classes"),
                "check SLDPRT object class agreement",
            )? {
                feature.input_class = Some(copy_class_text(ctx, first)?);
            }
        }
    }

    for (_, classes) in ctx.admit_iter(&mut direct_classes_by_name, "sort SLDPRT bound classes")? {
        ctx.sort_unstable_by(
            classes,
            |value| value,
            Ord::cmp,
            "sort SLDPRT history class names",
        )?;
        ctx.dedup_vec(classes, "deduplicate SLDPRT history class names")?;
    }
    let unique_names = unique_feature_names(ctx, &mut temporary, histories)?;
    let mut unique = unique_names.iter();
    for history in ctx.admit_iter(&mut *histories, "bind SLDPRT unique history feature names")? {
        for feature in ctx.admit_iter(
            &mut history.features,
            "bind SLDPRT unique history feature names",
        )? {
            let name_is_unique = unique.next() == Some(&true);
            if !name_is_unique || feature.input_class.is_some() || feature.source_id.is_some() {
                continue;
            }
            let Some(classes) = ctx.get_btree_map(
                &direct_classes_by_name,
                feature.name.as_str(),
                "find SLDPRT direct class candidates",
            )?
            else {
                continue;
            };
            let [class] = classes.as_slice() else {
                continue;
            };
            feature.input_class = Some(copy_class_text(ctx, class)?);
        }
    }

    let mut cosmetic_thread_classes = BTreeMap::<String, Vec<&str>>::new();
    for lane in ctx.admit_iter(lanes, "scan SLDPRT cosmetic thread lanes")? {
        let mut declared = temporary.with_storage(|| {
            let classes = ctx.admit_iter(&lane.classes, "find SLDPRT cosmetic thread classes")?;
            ctx.collect_vec(
                classes
                    .filter(|class| {
                        native_object_class(&class.name) == NativeClassKind::CosmeticThread
                    })
                    .map(|class| class.name.as_str()),
                "collect SLDPRT cosmetic thread classes",
            )
        })?;
        ctx.stable_sort_by(
            &mut declared,
            |value| value,
            Ord::cmp,
            "sort SLDPRT declared classes",
        )?;
        ctx.dedup_vec(&mut declared, "deduplicate SLDPRT declared classes")?;
        let [class] = declared.as_slice() else {
            continue;
        };
        let direct_name_offsets = temporary.with_storage(|| {
            ctx.collect_hash_set(
                ctx.admit_iter(&lane.classes, "collect SLDPRT direct class offsets")?
                    .filter_map(direct_name_offset),
                "collect SLDPRT direct class offsets",
            )
        })?;
        let object_names = ObjectNames::new(ctx, lane)?;
        let mut groups = BTreeMap::<u16, Vec<&Feature>>::new();
        for history in ctx.admit_iter(&*histories, "scan SLDPRT cosmetic thread histories")? {
            for feature in
                ctx.admit_iter(&history.features, "scan SLDPRT cosmetic thread features")?
            {
                if feature.input_class.is_some() {
                    continue;
                }
                let Some(name) = object_names.of(ctx, feature)? else {
                    continue;
                };
                if ctx.contains_hash_set(
                    &direct_name_offsets,
                    &name.offset,
                    "find direct SLDPRT class name offsets",
                )? || name.object_id.and_then(ObjectId::value).is_none()
                    || !ctx.equal(&name.value, &feature.name, "compare SLDPRT feature names")?
                {
                    continue;
                }
                let Some(token) = usize::try_from(name.offset)
                    .ok()
                    .and_then(|offset| repeated_class_token(&lane.native_payload, offset))
                    .filter(|token| is_class_token(*token))
                else {
                    continue;
                };
                temporary.with_storage(|| {
                    ctx.push_btree_group(
                        &mut groups,
                        token,
                        feature,
                        "bind SLDPRT history classes",
                        "bind SLDPRT history classes",
                    )
                })?;
            }
        }
        for (_, features) in ctx.admit_iter(&groups, "scan SLDPRT cosmetic thread groups")? {
            if !ctx.all_by(
                features,
                |feature| cosmetic_thread_parameter_shape(ctx, feature),
                "check SLDPRT cosmetic thread group",
            )? {
                continue;
            }
            for feature in ctx.admit_iter(features, "bind SLDPRT cosmetic thread group")? {
                let id = temporary.with_storage(|| copy_class_text(ctx, &feature.id))?;
                temporary.with_storage(|| {
                    ctx.push_btree_group(
                        &mut cosmetic_thread_classes,
                        id,
                        *class,
                        "bind SLDPRT history classes",
                        "bind SLDPRT history classes",
                    )
                })?;
            }
        }
    }
    for (_, classes) in ctx.admit_iter(&mut cosmetic_thread_classes, "sort SLDPRT bound classes")? {
        ctx.stable_sort_by(
            classes,
            |value| value,
            Ord::cmp,
            "sort SLDPRT bound classes",
        )?;
        ctx.dedup_vec(classes, "deduplicate SLDPRT bound classes")?;
    }
    for history in ctx.admit_iter(&mut *histories, "bind SLDPRT cosmetic thread classes")? {
        for feature in
            ctx.admit_iter(&mut history.features, "bind SLDPRT cosmetic thread classes")?
        {
            if feature.input_class.is_some() {
                continue;
            }
            if let Some(classes) = ctx.get_btree_map(
                &cosmetic_thread_classes,
                &feature.id,
                "find SLDPRT cosmetic thread class",
            )? {
                let [class] = classes.as_slice() else {
                    continue;
                };
                feature.input_class = Some(copy_class_text(ctx, class)?);
            }
        }
    }

    let mut native_startups = Vec::<[&str; 6]>::new();
    for lane in ctx.admit_iter(lanes, "scan SLDPRT native startup lanes")? {
        let resolved = temporary.with_storage(|| {
            let classes = ctx.admit_iter(&lane.classes, "find SLDPRT native startup classes")?;
            ctx.collect_vec(
                classes.filter_map(|class| {
                    matches!(
                        native_object_class(&class.name),
                        NativeClassKind::ReferencePlane
                            | NativeClassKind::OriginProfileFeature
                            | NativeClassKind::ProfileFeature
                            | NativeClassKind::Extrusion
                    )
                    .then_some(class.name.as_str())
                }),
                "collect SLDPRT native startup classes",
            )
        })?;
        for classes in ctx
            .admit_iter(&resolved, "scan SLDPRT native startup class windows")?
            .windows(const { crate::nonzero(4) })
        {
            let [plane, origin, sketch, extrusion] = classes else {
                continue;
            };
            if native_object_class(plane) == NativeClassKind::ReferencePlane
                && native_object_class(origin) == NativeClassKind::OriginProfileFeature
                && native_object_class(sketch) == NativeClassKind::ProfileFeature
                && native_object_class(extrusion) == NativeClassKind::Extrusion
            {
                temporary.with_storage(|| {
                    ctx.push_vec(
                        &mut native_startups,
                        [plane, plane, plane, origin, sketch, extrusion],
                        "bind SLDPRT history classes",
                    )
                })?;
            }
        }
    }
    ctx.sort_unstable_by(
        &mut native_startups,
        |value| value,
        Ord::cmp,
        "sort SLDPRT native startup classes",
    )?;
    ctx.dedup_vec(&mut native_startups, "deduplicate SLDPRT startup classes")?;
    if let [classes] = native_startups.as_slice() {
        for history in ctx.admit_iter(&mut *histories, "scan SLDPRT idless startup histories")? {
            let mut first = None;
            let mut multiple = false;
            for (index, records) in ctx
                .admit_iter(&history.features, "scan SLDPRT idless startup features")?
                .windows(const { crate::nonzero(6) })
                .enumerate()
            {
                if idless_legacy_startup_shape(ctx, records)? {
                    if first.is_some() {
                        multiple = true;
                        break;
                    }
                    first = Some(index);
                }
            }
            if let Some(index) = first.filter(|_| !multiple) {
                for (feature, class) in history.features[index..index + 6].iter_mut().zip(classes) {
                    feature.input_class = Some(copy_class_text(ctx, class)?);
                }
            }
        }
    }

    let mut classes_by_type = BTreeMap::<String, Vec<String>>::new();
    for history in ctx.admit_iter(&*histories, "scan SLDPRT class-by-type histories")? {
        for feature in ctx.admit_iter(&history.features, "scan SLDPRT class-by-type features")? {
            if let Some(class) = &feature.input_class {
                let kind = temporary.with_storage(|| copy_class_text(ctx, &feature.kind))?;
                let class = temporary.with_storage(|| copy_class_text(ctx, class))?;
                temporary.with_storage(|| {
                    ctx.push_btree_group(
                        &mut classes_by_type,
                        kind,
                        class,
                        "bind SLDPRT history classes",
                        "bind SLDPRT history classes",
                    )
                })?;
            }
        }
    }
    for (_, classes) in ctx.admit_iter(&mut classes_by_type, "sort SLDPRT bound classes")? {
        ctx.stable_sort_by(
            classes,
            |value| value,
            Ord::cmp,
            "sort SLDPRT bound classes",
        )?;
        ctx.dedup_vec(classes, "deduplicate SLDPRT classes by type")?;
    }
    for history in ctx.admit_iter(&mut *histories, "bind SLDPRT class-by-type features")? {
        for feature in
            ctx.admit_iter(&mut history.features, "bind SLDPRT class-by-type features")?
        {
            if feature.input_class.is_some() {
                continue;
            }
            if let Some(classes) = ctx.get_btree_map(
                &classes_by_type,
                &feature.kind,
                "find SLDPRT class by feature type",
            )? {
                let [class] = classes.as_slice() else {
                    continue;
                };
                feature.input_class = Some(copy_class_text(ctx, class)?);
            }
        }
    }

    let token_names = TokenNames::new(ctx, &mut temporary, lanes)?;
    let mut classes_by_token = BTreeMap::<(&str, u16), Vec<String>>::new();
    let mut unique = unique_names.iter();
    for history in ctx.admit_iter(&*histories, "scan SLDPRT token-binding histories")? {
        for feature in ctx.admit_iter(&history.features, "scan SLDPRT token-binding features")? {
            let name_is_unique = unique.next() == Some(&true);
            let Some(class) = &feature.input_class else {
                continue;
            };
            let Some(names) = token_names.of(ctx, feature, name_is_unique)? else {
                continue;
            };
            for &key in ctx.admit_iter(names, "bind SLDPRT token classes")? {
                let class = temporary.with_storage(|| copy_class_text(ctx, class))?;
                temporary.with_storage(|| {
                    ctx.push_btree_group(
                        &mut classes_by_token,
                        key,
                        class,
                        "bind SLDPRT history classes",
                        "bind SLDPRT history classes",
                    )
                })?;
            }
        }
    }
    for (_, classes) in ctx.admit_iter(&mut classes_by_token, "sort SLDPRT bound classes")? {
        ctx.stable_sort_by(
            classes,
            |value| value,
            Ord::cmp,
            "sort SLDPRT bound classes",
        )?;
        ctx.dedup_vec(classes, "deduplicate SLDPRT token classes")?;
    }
    let mut unique = unique_names.iter();
    for history in ctx.admit_iter(&mut *histories, "bind SLDPRT token classes")? {
        for feature in ctx.admit_iter(&mut history.features, "bind SLDPRT token classes")? {
            let name_is_unique = unique.next() == Some(&true);
            if feature.input_class.is_some() {
                continue;
            }
            let Some(names) = token_names.of(ctx, feature, name_is_unique)? else {
                continue;
            };
            let mut candidates = Vec::<&str>::new();
            for key in ctx.admit_iter(names, "scan SLDPRT fallback class names")? {
                if let Some(classes) =
                    ctx.get_btree_map(&classes_by_token, key, "find SLDPRT fallback token class")?
                {
                    if let [class] = classes.as_slice() {
                        temporary.with_storage(|| {
                            ctx.push_vec(
                                &mut candidates,
                                class.as_str(),
                                "bind SLDPRT history classes",
                            )
                        })?;
                    }
                }
            }
            ctx.stable_sort_by(
                &mut candidates,
                |value| value,
                Ord::cmp,
                "sort SLDPRT class candidates",
            )?;
            ctx.dedup_vec(&mut candidates, "deduplicate SLDPRT class candidates")?;
            if let [class] = candidates.as_slice() {
                feature.input_class = Some(copy_class_text(ctx, class)?);
            }
        }
    }

    let legacy_hole_bindings =
        legacy_repeated_hole_wizard_classes(ctx, &mut temporary, histories, lanes)?;
    for history in ctx.admit_iter(&mut *histories, "bind SLDPRT legacy hole classes")? {
        for feature in ctx.admit_iter(&mut history.features, "bind SLDPRT legacy hole classes")? {
            if feature.input_class.is_some() {
                continue;
            }
            if let Some(class) = ctx.get_hash_map(
                &legacy_hole_bindings,
                feature.id.as_str(),
                "find SLDPRT legacy hole class",
            )? {
                feature.input_class = Some(copy_class_text(ctx, class)?);
            }
        }
    }

    for history in ctx.admit_iter(
        &mut *histories,
        "bind SLDPRT classless dimension schema classes",
    )? {
        for feature in ctx.admit_iter(
            &mut history.features,
            "bind SLDPRT classless dimension schema classes",
        )? {
            if feature.input_class.is_some() {
                continue;
            }
            if let Some(class) = classless_dimension_schema_class(ctx, feature)? {
                feature.input_class = Some(copy_class_text(ctx, class)?);
            }
        }
    }
    Ok(())
}

fn legacy_repeated_hole_wizard_classes<'lanes>(
    ctx: &DecodeContext<'_>,
    temporary: &mut ScopedReservation<'_>,
    histories: &[FeatureHistory],
    lanes: &'lanes [FeatureInputLane],
) -> Result<HashMap<String, &'lanes str>, CodecError> {
    let mut by_name = HashMap::<&str, Option<&Feature>>::new();
    let mut hole_shapes = HashSet::<&str>::new();
    for history in ctx.admit_iter(histories, "scan SLDPRT legacy hole histories")? {
        for feature in ctx.admit_iter(&history.features, "index SLDPRT legacy hole features")? {
            if !feature.name.is_empty() {
                temporary
                    .with_storage(|| {
                        ctx.entry_hash_map(
                            &mut by_name,
                            feature.name.as_str(),
                            "index SLDPRT legacy hole names",
                        )
                    })?
                    .and_modify(|candidate| *candidate = None)
                    .or_insert(Some(feature));
            }
        }
        for records in ctx
            .admit_iter(&history.features, "scan SLDPRT legacy hole windows")?
            .windows(const { crate::nonzero(3) })
        {
            let [operation, first_sketch, second_sketch] = records else {
                continue;
            };
            if operation.input_class.is_none()
                && operation.source_id.is_none()
                && first_sketch.ordinal == operation.ordinal + 1
                && second_sketch.ordinal == operation.ordinal + 2
                && [first_sketch, second_sketch].into_iter().all(|sketch| {
                    sketch.input_class.as_deref().is_some_and(|class| {
                        native_object_class(class) == NativeClassKind::ProfileFeature
                    })
                })
                && ctx.eq_ignore_ascii_case(
                    &operation.xml_tag,
                    "Feature",
                    "check SLDPRT legacy hole operation tag",
                )?
            {
                temporary.with_storage(|| {
                    ctx.insert_hash_set(
                        &mut hole_shapes,
                        operation.id.as_str(),
                        "bind SLDPRT history classes",
                    )
                })?;
            }
        }
    }

    let mut bindings = HashMap::new();
    for lane in ctx.admit_iter(lanes, "scan SLDPRT legacy hole lanes")? {
        let mut declared = temporary.with_storage(|| {
            let classes = ctx.admit_iter(&lane.classes, "find SLDPRT hole wizard classes")?;
            ctx.collect_vec(
                classes
                    .filter(|class| native_object_class(&class.name) == NativeClassKind::HoleWizard)
                    .map(|class| class.name.as_str()),
                "collect SLDPRT hole wizard classes",
            )
        })?;
        ctx.sort_unstable_by(
            &mut declared,
            |value| value,
            Ord::cmp,
            "sort SLDPRT hole wizard classes",
        )?;
        ctx.dedup_vec(&mut declared, "deduplicate SLDPRT hole wizard classes")?;
        let [class] = declared.as_slice() else {
            continue;
        };
        let direct_name_offsets = temporary.with_storage(|| {
            ctx.collect_hash_set(
                ctx.admit_iter(&lane.classes, "index SLDPRT legacy direct class names")?
                    .filter_map(direct_name_offset),
                "index SLDPRT legacy direct class names",
            )
        })?;
        let mut groups = BTreeMap::<u16, Vec<&Feature>>::new();
        for name in ctx.admit_iter(&lane.names, "scan SLDPRT legacy hole names")? {
            if ctx.contains_hash_set(
                &direct_name_offsets,
                &name.offset,
                "find SLDPRT legacy direct class name",
            )? {
                continue;
            }
            let Some(feature) = ctx
                .get_hash_map(
                    &by_name,
                    name.value.as_str(),
                    "find SLDPRT legacy hole feature",
                )?
                .copied()
                .flatten()
            else {
                continue;
            };
            let Some(token) = usize::try_from(name.offset)
                .ok()
                .and_then(|offset| repeated_class_token(&lane.native_payload, offset))
                .filter(|token| is_class_token(*token))
            else {
                continue;
            };
            temporary.with_storage(|| {
                ctx.push_btree_group(
                    &mut groups,
                    token,
                    feature,
                    "bind SLDPRT history classes",
                    "bind SLDPRT history classes",
                )
            })?;
        }
        for (_, features) in ctx.admit_iter(&groups, "scan SLDPRT legacy hole groups")? {
            if !ctx.all_by(
                features,
                |feature| {
                    ctx.contains_hash_set(
                        &hole_shapes,
                        feature.id.as_str(),
                        "find SLDPRT legacy hole shape",
                    )
                },
                "check SLDPRT legacy hole group",
            )? {
                continue;
            }
            for feature in ctx.admit_iter(features, "bind SLDPRT legacy hole group")? {
                let id = temporary.with_storage(|| copy_class_text(ctx, &feature.id))?;
                temporary.with_storage(|| {
                    ctx.insert_hash_map(&mut bindings, id, *class, "bind SLDPRT history classes")
                })?;
            }
        }
    }
    Ok(bindings)
}

fn copy_class_text(ctx: &DecodeContext<'_>, text: &str) -> Result<String, CodecError> {
    ctx.copy_retained_text(text, "bind SLDPRT history classes")
}

fn classless_dimension_schema_class(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<Option<&'static str>, CodecError> {
    if feature.parameters.is_empty()
        || feature.parameters.len() > 2
        || feature.content.len() != feature.parameters.len()
        || !ctx.eq_ignore_ascii_case(
            &feature.xml_tag,
            "Feature",
            "check SLDPRT dimension schema tag",
        )?
    {
        return Ok(None);
    }
    for (name, _) in ctx.admit_iter(
        &feature.parameters,
        "scan SLDPRT dimension schema parameters",
    )? {
        let mut occurrences = 0usize;
        for content in ctx.admit_iter(&feature.content, "scan SLDPRT dimension schema content")? {
            if let crate::records::FeatureContent::Dimension(candidate) = content {
                if ctx.equal(
                    candidate.as_str(),
                    name.as_str(),
                    "compare SLDPRT dimension names",
                )? {
                    occurrences += 1;
                }
            }
        }
        if occurrences != 1 {
            return Ok(None);
        }
    }
    if cosmetic_thread_parameter_shape(ctx, feature)? {
        return Ok(Some("moCosmeticThread_c"));
    }
    if feature.parameters.len() == 2 {
        if !ctx.all_by(
            feature.parameters.keys(),
            |name| Ok(name.as_str() == "D1" || name.as_str() == "D2"),
            "scan SLDPRT chamfer parameter names",
        )? {
            return Ok(None);
        }
        let Some(diameter) =
            ctx.get_btree_map(&feature.parameters, "D1", "find SLDPRT chamfer diameter")?
        else {
            return Ok(None);
        };
        ctx.charge_work(
            u64_from_index(diameter.len()),
            "parse SLDPRT chamfer diameter",
        )?;
        if crate::history::literals::parse_positive_dimension_length_mm(diameter).is_none() {
            return Ok(None);
        }
        let Some(angle) =
            ctx.get_btree_map(&feature.parameters, "D2", "find SLDPRT chamfer angle")?
        else {
            return Ok(None);
        };
        let trimmed_angle = ctx.trim_text(angle, "trim SLDPRT dimension angle")?;
        if !(trimmed_angle.ends_with("deg")
            || trimmed_angle.ends_with('°')
            || trimmed_angle.ends_with("rad"))
        {
            return Ok(None);
        }
        ctx.charge_work(u64_from_index(angle.len()), "parse SLDPRT chamfer angle")?;
        if crate::history::literals::parse_angle_rad(angle)
            .is_some_and(|angle| angle.get() > 0.0 && angle.get() < std::f64::consts::PI)
        {
            return Ok(Some("Chamfer_c"));
        }
    }
    Ok(None)
}

fn cosmetic_thread_parameter_shape(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<bool, CodecError> {
    // The two names `D1` and `D2` are the only parameters this shape admits.
    if feature.parameters.len() > 2
        || !ctx.all_by(
            feature.parameters.keys(),
            |name| Ok(name.as_str() == "D1" || name.as_str() == "D2"),
            "scan SLDPRT cosmetic thread parameters",
        )?
        || !ctx.eq_ignore_ascii_case(
            &feature.xml_tag,
            "Feature",
            "check SLDPRT cosmetic thread tag",
        )?
    {
        return Ok(false);
    }
    let Some(expression) = ctx.get_btree_map(
        &feature.parameters,
        "D2",
        "find SLDPRT cosmetic thread diameter",
    )?
    else {
        return Ok(false);
    };
    let expression = ctx.trim_text(expression, "trim SLDPRT cosmetic thread diameter")?;
    Ok(expression.starts_with("<MOD-DIAM>") || expression.starts_with("&lt;MOD-DIAM&gt;"))
}

#[cfg(test)]
mod idless_history_binding_tests {
    use super::super::component_paths::is_profile_feature_object;
    use super::super::component_paths::profile_owns_intervening_sketch_blocks;
    use super::super::component_paths::project_adjacent_extrusion_profiles;
    use super::super::reference_geometry::enrich_history_reference_planes;
    use super::super::terminations::is_extrusion_end_spec_owner;
    use super::{
        bind_history_classes, classless_dimension_schema_class, legacy_repeated_hole_wizard_classes,
    };
    use crate::records::FeatureInputClass;
    use crate::records::FeatureInputLane;
    use crate::records::FeatureSource;
    use crate::records::ObjectId;
    use crate::records::{Feature, FeatureContent, FeatureHistory, FeatureInputName};
    use cadmpeg_core::decode::u64_from_index;
    use cadmpeg_ir::features::BooleanOp;
    use cadmpeg_ir::features::FeatureDefinition;
    use cadmpeg_ir::features::FeatureOperation;
    use cadmpeg_ir::features::LinearTermination;
    use std::collections::BTreeMap;

    fn feature(ordinal: u32, kind: &str) -> Feature {
        Feature {
            id: format!("feature-{ordinal}"),
            parent: "history".into(),
            xml_tag: "Feature".into(),
            tree_parent: None,
            source_id: None,
            ordinal,
            name: format!("name-{ordinal}"),
            kind: kind.into(),
            input_class: None,
            suppressed: false,
            parameters: BTreeMap::new(),
            dimension_properties: BTreeMap::new(),
            properties: BTreeMap::new(),
            text: None,
            content: Vec::new(),
        }
    }

    #[test]
    fn face_plane_record_suppresses_embedded_plane_source_candidate() {
        let mut offset = feature(0, "offset plane");
        offset.source_id = FeatureSource::from_value(10);
        offset.input_class = Some("moRefPlane_c".into());
        offset
            .parameters
            .insert(cadmpeg_core::nonblank_literal!("D1"), "0mm".into());
        let mut principal = feature(1, "principal plane");
        principal.source_id = FeatureSource::from_value(3);
        principal.input_class = Some("moRefPlane_c".into());

        let mut payload = Vec::new();
        payload.extend(3u32.to_le_bytes());
        payload.extend([0x43, 0xf6, 0x8a, 0x4d]);
        payload.extend([0; 2]);
        payload.extend(3u32.to_le_bytes());
        payload.extend(1u32.to_le_bytes());
        payload.extend([0; 4]);
        payload.extend(247u32.to_le_bytes());
        payload.extend([0; 12]);
        payload.extend([0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff]);
        let face_offset = payload.len();
        payload.resize(face_offset + 115, 0);
        payload[face_offset..face_offset + 2].copy_from_slice(&0x802d_u16.to_le_bytes());
        payload[face_offset + 2..face_offset + 6].copy_from_slice(&2u32.to_le_bytes());
        payload[face_offset + 45..face_offset + 61].fill(0xff);
        payload[face_offset + 69..face_offset + 73].copy_from_slice(&2u32.to_le_bytes());
        payload[face_offset + 73..face_offset + 77].copy_from_slice(&0x4c41_ac95_u32.to_le_bytes());
        payload[face_offset + 77..face_offset + 83].copy_from_slice(&[0, 0, 3, 0, 0, 0]);
        payload[face_offset + 83..face_offset + 87].copy_from_slice(&1u32.to_le_bytes());
        payload[face_offset + 91..face_offset + 95].copy_from_slice(&175u32.to_le_bytes());
        payload[face_offset + 99..face_offset + 103].copy_from_slice(&3u32.to_le_bytes());
        payload[face_offset + 107..face_offset + 115]
            .copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff]);
        let end = u64_from_index(payload.len());
        let mut histories = vec![FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![offset, principal],
        }];
        let lane = FeatureInputLane {
            id: "lane".into(),
            configuration: None,
            native_payload: payload,
            classes: Vec::new(),
            names: vec![
                FeatureInputName {
                    id: "offset-name".into(),
                    parent: "lane".into(),
                    ordinal: 0,
                    offset: 0,
                    object_id: ObjectId::from_value(10),
                    value: "name-0".into(),
                },
                FeatureInputName {
                    id: "principal-name".into(),
                    parent: "lane".into(),
                    ordinal: 1,
                    offset: end,
                    object_id: ObjectId::from_value(3),
                    value: "name-1".into(),
                },
            ],
            scalars: Vec::new(),
            relation_bindings: Vec::new(),
            relation_instances: Vec::new(),
            body_selections: Vec::new(),
            edge_selections: Vec::new(),
            surface_selections: Vec::new(),
            generated_surface_identities: Vec::new(),
            references: Vec::new(),
            sketch_entities: Vec::new(),
        };

        enrich_history_reference_planes(
            &cadmpeg_test_support::service_decode_context(),
            &mut histories,
            &[lane],
        )
        .unwrap();

        let properties = &histories[0].features[0].properties;
        assert!(!properties.contains_key("Reference"));
        assert!(properties.contains_key("ReferenceFaceNative"));
    }

    #[test]
    fn classless_sketch_objects_are_profile_features_only_with_source_identity() {
        let mut sketch = feature(1, "localized sketch");
        sketch.xml_tag = "Sketch".into();
        sketch.source_id = FeatureSource::from_value(77);
        assert!(is_profile_feature_object(&sketch));

        sketch.source_id = FeatureSource::from_value(0);
        assert!(!is_profile_feature_object(&sketch));
        sketch.source_id = FeatureSource::from_value(77);
        sketch.input_class = Some("moRefPlane_c".into());
        assert!(!is_profile_feature_object(&sketch));
    }

    #[test]
    fn history_metadata_does_not_interrupt_an_extrusion_profile_pair() {
        let mut profile = feature(1, "sketch");
        profile.id = "profile-native".into();
        profile.xml_tag = "Sketch".into();
        profile.input_class = Some("moProfileFeature_c".into());
        profile.source_id = FeatureSource::from_value(41);
        let mut metadata = feature(2, "attribute");
        metadata.id = "metadata-native".into();
        metadata.source_id = FeatureSource::from_value(42);
        metadata.input_class = Some("moAttribute_c".into());
        let mut extrusion = feature(2, "extrusion");
        extrusion.id = "extrusion-native".into();
        extrusion.xml_tag = "Extrusion".into();
        extrusion.source_id = FeatureSource::from_value(43);
        extrusion.properties.insert(
            cadmpeg_core::nonblank_literal!("Dissectable"),
            "true".into(),
        );
        let history = FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![profile, metadata, extrusion],
        };
        let lane = FeatureInputLane {
            id: "lane".into(),
            configuration: None,
            native_payload: Vec::new(),
            classes: Vec::new(),
            names: vec![
                FeatureInputName {
                    id: "profile-name".into(),
                    parent: "lane".into(),
                    ordinal: 0,
                    offset: 100,
                    object_id: ObjectId::from_value(41),
                    value: "name-1".into(),
                },
                FeatureInputName {
                    id: "metadata-name".into(),
                    parent: "lane".into(),
                    ordinal: 1,
                    offset: 150,
                    object_id: ObjectId::from_value(42),
                    value: "name-2".into(),
                },
                FeatureInputName {
                    id: "extrusion-name".into(),
                    parent: "lane".into(),
                    ordinal: 2,
                    offset: 200,
                    object_id: ObjectId::from_value(43),
                    value: "name-2".into(),
                },
            ],
            scalars: Vec::new(),
            relation_bindings: Vec::new(),
            relation_instances: Vec::new(),
            body_selections: Vec::new(),
            edge_selections: Vec::new(),
            surface_selections: Vec::new(),
            generated_surface_identities: Vec::new(),
            references: Vec::new(),
            sketch_entities: Vec::new(),
        };
        let profile_id = cadmpeg_ir::features::FeatureId::mint("synthetic:test:id#profile")
            .expect("identity grammar");
        let mut features = vec![
            cadmpeg_ir::features::Feature {
                id: profile_id.clone(),
                ordinal: 0,
                name: None,
                suppressed: Some(false),
                dependencies: cadmpeg_ir::features::DistinctMembers::default(),
                source_properties: BTreeMap::new(),
                source_tag: None,
                source_text: None,
                source_content: cadmpeg_ir::features::FeatureContent::default(),

                evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                    FeatureDefinition::Operation(FeatureOperation::Sketch {
                        sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(None),
                    }),
                ),
                native_ref: Some("profile-native".into()),
            },
            cadmpeg_ir::features::Feature {
                id: cadmpeg_ir::features::FeatureId::mint("synthetic:test:id#extrusion")
                    .expect("identity grammar"),
                ordinal: 1,
                name: None,
                suppressed: Some(false),
                dependencies: cadmpeg_ir::features::DistinctMembers::default(),
                source_properties: BTreeMap::new(),
                source_tag: None,
                source_text: None,
                source_content: cadmpeg_ir::features::FeatureContent::default(),

                evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                    FeatureDefinition::Operation(FeatureOperation::Extrude {
                        profile: cadmpeg_ir::features::ProfileRef::Planar(
                            cadmpeg_ir::features::PlanarProfileRef::Unresolved(
                                "extrusion-native".into(),
                            ),
                        ),
                        direction: cadmpeg_ir::features::ExtrudeDirection::ProfileNormal {},
                        start: cadmpeg_ir::features::ExtrudeStart::ProfilePlane {},
                        extent: cadmpeg_ir::features::ExtrudeExtent::OneSided {
                            side: cadmpeg_ir::features::ExtrudeSide {
                                termination: LinearTermination::Blind {
                                    length: cadmpeg_ir::scalar::NonZeroLength::new(1.0).unwrap(),
                                },
                                draft: None,
                            },
                        },
                        op: BooleanOp::Join,
                        solid: Some(true),
                        face_maker: None,
                        inner_wire_taper: None,
                        length_along_profile_normal: None,
                        allow_multi_profile_faces: None,
                    }),
                ),
                native_ref: Some("extrusion-native".into()),
            },
        ];

        project_adjacent_extrusion_profiles(
            &cadmpeg_test_support::service_decode_context(),
            &mut features,
            std::slice::from_ref(&history),
            std::slice::from_ref(&lane),
        )
        .unwrap();

        assert!(matches!(
            features[1].evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                profile: cadmpeg_ir::features::ProfileRef::Planar(cadmpeg_ir::features::PlanarProfileRef::Feature(actual)),
                ..
            }) if actual == &profile_id
        ));
        assert_eq!(features[1].dependencies.as_slice(), [profile_id]);
    }

    #[test]
    fn exact_dimension_schemas_bind_classless_thread_and_chamfer_features() {
        let ctx = cadmpeg_test_support::service_decode_context();
        let mut thread = feature(1, "localized thread");
        thread
            .parameters
            .insert(cadmpeg_core::nonblank_literal!("D2"), "<MOD-DIAM>8".into());
        thread.content.push(FeatureContent::Dimension("D2".into()));
        assert_eq!(
            classless_dimension_schema_class(&ctx, &thread).unwrap(),
            Some("moCosmeticThread_c")
        );

        let mut chamfer = feature(2, "localized chamfer");
        chamfer
            .parameters
            .insert(cadmpeg_core::nonblank_literal!("D1"), "0.57".into());
        chamfer
            .parameters
            .insert(cadmpeg_core::nonblank_literal!("D2"), "45°".into());
        chamfer.content.extend([
            FeatureContent::Dimension("D1".into()),
            FeatureContent::Dimension("D2".into()),
        ]);
        assert_eq!(
            classless_dimension_schema_class(&ctx, &chamfer).unwrap(),
            Some("Chamfer_c")
        );

        chamfer
            .parameters
            .insert(cadmpeg_core::nonblank_literal!("D2"), "1.25".into());
        assert_eq!(
            classless_dimension_schema_class(&ctx, &chamfer).unwrap(),
            None
        );
        chamfer.content.pop();
        assert_eq!(
            classless_dimension_schema_class(&ctx, &chamfer).unwrap(),
            None
        );
    }

    #[test]
    fn native_extrusion_class_establishes_an_end_spec_owner() {
        let mut cut = feature(1, "localized cut");
        cut.xml_tag = "Feature".into();
        cut.input_class = Some("moCut_c".into());
        assert!(is_extrusion_end_spec_owner(&cut));

        cut.input_class = Some("moRefPlane_c".into());
        assert!(!is_extrusion_end_spec_owner(&cut));
        cut.xml_tag = "Cut".into();
        assert!(is_extrusion_end_spec_owner(&cut));
    }

    #[test]
    fn repeated_legacy_holes_own_two_consecutive_profile_children() {
        let mut first = feature(10, "localized hole A");
        first.name = "hole A".into();
        let mut first_position = feature(11, "sketch");
        first_position.input_class = Some("moProfileFeature_c".into());
        let mut first_profile = feature(12, "sketch");
        first_profile.input_class = Some("moProfileFeature_c".into());
        let mut second = feature(20, "localized hole B");
        second.name = "hole B".into();
        let mut second_position = feature(21, "sketch");
        second_position.input_class = Some("moProfileFeature_c".into());
        let mut second_profile = feature(22, "sketch");
        second_profile.input_class = Some("moProfileFeature_c".into());
        let history = FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![
                first,
                first_position,
                first_profile,
                second,
                second_position,
                second_profile,
            ],
        };
        let mut payload = vec![0; 240];
        payload[98..100].copy_from_slice(&0x82a4u16.to_le_bytes());
        payload[198..200].copy_from_slice(&0x82a4u16.to_le_bytes());
        let lane = FeatureInputLane {
            id: "lane".into(),
            configuration: None,
            native_payload: payload,
            classes: vec![FeatureInputClass {
                id: "class".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: 0,
                name: "moHoleWzd_c".into(),
            }],
            names: vec![
                FeatureInputName {
                    id: "first".into(),
                    parent: "lane".into(),
                    ordinal: 0,
                    offset: 100,
                    object_id: ObjectId::from_value(41),
                    value: "hole A".into(),
                },
                FeatureInputName {
                    id: "second".into(),
                    parent: "lane".into(),
                    ordinal: 1,
                    offset: 200,
                    object_id: ObjectId::from_value(42),
                    value: "hole B".into(),
                },
            ],
            scalars: Vec::new(),
            relation_bindings: Vec::new(),
            relation_instances: Vec::new(),
            body_selections: Vec::new(),
            edge_selections: Vec::new(),
            surface_selections: Vec::new(),
            generated_surface_identities: Vec::new(),
            references: Vec::new(),
            sketch_entities: Vec::new(),
        };

        let ctx = cadmpeg_test_support::service_decode_context();
        let mut temporary = ctx.reserve_scoped(0, "test workspace").unwrap();
        let lanes = [lane];
        let bindings =
            legacy_repeated_hole_wizard_classes(&ctx, &mut temporary, &[history], &lanes).unwrap();
        assert_eq!(bindings.get("feature-10").copied(), Some("moHoleWzd_c"));
        assert_eq!(bindings.get("feature-20").copied(), Some("moHoleWzd_c"));
    }

    #[test]
    fn dissectable_profile_owns_its_block_object_sequence() {
        let mut profile = feature(0, "sketch");
        profile.input_class = Some("moProfileFeature_c".into());
        profile.properties.insert(
            cadmpeg_core::nonblank_literal!("DissectableChildren"),
            "23,27".into(),
        );
        let mut definition_a = feature(1, "block");
        definition_a.input_class = Some("moSketchBlockDef_c".into());
        definition_a.source_id = FeatureSource::from_value(23);
        let mut instance = feature(2, "block instance");
        instance.input_class = Some("moSketchBlockInst_c".into());
        instance.source_id = FeatureSource::from_value(25);
        let mut definition_b = feature(3, "block");
        definition_b.input_class = Some("moSketchBlockDef_c".into());
        definition_b.source_id = FeatureSource::from_value(27);
        let objects = [&definition_a, &instance, &definition_b];
        assert!(profile_owns_intervening_sketch_blocks(
            &cadmpeg_test_support::service_decode_context(),
            &profile,
            objects.iter().copied()
        )
        .unwrap());

        definition_b.input_class = Some("moRefPlane_c".into());
        let objects = [&definition_a, &instance, &definition_b];
        assert!(!profile_owns_intervening_sketch_blocks(
            &cadmpeg_test_support::service_decode_context(),
            &profile,
            objects.iter().copied()
        )
        .unwrap());
        definition_b.input_class = Some("moSketchBlockDef_c".into());
        let objects = [&definition_a, &instance, &definition_b];
        profile.properties.insert(
            cadmpeg_core::nonblank_literal!("DissectableChildren"),
            "23,23".into(),
        );
        assert!(!profile_owns_intervening_sketch_blocks(
            &cadmpeg_test_support::service_decode_context(),
            &profile,
            objects.iter().copied()
        )
        .unwrap());
    }

    #[test]
    fn closed_block_graph_binds_a_profile_without_an_explicit_child_list() {
        let profile = feature(0, "sketch");
        let mut instance = feature(1, "block instance");
        instance.input_class = Some("moSketchBlockInst_c".into());
        instance.source_id = FeatureSource::from_value(25);
        instance.properties.insert(
            cadmpeg_core::nonblank_literal!("BlockDefinition"),
            "23".into(),
        );
        let mut definition = feature(2, "block");
        definition.input_class = Some("moSketchBlockDef_c".into());
        definition.source_id = FeatureSource::from_value(23);

        assert!(profile_owns_intervening_sketch_blocks(
            &cadmpeg_test_support::service_decode_context(),
            &profile,
            [&instance, &definition]
        )
        .unwrap());

        instance.properties.insert(
            cadmpeg_core::nonblank_literal!("BlockDefinition"),
            "24".into(),
        );
        assert!(!profile_owns_intervening_sketch_blocks(
            &cadmpeg_test_support::service_decode_context(),
            &profile,
            [&instance, &definition]
        )
        .unwrap());
    }

    #[test]
    fn incomplete_block_graph_does_not_bind_a_profile() {
        let profile = feature(0, "sketch");
        let mut instance = feature(1, "block instance");
        instance.input_class = Some("moSketchBlockInst_c".into());
        instance.source_id = FeatureSource::from_value(25);
        instance.properties.insert(
            cadmpeg_core::nonblank_literal!("BlockDefinition"),
            "23".into(),
        );
        let mut referenced = feature(2, "block");
        referenced.input_class = Some("moSketchBlockDef_c".into());
        referenced.source_id = FeatureSource::from_value(23);
        let mut unused = feature(3, "block");
        unused.input_class = Some("moSketchBlockDef_c".into());
        unused.source_id = FeatureSource::from_value(24);

        assert!(!profile_owns_intervening_sketch_blocks(
            &cadmpeg_test_support::service_decode_context(),
            &profile,
            [&instance, &referenced, &unused]
        )
        .unwrap());
        assert!(!profile_owns_intervening_sketch_blocks(
            &cadmpeg_test_support::service_decode_context(),
            &profile,
            [&referenced]
        )
        .unwrap());

        let mut second_instance = feature(4, "block instance");
        second_instance.input_class = Some("moSketchBlockInst_c".into());
        second_instance.source_id = FeatureSource::from_value(26);
        second_instance.properties.insert(
            cadmpeg_core::nonblank_literal!("BlockDefinition"),
            "23".into(),
        );
        assert!(profile_owns_intervening_sketch_blocks(
            &cadmpeg_test_support::service_decode_context(),
            &profile,
            [&instance, &second_instance, &referenced]
        )
        .unwrap());
        second_instance.properties.insert(
            cadmpeg_core::nonblank_literal!("BlockDefinition"),
            "24".into(),
        );
        assert!(!profile_owns_intervening_sketch_blocks(
            &cadmpeg_test_support::service_decode_context(),
            &profile,
            [&instance, &second_instance, &referenced]
        )
        .unwrap());
    }

    #[test]
    fn exact_idless_startup_binds_from_the_native_class_roster() {
        let mut features = vec![
            feature(10, "localized plane"),
            feature(11, "localized plane"),
            feature(12, "localized plane"),
            feature(13, "localized origin"),
            feature(14, "localized sketch"),
            feature(15, "localized extrusion"),
        ];
        features[4]
            .parameters
            .insert(cadmpeg_core::nonblank_literal!("D1"), "88".into());
        features[5]
            .parameters
            .insert(cadmpeg_core::nonblank_literal!("D1"), "20".into());
        let mut histories = [FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features,
        }];
        let classes = [
            "moRefPlane_c",
            "moOriginProfileFeature_c",
            "moProfileFeature_c",
            "moExtrusion_c",
        ]
        .into_iter()
        .enumerate()
        .map(|(ordinal, name)| FeatureInputClass {
            id: format!("class-{ordinal}"),
            parent: "lane".into(),
            ordinal: u32::try_from(ordinal).expect("test class ordinal fits u32"),
            offset: u64_from_index(ordinal) * 100,
            name: name.into(),
        })
        .collect();
        let lane = FeatureInputLane {
            id: "lane".into(),
            configuration: None,
            native_payload: Vec::new(),
            classes,
            names: Vec::new(),
            scalars: Vec::new(),
            relation_bindings: Vec::new(),
            relation_instances: Vec::new(),
            body_selections: Vec::new(),
            edge_selections: Vec::new(),
            surface_selections: Vec::new(),
            generated_surface_identities: Vec::new(),
            references: Vec::new(),
            sketch_entities: Vec::new(),
        };

        bind_history_classes(
            &cadmpeg_test_support::service_decode_context(),
            &mut histories,
            &[lane],
        )
        .unwrap();

        assert_eq!(
            histories[0]
                .features
                .iter()
                .map(|feature| feature.input_class.as_deref())
                .collect::<Vec<_>>(),
            vec![
                Some("moRefPlane_c"),
                Some("moRefPlane_c"),
                Some("moRefPlane_c"),
                Some("moOriginProfileFeature_c"),
                Some("moProfileFeature_c"),
                Some("moExtrusion_c"),
            ]
        );
    }

    #[test]
    fn unique_idless_name_binds_to_its_direct_class_declaration() {
        let mut unique = feature(0, "localized thread");
        unique.name = "unique generated name".into();
        let mut duplicate_a = feature(1, "localized duplicate");
        duplicate_a.name = "duplicate name".into();
        let mut duplicate_b = feature(2, "localized duplicate");
        duplicate_b.name = duplicate_a.name.clone();
        let mut histories = [FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![unique, duplicate_a, duplicate_b],
        }];
        let class = |ordinal: u32, offset: u64, name: &str| FeatureInputClass {
            id: format!("class-{ordinal}"),
            parent: "lane".into(),
            ordinal,
            offset,
            name: name.into(),
        };
        let classes = vec![
            class(0, 100, "moCosmeticThread_c"),
            class(1, 200, "moRefAxis_c"),
        ];
        let name = |ordinal: u32, class: &FeatureInputClass, value: &str| FeatureInputName {
            id: format!("name-{ordinal}"),
            parent: "lane".into(),
            ordinal,
            offset: class.offset + 6 + u64_from_index(class.name.len()),
            object_id: None,
            value: value.into(),
        };
        let names = vec![
            name(0, &classes[0], "unique generated name"),
            name(1, &classes[1], "duplicate name"),
        ];
        let lane = FeatureInputLane {
            id: "lane".into(),
            configuration: None,
            native_payload: Vec::new(),
            classes,
            names,
            scalars: Vec::new(),
            relation_bindings: Vec::new(),
            relation_instances: Vec::new(),
            body_selections: Vec::new(),
            edge_selections: Vec::new(),
            surface_selections: Vec::new(),
            generated_surface_identities: Vec::new(),
            references: Vec::new(),
            sketch_entities: Vec::new(),
        };

        bind_history_classes(
            &cadmpeg_test_support::service_decode_context(),
            &mut histories,
            &[lane],
        )
        .unwrap();

        assert_eq!(
            histories[0].features[0].input_class.as_deref(),
            Some("moCosmeticThread_c")
        );
        assert_eq!(histories[0].features[1].input_class, None);
        assert_eq!(histories[0].features[2].input_class, None);
    }

    #[test]
    fn unique_idless_name_inherits_a_proven_repeated_class_token() {
        let mut direct = feature(0, "localized hole");
        direct.name = "direct hole".into();
        let mut repeated = feature(1, "localized hole");
        repeated.name = "repeated hole".into();
        let mut target = feature(2, "another localized hole");
        target.name = "target hole".into();
        let mut histories = [FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![direct, repeated, target],
        }];
        let class = FeatureInputClass {
            id: "class".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 100,
            name: "moHoleWzd_c".into(),
        };
        let direct_offset = class.offset + 6 + u64_from_index(class.name.len());
        let names = vec![
            FeatureInputName {
                id: "direct-name".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: direct_offset,
                object_id: ObjectId::from_value(1),
                value: "direct hole".into(),
            },
            FeatureInputName {
                id: "repeated-name".into(),
                parent: "lane".into(),
                ordinal: 1,
                offset: 400,
                object_id: ObjectId::from_value(2),
                value: "repeated hole".into(),
            },
            FeatureInputName {
                id: "target-name".into(),
                parent: "lane".into(),
                ordinal: 2,
                offset: 400,
                object_id: ObjectId::from_value(3),
                value: "target hole".into(),
            },
        ];
        let mut payload = vec![0; 500];
        payload[298..300].copy_from_slice(&0x82a4_u16.to_le_bytes());
        payload[398..400].copy_from_slice(&0x82a4_u16.to_le_bytes());
        let lane = FeatureInputLane {
            id: "lane".into(),
            configuration: None,
            native_payload: payload,
            classes: vec![class],
            names,
            scalars: Vec::new(),
            relation_bindings: Vec::new(),
            relation_instances: Vec::new(),
            body_selections: Vec::new(),
            edge_selections: Vec::new(),
            surface_selections: Vec::new(),
            generated_surface_identities: Vec::new(),
            references: Vec::new(),
            sketch_entities: Vec::new(),
        };

        bind_history_classes(
            &cadmpeg_test_support::service_decode_context(),
            &mut histories,
            &[lane],
        )
        .unwrap();

        assert!(histories[0]
            .features
            .iter()
            .all(|feature| feature.input_class.as_deref() == Some("moHoleWzd_c")));
    }

    #[test]
    fn diameter_parameter_schema_binds_a_repeated_cosmetic_thread_group() {
        let mut first = feature(0, "localized external thread");
        first.source_id = FeatureSource::from_value(11);
        first
            .parameters
            .insert(cadmpeg_core::nonblank_literal!("D1"), "12".into());
        first
            .parameters
            .insert(cadmpeg_core::nonblank_literal!("D2"), "<MOD-DIAM>8".into());
        let mut second = feature(1, "localized hole thread");
        second.source_id = FeatureSource::from_value(12);
        second.parameters.insert(
            cadmpeg_core::nonblank_literal!("D2"),
            "&lt;MOD-DIAM&gt;6".into(),
        );
        let mut histories = [FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![first, second],
        }];
        let class = FeatureInputClass {
            id: "class".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 100,
            name: "moCosmeticThread_c".into(),
        };
        let names = histories[0]
            .features
            .iter()
            .enumerate()
            .map(|(index, feature)| FeatureInputName {
                id: format!("name-{index}"),
                parent: "lane".into(),
                ordinal: u32::try_from(index).expect("test name ordinal fits u32"),
                offset: 300 + u64_from_index(index) * 100,
                object_id: feature.source_value().and_then(ObjectId::from_value),
                value: feature.name.clone(),
            })
            .collect::<Vec<_>>();
        let mut payload = vec![0; 500];
        for name in &names {
            let offset = usize::try_from(name.offset).expect("required invariant");
            payload[offset - 2..offset].copy_from_slice(&0x82a4_u16.to_le_bytes());
        }
        let lane = FeatureInputLane {
            id: "lane".into(),
            configuration: None,
            native_payload: payload,
            classes: vec![class],
            names,
            scalars: Vec::new(),
            relation_bindings: Vec::new(),
            relation_instances: Vec::new(),
            body_selections: Vec::new(),
            edge_selections: Vec::new(),
            surface_selections: Vec::new(),
            generated_surface_identities: Vec::new(),
            references: Vec::new(),
            sketch_entities: Vec::new(),
        };

        bind_history_classes(
            &cadmpeg_test_support::service_decode_context(),
            &mut histories,
            &[lane],
        )
        .unwrap();

        assert!(histories[0]
            .features
            .iter()
            .all(|feature| { feature.input_class.as_deref() == Some("moCosmeticThread_c") }));
    }
}
