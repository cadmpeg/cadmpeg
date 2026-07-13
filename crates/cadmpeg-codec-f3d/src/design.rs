// SPDX-License-Identifier: Apache-2.0
//! Decode Fusion Design object, sketch, identity, and construction records.
//!
//! These functions read Design `MetaStream.dat` and `BulkStream.dat` entries
//! selected by [`crate::container`]. Returned records retain source offsets and
//! stable identifiers for native regeneration.

use std::collections::{HashMap, HashSet};

use crate::records::{
    ConstructionRecipe, ConstructionRecipeKind, DesignBodyMember, DesignConfiguration,
    DesignConfigurationKind, DesignEntityHeader, DesignObject, DesignObjectKind, DesignParameter,
    DesignParameterKind, DesignParameterOwner, DesignRecordHeader, LostEdgeReference,
    PersistentReference, PersistentReferenceKind, SketchConstraintKind, SketchCurveGeometry,
    SketchCurveIdentity, SketchPoint, SketchRelation, SketchRelationOperand,
};
use cadmpeg_ir::codec::{CodecError, ReadSeek};
use cadmpeg_ir::le::{
    f64_at, f64s_at, lp_u32_bytes_at, u32_at, u32_at as read_u32, u64_at as read_u64, utf16le_at,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};

use crate::container::{role, ContainerScan};

const RECIPES: &[(&[u8], ConstructionRecipeKind)] = &[
    (b"body_recipe_data", ConstructionRecipeKind::Body),
    (b"face_recipe_data", ConstructionRecipeKind::Face),
    (
        b"bounded_face_recipe_data",
        ConstructionRecipeKind::BoundedFace,
    ),
    (b"edge_recipe_data", ConstructionRecipeKind::Edge),
    (b"vertex_recipe_data", ConstructionRecipeKind::Vertex),
];

/// Decode every JSON design-configuration table and rule entry.
pub fn decode_configurations(scan: &ContainerScan) -> Result<Vec<DesignConfiguration>, CodecError> {
    let configurations = scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::DESIGN_CONFIG)
        .map(|entry| {
            let bytes = scan.entry_bytes(&entry.name)?;
            let payload: serde_json::Value = serde_json::from_slice(bytes).map_err(|error| {
                CodecError::Malformed(format!(
                    "invalid F3D configuration JSON {}: {error}",
                    entry.name
                ))
            })?;
            if !payload.is_object() {
                return Err(CodecError::Malformed(format!(
                    "F3D configuration JSON must be an object: {}",
                    entry.name
                )));
            }
            let kind = if entry.name.ends_with(".dsgcfgrule") {
                DesignConfigurationKind::Rule
            } else {
                DesignConfigurationKind::Table
            };
            validate_configuration_payload(&entry.name, kind, &payload)?;
            Ok(DesignConfiguration {
                id: format!("f3d:configuration:entry#{}", entry.name),
                entry_name: entry.name.clone(),
                kind,
                payload,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut names = HashSet::new();
    let mut ids = HashSet::new();
    for configuration in &configurations {
        if !names.insert(configuration.entry_name.as_str())
            || !ids.insert(configuration.id.as_str())
        {
            return Err(CodecError::Malformed(format!(
                "duplicate F3D configuration identity: {}",
                configuration.entry_name
            )));
        }
    }
    Ok(configurations)
}

/// Validate the typed fields of one configuration document while permitting
/// unrecognized object members for forward-compatible native retention.
pub(crate) fn validate_configuration_payload(
    entry_name: &str,
    kind: DesignConfigurationKind,
    payload: &serde_json::Value,
) -> Result<(), CodecError> {
    let object = payload.as_object().ok_or_else(|| {
        CodecError::Malformed(format!(
            "F3D configuration JSON must be an object: {entry_name}"
        ))
    })?;
    if kind == DesignConfigurationKind::Rule {
        return Ok(());
    }
    let configurations = match object.get("configurations") {
        Some(value) => Some(value.as_object().ok_or_else(|| {
            CodecError::Malformed(format!(
                "F3D configuration table `configurations` must be an object: {entry_name}"
            ))
        })?),
        None => None,
    };
    if let Some(active) = object.get("active") {
        let active = active.as_str().ok_or_else(|| {
            CodecError::Malformed(format!(
                "F3D configuration table `active` must be a string: {entry_name}"
            ))
        })?;
        if !configurations.is_some_and(|variants| variants.contains_key(active)) {
            return Err(CodecError::Malformed(format!(
                "F3D active configuration `{active}` is not a named variant: {entry_name}"
            )));
        }
    }
    for (name, value) in configurations.into_iter().flatten() {
        let definition = value.as_object().ok_or_else(|| {
            CodecError::Malformed(format!(
                "F3D configuration variant `{name}` must be an object: {entry_name}"
            ))
        })?;
        if definition
            .get("parameters")
            .is_some_and(|value| !value.is_object())
        {
            return Err(CodecError::Malformed(format!(
                "F3D configuration variant `{name}` parameters must be an object: {entry_name}"
            )));
        }
        if let Some(suppressed) = definition.get("suppressed") {
            let valid = suppressed
                .as_array()
                .is_some_and(|values| values.iter().all(serde_json::Value::is_string));
            if !valid {
                return Err(CodecError::Malformed(format!(
                    "F3D configuration variant `{name}` suppressed list must contain strings: {entry_name}"
                )));
            }
        }
        if definition
            .get("material")
            .is_some_and(|value| !value.is_string())
        {
            return Err(CodecError::Malformed(format!(
                "F3D configuration variant `{name}` material must be a string: {entry_name}"
            )));
        }
    }
    Ok(())
}

/// Project named variants from configuration-table JSON into the neutral
/// configuration arena. Rule documents remain in the native arena because a
/// rule is a selector, not a model variant.
pub fn project_configurations(
    native: &[DesignConfiguration],
) -> Vec<cadmpeg_ir::features::DesignConfiguration> {
    use cadmpeg_ir::features::{ConfigurationId, DesignConfiguration as NeutralConfiguration};
    use std::collections::BTreeMap;

    let mut projected = Vec::new();
    for table in native
        .iter()
        .filter(|configuration| configuration.kind == DesignConfigurationKind::Table)
    {
        let active = table
            .payload
            .get("active")
            .and_then(serde_json::Value::as_str);
        let Some(configurations) = table
            .payload
            .get("configurations")
            .and_then(serde_json::Value::as_object)
        else {
            continue;
        };
        for (name, definition) in configurations {
            let mut properties = BTreeMap::new();
            let definition = definition.as_object();
            if let Some(parameters) = definition
                .and_then(|value| value.get("parameters"))
                .and_then(serde_json::Value::as_object)
            {
                for (parameter, value) in parameters {
                    properties.insert(format!("parameter:{parameter}"), json_scalar_text(value));
                }
            }
            if let Some(suppressed) = definition
                .and_then(|value| value.get("suppressed"))
                .and_then(serde_json::Value::as_array)
            {
                for feature in suppressed.iter().filter_map(serde_json::Value::as_str) {
                    properties.insert(format!("suppressed:{feature}"), "true".into());
                }
            }
            let material = definition
                .and_then(|value| value.get("material"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned);
            let ordinal = u32::try_from(projected.len()).unwrap_or(u32::MAX);
            projected.push(NeutralConfiguration {
                id: ConfigurationId(format!("f3d:configuration:variant#{ordinal}")),
                ordinal,
                active: active == Some(name.as_str()),
                source_index: None,
                name: name.clone(),
                material,
                properties,
                bodies: Vec::new(),
                native_ref: Some(table.id.clone()),
            });
        }
    }
    projected
}

/// Project document-owned user parameters into the neutral parameter arena.
/// Owned dimension and feature-input records remain native until their indexed
/// owner records have neutral constraint or feature identities.
pub fn project_user_parameters(
    native: &[DesignParameter],
) -> Vec<cadmpeg_ir::features::DesignParameter> {
    use cadmpeg_ir::features::{
        Angle, DesignParameter as NeutralParameter, Length, ParameterId, ParameterValue,
    };
    use std::collections::BTreeMap;

    let mut parameters = native
        .iter()
        .filter(|parameter| parameter.kind == DesignParameterKind::User)
        .map(|parameter| {
            let mut properties = BTreeMap::new();
            let value = match parameter.unit.as_deref() {
                Some("mm") => Some(ParameterValue::Length(Length(
                    parameter.evaluated_value * 10.0,
                ))),
                Some("deg") => Some(ParameterValue::Angle(Angle(parameter.evaluated_value))),
                None => Some(ParameterValue::Real(parameter.evaluated_value)),
                Some(unit) => {
                    properties.insert("unit".into(), unit.into());
                    None
                }
            };
            NeutralParameter {
                id: ParameterId(format!("f3d:model:parameter#{}", parameter.record_index)),
                owner: None,
                ordinal: parameter.source_ordinal,
                name: parameter.name.clone(),
                expression: parameter.expression.clone(),
                display: None,
                value,
                dependencies: Vec::new(),
                properties,
                pmi: None,
                native_ref: Some(parameter.id.clone()),
            }
        })
        .collect::<Vec<_>>();
    parameters.sort_by_key(|parameter| parameter.ordinal);

    let mut aliases = HashMap::<String, Option<ParameterId>>::new();
    for parameter in &parameters {
        aliases
            .entry(parameter.name.clone())
            .and_modify(|candidate| *candidate = None)
            .or_insert_with(|| Some(parameter.id.clone()));
    }
    for parameter in &mut parameters {
        let mut seen = HashSet::new();
        parameter.dependencies = expression_identifiers(&parameter.expression)
            .filter_map(|identifier| aliases.get(&identifier).and_then(Clone::clone))
            .filter(|dependency| dependency != &parameter.id && seen.insert(dependency.clone()))
            .collect();
    }
    parameters
}

fn expression_identifiers(expression: &str) -> impl Iterator<Item = String> + '_ {
    expression
        .split(|character: char| !(character.is_alphanumeric() || character == '_'))
        .filter(|token| {
            !token.is_empty()
                && token
                    .chars()
                    .next()
                    .is_some_and(|character| character.is_alphabetic() || character == '_')
        })
        .map(str::to_owned)
}

fn json_scalar_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(value) => value.clone(),
        serde_json::Value::Null => "null".into(),
        serde_json::Value::Bool(value) => value.to_string(),
        serde_json::Value::Number(value) => value.to_string(),
        value => value.to_string(),
    }
}

/// Decode every parametric construction-recipe record (`body_recipe_data`,
/// `face_recipe_data`, `bounded_face_recipe_data`, `edge_recipe_data`,
/// `vertex_recipe_data`) from each design `BulkStream` entry in `scan`.
/// `recipe_index` is assigned per `(kind, design_id)` group in stream order.
pub fn decode_recipes(
    _reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
) -> Result<Vec<ConstructionRecipe>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        decode_stream(bytes, &entry.name, &mut out);
    }
    Ok(out)
}

