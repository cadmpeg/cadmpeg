// SPDX-License-Identifier: Apache-2.0
//! The `f3d:` URN identifier scheme.
//!
//! Every persisted or compared identity key in this crate is a `f3d:` URN.
//! This module owns the scheme: the segment vocabulary, separators, ordering,
//! escaping, and the `#{len}:{key}` length-prefix conventions live here so the
//! byte layout of an identity key is defined in exactly one place. Callers
//! build IDs through the named functions below rather than by inlining
//! `format!("f3d:...")` at the use site.
//!
//! The strings these builders produce are identity keys that are stored and
//! compared, so each builder reproduces its historical byte layout exactly.
//! Two sites that share a prefix but differ in tail structure get distinct
//! builders rather than a single reshaped one.

use crate::records::{DesignParameter, DesignParameterScope, DesignSketchPlacement};

/// The scheme prefix shared by every `f3d:` URN. Used to strip or test the
/// scheme when parsing an identity key back into its stream and tail.
pub(crate) const SCHEME_PREFIX: &str = "f3d:";

/// The native stream used when an identity key carries no qualifying stream —
/// the fallback for `native_stream(id).unwrap_or(..)`.
pub(crate) const DEFAULT_STREAM: &str = "f3d:design";

/// Parse the native stream segment out of an identity key: the text before the
/// final `:` separator. Returns `None` when the key carries no separator.
pub(crate) fn native_stream(id: &str) -> Option<&str> {
    id.rsplit_once(':').map(|(stream, _)| stream)
}

/// The fixed key of the single source-image record a design carries.
pub(crate) const FILE_SOURCE_IMAGE_ID: &str = "f3d:file:source-image#0";

/// Percent-encode `#`, `%`, and whitespace so a value can occupy one
/// `#{len}:{key}` identity-key segment without colliding with the separators
/// or the reserved escape byte.
fn identity_key_component(value: &str) -> String {
    use std::fmt::Write as _;

    let mut encoded = String::with_capacity(value.len());
    for character in value.chars() {
        if character == '#' || character == '%' || character.is_whitespace() {
            let mut bytes = [0; 4];
            for byte in character.encode_utf8(&mut bytes).as_bytes() {
                write!(encoded, "%{byte:02X}").expect("writing to a String cannot fail");
            }
        } else {
            encoded.push(character);
        }
    }
    encoded
}

/// The neutral B-rep topology entity key for entity `index`.
pub(crate) fn brep_entity_id(index: impl std::fmt::Display) -> String {
    format!("f3d:brep:entity#{index}")
}

/// The Design configuration record key for the archive entry `entry_name`.
pub(crate) fn configuration_entry_id(entry_name: &str) -> String {
    format!("f3d:configuration:entry#{entry_name}")
}

/// The neutral configuration key for `variant_name` under `entry_name`, with
/// both names length-prefixed into `#{len}:{key}{len}:{key}` segments.
pub(crate) fn neutral_configuration_id(
    entry_name: &str,
    variant_name: &str,
) -> cadmpeg_ir::features::ConfigurationId {
    cadmpeg_ir::features::ConfigurationId(format!(
        "f3d:configuration:variant#{}:{}{}:{}",
        entry_name.len(),
        entry_name,
        variant_name.len(),
        variant_name,
    ))
}

/// The neutral feature key for a parameter `scope`.
pub(crate) fn neutral_feature_id(scope: &DesignParameterScope) -> cadmpeg_ir::features::FeatureId {
    neutral_feature_id_parts(
        native_stream(&scope.id).unwrap_or(DEFAULT_STREAM),
        &scope.kind,
        scope.feature_ordinal,
        scope.record_index,
    )
}

