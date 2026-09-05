// SPDX-License-Identifier: Apache-2.0
//! The `f3d:` URN identifier scheme.
//!
//! Segment vocabulary, separators, ordering, escaping, and `#{len}:{key}`
//! length-prefixes. Callers build IDs through the named functions below.

use crate::records::{
    DesignAssemblyAxialSelectorIdentity, DesignAssemblyLegacySelection,
    DesignCombineExternalBodyIdentity, DesignParameter, DesignParameterScope,
    DesignSketchPlacement,
};

/// The scheme prefix shared by every `f3d:` URN. Used to strip or test the
/// scheme when parsing an identity key back into its stream and tail.
pub(crate) const SCHEME_PREFIX: &str = "f3d:";

/// Format component of every entity ID this codec emits.
pub(crate) const ID_FORMAT: cadmpeg_asm::ids::IdFormat<'static> = cadmpeg_asm::ids::IdFormat("f3d");

/// The native stream used when an identity key carries no qualifying stream —
/// the fallback for `native_stream(id).unwrap_or(..)`.
pub(crate) const DEFAULT_STREAM: &str = "f3d:design";

/// Parse the native stream segment out of an identity key: the text before the
/// final `:` separator. Returns `None` when the key carries no separator.
pub(crate) fn native_stream(id: &str) -> Option<&str> {
    id.rsplit_once(':').map(|(stream, _)| stream)
}

/// Return whether two native IDs belong to the same root document or xref occurrence.
pub(crate) fn same_native_occurrence(left: &str, right: &str) -> bool {
    const OCCURRENCE_SEGMENT: &str = "/occurrence-";

    fn occurrence(id: &str) -> Option<&str> {
        let mut occurrence_end = None;
        for (at, _) in id.match_indices(OCCURRENCE_SEGMENT) {
            let digits_at = at + OCCURRENCE_SEGMENT.len();
            let end = id[digits_at..]
                .find('/')
                .map_or(id.len(), |end| digits_at + end);
            if end > digits_at && id[digits_at..end].bytes().all(|byte| byte.is_ascii_digit()) {
                occurrence_end = Some(end);
            }
        }
        occurrence_end.map(|end| &id[..end])
    }

    match (occurrence(left), occurrence(right)) {
        (Some(left), Some(right)) => left == right,
        (None, None) => !left.contains(OCCURRENCE_SEGMENT) && !right.contains(OCCURRENCE_SEGMENT),
        _ => false,
    }
}

/// Parse the Design segment shared by sibling `MetaStream.dat` and
/// `BulkStream.dat` entries from a native record identity.
pub(crate) fn design_segment(id: &str) -> Option<&str> {
    let stream = native_stream(id)?;
    let (segment, entry) = stream.rsplit_once('/')?;
    matches!(entry, "MetaStream.dat" | "BulkStream.dat").then_some(segment)
}

/// The fixed key of the single source-image record a design carries.
pub(crate) const FILE_SOURCE_IMAGE_ID: &str = "f3d:file:source-image#0";

/// Percent-encode identity separators, the escape byte, and whitespace.
fn identity_key_component(value: &str) -> String {
    use std::fmt::Write as _;

    let mut encoded = String::with_capacity(value.len());
    for character in value.chars() {
        if matches!(character, ':' | '#' | '%') || character.is_whitespace() {
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

/// Reverse [`identity_key_component`] for a complete encoded component.
pub(crate) fn decode_identity_key_component(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] != b'%' {
            decoded.push(bytes[at]);
            at += 1;
            continue;
        }
        let pair = bytes.get(at + 1..at + 3)?;
        let digit = |byte: u8| match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            b'A'..=b'F' => Some(byte - b'A' + 10),
            _ => None,
        };
        decoded.push(digit(pair[0])? * 16 + digit(pair[1])?);
        at += 3;
    }
    String::from_utf8(decoded).ok()
}