/// Decode every indexed parameter record in each Design `BulkStream`.
pub fn decode_parameters(
    _reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
) -> Result<Vec<DesignParameter>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let mut position = 0usize;
        while let Some(at) = next_indexed_record_offset(bytes, position) {
            let end = next_indexed_record_offset(bytes, at + 11).unwrap_or(bytes.len());
            if let Some(mut parameter) = parse_design_parameter(&bytes[at..end]) {
                parameter.id = format!("f3d:{}:design-parameter#{at}", entry.name);
                parameter.byte_offset = at as u64;
                parameter.expression_offset += at as u64;
                parameter.source_kind_offset += at as u64;
                parameter.unit_offset = parameter.unit_offset.map(|offset| offset + at as u64);
                parameter.name_offset += at as u64;
                parameter.evaluated_value_offset += at as u64;
                out.push(parameter);
                position = end;
            } else {
                position = at + 1;
            }
        }
    }
    out.sort_by_key(|parameter| parameter.record_index);
    Ok(out)
}

fn parse_design_parameter(payload: &[u8]) -> Option<DesignParameter> {
    let (class_tag, after_tag) = lp_ascii(payload, 0)?;
    if class_tag.len() != 3
        || !class_tag.bytes().all(|byte| byte.is_ascii_digit())
        || after_tag != 7
        || payload.get(11..31) != Some(&[0; 20])
    {
        return None;
    }
    let record_index = u32_at(payload, 7)?;
    let source_ordinal = u32_at(payload, 31)?;
    let (owner_record_index, expression_at, expression_trailer) = match payload.get(35)? {
        0 => (None, 36, [0, 0, 0, 0, 0, 0, 0, 0, 1]),
        1 if payload.get(40..46) == Some(&[0; 6]) => (Some(u32_at(payload, 36)?), 46, [0; 9]),
        _ => return None,
    };
    let (expression, expression_end) = lp_utf16(payload, expression_at)?;
    if payload.get(expression_end..expression_end + 9) != Some(&expression_trailer) {
        return None;
    }
    let source_kind_at = expression_end + 9;
    let (source_kind, source_kind_end) = lp_utf16(payload, source_kind_at)?;
    if u32_at(payload, source_kind_end) != Some(0) {
        return None;
    }
    let first_at = source_kind_end + 4;
    let (first, first_end) = lp_utf16(payload, first_at)?;
    let (unit, unit_offset, name, name_at, name_end) =
        if let Some((second, second_end)) = lp_utf16(payload, first_end) {
            (
                Some(first),
                Some(first_at + 4),
                second,
                first_end,
                second_end,
            )
        } else {
            (None, None, first, first_at, first_end)
        };
    let evaluated_value = f64_at(payload, name_end)?;
    if payload.get(name_end + 8..) != Some(&[0, 1, 19, 0, 0, 0, 0, 0, 0, 0, 0, 0])
        || expression.is_empty()
        || source_kind.is_empty()
        || name.is_empty()
        || !evaluated_value.is_finite()
    {
        return None;
    }
    let kind = if source_kind == "User Parameter" {
        DesignParameterKind::User
    } else if source_kind.contains("Dimension") {
        DesignParameterKind::Dimension
    } else {
        DesignParameterKind::Feature
    };
    Some(DesignParameter {
        id: String::new(),
        byte_offset: 0,
        class_tag,
        record_index,
        source_ordinal,
        owner_record_index,
        expression,
        expression_offset: (expression_at + 4) as u64,
        source_kind,
        source_kind_offset: (source_kind_at + 4) as u64,
        kind,
        unit,
        unit_offset: unit_offset.map(|offset| offset as u64),
        name,
        name_offset: (name_at + 4) as u64,
        evaluated_value,
        evaluated_value_offset: name_end as u64,
    })
}

/// Decode the fixed-width owner frame for every owned Design parameter.
pub fn decode_parameter_owners(
    scan: &ContainerScan,
    parameters: &[DesignParameter],
    headers: &[DesignRecordHeader],
) -> Result<Vec<DesignParameterOwner>, CodecError> {
    let headers = headers
        .iter()
        .map(|header| (header.record_index, header))
        .collect::<HashMap<_, _>>();
    let mut out = Vec::new();
    for parameter in parameters {
        let Some(owner_index) = parameter.owner_record_index else {
            continue;
        };
        let Some(header) = headers.get(&owner_index) else {
            continue;
        };
        let entry = scan.entries.iter().find(|entry| {
            entry.role == role::BULKSTREAM
                && entry.name.contains("Design")
                && parameter.id.starts_with(&format!("f3d:{}:", entry.name))
        });
        let Some(entry) = entry else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let at = usize::try_from(header.byte_offset).ok();
        let frame = at.and_then(|at| at.checked_add(104).and_then(|end| bytes.get(at..end)));
        let Some(mut owner) = frame.and_then(parse_parameter_owner) else {
            continue;
        };
        owner.id = format!(
            "f3d:{}:design-parameter-owner#{}",
            entry.name, header.byte_offset
        );
        owner.byte_offset = header.byte_offset;
        owner.evaluated_value_offset += header.byte_offset;
        out.push(owner);
    }
    out.sort_by_key(|owner| owner.record_index);
    Ok(out)
}

fn parse_parameter_owner(frame: &[u8]) -> Option<DesignParameterOwner> {
    let (class_tag, after_tag) = lp_ascii(frame, 0)?;
    if frame.len() != 104
        || after_tag != 7
        || class_tag.len() != 3
        || !class_tag.bytes().all(|byte| byte.is_ascii_digit())
        || frame.get(11..19) != Some(&[0; 8])
        || frame.get(19..24) != Some(&[1, 1, 0, 0, 0])
        || frame.get(24) != Some(&1)
        || frame.get(29..35) != Some(&[0; 6])
        || frame.get(39) != Some(&0)
        || frame.get(48) != Some(&1)
        || frame.get(53..59) != Some(&[0; 6])
        || frame.get(63..67) != Some(&[0; 4])
        || frame.get(67) != Some(&1)
        || frame.get(72..78) != Some(&[0; 6])
        || frame.get(78) != Some(&1)
        || frame.get(80) != Some(&0)
        || frame.get(81) != Some(&1)
        || frame.get(86..93) != Some(&[0; 7])
        || frame.get(93) != Some(&1)
        || frame.get(98..104) != Some(&[0; 6])
    {
        return None;
    }
    let record_index = u32_at(frame, 7)?;
    let scope_record_index = u32_at(frame, 25)?;
    if u32_at(frame, 68)? != scope_record_index
        || u32_at(frame, 94)? != scope_record_index
        || u32_at(frame, 49)? != record_index.checked_add(1)?
        || u32_at(frame, 82)? != record_index.checked_add(2)?
    {
        return None;
    }
    let evaluated_value = f64_at(frame, 40)?;
    if !evaluated_value.is_finite() {
        return None;
    }
    Some(DesignParameterOwner {
        id: String::new(),
        byte_offset: 0,
        class_tag,
        record_index,
        scope_record_index,
        local_ordinal: u32_at(frame, 35)?,
        evaluated_value,
        evaluated_value_offset: 40,
        parameter_record_index: u32_at(frame, 49)?,
        owned_ordinal: u32_at(frame, 59)?,
        variant: *frame.get(79)?,
        companion_record_index: u32_at(frame, 82)?,
    })
}

/// Decode the persistent u64 point and curve identity references
/// (`pt_tag`, `crv_primary_id`, `crv_secondary_id`, each typed
/// `IntrinsicMetaTypeuint64`) from every design `BulkStream` entry in `scan`,
/// sorted by stream offset.
pub fn decode_persistent_references(
    _reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
) -> Result<Vec<PersistentReference>, CodecError> {
    let mut out = Vec::new();
    for (entry_ordinal, entry) in scan
        .entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        for &(name, kind) in &[
            (b"pt_tag".as_slice(), PersistentReferenceKind::Point),
            (
                b"crv_primary_id".as_slice(),
                PersistentReferenceKind::CurvePrimary,
            ),
            (
                b"crv_secondary_id".as_slice(),
                PersistentReferenceKind::CurveSecondary,
            ),
        ] {
            let mut cursor = 0;
            while let Some(relative) = bytes[cursor..].windows(name.len()).position(|w| w == name) {
                let offset = cursor + relative;
                cursor = offset + name.len();
                let compact_type_offset = offset + name.len();
                let type_offset = if u32_at(bytes, compact_type_offset) == Some(23) {
                    compact_type_offset
                } else if u32_at(bytes, compact_type_offset) == Some(2)
                    && u32_at(bytes, compact_type_offset + 4) == Some(14)
                    && bytes
                        .get(compact_type_offset + 8..compact_type_offset + 22)
                        .is_some()
                    && u32_at(bytes, compact_type_offset + 22) == Some(23)
                {
                    compact_type_offset + 22
                } else {
                    continue;
                };
                let Some(length_bytes) = bytes.get(type_offset..type_offset + 4) else {
                    continue;
                };
                if u32::from_le_bytes(length_bytes.try_into().expect(
                    "invariant: length_bytes is a 4-byte slice from bytes.get(range) of length 4",
                )) != 23
                {
                    continue;
                }
                let type_name = b"IntrinsicMetaTypeuint64";
                if bytes.get(type_offset + 4..type_offset + 4 + type_name.len()) != Some(type_name)
                {
                    continue;
                }
                let value_offset = type_offset + 4 + type_name.len();
                let Some(raw) = bytes.get(value_offset..value_offset + 8) else {
                    continue;
                };
                out.push((
                    entry_ordinal,
                    PersistentReference {
                        id: format!("f3d:{}:persistent-reference#{offset}", entry.name),
                        byte_offset: offset as u64,
                        value_offset: (value_offset - offset) as u32,
                        kind,
                        value: u64::from_le_bytes(raw.try_into().expect(
                            "invariant: raw is an 8-byte slice from bytes.get(range) of length 8",
                        )),
                    },
                ));
            }
        }
    }
    out.sort_by_key(|(entry_ordinal, reference)| (*entry_ordinal, reference.byte_offset));
    Ok(out.into_iter().map(|(_, reference)| reference).collect())
}