/// The neutral feature key from its `stream`, `kind`, ordinal, and scope record
/// index, with `stream` and `kind` length-prefixed into `#{len}:{key}` segments.
pub(crate) fn neutral_feature_id_parts(
    stream: &str,
    kind: &str,
    feature_ordinal: u32,
    scope_record_index: u32,
) -> cadmpeg_ir::features::FeatureId {
    let stream = identity_key_component(stream);
    let kind = identity_key_component(kind);
    cadmpeg_ir::features::FeatureId(format!(
        "f3d:model:feature#{}:{}{}:{}{}:{}",
        stream.len(),
        stream,
        kind.len(),
        kind,
        feature_ordinal,
        scope_record_index,
    ))
}

/// The neutral parameter key for a design `parameter`.
pub(crate) fn neutral_parameter_id(
    parameter: &DesignParameter,
) -> cadmpeg_ir::features::ParameterId {
    neutral_parameter_id_parts(
        native_stream(&parameter.id).unwrap_or(DEFAULT_STREAM),
        parameter.source_ordinal,
    )
}

/// The neutral parameter key from its `stream` and source ordinal, with
/// `stream` length-prefixed into a `#{len}:{key}` segment.
pub(crate) fn neutral_parameter_id_parts(
    stream: &str,
    source_ordinal: u32,
) -> cadmpeg_ir::features::ParameterId {
    let stream = identity_key_component(stream);
    cadmpeg_ir::features::ParameterId(format!(
        "f3d:model:parameter#{}:{}{}",
        stream.len(),
        stream,
        source_ordinal,
    ))
}

/// The neutral planar-sketch key for a sketch `placement`.
pub(crate) fn neutral_sketch_id(
    placement: &DesignSketchPlacement,
) -> cadmpeg_ir::sketches::SketchId {
    cadmpeg_ir::sketches::SketchId(sketch_placement_id("sketch", placement))
}

/// The neutral spatial-sketch key for a sketch `placement`.
pub(crate) fn neutral_spatial_sketch_id(
    placement: &DesignSketchPlacement,
) -> cadmpeg_ir::sketches::SpatialSketchId {
    cadmpeg_ir::sketches::SpatialSketchId(sketch_placement_id("spatial-sketch", placement))
}

/// The shared body of a sketch or spatial-sketch placement key: the placement's
/// stream, escaped, joined to its entity suffix by `@`. The `segment` selects
/// the `sketch` or `spatial-sketch` URN kind; the byte layout is otherwise
/// identical between the planar and spatial variants.
fn sketch_placement_id(segment: &str, placement: &DesignSketchPlacement) -> String {
    let stream = identity_key_component(native_stream(&placement.id).unwrap_or(DEFAULT_STREAM));
    format!("f3d:model:{segment}#{stream}@{}", placement.entity_suffix)
}

/// The neutral planar-sketch point-entity key under `sketch`.
pub(crate) fn neutral_sketch_point_id(
    sketch: &cadmpeg_ir::sketches::SketchId,
    persistent_id: u64,
) -> cadmpeg_ir::sketches::SketchEntityId {
    cadmpeg_ir::sketches::SketchEntityId(sketch_entity_tagged(
        "sketch-entity",
        &sketch.0,
        'p',
        persistent_id,
    ))
}

/// The neutral planar-sketch curve-entity key under `sketch`.
pub(crate) fn neutral_sketch_curve_id(
    sketch: &cadmpeg_ir::sketches::SketchId,
    primary_id: u64,
    secondary_id: u64,
) -> cadmpeg_ir::sketches::SketchEntityId {
    cadmpeg_ir::sketches::SketchEntityId(sketch_entity_curve(
        "sketch-entity",
        &sketch.0,
        primary_id,
        secondary_id,
    ))
}

/// The neutral planar-sketch text-entity key under `sketch`.
pub(crate) fn neutral_sketch_text_id(
    sketch: &cadmpeg_ir::sketches::SketchId,
    persistent_id: u64,
) -> cadmpeg_ir::sketches::SketchEntityId {
    cadmpeg_ir::sketches::SketchEntityId(sketch_entity_tagged(
        "sketch-entity",
        &sketch.0,
        't',
        persistent_id,
    ))
}