/// The neutral B-rep topology entity key for entity `index`.
pub(crate) fn brep_entity_id(index: impl std::fmt::Display) -> String {
    format!("f3d:brep:entity#{index}")
}

/// Neutral product occurrence projected from one external-reference placement.
pub(crate) fn neutral_xref_occurrence_id(
    reference_ordinal: u32,
    occurrence_ordinal: u32,
) -> cadmpeg_ir::ids::OccurrenceId {
    cadmpeg_ir::ids::OccurrenceId::mint(format!(
        "f3d:model:occurrence#xref-{reference_ordinal}-{occurrence_ordinal}"
    ))
    .expect("identity grammar")
}

/// Neutral local component definition projected from its stable Design GUID.
pub(crate) fn neutral_component_id(guid: &str) -> cadmpeg_ir::ids::ProductDefinitionId {
    cadmpeg_ir::ids::ProductDefinitionId::mint(format!(
        "f3d:model:component#{}",
        guid.to_ascii_lowercase()
    ))
    .expect("identity grammar")
}

/// Neutral local occurrence projected from its stable Design GUID.
pub(crate) fn neutral_component_occurrence_id(guid: &str) -> cadmpeg_ir::ids::OccurrenceId {
    cadmpeg_ir::ids::OccurrenceId::mint(format!(
        "f3d:model:occurrence#{}",
        guid.to_ascii_lowercase()
    ))
    .expect("identity grammar")
}

/// Neutral occurrence identity for an external component-insert scope whose
/// target document is not present in the container.
pub(crate) fn neutral_component_insert_occurrence_id(
    scope: &DesignParameterScope,
) -> cadmpeg_ir::ids::OccurrenceId {
    let stream = identity_key_component(native_stream(&scope.id).unwrap_or(DEFAULT_STREAM));
    cadmpeg_ir::ids::OccurrenceId::mint(format!(
        "f3d:model:occurrence#component-insert-{}:{}{}:{}",
        stream.len(),
        stream,
        scope.feature_ordinal,
        scope.record_index,
    ))
    .expect("identity grammar")
}

/// Neutral assembly-joint key projected from one Design parameter scope.
pub(crate) fn neutral_assembly_joint_id(
    scope: &crate::records::DesignParameterScope,
) -> cadmpeg_ir::products::JointId {
    let stream = identity_key_component(native_stream(&scope.id).unwrap_or(DEFAULT_STREAM));
    cadmpeg_ir::products::JointId(format!(
        "f3d:model:joint#{}:{}{}",
        stream.len(),
        stream,
        scope.record_index
    ))
}

/// The Design configuration record key for the archive entry `entry_name`.
pub(crate) fn configuration_entry_id(entry_name: &str) -> String {
    format!(
        "f3d:configuration:entry#{}",
        identity_key_component(entry_name)
    )
}

/// Neutral per-face appearance binding joined through two GUIDs and a face.
pub(crate) fn neutral_face_appearance_binding_id(
    face_guid: &str,
    visual_guid: &str,
    face: &cadmpeg_ir::ids::FaceId,
) -> String {
    let face = identity_key_component(&face.0);
    format!(
        "f3d:appearance:face#{face_guid}:{visual_guid}:{}:{face}",
        face.len()
    )
}

