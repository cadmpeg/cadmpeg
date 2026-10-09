// SPDX-License-Identifier: Apache-2.0
//! Construct input-derived identities under the caller decode budget.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use std::fmt;

struct EncodedText<'ctx> {
    text: String,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

fn encode_component<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    value: &str,
    lowercase_ascii: bool,
    operation: &'static str,
) -> Result<EncodedText<'ctx>, CodecError> {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut storage = ctx.reserve_scoped(0, operation)?;
    let mut text = String::new();
    let mut characters = value.chars();
    while !characters.as_str().is_empty() {
        if let Some(character) = ctx.next_charged(&mut characters, operation)? {
            let character = if lowercase_ascii {
                character.to_ascii_lowercase()
            } else {
                character
            };
            let escaped = matches!(character, ':' | '#' | '%') || character.is_whitespace();
            let width = character.len_utf8() * if escaped { 3 } else { 1 };
            if text.capacity() - text.len() < width {
                // Reserve actual geometric growth for this reached scalar.
                // Only this text backing belongs to the reservation.
                let growth = text.capacity().max(width);
                ctx.reserve_scoped_string(&mut storage, &mut text, growth, operation)?;
            }
            if escaped {
                let mut bytes = [0u8; 4];
                let encoded = character.encode_utf8(&mut bytes);
                for byte in encoded.as_bytes() {
                    for escaped in [
                        '%',
                        char::from(HEX[usize::from(byte >> 4)]),
                        char::from(HEX[usize::from(byte & 0x0f)]),
                    ] {
                        storage.with_storage(|| {
                            ctx.push_retained_char(&mut text, escaped, operation)
                        })?;
                    }
                }
            } else {
                storage.with_storage(|| {
                    ctx.push_retained_char(&mut text, character, operation)
                })?;
            }
        }
    }
    Ok(EncodedText { text, _storage: storage })
}

pub(super) fn neutral_configuration_id(
    ctx: &DecodeContext<'_>,
    entry: &str,
    name: &str,
) -> Result<cadmpeg_ir::features::ConfigurationId, CodecError> {
    let operation = "f3d configuration identifier";
    let encoded_entry = encode_component(ctx, entry, false, operation)?;
    let entry_len = encoded_entry.text.len();
    let encoded_name = encode_component(ctx, name, false, operation)?;
    let name_len = encoded_name.text.len();
    let text = ctx.format_retained(
        format_args!(
            "f3d:configuration:variant#{entry_len}:{}{name_len}:{}",
            encoded_entry.text,
            encoded_name.text
        ),
        operation,
    )?;
    cadmpeg_ir::features::ConfigurationId::mint(text)
        .map_err(|error| crate::design::text::malformed_design(ctx, format_args!("{error}")))
}

