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
    scalar: &FeatureInputScalar,
    feature: &str,
    start: u64,
    end: u64,
) -> bool {
    scalar.feature_ref.as_deref() == Some(feature)
        || (scalar.feature_ref.is_none() && scalar.offset > start && scalar.offset < end)
}

/// Add unambiguous `ResolvedFeatures` length parameters to a projection copy of history.
pub(crate) fn enrich_history_parameters<'a>(
    ctx: &DecodeContext<'_>,
    histories: &mut [crate::records::FeatureHistory],
    lanes: impl IntoIterator<Item = &'a FeatureInputLane>,
    replace_existing: bool,
) -> Result<(), CodecError> {
    let mut candidates = BTreeMap::<(usize, usize, String), Vec<(f64, ScalarUnit)>>::new();
    for lane in lanes {
        let feature_count = histories.iter().try_fold(0usize, |count, history| {
            count.checked_add(history.features.len()).ok_or_else(|| {
                ctx.refuse_codec_limit("scan SLDPRT parameter candidates", u64::MAX - 1, u64::MAX)
            })
        })?;
        let work = lane
            .classes
            .len()
            .checked_add(lane.names.len())
            .and_then(|count| count.checked_mul(lane.scalars.len()))
            .and_then(|count| {
                feature_count
                    .checked_mul(lane.names.len().checked_add(lane.scalars.len())?)
                    .and_then(|features| count.checked_add(features))
            })
            .ok_or_else(|| {
                ctx.refuse_codec_limit("scan SLDPRT parameter candidates", u64::MAX - 1, u64::MAX)
            })?;
        ctx.charge_work(
            u64::try_from(work).map_err(|_| {
                ctx.refuse_codec_limit("scan SLDPRT parameter candidates", u64::MAX - 1, u64::MAX)
            })?,
            "scan SLDPRT parameter candidates",
        )?;
        ctx.charge_collection_items(
            u64::try_from(lane.names.len()).map_err(|_| {
                ctx.refuse_codec_limit("index SLDPRT parameter names", u64::MAX - 1, u64::MAX)
            })?,
            "index SLDPRT parameter names",
        )?;
        let mut names_by_id = HashMap::new();
        names_by_id.try_reserve(lane.names.len()).map_err(|_| {
            ctx.refuse_codec_limit("index SLDPRT parameter names", u64::MAX - 1, u64::MAX)
        })?;
        for name in &lane.names {
            names_by_id.insert(name.id.as_str(), name);
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
        let units = lane
            .scalars
            .iter()
            .filter_map(|scalar| {
                let name = names_by_id.get(scalar.name.as_str())?;
                let parameter_class = lane
                    .classes
                    .iter()
                    .filter(|class| class.offset < name.offset)
                    .max_by_key(|class| class.offset)?;
                if lane.names.iter().any(|intervening| {
                    intervening.offset > parameter_class.offset && intervening.offset < name.offset
                }) {
                    return None;
                }
                let unit = match parameter_class.name.as_str() {
                    "moLengthParameter_c" => ScalarUnit::Length,
                    "moAngleParameter_c" => ScalarUnit::Angle,
                    _ => return None,
                };
                Some((scalar.id.as_str(), unit))
            })
            .chain(
                lane.relation_bindings
                    .iter()
                    .map(|binding| (binding.scalar_ref.as_str(), relation_unit(binding.family))),
            );
        let mut scalar_units = HashMap::new();
        for (scalar, unit) in units {
            if !scalar_units.contains_key(scalar) {
                ctx.charge_collection_items(1, "index SLDPRT scalar units")?;
                scalar_units.try_reserve(1).map_err(|_| {
                    ctx.refuse_codec_limit("index SLDPRT scalar units", u64::MAX - 1, u64::MAX)
                })?;
            }
            scalar_units.insert(scalar, unit);
        }
        for relation in &lane.relation_instances {
            let unit = relation_unit(relation.family);
            for scalar in relation.scalar_refs() {
                if !scalar_units.contains_key(scalar.as_str()) {
                    ctx.charge_collection_items(1, "index SLDPRT scalar units")?;
                    scalar_units.try_reserve(1).map_err(|_| {
                        ctx.refuse_codec_limit("index SLDPRT scalar units", u64::MAX - 1, u64::MAX)
                    })?;
                }
                scalar_units.insert(scalar.as_str(), unit);
            }
        }
        let mut starts = Vec::<(u64, usize, usize)>::new();
        for (history_index, history) in histories.iter().enumerate() {
            for (feature_index, feature) in history.features.iter().enumerate() {
                let Some(name) = feature_object_name(feature, lane) else {
                    continue;
                };
                ctx.reserve_collection_vec(
                    &mut starts,
                    1,
                    "collect SLDPRT parameter feature starts",
                )?;
                starts.push((name.offset, history_index, feature_index));
            }
        }
        ctx.stable_sort_by(
            &mut starts,
            |left, right| left.0.cmp(&right.0),
            |_| 0,
            "sort SLDPRT feature starts",
        )?;
        for (index, &(start, history_index, feature_index)) in starts.iter().enumerate() {
            let end = starts.get(index + 1).map_or(u64::MAX, |next| next.0);
            let feature = &histories[history_index].features[feature_index];
            let mut owned = BTreeMap::<&str, Vec<&FeatureInputScalar>>::new();
            for scalar in lane
                .scalars
                .iter()
                .filter(|scalar| scalar_owned_by_feature(scalar, &feature.id, start, end))
            {
                let Some(name) = names_by_id.get(scalar.name.as_str()) else {
                    continue;
                };
                if !owned.contains_key(name.value.as_str()) {
                    ctx.charge_collection_items(1, "index SLDPRT owned scalar names")?;
                    owned.insert(name.value.as_str(), Vec::new());
                }
                if let Some(group) = owned.get_mut(name.value.as_str()) {
                    ctx.reserve_collection_vec(group, 1, "collect SLDPRT owned scalars")?;
                    group.push(scalar);
                }
            }
            for (name, scalars) in owned {
                let mut driving = Vec::new();
                for scalar in scalars
                    .iter()
                    .filter(|scalar| scalar.role == FeatureInputScalarRole::Driving)
                {
                    ctx.reserve_collection_vec(&mut driving, 1, "collect SLDPRT driving scalars")?;
                    driving.push(*scalar);
                }
                let candidates_for_name = if driving.is_empty() {
                    let mut native = Vec::new();
                    for scalar in scalars
                        .into_iter()
                        .filter(|scalar| scalar.role == FeatureInputScalarRole::Native)
                    {
                        ctx.reserve_collection_vec(
                            &mut native,
                            1,
                            "collect SLDPRT native scalars",
                        )?;
                        native.push(scalar);
                    }
                    native
                } else {
                    driving
                };
                if let [scalar] = candidates_for_name.as_slice() {
                    let value_only = names_by_id.get(scalar.name.as_str()).is_some_and(|name| {
                        value_only_scalar_offset(&lane.native_payload, name)
                            == usize::try_from(scalar.offset).ok()
                    });
                    let unit = scalar_units
                        .get(scalar.id.as_str())
                        .copied()
                        .or_else(|| scalar_unit_from_feature_parameter(feature, name))
                        .unwrap_or(ScalarUnit::Native);
                    if value_only {
                        continue;
                    }
                    let name = crate::text_admission::format_retained(
                        ctx,
                        format_args!("{name}"),
                        "retain SLDPRT parameter candidate name",
                    )?;
                    let key = (history_index, feature_index, name);
                    match candidates.entry(key) {
                        std::collections::btree_map::Entry::Occupied(mut entry) => {
                            ctx.reserve_collection_vec(
                                entry.get_mut(),
                                1,
                                "collect SLDPRT parameter candidates",
                            )?;
                            entry.get_mut().push((scalar.value.get(), unit));
                        }
                        std::collections::btree_map::Entry::Vacant(entry) => {
                            ctx.charge_collection_items(1, "index SLDPRT parameter candidates")?;
                            let mut values = Vec::new();
                            ctx.reserve_collection_vec(
                                &mut values,
                                1,
                                "collect SLDPRT parameter candidates",
                            )?;
                            values.push((scalar.value.get(), unit));
                            entry.insert(values);
                        }
                    }
                }
            }
        }
    }

    for ((history_index, feature_index, name), values) in candidates {
        let Some((&(first, unit), rest)) = values.split_first() else {
            continue;
        };
        if rest.iter().any(|(value, candidate_unit)| {
            value.to_bits() != first.to_bits() || *candidate_unit != unit
        }) {
            continue;
        }
        let feature = &mut histories[history_index].features[feature_index];
        let source_dimension = feature.content.iter().any(|content| {
            matches!(content, crate::records::FeatureContent::Dimension(dimension) if dimension == &name)
        });
        if unit == ScalarUnit::Native
            && source_dimension
            && feature.parameters.contains_key(name.as_str())
        {
            continue;
        }
        if unit == ScalarUnit::Native
            && feature
                .parameters
                .get(name.as_str())
                .is_some_and(|expression| {
                    !native_scalar_matches_discrete_parameter(feature, &name, expression, first)
                })
        {
            continue;
        }
        let expression = match unit {
            ScalarUnit::Native => crate::history::parameters::format_native_scalar(
                feature,
                &name,
                first,
                feature.parameters.get(name.as_str()).map(String::as_str),
            ),
            ScalarUnit::Length
                if feature
                    .parameters
                    .get(name.as_str())
                    .is_some_and(|expression| {
                        crate::history::literals::strip_diameter_modifier(expression).is_some()
                    }) =>
            {
                crate::history::parameters::format_native_scalar(
                    feature,
                    &name,
                    first,
                    feature.parameters.get(name.as_str()).map(String::as_str),
                )
            }
            ScalarUnit::Length => cadmpeg_ir::scalar::Length::new(first * 1000.0)
                .map(crate::history::literals::format_length_mm),
            ScalarUnit::Angle => cadmpeg_ir::scalar::Angle::new(first)
                .map(crate::history::literals::format_angle_rad),
        };
        let Some(expression) = expression else {
            continue;
        };
        let Some(name) = cadmpeg_core::text::NonBlankString::new(name) else {
            continue;
        };
        if !feature.parameters.contains_key(name.as_str()) {
            ctx.charge_collection_items(1, "insert SLDPRT enriched parameter")?;
        }
        if replace_existing {
            feature.parameters.insert(name, expression);
        } else {
            feature.parameters.entry(name).or_insert(expression);
        }
    }
    Ok(())
}

/// Infer a length unit from the owning operation and its native display role.
/// Move Face stores `D1` as distance. A standard fillet placeholder such as
/// `R0` also identifies a radius; variable fillets use indexed radii instead.
fn scalar_unit_from_feature_parameter(
    feature: &crate::records::Feature,
    name: &str,
) -> Option<ScalarUnit> {
    if matches!(name, "D5" | "D6" | "D7")
        && crate::history::classify::matches_alnum_ascii(&feature.kind, b"cutextrudethin")
    {
        return Some(ScalarUnit::Length);
    }
    if name == "D1"
        && crate::classification::classify(feature)
            == Some(crate::classification::FeatureClass::MoveFace)
        && feature.properties.get("Mode").is_some_and(|mode| {
            mode.eq_ignore_ascii_case("Offset") || mode.eq_ignore_ascii_case("Translate")
        })
    {
        return Some(ScalarUnit::Length);
    }
    let expression = feature.parameters.get(name)?;
    let source_sketch_dimension = crate::classification::classify(feature)
        == Some(crate::classification::FeatureClass::Sketch)
        && feature.content.iter().any(|content| {
            matches!(content, crate::records::FeatureContent::Dimension(dimension) if dimension == name)
        });
    if source_sketch_dimension {
        return if crate::history::literals::parse_angle_rad(expression).is_some() {
            Some(ScalarUnit::Angle)
        } else {
            crate::history::literals::parse_dimension_display_length(expression)
                .map(|_| ScalarUnit::Length)
        };
    }
    if crate::history::project::modify::fillet_radius_parameter_has_native_display(
        feature, name, expression,
    ) {
        return Some(ScalarUnit::Length);
    }
    None
}

pub(super) fn value_only_scalar_offset(payload: &[u8], name: &FeatureInputName) -> Option<usize> {
    let name_offset = usize::try_from(name.offset).ok()?;
    let header_offset = name_offset
        .checked_add(NAME_MARKER.len() + 1)?
        .checked_add(name.value.encode_utf16().count().checked_mul(2)?)?;
    (payload.get(header_offset..header_offset + VALUE_ONLY_SCALAR_HEADER.len())
        == Some(VALUE_ONLY_SCALAR_HEADER))
    .then_some(header_offset + VALUE_ONLY_SCALAR_HEADER.len())
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
        starts.sort_by_key(|(offset, _)| *offset);
        let mut updates = Vec::<(usize, f64)>::new();
        for (index, &(start, feature)) in starts.iter().enumerate() {
            let end = starts
                .get(index + 1)
                .map_or(u64::MAX, |(offset, _)| *offset);
            for (name, expression) in &feature.parameters {
                if !changed.contains(&(feature.id.clone(), name.clone())) {
                    continue;
                }
                let candidates = lane
                    .scalars
                    .iter()
                    .enumerate()
                    .filter(|(_, scalar)| scalar_owned_by_feature(scalar, &feature.id, start, end))
                    .filter(|(_, scalar)| {
                        names_by_id.get(scalar.name.as_str()) == Some(&name.as_str())
                    })
                    .collect::<Vec<_>>();
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