/// The neutral spatial-sketch curve-entity key under `sketch`.
pub(crate) fn neutral_spatial_sketch_curve_id(
    sketch: &cadmpeg_ir::sketches::SpatialSketchId,
    primary_id: u64,
    secondary_id: u64,
) -> cadmpeg_ir::sketches::SpatialSketchEntityId {
    cadmpeg_ir::sketches::SpatialSketchEntityId(sketch_entity_curve(
        "spatial-sketch-entity",
        &sketch.0,
        primary_id,
        secondary_id,
    ))
}

/// The neutral spatial-sketch point-entity key under `sketch`.
pub(crate) fn neutral_spatial_sketch_point_id(
    sketch: &cadmpeg_ir::sketches::SpatialSketchId,
    persistent_id: u64,
) -> cadmpeg_ir::sketches::SpatialSketchEntityId {
    cadmpeg_ir::sketches::SpatialSketchEntityId(sketch_entity_tagged(
        "spatial-sketch-entity",
        &sketch.0,
        'p',
        persistent_id,
    ))
}

/// The neutral spatial-sketch surface-entity key under `sketch`.
pub(crate) fn neutral_spatial_sketch_surface_id(
    sketch: &cadmpeg_ir::sketches::SpatialSketchId,
    persistent_id: u64,
) -> cadmpeg_ir::sketches::SpatialSketchEntityId {
    cadmpeg_ir::sketches::SpatialSketchEntityId(sketch_entity_tagged(
        "spatial-sketch-entity",
        &sketch.0,
        's',
        persistent_id,
    ))
}

/// A single-tag sketch-entity key: the escaped owning-sketch key, length-
/// prefixed, followed by a one-character `tag` (`p`/`t`/`s`) and one id. The
/// `segment` selects `sketch-entity` or `spatial-sketch-entity`; every other
/// byte is identical across the planar and spatial variants.
fn sketch_entity_tagged(segment: &str, sketch_key: &str, tag: char, id: u64) -> String {
    let sketch = identity_key_component(sketch_key);
    format!("f3d:model:{segment}#{}:{}{tag}{id}", sketch.len(), sketch)
}

/// A curve sketch-entity key: the escaped owning-sketch key, length-prefixed,
/// followed by `c`, the primary id, and the colon-joined secondary id. The
/// `segment` selects `sketch-entity` or `spatial-sketch-entity`; every other
/// byte is identical across the planar and spatial variants.
fn sketch_entity_curve(
    segment: &str,
    sketch_key: &str,
    primary_id: u64,
    secondary_id: u64,
) -> String {
    let sketch = identity_key_component(sketch_key);
    format!(
        "f3d:model:{segment}#{}:{}c{primary_id}:{secondary_id}",
        sketch.len(),
        sketch,
    )
}

/// The neutral sketch-constraint key for `native_ref` at `record_index`.
pub(crate) fn neutral_sketch_constraint_id(
    native_ref: &str,
    record_index: u32,
) -> cadmpeg_ir::sketches::SketchConstraintId {
    let stream = identity_key_component(native_stream(native_ref).unwrap_or(DEFAULT_STREAM));
    cadmpeg_ir::sketches::SketchConstraintId(format!(
        "f3d:model:sketch-constraint#{stream}@{record_index}"
    ))
}

/// The neutral dimension-constraint key derived from a `parameter` key and a
/// dimension `form`, with the parameter key tail and form length-prefixed.
pub(crate) fn neutral_dimension_constraint_id(
    parameter: &cadmpeg_ir::features::ParameterId,
    form: &str,
) -> cadmpeg_ir::sketches::SketchConstraintId {
    let parameter_key = parameter
        .0
        .split_once('#')
        .map_or(parameter.0.as_str(), |(_, key)| key);
    cadmpeg_ir::sketches::SketchConstraintId(format!(
        "f3d:model:sketch-constraint#dimension:{}:{}{}:{}",
        parameter_key.len(),
        parameter_key,
        form.len(),
        form,
    ))
}

