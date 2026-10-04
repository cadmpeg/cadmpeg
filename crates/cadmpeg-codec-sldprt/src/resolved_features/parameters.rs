//! Design parameter enrichment and scalar synchronisation.

use super::scalars::feature_object_name;
use super::{NAME_MARKER, VALUE_ONLY_SCALAR_HEADER};
use crate::records::{
    FeatureInputLane, FeatureInputName, FeatureInputRelationFamily, FeatureInputScalar,
    FeatureInputScalarRole,
};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScalarUnit {
    Native,
    Length,
    Angle,
}

fn scalar_owned_by_feature(
    ctx: &DecodeContext<'_>,
    scalar: &FeatureInputScalar,
    feature: &str,
    start: u64,
    end: Option<u64>,
) -> Result<bool, CodecError> {
    if let Some(owner) = scalar.feature_ref.as_deref() {
        return ctx.equal(owner, feature, "compare SLDPRT scalar owner");
    }
    Ok(scalar.offset > start && end.is_none_or(|end| scalar.offset < end))
}

/// Add unambiguous `ResolvedFeatures` length parameters to a projection copy of history.
pub(crate) fn enrich_history_parameters<'a>(
    ctx: &DecodeContext<'_>,
    histories: &mut [crate::records::FeatureHistory],
    lanes: impl IntoIterator<Item = &'a FeatureInputLane>,
    replace_existing: bool,
) -> Result<(), CodecError> {
let mut temporary_storage = ctx.reserve_scoped(0, "SLDPRT parameters temporary storage")?;

    let mut candidates = BTreeMap::<(usize, usize, String), Vec<(f64, ScalarUnit)>>::new();
    for lane in lanes {
        let feature_count = ctx
            .admit_iter(&histories[..], "count SLDPRT parameter features")?
            .try_fold(0usize, |count, history| {
            count.checked_add(history.features.len()).ok_or_else(|| {
                ctx.refuse_codec_limit("scan SLDPRT parameter candidates", u64::MAX - 1, u64::MAX)
            })
        })?;
        let work = feature_count
            .checked_mul(lane.names.len())
            .ok_or_else(|| {
                ctx.refuse_codec_limit("scan SLDPRT parameter candidates", u64::MAX - 1, u64::MAX)
            })?;
        ctx.charge_work(
            u64::try_from(work).map_err(|_| {
                ctx.refuse_codec_limit("scan SLDPRT parameter candidates", u64::MAX - 1, u64::MAX)
            })?,
            "scan SLDPRT parameter candidates",
        )?;
        let mut names_by_id = HashMap::new();
        for name in ctx.admit_iter(&lane.names, "index SLDPRT parameter names")? {
            temporary_storage.with_storage(|| ctx.insert_hash_map(
                &mut names_by_id,
                name.id.as_str(),
                name,
                "index SLDPRT parameter names",
            ))?;
        }
        let relation_unit = |family| match family {
            FeatureInputRelationFamily::Angle => ScalarUnit::Angle,
            FeatureInputRelationFamily::LineLineDistance
            | FeatureInputRelationFamily::PointPointDistance
            | FeatureInputRelationFamily::PointLineDistance
            | FeatureInputRelationFamily::PointPointHorizontalDistance
            | FeatureInputRelationFamily::PointPointVerticalDistance
            | FeatureInputRelationFamily::CircleDiameter => ScalarUnit::Length,
        };
        let mut scalar_units = HashMap::new();
        for scalar in ctx.admit_iter(&lane.scalars, "infer SLDPRT scalar units")? {
            let Some(name) = ctx.get_hash_map(
                &names_by_id,
                scalar.name.as_str(),
                "lookup SLDPRT scalar name",
            )? else {
                continue;
            };
            let parameter_class = ctx
                .max_by_key(
                    &lane.classes,
                    |class| Ok((class.offset < name.offset, class.offset)),
                    |left, right| match ctx.compare(
                        &left.0,
                        &right.0,
                        "compare SLDPRT parameter class eligibility",
                    )? {
                        std::cmp::Ordering::Equal => ctx.compare(
                            &left.1,
                            &right.1,
                            "compare SLDPRT parameter class offsets",
                        ),
                        order => Ok(order),
                    },
                    "find SLDPRT scalar parameter class",
                )?
                .filter(|class| class.offset < name.offset);
            let Some(parameter_class) = parameter_class
            else {
                continue;
            };
            let has_intervening_name = ctx
                .admit_iter(&lane.names, "find intervening SLDPRT parameter name")?
                .any(|intervening| {
                    intervening.offset > parameter_class.offset
                        && intervening.offset < name.offset
                });
            if has_intervening_name {
                continue;
            }
            let unit = match parameter_class.name.as_str() {
                "moLengthParameter_c" => ScalarUnit::Length,
                "moAngleParameter_c" => ScalarUnit::Angle,
                _ => continue,
            };
            temporary_storage.with_storage(|| ctx.insert_hash_map(
                &mut scalar_units,
                scalar.id.as_str(),
                unit,
                "index SLDPRT scalar units",
            ))?;
        }
        for binding in ctx.admit_iter(
            &lane.relation_bindings,
            "index SLDPRT relation scalar units",
        )? {
            temporary_storage.with_storage(|| ctx.insert_hash_map(
                &mut scalar_units,
                binding.scalar_ref.as_str(),
                relation_unit(binding.family),
                "index SLDPRT scalar units",
            ))?;
        }
        for relation in ctx.admit_iter(
            &lane.relation_instances,
            "index SLDPRT relation scalar units",
        )? {
            let unit = relation_unit(relation.family);
            for scalar in ctx.admit_iter(
                relation.scalar_refs(),
                "index SLDPRT relation scalar references",
            )? {
                temporary_storage.with_storage(|| ctx.insert_hash_map(
                    &mut scalar_units,
                    scalar.as_str(),
                    unit,
                    "index SLDPRT scalar units",
                ))?;
            }
        }
        let mut starts = Vec::<(u64, usize, usize)>::new();
        for (history_index, history) in
            ctx.admit_iter(&histories[..], "collect SLDPRT parameter feature starts")?
                .enumerate()
        {
            for (feature_index, feature) in ctx
                .admit_iter(&history.features, "collect SLDPRT parameter feature starts")?
                .enumerate()
            {
                let Some(name) = feature_object_name(feature, lane) else {
                    continue;
                };
                ctx.reserve_vec(&mut starts, 1, "collect SLDPRT parameter feature starts")?;
                starts.push((name.offset, history_index, feature_index));
            }
        }
        ctx.stable_sort_by(
            &mut starts,
            |value| &value.0,
            Ord::cmp,
            "sort SLDPRT feature starts",
        )?;
        for (index, &(start, history_index, feature_index)) in ctx
            .admit_iter(&starts, "scan SLDPRT parameter feature starts")?
            .enumerate()
        {
            let end = starts.get(index + 1).map(|next| next.0);
            let feature = &histories[history_index].features[feature_index];
            let mut owned = BTreeMap::<&str, Vec<&FeatureInputScalar>>::new();
            for scalar in ctx.admit_iter(&lane.scalars, "collect SLDPRT owned scalars")? {
                if !scalar_owned_by_feature(ctx, scalar, &feature.id, start, end)? {
                    continue;
                }
                let Some(name) = ctx.get_hash_map(
                    &names_by_id,
                    scalar.name.as_str(),
                    "lookup SLDPRT scalar name",
                )? else {
                    continue;
                };
                ctx.push_btree_group(
                    &mut owned,
                    name.value.as_str(),
                    scalar,
                    "index SLDPRT owned scalar names",
                    "collect SLDPRT owned scalars",
                )?;
            }
            for (name, scalars) in ctx.admit_iter(&owned, "scan SLDPRT owned scalar names")? {
                let mut driving = Vec::new();
                for scalar in ctx
                    .admit_iter(scalars, "collect SLDPRT driving scalars")?
                    .filter(|scalar| scalar.role == FeatureInputScalarRole::Driving)
                {
                    ctx.reserve_vec(&mut driving, 1, "collect SLDPRT driving scalars")?;
                    driving.push(*scalar);
                }
                let candidates_for_name = if driving.is_empty() {
                    let mut native = Vec::new();
                    for scalar in ctx
                        .admit_iter(scalars, "collect SLDPRT native scalars")?
                        .filter(|scalar| scalar.role == FeatureInputScalarRole::Native)
                    {
                        ctx.reserve_vec(&mut native, 1, "collect SLDPRT native scalars")?;
                        native.push(*scalar);
                    }
                    native
                } else {
                    driving
                };
                if let [scalar] = candidates_for_name.as_slice() {
                    let value_only = match ctx.get_hash_map(
                        &names_by_id,
                        scalar.name.as_str(),
                        "lookup SLDPRT scalar name",
                    )? {
                        Some(name) => {
                            value_only_scalar_offset(ctx, &lane.native_payload, name)?
                                == usize::try_from(scalar.offset).ok()
                        }
                        None => false,
                    };
                    let unit = match ctx.get_hash_map(
                        &scalar_units,
                        scalar.id.as_str(),
                        "lookup SLDPRT scalar unit",
                    )? {
                        Some(unit) => *unit,
                        None => scalar_unit_from_feature_parameter(ctx, feature, name)?
                            .unwrap_or(ScalarUnit::Native),
                    };
                    if value_only {
                        continue;
                    }
                    let name = ctx.format_retained(
                        format_args!("{name}"),
                        "retain SLDPRT parameter candidate name",
                    )?;
                    let key = (history_index, feature_index, name);
                    ctx.push_btree_group(
                        &mut candidates,
                        key,
                        (scalar.value.get(), unit),
                        "index SLDPRT parameter candidates",
                        "collect SLDPRT parameter candidates",
                    )?;
                }
            }
        }
    }

    for ((history_index, feature_index, name), values) in
        ctx.admit_iter(&candidates, "apply SLDPRT parameter candidates")?
    {
        let Some((&(first, unit), rest)) = values.split_first() else {
            continue;
        };
        if rest.iter().any(|(value, candidate_unit)| {
            value.to_bits() != first.to_bits() || *candidate_unit != unit
        }) {
            continue;
        }
        let feature = &mut histories[*history_index].features[*feature_index];
        let mut source_dimension = false;
        for content in ctx.admit_iter(&feature.content, "check SLDPRT source dimensions")? {
            if let crate::records::FeatureContent::Dimension(dimension) = content {
                if ctx.equal(dimension.as_str(), name, "compare SLDPRT source dimension")? {
                    source_dimension = true;
                    break;
                }
            }
        }
        if unit == ScalarUnit::Native
            && source_dimension
            && ctx.contains_key_btree_map(
                &feature.parameters,
                name.as_str(),
                "check existing SLDPRT parameter",
            )?
        {
            continue;
        }
        if unit == ScalarUnit::Native {
            if let Some(expression) = ctx.get_btree_map(
                &feature.parameters,
                name.as_str(),
                "lookup existing SLDPRT parameter",
            )? {
                if !native_scalar_matches_discrete_parameter(feature, name, expression, first) {
                    continue;
                }
            }
        }
        let expression = match unit {
            ScalarUnit::Native => {
                let previous = ctx
                    .get_btree_map(
                        &feature.parameters,
                        name.as_str(),
                        "lookup existing SLDPRT parameter",
                    )?
                    .map(String::as_str);
                crate::history::parameters::format_native_scalar(
                    feature,
                    name,
                    first,
                    previous,
                )
            }
            ScalarUnit::Length => {
                let previous = ctx
                    .get_btree_map(
                        &feature.parameters,
                        name.as_str(),
                        "lookup existing SLDPRT parameter",
                    )?
                    .map(String::as_str);
                if previous.is_some_and(|expression| {
                    crate::history::literals::strip_diameter_modifier(expression).is_some()
                }) {
                    crate::history::parameters::format_native_scalar(
                        feature, name, first, previous,
                    )
                } else {
                    cadmpeg_ir::scalar::Length::new(first * 1000.0)
                        .map(crate::history::literals::format_length_mm)
                }
            }
            ScalarUnit::Angle => cadmpeg_ir::scalar::Angle::new(first)
                .map(crate::history::literals::format_angle_rad),
        };
        let Some(expression) = expression else {
            continue;
        };
        let Some(name) = cadmpeg_core::text::NonBlankString::for_decode(
            ctx,
            name.as_str(),
            "validate nonblank text",
        )?
        else {
            continue;
        };
        if replace_existing {
            ctx.insert_btree_map(
                &mut feature.parameters,
                name,
                expression,
                "insert SLDPRT enriched parameter",
            )?;
        } else {
            ctx.entry_btree_map(
                &mut feature.parameters,
                name,
                "insert SLDPRT enriched parameter",
            )?
            .or_insert(expression);
        }
    }
    Ok(())
}