fn mint<T: TryFrom<String>>(ctx: &DecodeContext<'_>, text: String) -> Result<T, CodecError>
where
    T::Error: fmt::Display,
{
    T::try_from(text)
        .map_err(|error| crate::design::text::malformed_design(ctx, format_args!("{error}")))
}

pub(super) fn neutral_feature_id(
    ctx: &DecodeContext<'_>,
    scope: &crate::records::feature::scope::DesignParameterScope,
) -> Result<cadmpeg_ir::features::FeatureId, CodecError> {
    let operation = "f3d feature identifier";
    let stream = crate::ids::native_stream(&scope.id).unwrap_or(crate::ids::DEFAULT_STREAM);
    let kind = scope.kind_name();
    let encoded_stream = encode_component(ctx, stream, false, operation)?;
    let stream_len = encoded_stream.text.len();
    let encoded_kind = encode_component(ctx, kind, false, operation)?;
    let kind_len = encoded_kind.text.len();
    mint(
        ctx,
        ctx.format_retained(
            format_args!(
                "f3d:model:feature#{stream_len}:{}{kind_len}:{}{}:{}",
                encoded_stream.text,
                encoded_kind.text,
                scope.feature_ordinal.get(),
                scope.record_index
            ),
            operation,
        )?,
    )
}

pub(super) fn neutral_parameter_id(
    ctx: &DecodeContext<'_>,
    parameter: &crate::records::parameters::DesignParameter,
) -> Result<cadmpeg_ir::features::ParameterId, CodecError> {
    let operation = "f3d parameter identifier";
    let stream = crate::ids::native_stream(&parameter.id).unwrap_or(crate::ids::DEFAULT_STREAM);
    let encoded_stream = encode_component(ctx, stream, false, operation)?;
    let len = encoded_stream.text.len();
    mint(
        ctx,
        ctx.format_retained(
            format_args!(
                "f3d:model:parameter#{len}:{}{}",
                encoded_stream.text,
                parameter.record_index
            ),
            operation,
        )?,
    )
}

pub(super) fn neutral_sketch_id(
    ctx: &DecodeContext<'_>,
    placement: &crate::records::sketch_placement::DesignSketchPlacement,
) -> Result<cadmpeg_ir::sketches::SketchId, CodecError> {
    let stream = crate::ids::native_stream(&placement.id).unwrap_or(crate::ids::DEFAULT_STREAM);
    let encoded_stream = encode_component(ctx, stream, false, "f3d sketch identifier")?;
    mint(
        ctx,
        ctx.format_retained(
            format_args!(
                "f3d:model:sketch#{}@{}",
                encoded_stream.text,
                placement.entity_id.suffix()
            ),
            "f3d sketch identifier",
        )?,
    )
}

pub(super) fn neutral_spatial_sketch_id(
    ctx: &DecodeContext<'_>,
    placement: &crate::records::sketch_placement::DesignSketchPlacement,
) -> Result<cadmpeg_ir::sketches::SpatialSketchId, CodecError> {
    let stream = crate::ids::native_stream(&placement.id).unwrap_or(crate::ids::DEFAULT_STREAM);
    let encoded_stream = encode_component(ctx, stream, false, "f3d spatial sketch identifier")?;
    mint(
        ctx,
        ctx.format_retained(
            format_args!(
                "f3d:model:spatial-sketch#{}@{}",
                encoded_stream.text,
                placement.entity_id.suffix()
            ),
            "f3d spatial sketch identifier",
        )?,
    )
}

macro_rules! tagged_entity_id {
    ($name:ident, $owner:ty, $result:ty, $namespace:literal, $tag:literal, $index:ty, $operation:literal) => {
        pub(super) fn $name(
            ctx: &DecodeContext<'_>,
            sketch: &$owner,
            index: $index,
        ) -> Result<$result, CodecError> {
            let encoded_sketch = encode_component(ctx, sketch.as_str(), false, $operation)?;
            let len = encoded_sketch.text.len();
            mint(
                ctx,
                ctx.format_retained(
                    format_args!(
                        concat!($namespace, "#{}:{}", $tag, "{}"),
                        len,
                        encoded_sketch.text,
                        index
                    ),
                    $operation,
                )?,
            )
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
    ctx: &DecodeContext<'_>,
    sketch: &cadmpeg_ir::sketches::SketchId,
    primary: u64,
    secondary: u64,
) -> Result<cadmpeg_ir::sketches::SketchEntityId, CodecError> {
    let operation = "f3d sketch curve identifier";
    let encoded_sketch = encode_component(ctx, sketch.as_str(), false, operation)?;
    let len = encoded_sketch.text.len();
    mint(
        ctx,
        ctx.format_retained(
            format_args!(
                "f3d:model:sketch-entity#{len}:{}c{primary}:{secondary}",
                encoded_sketch.text
            ),
            operation,
        )?,
    )
}

pub(super) fn neutral_spatial_sketch_curve_id(
    ctx: &DecodeContext<'_>,
    sketch: &cadmpeg_ir::sketches::SpatialSketchId,
    primary: u64,
    secondary: u64,
) -> Result<cadmpeg_ir::sketches::SpatialSketchEntityId, CodecError> {
    let operation = "f3d spatial sketch curve identifier";
    let encoded_sketch = encode_component(ctx, sketch.as_str(), false, operation)?;
    let len = encoded_sketch.text.len();
    mint(
        ctx,
        ctx.format_retained(
            format_args!(
                "f3d:model:spatial-sketch-entity#{len}:{}c{primary}:{secondary}",
                encoded_sketch.text
            ),
            operation,
        )?,
    )
}

pub(super) fn neutral_sketch_constraint_id(
    ctx: &DecodeContext<'_>,
    native_ref: &str,
    record: u32,
) -> Result<cadmpeg_ir::sketches::SketchConstraintId, CodecError> {
    let stream = crate::ids::native_stream(native_ref).unwrap_or(crate::ids::DEFAULT_STREAM);
    let encoded_stream = encode_component(ctx, stream, false, "f3d sketch constraint identifier")?;
    mint(
        ctx,
        ctx.format_retained(
            format_args!(
                "f3d:model:sketch-constraint#{}@{record}",
                encoded_stream.text
            ),
            "f3d sketch constraint identifier",
        )?,
    )
}

pub(super) fn neutral_dimension_constraint_id(
    ctx: &DecodeContext<'_>,
    parameter: &cadmpeg_ir::features::ParameterId,
    form: &str,
) -> Result<cadmpeg_ir::sketches::SketchConstraintId, CodecError> {
    let operation = "f3d dimension constraint identifier";
    let key = identity_key(ctx, parameter.as_str())?;
    let encoded_form = encode_component(ctx, form, false, operation)?;
    let form_len = encoded_form.text.len();
    mint(
        ctx,
        ctx.format_retained(
            format_args!(
                "f3d:model:sketch-constraint#dimension:{}:{key}{form_len}:{}",
                key.len(),
                encoded_form.text
            ),
            operation,
        )?,
    )
}

pub(super) fn identity_key<'id>(
    ctx: &DecodeContext<'_>,
    id: &'id str,
) -> Result<&'id str, CodecError> {
    ctx.split_once(id, "#", "f3d identity key split")?
        .map(|(_, key)| key)
        .ok_or_else(|| CodecError::malformed("validated identity has no key"))
}

pub(super) fn neutral_component_insert_occurrence_id(
    ctx: &DecodeContext<'_>,
    scope: &crate::records::feature::scope::DesignParameterScope,
) -> Result<cadmpeg_ir::ids::OccurrenceId, CodecError> {
    let operation = "f3d component insert occurrence identifier";
    let stream = crate::ids::native_stream(&scope.id).unwrap_or(crate::ids::DEFAULT_STREAM);
    let encoded_stream = encode_component(ctx, stream, false, operation)?;
    let len = encoded_stream.text.len();
    mint(
        ctx,
        ctx.format_retained(
            format_args!(
                "f3d:model:occurrence#component-insert-{len}:{}{}:{}",
                encoded_stream.text,
                scope.feature_ordinal.get(),
                scope.record_index
            ),
            operation,
        )?,
    )
}

pub(super) fn neutral_assembly_joint_id(
    ctx: &DecodeContext<'_>,
    scope: &crate::records::feature::scope::DesignParameterScope,
) -> Result<cadmpeg_ir::products::JointId, CodecError> {
    let operation = "f3d assembly joint identifier";
    let stream = crate::ids::native_stream(&scope.id).unwrap_or(crate::ids::DEFAULT_STREAM);
    let encoded_stream = encode_component(ctx, stream, false, operation)?;
    let len = encoded_stream.text.len();
    mint(
        ctx,
        ctx.format_retained(
            format_args!(
                "f3d:model:joint#{len}:{}{}",
                encoded_stream.text,
                scope.record_index
            ),
            operation,
        )?,
    )
}

pub(super) fn configuration_entry_id(
    ctx: &DecodeContext<'_>,
    entry: &str,
) -> Result<String, CodecError> {
    let encoded_entry = encode_component(ctx, entry, false, "f3d configuration native identifier")?;
    ctx.format_retained(
        format_args!("f3d:configuration:entry#{}", encoded_entry.text),
        "f3d configuration native identifier",
    )
}

pub(super) fn history_input_prefix(
    ctx: &DecodeContext<'_>,
    key: &str,
    previous: i64,
) -> Result<cadmpeg_ir::ids::IdentityKey, CodecError> {
    let text = ctx.format_retained(
        format_args!("{}:{key}:{previous}", key.len()),
        "f3d history input prefix",
    )?;
    cadmpeg_ir::ids::IdentityKey::try_new(text)
        .map_err(|error| crate::design::text::malformed_design(ctx, format_args!("{error}")))
}

pub(super) fn feature_input_topology_id(
    ctx: &DecodeContext<'_>,
    feature: &cadmpeg_ir::features::FeatureId,
    previous: i64,
) -> Result<cadmpeg_ir::ids::FeatureInputTopologyId, CodecError> {
    let key = identity_key(ctx, feature.as_str())?;
    mint(
        ctx,
        ctx.format_retained(
            format_args!("f3d:history-input:state#{}:{key}:{previous}", key.len()),
            "f3d feature input topology identifier",
        )?,
    )
}

pub(super) fn history_input_edge_id(
    ctx: &DecodeContext<'_>,
    prefix: &cadmpeg_ir::ids::IdentityKey,
    slot: i64,
    operation: &'static str,
) -> Result<cadmpeg_ir::ids::HistoricalEdgeId, CodecError> {
    mint(
        ctx,
        ctx.format_retained(
            format_args!("f3d:history-input:edge#{}:{slot}", prefix.as_str()),
            operation,
        )?,
    )
}

pub(super) fn history_input_face_id(
    ctx: &DecodeContext<'_>,
    prefix: &cadmpeg_ir::ids::IdentityKey,
    slot: i64,
    operation: &'static str,
) -> Result<cadmpeg_ir::ids::HistoricalFaceId, CodecError> {
    mint(
        ctx,
        ctx.format_retained(
            format_args!("f3d:history-input:face#{}:{slot}", prefix.as_str()),
            operation,
        )?,
    )
}

pub(super) fn history_input_vertex_id(
    ctx: &DecodeContext<'_>,
    prefix: &cadmpeg_ir::ids::IdentityKey,
    slot: i64,
    operation: &'static str,
) -> Result<cadmpeg_ir::ids::HistoricalVertexId, CodecError> {
    mint(
        ctx,
        ctx.format_retained(
            format_args!("f3d:history-input:vertex#{}:{slot}", prefix.as_str()),
            operation,
        )?,
    )
}

pub(super) fn neutral_assembly_axial_object_id(
    ctx: &DecodeContext<'_>,
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
    let encoded_asset = encode_component(ctx, asset, true, operation)?;
    let asset_len = encoded_asset.text.len();
    let encoded_context = encode_component(ctx, context, true, operation)?;
    let context_len = encoded_context.text.len();
    let encoded_external = encode_component(ctx, external, true, operation)?;
    let external_len = encoded_external.text.len();
    let encoded_link = encode_component(ctx, link, false, operation)?;
    let link_len = encoded_link.text.len();
    let encoded_property = encode_component(ctx, property, true, operation)?;
    let property_len = encoded_property.text.len();
    let encoded_version = encode_component(ctx, version, false, operation)?;
    let version_len = encoded_version.text.len();
    let present = u8::from(identity.external_version.is_some());
    ctx.format_retained(format_args!("f3d:feature-input:connector#assembly-axial:{asset_len}:{}:{context_len}:{}:{}:{}:{external_len}:{}:{link_len}:{}:{present}:{property_len}:{}:{present}:{version_len}:{}", encoded_asset.text, encoded_context.text, identity.external_object_reference, identity.external_segment, encoded_external.text, encoded_link.text, encoded_property.text, encoded_version.text), operation)
}

pub(super) fn neutral_assembly_legacy_object_id(
    ctx: &DecodeContext<'_>,
    selection: &crate::records::feature::assembly::DesignAssemblyLegacySelection,
) -> Result<String, CodecError> {
    let operation = "f3d assembly legacy connector identifier";
    let asset = selection.asset_id.as_str();
    let context = selection.context_id.as_str();
    let recipe = selection.recipe_id.as_str();
    let encoded_asset = encode_component(ctx, asset, true, operation)?;
    let asset_len = encoded_asset.text.len();
    let encoded_context = encode_component(ctx, context, true, operation)?;
    let context_len = encoded_context.text.len();
    let encoded_recipe = encode_component(ctx, recipe, true, operation)?;
    let recipe_len = encoded_recipe.text.len();
    ctx.format_retained(format_args!("f3d:feature-input:connector#assembly-legacy:{asset_len}:{}:{context_len}:{}:{recipe_len}:{}:{}:{}", encoded_asset.text, encoded_context.text, encoded_recipe.text, selection.record_index, selection.recipe_record_index), operation)
}

#[cfg(test)]
mod tests;
