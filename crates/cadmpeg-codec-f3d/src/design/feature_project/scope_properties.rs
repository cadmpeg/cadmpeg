// SPDX-License-Identifier: Apache-2.0
//! Scope property and parameter-expression projection.

use crate::design::feature_project::DESIGN_LENGTH_TO_MILLIMETRE;
use crate::ids::native_stream;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::parameters::DesignParameter;
use crate::records::sketch_placement::DesignSketchPlacement;
use cadmpeg_core::CodecError;

pub(super) fn scope_properties(
    scope: &DesignParameterScope,
    native_scope: &str,
    placements: &[DesignSketchPlacement],
    parameters: &[(u32, &DesignParameter)],
) -> Result<std::collections::BTreeMap<cadmpeg_core::text::NonBlankString, String>, CodecError> {
    let mut properties = std::collections::BTreeMap::new();
    if scope.joint_origin_frame().is_some()
        || scope.kind() == crate::records::feature::scope::DesignFeatureKind::Assemble
            && scope.assembly_alignment().is_some_and(|alignment| {
                alignment.additional_tail_reference.is_some()
                    || alignment
                        .additional_owner_run
                        .as_ref()
                        .is_some_and(|run| run.count.value != 0)
            })
    {
        for (_, parameter) in parameters {
            properties.insert(
                cadmpeg_core::text::NonBlankString::new(format!("parameter:{}", parameter.name()))
                    .ok_or_else(|| {
                        CodecError::malformed("assembly parameter property key is blank")
                    })?,
                parameter.expression().to_owned(),
            );
        }
    }
    for (ordinal, record_index) in scope.reference_members().values().enumerate() {
        properties.insert(
            cadmpeg_core::text::NonBlankString::new(format!("reference:{ordinal}"))
                .ok_or_else(|| CodecError::malformed("reference property key is blank"))?,
            format!("{record_index}"),
        );
        if scope.kind() == crate::records::feature::scope::DesignFeatureKind::PathPattern
            && scope
                .unclosed_construction_operand_groups
                .contains(record_index)
        {
            properties.insert(
                cadmpeg_core::text::NonBlankString::new(format!("unclosed-group:{ordinal}"))
                    .ok_or_else(|| CodecError::malformed("unclosed group property key is blank"))?,
                format!("{record_index}"),
            );
        }
    }
    if let Some(profile) = scope
        .extrude_profile()
        .or(scope.base_flange_profile())
        .or_else(|| {
            scope
                .emboss_construction()
                .and_then(|construction| construction.sketch_profile.as_ref())
        })
    {
        if let Some(placement) = placements.iter().find(|placement| {
            native_stream(&placement.id) == Some(native_scope)
                && placement.entity_id == profile.entity_id
        }) {
            let id = crate::ids::neutral_sketch_id(placement);
            properties.insert(cadmpeg_core::nonblank_literal!("profile"), id.into_string());
        }
    }
    if let Some(construction) = scope.mesh_face_groups() {
        for (key, field) in [
            (
                cadmpeg_core::nonblank_literal!("mesh-face-groups:method-code"),
                &construction.method_code,
            ),
            (
                cadmpeg_core::nonblank_literal!("mesh-face-groups:source-mesh"),
                &construction.source_mesh,
            ),
            (
                cadmpeg_core::nonblank_literal!("mesh-face-groups:related-mesh"),
                &construction.related_mesh,
            ),
            (
                cadmpeg_core::nonblank_literal!("mesh-face-groups:group-count"),
                &construction.group_count,
            ),
        ] {
            if let Some(field) = field {
                properties.insert(key, field.value.to_string());
            }
        }
        if let Some(hint) = &construction.group_hint {
            properties.insert(
                cadmpeg_core::nonblank_literal!("mesh-face-groups:group-hint"),
                hint.value.clone(),
            );
        }
        for (ordinal, group) in construction.groups.iter().enumerate() {
            properties.insert(
                cadmpeg_core::text::NonBlankString::new(format!(
                    "mesh-face-groups:group:{ordinal}"
                ))
                .ok_or_else(|| CodecError::malformed("blank mesh group property"))?,
                group.value.clone(),
            );
        }
    }
    if matches!(
        scope.kind(),
        crate::records::feature::scope::DesignFeatureKind::Hole
            | crate::records::feature::scope::DesignFeatureKind::Emboss
    ) {
        for (_, parameter) in parameters {
            let key =
                cadmpeg_core::text::NonBlankString::new(format!("parameter:{}", parameter.name()))
                    .ok_or_else(|| CodecError::malformed("Hole parameter property key is blank"))?;
            let key = if properties.contains_key(&key) {
                cadmpeg_core::text::NonBlankString::new(format!(
                    "parameter-native:{}",
                    parameter.id
                ))
                .ok_or_else(|| CodecError::malformed("Hole native parameter key is blank"))?
            } else {
                key
            };
            properties.insert(key, parameter.expression().to_owned());
        }
    }
    if scope.kind() == crate::records::feature::scope::DesignFeatureKind::Emboss {
        if let Some(construction) = scope.emboss_construction() {
            use crate::records::feature::emboss::DesignEmbossControlWords;
            let controls: &[u32] = match &construction.control_words {
                DesignEmbossControlWords::One(words) => words,
                DesignEmbossControlWords::Two(words) => words,
            };
            for (ordinal, word) in controls.iter().enumerate() {
                properties.insert(
                    cadmpeg_core::text::NonBlankString::new(format!("emboss:control:{ordinal}"))
                        .ok_or_else(|| CodecError::malformed("Emboss control key is blank"))?,
                    word.to_string(),
                );
            }
            for (ordinal, pair) in construction
                .native_pair
                .iter()
                .chain(construction.additional_native_pairs.iter().flatten())
                .enumerate()
            {
                for (lane, word) in pair.iter().enumerate() {
                    properties.insert(
                        cadmpeg_core::text::NonBlankString::new(format!(
                            "emboss:native_pair:{ordinal}:{lane}"
                        ))
                        .ok_or_else(|| CodecError::malformed("Emboss pair key is blank"))?,
                        word.to_string(),
                    );
                }
            }
            for (reference, role) in [
                (construction.profile_group, "profile"),
                (construction.face_group, "face"),
                (construction.auxiliary_group, "auxiliary"),
            ] {
                properties.insert(
                    cadmpeg_core::text::NonBlankString::new(format!("emboss:group:{role}"))
                        .ok_or_else(|| CodecError::malformed("Emboss group key is blank"))?,
                    reference.value.to_string(),
                );
            }
            for (reference, role) in construction.scalar_owners() {
                properties.insert(
                    cadmpeg_core::text::NonBlankString::new(format!("emboss:owner:{role}"))
                        .ok_or_else(|| CodecError::malformed("Emboss owner key is blank"))?,
                    reference.value.to_string(),
                );
            }
        }
        for (role, unit, value) in [
            (
                "Depth",
                "mm",
                emboss_parameter(scope, parameters, "Depth")
                    .and_then(emboss_length)
                    .map(cadmpeg_ir::scalar::Length::get),
            ),
            (
                "Horizontal Distance",
                "mm",
                emboss_parameter(scope, parameters, "Horizontal Distance")
                    .and_then(emboss_length)
                    .map(cadmpeg_ir::scalar::Length::get),
            ),
            (
                "Vertical Distance",
                "mm",
                emboss_parameter(scope, parameters, "Vertical Distance")
                    .and_then(emboss_length)
                    .map(cadmpeg_ir::scalar::Length::get),
            ),
            (
                "Rotation Angle",
                "rad",
                emboss_parameter(scope, parameters, "Rotation Angle")
                    .map(|parameter| parameter.evaluated_value().get()),
            ),
        ] {
            if let Some(value) = value {
                properties.insert(
                    cadmpeg_core::text::NonBlankString::new(format!("emboss:scalar:{role}:{unit}"))
                        .ok_or_else(|| CodecError::malformed("Emboss scalar key is blank"))?,
                    value.to_string(),
                );
            }
        }
    }
    if scope.kind() == crate::records::feature::scope::DesignFeatureKind::Hole {
        if let Some(extent) = scope.hole_extent() {
            if let Some(hole_type) = &extent.hole_type {
                properties.insert(
                    cadmpeg_core::nonblank_literal!("hole:type"),
                    hole_type.value.to_string(),
                );
            }
            if let Some(word) = &extent.leading_word {
                properties.insert(
                    cadmpeg_core::nonblank_literal!("hole:extent_leading_word"),
                    word.value.to_string(),
                );
            }
            if let Some(control) = extent.extent_control {
                properties.insert(
                    cadmpeg_core::nonblank_literal!("hole:extent_control"),
                    control.to_string(),
                );
            }
            if let Some(depth) = &extent.saved_non_distance_depth {
                properties.insert(
                    cadmpeg_core::nonblank_literal!("hole:saved_non_distance_depth_cm"),
                    depth.value.get().to_string(),
                );
            }
            if let Some(control) = &extent.direction_control {
                properties.insert(
                    cadmpeg_core::nonblank_literal!("hole:direction_control"),
                    control.value.to_string(),
                );
            }
            if let Some(standard) = &extent.standard {
                properties.insert(
                    cadmpeg_core::nonblank_literal!("hole:standard"),
                    standard.standard.clone(),
                );
                properties.insert(
                    cadmpeg_core::nonblank_literal!("hole:fit"),
                    standard.fit.clone(),
                );
                properties.insert(
                    cadmpeg_core::nonblank_literal!("hole:size"),
                    standard.size.clone(),
                );
                properties.insert(
                    cadmpeg_core::nonblank_literal!("hole:fastener"),
                    standard.fastener.clone(),
                );
            }
        }
    }
    Ok(properties)
}

