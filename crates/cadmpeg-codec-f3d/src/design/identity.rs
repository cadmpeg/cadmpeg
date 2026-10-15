// SPDX-License-Identifier: Apache-2.0
//! Construct input-derived identities.

use cadmpeg_core::CodecError;
use std::fmt::{self, Write};

struct Encoded<'a>(&'a str, bool);

impl fmt::Display for Encoded<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        for character in self.0.chars() {
            let character = if self.1 {
                character.to_ascii_lowercase()
            } else {
                character
            };
            let mut bytes = [0u8; 4];
            let text = character.encode_utf8(&mut bytes);
            if matches!(character, ':' | '#' | '%') || character.is_whitespace() {
                for byte in text.as_bytes() {
                    let escaped = [
                        b'%',
                        HEX[usize::from(byte >> 4)],
                        HEX[usize::from(byte & 0x0f)],
                    ];
                    let escaped = std::str::from_utf8(&escaped).map_err(|_| fmt::Error)?;
                    formatter.write_str(escaped)?;
                }
            } else {
                formatter.write_str(text)?;
            }
        }
        Ok(())
    }
}

struct Length(usize);

impl fmt::Write for Length {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        self.0 = self.0.checked_add(text.len()).ok_or(fmt::Error)?;
        Ok(())
    }
}

fn encoded_length(value: &str, operation: &'static str) -> Result<usize, CodecError> {
    let mut length = Length(0);
    write!(&mut length, "{}", Encoded(value, false))
        .map_err(|_| CodecError::malformed(format_args!("{operation} length overflows")))?;
    Ok(length.0)
}

pub(super) fn neutral_configuration_id(
    entry: &str,
    name: &str,
) -> Result<cadmpeg_ir::features::ConfigurationId, CodecError> {
    let operation = "f3d configuration identifier";
    let entry_len = encoded_length(entry, operation)?;
    let name_len = encoded_length(name, operation)?;
    let text = format!(
        "f3d:configuration:variant#{entry_len}:{}{name_len}:{}",
        Encoded(entry, false),
        Encoded(name, false)
    );
    cadmpeg_ir::features::ConfigurationId::mint(text)
        .map_err(|error| crate::design::text::malformed_design(format_args!("{error}")))
}

fn mint<T: TryFrom<String>>(text: String) -> Result<T, CodecError>
where
    T::Error: fmt::Display,
{
    T::try_from(text)
        .map_err(|error| crate::design::text::malformed_design(format_args!("{error}")))
}

pub(super) fn neutral_feature_id(
    scope: &crate::records::feature::scope::DesignParameterScope,
) -> Result<cadmpeg_ir::features::FeatureId, CodecError> {
    let operation = "f3d feature identifier";
    let stream = crate::ids::native_stream(&scope.id).unwrap_or(crate::ids::DEFAULT_STREAM);
    let kind = scope.kind_name();
    let stream_len = encoded_length(stream, operation)?;
    let kind_len = encoded_length(kind, operation)?;
    mint(format!(
        "f3d:model:feature#{stream_len}:{}{kind_len}:{}{}:{}",
        Encoded(stream, false),
        Encoded(kind, false),
        scope.feature_ordinal.get(),
        scope.record_index
    ))
}

pub(super) fn neutral_parameter_id(
    parameter: &crate::records::parameters::DesignParameter,
) -> Result<cadmpeg_ir::features::ParameterId, CodecError> {
    let operation = "f3d parameter identifier";
    let stream = crate::ids::native_stream(&parameter.id).unwrap_or(crate::ids::DEFAULT_STREAM);
    let len = encoded_length(stream, operation)?;
    mint(format!(
        "f3d:model:parameter#{len}:{}{}",
        Encoded(stream, false),
        parameter.record_index
    ))
}

pub(super) fn neutral_sketch_id(
    placement: &crate::records::sketch_placement::DesignSketchPlacement,
) -> Result<cadmpeg_ir::sketches::SketchId, CodecError> {
    let stream = crate::ids::native_stream(&placement.id).unwrap_or(crate::ids::DEFAULT_STREAM);
    mint(format!(
        "f3d:model:sketch#{}@{}",
        Encoded(stream, false),
        placement.entity_id.suffix()
    ))
}

pub(super) fn neutral_spatial_sketch_id(
    placement: &crate::records::sketch_placement::DesignSketchPlacement,
) -> Result<cadmpeg_ir::sketches::SpatialSketchId, CodecError> {
    let stream = crate::ids::native_stream(&placement.id).unwrap_or(crate::ids::DEFAULT_STREAM);
    mint(format!(
        "f3d:model:spatial-sketch#{}@{}",
        Encoded(stream, false),
        placement.entity_id.suffix()
    ))
}