/// Decode every `EDGE_REFERENCE_LOST` marker record from each design
/// `BulkStream` entry in `scan`: the ASCII literal, a `u32` length of `3`, a
/// three-digit class tag, and a `u32` record index.
pub fn decode_lost_edge_references(
    _reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
) -> Result<Vec<LostEdgeReference>, CodecError> {
    let mut out = Vec::new();
    let marker = b"EDGE_REFERENCE_LOST";
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let mut cursor = 0;
        while let Some(relative) = bytes[cursor..]
            .windows(marker.len())
            .position(|window| window == marker)
        {
            let offset = cursor + relative;
            cursor = offset + marker.len();
            let payload = offset + marker.len();
            let Some(length) = bytes.get(payload..payload + 4) else {
                continue;
            };
            if u32::from_le_bytes(
                length.try_into().expect(
                    "invariant: length is a 4-byte slice from bytes.get(range) of length 4",
                ),
            ) != 3
            {
                continue;
            }
            let Some(class_tag) = bytes.get(payload + 4..payload + 7) else {
                continue;
            };
            if !class_tag.iter().all(u8::is_ascii_digit) {
                continue;
            }
            let Some(index) = bytes.get(payload + 7..payload + 11) else {
                continue;
            };
            out.push(LostEdgeReference {
                id: format!("f3d:{}:lost-edge-reference#{offset}", entry.name),
                byte_offset: offset as u64,
                class_tag_offset: (payload + 4) as u64,
                class_tag: String::from_utf8_lossy(class_tag).into_owned(),
                record_index: u32::from_le_bytes(index.try_into().expect(
                    "invariant: index is a 4-byte slice from bytes.get(range) of length 4",
                )),
                record_index_offset: (payload + 7) as u64,
            });
        }
    }
    Ok(out)
}

/// Decode every GUID-owned design object record from each design
/// `MetaStream` entry in `scan` ([spec §8.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#81-design-metadata)): an ASCII type name, the design
/// entity IDs it owns, its self GUID, an optional parent GUID, and a
/// revision. Records whose type name does not match a known
/// [`DesignObjectKind`] are skipped.
pub fn decode_objects(
    _reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
) -> Result<Vec<DesignObject>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::METASTREAM && entry.name.contains("Design"))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let mut offset = 0usize;
        while offset + 8 <= bytes.len() {
            let Some((name, after_name)) = lp_ascii(bytes, offset) else {
                offset += 1;
                continue;
            };
            let Some(kind) = object_kind(&name) else {
                offset += 1;
                continue;
            };
            let Some(count_raw) = bytes.get(after_name..after_name + 4) else {
                break;
            };
            let count = usize::try_from(u32::from_le_bytes(count_raw.try_into().expect(
                "invariant: count_raw is a 4-byte slice from bytes.get(range) of length 4",
            )))
            .unwrap_or(usize::MAX);
            let ids_end = after_name
                .checked_add(4)
                .and_then(|at| count.checked_mul(8).and_then(|size| at.checked_add(size)));
            let Some(ids_end) = ids_end.filter(|end| count <= 200 && *end <= bytes.len()) else {
                offset += 1;
                continue;
            };
            let entity_ids = bytes[after_name + 4..ids_end]
                .chunks_exact(8)
                .map(|raw| {
                    u64::from_le_bytes(
                        raw.try_into()
                            .expect("invariant: chunks_exact(8) yields 8-byte slices"),
                    )
                })
                .collect::<Vec<_>>();
            let entity_id_offsets = (0..entity_ids.len())
                .map(|index| (after_name + 4 + index * 8) as u64)
                .collect();
            let Some((self_guid, after_self)) =
                lp_ascii(bytes, ids_end).filter(|(guid, _)| is_guid(guid))
            else {
                offset += 1;
                continue;
            };
            let mut tail = after_self;
            while bytes.get(tail) == Some(&0) {
                tail += 1;
            }
            let zero_run_length = u32::try_from(tail - after_self).unwrap_or(u32::MAX);
            let (parent_guid, parent_guid_offset, revision_offset) = lp_ascii(bytes, tail)
                .filter(|(guid, _)| is_guid(guid))
                .map_or((None, None, tail), |(guid, end)| {
                    (Some(guid), Some((tail + 4) as u64), end)
                });
            let Some(revision_raw) = bytes.get(revision_offset..revision_offset + 4) else {
                offset += 1;
                continue;
            };
            let revision = u32::from_le_bytes(revision_raw.try_into().expect(
                "invariant: revision_raw is a 4-byte slice from bytes.get(range) of length 4",
            ));
            if revision > 10_000 {
                offset += 1;
                continue;
            }
            out.push(DesignObject {
                id: format!("f3d:{}:design-object#{offset}", entry.name),
                byte_offset: offset as u64,
                kind,
                entity_ids,
                entity_id_offsets,
                self_guid,
                self_guid_offset: (ids_end + 4) as u64,
                zero_run_length,
                parent_guid,
                parent_guid_offset,
                revision,
                revision_offset: revision_offset as u64,
            });
            offset = revision_offset + 4;
        }
    }
    Ok(out)
}

/// Decode every self-validating per-entity design `BulkStream` header (spec
/// [§8.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#81-design-metadata)): a three-digit class tag, an entity suffix, a UTF-16LE entity ID
/// whose numeric suffix must match the header's entity suffix, and, for
/// sketch-typed entities, the trailing reference-list header.
pub fn decode_entity_headers(
    reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
) -> Result<Vec<DesignEntityHeader>, CodecError> {
    let mut out = Vec::new();
    let mut object_kinds = HashMap::new();
    for object in decode_objects(reader, scan)? {
        for entity_id in object.entity_ids {
            object_kinds.entry(entity_id).or_insert(object.kind);
        }
    }
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let mut offset = 0usize;
        while offset + 30 <= bytes.len() {
            let Some(relative) = bytes[offset..]
                .windows(4)
                .position(|window| window == [3, 0, 0, 0])
            else {
                break;
            };
            let start = offset + relative;
            offset = start + 1;
            let Some(class_tag) = bytes.get(start + 4..start + 7) else {
                break;
            };
            if !class_tag.iter().all(u8::is_ascii_digit) {
                continue;
            }
            let Some(entity_raw) = bytes.get(start + 7..start + 15) else {
                break;
            };
            let entity_suffix = u64::from_le_bytes(entity_raw.try_into().expect(
                "invariant: entity_raw is an 8-byte slice from bytes.get(range) of length 8",
            ));
            if entity_suffix == 0
                || entity_suffix >= 1 << 32
                || bytes.get(start + 15..start + 20) != Some(&[0u8; 5])
            {
                continue;
            }
            let (optional_slot_present, string_offset) = match bytes[start + 20] {
                0 => (false, start + 21),
                1 if bytes.get(start + 21..start + 25) == Some(&[0u8; 4]) => (true, start + 25),
                _ => continue,
            };
            let Some((entity_id, end)) = lp_utf16(bytes, string_offset) else {
                continue;
            };
            let Some((_, suffix)) = entity_id.rsplit_once('_') else {
                continue;
            };
            if suffix.parse::<u64>().ok() != Some(entity_suffix) {
                continue;
            }
            let object_kind = object_kinds.get(&entity_suffix).copied();
            let (
                record_reference,
                record_reference_offset,
                declared_reference_count,
                reference_indices,
                reference_offsets,
                record_end,
            ) = if object_kind == Some(DesignObjectKind::Sketch) {
                decode_reference_list(bytes, end).map_or_else(
                    || (None, None, None, Vec::new(), Vec::new(), end),
                    |list| {
                        (
                            Some(list.record_reference),
                            Some(list.record_reference_offset as u64),
                            Some(list.declared_count),
                            list.references,
                            list.reference_offsets
                                .into_iter()
                                .map(|offset| offset as u64)
                                .collect(),
                            list.end,
                        )
                    },
                )
            } else {
                (None, None, None, Vec::new(), Vec::new(), end)
            };
            out.push(DesignEntityHeader {
                id: format!("f3d:{}:design-entity-header#{start}", entry.name),
                byte_offset: start as u64,
                entity_suffix,
                entity_id,
                class_tag: String::from_utf8_lossy(class_tag).into_owned(),
                optional_slot_present,
                object_kind,
                record_reference,
                record_reference_offset,
                declared_reference_count,
                reference_indices,
                reference_offsets,
            });
            offset = record_end;
        }
    }
    Ok(out)
}

/// Decode the indexed dynamic-class record headers ([spec §8.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#81-design-metadata)) that `entities`'
/// reference-list entries point at: a `u32` record index and a three-digit
/// class tag, for each record index named by any [`DesignEntityHeader`] in
/// `entities`.
pub fn decode_record_headers(
    reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
    entities: &[DesignEntityHeader],
) -> Result<Vec<DesignRecordHeader>, CodecError> {
    let wanted = entities
        .iter()
        .flat_map(|entity| &entity.reference_indices)
        .copied()
        .collect::<std::collections::HashSet<_>>();
    decode_headers_for_indices(reader, scan, &wanted)
}