/// Infer a length unit from the owning operation and its native display role.
/// Move Face stores `D1` as distance. A standard fillet placeholder such as
/// `R0` also identifies a radius; variable fillets use indexed radii instead.
fn scalar_unit_from_feature_parameter(
    ctx: &DecodeContext<'_>,
    feature: &crate::records::Feature,
    name: &str,
) -> Result<Option<ScalarUnit>, CodecError> {
    if matches!(name, "D5" | "D6" | "D7")
        && crate::history::classify::matches_alnum_ascii(&feature.kind, b"cutextrudethin")
    {
        return Ok(Some(ScalarUnit::Length));
    }
    if name == "D1"
        && crate::classification::classify(feature)
            == Some(crate::classification::FeatureClass::MoveFace)
    {
        if let Some(mode) = ctx.get_btree_map(
            &feature.properties,
            "Mode",
            "lookup SLDPRT move-face mode",
        )? {
            if ctx.eq_ignore_ascii_case(mode, "Offset", "compare SLDPRT move-face mode")?
                || ctx.eq_ignore_ascii_case(mode, "Translate", "compare SLDPRT move-face mode")?
            {
                return Ok(Some(ScalarUnit::Length));
            }
        }
    }
    let Some(expression) = ctx.get_btree_map(
        &feature.parameters,
        name,
        "lookup SLDPRT feature parameter",
    )? else {
        return Ok(None);
    };
    let mut source_sketch_dimension = false;
    if crate::classification::classify(feature)
        == Some(crate::classification::FeatureClass::Sketch)
    {
        for content in ctx.admit_iter(&feature.content, "find SLDPRT sketch dimension")? {
            if let crate::records::FeatureContent::Dimension(dimension) = content {
                if ctx.equal(dimension.as_str(), name, "compare SLDPRT sketch dimension")? {
                    source_sketch_dimension = true;
                    break;
                }
            }
        }
    }
    if source_sketch_dimension {
        return if crate::history::literals::parse_angle_rad(expression).is_some() {
            Ok(Some(ScalarUnit::Angle))
        } else {
            Ok(crate::history::literals::parse_dimension_display_length(expression)
                .map(|_| ScalarUnit::Length))
        };
    }
    if crate::history::project::modify::fillet_radius_parameter_has_native_display(
        feature, name, expression,
    ) {
        return Ok(Some(ScalarUnit::Length));
    }
    Ok(None)
}