macro_rules! tagged_entity_id {
    ($name:ident, $owner:ty, $result:ty, $namespace:literal, $tag:literal, $index:ty, $operation:literal) => {
        pub(super) fn $name(sketch: &$owner, index: $index) -> Result<$result, CodecError> {
            let len = encoded_length(sketch.as_str(), $operation)?;
            mint(format!(
                concat!($namespace, "#{}:{}", $tag, "{}"),
                len,
                Encoded(sketch.as_str(), false),
                index
            ))
        }
    };
}

tagged_entity_id!(
    neutral_sketch_point_id,
    cadmpeg_ir::sketches::SketchId,
    cadmpeg_ir::sketches::SketchEntityId,
    "f3d:model:sketch-entity",
    "p",
    u64,
    "f3d sketch point identifier"
);
tagged_entity_id!(
    neutral_sketch_text_id,
    cadmpeg_ir::sketches::SketchId,
    cadmpeg_ir::sketches::SketchEntityId,
    "f3d:model:sketch-entity",
    "t",
    u64,
    "f3d sketch text identifier"
);
tagged_entity_id!(
    neutral_sketch_record_id,
    cadmpeg_ir::sketches::SketchId,
    cadmpeg_ir::sketches::SketchEntityId,
    "f3d:model:sketch-entity",
    "x",
    u32,
    "f3d sketch record identifier"
);
tagged_entity_id!(
    neutral_spatial_sketch_point_id,
    cadmpeg_ir::sketches::SpatialSketchId,
    cadmpeg_ir::sketches::SpatialSketchEntityId,
    "f3d:model:spatial-sketch-entity",
    "p",
    u64,
    "f3d spatial sketch point identifier"
);
tagged_entity_id!(
    neutral_spatial_sketch_record_id,
    cadmpeg_ir::sketches::SpatialSketchId,
    cadmpeg_ir::sketches::SpatialSketchEntityId,
    "f3d:model:spatial-sketch-entity",
    "x",
    u32,
    "f3d spatial sketch record identifier"
);
tagged_entity_id!(
    neutral_spatial_sketch_surface_id,
    cadmpeg_ir::sketches::SpatialSketchId,
    cadmpeg_ir::sketches::SpatialSketchEntityId,
    "f3d:model:spatial-sketch-entity",
    "s",
    u64,
    "f3d spatial sketch surface identifier"
);

pub(super) fn neutral_sketch_curve_id(
    sketch: &cadmpeg_ir::sketches::SketchId,
    primary: u64,
    secondary: u64,
) -> Result<cadmpeg_ir::sketches::SketchEntityId, CodecError> {
    let operation = "f3d sketch curve identifier";
    let len = encoded_length(sketch.as_str(), operation)?;
    mint(format!(
        "f3d:model:sketch-entity#{len}:{}c{primary}:{secondary}",
        Encoded(sketch.as_str(), false)
    ))
}

pub(super) fn neutral_spatial_sketch_curve_id(
    sketch: &cadmpeg_ir::sketches::SpatialSketchId,
    primary: u64,
    secondary: u64,
) -> Result<cadmpeg_ir::sketches::SpatialSketchEntityId, CodecError> {
    let operation = "f3d spatial sketch curve identifier";
    let len = encoded_length(sketch.as_str(), operation)?;
    mint(format!(
        "f3d:model:spatial-sketch-entity#{len}:{}c{primary}:{secondary}",
        Encoded(sketch.as_str(), false)
    ))
}

pub(super) fn neutral_sketch_constraint_id(
    native_ref: &str,
    record: u32,
) -> Result<cadmpeg_ir::sketches::SketchConstraintId, CodecError> {
    let stream = crate::ids::native_stream(native_ref).unwrap_or(crate::ids::DEFAULT_STREAM);
    mint(format!(
        "f3d:model:sketch-constraint#{}@{record}",
        Encoded(stream, false)
    ))
}

pub(super) fn neutral_dimension_constraint_id(
    parameter: &cadmpeg_ir::features::ParameterId,
    form: &str,
) -> Result<cadmpeg_ir::sketches::SketchConstraintId, CodecError> {
    let operation = "f3d dimension constraint identifier";
    let key = identity_key(parameter.as_str())?;
    let form_len = encoded_length(form, operation)?;
    mint(format!(
        "f3d:model:sketch-constraint#dimension:{}:{key}{form_len}:{}",
        key.len(),
        Encoded(form, false)
    ))
}

pub(super) fn identity_key(id: &str) -> Result<&str, CodecError> {
    id.split_once('#')
        .map(|(_, key)| key)
        .ok_or_else(|| CodecError::malformed("validated identity has no key"))
}

pub(super) fn neutral_component_insert_occurrence_id(
    scope: &crate::records::feature::scope::DesignParameterScope,
) -> Result<cadmpeg_ir::ids::OccurrenceId, CodecError> {
    let operation = "f3d component insert occurrence identifier";
    let stream = crate::ids::native_stream(&scope.id).unwrap_or(crate::ids::DEFAULT_STREAM);
    let len = encoded_length(stream, operation)?;
    mint(format!(
        "f3d:model:occurrence#component-insert-{len}:{}{}:{}",
        Encoded(stream, false),
        scope.feature_ordinal.get(),
        scope.record_index
    ))
}