pub(super) fn scope_parameter_expressions(
    scope: &DesignParameterScope,
    parameters: &[(u32, &DesignParameter)],
) -> Result<std::collections::BTreeMap<cadmpeg_core::text::NonBlankString, String>, CodecError> {
    let mut properties = std::collections::BTreeMap::new();
    for (_, parameter) in parameters {
        let name = parameter.name().to_owned();
        let Some(name) = cadmpeg_core::text::NonBlankString::new(name) else {
            continue;
        };
        if properties.contains_key(&name) {
            return Err(cadmpeg_core::text::NamedEntryError::Restated {
                record: scope.id.clone(),
                key: name,
            }
            .into());
        }
        let expression = parameter.expression().to_owned();
        properties.insert(name, expression);
    }

    Ok(properties)
}

pub(super) fn emboss_parameter<'b>(
    scope: &DesignParameterScope,
    parameters: &[(u32, &'b DesignParameter)],
    source_kind: &str,
) -> Option<&'b DesignParameter> {
    if let Some(reference) = scope.emboss_construction().and_then(|construction| {
        construction
            .scalar_owners()
            .into_iter()
            .find_map(|(reference, role)| (role == source_kind).then_some(reference.value))
    }) {
        let mut named = parameters.iter().filter(|(_, parameter)| {
            parameter.source_kind_str() == source_kind
                && parameter.owner_record_index() == Some(reference)
        });
        if let Some((_, parameter)) = named.next() {
            if named.all(|(_, next)| next == parameter) {
                return Some(parameter);
            }
        }
    }
    let mut matching = parameters
        .iter()
        .filter(|(_, parameter)| parameter.source_kind_str() == source_kind)
        .map(|(_, parameter)| *parameter);
    let parameter = matching.next()?;
    matching.all(|next| next == parameter).then_some(parameter)
}

pub(super) fn emboss_length(parameter: &DesignParameter) -> Option<cadmpeg_ir::scalar::Length> {
    cadmpeg_ir::scalar::Length::new(parameter.evaluated_value().get() * DESIGN_LENGTH_TO_MILLIMETRE)
}