pub(super) fn value_only_scalar_offset(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    name: &FeatureInputName,
) -> Result<Option<usize>, CodecError> {
    let name_offset = usize::try_from(name.offset).map_err(|_| {
        ctx.refuse_codec_limit("locate SLDPRT value-only scalar", u64::MAX - 1, u64::MAX)
    })?;
    let name_length = ctx
        .admit_iter(name.value.as_str(), "measure SLDPRT scalar name")?
        .encode_utf16()
        .count();
    let name_bytes = name_length.checked_mul(2).ok_or_else(|| {
        ctx.refuse_codec_limit("locate SLDPRT value-only scalar", u64::MAX - 1, u64::MAX)
    })?;
    let header_offset = name_offset
        .checked_add(NAME_MARKER.len() + 1)
        .and_then(|offset| offset.checked_add(name_bytes))
        .ok_or_else(|| {
            ctx.refuse_codec_limit("locate SLDPRT value-only scalar", u64::MAX - 1, u64::MAX)
        })?;
    let value_offset = header_offset
        .checked_add(VALUE_ONLY_SCALAR_HEADER.len())
        .ok_or_else(|| {
            ctx.refuse_codec_limit("locate SLDPRT value-only scalar", u64::MAX - 1, u64::MAX)
        })?;
    Ok((payload.get(header_offset..value_offset) == Some(VALUE_ONLY_SCALAR_HEADER))
        .then_some(value_offset))
}