/// Decode the indexed dynamic-class record headers ([spec §8.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#81-design-metadata)) named by
/// `indices` directly, bypassing entity reference lists. Used to fetch record
/// headers referenced by records other than [`DesignEntityHeader`] (for
/// example, sketch relation records).
pub fn decode_related_record_headers(
    reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
    indices: &[u32],
) -> Result<Vec<DesignRecordHeader>, CodecError> {
    let wanted = indices
        .iter()
        .copied()
        .collect::<std::collections::HashSet<_>>();
    decode_headers_for_indices(reader, scan, &wanted)
}

fn decode_headers_for_indices(
    _reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
    wanted: &std::collections::HashSet<u32>,
) -> Result<Vec<DesignRecordHeader>, CodecError> {
    if wanted.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    let mut emitted = std::collections::HashSet::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let mut position = 0usize;
        while position + 11 <= bytes.len() {
            let Some((class_tag, after_tag)) = lp_ascii(bytes, position) else {
                position += 1;
                continue;
            };
            if class_tag.len() != 3 || !class_tag.bytes().all(|byte| byte.is_ascii_digit()) {
                position += 1;
                continue;
            }
            let Some(raw) = bytes.get(after_tag..after_tag + 4) else {
                break;
            };
            let record_index = u32::from_le_bytes(
                raw.try_into()
                    .expect("invariant: raw is a 4-byte slice from bytes.get(range) of length 4"),
            );
            if wanted.contains(&record_index) && emitted.insert(record_index) {
                out.push(DesignRecordHeader {
                    id: format!("f3d:{}:design-record-header#{position}", entry.name),
                    record_index,
                    class_tag,
                    byte_offset: position as u64,
                });
            }
            // Headers are located in an otherwise heterogeneous stream. Keep
            // the scan byte-aligned so a plausible length-prefixed string in
            // an enclosing payload cannot skip a real nested header.
            position += 1;
        }
    }
    out.sort_by_key(|record| record.record_index);
    Ok(out)
}

/// Decode the sketch-relation body at each `records` entry's offset: the
/// owning sketch relation's member reference list, owner reference, state,
/// and return-member list. `records` supplies the byte offsets and class tags
/// (typically from [`decode_related_record_headers`]).
pub fn decode_sketch_relations(
    _reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
    records: &[DesignRecordHeader],
    entities: &[DesignEntityHeader],
) -> Result<Vec<SketchRelation>, CodecError> {
    let mut out = Vec::new();
    let owners = entities
        .iter()
        .filter(|entity| entity.object_kind == Some(DesignObjectKind::Sketch))
        .map(|entity| entity.entity_suffix as u32)
        .collect::<std::collections::HashSet<_>>();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        for record in records {
            let Ok(at) = usize::try_from(record.byte_offset) else {
                continue;
            };
            let record_end = next_indexed_record_offset(bytes, at + 11).unwrap_or(bytes.len());
            let Some(payload) = bytes.get(at..record_end) else {
                continue;
            };
            let Some((
                members,
                member_offsets,
                auxiliary_references,
                auxiliary_reference_offsets,
                owner_reference,
                owner_reference_offset,
                state,
                state_offset,
                return_members,
                return_member_offsets,
                parsed_end,
            )) = parse_sketch_relation(payload, &owners)
            else {
                continue;
            };
            if payload
                .get(parsed_end..)
                .is_none_or(|padding| padding.iter().any(|byte| *byte != 0))
            {
                continue;
            }
            let (constraint_kinds, unknown_constraint_bits) = decode_constraint_kinds(state);
            out.push(SketchRelation {
                id: format!("f3d:{}:sketch-relation#{}", entry.name, record.record_index),
                record_index: record.record_index,
                class_tag: record.class_tag.clone(),
                byte_offset: record.byte_offset,
                state_offset: state_offset as u32,
                owner_reference,
                owner_entity_id: String::new(),
                owner_reference_offset: owner_reference_offset as u32,
                auxiliary_references,
                auxiliary_reference_offsets: auxiliary_reference_offsets
                    .into_iter()
                    .map(|offset| offset as u32)
                    .collect(),
                members,
                resolved_members: Vec::new(),
                member_offsets: member_offsets
                    .into_iter()
                    .map(|offset| offset as u32)
                    .collect(),
                state,
                constraint_kinds,
                unknown_constraint_bits,
                return_members,
                resolved_return_members: Vec::new(),
                return_member_offsets: return_member_offsets
                    .into_iter()
                    .map(|offset| offset as u32)
                    .collect(),
                raw_bytes: payload.to_vec(),
            });
        }
    }
    Ok(out)
}

pub(crate) const SKETCH_CONSTRAINT_MASK: u32 = 0x3000_3fff;

pub(crate) fn decode_constraint_kinds(state: u32) -> (Vec<SketchConstraintKind>, u32) {
    let definitions = [
        (0x0000_0001, SketchConstraintKind::Coincident),
        (0x0000_0002, SketchConstraintKind::Colinear),
        (0x0000_0004, SketchConstraintKind::Concentric),
        (0x0000_0008, SketchConstraintKind::EqualLength),
        (0x0000_0010, SketchConstraintKind::Parallel),
        (0x0000_0020, SketchConstraintKind::Perpendicular),
        (0x0000_0040, SketchConstraintKind::Horizontal),
        (0x0000_0080, SketchConstraintKind::Vertical),
        (0x0000_0100, SketchConstraintKind::Tangent),
        (0x0000_0200, SketchConstraintKind::Curvature),
        (0x0000_0400, SketchConstraintKind::Symmetry),
        (0x0000_0800, SketchConstraintKind::Equal),
        (0x0000_1000, SketchConstraintKind::Midpoint),
        (0x0000_2000, SketchConstraintKind::Polygon),
        (0x1000_0000, SketchConstraintKind::CircularPattern),
        (0x2000_0000, SketchConstraintKind::RectangularPattern),
    ];
    let mut kinds = if state == 0 {
        vec![SketchConstraintKind::Coincident]
    } else {
        Vec::new()
    };
    let mut recognized = 0u32;
    for (bit, kind) in definitions {
        if state & bit != 0 {
            kinds.push(kind);
            recognized |= bit;
        }
    }
    debug_assert_eq!(recognized, state & SKETCH_CONSTRAINT_MASK);
    (kinds, state & !SKETCH_CONSTRAINT_MASK)
}

/// Decode every sketch-point record ([spec §8.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#81-design-metadata), `pt_tag`) from each design
/// `BulkStream` entry in `scan`: the persistent point id, a paired record
/// reference, and the sketch `(u, v)` coordinates, converted centimetre→
/// millimetre. Records whose scaled coordinates are non-finite are skipped.
pub fn decode_sketch_points(
    _reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
) -> Result<Vec<SketchPoint>, CodecError> {
    let mut out = Vec::new();
    let mut emitted = std::collections::HashSet::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let mut at = 0usize;
        while at + 112 <= bytes.len() {
            let Some((class_tag, after_tag)) = lp_ascii(bytes, at) else {
                at += 1;
                continue;
            };
            if class_tag.len() != 3 || !class_tag.bytes().all(|byte| byte.is_ascii_digit()) {
                at += 1;
                continue;
            }
            let Some(record_index) = u32_at(bytes, after_tag) else {
                break;
            };
            let payload = &bytes[at..];
            let Some((persistent_id, paired_reference, x, y, shift, entity_genesis)) =
                decode_sketch_point(payload)
            else {
                at += 1;
                continue;
            };
            let (u, v) = (x * 10.0, y * 10.0);
            if !u.is_finite() || !v.is_finite() {
                at += 1;
                continue;
            }
            if emitted.insert(record_index) {
                out.push(SketchPoint {
                    id: format!("f3d:{}:sketch-point#{at}", entry.name),
                    record_index,
                    owner_reference: None,
                    class_tag,
                    byte_offset: at as u64,
                    coordinate_offset: (89 + shift) as u32,
                    entity_genesis,
                    persistent_id,
                    paired_reference,
                    coordinates: Point2::new(u, v),
                    raw_bytes: payload[..112 + shift].to_vec(),
                });
            }
            at += 112;
        }
    }
    Ok(out)
}

fn decode_sketch_point(payload: &[u8]) -> Option<(u64, u32, f64, f64, usize, Option<u64>)> {
    if let Some(point) = decode_sketch_point_variant(payload, 0, 1) {
        return Some((point.0, point.1, point.2, point.3, 0, None));
    }
    if u32_at(payload, 25) != Some(13)
        || payload.get(29..42) != Some(b"EntityGenesis")
        || u32_at(payload, 42) != Some(23)
        || payload.get(46..69) != Some(b"IntrinsicMetaTypeuint64")
    {
        return None;
    }
    let entity_genesis = u64::from_le_bytes(payload.get(69..77)?.try_into().ok()?);
    decode_sketch_point_variant(payload, 52, 2)
        .map(|point| (point.0, point.1, point.2, point.3, 52, Some(entity_genesis)))
}

fn decode_sketch_point_variant(
    payload: &[u8],
    shift: usize,
    property_count: u32,
) -> Option<(u64, u32, f64, f64)> {
    if payload.get(20) != Some(&1)
        || u32_at(payload, 21) != Some(property_count)
        || u32_at(payload, 25 + shift) != Some(6)
        || payload.get(29 + shift..35 + shift) != Some(b"pt_tag")
        || u32_at(payload, 35 + shift) != Some(23)
        || payload.get(39 + shift..62 + shift) != Some(b"IntrinsicMetaTypeuint64")
        || payload.get(70 + shift) != Some(&1)
        || !payload
            .get(75 + shift..89 + shift)?
            .iter()
            .all(|&byte| byte <= 1)
    {
        return None;
    }
    Some((
        u64::from_le_bytes(payload.get(62 + shift..70 + shift)?.try_into().ok()?),
        u32_at(payload, 71 + shift)?,
        f64_at(payload, 89 + shift)?,
        f64_at(payload, 97 + shift)?,
    ))
}