/// The neutral configuration key for `variant_name` under `entry_name`, with
/// both names length-prefixed into `#{len}:{key}{len}:{key}` segments.
pub(crate) fn neutral_configuration_id(
    entry_name: &str,
    variant_name: &str,
) -> cadmpeg_ir::features::ConfigurationId {
    let entry_name = identity_key_component(entry_name);
    let variant_name = identity_key_component(variant_name);
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
        scope.kind_name(),
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

/// Feature-input-local body key for one complete external `Combine` selector path.
pub(crate) fn neutral_combine_external_body_id(
    identity: &DesignCombineExternalBodyIdentity,
) -> String {
    let selector_asset = identity_key_component(&identity.selector_asset_id);
    let selector_context = identity_key_component(&identity.selector_context_id);
    let external_asset = identity_key_component(&identity.external_asset_id);
    let link_name = identity_key_component(&identity.external_link_name);
    let property_key = identity
        .external_property_key
        .as_deref()
        .map(identity_key_component)
        .unwrap_or_default();
    let version_urn = identity
        .external_version_urn
        .as_deref()
        .map(identity_key_component)
        .unwrap_or_default();
    format!(
        "f3d:feature-input:body#combine-external:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}",
        selector_asset.len(),
        selector_asset,
        selector_context.len(),
        selector_context,
        identity.occurrence_reference,
        identity.external_body_reference,
        identity.external_segment,
        external_asset.len(),
        external_asset,
        link_name.len(),
        link_name,
        u8::from(identity.external_property_key.is_some()),
        property_key.len(),
        property_key,
        u8::from(identity.external_version_urn.is_some()),
        version_urn.len(),
        version_urn,
    )
}

/// Feature-input-local connector key for one pathless axial assembly selector.
pub(crate) fn neutral_assembly_axial_object_id(
    identity: &DesignAssemblyAxialSelectorIdentity,
) -> String {
    let selector_asset = identity_key_component(&identity.selector_asset_id.to_ascii_lowercase());
    let selector_context =
        identity_key_component(&identity.selector_context_id.to_ascii_lowercase());
    let external_asset = identity_key_component(&identity.external_asset_id.to_ascii_lowercase());
    let link_name = identity_key_component(&identity.external_link_name);
    let property_key = identity
        .external_property_key
        .as_deref()
        .map(|value| identity_key_component(&value.to_ascii_lowercase()))
        .unwrap_or_default();
    let version_urn = identity
        .external_version_urn
        .as_deref()
        .map(identity_key_component)
        .unwrap_or_default();
    format!(
        "f3d:feature-input:connector#assembly-axial:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}",
        selector_asset.len(),
        selector_asset,
        selector_context.len(),
        selector_context,
        identity.external_object_reference,
        identity.external_segment,
        external_asset.len(),
        external_asset,
        link_name.len(),
        link_name,
        u8::from(identity.external_property_key.is_some()),
        property_key.len(),
        property_key,
        u8::from(identity.external_version_urn.is_some()),
        version_urn.len(),
        version_urn,
    )
}

/// Feature-input-local connector key for one direct legacy `As-built` face
/// selection.
pub(crate) fn neutral_assembly_legacy_object_id(
    selection: &DesignAssemblyLegacySelection,
) -> String {
    let asset = identity_key_component(&selection.asset_id.to_ascii_lowercase());
    let context = identity_key_component(&selection.context_id.to_ascii_lowercase());
    let recipe = identity_key_component(&selection.recipe_id.to_ascii_lowercase());
    format!(
        "f3d:feature-input:connector#assembly-legacy:{}:{}:{}:{}:{}:{}:{}:{}",
        asset.len(),
        asset,
        context.len(),
        context,
        recipe.len(),
        recipe,
        selection.record_index,
        selection.recipe_record_index,
    )
}

/// The neutral embedded-asset key for one exact archive entry.
pub(crate) fn neutral_asset_id(entry_name: &str) -> cadmpeg_ir::assets::AssetId {
    let entry_name = identity_key_component(entry_name);
    cadmpeg_ir::assets::AssetId(format!("f3d:model:asset#{}:{entry_name}", entry_name.len()))
}

/// The neutral parameter key for a design `parameter`.
pub(crate) fn neutral_parameter_id(
    parameter: &DesignParameter,
) -> cadmpeg_ir::features::ParameterId {
    neutral_parameter_id_parts(
        native_stream(&parameter.id).unwrap_or(DEFAULT_STREAM),
        parameter.record_index,
    )
}

/// The neutral parameter key from its `stream` and indexed-record identity, with
/// `stream` length-prefixed into a `#{len}:{key}` segment.
pub(crate) fn neutral_parameter_id_parts(
    stream: &str,
    record_index: u32,
) -> cadmpeg_ir::features::ParameterId {
    let stream = identity_key_component(stream);
    cadmpeg_ir::features::ParameterId(format!(
        "f3d:model:parameter#{}:{}{}",
        stream.len(),
        stream,
        record_index,
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

/// The source-local neutral key for a planar sketch record that has no
/// persistent entity identity.
pub(crate) fn neutral_sketch_record_id(
    sketch: &cadmpeg_ir::sketches::SketchId,
    record_index: u32,
) -> cadmpeg_ir::sketches::SketchEntityId {
    cadmpeg_ir::sketches::SketchEntityId(sketch_entity_tagged(
        "sketch-entity",
        &sketch.0,
        'x',
        u64::from(record_index),
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

/// The source-local neutral key for a spatial-sketch record that has no
/// persistent entity identity.
pub(crate) fn neutral_spatial_sketch_record_id(
    sketch: &cadmpeg_ir::sketches::SpatialSketchId,
    record_index: u32,
) -> cadmpeg_ir::sketches::SpatialSketchEntityId {
    cadmpeg_ir::sketches::SpatialSketchEntityId(sketch_entity_tagged(
        "spatial-sketch-entity",
        &sketch.0,
        'x',
        u64::from(record_index),
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
/// prefixed, followed by a one-character `tag` (`p`/`t`/`s`/`x`) and one id. The
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
    let form = identity_key_component(form);
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
    let feature_key = identity_key_component(feature_key);
    format!("{}:{feature_key}:{previous_state_id}", feature_key.len())
}

/// The history-input state key for a `prefix` from [`history_input_prefix`].
pub(crate) fn history_input_state_id(prefix: &str) -> cadmpeg_ir::ids::FeatureInputTopologyId {
    cadmpeg_ir::ids::FeatureInputTopologyId::mint(format!("f3d:history-input:state#{prefix}"))
        .expect("identity grammar")
}

/// The history-input edge key for `slot` under a `prefix`.
pub(crate) fn history_input_edge_id(
    prefix: &str,
    slot: impl std::fmt::Display,
) -> cadmpeg_ir::ids::HistoricalEdgeId {
    cadmpeg_ir::ids::HistoricalEdgeId::mint(format!("f3d:history-input:edge#{prefix}:{slot}"))
        .expect("identity grammar")
}

/// The history-input vertex key for `slot` under a `prefix`.
pub(crate) fn history_input_vertex_id(
    prefix: &str,
    slot: impl std::fmt::Display,
) -> cadmpeg_ir::ids::HistoricalVertexId {
    cadmpeg_ir::ids::HistoricalVertexId::mint(format!("f3d:history-input:vertex#{prefix}:{slot}"))
        .expect("identity grammar")
}

/// The history-input face key for `slot` under a `prefix`.
pub(crate) fn history_input_face_id(
    prefix: &str,
    slot: impl std::fmt::Display,
) -> cadmpeg_ir::ids::HistoricalFaceId {
    cadmpeg_ir::ids::HistoricalFaceId::mint(format!("f3d:history-input:face#{prefix}:{slot}"))
        .expect("identity grammar")
}

/// The history-input body key for `slot` under a `prefix`.
pub(crate) fn history_input_body_id(
    prefix: &str,
    slot: impl std::fmt::Display,
) -> cadmpeg_ir::ids::HistoricalBodyId {
    cadmpeg_ir::ids::HistoricalBodyId::mint(format!("f3d:history-input:body#{prefix}:{slot}"))
        .expect("identity grammar")
}

// --- native design-record keys ---------------------------------------------
//
// Native design records are keyed `f3d:{scope}:{kind}#{offset}`, where `scope`
// is the escaped archive stream name and `offset` is the record's byte offset
// or index within that stream.

/// The native scope key for an archive entry or stream `name`.
pub(crate) fn native_scope(name: &str) -> String {
    format!("f3d:{}", identity_key_component(name))
}

/// The escaped native scope key with a trailing separator for prefix tests.
pub(crate) fn native_scope_prefix(name: &str) -> String {
    format!("{}:", native_scope(name))
}

/// Build one record ID in an archive-entry-qualified native scope.
pub(crate) fn native_scoped_id(scope: &str, kind: &str, key: impl std::fmt::Display) -> String {
    format!("{}:{kind}#{key}", native_scope(scope))
}

/// Macro defining one `f3d:{scope}:{kind}#{offset}` native-record builder.
macro_rules! native_record_id {
    ($(#[$meta:meta])* $name:ident, $kind:literal) => {
        $(#[$meta])*
        pub(crate) fn $name(scope: &str, offset: impl std::fmt::Display) -> String {
            native_scoped_id(scope, $kind, offset)
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
    /// The native `SurfaceTrim` BRep-cell carrier record key.
    native_design_surface_trim_operation_id,
    "design-surface-trim-operation"
);
native_record_id!(
    /// The native ordered Design feature-timeline record key.
    native_design_feature_timeline_id,
    "design-feature-timeline"
);
native_record_id!(
    /// The native Design component naming-space binding key.
    native_design_component_naming_space_id,
    "design-component-naming-space"
);

/// The native ordered Design feature-timeline key in an already encoded
/// `f3d:` stream scope.
pub(crate) fn native_design_feature_timeline_id_in_stream(
    stream: &str,
    offset: impl std::fmt::Display,
) -> String {
    format!("{stream}:design-feature-timeline#{offset}")
}
native_record_id!(
    /// The native design Canvas image-plane binding key.
    native_design_canvas_image_id,
    "design-canvas-image"
);
native_record_id!(
    /// The native design Decal image and target binding key.
    native_design_decal_image_id,
    "design-decal-image"
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
    /// The native design-dimension-presentation-frame record key.
    native_design_dimension_presentation_frame_id,
    "design-dimension-presentation-frame"
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
    /// The native legacy-Loft body-carrier record key.
    native_design_loft_legacy_body_carrier_id,
    "design-loft-legacy-body-carrier"
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
    /// The native design-face-source-group record key.
    native_design_face_source_group_id,
    "design-face-source-group"
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
    /// The native design-type record key.
    native_design_type_id,
    "design-type"
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
    /// The native mesh-body record key.
    native_mesh_body_id,
    "mesh-body"
);
native_record_id!(
    /// The native Design mesh-feature graph key.
    native_design_mesh_feature_id,
    "design-mesh-feature"
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

#[cfg(test)]
mod tests {
    use super::{
        decode_identity_key_component, design_segment, native_design_feature_timeline_id_in_stream,
        native_design_type_id, native_scope, neutral_assembly_legacy_object_id,
        neutral_face_appearance_binding_id, neutral_sketch_record_id, neutral_sketch_text_id,
        same_native_occurrence, SCHEME_PREFIX,
    };
    use crate::records::{ConstructionRecipeKind, DesignAssemblyLegacySelection};

    #[test]
    fn design_segment_joins_sibling_meta_and_bulk_stream_ids() {
        let meta = "f3d:Asset/Design1/MetaStream.dat:design-type#10";
        let bulk = "f3d:Asset/Design1/BulkStream.dat:design-canvas-image#20";
        assert_eq!(design_segment(meta), Some("f3d:Asset/Design1"));
        assert_eq!(design_segment(meta), design_segment(bulk));
        assert_eq!(
            design_segment("f3d:Asset/Design1/Other.dat:record#20"),
            None
        );
    }

    #[test]
    fn native_ids_escape_archive_names_without_losing_the_raw_stream() {
        let entry = "Simulation Case/Design:1/MetaStream%20.dat";
        let id = native_design_type_id(entry, 10);
        assert_eq!(
            id,
            "f3d:Simulation%20Case/Design%3A1/MetaStream%2520.dat:design-type#10"
        );
        let encoded = native_scope(entry)
            .strip_prefix(SCHEME_PREFIX)
            .expect("native scheme")
            .to_owned();
        assert_eq!(
            decode_identity_key_component(&encoded).as_deref(),
            Some(entry)
        );
        assert_eq!(
            crate::writer::patch::records::native_stream(&id, ":design-type#")
                .expect("writer stream"),
            entry
        );
        assert_eq!(
            native_design_feature_timeline_id_in_stream(&native_scope(entry), 20),
            "f3d:Simulation%20Case/Design%3A1/MetaStream%2520.dat:design-feature-timeline#20"
        );
    }

    #[test]
    fn face_appearance_binding_escapes_the_nested_face_identity() {
        let id = neutral_face_appearance_binding_id(
            "face-guid",
            "visual-guid",
            &cadmpeg_ir::ids::FaceId::mint("f3d:brep/path:face#12").expect("identity grammar"),
        );
        assert_eq!(
            id,
            "f3d:appearance:face#face-guid:visual-guid:27:f3d%3Abrep/path%3Aface%2312"
        );
        assert_eq!(id.matches('#').count(), 1);
    }

    #[test]
    fn native_occurrence_scope_isolates_xrefs_and_includes_root_streams() {
        assert!(same_native_occurrence(
            "f3d:Asset/Design1/BulkStream.dat:record#1",
            "f3d:design:persistent-subentity-tag#1",
        ));
        assert!(same_native_occurrence(
            "f3d:xref/root/occurrence-0/Asset/Design1/BulkStream.dat:record#1",
            "f3d:xref/root/occurrence-0/design:persistent-subentity-tag#1",
        ));
        assert!(!same_native_occurrence(
            "f3d:xref/root/occurrence-0/Asset/Design1/BulkStream.dat:record#1",
            "f3d:xref/other/occurrence-0/design:persistent-subentity-tag#1",
        ));
        assert!(!same_native_occurrence(
            "f3d:xref/root/occurrence-0/xref/child/occurrence-0/design:record#1",
            "f3d:xref/root/occurrence-0/design:persistent-subentity-tag#1",
        ));
        assert!(!same_native_occurrence(
            "f3d:xref/root/occurrence-invalid/design:record#1",
            "f3d:design:persistent-subentity-tag#1",
        ));
    }

    #[test]
    fn identityless_sketch_geometry_uses_a_disjoint_source_record_namespace() {
        let sketch = cadmpeg_ir::sketches::SketchId("f3d:model:sketch#example".into());
        let persistent = neutral_sketch_text_id(&sketch, 42);
        let source_record = neutral_sketch_record_id(&sketch, 42);
        assert_ne!(persistent, source_record);
        assert_eq!(source_record, neutral_sketch_record_id(&sketch, 42));
        assert_ne!(source_record, neutral_sketch_record_id(&sketch, 43));
    }

    #[test]
    fn legacy_assembly_connector_key_is_namespace_and_recipe_scoped() {
        let selection = DesignAssemblyLegacySelection {
            record_index: 7,
            byte_offset: 100,
            class_tag: "264".into(),
            asset_id: "A B".into(),
            asset_id_offset: 110,
            context_id: "CTX#".into(),
            context_id_offset: 120,
            recipe_record_index: 8,
            recipe_record_byte_offset: 130,
            recipe_id: "Recipe:1".into(),
            recipe_kind: ConstructionRecipeKind::Face,
            recipe_references: Vec::new(),
            next_byte_offset: 140,
        };
        assert_eq!(
            neutral_assembly_legacy_object_id(&selection),
            "f3d:feature-input:connector#assembly-legacy:5:a%20b:6:ctx%23:10:recipe%3A1:7:8"
        );
        let mut second = selection.clone();
        second.record_index += 1;
        assert_ne!(
            neutral_assembly_legacy_object_id(&selection),
            neutral_assembly_legacy_object_id(&second)
        );
    }
}