fn native_scalar_matches_discrete_parameter(
    feature: &crate::records::Feature,
    name: &str,
    expression: &str,
    value: f64,
) -> bool {
    match crate::history::parameters::parse_native_parameter_literal(feature, name, expression) {
        Some(cadmpeg_ir::features::ParameterValue::Integer(expected)) => {
            crate::history::parameters::eval::exact_integer_f64(expected) == Some(value)
        }
        Some(cadmpeg_ir::features::ParameterValue::Boolean(expected)) => {
            value == if expected { 1.0 } else { 0.0 }
        }
        _ => true,
    }
}

pub(crate) fn sync_changed_feature_scalars(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    histories: &[crate::records::FeatureHistory],
    lanes: &mut [FeatureInputLane],
    changed: &HashSet<(String, cadmpeg_core::text::NonBlankString)>,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::ParameterValue;

    for lane in lanes {
        let names_by_id = lane
            .names
            .iter()
            .map(|name| (name.id.as_str(), name.value.as_str()))
            .collect::<HashMap<_, _>>();
        let mut starts = histories
            .iter()
            .flat_map(|history| &history.features)
            .filter_map(|feature| {
                feature_object_name(feature, lane).map(|name| (name.offset, feature))
            })
            .collect::<Vec<_>>();
        ctx.stable_sort_by(
            &mut starts,
            |value| &value.0,
            Ord::cmp,
            "sldprt feature name starts sort",
        )?;
        let mut updates = Vec::<(usize, f64)>::new();
        for (index, &(start, feature)) in starts.iter().enumerate() {
            let end = starts.get(index + 1).map(|(offset, _)| *offset);
            for (name, expression) in &feature.parameters {
                if !changed.contains(&(feature.id.clone(), name.clone())) {
                    continue;
                }
                let candidates = lane.scalars.iter().enumerate().try_fold(
                    Vec::new(),
                    |mut candidates, (scalar_index, scalar)| -> Result<_, CodecError> {
                        if scalar_owned_by_feature(ctx, scalar, &feature.id, start, end)?
                            && names_by_id.get(scalar.name.as_str()) == Some(&name.as_str())
                        {
                            candidates.push((scalar_index, scalar));
                        }
                        Ok(candidates)
                    },
                )?;
                let driving = candidates
                    .iter()
                    .filter(|(_, scalar)| scalar.role == FeatureInputScalarRole::Driving)
                    .copied()
                    .collect::<Vec<_>>();
                let candidates = if driving.is_empty() {
                    candidates
                        .into_iter()
                        .filter(|(_, scalar)| scalar.role == FeatureInputScalarRole::Native)
                        .collect::<Vec<_>>()
                } else {
                    driving
                };
                let [(scalar_index, _)] = candidates.as_slice() else {
                    continue;
                };
                let value = match crate::history::parameters::parse_native_parameter_literal(
                    feature,
                    name.as_str(),
                    expression,
                ) {
                    Some(ParameterValue::Length(value)) => value.get() / 1000.0,
                    Some(ParameterValue::Angle(value)) => value.get(),
                    Some(ParameterValue::Real(value)) => value.get(),
                    _ => continue,
                };
                updates.push((*scalar_index, value));
            }
        }
        for (scalar_index, value) in updates {
            let scalar = &mut lane.scalars[scalar_index];
            let offset = usize::try_from(scalar.offset).map_err(|_| {
                cadmpeg_core::CodecError::Malformed(
                    "SLDPRT scalar offset exceeds address space".into(),
                )
            })?;
            let bytes = lane
                .native_payload
                .get_mut(offset..offset + 8)
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed(format_args!(
                        "SLDPRT scalar {} lies outside its payload",
                        scalar.id
                    ))
                })?;
            let checked = cadmpeg_ir::scalar::FiniteReal::new(value).ok_or_else(|| {
                cadmpeg_core::CodecError::malformed("SLDPRT scalar update is non-finite")
            })?;
            bytes.copy_from_slice(&value.to_le_bytes());
            scalar.value = checked;
        }
    }
    Ok(())
}

#[cfg(test)]
mod parameters_tests;