/// Decode every sketch-curve record ([spec §8.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#81-design-metadata), `crv_primary_id`/
/// `crv_secondary_id`) from each design `BulkStream` entry in `scan`: the
/// curve's persistent primary and secondary identities plus its NURBS, circular
/// arc, line, or referenced analytic geometry.
pub fn decode_sketch_curve_identities(
    _reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
) -> Result<Vec<SketchCurveIdentity>, CodecError> {
    let mut out = Vec::new();
    let mut emitted = std::collections::HashSet::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let mut at = 0usize;
        while at + 133 <= bytes.len() {
            let Some((class_tag, after_tag)) = lp_ascii(bytes, at) else {
                at += 1;
                continue;
            };
            if class_tag.len() != 3 || !class_tag.bytes().all(|byte| byte.is_ascii_digit()) {
                at += 1;
                continue;
            }
            let Some(record_index) = u32_at(bytes, after_tag) else {
                break;
            };
            let payload = &bytes[at..];
            let Some((primary_id, secondary_id, geometry_shift, entity_genesis)) =
                decode_sketch_curve_identity(payload)
            else {
                at += 1;
                continue;
            };
            if emitted.insert(record_index) {
                let geometry_payload = payload
                    .get(geometry_shift..)
                    .expect("invariant: geometry_shift (0 or 52) is <= payload.len() (checked >= 133 by the at + 133 <= bytes.len() loop guard)");
                out.push(SketchCurveIdentity {
                    id: format!("f3d:{}:sketch-curve-identity#{at}", entry.name),
                    record_index,
                    owner_reference: None,
                    class_tag,
                    byte_offset: at as u64,
                    geometry_offset: (133 + geometry_shift) as u32,
                    entity_genesis,
                    primary_id,
                    secondary_id,
                    geometry: decode_sketch_nurbs(geometry_payload)
                        .or_else(|| decode_circular_arc(geometry_payload))
                        .or_else(|| decode_line(geometry_payload))
                        .or_else(|| decode_referenced_analytic(geometry_payload)),
                });
            }
            at += 133;
        }
    }
    Ok(out)
}

/// Bind relation-connected sketch geometry to its unique owning sketch.
pub(crate) fn bind_sketch_graph(
    entities: &[DesignEntityHeader],
    points: &mut [SketchPoint],
    curves: &mut [SketchCurveIdentity],
    relations: &mut [SketchRelation],
) -> Result<(), CodecError> {
    let sketch_owners = entities
        .iter()
        .filter(|entity| entity.object_kind == Some(DesignObjectKind::Sketch))
        .map(|entity| (entity.entity_suffix as u32, entity.entity_id.as_str()))
        .collect::<std::collections::HashMap<_, _>>();
    for relation in relations.iter_mut() {
        relation.owner_entity_id = sketch_owners
            .get(&relation.owner_reference)
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "Fusion sketch relation {} has no owning Design entity {}",
                    relation.record_index, relation.owner_reference
                ))
            })?
            .to_string();
    }
    let typed_records = points
        .iter()
        .map(|point| point.record_index)
        .chain(curves.iter().map(|curve| curve.record_index))
        .collect::<std::collections::HashSet<_>>();
    let mut owners = std::collections::HashMap::new();
    for relation in relations.iter() {
        for record_index in relation.members.iter().chain(&relation.return_members) {
            if !typed_records.contains(record_index) {
                continue;
            }
            if owners
                .insert(*record_index, relation.owner_reference)
                .is_some_and(|owner| owner != relation.owner_reference)
            {
                return Err(CodecError::Malformed(format!(
                    "Fusion sketch record {record_index} belongs to multiple sketches"
                )));
            }
        }
    }
    for point in points.iter_mut() {
        point.owner_reference = owners.get(&point.record_index).copied();
    }
    for curve in curves.iter_mut() {
        curve.owner_reference = owners.get(&curve.record_index).copied();
    }
    let operands = points
        .iter()
        .map(|point| {
            (
                point.record_index,
                SketchRelationOperand::Point {
                    record_index: point.record_index,
                    persistent_id: point.persistent_id,
                },
            )
        })
        .chain(curves.iter().map(|curve| {
            (
                curve.record_index,
                SketchRelationOperand::Curve {
                    record_index: curve.record_index,
                    primary_id: curve.primary_id,
                    secondary_id: curve.secondary_id,
                },
            )
        }))
        .collect::<std::collections::HashMap<_, _>>();
    let resolve = |indices: &[u32]| {
        indices
            .iter()
            .map(|record_index| {
                operands
                    .get(record_index)
                    .cloned()
                    .unwrap_or(SketchRelationOperand::Record {
                        record_index: *record_index,
                    })
            })
            .collect()
    };
    for relation in relations {
        relation.resolved_members = resolve(&relation.members);
        relation.resolved_return_members = resolve(&relation.return_members);
    }
    Ok(())
}

fn decode_sketch_curve_identity(payload: &[u8]) -> Option<(u64, u64, usize, Option<u64>)> {
    if let Some((primary, secondary)) = decode_sketch_curve_identity_variant(payload, 0, 2) {
        return Some((primary, secondary, 0, None));
    }
    if u32_at(payload, 25) != Some(13)
        || payload.get(29..42) != Some(b"EntityGenesis")
        || u32_at(payload, 42) != Some(23)
        || payload.get(46..69) != Some(b"IntrinsicMetaTypeuint64")
    {
        return None;
    }
    let entity_genesis = u64::from_le_bytes(payload.get(69..77)?.try_into().ok()?);
    decode_sketch_curve_identity_variant(payload, 52, 3)
        .map(|(primary, secondary)| (primary, secondary, 52, Some(entity_genesis)))
}

fn decode_sketch_curve_identity_variant(
    payload: &[u8],
    shift: usize,
    property_count: u32,
) -> Option<(u64, u64)> {
    if payload.get(20) != Some(&1)
        || u32_at(payload, 21) != Some(property_count)
        || u32_at(payload, 25 + shift) != Some(14)
        || payload.get(29 + shift..43 + shift) != Some(b"crv_primary_id")
        || u32_at(payload, 43 + shift) != Some(23)
        || payload.get(47 + shift..70 + shift) != Some(b"IntrinsicMetaTypeuint64")
        || u32_at(payload, 78 + shift) != Some(16)
        || payload.get(82 + shift..98 + shift) != Some(b"crv_secondary_id")
        || u32_at(payload, 98 + shift) != Some(23)
        || payload.get(102 + shift..125 + shift) != Some(b"IntrinsicMetaTypeuint64")
    {
        return None;
    }
    Some((
        u64::from_le_bytes(payload.get(70 + shift..78 + shift)?.try_into().ok()?),
        u64::from_le_bytes(payload.get(125 + shift..133 + shift)?.try_into().ok()?),
    ))
}

fn decode_circular_arc(payload: &[u8]) -> Option<SketchCurveGeometry> {
    let values = (0..12)
        .map(|ordinal| f64_at(payload, 133 + ordinal * 8))
        .collect::<Option<Vec<_>>>()?;
    if values.iter().any(|value| !value.is_finite()) {
        return None;
    }
    let normal = Vector3::new(values[3], values[4], values[5]);
    let reference_direction = Vector3::new(values[6], values[7], values[8]);
    let dot = normal.x * reference_direction.x
        + normal.y * reference_direction.y
        + normal.z * reference_direction.z;
    if (normal.norm() - 1.0).abs() > 1.0e-9
        || (reference_direction.norm() - 1.0).abs() > 1.0e-9
        || dot.abs() > 1.0e-9
        || values[9] <= 0.0
        || values[10].abs() > std::f64::consts::TAU + 1.0e-9
        || values[11].abs() > std::f64::consts::TAU + 1.0e-9
        || (values[11] - values[10]).abs() < 1.0e-12
    {
        return None;
    }
    Some(SketchCurveGeometry::Arc {
        center: Point3::new(values[0] * 10.0, values[1] * 10.0, values[2] * 10.0),
        normal,
        reference_direction,
        radius: values[9] * 10.0,
        start_angle: values[10],
        end_angle: values[11],
    })
}

fn decode_referenced_analytic(payload: &[u8]) -> Option<SketchCurveGeometry> {
    if payload.get(133) != Some(&1) || payload.get(138..144) != Some(&[0; 6]) {
        return None;
    }
    let shifted = payload.get(11..)?;
    decode_circular_arc(shifted).or_else(|| decode_line(shifted))
}

fn decode_sketch_nurbs(payload: &[u8]) -> Option<SketchCurveGeometry> {
    let base = 133usize;
    let prefix = payload.get(base..base + 8)?;
    let carrier_reference = (prefix != [0xff; 8]).then(|| {
        u64::from_le_bytes(
            prefix
                .try_into()
                .expect("invariant: prefix is an 8-byte slice from payload.get(range) of length 8"),
        )
    });
    if u32_at(payload, base + 8) != Some(3) || payload.get(base + 88) != Some(&1) {
        return None;
    }
    let subtype_class_tag = std::str::from_utf8(payload.get(base + 12..base + 15)?)
        .ok()?
        .to_string();
    if !subtype_class_tag.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let degree = u32_at(payload, base + 90)?;
    let fit_tolerance = f64_at(payload, base + 94)?;
    let knot_count = usize::try_from(u32_at(payload, base + 102)?).ok()?;
    if u32_at(payload, base + 106)? as usize != knot_count
        || u32_at(payload, base + 110)? != 8
        || knot_count > 100_000
    {
        return None;
    }
    let knots = f64s_at(payload, base + 114, knot_count)?;
    let weights_at = base + 114 + knot_count * 8;
    let weight_count = usize::try_from(u32_at(payload, weights_at)?).ok()?;
    if u32_at(payload, weights_at + 4)? as usize != weight_count
        || u32_at(payload, weights_at + 8)? != 8
        || weight_count > 100_000
    {
        return None;
    }
    let weights = f64s_at(payload, weights_at + 12, weight_count)?;
    let points_at = weights_at + 12 + weight_count * 8;
    let point_count = usize::try_from(u32_at(payload, points_at)?).ok()?;
    if (weight_count != 0 && point_count != weight_count)
        || u32_at(payload, points_at + 4)? as usize != point_count
        || u32_at(payload, points_at + 8)? != 8
        || knot_count != point_count.checked_add(degree as usize + 1)?
    {
        return None;
    }
    let coordinates = f64s_at(payload, points_at + 12, point_count.checked_mul(3)?)?;
    if knots.windows(2).any(|pair| pair[0] > pair[1])
        || weights
            .iter()
            .any(|weight| !weight.is_finite() || *weight <= 0.0)
        || coordinates.iter().any(|value| !value.is_finite())
        || !fit_tolerance.is_finite()
    {
        return None;
    }
    let control_points = coordinates
        .chunks_exact(3)
        .map(|point| Point3::new(point[0] * 10.0, point[1] * 10.0, point[2] * 10.0))
        .collect();
    Some(SketchCurveGeometry::Nurbs {
        carrier_reference,
        subtype_class_tag,
        subtype_record_index: u32_at(payload, base + 15)?,
        degree,
        fit_tolerance: fit_tolerance * 10.0,
        scalar_width: 8,
        knots,
        weights,
        control_points,
    })
}