// --- history-input topology keys -------------------------------------------
//
// A history-input key names a feature's boundary topology relative to a prior
// history state. The shared body is `{len}:{feature_key}:{previous_state_id}`;
// the entity kinds (edge/face/body) append `:{slot}`, and the state key stops
// at the body.

/// The shared body of a history-input key: the feature key length-prefixed and
/// joined to `previous_state_id` by colons.
pub(crate) fn history_input_prefix(
    feature_key: &str,
    previous_state_id: impl std::fmt::Display,
) -> String {
    format!("{}:{feature_key}:{previous_state_id}", feature_key.len())
}

/// The history-input state key for a `prefix` from [`history_input_prefix`].
pub(crate) fn history_input_state_id(prefix: &str) -> cadmpeg_ir::ids::FeatureInputTopologyId {
    cadmpeg_ir::ids::FeatureInputTopologyId(format!("f3d:history-input:state#{prefix}"))
}

/// The history-input edge key for `slot` under a `prefix`.
pub(crate) fn history_input_edge_id(
    prefix: &str,
    slot: impl std::fmt::Display,
) -> cadmpeg_ir::ids::HistoricalEdgeId {
    cadmpeg_ir::ids::HistoricalEdgeId(format!("f3d:history-input:edge#{prefix}:{slot}"))
}

/// The history-input face key for `slot` under a `prefix`.
pub(crate) fn history_input_face_id(
    prefix: &str,
    slot: impl std::fmt::Display,
) -> cadmpeg_ir::ids::HistoricalFaceId {
    cadmpeg_ir::ids::HistoricalFaceId(format!("f3d:history-input:face#{prefix}:{slot}"))
}

/// The history-input body key for `slot` under a `prefix`.
pub(crate) fn history_input_body_id(
    prefix: &str,
    slot: impl std::fmt::Display,
) -> cadmpeg_ir::ids::HistoricalBodyId {
    cadmpeg_ir::ids::HistoricalBodyId(format!("f3d:history-input:body#{prefix}:{slot}"))
}

// --- native design-record keys ---------------------------------------------
//
// Native design records are keyed `f3d:{scope}:{kind}#{offset}`, where `scope`
// is the archive stream name (`f3d:{entry_name}` without the scheme prefix, so
// the stored key reads `f3d:{entry_name}:...`) and `offset` is the record's
// byte offset or index within that stream.

/// The native scope key `f3d:{name}` for an archive entry or stream `name`.
pub(crate) fn native_scope(name: &str) -> String {
    format!("f3d:{name}")
}

/// The native scope key with a trailing separator, `f3d:{name}:`, for prefix
/// tests against keys owned by `name`.
pub(crate) fn native_scope_prefix(name: &str) -> String {
    format!("f3d:{name}:")
}