pub(super) fn neutral_assembly_joint_id(
    scope: &crate::records::feature::scope::DesignParameterScope,
) -> Result<cadmpeg_ir::products::JointId, CodecError> {
    let operation = "f3d assembly joint identifier";
    let stream = crate::ids::native_stream(&scope.id).unwrap_or(crate::ids::DEFAULT_STREAM);
    let len = encoded_length(stream, operation)?;
    mint(format!(
        "f3d:model:joint#{len}:{}{}",
        Encoded(stream, false),
        scope.record_index
    ))
}

pub(super) fn configuration_entry_id(entry: &str) -> String {
    format!("f3d:configuration:entry#{}", Encoded(entry, false))
}

pub(super) fn history_input_prefix(
    key: &str,
    previous: i64,
) -> Result<cadmpeg_ir::ids::IdentityKey, CodecError> {
    let text = format!("{}:{key}:{previous}", key.len());
    cadmpeg_ir::ids::IdentityKey::try_new(text)
        .map_err(|error| crate::design::text::malformed_design(format_args!("{error}")))
}

pub(super) fn feature_input_topology_id(
    feature: &cadmpeg_ir::features::FeatureId,
    previous: i64,
) -> Result<cadmpeg_ir::ids::FeatureInputTopologyId, CodecError> {
    let key = identity_key(feature.as_str())?;
    mint(format!(
        "f3d:history-input:state#{}:{key}:{previous}",
        key.len()
    ))
}

pub(super) fn history_input_edge_id(
    prefix: &cadmpeg_ir::ids::IdentityKey,
    slot: i64,
) -> Result<cadmpeg_ir::ids::HistoricalEdgeId, CodecError> {
    mint(format!("f3d:history-input:edge#{}:{slot}", prefix.as_str()))
}

pub(super) fn history_input_face_id(
    prefix: &cadmpeg_ir::ids::IdentityKey,
    slot: i64,
) -> Result<cadmpeg_ir::ids::HistoricalFaceId, CodecError> {
    mint(format!("f3d:history-input:face#{}:{slot}", prefix.as_str()))
}

pub(super) fn history_input_vertex_id(
    prefix: &cadmpeg_ir::ids::IdentityKey,
    slot: i64,
) -> Result<cadmpeg_ir::ids::HistoricalVertexId, CodecError> {
    mint(format!(
        "f3d:history-input:vertex#{}:{slot}",
        prefix.as_str()
    ))
}

pub(super) fn neutral_assembly_axial_object_id(
    identity: &crate::records::feature::assembly::DesignAssemblyAxialSelectorIdentity,
) -> Result<String, CodecError> {
    let operation = "f3d assembly axial connector identifier";
    let asset = identity.selector_asset_id.as_str();
    let context = identity.selector_context_id.as_str();
    let external = identity.external_asset_id.as_str();
    let link = identity.external_link_name.as_str();
    let property = identity
        .external_version
        .as_ref()
        .map_or("", |version| version.property_key.value.as_str());
    let version = identity
        .external_version
        .as_ref()
        .map_or("", |version| version.version_urn.value.as_str());
    let asset_len = encoded_length(asset, operation)?;
    let context_len = encoded_length(context, operation)?;
    let external_len = encoded_length(external, operation)?;
    let link_len = encoded_length(link, operation)?;
    let property_len = encoded_length(property, operation)?;
    let version_len = encoded_length(version, operation)?;
    let present = u8::from(identity.external_version.is_some());
    Ok(format!("f3d:feature-input:connector#assembly-axial:{asset_len}:{}:{context_len}:{}:{}:{}:{external_len}:{}:{link_len}:{}:{present}:{property_len}:{}:{present}:{version_len}:{}", Encoded(asset, true), Encoded(context, true), identity.external_object_reference, identity.external_segment, Encoded(external, true), Encoded(link, false), Encoded(property, true), Encoded(version, false)))
}

pub(super) fn neutral_assembly_legacy_object_id(
    selection: &crate::records::feature::assembly::DesignAssemblyLegacySelection,
) -> Result<String, CodecError> {
    let operation = "f3d assembly legacy connector identifier";
    let asset = selection.asset_id.as_str();
    let context = selection.context_id.as_str();
    let recipe = selection.recipe_id.as_str();
    let asset_len = encoded_length(asset, operation)?;
    let context_len = encoded_length(context, operation)?;
    let recipe_len = encoded_length(recipe, operation)?;
    Ok(format!("f3d:feature-input:connector#assembly-legacy:{asset_len}:{}:{context_len}:{}:{recipe_len}:{}:{}:{}", Encoded(asset, true), Encoded(context, true), Encoded(recipe, true), selection.record_index, selection.recipe_record_index))
}

#[cfg(test)]
mod tests;