fn decode_line(payload: &[u8]) -> Option<SketchCurveGeometry> {
    let values = (0..12)
        .map(|ordinal| f64_at(payload, 133 + ordinal * 8))
        .collect::<Option<Vec<_>>>()?;
    if values.iter().any(|value| !value.is_finite()) {
        return None;
    }
    let displacement = Vector3::new(values[3], values[4], values[5]);
    let direction = Vector3::new(values[6], values[7], values[8]);
    let normal = Vector3::new(values[9], values[10], values[11]);
    let length = displacement.norm();
    if length <= 0.0 {
        return None;
    }
    let parallel_error = Vector3::new(
        displacement.x / length - direction.x,
        displacement.y / length - direction.y,
        displacement.z / length - direction.z,
    )
    .norm();
    let dot = direction.x * normal.x + direction.y * normal.y + direction.z * normal.z;
    if (direction.norm() - 1.0).abs() > 1.0e-9
        || (normal.norm() - 1.0).abs() > 1.0e-9
        || parallel_error > 1.0e-9
        || dot.abs() > 1.0e-9
    {
        return None;
    }
    let start = Point3::new(values[0] * 10.0, values[1] * 10.0, values[2] * 10.0);
    Some(SketchCurveGeometry::Line {
        start,
        end: Point3::new(
            start.x + displacement.x * 10.0,
            start.y + displacement.y * 10.0,
            start.z + displacement.z * 10.0,
        ),
        direction,
        normal,
    })
}

type ParsedSketchRelation = (
    Vec<u32>,
    Vec<usize>,
    Vec<u32>,
    Vec<usize>,
    u32,
    usize,
    u32,
    usize,
    Vec<u32>,
    Vec<usize>,
    usize,
);

fn parse_sketch_relation(
    payload: &[u8],
    owners: &std::collections::HashSet<u32>,
) -> Option<ParsedSketchRelation> {
    if payload.get(19) != Some(&1) {
        return None;
    }
    let member_count = usize::try_from(u32_at(payload, 20)?).ok()?;
    if member_count > 64 {
        return None;
    }
    let mut cursor = 24;
    let mut members = Vec::with_capacity(member_count);
    let mut member_offsets = Vec::with_capacity(member_count);
    for _ in 0..member_count {
        let (value, end) = marked_u32(payload, cursor)?;
        members.push(value);
        member_offsets.push(cursor + 1);
        cursor = next_reference_marker(payload, end)?;
    }
    let mut auxiliary_references = Vec::new();
    let mut auxiliary_reference_offsets = Vec::new();
    let (owner_reference, owner_reference_offset, end) = loop {
        let (reference, end) = marked_u32(payload, cursor)?;
        if owners.contains(&reference) {
            break (reference, cursor + 1, end);
        }
        auxiliary_references.push(reference);
        auxiliary_reference_offsets.push(cursor + 1);
        cursor = next_reference_marker(payload, end)?;
    };
    cursor = next_nonzero(payload, end)?;
    let state_offset = cursor + usize::from(payload.get(cursor) == Some(&1));
    let (state, end) = if payload.get(cursor) == Some(&1) {
        marked_u32(payload, cursor)?
    } else {
        (u32_at(payload, cursor)?, cursor + 4)
    };
    cursor = next_nonzero(payload, end)?;
    let return_count = usize::try_from(u32_at(payload, cursor)?).ok()?;
    if return_count > 64 {
        return None;
    }
    cursor += 4;
    let mut return_members = Vec::with_capacity(return_count);
    let mut return_member_offsets = Vec::with_capacity(return_count);
    for ordinal in 0..return_count {
        cursor = next_reference_marker(payload, cursor)?;
        let (value, end) = marked_u32(payload, cursor)?;
        return_members.push(value);
        return_member_offsets.push(cursor + 1);
        cursor = end;
        if ordinal + 1 < return_count {
            cursor = next_reference_marker(payload, cursor)?;
        }
    }
    let parsed_end = cursor;
    Some((
        members,
        member_offsets,
        auxiliary_references,
        auxiliary_reference_offsets,
        owner_reference,
        owner_reference_offset,
        state,
        state_offset,
        return_members,
        return_member_offsets,
        parsed_end,
    ))
}

fn next_indexed_record_offset(bytes: &[u8], mut position: usize) -> Option<usize> {
    while position + 11 <= bytes.len() {
        let Some((class_tag, after_tag)) = lp_ascii(bytes, position) else {
            position += 1;
            continue;
        };
        if class_tag.len() == 3
            && class_tag.bytes().all(|byte| byte.is_ascii_digit())
            && bytes.get(after_tag..after_tag + 4).is_some()
        {
            return Some(position);
        }
        position += 1;
    }
    None
}

fn marked_u32(bytes: &[u8], position: usize) -> Option<(u32, usize)> {
    (bytes.get(position) == Some(&1)).then_some((u32_at(bytes, position + 1)?, position + 5))
}

fn next_reference_marker(bytes: &[u8], mut position: usize) -> Option<usize> {
    while position + 5 <= bytes.len() {
        if bytes.get(position) == Some(&1) {
            let reference = u32_at(bytes, position + 1)?;
            if reference <= 10_000_000 {
                return Some(position);
            }
        }
        position += 1;
    }
    None
}

fn next_nonzero(bytes: &[u8], mut position: usize) -> Option<usize> {
    while bytes.get(position) == Some(&0) {
        position += 1;
    }
    (position + 4 <= bytes.len()).then_some(position)
}

struct SketchReferenceList {
    record_reference: u32,
    record_reference_offset: usize,
    declared_count: u32,
    references: Vec<u32>,
    reference_offsets: Vec<usize>,
    end: usize,
}

fn decode_reference_list(bytes: &[u8], position: usize) -> Option<SketchReferenceList> {
    let record_reference = u32::from_le_bytes(bytes.get(position..position + 4)?.try_into().ok()?);
    if bytes.get(position + 4..position + 8) != Some(&[0; 4]) || bytes.get(position + 8) != Some(&1)
    {
        return None;
    }
    let declared_count =
        u32::from_le_bytes(bytes.get(position + 9..position + 13)?.try_into().ok()?);
    let mut cursor = position + 13;
    let mut references = Vec::new();
    let mut reference_offsets = Vec::new();
    while bytes.get(cursor) == Some(&1) && bytes.get(cursor + 5..cursor + 11) == Some(&[0; 6]) {
        references.push(u32::from_le_bytes(
            bytes.get(cursor + 1..cursor + 5)?.try_into().ok()?,
        ));
        reference_offsets.push(cursor + 1);
        cursor += 11;
    }
    (references.len() == declared_count as usize).then_some(SketchReferenceList {
        record_reference,
        record_reference_offset: position,
        declared_count,
        references,
        reference_offsets,
        end: cursor,
    })
}

/// Decode the `BodiesRoot` member list following the doubled `BodiesRoot`
/// marker in each design `BulkStream` entry in `scan`: each member's entity
/// suffix and flags. The decode is rejected (no members returned for that
/// stream) unless the declared count is fully consumed and immediately
/// followed by a zero byte.
pub fn decode_body_members(
    _reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
) -> Result<Vec<DesignBodyMember>, CodecError> {
    let mut out = Vec::new();
    let mut prefix = Vec::new();
    prefix.extend_from_slice(&10u32.to_le_bytes());
    prefix.extend_from_slice(b"BodiesRoot");
    prefix.extend_from_slice(&0u16.to_le_bytes());
    prefix.extend_from_slice(&10u32.to_le_bytes());
    prefix.extend_from_slice(b"BodiesRoot");
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some(start) = bytes
            .windows(prefix.len())
            .position(|window| window == prefix)
        else {
            continue;
        };
        let count_offset = start + prefix.len();
        let Some(count_raw) = bytes.get(count_offset..count_offset + 4) else {
            continue;
        };
        let count =
            usize::try_from(u32::from_le_bytes(count_raw.try_into().expect(
                "invariant: count_raw is a 4-byte slice from bytes.get(range) of length 4",
            )))
            .unwrap_or(usize::MAX);
        if count > 100_000 {
            continue;
        }
        let mut cursor = count_offset + 4;
        let mut decoded = Vec::with_capacity(count);
        for _ in 0..count {
            if bytes.get(cursor) != Some(&1) {
                decoded.clear();
                break;
            }
            let Some(id_raw) = bytes.get(cursor + 1..cursor + 9) else {
                decoded.clear();
                break;
            };
            let Some(flags_raw) = bytes.get(cursor + 9..cursor + 11) else {
                decoded.clear();
                break;
            };
            decoded.push(DesignBodyMember {
                id: format!("f3d:{}:design-body-member#{cursor}", entry.name),
                byte_offset: cursor as u64,
                entity_suffix: u64::from_le_bytes(id_raw.try_into().expect(
                    "invariant: id_raw is an 8-byte slice from bytes.get(range) of length 8",
                )),
                flags: u16::from_le_bytes(flags_raw.try_into().expect(
                    "invariant: flags_raw is a 2-byte slice from bytes.get(range) of length 2",
                )),
            });
            cursor += 11;
        }
        if decoded.len() == count && bytes.get(cursor) == Some(&0) {
            out.extend(decoded);
        }
    }
    Ok(out)
}