/// Macro defining one `f3d:{scope}:{kind}#{offset}` native-record builder.
macro_rules! native_record_id {
    ($(#[$meta:meta])* $name:ident, $kind:literal) => {
        $(#[$meta])*
        pub(crate) fn $name(scope: &str, offset: impl std::fmt::Display) -> String {
            format!(concat!("f3d:{scope}:", $kind, "#{offset}"), scope = scope, offset = offset)
        }
    };
}

native_record_id!(
    /// The native design-parameter record key.
    native_design_parameter_id,
    "design-parameter"
);
native_record_id!(
    /// The native design-parameter-owner record key.
    native_design_parameter_owner_id,
    "design-parameter-owner"
);
native_record_id!(
    /// The native design-parameter-companion record key.
    native_design_parameter_companion_id,
    "design-parameter-companion"
);
native_record_id!(
    /// The native design-parameter-scope record key.
    native_design_parameter_scope_id,
    "design-parameter-scope"
);
native_record_id!(
    /// The native design Canvas image-plane binding key.
    native_design_canvas_image_id,
    "design-canvas-image"
);
native_record_id!(
    /// The native design-dimension-recipe-record key.
    native_design_dimension_recipe_record_id,
    "design-dimension-recipe-record"
);
native_record_id!(
    /// The native design-dimension-locus-pair record key.
    native_design_dimension_locus_pair_id,
    "design-dimension-locus-pair"
);
native_record_id!(
    /// The native design-dimension-null-locus-pair record key.
    native_design_dimension_null_locus_pair_id,
    "design-dimension-null-locus-pair"
);
native_record_id!(
    /// The native design-dimension-annotation-frame record key.
    native_design_dimension_annotation_frame_id,
    "design-dimension-annotation-frame"
);
native_record_id!(
    /// The native design-dimension-locus-group record key.
    native_design_dimension_locus_group_id,
    "design-dimension-locus-group"
);
native_record_id!(
    /// The native design-edge-identity-operand record key.
    native_design_edge_identity_operand_id,
    "design-edge-identity-operand"
);
native_record_id!(
    /// The native design-extrude-selection-group record key.
    native_design_extrude_selection_group_id,
    "design-extrude-selection-group"
);
native_record_id!(
    /// The native design-extrude-selection-member record key.
    native_design_extrude_selection_member_id,
    "design-extrude-selection-member"
);
native_record_id!(
    /// The native design-construction-operand-group record key.
    native_design_construction_operand_group_id,
    "design-construction-operand-group"
);
native_record_id!(
    /// The native design-construction-operand-identity record key.
    native_design_construction_operand_identity_id,
    "design-construction-operand-identity"
);
native_record_id!(
    /// The native design-entity-selection-operand record key.
    native_design_entity_selection_operand_id,
    "design-entity-selection-operand"
);
native_record_id!(
    /// The native design-body-recipe-operand record key.
    native_design_body_recipe_operand_id,
    "design-body-recipe-operand"
);
native_record_id!(
    /// The native design-edge-operand record key.
    native_design_edge_operand_id,
    "design-edge-operand"
);
native_record_id!(
    /// The native design-face-operand record key.
    native_design_face_operand_id,
    "design-face-operand"
);
native_record_id!(
    /// The native design-sketch-placement record key.
    native_design_sketch_placement_id,
    "design-sketch-placement"
);
native_record_id!(
    /// The native persistent-reference record key.
    native_persistent_reference_id,
    "persistent-reference"
);
native_record_id!(
    /// The native lost-edge-reference record key.
    native_lost_edge_reference_id,
    "lost-edge-reference"
);
native_record_id!(
    /// The native design-object record key.
    native_design_object_id,
    "design-object"
);
native_record_id!(
    /// The native design-entity-header record key.
    native_design_entity_header_id,
    "design-entity-header"
);
native_record_id!(
    /// The native design-record-header record key.
    native_design_record_header_id,
    "design-record-header"
);
native_record_id!(
    /// The native sketch-relation record key.
    native_sketch_relation_id,
    "sketch-relation"
);
native_record_id!(
    /// The native sketch-point record key.
    native_sketch_point_id,
    "sketch-point"
);
native_record_id!(
    /// The native sketch-text record key.
    native_sketch_text_id,
    "sketch-text"
);
native_record_id!(
    /// The native sketch-curve-identity record key.
    native_sketch_curve_identity_id,
    "sketch-curve-identity"
);
native_record_id!(
    /// The native sketch-surface record key.
    native_sketch_surface_id,
    "sketch-surface"
);
native_record_id!(
    /// The native design-body-member record key.
    native_design_body_member_id,
    "design-body-member"
);
native_record_id!(
    /// The native design-body-bounds record key.
    native_design_body_bounds_id,
    "design-body-bounds"
);
native_record_id!(
    /// The native construction-recipe record key.
    native_construction_recipe_id,
    "construction-recipe"
);
native_record_id!(
    /// The native design-body-binding record key.
    native_design_body_binding_id,
    "design-body-binding"
);