fn object_kind(name: &str) -> Option<DesignObjectKind> {
    match name {
        "Fusion" => Some(DesignObjectKind::Fusion),
        "Body" => Some(DesignObjectKind::Body),
        "Component" => Some(DesignObjectKind::Component),
        "Geometry" => Some(DesignObjectKind::Geometry),
        "MSketch" => Some(DesignObjectKind::Sketch),
        "Dimension" => Some(DesignObjectKind::Dimension),
        "Scene" => Some(DesignObjectKind::Scene),
        "EntityTracking" => Some(DesignObjectKind::EntityTracking),
        "CommonData" => Some(DesignObjectKind::CommonData),
        _ => None,
    }
}

fn lp_ascii(bytes: &[u8], offset: usize) -> Option<(String, usize)> {
    let length = usize::try_from(u32_at(bytes, offset)?).ok()?;
    if length > 2_000 {
        return None;
    }
    let (raw, end) = lp_u32_bytes_at(bytes, offset)?;
    raw.iter()
        .all(u8::is_ascii_graphic)
        .then(|| (String::from_utf8_lossy(raw).into_owned(), end))
}

fn lp_utf16(bytes: &[u8], offset: usize) -> Option<(String, usize)> {
    let length = usize::try_from(u32_at(bytes, offset)?).ok()?;
    if !(1..=256).contains(&length) {
        return None;
    }
    utf16le_at(bytes, offset.checked_add(4)?, length)
}

fn is_guid(value: &str) -> bool {
    matches!(value.len(), 36..=38)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn decode_stream(bytes: &[u8], stream: &str, out: &mut Vec<ConstructionRecipe>) {
    let mut counters: HashMap<(ConstructionRecipeKind, Option<String>), u32> = HashMap::new();
    for &(name, kind) in RECIPES {
        let mut cursor = 0;
        while let Some(relative) = bytes[cursor..].windows(name.len()).position(|w| w == name) {
            let offset = cursor + relative;
            cursor = offset + 1;
            if kind == ConstructionRecipeKind::Face
                && offset >= 8
                && &bytes[offset - 8..offset] == b"bounded_"
            {
                continue;
            }
            let framed_name = offset
                .checked_sub(4)
                .and_then(|at| u32_at(bytes, at))
                .and_then(|length| usize::try_from(length).ok())
                == Some(name.len());
            if !framed_name {
                continue;
            }
            let design_id_field = recipe_design_id(bytes, offset, name);
            let design_id = design_id_field.as_ref().map(|field| field.0.clone());
            let key = (kind, design_id.clone());
            let counter = counters.entry(key).or_default();
            let recipe_index = *counter;
            *counter += 1;
            let record_index_offset = offset.checked_sub(16);
            let record_index = record_index_offset
                .and_then(|at| bytes.get(at..at + 4))
                .map(|raw| {
                    i32::from_le_bytes(
                        raw.try_into()
                            .expect("invariant: bytes.get(at..at+4) is a 4-byte slice"),
                    )
                })
                .unwrap_or_default();
            out.push(ConstructionRecipe {
                id: format!("f3d:{stream}:construction-recipe#{offset}"),
                byte_offset: offset as u64,
                record_index_offset: record_index_offset.map(|offset| offset as u64),
                kind,
                design_id,
                design_id_offset: design_id_field.as_ref().map(|field| field.1 as u64),
                design_id_binary_u32: design_id_field.is_some_and(|field| field.2),
                recipe_index,
                record_index,
            });
        }
    }
    out.sort_by_key(|recipe| recipe.record_index);
}

fn recipe_design_id(bytes: &[u8], offset: usize, name: &[u8]) -> Option<(String, usize, bool)> {
    let pre = offset.checked_sub(27)?;
    if let Some((id, value_offset)) = ascii_id_at(bytes, pre) {
        return Some((id, value_offset, false));
    }
    if offset >= 23 {
        let candidate = bytes.get(offset - 23..offset - 20)?;
        if candidate.iter().all(u8::is_ascii_digit) {
            return Some((
                String::from_utf8_lossy(candidate).into_owned(),
                offset - 23,
                false,
            ));
        }
    }
    if name == b"bounded_face_recipe_data" && offset >= 16 {
        let id = u32::from_le_bytes(bytes[offset - 16..offset - 12].try_into().ok()?);
        let zeros = bytes.get(offset - 12..offset - 4)?;
        if (100..100_000).contains(&id) && zeros.iter().all(|byte| *byte == 0) {
            return Some((id.to_string(), offset - 16, true));
        }
    }
    ascii_id_at(bytes, offset + name.len() + 8).map(|(id, value_offset)| (id, value_offset, false))
}

fn ascii_id_at(bytes: &[u8], length_offset: usize) -> Option<(String, usize)> {
    let length = usize::try_from(u32::from_le_bytes(
        bytes
            .get(length_offset..length_offset + 4)?
            .try_into()
            .ok()?,
    ))
    .ok()?;
    if !(1..=8).contains(&length) {
        return None;
    }
    let value = bytes.get(length_offset + 4..length_offset + 4 + length)?;
    value.iter().all(u8::is_ascii_alphanumeric).then(|| {
        (
            String::from_utf8_lossy(value).into_owned(),
            length_offset + 4,
        )
    })
}

/// One `(asm_body_key, entity_suffix)` pair from a Design `BulkStream` BREP
/// body-map record, with the named B-rep blob the key resolves in and the
/// suffix's byte offset for native patching.
pub(crate) struct BodyBinding {
    /// Basename of the B-rep blob entry the ASM key resolves in.
    pub blob_name: String,
    /// The referenced ASM body key.
    pub asm_key: u64,
    /// Byte offset of `asm_key` within the stream.
    pub asm_key_offset: usize,
    /// The body's design-entity suffix.
    pub entity_suffix: u64,
    /// Byte offset of `entity_suffix` within the stream.
    pub entity_suffix_offset: usize,
}

/// Parse every BREP body-map record in a Design `BulkStream`: a `u32` pair
/// count, `count` pairs of `(u64 asm_body_key, u64 entity_suffix)`, the
/// trailing record ref and pad, then the length-prefixed UTF-16 blob name
/// ([spec §8.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#81-design-metadata)).
pub(crate) fn body_bindings(bytes: &[u8]) -> Vec<BodyBinding> {
    let needle: Vec<u8> = "BREP.".encode_utf16().flat_map(u16::to_le_bytes).collect();
    let mut out = Vec::new();
    for offset in bytes
        .windows(needle.len())
        .enumerate()
        .filter_map(|(offset, window)| (window == needle).then_some(offset))
    {
        let Some(name_chars) = offset
            .checked_sub(4)
            .and_then(|at| read_u32(bytes, at))
            .map(|chars| chars as usize)
        else {
            continue;
        };
        let Some(blob_name) = bytes
            .get(offset..offset + name_chars * 2)
            .map(utf16_le_string)
        else {
            continue;
        };
        // 16 bytes separate the pairs from the name: the 12-byte record tail
        // and the name's u32 length prefix.
        let Some(pairs_end) = offset.checked_sub(16) else {
            continue;
        };
        // The pair count precedes the pairs; scanning ascending is unambiguous
        // because the high halves of the little-endian ids are zero.
        for count in 1usize..=64 {
            let span = 16 * count;
            let Some(count_at) = pairs_end.checked_sub(span + 4) else {
                break;
            };
            if read_u32(bytes, count_at) != Some(count as u32) {
                continue;
            }
            for pair in 0..count {
                let at = count_at + 4 + pair * 16;
                if let (Some(key), Some(suffix)) = (read_u64(bytes, at), read_u64(bytes, at + 8)) {
                    out.push(BodyBinding {
                        blob_name: blob_name.clone(),
                        asm_key: key,
                        asm_key_offset: at,
                        entity_suffix: suffix,
                        entity_suffix_offset: at + 8,
                    });
                }
            }
            break;
        }
    }
    out
}

/// Decode per-body display visibility from the Design `BulkStream`.
///
/// The BREP body-map record resolves ASM body keys of `active_brep_entry` to
/// design-entity suffixes, and each entity's browser-node record carries a
/// hidden flag directly after the node GUID
/// ([spec §8.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#81-design-metadata)).
/// The result maps each ASM body key to its display visibility; bodies
/// without records are absent.
#[derive(Debug, Clone)]
pub(crate) struct DecodedBodyVisibility {
    pub stream: String,
    pub byte_offset: u64,
    pub asm_body_key_offset: u64,
    pub entity_suffix: u64,
    pub visible: bool,
}

pub(crate) fn decode_body_visibility(
    _reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
    active_brep_entry: &str,
) -> Result<HashMap<u64, DecodedBodyVisibility>, CodecError> {
    let Some(basename) = active_brep_entry
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
    else {
        return Ok(HashMap::new());
    };
    let mut out = HashMap::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let hidden_by_entity = browser_node_hidden_flags(bytes);
        for binding in body_bindings(bytes) {
            if binding.blob_name != basename {
                continue;
            }
            if let Some(node) = hidden_by_entity.get(&binding.entity_suffix) {
                out.insert(
                    binding.asm_key,
                    DecodedBodyVisibility {
                        stream: entry.name.clone(),
                        byte_offset: node.byte_offset,
                        asm_body_key_offset: binding.asm_key_offset as u64,
                        entity_suffix: binding.entity_suffix,
                        visible: !node.hidden,
                    },
                );
            }
        }
    }
    Ok(out)
}

/// Scan for browser-node records: a length-prefixed 36-character UTF-16 GUID,
/// one hidden-flag byte, the `01 01` marker, and the `u64` design-entity
/// suffix.
#[derive(Debug, Clone, Copy)]
struct BrowserNodeVisibility {
    byte_offset: u64,
    hidden: bool,
}

fn browser_node_hidden_flags(bytes: &[u8]) -> HashMap<u64, BrowserNodeVisibility> {
    const GUID_CHARS: usize = 36;
    const GUID_BYTES: usize = GUID_CHARS * 2;
    let mut out = HashMap::new();
    let mut at = 0usize;
    while at + 4 + GUID_BYTES + 3 + 8 <= bytes.len() {
        if read_u32(bytes, at) != Some(GUID_CHARS as u32)
            || !is_utf16_guid(&bytes[at + 4..at + 4 + GUID_BYTES])
        {
            at += 1;
            continue;
        }
        let flag_at = at + 4 + GUID_BYTES;
        if bytes.get(flag_at + 1..flag_at + 3) == Some(&[0x01, 0x01]) {
            if let (flag @ (0 | 1), Some(member)) = (bytes[flag_at], read_u64(bytes, flag_at + 3)) {
                out.insert(
                    member,
                    BrowserNodeVisibility {
                        byte_offset: flag_at as u64,
                        hidden: flag == 1,
                    },
                );
            }
        }
        at += 1;
    }
    out
}

fn utf16_le_string(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    String::from_utf16_lossy(&units)
}

fn is_utf16_guid(bytes: &[u8]) -> bool {
    bytes
        .chunks_exact(2)
        .all(|pair| pair[1] == 0 && (pair[0].is_ascii_hexdigit() || pair[0] == b'-'))
}

#[cfg(test)]
mod relation_tests {
    use super::{
        next_indexed_record_offset, parse_design_parameter, parse_parameter_owner,
        parse_sketch_relation, project_user_parameters,
    };
    use crate::records::DesignParameterKind;
    use cadmpeg_ir::features::{Length, ParameterValue};
    use std::collections::HashSet;

    fn lp_utf16(out: &mut Vec<u8>, value: &str) {
        let units = value.encode_utf16().collect::<Vec<_>>();
        out.extend_from_slice(&(units.len() as u32).to_le_bytes());
        for unit in units {
            out.extend_from_slice(&unit.to_le_bytes());
        }
    }

    fn parameter_record(
        owner: Option<u32>,
        expression: &str,
        source_kind: &str,
        unit: Option<&str>,
        name: &str,
        evaluated_value: f64,
    ) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&3u32.to_le_bytes());
        out.extend_from_slice(b"305");
        out.extend_from_slice(&71u32.to_le_bytes());
        out.extend_from_slice(&[0; 20]);
        out.extend_from_slice(&9u32.to_le_bytes());
        match owner {
            Some(owner) => {
                out.push(1);
                out.extend_from_slice(&owner.to_le_bytes());
                out.extend_from_slice(&[0; 6]);
            }
            None => out.push(0),
        }
        lp_utf16(&mut out, expression);
        out.extend_from_slice(if owner.is_some() {
            &[0; 9]
        } else {
            &[0, 0, 0, 0, 0, 0, 0, 0, 1]
        });
        lp_utf16(&mut out, source_kind);
        out.extend_from_slice(&0u32.to_le_bytes());
        if let Some(unit) = unit {
            lp_utf16(&mut out, unit);
        }
        lp_utf16(&mut out, name);
        out.extend_from_slice(&evaluated_value.to_le_bytes());
        out.extend_from_slice(&[0, 1, 19, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        out
    }

    #[test]
    fn parameter_variants_have_exact_string_and_scalar_boundaries() {
        let user = parse_design_parameter(&parameter_record(
            None,
            "60 mm",
            "User Parameter",
            Some("mm"),
            "Width",
            6.0,
        ))
        .unwrap();
        assert_eq!(user.kind, DesignParameterKind::User);
        assert_eq!(user.owner_record_index, None);
        assert_eq!(user.unit.as_deref(), Some("mm"));
        assert_eq!(user.evaluated_value, 6.0);

        let feature = parse_design_parameter(&parameter_record(
            Some(44),
            "Width / 2",
            "AlongDistance",
            Some("mm"),
            "d12",
            3.0,
        ))
        .unwrap();
        assert_eq!(feature.kind, DesignParameterKind::Feature);
        assert_eq!(feature.owner_record_index, Some(44));
        assert_eq!(feature.expression, "Width / 2");

        let boolean = parse_design_parameter(&parameter_record(
            None,
            "1",
            "User Parameter",
            None,
            "OnOff",
            1.0,
        ))
        .unwrap();
        assert_eq!(boolean.unit, None);
        assert_eq!(boolean.name, "OnOff");
    }

    #[test]
    fn parameter_record_rejects_noncanonical_tail() {
        let mut record = parameter_record(
            Some(44),
            "45 deg",
            "TaperAngle",
            Some("deg"),
            "d13",
            std::f64::consts::FRAC_PI_4,
        );
        *record.last_mut().unwrap() = 1;
        assert!(parse_design_parameter(&record).is_none());
    }

    fn parameter_owner_frame() -> Vec<u8> {
        let mut frame = vec![0; 104];
        frame[0..4].copy_from_slice(&3u32.to_le_bytes());
        frame[4..7].copy_from_slice(b"292");
        frame[7..11].copy_from_slice(&44u32.to_le_bytes());
        frame[19] = 1;
        frame[20..24].copy_from_slice(&1u32.to_le_bytes());
        frame[24] = 1;
        frame[25..29].copy_from_slice(&12u32.to_le_bytes());
        frame[35..39].copy_from_slice(&2u32.to_le_bytes());
        frame[40..48].copy_from_slice(&6.0f64.to_le_bytes());
        frame[48] = 1;
        frame[49..53].copy_from_slice(&45u32.to_le_bytes());
        frame[59..63].copy_from_slice(&9u32.to_le_bytes());
        frame[67] = 1;
        frame[68..72].copy_from_slice(&12u32.to_le_bytes());
        frame[78] = 1;
        frame[79] = 1;
        frame[81] = 1;
        frame[82..86].copy_from_slice(&46u32.to_le_bytes());
        frame[93] = 1;
        frame[94..98].copy_from_slice(&12u32.to_le_bytes());
        frame
    }

    #[test]
    fn parameter_owner_frame_has_repeated_scope_and_consecutive_records() {
        let parsed = parse_parameter_owner(&parameter_owner_frame()).unwrap();
        assert_eq!(parsed.record_index, 44);
        assert_eq!(parsed.scope_record_index, 12);
        assert_eq!(parsed.local_ordinal, 2);
        assert_eq!(parsed.evaluated_value, 6.0);
        assert_eq!(parsed.parameter_record_index, 45);
        assert_eq!(parsed.owned_ordinal, 9);
        assert_eq!(parsed.variant, 1);
        assert_eq!(parsed.companion_record_index, 46);

        let mut malformed = parameter_owner_frame();
        malformed[94..98].copy_from_slice(&13u32.to_le_bytes());
        assert!(parse_parameter_owner(&malformed).is_none());
    }

    #[test]
    fn user_parameters_project_in_source_order_with_units_and_dependencies() {
        let mut width = parse_design_parameter(&parameter_record(
            None,
            "60 mm",
            "User Parameter",
            Some("mm"),
            "Width",
            6.0,
        ))
        .unwrap();
        width.id = "f3d:native:parameter#width".into();
        width.record_index = 20;
        width.source_ordinal = 4;
        let mut half = parse_design_parameter(&parameter_record(
            None,
            "Width / 2",
            "User Parameter",
            Some("mm"),
            "HalfWidth",
            3.0,
        ))
        .unwrap();
        half.id = "f3d:native:parameter#half".into();
        half.record_index = 21;
        half.source_ordinal = 5;

        let projected = project_user_parameters(&[half, width]);
        assert_eq!(projected[0].name, "Width");
        assert_eq!(projected[0].owner, None);
        assert_eq!(
            projected[0].value,
            Some(ParameterValue::Length(Length(60.0)))
        );
        assert_eq!(projected[1].dependencies, [projected[0].id.clone()]);
        assert_eq!(
            projected[1].native_ref.as_deref(),
            Some("f3d:native:parameter#half")
        );
    }

    #[test]
    fn variable_width_relation_uses_counted_runs_and_next_record_boundary() {
        let mut record = vec![0u8; 127];
        record[0..4].copy_from_slice(&3u32.to_le_bytes());
        record[4..7].copy_from_slice(b"286");
        record[7..11].copy_from_slice(&1239u32.to_le_bytes());
        record[19] = 1;
        record[20..24].copy_from_slice(&3u32.to_le_bytes());
        for (marker, reference) in [(24, 1224u32), (39, 1228), (54, 1236), (65, 0), (70, 1041)] {
            record[marker] = 1;
            record[marker + 1..marker + 5].copy_from_slice(&reference.to_le_bytes());
        }
        record[82..86].copy_from_slice(&4u32.to_le_bytes());
        record[89..93].copy_from_slice(&3u32.to_le_bytes());
        for (marker, reference) in [(93, 1224u32), (104, 1228), (115, 1236)] {
            record[marker] = 1;
            record[marker + 1..marker + 5].copy_from_slice(&reference.to_le_bytes());
        }
        let mut bytes = record.clone();
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"277");
        bytes.extend_from_slice(&1240u32.to_le_bytes());

        assert_eq!(next_indexed_record_offset(&bytes, 11), Some(127));
        let parsed = parse_sketch_relation(&record, &HashSet::from([1041])).unwrap();
        assert_eq!(parsed.0, [1224, 1228, 1236]);
        assert_eq!(parsed.2, [0]);
        assert_eq!(parsed.4, 1041);
        assert_eq!(parsed.6, 4);
        assert_eq!(parsed.8, [1224, 1228, 1236]);
        assert_eq!(parsed.10, 120);
    }
}
