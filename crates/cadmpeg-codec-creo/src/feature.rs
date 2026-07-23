// SPDX-License-Identifier: Apache-2.0
//! Structural `AllFeatur` feature-to-generated-entity bindings.
//!
//! A mixed generated-entity table is `f8 <count> f7 <table-class> fb e3`, followed by
//! exactly `<count>` compact entity identifiers, each terminated by `e3`.
//! `f7 <entry-class>` may prefix the first entry. The table belongs to an `AllFeatur` row only
//! when its byte offset is bounded by that row's known feature-id header.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::cursor::bounded_len;

use crate::psb;
use crate::scalar;

/// Exact procedural recipe stored in a feature-state record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureRecipe {
    /// Additive linear section sweep named `protextrude`.
    ProtrudeExtrude,
    /// Subtractive linear section sweep named `cutextrude`.
    CutExtrude,
    /// Additive rotational section sweep named `protrevolve`.
    ProtrudeRevolve,
    /// Subtractive rotational section sweep named `cutrevolve`.
    CutRevolve,
}

/// Geometry family selected by a procedural feature recipe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureRecipeKind {
    /// Linear section sweep.
    Extrude,
    /// Rotational section sweep.
    Revolve,
}

/// Boolean effect selected by a procedural feature recipe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureRecipeEffect {
    /// Material-adding operation.
    Protrude,
    /// Material-removing operation.
    Cut,
}

impl FeatureRecipe {
    /// Exact stored recipe name without its NUL terminator.
    pub const fn name(self) -> &'static str {
        match self {
            Self::ProtrudeExtrude => "protextrude",
            Self::CutExtrude => "cutextrude",
            Self::ProtrudeRevolve => "protrevolve",
            Self::CutRevolve => "cutrevolve",
        }
    }

    /// Section-sweep geometry family.
    pub const fn kind(self) -> FeatureRecipeKind {
        match self {
            Self::ProtrudeExtrude | Self::CutExtrude => FeatureRecipeKind::Extrude,
            Self::ProtrudeRevolve | Self::CutRevolve => FeatureRecipeKind::Revolve,
        }
    }

    /// Boolean effect of the section sweep.
    pub const fn effect(self) -> FeatureRecipeEffect {
        match self {
            Self::ProtrudeExtrude | Self::ProtrudeRevolve => FeatureRecipeEffect::Protrude,
            Self::CutExtrude | Self::CutRevolve => FeatureRecipeEffect::Cut,
        }
    }
}

const FEATURE_RECIPES: &[(&[u8], FeatureRecipe)] = &[
    (b"protextrude\0", FeatureRecipe::ProtrudeExtrude),
    (b"cutextrude\0", FeatureRecipe::CutExtrude),
    (b"protrevolve\0", FeatureRecipe::ProtrudeRevolve),
    (b"cutrevolve\0", FeatureRecipe::CutRevolve),
];

/// Feature-operation family named by a feature-state record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureOperation {
    /// Numeric feature identifier following `id` in the stored name.
    pub feature_id: u32,
    /// Stored operation-family name.
    pub kind: String,
    /// Whether `kind` came from a stored `<Kind> id <N>` display name.
    pub display_name_stored: bool,
    /// Display form of the stored operation name. Recipe-only states have no
    /// stored name.
    pub stored_name: Option<String>,
    /// Exact stored operation-name bytes excluding the NUL terminator.
    pub stored_name_bytes: Option<Vec<u8>>,
    /// Stored identifier keyword, preserving `id` versus `ID`.
    pub identifier_keyword: Option<String>,
    /// Optional stored-name byte immediately preceding the family name.
    pub stored_name_prefix: Option<u8>,
    /// Procedural recipe name stored in the same current-state record.
    pub recipe: Option<FeatureRecipe>,
    /// Root feature-definition schema class from a DEPDB recipe prefix.
    pub root_schema_class: Option<u32>,
    /// Previous or parent feature identifier from a DEPDB recipe prefix.
    pub parent_feature_id: Option<u32>,
    /// Byte offset of the operation name in the original stream.
    pub offset: usize,
    /// Byte offset including the optional stored-name prefix.
    pub state_offset: usize,
}

/// Feature name joined to its model feature identifier by `mdl_feat_ref_info_new`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureReferenceName {
    /// Numeric model feature identifier.
    pub feature_id: u32,
    /// Stored feature name.
    pub name: String,
    /// Exact stored feature-name bytes excluding the NUL terminator.
    pub name_bytes: Vec<u8>,
    /// Reference-database object identifier.
    pub own_reference_id: u32,
    /// Stored reference type.
    pub reference_type: u32,
    /// Byte offset of the `f7 0x71` entry header.
    pub offset: usize,
}

/// Decode structurally closed feature-name entries from model reference data.
pub fn reference_names(payload: &[u8]) -> Vec<FeatureReferenceName> {
    let mut names = Vec::new();
    for offset in 0..payload.len().saturating_sub(2) {
        if payload.get(offset..offset + 2) != Some(&[psb::token::ENTITY_REF, 0x71]) {
            continue;
        }
        let (own_reference_id, after_reference) = psb::compact_int(payload, offset + 2);
        let (reference_type, after_type) = psb::compact_int(payload, after_reference);
        let (feature_id, name_start) = psb::compact_int(payload, after_type);
        if after_reference == offset + 2
            || after_type == after_reference
            || name_start == after_type
            || feature_id == 0
        {
            continue;
        }
        let Some(name_end) = payload
            .get(name_start..name_start.saturating_add(256).min(payload.len()))
            .and_then(|tail| tail.iter().position(|byte| *byte == 0))
            .map(|relative| name_start + relative)
        else {
            continue;
        };
        let name_bytes = &payload[name_start..name_end];
        if name_bytes.is_empty() || name_bytes.iter().any(u8::is_ascii_control) {
            continue;
        }
        let (first_close, after_first_close) = psb::compact_int(payload, name_end + 1);
        let (second_close, after_second_close) = psb::compact_int(payload, after_first_close);
        if after_first_close == name_end + 1
            || after_second_close == after_first_close
            || first_close != own_reference_id
            || second_close != own_reference_id
        {
            continue;
        }
        names.push(FeatureReferenceName {
            feature_id,
            name: String::from_utf8_lossy(name_bytes).into_owned(),
            name_bytes: name_bytes.to_vec(),
            own_reference_id,
            reference_type,
            offset,
        });
    }
    names
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FeatureRecipeBinding {
    recipe: FeatureRecipe,
    root_schema_class: u32,
    parent_feature_id: u32,
    offset: usize,
}

fn recipe_bindings(payload: &[u8]) -> Vec<(u32, FeatureRecipeBinding)> {
    let mut bindings = Vec::new();
    for marker in 0..payload.len() {
        if payload.get(marker) != Some(&psb::token::ENTITY_REF) {
            continue;
        }
        let Ok((_, after_marker)) = psb::reference_id(payload, marker + 1) else {
            continue;
        };
        let (feature_id, after_feature) = psb::compact_int(payload, after_marker);
        let (schema_class, after_schema) = psb::compact_int(payload, after_feature);
        if after_feature == after_marker
            || after_schema == after_feature
            || !matches!(schema_class, 916 | 917)
            || payload.get(after_schema) != Some(&0xf6)
        {
            continue;
        }
        let (parent_feature_id, display_start) = psb::compact_int(payload, after_schema + 1);
        let Some(display_end) = payload
            .get(display_start..display_start.saturating_add(96).min(payload.len()))
            .and_then(|bytes| bytes.iter().position(|byte| *byte == 0))
            .map(|relative| display_start + relative)
        else {
            continue;
        };
        let recipe_start = display_end + 3;
        if display_end == display_start
            || payload.get(display_end + 1..recipe_start) != Some(&[0xf6, 0x00])
        {
            continue;
        }
        if let Some((_, recipe)) = FEATURE_RECIPES
            .iter()
            .find(|(name, _)| payload.get(recipe_start..recipe_start + name.len()) == Some(*name))
        {
            bindings.push((
                feature_id,
                FeatureRecipeBinding {
                    recipe: *recipe,
                    root_schema_class: schema_class,
                    parent_feature_id,
                    offset: marker,
                },
            ));
        }
    }
    bindings
}

/// Decode every NUL-terminated `<Kind> id <N>` operation state and bounded
/// procedural-recipe record from one feature-state namespace, in byte order.
pub fn operation_states(payload: &[u8]) -> Vec<FeatureOperation> {
    const SEPARATORS: &[&[u8]] = &[b" id ", b" ID "];
    let family_byte = |byte: u8| {
        byte.is_ascii_alphanumeric()
            || byte >= 0x80
            || matches!(byte, b' ' | b'_' | b'-' | b'/' | b'(' | b')')
    };
    let bound_recipes = recipe_bindings(payload);
    let recipe_binding_counts = bound_recipes.iter().fold(
        BTreeMap::<u32, usize>::new(),
        |mut counts, (feature_id, _)| {
            *counts.entry(*feature_id).or_default() += 1;
            counts
        },
    );
    let mut result = Vec::new();
    for separator in 0..payload.len().saturating_sub(4) {
        let Some(separator_bytes) = SEPARATORS.iter().find(|candidate| {
            payload.get(separator..separator + candidate.len()) == Some(**candidate)
        }) else {
            continue;
        };
        let mut offset = separator;
        while offset > 0 && family_byte(payload[offset - 1]) {
            offset -= 1;
        }
        while offset < separator && std::str::from_utf8(&payload[offset..separator]).is_err() {
            offset += 1;
        }
        let state_offset = offset;
        let stored_family = &payload[offset..separator];
        let family = stored_family;
        if family.is_empty() || family.first() == Some(&b' ') || family.last() == Some(&b' ') {
            continue;
        }
        let (stored_name_prefix, family) = match family {
            [prefix @ (b'o' | b'x' | b'y' | b'z'), first, ..] if first.is_ascii_uppercase() => {
                offset += 1;
                (Some(*prefix), &family[1..])
            }
            _ => (None, family),
        };
        let digits = &payload[separator + separator_bytes.len()..];
        let Some(end) = digits.iter().position(|byte| *byte == 0) else {
            continue;
        };
        if end == 0 || !digits[..end].iter().all(u8::is_ascii_digit) {
            continue;
        }
        let Ok(feature_id) = String::from_utf8_lossy(&digits[..end]).parse::<u32>() else {
            continue;
        };
        let record_start = payload[..offset]
            .iter()
            .rposition(|byte| *byte == 0xe3)
            .map_or(0, |position| position + 1);
        let record = &payload[record_start..offset];
        let matching_recipes = bound_recipes
            .iter()
            .filter(|(candidate, _)| *candidate == feature_id)
            .map(|(_, binding)| *binding)
            .collect::<Vec<_>>();
        let bound_recipe = match matching_recipes.as_slice() {
            [binding] => Some(*binding),
            _ => None,
        };
        let recipe = bound_recipe.map(|binding| binding.recipe).or_else(|| {
            FEATURE_RECIPES.iter().copied().find_map(|(name, recipe)| {
                record
                    .windows(name.len())
                    .any(|window| window == name)
                    .then_some(recipe)
            })
        });
        result.push(FeatureOperation {
            feature_id,
            kind: String::from_utf8_lossy(family).into_owned(),
            display_name_stored: true,
            stored_name: Some(
                String::from_utf8_lossy(
                    &payload[state_offset..separator + separator_bytes.len() + end],
                )
                .into_owned(),
            ),
            stored_name_bytes: Some(
                payload[state_offset..separator + separator_bytes.len() + end].to_vec(),
            ),
            identifier_keyword: Some(
                String::from_utf8_lossy(&separator_bytes[1..separator_bytes.len() - 1])
                    .into_owned(),
            ),
            stored_name_prefix,
            recipe,
            root_schema_class: bound_recipe.map(|binding| binding.root_schema_class),
            parent_feature_id: bound_recipe.map(|binding| binding.parent_feature_id),
            offset,
            state_offset,
        });
    }
    for (feature_id, binding) in bound_recipes {
        if recipe_binding_counts.get(&feature_id) == Some(&1)
            && result.iter().any(|operation| {
                operation.feature_id == feature_id
                    && operation.recipe == Some(binding.recipe)
                    && operation.root_schema_class == Some(binding.root_schema_class)
                    && operation.parent_feature_id == Some(binding.parent_feature_id)
            })
        {
            continue;
        }
        result.push(FeatureOperation {
            feature_id,
            kind: match binding.recipe.kind() {
                FeatureRecipeKind::Extrude => "Extrude",
                FeatureRecipeKind::Revolve => "Revolve",
            }
            .to_string(),
            display_name_stored: false,
            stored_name: None,
            stored_name_bytes: None,
            identifier_keyword: None,
            stored_name_prefix: None,
            recipe: Some(binding.recipe),
            root_schema_class: Some(binding.root_schema_class),
            parent_feature_id: Some(binding.parent_feature_id),
            offset: binding.offset,
            state_offset: binding.offset,
        });
    }
    result.sort_by_key(|operation| operation.offset);
    result
}

/// Decode the current operation state for each feature identifier.
pub fn operations(payload: &[u8]) -> Vec<FeatureOperation> {
    let mut current = operation_states(payload)
        .into_iter()
        .map(|operation| (operation.feature_id, operation))
        .collect::<BTreeMap<_, _>>()
        .into_values()
        .collect::<Vec<_>>();
    current.sort_by_key(|operation| operation.offset);
    current
}

/// One `AllFeatur` mixed generated-entity table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureEntityTable {
    /// Owning feature when the table lies in a bounded `AllFeatur` feature row.
    pub feature_id: Option<u32>,
    /// Entity-class identifier following the table's `f7` marker.
    pub table_class_id: u32,
    /// Entity identifiers in their declared generated-entity order.
    pub entry_ids: Vec<u32>,
    /// Structurally bounded records in their declared generated-entity order.
    pub entries: Vec<FeatureEntityTableEntry>,
    /// Entries that are materialized `srf_array` identifiers.
    pub surface_ids: Vec<u32>,
    /// Entries outside the materialized surface namespace.
    pub non_surface_entity_ids: Vec<u32>,
    /// Byte offset of the `f8` table opener in the original stream.
    pub offset: usize,
}

/// One record in an `AllFeatur` mixed generated-entity table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureEntityTableEntry {
    /// Entity identifier at the start of the record body.
    pub entity_id: u32,
    /// Positional entry class following the entity identifier.
    pub class_id: u32,
    /// Source section entity identifier carried by class `200` entries.
    pub source_entity_id: Option<u32>,
    /// Whether the record starts with the `f7 1e` entry prefix.
    pub prefixed: bool,
    /// Byte offset of the entity identifier in the original stream.
    pub offset: usize,
    /// Byte offset immediately after the entry body. This follows the
    /// structural `e3`, or points at the enclosing `f2 f7` table separator
    /// when the final entry uses that separator as its terminator.
    pub end_offset: usize,
}

/// One byte-bounded positional `AllFeatur` row for a known model feature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureRow {
    /// Feature identifier decoded from the row prefix.
    pub feature_id: u32,
    /// Two-byte row-header family discriminator.
    pub header: [u8; 2],
    /// Root `FeatDefs` schema class from the fixed row prefix.
    pub root_schema_class: Option<u32>,
    /// Absolute offset of the containing `AllFeatur` section. Replay state is
    /// scoped to this stream.
    pub stream_offset: usize,
    /// Row bytes after the compact feature identifier, ending before the next
    /// known feature row or at the end of the section.
    pub body: Vec<u8>,
    /// Byte offset of `body[0]` in the original stream.
    pub body_offset: usize,
    /// Byte offset of the feature identifier in the original stream.
    pub offset: usize,
}

/// One labeled procedural-choice span inside a known feature row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureChoice {
    /// Owning feature row identifier.
    pub feature_id: u32,
    /// Procedural choice label without its NUL terminator.
    pub label: String,
    /// Named-record type byte when the label has an `e0` header.
    pub type_byte: Option<u8>,
    /// Exact bytes from the label terminator to the next choice span.
    pub payload: Vec<u8>,
    /// Byte offset of `payload[0]` in the original stream.
    pub payload_offset: usize,
    /// Byte offset of the choice header or bare label in the original stream.
    pub offset: usize,
}

/// Byte-declared wrapper around one procedural choice field value.
#[derive(Debug, Clone, PartialEq)]
pub enum FeatureFieldValue {
    /// No payload bytes follow the field header.
    Empty,
    /// One compact integer occupying the complete field payload.
    CompactInt(u32),
    /// An `f8` count followed by exactly that many compact integers.
    CompactIntArray(Vec<u32>),
    /// One canonical `f7` entity reference, optionally followed by `fb`.
    EntityReference {
        /// Walker-order entity identifier.
        entity_id: u32,
        /// Whether an `fb` terminator follows the identifier.
        terminated: bool,
    },
    /// An `f9 <dimensions> <count>` scalar-array wrapper and its undecoded body.
    ScalarArray {
        /// Scalar tuple dimensionality from the wrapper.
        dimensions: u32,
        /// Number of scalar tuples from the wrapper.
        count: u32,
        /// Exact scalar-body bytes after the wrapper header.
        body: Vec<u8>,
        /// Values when exactly `dimensions × count` defined scalar tokens
        /// consume the complete body.
        decoded_values: Option<Vec<f64>>,
    },
    /// Bytes whose enclosing field is known but whose wrapper is not.
    Raw(Vec<u8>),
}

/// One named field bounded inside a procedural choice span.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureChoiceField {
    /// Owning feature identifier.
    pub feature_id: u32,
    /// Owning procedural choice label.
    pub choice_label: String,
    /// Field name from its named-record header.
    pub name: String,
    /// Named-record type byte.
    pub type_byte: u8,
    /// Structurally decoded field-value wrapper.
    pub value: FeatureFieldValue,
    /// Byte offset of the named-record header in the original stream.
    pub offset: usize,
}

/// Generated-geometry namespace declared inside a feature row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureGeometryTableKind {
    /// `edg_id_tab_ptr` edge identifiers.
    EdgeIds,
    /// `lo_id_tab_ptr` loop identifiers.
    LoopIds,
    /// `bnd_type` boundary records.
    Boundaries,
    /// `used_bodies` body references.
    UsedBodies,
    /// `geom_lists` geometry-list references.
    GeometryLists,
    /// `dtm_id_tab` datum identifiers.
    DatumIds,
}

/// One typed generated-geometry table header owned by a feature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureGeometryTable {
    /// Owning feature identifier.
    pub feature_id: u32,
    /// Declared namespace kind.
    pub kind: FeatureGeometryTableKind,
    /// Declared entry count.
    pub count: u32,
    /// Entity-class identifier following the `f7` marker.
    pub entity_class: u32,
    /// Complete datum identifiers for a `dtm_id_tab`; other table bodies remain
    /// untyped.
    pub entry_ids: Option<Vec<u32>>,
    /// Byte offset of the field label in the original stream.
    pub offset: usize,
}

/// Namespace of IDs affected by a procedural feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AffectedIdKind {
    /// `geoms_affected` geometry identifiers.
    Geometry,
    /// `edgs_affected` edge identifiers.
    Edges,
    /// `strong_parents` parent-feature identifiers.
    StrongParents,
    /// `parent_table` regeneration-parent feature identifiers.
    Parents,
    /// `contours` contour identifiers.
    Contours,
}

/// One complete affected-ID array owned by a feature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureAffectedIds {
    /// Owning feature identifier.
    pub feature_id: u32,
    /// Affected namespace.
    pub kind: AffectedIdKind,
    /// Declared compact identifiers in stored order.
    pub ids: Vec<u32>,
    /// Byte offset of the named field header in the original stream.
    pub offset: usize,
}

/// Whether an affected-array extent is present or inherited at its schema
/// position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayExtentSource {
    /// An `f8 <count>` opener occurs at this position.
    Explicit,
    /// The position omits `f8` and reuses the preceding extent in this schema
    /// stream.
    Inherited,
}

/// Geometry and edge operands recovered from a class-913 positional replay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureReplayAffectedIds {
    /// Owning feature identifier.
    pub feature_id: u32,
    /// Geometry identifiers at the first affected-array schema position.
    pub geometry_ids: Vec<u32>,
    /// Edge identifiers at the second affected-array schema position.
    pub edge_ids: Vec<u32>,
    /// Encoding of the geometry-array extent.
    pub geometry_extent: ReplayExtentSource,
    /// Encoding of the edge-array extent.
    pub edge_extent: ReplayExtentSource,
    /// Byte offset of the replay anchor in the original stream.
    pub offset: usize,
}

/// Which named direction lane occurs in a loop-restoration record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopRestoreDirectionLane {
    /// `direction`.
    Primary,
    /// `direction2`.
    Secondary,
}

/// One named compact direction value in a loop-restoration record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureLoopRestoreDirection {
    /// Owning feature identifier.
    pub feature_id: u32,
    /// Primary or secondary direction lane.
    pub lane: LoopRestoreDirectionLane,
    /// Complete compact-integer value.
    pub value: u32,
    /// Byte offset of the named field header in the original stream.
    pub offset: usize,
}

/// One ordered feature-local loop identity from a complete `lo_hist` roster.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureLoopHistoryEntry {
    /// Owning feature identifier.
    pub feature_id: u32,
    /// Zero-based position in the feature's loop roster.
    pub ordinal: u32,
    /// Feature-local loop identifier.
    pub loop_id: u32,
    /// Four required row fields and the optional final field, in stored order.
    pub field_bytes: Vec<Vec<u8>>,
    /// Stored row boundary form.
    pub boundary: FeatureLoopHistoryBoundary,
    /// Byte offset of the loop identifier in the original stream.
    pub offset: usize,
    /// Byte offset immediately after the row, excluding a following named header.
    pub end_offset: usize,
}

/// Boundary form terminating one `lo_hist` row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureLoopHistoryBoundary {
    /// Bare `e3` terminator.
    CompoundClose,
    /// `f1 f7 <reference> e3` terminator.
    ReferenceContinue(u32),
    /// `f2 f7 <reference> e3` terminator.
    ReferenceFinal(u32),
    /// The next named-record header bounds the final row.
    NamedRecord,
}

/// Angular termination selected by a rotational feature row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureRevolutionExtentKind {
    /// Complete 360-degree travel.
    FullTurn,
}

/// One resolved rotational extent from an `AllFeatur` feature row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureRevolutionExtent {
    /// Owning feature identifier.
    pub feature_id: u32,
    /// Resolved angular termination.
    pub kind: FeatureRevolutionExtentKind,
    /// Byte offset of the stored `angle_choice` value.
    pub offset: usize,
}

/// Definition-space parameter-frame field in a `FeatDefs` record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureParameterFrameKind {
    /// `local_sys` frame field.
    LocalSystem,
    /// `transf` transform field.
    Transform,
}

/// One `f9 04 03` definition-space parameter frame.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureParameterFrame {
    /// Frame field kind.
    pub kind: FeatureParameterFrameKind,
    /// Exact scalar-body bytes after `f9 04 03`.
    pub body: Vec<u8>,
    /// Twelve values when the body consists entirely of defined scalar tokens.
    pub decoded_values: Option<Vec<f64>>,
    /// Byte offset of the field label in the original stream.
    pub offset: usize,
}

/// One instantiated row from a feature definition's `place_instruction_ptrs`
/// table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeaturePlacementInstruction {
    /// Stored placement instruction family.
    pub kind: u32,
    /// Whether the scalar offset lane stores exact zero.
    pub zero_offset: bool,
    /// Optional driving dimension identifier.
    pub dimension_id: Option<u32>,
    /// Optional referenced placement object.
    pub reference_id: Option<u32>,
    /// First optional geometry operand.
    pub geometry1_id: Option<u32>,
    /// Second optional geometry operand.
    pub geometry2_id: Option<u32>,
    /// First membership selector.
    pub member1: u32,
    /// Second membership selector.
    pub member2: u32,
    /// Byte offset of the positional row marker.
    pub offset: usize,
}

/// Feature-history phase associated with a local outline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutlinePhase {
    /// Labeled `outline` before rollback.
    PreRollback,
    /// Positional replay after rollback.
    PostRollback,
    /// Positional replay after regeneration.
    PostRegen,
}

/// Six-slot feature-local outline bounds.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureOutline {
    /// Feature-history phase.
    pub phase: OutlinePhase,
    /// Six feature-local scalar slots; undefined prefixes remain `None`.
    pub local_values: Vec<Option<f64>>,
    /// Byte offset of the outline label in the original stream.
    pub offset: usize,
}

/// One positional solver-variable row from `var_arr`.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureVariableRow {
    /// Variable class: `1` is section `u`, `2` is section `v`, `3` is radius.
    pub variable_type: u32,
    /// Point or solver-variable key.
    pub key: u32,
    /// Solved value when the scalar token is defined inline.
    pub value: Option<f64>,
    /// Pre-solve estimate when defined inline.
    pub guess: Option<f64>,
    /// Stored solver-known flag.
    pub known: Option<u32>,
    /// Stored solver homogeneity class.
    pub homogeneity: Option<u32>,
    /// Solver unknown identifier from the third trailing compact field.
    pub uvar_id: Option<u32>,
    /// Whether the value used the nine-byte dimension-driven sentinel.
    pub dimension_driven: bool,
    /// Byte offset of the row in the original stream.
    pub offset: usize,
}

/// One section-frame point joined from `var_arr` type-1/type-2 rows.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureSectionPoint {
    /// Shared variable-row key.
    pub point_id: u32,
    /// Section `u` coordinate.
    pub u: Option<f64>,
    /// Section `v` coordinate.
    pub v: Option<f64>,
}

/// Solved section-variable table from one feature definition.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureVariableTable {
    /// Count declared by the `f8` opener.
    pub declared_count: u32,
    /// Entity-table reference following the opener.
    pub entity_ref: Option<u32>,
    /// Positional variable rows in stored order.
    pub rows: Vec<FeatureVariableRow>,
    /// Section points joined by row key.
    pub points: Vec<FeatureSectionPoint>,
    /// Byte offset of the `var_arr` label in the original stream.
    pub offset: usize,
}

impl FeatureVariableTable {
    /// Whether every row declared by the table decoded.
    pub fn is_complete(&self) -> bool {
        usize::try_from(self.declared_count).ok() == Some(self.rows.len())
    }

    /// Reconcile repeated and complementary section-point rows by identity.
    pub fn reconciled_points(&self) -> (BTreeMap<u32, [Option<f64>; 2]>, BTreeSet<u32>) {
        let point_ids = self
            .points
            .iter()
            .map(|point| point.point_id)
            .chain(
                self.rows
                    .iter()
                    .filter_map(|row| matches!(row.variable_type, 1 | 2).then_some(row.key)),
            )
            .collect::<BTreeSet<_>>();
        let mut points = BTreeMap::new();
        let mut ambiguous = BTreeSet::new();
        for point_id in point_ids {
            let mut point = [None; 2];
            let mut conflict = false;
            for coordinate in 0..2 {
                let variable_type = coordinate as u32 + 1;
                let raw_rows = self
                    .rows
                    .iter()
                    .filter(|row| row.key == point_id && row.variable_type == variable_type)
                    .collect::<Vec<_>>();
                let values = if raw_rows.is_empty() {
                    self.points
                        .iter()
                        .filter(|point| point.point_id == point_id)
                        .filter_map(|point| [point.u, point.v][coordinate])
                        .collect::<Vec<_>>()
                } else {
                    raw_rows
                        .into_iter()
                        .filter_map(|row| row.value)
                        .collect::<Vec<_>>()
                };
                let Some(first) = values.first().copied() else {
                    continue;
                };
                let scale = values.iter().map(|value| value.abs()).fold(1.0, f64::max);
                if values
                    .iter()
                    .all(|candidate| (*candidate - first).abs() <= 1e-9 * scale)
                {
                    point[coordinate] = Some(first);
                } else {
                    conflict = true;
                }
            }
            if conflict {
                ambiguous.insert(point_id);
            } else {
                points.insert(point_id, point);
            }
        }
        (points, ambiguous)
    }
}

/// Defined positional segment family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureSegmentKind {
    /// Type `2` line segment.
    Line,
    /// Type `3` circular-arc segment.
    Arc,
    /// Type `5` isolated point entity.
    Point,
}

/// One positional `segtab_ptr` replay row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureSegment {
    /// Line or arc discriminator.
    pub kind: FeatureSegmentKind,
    /// Three direction fields; control-range sentinels remain `None`.
    pub directions: [Option<u32>; 3],
    /// Endpoint IDs into the section variable table. Point entities normalize
    /// their single stored point identifier into both slots.
    pub point_ids: [u32; 2],
    /// Arc center point ID, or `None` for the null sentinel.
    pub center_id: Option<u32>,
    /// Arc orientation field.
    pub arc_orientation: Option<u32>,
    /// Vertical/horizontal constraint field.
    pub vertical_horizontal: Option<u32>,
    /// Radius reference field.
    pub radius_ref: Option<u32>,
    /// Secondary radius reference field.
    pub radius2_ref: Option<u32>,
    /// External segment identifier used by the order table.
    pub external_id: u32,
    /// Byte offset of the positional row in the original stream.
    pub offset: usize,
}

/// One fully framed `segtab_ptr` row outside the core segment-family enum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureOpaqueSegment {
    /// Stored segment-family discriminator.
    pub kind: u32,
    /// Three stored direction fields.
    pub directions: [Option<u32>; 3],
    /// Two stored point fields.
    pub point_ids: [Option<u32>; 2],
    /// Stored center-point field.
    pub center_id: Option<u32>,
    /// Stored arc-orientation field.
    pub arc_orientation: Option<u32>,
    /// Stored vertical/horizontal field.
    pub vertical_horizontal: Option<u32>,
    /// Stored radius-reference field.
    pub radius_ref: Option<u32>,
    /// Stored secondary-radius-reference field.
    pub radius2_ref: Option<u32>,
    /// External segment identifier used by section tables.
    pub external_id: u32,
    /// Byte offset of the row in the original stream.
    pub offset: usize,
}

/// Defining-sketch segment table from one feature definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureSegmentTable {
    /// Count declared by the `f8` opener.
    pub declared_count: u32,
    /// Entity-table reference following the opener.
    pub entity_ref: Option<u32>,
    /// Fully aligned line and arc rows.
    pub rows: Vec<FeatureSegment>,
    /// Fully aligned rows with unsupported segment-family discriminators.
    pub opaque_rows: Vec<FeatureOpaqueSegment>,
    /// Byte offset of the `segtab_ptr` label in the original stream.
    pub offset: usize,
}

impl FeatureSegmentTable {
    /// Whether every row declared by the table decoded.
    pub fn is_complete(&self) -> bool {
        usize::try_from(self.declared_count).ok() == Some(self.rows.len() + self.opaque_rows.len())
    }

    /// Resolve a uniquely identified defining-sketch segment.
    pub fn segment(&self, external_id: u32) -> Option<&FeatureSegment> {
        self.is_complete().then_some(())?;
        let mut matches = self
            .rows
            .iter()
            .filter(|segment| segment.external_id == external_id);
        let segment = matches.next()?;
        (matches.next().is_none()
            && !self
                .opaque_rows
                .iter()
                .any(|row| row.external_id == external_id))
        .then_some(segment)
    }
}

/// Solved/trimmed section entity family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrimEntityKind {
    /// No center vertex: trimmed line.
    Line,
    /// Center vertex present: trimmed circular arc.
    Arc,
}

/// One positional `ent_tab` replay row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureTrimEntity {
    /// External ID matching a `segtab` row.
    pub external_id: u32,
    /// Entity mode field.
    pub mode: Option<u32>,
    /// Solved start and end vertex IDs.
    pub vertices: [u32; 2],
    /// Solved center vertex ID for an arc.
    pub center_vertex: Option<u32>,
    /// Line or arc classification derived from center presence.
    pub kind: TrimEntityKind,
    /// Byte offset of the positional row in the original stream.
    pub offset: usize,
}

/// One stored hash bucket in a native trim table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureTrimBucket {
    /// Zero-based bucket index.
    pub index: u32,
    /// Number of entries declared by the bucket array opener.
    pub declared_entry_count: u32,
    /// Number of structurally complete entries decoded within the bucket frame.
    pub decoded_entry_count: u32,
    /// Byte offset of the stored bucket index.
    pub offset: usize,
}

impl FeatureTrimBucket {
    /// Whether every declared entry has one complete stored body.
    pub fn is_complete(&self) -> bool {
        self.decoded_entry_count == self.declared_entry_count
    }
}

/// Solved/trimmed entity graph for one feature definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureTrimEntityTable {
    /// Count declared by the table opener when present.
    pub declared_count: Option<u32>,
    /// Native table-class reference when present.
    pub entity_ref: Option<u32>,
    /// Native row-class reference when present.
    pub entry_ref: Option<u32>,
    /// Explicit hash buckets decoded in stored order.
    pub buckets: Vec<FeatureTrimBucket>,
    /// Complete positional rows in stored order.
    pub rows: Vec<FeatureTrimEntity>,
    /// Sorted external IDs present in the trimmed profile.
    pub solved_external_ids: Vec<u32>,
    /// Byte offset of the `ent_tab` label in the original stream.
    pub offset: usize,
}

impl FeatureTrimEntityTable {
    /// Whether every declared hash-bucket index was decoded in order.
    pub fn has_complete_bucket_index_sequence(&self) -> bool {
        complete_bucket_index_sequence(self.declared_count, &self.buckets)
    }

    /// Whether every declared bucket and entry body is structurally complete.
    pub fn has_complete_bucket_frame(&self) -> bool {
        self.has_complete_bucket_index_sequence()
            && self.buckets.iter().all(FeatureTrimBucket::is_complete)
    }

    /// Whether each retained external entity identifier occurs once.
    pub fn has_unique_external_ids(&self) -> bool {
        let mut ids = BTreeSet::new();
        self.rows.iter().all(|row| ids.insert(row.external_id))
    }
}

/// One solved trim vertex and the two trimmed entities incident to it.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureTrimVertex {
    /// Vertex identifier shared with `ent_tab` endpoint and center fields.
    pub vertex_id: u32,
    /// Distinct `ent_tab` external entity identifiers meeting at the vertex.
    pub entities: Vec<u32>,
    /// Solved section-frame coordinates for a nonparallel line-line junction.
    pub section_coordinates: Option<[f64; 2]>,
    /// Byte offset of the positional triple in the original stream.
    pub offset: usize,
}

/// Solved trim-vertex adjacency table for one feature definition.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureTrimVertexTable {
    /// Count declared by the table opener when present.
    pub declared_count: Option<u32>,
    /// Native table-class reference when present.
    pub entity_ref: Option<u32>,
    /// Native row-class reference when present.
    pub entry_ref: Option<u32>,
    /// Explicit hash buckets decoded in stored order.
    pub buckets: Vec<FeatureTrimBucket>,
    /// Complete validated vertex rows in stored order.
    pub rows: Vec<FeatureTrimVertex>,
    /// Byte offset of the `vert_tab` label in the original stream.
    pub offset: usize,
}

impl FeatureTrimVertexTable {
    /// Whether every declared hash-bucket index was decoded in order.
    pub fn has_complete_bucket_index_sequence(&self) -> bool {
        complete_bucket_index_sequence(self.declared_count, &self.buckets)
    }

    /// Whether every declared bucket and entry body is structurally complete.
    pub fn has_complete_bucket_frame(&self) -> bool {
        self.has_complete_bucket_index_sequence()
            && self.buckets.iter().all(FeatureTrimBucket::is_complete)
    }
}

fn complete_bucket_index_sequence(
    declared_count: Option<u32>,
    buckets: &[FeatureTrimBucket],
) -> bool {
    declared_count.is_none_or(|count| {
        usize::try_from(count).ok() == Some(buckets.len())
            && buckets.iter().map(|bucket| bucket.index).eq(0..count)
    })
}

/// One generated-entity ordering row from a gsec3d section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureOrderRow {
    /// Section entity identifier matching a defining-sketch segment.
    pub external_id: u32,
    /// One-based position in the feature's generated-entity table.
    pub internal_id: u32,
    /// Orientation and side flags stored for the generated entity.
    pub bitmask: u32,
    /// Byte offset of the positional triple in the original stream.
    pub offset: usize,
}

/// Generated-entity ordering table for one gsec3d section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureOrderTable {
    /// Count declared by the `f8` opener.
    pub declared_count: u32,
    /// Whether `declared_count` includes a structural prototype outside `rows`.
    pub has_prototype: bool,
    /// Entity-table class reference following the opener.
    pub entity_ref: Option<u32>,
    /// Complete positional triples in stored order.
    pub rows: Vec<FeatureOrderRow>,
    /// Byte offset of the `order_table` label in the original stream.
    pub offset: usize,
}

impl FeatureOrderTable {
    /// Whether every entry declared by the table opener was decoded.
    pub fn is_complete(&self) -> bool {
        usize::try_from(self.declared_count).ok()
            == Some(usize::from(self.has_prototype) + self.rows.len())
    }

    /// Resolve a generated-entity position to its section entity identifier.
    pub fn external_id(&self, internal_id: u32) -> Option<u32> {
        self.is_complete().then_some(())?;
        let mut matches = self
            .rows
            .iter()
            .filter(|row| row.internal_id == internal_id);
        let row = matches.next()?;
        (matches.next().is_none()
            && self
                .rows
                .iter()
                .filter(|candidate| candidate.external_id == row.external_id)
                .count()
                == 1)
            .then_some(row.external_id)
    }

    /// Resolve a section entity identifier to its generated-entity position.
    pub fn internal_id(&self, external_id: u32) -> Option<u32> {
        self.is_complete().then_some(())?;
        let mut matches = self
            .rows
            .iter()
            .filter(|row| row.external_id == external_id);
        let row = matches.next()?;
        (matches.next().is_none()
            && self
                .rows
                .iter()
                .filter(|candidate| candidate.internal_id == row.internal_id)
                .count()
                == 1)
            .then_some(row.internal_id)
    }
}

/// Defined value of a one-byte binary section flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryFlag {
    /// Stored byte `00`.
    Clear,
    /// Stored byte `01`.
    Set,
}

impl BinaryFlag {
    fn decode(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Clear),
            1 => Some(Self::Set),
            _ => None,
        }
    }
}

/// Reference fields that orient a gsec3d sketch frame.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FeatureSectionOrientation {
    /// Section-side flip.
    pub section_flip: Option<BinaryFlag>,
    /// Orientation-reference type discriminator.
    pub reference_type: Option<u32>,
    /// Referenced sketch segment identifier.
    pub segment_id: Option<u32>,
    /// Referenced-plane flip.
    pub reference_flip: Option<BinaryFlag>,
}

/// Byte-backed gsec3d placement and ordering inputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureSection3d {
    /// Sketch-plane entity identifier.
    pub sketch_plane_entity_id: Option<u32>,
    /// Sketch-plane side flag.
    pub sketch_plane_flip: Option<BinaryFlag>,
    /// Entity references that orient the sketch plane.
    pub reference_plane_entity_ids: Vec<u32>,
    /// Geometry identifier joining the reference plane to its datum surface.
    pub reference_plane_datum_geometry_id: Option<u32>,
    /// Section-frame orientation reference fields.
    pub orientation: FeatureSectionOrientation,
    /// Stored dimension identifiers in section order.
    pub dimension_ids: Vec<u32>,
    /// Byte offset of the gsec3d record header in the original stream.
    pub offset: usize,
}

/// Interpretation of a stored feature-dimension value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DimensionUnit {
    /// Type `0x0a` angle value stored in radians.
    Radians,
    /// Linear dimension value stored in model millimeters.
    Millimeters,
    /// Dimension type whose unit is defined by its enclosing section schema.
    SchemaDefined,
}

/// One dimension record from a gsec2d `dimtab_ptr` table.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureDimension {
    /// Dimension type discriminator.
    pub dimension_type: u32,
    /// Decoded primary scalar, when its prefix is defined.
    pub value: Option<f64>,
    /// Exact bounded placeholder token when the primary scalar is unresolved.
    pub unresolved_value_token: Option<Vec<u8>>,
    /// Unit interpretation selected by the dimension type.
    pub value_unit: DimensionUnit,
    /// Stored direction byte.
    pub direction_byte: u8,
    /// Decoded auxiliary scalar, when its prefix is defined.
    pub auxiliary_value: Option<f64>,
    /// External dimension identifier.
    pub external_id: u32,
    /// Byte offset of the row in the original stream.
    pub offset: usize,
}

/// Dimension table for one gsec2d section.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureDimensionTable {
    /// Count declared by the `f8` opener.
    pub declared_count: u32,
    /// Entity-table class reference following the opener.
    pub entity_ref: Option<u32>,
    /// Labeled prototype followed by positional replay rows.
    pub rows: Vec<FeatureDimension>,
    /// Byte offset of the `dimtab_ptr` label in the original stream.
    pub offset: usize,
}

/// One positional constraint-relation row from `relat_ptr`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureRelation {
    /// Relation identifier from the first positional field.
    pub relation_id: u32,
    /// Stored `used` field from the second positional field.
    pub used: u32,
    /// Exact encoded `a`, `b`, and `c` operand-vector block.
    pub operands: Vec<u8>,
    /// Decoded four-slot `a`, `b`, and `c` operand vectors.
    pub operand_vectors: Option<[[Option<u32>; 4]; 3]>,
    /// Stored relation sign selector.
    pub sign: u32,
    /// Stored dimension selector.
    pub dimension_id: u32,
    /// Stored relation-type discriminator.
    pub relation_type: u32,
    /// Complete positional fields before the `e2` row terminator.
    pub body: Vec<u8>,
    /// Byte offset of the positional row in the original stream.
    pub offset: usize,
}

/// Counted `relat_ptr` constraint-relation table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureRelationTable {
    /// Allocation count declared by the table's `f8` opener. Two entries are
    /// structural; positional row count is `declared_count - 2`.
    pub declared_count: u32,
    /// Relation entity-class reference following the opener.
    pub entity_ref: Option<u32>,
    /// Complete positional relation rows in stored order.
    pub rows: Vec<FeatureRelation>,
    /// Section-entity incidence records used by solver equations.
    pub skamps: Vec<FeatureSkamp>,
    /// Count, class, and source location of `skamp_ptr`.
    pub skamp_header: Option<FeatureSolverTableHeader>,
    /// Joins between relation, equation, and incidence identifiers.
    pub triples: Vec<FeatureRelationTriple>,
    /// Count, class, and source location of `triples_ptr`.
    pub triples_header: Option<FeatureSolverTableHeader>,
    /// Byte offset of the `relat_ptr` label in the original stream.
    pub offset: usize,
}

/// Header identity for a counted solver subtable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureSolverTableHeader {
    /// Count declared by the table's `f8` opener.
    pub declared_count: u32,
    /// Table-class reference following the count.
    pub entity_ref: u32,
    /// Byte offset of the table label or positional array opener.
    pub offset: usize,
}

/// One entity incidence within a section solver `skamp_ptr` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureSkampItem {
    /// External section-entity identifier.
    pub entity_id: u32,
    /// Stored endpoint or locus selector.
    pub sense: u32,
}

/// One counted section solver `skamp_ptr` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureSkamp {
    /// Incidence identifier referenced by `triples_ptr`.
    pub id: u32,
    /// Stored incidence family.
    pub kind: u32,
    /// Stored flags.
    pub flags: u32,
    /// Stored solver status.
    pub status: u32,
    /// Counted entity incidences in stored order.
    pub items: Vec<FeatureSkampItem>,
    /// Byte offset of the row in the original stream.
    pub offset: usize,
}

/// One `triples_ptr` join between solver namespaces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureRelationTriple {
    /// Relation identifier, or the native null sentinel.
    pub relation_id: Option<u32>,
    /// Equation identifier, or the native null sentinel.
    pub equation_id: Option<u32>,
    /// Incidence identifier, or the native null sentinel.
    pub skamp_id: Option<u32>,
    /// Byte offset of the row in the original stream.
    pub offset: usize,
}

/// One solved line retained in feature-definition section coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureSavedLine {
    /// Saved-section entity identifier.
    pub entity_id: u32,
    /// Entity references preceding or embedded in the record.
    pub references: Vec<u32>,
    /// Five-byte `eb` attribute payloads in stored order.
    pub attributes: Vec<[u8; 5]>,
    /// Two three-dimensional endpoints in the section sketch frame.
    pub endpoints: [[Option<f64>; 3]; 2],
    /// Byte offset of the record preamble in the original stream.
    pub offset: usize,
}

/// One solved circular arc retained in section coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureSavedArc {
    /// Saved-section entity identifier.
    pub entity_id: u32,
    /// Arc center in the section sketch frame.
    pub center: [Option<f64>; 3],
    /// Arc radius.
    pub radius: Option<f64>,
    /// Trimmed arc endpoints in the section sketch frame.
    pub endpoints: [[Option<f64>; 3]; 2],
    /// Start and end curve parameters.
    pub parameters: [Option<f64>; 2],
    /// Byte offset of the entity label in the original stream.
    pub offset: usize,
}

/// One solved circle retained in section coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureSavedCircle {
    /// Saved-section entity identifier.
    pub entity_id: u32,
    /// Circle center in the section sketch frame.
    pub center: [Option<f64>; 3],
    /// Circle radius.
    pub radius: Option<f64>,
    /// Byte offset of the entity label in the original stream.
    pub offset: usize,
}

/// One saved interpolation spline retained in section coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureSavedSpline {
    /// Saved-section entity identifier, when stored.
    pub entity_id: Option<u32>,
    /// Declared interpolation-point count, when its extent is valid.
    pub declared_point_count: Option<u32>,
    /// Complete interpolation-point prefix in stored parameter order.
    pub interpolation_points: Vec<[f64; 3]>,
    /// Two stored endpoint tangent triples, when every scalar is defined.
    pub endpoint_tangents: Option<[[f64; 3]; 2]>,
    /// One stored interpolation parameter per point, when complete.
    pub parameters: Option<Vec<f64>>,
    /// Byte offset of the entity label in the original stream.
    pub offset: usize,
}

/// One saved placeholder entity without analytic geometry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureSavedDummy {
    /// Saved-section entity identifier, when stored.
    pub entity_id: Option<u32>,
    /// Byte offset of the entity label in the original stream.
    pub offset: usize,
}

/// Solved saved-section entity with kind-specific valid fields.
#[derive(Debug, Clone, PartialEq)]
pub enum FeatureSavedEntity {
    /// Saved straight-line entity.
    Line(FeatureSavedLine),
    /// Saved circular-arc entity.
    Arc(FeatureSavedArc),
    /// Saved full-circle entity.
    Circle(FeatureSavedCircle),
    /// Saved interpolation-spline entity.
    Spline(FeatureSavedSpline),
    /// Saved non-geometric placeholder.
    Dummy(FeatureSavedDummy),
}

/// Solved entity table stored below `p_saved_result`.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureSavedSection {
    /// Solved entities in stored table order.
    pub entities: Vec<FeatureSavedEntity>,
    /// Byte offset of the `p_saved_result` record header in the original stream.
    pub offset: usize,
}

/// One byte-bounded feature-definition template or instantiated saved section.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureDefinition {
    /// Numeric identifier embedded in `feat_defs_<id>`. A positional replay
    /// inherits that schema identifier until an exact owner join replaces it
    /// with the canonical feature identifier.
    pub id: u32,
    /// Canonical definition owner, joining the definition to its modeling
    /// feature.
    pub owner_feature_id: Option<u32>,
    /// Exact record bytes through the next feature definition or section end.
    pub body: Vec<u8>,
    /// Definition-space local-system and transform fields.
    pub parameter_frames: Vec<FeatureParameterFrame>,
    /// Feature-local outline records in history order.
    pub outlines: Vec<FeatureOutline>,
    /// Section solver-variable table, when present and structurally valid.
    pub variables: Option<FeatureVariableTable>,
    /// Defining-sketch segment table, when present and structurally valid.
    pub segments: Option<FeatureSegmentTable>,
    /// Solved/trimmed entity graph, when present and structurally valid.
    pub trim_entities: Option<FeatureTrimEntityTable>,
    /// Solved trim-vertex adjacency, when present and structurally valid.
    pub trim_vertices: Option<FeatureTrimVertexTable>,
    /// gsec3d generated-entity ordering, when present and structurally valid.
    pub order_table: Option<FeatureOrderTable>,
    /// gsec3d placement and ordering inputs, when present.
    pub section_3d: Option<FeatureSection3d>,
    /// gsec2d dimension table, when present and structurally valid.
    pub dimensions: Option<FeatureDimensionTable>,
    /// gsec2d constraint-relation table, when present and structurally valid.
    pub relations: Option<FeatureRelationTable>,
    /// Solved saved-section entities, when present and structurally valid.
    pub saved_section: Option<FeatureSavedSection>,
    /// Byte offset of the record name in the original stream.
    pub offset: usize,
}

/// One named record in the implicit `AllFeatur` walker-order entity table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureEntity {
    /// Zero-based walker-order identifier used by `f7` references.
    pub entity_id: u32,
    /// Named-record type byte.
    pub type_byte: u8,
    /// NUL-terminated named-record name.
    pub name: String,
    /// Byte offset of the `e0` header in the original stream.
    pub offset: usize,
}

/// One `f7 <id>` reference in `AllFeatur`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureEntityReference {
    /// Walker-order entity containing this token, when one precedes it.
    pub source_entity_id: Option<u32>,
    /// Referenced walker-order entity identifier.
    pub target_entity_id: u32,
    /// Whether the target identifier exists in the decoded entity table.
    pub target_resolved: bool,
    /// Byte offset of the `f7` token in the original stream.
    pub offset: usize,
}

const ROW_HEADERS: &[&[u8]] = &[&[0xeb, 0x04], &[0x90, 0x01], &[0xc8, 0x10]];
const CHOICE_LABELS: &[&[u8]] = &[
    b"blend_choice",
    b"depth_choice",
    b"angle_choice",
    b"pat_choice",
    b"round_choice",
    b"subsec_choice",
    b"sweep_choice",
    b"dome_choice",
    b"draft_choice",
    b"misc_choice",
];

fn row_spans(payload: &[u8], feature_ids: &BTreeSet<u32>) -> Vec<(usize, usize, u32)> {
    let mut starts = Vec::new();
    for offset in 0..payload.len() {
        let Ok((id, after)) = psb::reference_id(payload, offset) else {
            continue;
        };
        if feature_ids.contains(&id)
            && ROW_HEADERS
                .iter()
                .any(|header| payload.get(after..after + header.len()) == Some(*header))
        {
            starts.push((offset, id));
        }
    }
    starts.sort_unstable();
    let mut seen = BTreeSet::new();
    starts.retain(|(_, id)| seen.insert(*id));
    starts
        .iter()
        .enumerate()
        .map(|(index, &(start, id))| {
            let end = starts
                .get(index + 1)
                .map_or(payload.len(), |&(next, _)| next);
            (start, end, id)
        })
        .collect()
}

/// Decode positional `AllFeatur` rows whose identifiers exist in a decoded
/// model-feature namespace. Unknown feature-like byte sequences remain unclaimed.
pub fn rows(payload: &[u8], feature_ids: &BTreeSet<u32>) -> Vec<FeatureRow> {
    row_spans(payload, feature_ids)
        .into_iter()
        .filter_map(|(start, end, feature_id)| {
            let (_, body_start) = psb::reference_id(payload, start).ok()?;
            let body = payload.get(body_start..end)?;
            let header = payload.get(body_start..body_start + 2)?.try_into().ok()?;
            let root_schema_class = body[..body.len().min(16)]
                .windows(2)
                .position(|window| window == [0xe3, 0xf6])
                .and_then(|relative| {
                    let value_offset = body_start + relative + 2;
                    let (value, after) = psb::compact_int(payload, value_offset);
                    (after > value_offset && payload.get(after) == Some(&0xe1)).then_some(value)
                });
            Some(FeatureRow {
                feature_id,
                header,
                root_schema_class,
                stream_offset: 0,
                body: body.to_vec(),
                body_offset: body_start,
                offset: start,
            })
        })
        .collect()
}

/// Bound recognized procedural-choice labels within decoded feature rows.
pub fn choices(rows: &[FeatureRow]) -> Vec<FeatureChoice> {
    let mut result = Vec::new();
    for row in rows {
        let mut hits = Vec::new();
        for &label in CHOICE_LABELS {
            let mut from = 0;
            while let Some(relative) = row.body.get(from..).and_then(|tail| {
                tail.windows(label.len() + 1)
                    .position(|window| window == [label, b"\0"].concat())
            }) {
                let label_offset = from + relative;
                let (header_offset, type_byte) = if label_offset >= 2
                    && row.body[label_offset - 2] == psb::token::NAMED_RECORD
                {
                    (label_offset - 2, Some(row.body[label_offset - 1]))
                } else {
                    (label_offset, None)
                };
                hits.push((header_offset, label_offset, label, type_byte));
                from = label_offset + label.len() + 1;
            }
        }
        hits.sort_by_key(|hit| hit.0);
        for (index, &(header, label_at, label, type_byte)) in hits.iter().enumerate() {
            let value = label_at + label.len() + 1;
            let end = hits.get(index + 1).map_or(row.body.len(), |hit| hit.0);
            result.push(FeatureChoice {
                feature_id: row.feature_id,
                label: String::from_utf8_lossy(label).into_owned(),
                type_byte,
                payload: row.body[value..end].to_vec(),
                payload_offset: row.body_offset + value,
                offset: row.body_offset + header,
            });
        }
    }
    result.sort_by_key(|choice| choice.offset);
    result
}

fn decode_exact_scalars(
    payload: &[u8],
    slot_count: usize,
    cache: &scalar::ScalarCache,
) -> Option<Vec<f64>> {
    // Each slot decodes at least one payload byte and the whole payload must be
    // consumed, so a valid slot count cannot exceed the payload length.
    bounded_len(slot_count as u64, 1, payload.len())?;
    let mut values = Vec::with_capacity(slot_count);
    let mut cursor = psb::Cursor::new(payload);
    for _ in 0..slot_count {
        values.push(cursor.take_with(|data, pos| scalar::decode_in_lane(data, pos, cache))?);
    }
    (cursor.pos() == payload.len()).then_some(values)
}

fn decode_optional_scalars(
    payload: &[u8],
    mut cursor: usize,
    count: usize,
    cache: &scalar::ScalarCache,
) -> Vec<Option<f64>> {
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        if let Some((value, next)) = scalar::decode_in_lane(payload, cursor, cache) {
            values.push(Some(value));
            cursor = next;
        } else if cursor < payload.len() {
            values.push(None);
            cursor += 1;
        } else {
            values.push(None);
        }
    }
    values
}

fn find_bytes(payload: &[u8], needle: &[u8], start: usize, end: usize) -> Option<usize> {
    payload
        .get(start..end)?
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|relative| start + relative)
}

fn decode_parameter_scalar(
    payload: &[u8],
    offset: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> Option<(f64, usize)> {
    const DICT_PREFIXES: &[u8] = &[
        0x5e, 0x60, 0x68, 0x6f, 0x71, 0x74, 0x81, 0x85, 0x8b, 0x90, 0x91, 0x99, 0xa1, 0xa2, 0xb7,
    ];
    let prefix = *payload.get(offset)?;
    if DICT_PREFIXES.contains(&prefix) && offset + 7 <= end {
        let (first, second) = if prefix == 0xb7 {
            (0x3f, 0xe4)
        } else {
            let second = prefix.wrapping_sub(0x8b);
            (if second >= 0x80 { 0x3f } else { 0x40 }, second)
        };
        let mut raw = [0; 8];
        raw[0] = first;
        raw[1] = second;
        raw[2..].copy_from_slice(&payload[offset + 1..offset + 7]);
        return Some((f64::from_be_bytes(raw), offset + 7));
    }
    if let Some((value, next)) =
        scalar::decode_in_lane(payload, offset, cache).filter(|(_, next)| *next <= end)
    {
        return Some((value, next));
    }
    None
}

fn decode_variable_scalar(
    payload: &[u8],
    offset: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> (Option<f64>, usize, bool) {
    let Some(&prefix) = payload.get(offset).filter(|_| offset < end) else {
        return (None, offset, false);
    };
    if matches!(prefix, 0x90 | 0xd7) && offset + 7 <= end {
        let mut raw = [0; 8];
        raw[..2].copy_from_slice(if prefix == 0x90 {
            &[0x40, 0x05]
        } else {
            &[0xc0, 0x05]
        });
        raw[2..].copy_from_slice(&payload[offset + 1..offset + 7]);
        return (Some(f64::from_be_bytes(raw)), offset + 7, false);
    }
    if prefix == 0xd5 && offset + 7 <= end {
        let mut raw = [0; 8];
        raw[0] = 0xbf;
        raw[1..7].copy_from_slice(&payload[offset + 1..offset + 7]);
        return (Some(f64::from_be_bytes(raw)), offset + 7, false);
    }
    if prefix == 0x28 && offset + 8 <= end {
        let mut raw = [0; 8];
        raw[0] = 0x3f;
        raw[1..].copy_from_slice(&payload[offset + 1..offset + 8]);
        return (Some(f64::from_be_bytes(raw)), offset + 8, false);
    }
    if prefix == 0x31 && offset + 7 <= end {
        let mut raw = [0; 8];
        raw[0] = 0x40;
        raw[1..7].copy_from_slice(&payload[offset + 1..offset + 7]);
        return (Some(f64::from_be_bytes(raw)), offset + 7, false);
    }
    let variable_dict = match prefix {
        0x53..=0xa3 => Some((0x3f75_u16 + u16::from(prefix)).to_be_bytes()),
        0xad => Some([0x3f, 0xd9]),
        0xb3 => Some([0xbf, 0xe0]),
        0xc6 => Some([0xbf, 0xf3]),
        0xc8 => Some([0xbf, 0xf5]),
        0xcb => Some([0xbf, 0xf8]),
        0xcc => Some([0xbf, 0xf9]),
        0xd0 => Some([0xbf, 0xfe]),
        0xd2 => Some([0xc0, 0x00]),
        0xd6 => Some([0xc0, 0x04]),
        0xdd => Some([0xc0, 0x0c]),
        _ => None,
    };
    if let (Some(head), Some(tail)) = (variable_dict, payload.get(offset + 1..offset + 7)) {
        let mut raw = [0; 8];
        raw[..2].copy_from_slice(&head);
        raw[2..].copy_from_slice(tail);
        return (Some(f64::from_be_bytes(raw)), offset + 7, false);
    }
    if prefix == 0x18
        && payload
            .get(offset + 1)
            .is_some_and(|next| matches!(next, 0x18 | 0xe0 | 0xe2 | 0xe3 | 0x10 | 0xe4 | 0xe6))
    {
        return (Some(0.0), offset + 1, false);
    }
    if prefix == 0xed && offset + 9 <= end {
        return (None, offset + 9, true);
    }
    decode_parameter_scalar(payload, offset, end, cache)
        .map_or((None, offset + 1, false), |(value, next)| {
            (Some(value), next, false)
        })
}

fn decode_section_coordinate_scalar(
    payload: &[u8],
    offset: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> (Option<f64>, usize, bool) {
    if payload.get(offset) == Some(&0x2d) && offset + 8 <= end {
        let mut raw = [0; 8];
        raw[0] = 0x40;
        raw[1..].copy_from_slice(&payload[offset + 1..offset + 8]);
        return (Some(f64::from_be_bytes(raw)), offset + 8, false);
    }
    decode_variable_scalar(payload, offset, end, cache)
}

fn variable_table(
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> Option<FeatureVariableTable> {
    let table = find_bytes(payload, b"var_arr\0", start, end)?;
    let mut cursor = table + b"var_arr\0".len();
    (payload.get(cursor) == Some(&psb::token::ARRAY_OPEN)).then_some(())?;
    let (declared_count, after_count) = psb::compact_int(payload, cursor + 1);
    cursor = after_count;
    let entity_ref = if payload.get(cursor) == Some(&psb::token::ENTITY_REF) {
        let (value, next) = psb::compact_int(payload, cursor + 1);
        cursor = next;
        Some(value)
    } else {
        None
    };
    let close = find_bytes(payload, &[0xf1, psb::token::ENTITY_REF], cursor, end)?;
    let named_row = (|| {
        let type_label = find_bytes(payload, b"type\0", cursor, close)?;
        let variable_type = named_compact_int(payload, b"type\0", cursor, close)?;
        let key = named_compact_int(payload, b"key\0", cursor, close)?;
        let value_label = find_bytes(payload, b"value\0", cursor, close)? + b"value\0".len();
        let (value, _, dimension_driven) =
            decode_section_coordinate_scalar(payload, value_label, close, cache);
        let guess_label = find_bytes(payload, b"guess\0", cursor, close)? + b"guess\0".len();
        let (guess, _, _) = decode_section_coordinate_scalar(payload, guess_label, close, cache);
        Some(FeatureVariableRow {
            variable_type,
            key,
            value,
            guess,
            known: named_compact_int(payload, b"known\0", cursor, close),
            homogeneity: named_compact_int(payload, b"homogeneity\0", cursor, close),
            uvar_id: named_compact_int(payload, b"uvar_id\0", cursor, close),
            dimension_driven,
            offset: type_label.saturating_sub(2),
        })
    })();
    let (_, after_close_ref) = psb::compact_int(payload, close + 2);
    cursor = after_close_ref;
    if payload.get(cursor) == Some(&0xe2) {
        cursor += 1;
    }
    let mut rows = named_row.into_iter().collect::<Vec<_>>();
    let max_rows = usize::try_from(declared_count)
        .unwrap_or(usize::MAX)
        .min(end.saturating_sub(cursor));
    while cursor < end && rows.len() < max_rows {
        if payload[cursor] == 0xe2 {
            cursor += 1;
            continue;
        }
        if payload[cursor] >= 0xc0 {
            break;
        }
        let row_offset = cursor;
        let (variable_type, next) = psb::compact_int(payload, cursor);
        cursor = next;
        if cursor >= end || payload[cursor] >= 0xc0 {
            break;
        }
        let (key, next) = psb::compact_int(payload, cursor);
        cursor = next;
        let (value, next, dimension_driven) =
            decode_section_coordinate_scalar(payload, cursor, end, cache);
        cursor = next;
        let (guess, next, _) = decode_section_coordinate_scalar(payload, cursor, end, cache);
        cursor = next;
        let mut trailing = Vec::new();
        while cursor < end && payload[cursor] != 0xe2 && trailing.len() < 3 {
            if payload[cursor] >= 0xc0 {
                break;
            }
            let (field, next) = psb::compact_int(payload, cursor);
            if next == cursor {
                break;
            }
            trailing.push(field);
            cursor = next;
        }
        let row = FeatureVariableRow {
            variable_type,
            key,
            value,
            guess,
            known: trailing.first().copied(),
            homogeneity: trailing.get(1).copied(),
            uvar_id: trailing.get(2).copied(),
            dimension_driven,
            offset: row_offset,
        };
        let Some(delimiter) = payload[cursor..end].iter().position(|&byte| byte == 0xe2) else {
            break;
        };
        cursor += delimiter + 1;
        rows.push(row);
    }
    Some(variable_table_from_rows(
        declared_count,
        entity_ref,
        rows,
        table,
    ))
}

fn variable_table_from_rows(
    declared_count: u32,
    entity_ref: Option<u32>,
    rows: Vec<FeatureVariableRow>,
    offset: usize,
) -> FeatureVariableTable {
    let mut coordinates = BTreeMap::<u32, (Option<f64>, Option<f64>)>::new();
    for row in rows.iter().filter(|row| matches!(row.variable_type, 1 | 2)) {
        coordinates.entry(row.key).or_insert((None, None));
    }
    for (&point_id, point) in &mut coordinates {
        let mut u_rows = rows
            .iter()
            .filter(|row| row.key == point_id && row.variable_type == 1);
        let u = u_rows.next();
        if u_rows.next().is_none() {
            point.0 = u.and_then(|row| row.value);
        }
        let mut v_rows = rows
            .iter()
            .filter(|row| row.key == point_id && row.variable_type == 2);
        let v = v_rows.next();
        if v_rows.next().is_none() {
            point.1 = v.and_then(|row| row.value);
        }
    }
    FeatureVariableTable {
        declared_count,
        entity_ref,
        rows,
        points: coordinates
            .into_iter()
            .map(|(point_id, (u, v))| FeatureSectionPoint { point_id, u, v })
            .collect(),
        offset,
    }
}

fn positional_variable_table(
    payload: &[u8],
    start: usize,
    end: usize,
    table_class: u32,
    cache: &scalar::ScalarCache,
) -> Option<FeatureVariableTable> {
    let (table, declared_count, mut cursor, reference_bytes) = (start..end).find_map(|table| {
        (payload.get(table) == Some(&psb::token::ARRAY_OPEN)).then_some(())?;
        let (declared_count, after_count) = psb::compact_int(payload, table + 1);
        (payload.get(after_count) == Some(&psb::token::ENTITY_REF)).then_some(())?;
        let (class, after_reference) = psb::reference_id(payload, after_count + 1).ok()?;
        (class == table_class
            && payload.get(after_reference..after_reference + 2) == Some(&[0xfb, 0xe2]))
        .then(|| {
            (
                table,
                declared_count,
                after_reference + 2,
                payload[after_count + 1..after_reference].to_vec(),
            )
        })
    })?;
    (payload.get(cursor) == Some(&psb::token::ENTITY_REF)).then_some(())?;
    let (_, after_row_class) = psb::reference_id(payload, cursor + 1).ok()?;
    cursor = after_row_class;

    let row_limit = usize::try_from(declared_count).unwrap_or(usize::MAX);
    // Each row consumes at least one byte before its 0xe2 separator, so the row
    // count cannot exceed the unread bytes in the table window.
    let capacity =
        bounded_len(u64::from(declared_count), 1, end.saturating_sub(cursor)).unwrap_or(0);
    let mut rows = Vec::with_capacity(capacity);
    let mut prototype_separator = vec![0xf1, psb::token::ENTITY_REF];
    prototype_separator.extend_from_slice(&reference_bytes);
    prototype_separator.push(0xe2);
    'rows: while cursor < end && rows.len() < row_limit {
        let row_offset = cursor;
        let (variable_type, next) = psb::compact_int(payload, cursor);
        cursor = next;
        let (key, next) = psb::compact_int(payload, cursor);
        cursor = next;
        let (value, next, dimension_driven) =
            decode_section_coordinate_scalar(payload, cursor, end, cache);
        cursor = next;
        let (guess, next, _) = decode_section_coordinate_scalar(payload, cursor, end, cache);
        cursor = next;
        let mut trailing = Vec::with_capacity(3);
        while cursor < end && payload[cursor] != 0xe2 && trailing.len() < 3 {
            if payload[cursor] >= 0xc0 {
                break 'rows;
            }
            let (field, next) = psb::compact_int(payload, cursor);
            if next <= cursor {
                break 'rows;
            }
            trailing.push(field);
            cursor = next;
        }
        let row = FeatureVariableRow {
            variable_type,
            key,
            value,
            guess,
            known: trailing.first().copied(),
            homogeneity: trailing.get(1).copied(),
            uvar_id: trailing.get(2).copied(),
            dimension_driven,
            offset: row_offset,
        };
        if rows.len() + 1 < row_limit {
            if rows.is_empty() {
                if payload.get(cursor..cursor + prototype_separator.len())
                    != Some(prototype_separator.as_slice())
                {
                    break;
                }
                cursor += prototype_separator.len();
            } else {
                if payload.get(cursor) != Some(&0xe2) {
                    break;
                }
                cursor += 1;
            }
        }
        rows.push(row);
    }
    Some(variable_table_from_rows(
        declared_count,
        Some(table_class),
        rows,
        table,
    ))
}

fn segment_int(payload: &[u8], offset: usize) -> (Option<u32>, usize) {
    let Some(&head) = payload.get(offset) else {
        return (None, offset);
    };
    match head {
        0..=0x7f => (Some(u32::from(head)), offset + 1),
        0x80..=0xbf => payload.get(offset + 1).map_or((None, offset + 1), |&tail| {
            (
                Some((u32::from(head - 0x80) << 8) | u32::from(tail)),
                offset + 2,
            )
        }),
        _ => (None, offset + 1),
    }
}

fn next_segment_int(payload: &[u8], offset: &mut usize) -> Option<u32> {
    let (value, next) = segment_int(payload, *offset);
    *offset = next;
    value
}

fn next_solver_int(payload: &[u8], offset: &mut usize) -> Option<u32> {
    let &head = payload.get(*offset)?;
    if (0xc0..=0xdf).contains(&head) {
        let high = *payload.get(*offset + 1)?;
        let low = *payload.get(*offset + 2)?;
        *offset += 3;
        return Some((u32::from(head - 0xc0) << 16) | (u32::from(high) << 8) | u32::from(low));
    }
    next_segment_int(payload, offset)
}

fn next_nullable_segment_int(payload: &[u8], offset: &mut usize) -> Result<Option<u32>, ()> {
    if payload.get(*offset) == Some(&0xf6) {
        *offset += 1;
        return Ok(None);
    }
    next_segment_int(payload, offset).map(Some).ok_or(())
}

fn segment_slots(payload: &[u8], offset: &mut usize, count: usize) -> Option<Vec<Option<u32>>> {
    let mut values = Vec::with_capacity(count);
    while values.len() < count {
        match *payload.get(*offset)? {
            0xe4 => {
                values.push(Some(1));
                *offset += 1;
            }
            0xe5 => {
                (values.len() + 2 <= count).then_some(())?;
                values.extend([Some(0), Some(0)]);
                *offset += 1;
            }
            0xe6 => {
                (values.len() + 3 <= count).then_some(())?;
                values.extend([Some(0), Some(0), Some(0)]);
                *offset += 1;
            }
            0xf6 => {
                values.push(None);
                *offset += 1;
            }
            _ => values.push(Some(next_segment_int(payload, offset)?)),
        }
    }
    Some(values)
}

/// Decode instantiated placement-instruction rows from one bounded feature
/// definition.
pub fn placement_instructions(definition: &FeatureDefinition) -> Vec<FeaturePlacementInstruction> {
    placement_instruction_rows(&definition.body, definition.offset)
}

fn placement_instruction_rows(
    payload: &[u8],
    definition_offset: usize,
) -> Vec<FeaturePlacementInstruction> {
    let Some(table_class) =
        named_array_class(payload, b"place_instruction_ptrs\0", 0, payload.len())
    else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    for marker in 0..payload.len() {
        if payload.get(marker..marker + 2) != Some(&[0xf1, psb::token::ENTITY_REF]) {
            continue;
        }
        let Ok((class, after_class)) = psb::reference_id(payload, marker + 2) else {
            continue;
        };
        if class != table_class || payload.get(after_class) != Some(&psb::token::COMPOUND_CLOSE) {
            continue;
        }
        let mut cursor = after_class + 1;
        let Some(kind) = next_solver_int(payload, &mut cursor) else {
            continue;
        };
        let zero_offset = payload.get(cursor) == Some(&0x18);
        if !zero_offset {
            continue;
        }
        cursor += 1;
        let Ok(dimension_id) = next_nullable_segment_int(payload, &mut cursor) else {
            continue;
        };
        let Ok(reference_id) = next_nullable_segment_int(payload, &mut cursor) else {
            continue;
        };
        let Ok(geometry1_id) = next_nullable_segment_int(payload, &mut cursor) else {
            continue;
        };
        let Ok(geometry2_id) = next_nullable_segment_int(payload, &mut cursor) else {
            continue;
        };
        let Some(member1) = next_segment_int(payload, &mut cursor) else {
            continue;
        };
        let Some(member2) = next_segment_int(payload, &mut cursor) else {
            continue;
        };
        rows.push(FeaturePlacementInstruction {
            kind,
            zero_offset,
            dimension_id,
            reference_id,
            geometry1_id,
            geometry2_id,
            member1,
            member2,
            offset: definition_offset + marker,
        });
    }
    rows
}

fn segment_table(payload: &[u8], start: usize, end: usize) -> Option<FeatureSegmentTable> {
    let table = find_bytes(payload, b"segtab_ptr\0", start, end)?;
    let mut cursor = table + b"segtab_ptr\0".len();
    while payload
        .get(cursor)
        .is_some_and(|byte| matches!(byte, 0xf1..=0xf3))
    {
        cursor += 1;
    }
    segment_table_body(payload, table, cursor, end)
}

fn positional_segment_table(
    payload: &[u8],
    start: usize,
    end: usize,
) -> Option<FeatureSegmentTable> {
    let name_end = find_bytes(payload, b"S2D", start, start.saturating_add(256).min(end))?;
    let cursor = payload[name_end..end].iter().position(|&byte| byte == 0)? + name_end + 1;
    segment_table_body(payload, cursor, cursor, end)
}

fn segment_table_body(
    payload: &[u8],
    table: usize,
    mut cursor: usize,
    end: usize,
) -> Option<FeatureSegmentTable> {
    (payload.get(cursor) == Some(&psb::token::ARRAY_OPEN)).then_some(())?;
    let (declared_count, after_count) = psb::compact_int(payload, cursor + 1);
    cursor = after_count;
    let entity_ref = if payload.get(cursor) == Some(&psb::token::ENTITY_REF) {
        let (value, next) = psb::compact_int(payload, cursor + 1);
        cursor = next;
        Some(value)
    } else {
        None
    };
    let close = find_bytes(payload, &[0xf2, psb::token::ENTITY_REF], cursor, end)?;
    let named_values = |label: &[u8], count: usize| -> Option<(usize, Vec<Option<u32>>)> {
        let offset = find_bytes(payload, label, cursor, close)?;
        let mut p = offset + label.len();
        if payload.get(p) == Some(&psb::token::ARRAY_OPEN) {
            let (declared, next) = psb::compact_int(payload, p + 1);
            (usize::try_from(declared).ok()? == count).then_some(())?;
            p = next;
        }
        if label == b"type\0" && payload.get(p..p + 2) == Some(&[0xc0, 0x80]) {
            p += 2;
        }
        let values = segment_slots(payload, &mut p, count)?;
        Some((offset, values))
    };
    let named_row = (|| {
        let (offset, kind) = named_values(b"type\0", 1)?;
        let (_, directions) = named_values(b"dir\0", 3)?;
        let (_, point_ids) = named_values(b"pointid\0", 2)?;
        let (_, center_id) = named_values(b"cntrid\0", 1)?;
        let (_, arc_orientation) = named_values(b"arcorient\0", 1)?;
        let (_, vertical_horizontal) = named_values(b"verhor\0", 1)?;
        let (_, radius_ref) = named_values(b"radius\0", 1)?;
        let (_, radius2_ref) = named_values(b"radius2\0", 1)?;
        let (_, external_id) = named_values(b"ext_id\0", 1)?;
        Some(FeatureOpaqueSegment {
            kind: kind[0]?,
            directions: [directions[0], directions[1], directions[2]],
            point_ids: [point_ids[0], point_ids[1]],
            center_id: center_id[0],
            arc_orientation: arc_orientation[0],
            vertical_horizontal: vertical_horizontal[0],
            radius_ref: radius_ref[0],
            radius2_ref: radius2_ref[0],
            external_id: external_id[0]?,
            offset,
        })
    })();
    let (_, after_close_ref) = psb::compact_int(payload, close + 2);
    cursor = after_close_ref;
    if payload.get(cursor) == Some(&0xe2) {
        cursor += 1;
    }
    let region_end = [
        b"order_table".as_slice(),
        b"dimtab_ptr\0",
        b"relat_ptr\0",
        b"var_arr\0",
        b"gsec3d_ptr\0",
        b"order_ptr\0",
        b"p_saved_result\0",
        b"S2D",
    ]
    .into_iter()
    .filter_map(|label| find_bytes(payload, label, cursor, end))
    .min()
    .unwrap_or(end);
    let mut rows = Vec::new();
    let mut opaque_rows = Vec::new();
    if let Some(row) = named_row {
        retain_segment_row(row, &mut rows, &mut opaque_rows);
    }
    let first_row = cursor;
    let row_limit = usize::try_from(declared_count).unwrap_or(usize::MAX);
    while cursor < region_end && rows.len() + opaque_rows.len() < row_limit {
        let row_start = cursor;
        let kind_offset = if matches!(
            payload.get(cursor..cursor + 2),
            Some([0xc0, 0x80] | [0xc1, 0x00])
        ) {
            cursor + 2
        } else {
            cursor
        };
        if payload.get(kind_offset).is_none_or(|kind| *kind > 0x7f)
            || (row_start != first_row
                && payload.get(row_start.saturating_sub(1)) != Some(&0xe3)
                && payload.get(row_start.saturating_sub(4)..row_start)
                    != Some(&[0xe2, 0x00, 0xf6, 0xe2]))
        {
            cursor += 1;
            continue;
        }
        let mut p = kind_offset;
        let (kind_raw, next) = segment_int(payload, p);
        p = next;
        let Some(kind) = kind_raw else {
            cursor += 1;
            continue;
        };
        let Some(prefix) = segment_slots(payload, &mut p, 7)
            .and_then(|values| <[Option<u32>; 7]>::try_from(values).ok())
        else {
            cursor += 1;
            continue;
        };
        let directions = [prefix[0], prefix[1], prefix[2]];
        let point0 = prefix[3];
        let point1 = prefix[4];
        let center_id = prefix[5];
        let arc_orientation = prefix[6];
        let verhor_flag = payload.get(p) == Some(&0xf5);
        let vertical_horizontal = if verhor_flag {
            p += 1;
            if segment_slots(payload, &mut p, 1).is_none() {
                cursor += 1;
                continue;
            }
            None
        } else {
            let Some(values) = segment_slots(payload, &mut p, 1) else {
                cursor += 1;
                continue;
            };
            values[0]
        };
        let Some(suffix) = segment_slots(payload, &mut p, 3)
            .and_then(|values| <[Option<u32>; 3]>::try_from(values).ok())
        else {
            cursor += 1;
            continue;
        };
        let radius_ref = suffix[0];
        let radius2_ref = suffix[1];
        let Some(external_id) = suffix[2] else {
            cursor += 1;
            continue;
        };
        if payload.get(p) == Some(&0xe2) {
            retain_segment_row(
                FeatureOpaqueSegment {
                    kind,
                    directions,
                    point_ids: [point0, point1],
                    center_id,
                    arc_orientation,
                    vertical_horizontal,
                    radius_ref,
                    radius2_ref,
                    external_id,
                    offset: row_start,
                },
                &mut rows,
                &mut opaque_rows,
            );
            cursor = p + 1;
        } else {
            cursor += 1;
        }
    }
    Some(FeatureSegmentTable {
        declared_count,
        entity_ref,
        rows,
        opaque_rows,
        offset: table,
    })
}

fn retain_segment_row(
    row: FeatureOpaqueSegment,
    rows: &mut Vec<FeatureSegment>,
    opaque_rows: &mut Vec<FeatureOpaqueSegment>,
) {
    let kind = match row.kind {
        2 => FeatureSegmentKind::Line,
        3 => FeatureSegmentKind::Arc,
        5 => FeatureSegmentKind::Point,
        _ => {
            opaque_rows.push(row);
            return;
        }
    };
    let Some(point0) = row.point_ids[0] else {
        return;
    };
    let point1 = if kind == FeatureSegmentKind::Point {
        point0
    } else {
        let Some(point1) = row.point_ids[1] else {
            return;
        };
        point1
    };
    rows.push(FeatureSegment {
        kind,
        directions: row.directions,
        point_ids: [point0, point1],
        center_id: row.center_id,
        arc_orientation: row.arc_orientation,
        vertical_horizontal: row.vertical_horizontal,
        radius_ref: row.radius_ref,
        radius2_ref: row.radius2_ref,
        external_id: row.external_id,
        offset: row.offset,
    });
}

fn trim_entity_table(payload: &[u8], start: usize, end: usize) -> Option<FeatureTrimEntityTable> {
    let table = find_bytes(payload, b"ent_tab\0", start, end)?;
    let header = trim_table_header(payload, b"ent_tab\0", start, end);
    let prototype = find_bytes(payload, b"entry_ptr(entity_entry)", table, end)?;
    let mut cursor = header
        .and_then(|header| {
            (prototype..end).find_map(|offset| {
                (payload.get(offset..offset + 3) == Some(&[0xf4, 0x04, psb::token::ENTITY_REF]))
                    .then_some(())?;
                let (class, after_reference) = psb::reference_id(payload, offset + 3).ok()?;
                (class == header.classes.table && payload.get(after_reference) == Some(&0xe2))
                    .then_some(after_reference + 1)
            })
        })
        .or_else(|| {
            let close = find_bytes(payload, &[0xf2, psb::token::ENTITY_REF], prototype, end)?;
            let (_, after_reference) = psb::reference_id(payload, close + 2).ok()?;
            Some(after_reference)
        })?;
    if payload.get(cursor) == Some(&0xe3) {
        cursor += 1;
    }
    let first_row = cursor;
    let region_end = find_bytes(payload, b"vert_tab", cursor, end).unwrap_or(end);
    let buckets = header.map_or_else(Vec::new, |header| {
        trim_buckets(payload, table, region_end, header, TrimEntryKind::Entity)
    });
    let mut rows = Vec::new();
    let mut seen = BTreeSet::new();
    while cursor < region_end {
        if cursor != first_row && payload.get(cursor.saturating_sub(1)) != Some(&0xe3) {
            cursor += 1;
            continue;
        }
        let row_offset = cursor;
        let mut p = row_offset;
        let external_id = next_segment_int(payload, &mut p);
        let mode = next_segment_int(payload, &mut p);
        let start_vertex = next_segment_int(payload, &mut p);
        let end_vertex = next_segment_int(payload, &mut p);
        let center_vertex = next_segment_int(payload, &mut p);
        if let (Some(external_id), Some(start_vertex), Some(end_vertex)) =
            (external_id, start_vertex, end_vertex)
        {
            if external_id != 0 && payload.get(p) == Some(&0) {
                seen.insert(external_id);
                rows.push(FeatureTrimEntity {
                    external_id,
                    mode,
                    vertices: [start_vertex, end_vertex],
                    center_vertex,
                    kind: if center_vertex.is_some() {
                        TrimEntityKind::Arc
                    } else {
                        TrimEntityKind::Line
                    },
                    offset: row_offset,
                });
            }
        }
        cursor += 1;
    }
    Some(FeatureTrimEntityTable {
        declared_count: header.map(|header| header.declared_count),
        entity_ref: header.map(|header| header.classes.table),
        entry_ref: header.map(|header| header.classes.entry),
        buckets,
        solved_external_ids: seen.into_iter().collect(),
        rows,
        offset: table,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TrimTableClasses {
    table: u32,
    bucket: u32,
    entry: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TrimTableHeader {
    declared_count: u32,
    classes: TrimTableClasses,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrimEntryKind {
    Entity,
    Vertex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TrimBucketStart {
    index: u32,
    declared_entry_count: u32,
    offset: usize,
    body_start: usize,
}

fn trim_buckets(
    payload: &[u8],
    table: usize,
    end: usize,
    header: TrimTableHeader,
    kind: TrimEntryKind,
) -> Vec<FeatureTrimBucket> {
    if header.declared_count == 0 {
        return Vec::new();
    }
    let Some(label) = find_bytes(payload, b"bucket_index\0", table, end) else {
        return Vec::new();
    };
    let first_offset = label + b"bucket_index\0".len();
    let (Some(first), mut cursor) = segment_int(payload, first_offset) else {
        return Vec::new();
    };
    if first != 0 {
        return Vec::new();
    }
    let Some((first_count, first_body)) =
        named_trim_bucket_count(payload, cursor, end, header.classes.bucket)
    else {
        return Vec::new();
    };
    let mut starts = vec![TrimBucketStart {
        index: first,
        declared_entry_count: first_count,
        offset: first_offset,
        body_start: first_body,
    }];
    while starts.len() < usize::try_from(header.declared_count).unwrap_or(usize::MAX) {
        let expected = u32::try_from(starts.len()).unwrap_or(u32::MAX);
        let Some((offset, index, next)) = (cursor..end).find_map(|offset| {
            (payload.get(offset.saturating_sub(1)) == Some(&0xe2)).then_some(())?;
            let (Some(index), next) = segment_int(payload, offset) else {
                return None;
            };
            (index == expected).then_some((offset, index, next))
        }) else {
            break;
        };
        let Some((declared_entry_count, body_start)) =
            positional_trim_bucket_count(payload, next, end, header.classes)
        else {
            break;
        };
        starts.push(TrimBucketStart {
            index,
            declared_entry_count,
            offset,
            body_start,
        });
        cursor = next;
    }
    starts
        .iter()
        .enumerate()
        .map(|(position, start)| {
            let body_end = starts
                .get(position + 1)
                .map_or(end, |next| next.offset.saturating_sub(1));
            FeatureTrimBucket {
                index: start.index,
                declared_entry_count: start.declared_entry_count,
                decoded_entry_count: trim_bucket_entry_count(
                    payload,
                    start.body_start,
                    body_end,
                    header.classes,
                    kind,
                    position == 0,
                ),
                offset: start.offset,
            }
        })
        .collect()
}

fn named_trim_bucket_count(
    payload: &[u8],
    start: usize,
    end: usize,
    bucket_class: u32,
) -> Option<(u32, usize)> {
    let label = find_bytes(payload, b"bucket_xar\0", start, end)? + b"bucket_xar\0".len();
    let opener = (label..end).find(|&offset| payload[offset] == psb::token::ARRAY_OPEN)?;
    trim_bucket_array_count(payload, opener, bucket_class)
}

fn positional_trim_bucket_count(
    payload: &[u8],
    mut cursor: usize,
    end: usize,
    classes: TrimTableClasses,
) -> Option<(u32, usize)> {
    match payload.get(cursor)? {
        &psb::token::ARRAY_OPEN => trim_bucket_array_count(payload, cursor, classes.bucket),
        0xf0 => {
            (payload.get(cursor + 1) == Some(&psb::token::ENTITY_REF)).then_some(())?;
            let (class, next) = psb::reference_id(payload, cursor + 2).ok()?;
            (class == classes.bucket).then_some(())?;
            cursor = next;
            (payload.get(cursor) == Some(&psb::token::ARRAY_OPEN)).then_some(())?;
            trim_bucket_array_count(payload, cursor, classes.bucket)
        }
        0xf1 => {
            (payload.get(cursor + 1) == Some(&psb::token::ENTITY_REF)).then_some(())?;
            let (class, next) = psb::reference_id(payload, cursor + 2).ok()?;
            (class == classes.table && payload.get(next) == Some(&0xe2)).then_some((0, next + 1))
        }
        0xe2 | 0xe0 if cursor < end => Some((0, cursor + 1)),
        _ => None,
    }
}

fn trim_bucket_array_count(
    payload: &[u8],
    opener: usize,
    bucket_class: u32,
) -> Option<(u32, usize)> {
    let (count, after_count) = psb::compact_int(payload, opener + 1);
    (payload.get(after_count) == Some(&psb::token::ENTITY_REF)).then_some(())?;
    let (class, after_reference) = psb::reference_id(payload, after_count + 1).ok()?;
    (class == bucket_class
        && payload.get(after_reference..after_reference + 2) == Some(&[0xfb, 0xe3]))
    .then_some((count, after_reference + 2))
}

fn trim_bucket_entry_count(
    payload: &[u8],
    start: usize,
    end: usize,
    classes: TrimTableClasses,
    kind: TrimEntryKind,
    named_first: bool,
) -> u32 {
    match kind {
        TrimEntryKind::Entity => {
            let rows = (start..end)
                .filter(|&offset| {
                    payload.get(offset.saturating_sub(1)) == Some(&0xe3)
                        && complete_trim_entity_entry(payload, offset, end)
                })
                .count();
            let prototype = usize::from(
                named_first && named_trim_entity_prototype_complete(payload, start, end, classes),
            );
            u32::try_from(rows.saturating_add(prototype)).unwrap_or(u32::MAX)
        }
        TrimEntryKind::Vertex => {
            let mut rows = BTreeSet::new();
            for offset in start..end {
                if payload.get(offset) == Some(&psb::token::ENTITY_REF) {
                    if let Ok((class, row)) = psb::reference_id(payload, offset + 1) {
                        if class == classes.entry && trim_vertex_entry(payload, row, end).is_some()
                        {
                            rows.insert(row);
                        }
                    }
                }
                if payload.get(offset.saturating_sub(1)) == Some(&0xe3)
                    && trim_vertex_entry(payload, offset, end).is_some()
                {
                    rows.insert(offset);
                }
            }
            let prototype = usize::from(
                named_first && named_trim_vertex_prototype_complete(payload, start, end, classes),
            );
            u32::try_from(rows.len().saturating_add(prototype)).unwrap_or(u32::MAX)
        }
    }
}

fn complete_trim_entity_entry(payload: &[u8], offset: usize, end: usize) -> bool {
    let mut cursor = offset;
    for _ in 0..5 {
        let Some(next) = trim_entry_field(payload, cursor, end) else {
            return false;
        };
        cursor = next;
    }
    cursor < end && payload.get(cursor) == Some(&0)
}

fn trim_vertex_entry(payload: &[u8], offset: usize, end: usize) -> Option<(Vec<u32>, u32, usize)> {
    let mut cursor = offset;
    if payload.get(cursor) == Some(&psb::token::ARRAY_OPEN) {
        let (count, next) = psb::compact_int(payload, cursor + 1);
        cursor = next;
        let mut entities = Vec::with_capacity(usize::try_from(count).ok()?);
        for _ in 0..count {
            let (value, next) = segment_int(payload, cursor);
            entities.push(value?);
            (next <= end).then_some(())?;
            cursor = next;
        }
        let (vertex_id, next) = segment_int(payload, cursor);
        let vertex_id = vertex_id?;
        return (next < end && payload.get(next) == Some(&0)).then_some((
            entities,
            vertex_id,
            next + 1,
        ));
    }
    let mut values = Vec::new();
    while cursor < end && payload.get(cursor) != Some(&0) {
        let (value, next) = segment_int(payload, cursor);
        values.push(value?);
        (next <= end).then_some(())?;
        cursor = next;
        if values.len() > 64 {
            return None;
        }
    }
    let vertex_id = values.pop()?;
    (values.len() >= 2 && cursor < end).then_some((values, vertex_id, cursor + 1))
}

fn trim_entry_field(payload: &[u8], offset: usize, end: usize) -> Option<usize> {
    let &head = payload.get(offset)?;
    let next = match head {
        0..=0x7f | 0xf6 => offset + 1,
        0x80..=0xbf if offset + 1 < end => offset + 2,
        _ => return None,
    };
    (next <= end).then_some(next)
}

fn named_trim_entity_prototype_complete(
    payload: &[u8],
    start: usize,
    end: usize,
    classes: TrimTableClasses,
) -> bool {
    let entry_label = b"entry_ptr(entity_entry)\0";
    let Some(entry) = find_bytes(payload, entry_label, start, end) else {
        return false;
    };
    let mut cursor = entry + entry_label.len();
    if payload.get(cursor) != Some(&0xe3) {
        return false;
    }
    cursor += 1;
    let labels = [
        b"xid\0".as_slice(),
        b"ent_mode\0",
        b"start_vtx\0",
        b"end_vtx\0",
        b"center_vtx\0",
        b"pers_attribs\0",
    ];
    for label in labels {
        let Some(offset) = find_bytes(payload, label, cursor, end) else {
            return false;
        };
        let Some(next) = trim_entry_field(payload, offset + label.len(), end) else {
            return false;
        };
        cursor = next;
    }
    (cursor..end).any(|offset| {
        if payload.get(offset..offset + 3) != Some(&[0xf4, 0x04, psb::token::ENTITY_REF]) {
            return false;
        }
        psb::reference_id(payload, offset + 3)
            .is_ok_and(|(class, next)| class == classes.table && payload.get(next) == Some(&0xe2))
    })
}

fn named_trim_vertex_prototype_complete(
    payload: &[u8],
    start: usize,
    end: usize,
    classes: TrimTableClasses,
) -> bool {
    let Some(entity_ids) = find_bytes(payload, b"ent_ids\0", start, end) else {
        return false;
    };
    let array = entity_ids + b"ent_ids\0".len();
    if payload.get(array) != Some(&psb::token::ARRAY_OPEN) {
        return false;
    }
    let (count, mut cursor) = psb::compact_int(payload, array + 1);
    if count < 2 {
        return false;
    }
    for _ in 0..count {
        let (value, next) = segment_int(payload, cursor);
        if value.is_none() || next > end {
            return false;
        }
        cursor = next;
    }
    let Some(vertex_id) = find_bytes(payload, b"vertex_id\0", cursor, end) else {
        return false;
    };
    let (vertex, next) = segment_int(payload, vertex_id + b"vertex_id\0".len());
    if vertex.is_none() || next > end {
        return false;
    }
    let Some(attributes) = find_bytes(payload, b"attribs\0", next, end) else {
        return false;
    };
    let Some(next) = trim_entry_field(payload, attributes + b"attribs\0".len(), end) else {
        return false;
    };
    (next..end).any(|offset| {
        if payload.get(offset..offset + 2) != Some(&[0xf3, psb::token::ENTITY_REF]) {
            return false;
        }
        psb::reference_id(payload, offset + 2)
            .is_ok_and(|(class, next)| class == classes.table && payload.get(next) == Some(&0xe2))
    })
}

fn trim_table_header(
    payload: &[u8],
    label: &[u8],
    start: usize,
    end: usize,
) -> Option<TrimTableHeader> {
    let table = find_bytes(payload, label, start, end)? + label.len();
    let opener = (table..end).find(|&offset| payload[offset] == psb::token::ARRAY_OPEN)?;
    let (declared_count, after_count) = psb::compact_int(payload, opener + 1);
    (payload.get(after_count) == Some(&psb::token::ENTITY_REF)).then_some(())?;
    let (table_class, _) = psb::reference_id(payload, after_count + 1).ok()?;
    let bucket_label = find_bytes(payload, b"bucket_xar\0", table, end)? + b"bucket_xar\0".len();
    let bucket_opener =
        (bucket_label..end).find(|&offset| payload[offset] == psb::token::ARRAY_OPEN)?;
    let (_, after_bucket_count) = psb::compact_int(payload, bucket_opener + 1);
    (payload.get(after_bucket_count) == Some(&psb::token::ENTITY_REF)).then_some(())?;
    let (bucket_class, _) = psb::reference_id(payload, after_bucket_count + 1).ok()?;
    let entry_class = (after_count..end).find_map(|offset| {
        (payload.get(offset) == Some(&psb::token::ENTITY_REF)).then_some(())?;
        let (class, after_reference) = psb::reference_id(payload, offset + 1).ok()?;
        if label == b"vert_tab\0" {
            let (first, next) = segment_int(payload, after_reference);
            let (second, next) = segment_int(payload, next);
            let (third, next) = segment_int(payload, next);
            return (class != table_class
                && first.is_some()
                && second.is_some()
                && third.is_some()
                && payload.get(next) == Some(&0))
            .then_some(class);
        }
        (payload.get(after_reference..after_reference + 2) == Some(&[0, 0xe3])).then_some(class)
    })?;
    Some(TrimTableHeader {
        declared_count,
        classes: TrimTableClasses {
            table: table_class,
            bucket: bucket_class,
            entry: entry_class,
        },
    })
}

fn positional_table_region(
    payload: &[u8],
    start: usize,
    end: usize,
    table_class: u32,
    next_table_class: Option<u32>,
) -> Option<(usize, u32, usize, usize)> {
    let (table, declared_count, rows_start) = (start..end).find_map(|table| {
        (payload.get(table) == Some(&psb::token::ARRAY_OPEN)).then_some(())?;
        let (declared_count, after_count) = psb::compact_int(payload, table + 1);
        (payload.get(after_count) == Some(&psb::token::ENTITY_REF)).then_some(())?;
        let (class, after_reference) = psb::reference_id(payload, after_count + 1).ok()?;
        (class == table_class
            && payload.get(after_reference..after_reference + 2) == Some(&[0xfb, 0xe2]))
        .then_some((table, declared_count, after_reference + 2))
    })?;
    let region_end = next_table_class
        .and_then(|next_class| {
            (rows_start..end).find(|&offset| {
                if payload.get(offset) != Some(&psb::token::ARRAY_OPEN) {
                    return false;
                }
                let (_, after_count) = psb::compact_int(payload, offset + 1);
                if payload.get(after_count) != Some(&psb::token::ENTITY_REF) {
                    return false;
                }
                psb::reference_id(payload, after_count + 1).is_ok_and(|(class, after_reference)| {
                    class == next_class
                        && payload.get(after_reference..after_reference + 2) == Some(&[0xfb, 0xe2])
                })
            })
        })
        .unwrap_or(end);
    Some((table, declared_count, rows_start, region_end))
}

fn positional_trim_entity_table(
    payload: &[u8],
    start: usize,
    end: usize,
    classes: TrimTableClasses,
    next_table_class: Option<u32>,
) -> Option<FeatureTrimEntityTable> {
    let TrimTableClasses {
        table: table_class,
        entry: entry_class,
        ..
    } = classes;
    let (table, declared_count, rows_start, region_end) =
        positional_table_region(payload, start, end, table_class, next_table_class)?;
    let mut rows = Vec::new();
    let mut seen = BTreeSet::new();
    let has_entry_class = (rows_start..region_end).any(|offset| {
        if payload.get(offset) != Some(&psb::token::ENTITY_REF) {
            return false;
        }
        psb::reference_id(payload, offset + 1).is_ok_and(|(class, after_reference)| {
            class == entry_class
                && payload.get(after_reference..after_reference + 2) == Some(&[0, 0xe3])
        })
    });
    let mut cursor = if declared_count == 0 || has_entry_class {
        rows_start
    } else {
        region_end
    };
    while cursor < region_end {
        if cursor == rows_start || payload.get(cursor.saturating_sub(1)) != Some(&0xe3) {
            cursor += 1;
            continue;
        }
        let row_offset = cursor;
        let mut p = row_offset;
        let external_id = next_segment_int(payload, &mut p);
        let mode = next_segment_int(payload, &mut p);
        let start_vertex = next_segment_int(payload, &mut p);
        let end_vertex = next_segment_int(payload, &mut p);
        let center_vertex = next_segment_int(payload, &mut p);
        if let (Some(external_id), Some(start_vertex), Some(end_vertex)) =
            (external_id, start_vertex, end_vertex)
        {
            if external_id != 0 && payload.get(p) == Some(&0) {
                seen.insert(external_id);
                rows.push(FeatureTrimEntity {
                    external_id,
                    mode,
                    vertices: [start_vertex, end_vertex],
                    center_vertex,
                    kind: if center_vertex.is_some() {
                        TrimEntityKind::Arc
                    } else {
                        TrimEntityKind::Line
                    },
                    offset: row_offset,
                });
            }
        }
        cursor += 1;
    }
    Some(FeatureTrimEntityTable {
        declared_count: Some(declared_count),
        entity_ref: Some(table_class),
        entry_ref: Some(entry_class),
        buckets: trim_buckets(
            payload,
            table,
            region_end,
            TrimTableHeader {
                declared_count,
                classes,
            },
            TrimEntryKind::Entity,
        ),
        solved_external_ids: seen.into_iter().collect(),
        rows,
        offset: table,
    })
}

fn trim_vertex_table(
    payload: &[u8],
    start: usize,
    end: usize,
    segments: Option<&FeatureSegmentTable>,
    variables: Option<&FeatureVariableTable>,
) -> Option<FeatureTrimVertexTable> {
    let table = find_bytes(payload, b"vert_tab\0", start, end)?;
    let header = trim_table_header(payload, b"vert_tab\0", start, end);
    let region_end = [
        b"skamp_ptr\0".as_slice(),
        b"triples_ptr\0",
        b"order_table\0",
        b"dimtab_ptr\0",
        b"relat_ptr\0",
        b"p_saved_result\0",
        b"S2D",
    ]
    .into_iter()
    .filter_map(|label| find_bytes(payload, label, table + b"vert_tab\0".len(), end))
    .min()
    .unwrap_or(end);
    let chains_end = table
        .saturating_add(b"vert_tab\0".len())
        .saturating_add(120)
        .min(end);
    let chains = find_bytes(payload, b"chains\0", table, chains_end)?;
    let mut cursor = chains + b"chains\0".len();
    (payload.get(cursor) == Some(&psb::token::ARRAY_OPEN)).then_some(())?;
    let (_, after_count) = psb::compact_int(payload, cursor + 1);
    cursor = after_count;
    (payload.get(cursor) == Some(&psb::token::ENTITY_REF)).then_some(())?;
    let reference_start = cursor + 1;
    let (_, reference_end) = psb::reference_id(payload, reference_start).ok()?;
    let mut block_marker = vec![0xf3, psb::token::ENTITY_REF];
    block_marker.extend_from_slice(payload.get(reference_start..reference_end)?);
    block_marker.push(0xe2);
    cursor = find_bytes(payload, &block_marker, reference_end, region_end)?;

    let mut rows = Vec::new();
    while cursor < region_end {
        if payload.get(cursor..cursor + block_marker.len()) == Some(block_marker.as_slice()) {
            cursor += block_marker.len();
            let (_, next) = segment_int(payload, cursor);
            cursor = next;
            continue;
        }
        match payload[cursor] {
            psb::token::ARRAY_OPEN => {
                if let Some((entities, vertex_id, next)) =
                    trim_vertex_entry(payload, cursor, region_end)
                {
                    rows.push(FeatureTrimVertex {
                        section_coordinates: trim_vertex_intersection(
                            &entities, segments, variables,
                        ),
                        vertex_id,
                        entities,
                        offset: cursor,
                    });
                    cursor = next;
                } else {
                    let (_, next) = psb::compact_int(payload, cursor + 1);
                    cursor = next;
                }
                continue;
            }
            psb::token::ENTITY_REF => {
                let Ok((class, next)) = psb::reference_id(payload, cursor + 1) else {
                    cursor += 1;
                    continue;
                };
                if header.is_some_and(|header| class == header.classes.entry) {
                    if let Some((entities, vertex_id, after_entry)) =
                        trim_vertex_entry(payload, next, region_end)
                    {
                        rows.push(FeatureTrimVertex {
                            section_coordinates: trim_vertex_intersection(
                                &entities, segments, variables,
                            ),
                            vertex_id,
                            entities,
                            offset: next,
                        });
                        cursor = after_entry;
                        continue;
                    }
                }
                cursor = next;
                continue;
            }
            0x00 | 0xf1 | 0xe2 | 0xe3 | 0xfb => {
                cursor += 1;
                continue;
            }
            _ => {}
        }
        let row_offset = cursor;
        let Some((entities, vertex_id, next)) = trim_vertex_entry(payload, cursor, region_end)
        else {
            cursor += 1;
            continue;
        };
        rows.push(FeatureTrimVertex {
            section_coordinates: trim_vertex_intersection(&entities, segments, variables),
            vertex_id,
            entities,
            offset: row_offset,
        });
        cursor = next;
    }
    Some(FeatureTrimVertexTable {
        declared_count: header.map(|header| header.declared_count),
        entity_ref: header.map(|header| header.classes.table),
        entry_ref: header.map(|header| header.classes.entry),
        buckets: header.map_or_else(Vec::new, |header| {
            trim_buckets(payload, table, region_end, header, TrimEntryKind::Vertex)
        }),
        rows,
        offset: table,
    })
}

fn positional_trim_vertex_table(
    payload: &[u8],
    start: usize,
    end: usize,
    classes: TrimTableClasses,
    segments: Option<&FeatureSegmentTable>,
    variables: Option<&FeatureVariableTable>,
) -> Option<FeatureTrimVertexTable> {
    let TrimTableClasses {
        table: table_class,
        entry: entry_class,
        ..
    } = classes;
    let (table, declared_count, rows_start, region_end) =
        positional_table_region(payload, start, end, table_class, None)?;
    let mut rows = Vec::new();
    let mut cursor = rows_start;
    while cursor < region_end {
        if payload.get(cursor) != Some(&psb::token::ENTITY_REF) {
            cursor += 1;
            continue;
        }
        let Ok((class, after_reference)) = psb::reference_id(payload, cursor + 1) else {
            cursor += 1;
            continue;
        };
        if class != entry_class {
            cursor += 1;
            continue;
        }
        let row_offset = after_reference;
        let Some((entities, vertex_id, next)) = trim_vertex_entry(payload, row_offset, region_end)
        else {
            cursor += 1;
            continue;
        };
        rows.push(FeatureTrimVertex {
            section_coordinates: trim_vertex_intersection(&entities, segments, variables),
            vertex_id,
            entities,
            offset: row_offset,
        });
        cursor = next.max(cursor + 1);
    }
    Some(FeatureTrimVertexTable {
        declared_count: Some(declared_count),
        entity_ref: Some(table_class),
        entry_ref: Some(entry_class),
        buckets: trim_buckets(
            payload,
            table,
            region_end,
            TrimTableHeader {
                declared_count,
                classes,
            },
            TrimEntryKind::Vertex,
        ),
        rows,
        offset: table,
    })
}

fn trim_vertex_intersection(
    entities: &[u32],
    segments: Option<&FeatureSegmentTable>,
    variables: Option<&FeatureVariableTable>,
) -> Option<[f64; 2]> {
    let [first, second] = entities else {
        return None;
    };
    (first != second)
        .then(|| entity_intersection([*first, *second], segments, variables))
        .flatten()
}

fn entity_intersection(
    entity_ids: [u32; 2],
    segments: Option<&FeatureSegmentTable>,
    variables: Option<&FeatureVariableTable>,
) -> Option<[f64; 2]> {
    let segments = segments?;
    let variables = variables?;
    let (points, _) = variables.reconciled_points();
    let point = |point_id| {
        let point = points.get(&point_id)?;
        Some([point[0]?, point[1]?])
    };
    let first = segments.segment(entity_ids[0])?;
    let second = segments.segment(entity_ids[1])?;
    let shared = first
        .point_ids
        .into_iter()
        .filter(|point_id| second.point_ids.contains(point_id))
        .collect::<BTreeSet<_>>();
    if let [point_id] = shared.iter().copied().collect::<Vec<_>>().as_slice() {
        return point(*point_id);
    }
    if first.kind != FeatureSegmentKind::Line || second.kind != FeatureSegmentKind::Line {
        return None;
    }
    let [x1, y1] = point(first.point_ids[0])?;
    let [x2, y2] = point(first.point_ids[1])?;
    let [x3, y3] = point(second.point_ids[0])?;
    let [x4, y4] = point(second.point_ids[1])?;
    let denominator = (x1 - x2).mul_add(y3 - y4, -(y1 - y2) * (x3 - x4));
    if denominator == 0.0 {
        return None;
    }
    let first_cross = x1.mul_add(y2, -(y1 * x2));
    let second_cross = x3.mul_add(y4, -(y3 * x4));
    Some([
        first_cross.mul_add(x3 - x4, -(x1 - x2) * second_cross) / denominator,
        first_cross.mul_add(y3 - y4, -(y1 - y2) * second_cross) / denominator,
    ])
}

fn order_table(payload: &[u8], start: usize, end: usize) -> Option<FeatureOrderTable> {
    let table = find_bytes(payload, b"order_table\0", start, end)?;
    let mut cursor = table + b"order_table\0".len();
    (payload.get(cursor) == Some(&psb::token::ARRAY_OPEN)).then_some(())?;
    let (declared_count, next) = psb::compact_int(payload, cursor + 1);
    cursor = next;
    let entity_ref = if payload.get(cursor) == Some(&psb::token::ENTITY_REF) {
        let (value, next) = psb::reference_id(payload, cursor + 1).ok()?;
        cursor = next;
        Some(value)
    } else {
        None
    };
    let close = find_bytes(payload, &[0xf1, psb::token::ENTITY_REF], cursor, end)?;
    let prototype = (|| {
        let mut field = cursor;
        for label in [b"ext_id\0".as_slice(), b"int_id\0", b"bitmask\0"] {
            let offset = find_bytes(payload, label, field, close)?;
            let (_, next) = segment_int(payload, offset + label.len());
            (next > offset + label.len() && next <= close).then_some(())?;
            field = next;
        }
        Some(())
    })();
    let (_, next) = psb::reference_id(payload, close + 2).ok()?;
    cursor = next;
    if payload.get(cursor) == Some(&0xe2) {
        cursor += 1;
    }
    let mut rows = Vec::new();
    let mut external_ids = BTreeSet::new();
    let mut internal_ids = BTreeSet::new();
    let row_limit = usize::try_from(declared_count.saturating_sub(u32::from(prototype.is_some())))
        .unwrap_or(usize::MAX);
    while cursor < end && rows.len() < row_limit {
        if payload[cursor] == 0xe2 {
            cursor += 1;
            continue;
        }
        if matches!(payload[cursor], 0xe0 | 0xf1) {
            break;
        }
        let row_offset = cursor;
        let (external_id, next) = segment_int(payload, cursor);
        let (internal_id, next) = segment_int(payload, next);
        let (bitmask, next) = segment_int(payload, next);
        let (Some(external_id), Some(internal_id), Some(bitmask)) =
            (external_id, internal_id, bitmask)
        else {
            break;
        };
        let row_separator = payload.get(next) == Some(&0xe2);
        let table_boundary = next == end
            || payload
                .get(next)
                .is_some_and(|byte| matches!(byte, 0xe0 | 0xf1 | 0xf3));
        if (!row_separator && !table_boundary)
            || !external_ids.insert(external_id)
            || !internal_ids.insert(internal_id)
        {
            break;
        }
        rows.push(FeatureOrderRow {
            external_id,
            internal_id,
            bitmask,
            offset: row_offset,
        });
        if !row_separator {
            break;
        }
        cursor = next + 1;
    }
    Some(FeatureOrderTable {
        declared_count,
        has_prototype: prototype.is_some(),
        entity_ref,
        rows,
        offset: table,
    })
}

fn positional_order_table(
    payload: &[u8],
    start: usize,
    end: usize,
    table_class: u32,
) -> Option<FeatureOrderTable> {
    let (table, declared_count, cursor) = (start..end).find_map(|table| {
        (payload.get(table) == Some(&psb::token::ARRAY_OPEN)).then_some(())?;
        let (declared_count, after_count) = psb::compact_int(payload, table + 1);
        (payload.get(after_count) == Some(&psb::token::ENTITY_REF)).then_some(())?;
        let (class, after_reference) = psb::reference_id(payload, after_count + 1).ok()?;
        (class == table_class
            && payload.get(after_reference..after_reference + 2) == Some(&[0xfb, 0xe2]))
        .then_some((table, declared_count, after_reference + 2))
    })?;
    let prototype = (|| {
        (payload.get(cursor) == Some(&psb::token::ENTITY_REF)).then_some(())?;
        let (_, mut prototype) = psb::reference_id(payload, cursor + 1).ok()?;
        for _ in 0..3 {
            let (_, next) = segment_int(payload, prototype);
            (next > prototype).then_some(())?;
            prototype = next;
        }
        (payload.get(prototype..prototype + 2) == Some(&[0xf1, psb::token::ENTITY_REF]))
            .then_some(())?;
        let (class, after_reference) = psb::reference_id(payload, prototype + 2).ok()?;
        (class == table_class && payload.get(after_reference) == Some(&0xe2)).then_some(())?;
        Some(after_reference + 1)
    })();
    let row_limit = usize::try_from(declared_count.saturating_sub(1)).unwrap_or(usize::MAX);
    // Each row consumes at least one byte before its 0xe2 separator, so the row
    // count cannot exceed the unread bytes in the table window.
    let capacity = bounded_len(
        u64::from(declared_count.saturating_sub(1)),
        1,
        end.saturating_sub(prototype.unwrap_or(end)),
    )
    .unwrap_or(0);
    let mut rows = Vec::with_capacity(capacity);
    let mut cursor = prototype.unwrap_or(end);
    let mut external_ids = BTreeSet::new();
    let mut internal_ids = BTreeSet::new();
    while cursor < end && rows.len() < row_limit {
        let row_offset = cursor;
        let (external_id, next) = segment_int(payload, cursor);
        let (internal_id, next) = segment_int(payload, next);
        let (bitmask, next) = segment_int(payload, next);
        let (Some(external_id), Some(internal_id), Some(bitmask)) =
            (external_id, internal_id, bitmask)
        else {
            break;
        };
        if !external_ids.insert(external_id) || !internal_ids.insert(internal_id) {
            break;
        }
        let row = FeatureOrderRow {
            external_id,
            internal_id,
            bitmask,
            offset: row_offset,
        };
        cursor = next;
        if rows.len() + 1 == row_limit {
            rows.push(row);
            break;
        }
        if payload.get(cursor) != Some(&0xe2) {
            break;
        }
        cursor += 1;
        rows.push(row);
    }
    Some(FeatureOrderTable {
        declared_count,
        has_prototype: prototype.is_some(),
        entity_ref: Some(table_class),
        rows,
        offset: table,
    })
}

fn named_compact_int(payload: &[u8], label: &[u8], start: usize, end: usize) -> Option<u32> {
    let at = find_bytes(payload, label, start, end)? + label.len();
    let (value, next) = segment_int(payload, at);
    value.filter(|_| next <= end)
}

fn section_3d(payload: &[u8], start: usize, end: usize) -> Option<FeatureSection3d> {
    let section = find_bytes(payload, b"\xe0\x00gsec3d_ptr\0", start, end)?;
    let nearby_end = section.saturating_add(260).min(end);
    let sketch_plane_entity_id = named_compact_int(payload, b"plane_id\0", section, nearby_end);
    let sketch_plane_flip = find_bytes(payload, b"plane_flip\0", section, nearby_end)
        .and_then(|at| payload.get(at + b"plane_flip\0".len()).copied())
        .and_then(BinaryFlag::decode);

    let mut reference_plane_entity_ids = Vec::new();
    let mut reference_plane_datum_geometry_id = None;
    if let Some(references) = find_bytes(payload, b"\xe0\x00ref_planes\0", section, nearby_end) {
        let mut cursor = references + b"\xe0\x00ref_planes\0".len();
        if payload.get(cursor) == Some(&psb::token::ARRAY_OPEN) {
            let (count, next) = psb::compact_int(payload, cursor + 1);
            cursor = next;
            for _ in 0..count {
                if payload.get(cursor) != Some(&psb::token::ENTITY_REF) {
                    break;
                }
                let Ok((entity_id, next)) = psb::reference_id(payload, cursor + 1) else {
                    break;
                };
                reference_plane_entity_ids.push(entity_id);
                cursor = next;
            }
            let nested_end = cursor.saturating_add(48).min(end);
            reference_plane_datum_geometry_id =
                named_compact_int(payload, b"\xe0\x01plane_id\0", cursor, nested_end);
        }
    }

    let placement_end = find_bytes(payload, b"\xe0\x00p_saved_result\0", section, end)
        .unwrap_or_else(|| section.saturating_add(400).min(end));
    let named_flag = |label: &[u8]| {
        find_bytes(payload, label, section, placement_end)
            .and_then(|at| payload.get(at + label.len()).copied())
            .and_then(BinaryFlag::decode)
    };
    let orientation = FeatureSectionOrientation {
        section_flip: named_flag(b"\xe0\x01flip\0"),
        reference_type: named_compact_int(payload, b"\xe0\x01ref_type\0", section, placement_end),
        segment_id: named_compact_int(payload, b"\xe0\x01seg_id\0", section, placement_end),
        reference_flip: named_flag(b"\xe0\x01flip_flag\0"),
    };

    let mut dimension_ids = Vec::new();
    if let Some(table) = find_bytes(payload, b"dim_id_tab\0", section, end) {
        let mut cursor = table + b"dim_id_tab\0".len();
        while payload
            .get(cursor)
            .is_some_and(|byte| matches!(byte, 0xf1..=0xf3))
        {
            cursor += 1;
        }
        if payload.get(cursor) == Some(&psb::token::ARRAY_OPEN) {
            let (count, next) = psb::compact_int(payload, cursor + 1);
            cursor = next;
            for _ in 0..count {
                let (Some(value), next) = segment_int(payload, cursor) else {
                    break;
                };
                dimension_ids.push(value);
                cursor = next;
            }
        }
    }
    Some(FeatureSection3d {
        sketch_plane_entity_id,
        sketch_plane_flip,
        reference_plane_entity_ids,
        reference_plane_datum_geometry_id,
        orientation,
        dimension_ids,
        offset: section,
    })
}

fn positional_section_3d(payload: &[u8], start: usize, end: usize) -> Option<FeatureSection3d> {
    let (section, name_end) = payload[start..end]
        .windows(4)
        .enumerate()
        .filter(|(_, window)| *window == b"\x07S2D")
        .find_map(|(relative, _)| {
            let section = start + relative;
            let name_end = payload[section + 1..end]
                .iter()
                .position(|&byte| byte == 0)?
                + section
                + 1;
            Some((section, name_end))
        })?;
    let mut result = FeatureSection3d {
        sketch_plane_entity_id: None,
        sketch_plane_flip: None,
        reference_plane_entity_ids: Vec::new(),
        reference_plane_datum_geometry_id: None,
        orientation: FeatureSectionOrientation::default(),
        dimension_ids: Vec::new(),
        offset: section,
    };
    let mut cursor = name_end + 1;
    let Some(section_flip) = payload.get(cursor).copied() else {
        return Some(result);
    };
    result.orientation.section_flip = BinaryFlag::decode(section_flip);
    cursor += 1;
    for _ in 0..3 {
        let (_, next) = segment_int(payload, cursor);
        if next <= cursor {
            return Some(result);
        }
        cursor = next;
    }
    let (sketch_plane_entity_id, next) = segment_int(payload, cursor);
    if next <= cursor {
        return Some(result);
    }
    result.sketch_plane_entity_id = sketch_plane_entity_id;
    cursor = next;
    let Some(sketch_plane_flip) = payload.get(cursor).copied() else {
        return Some(result);
    };
    result.sketch_plane_flip = BinaryFlag::decode(sketch_plane_flip);
    cursor += 1;
    if payload.get(cursor) != Some(&psb::token::ARRAY_OPEN) {
        return Some(result);
    }
    let (reference_count, next) = psb::compact_int(payload, cursor + 1);
    if next <= cursor + 1 {
        return Some(result);
    }
    cursor = next;
    if payload.get(cursor) != Some(&psb::token::ENTITY_REF) {
        return Some(result);
    }
    let table_reference_start = cursor + 1;
    let Ok((_, next)) = psb::reference_id(payload, table_reference_start) else {
        return Some(result);
    };
    let table_reference = payload[table_reference_start..next].to_vec();
    cursor = next;
    if payload.get(cursor..cursor + 2) != Some(&[0xfb, 0xe2]) {
        return Some(result);
    }
    cursor += 2;
    if payload.get(cursor) != Some(&psb::token::ENTITY_REF) {
        return Some(result);
    }
    let Ok((_, next)) = psb::reference_id(payload, cursor + 1) else {
        return Some(result);
    };
    cursor = next;

    let row_count = usize::try_from(reference_count).unwrap_or(usize::MAX);
    let mut separator = vec![0xf2, psb::token::ENTITY_REF];
    separator.extend_from_slice(&table_reference);
    separator.push(0xe2);
    for row in 0..row_count {
        let (Some(plane_id), next) = segment_int(payload, cursor) else {
            break;
        };
        cursor = next;
        let (reference_type, next) = segment_int(payload, cursor);
        if next <= cursor {
            break;
        }
        cursor = next;
        let (_, next) = segment_int(payload, cursor);
        if next <= cursor {
            break;
        }
        cursor = next;
        let (segment_id, next) = segment_int(payload, cursor);
        if next <= cursor {
            break;
        }
        cursor = next;
        let (_, next) = segment_int(payload, cursor);
        if next <= cursor {
            break;
        }
        cursor = next;
        let reference_flip = payload.get(cursor).copied().and_then(BinaryFlag::decode);
        let (_, next) = segment_int(payload, cursor);
        if next <= cursor {
            break;
        }
        cursor = next;
        result.reference_plane_entity_ids.push(plane_id);
        if row == 0 {
            result.orientation.reference_type = reference_type;
            result.orientation.segment_id = segment_id;
            result.orientation.reference_flip = reference_flip;
        }
        if row + 1 < row_count {
            let Some(separator_at) = find_bytes(payload, &separator, cursor, end) else {
                break;
            };
            cursor = separator_at + separator.len();
        }
    }
    Some(result)
}

fn dimension_unit(dimension_type: u32) -> DimensionUnit {
    match dimension_type {
        0x0a => DimensionUnit::Radians,
        0x01..=0x05 => DimensionUnit::Millimeters,
        _ => DimensionUnit::SchemaDefined,
    }
}

fn unresolved_dimension_value_token(bytes: &[u8]) -> Option<Vec<u8>> {
    match bytes {
        [0x00, _, _] | [0x01, _, _, _] => Some(bytes.to_vec()),
        _ => None,
    }
}

fn labeled_dimension(
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> Option<FeatureDimension> {
    let type_label = find_bytes(payload, b"type\0", start, end)?;
    let (dimension_type, after_type) = segment_int(payload, type_label + b"type\0".len());
    let dimension_type = dimension_type?;
    let value_label = find_bytes(payload, b"value\0", after_type, end)?;
    let value_start = value_label + b"value\0".len();
    let (value, after_value, _) = decode_variable_scalar(payload, value_start, end, cache);
    let unresolved_value_token = value
        .is_none()
        .then(|| payload.get(value_start..after_value))
        .flatten()
        .and_then(unresolved_dimension_value_token);
    let direction_label = find_bytes(payload, b"direct\0", after_value, end)?;
    let direction_byte = *payload.get(direction_label + b"direct\0".len())?;
    let auxiliary_label = find_bytes(payload, b"aux_value\0", direction_label, end)?;
    let (auxiliary_value, after_auxiliary, _) =
        decode_variable_scalar(payload, auxiliary_label + b"aux_value\0".len(), end, cache);
    let external_label = find_bytes(payload, b"ext_id\0", after_auxiliary, end)?;
    let (external_id, _) = segment_int(payload, external_label + b"ext_id\0".len());
    Some(FeatureDimension {
        dimension_type,
        value,
        unresolved_value_token,
        value_unit: dimension_unit(dimension_type),
        direction_byte,
        auxiliary_value,
        external_id: external_id?,
        offset: type_label,
    })
}

fn positional_dimension(
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> Option<FeatureDimension> {
    let (dimension_type, cursor) = segment_int(payload, start);
    let dimension_type = dimension_type?;
    let value_start = cursor;
    let (value, cursor, _) = match payload.get(cursor) {
        Some(0x00) if cursor + 3 <= end => (None, cursor + 3, true),
        Some(0x01) if cursor + 4 <= end => (None, cursor + 4, true),
        Some(0x0e) => (Some(-0.5), cursor + 1, false),
        Some(0x18) => (Some(0.0), cursor + 1, false),
        _ => decode_variable_scalar(payload, cursor, end, cache),
    };
    let unresolved_value_token = value
        .is_none()
        .then(|| payload.get(value_start..cursor))
        .flatten()
        .and_then(unresolved_dimension_value_token);
    let direction_byte = *payload.get(cursor).filter(|_| cursor < end)?;
    let auxiliary_start = cursor + 1;
    let (auxiliary_value, cursor) = if payload.get(auxiliary_start) == Some(&0x18) {
        (Some(0.0), auxiliary_start + 1)
    } else {
        let (value, next, _) = decode_variable_scalar(payload, auxiliary_start, end, cache);
        (value, next)
    };
    let (external_id, _) = segment_int(payload, cursor);
    Some(FeatureDimension {
        dimension_type,
        value,
        unresolved_value_token,
        value_unit: dimension_unit(dimension_type),
        direction_byte,
        auxiliary_value,
        external_id: external_id?,
        offset: start,
    })
}

fn dimension_table(
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> Option<FeatureDimensionTable> {
    let table = find_bytes(payload, b"dimtab_ptr\0", start, end)?;
    let mut cursor = table + b"dimtab_ptr\0".len();
    while payload
        .get(cursor)
        .is_some_and(|byte| matches!(byte, 0xf1..=0xf3))
    {
        cursor += 1;
    }
    (payload.get(cursor) == Some(&psb::token::ARRAY_OPEN)).then_some(())?;
    let (declared_count, next) = psb::compact_int(payload, cursor + 1);
    cursor = next;
    let mut reference_bytes = None;
    let entity_ref = if payload.get(cursor) == Some(&psb::token::ENTITY_REF) {
        let reference_start = cursor + 1;
        let (value, next) = psb::reference_id(payload, reference_start).ok()?;
        reference_bytes = payload.get(reference_start..next).map(<[u8]>::to_vec);
        cursor = next;
        Some(value)
    } else {
        None
    };
    let region_end = find_bytes(payload, b"\xe0\x00relat_ptr\0", cursor, end).unwrap_or(end);
    let mut separator = vec![0xf3, psb::token::ENTITY_REF];
    if let Some(bytes) = &reference_bytes {
        separator.extend_from_slice(bytes);
    }
    separator.push(0xe2);
    let first_end = if reference_bytes.is_some() {
        find_bytes(payload, &separator, cursor, region_end).unwrap_or(region_end)
    } else {
        region_end
    };
    let mut rows = Vec::new();
    if let Some(row) = labeled_dimension(payload, cursor, first_end, cache) {
        rows.push(row);
    }
    if reference_bytes.is_some() {
        let mut replay = first_end;
        while replay < region_end
            && rows.len() < usize::try_from(declared_count).unwrap_or(usize::MAX)
        {
            if payload.get(replay..replay + separator.len()) != Some(separator.as_slice()) {
                break;
            }
            replay += separator.len();
            let next_separator =
                find_bytes(payload, &separator, replay, region_end).unwrap_or(region_end);
            let Some(row) = positional_dimension(payload, replay, next_separator, cache) else {
                break;
            };
            rows.push(row);
            replay = next_separator;
        }
    }
    Some(FeatureDimensionTable {
        declared_count,
        entity_ref,
        rows,
        offset: table,
    })
}

fn positional_dimension_table(
    payload: &[u8],
    start: usize,
    end: usize,
    table_class: u32,
    cache: &scalar::ScalarCache,
) -> Option<FeatureDimensionTable> {
    let (table, declared_count, mut cursor, reference_bytes) = (start..end).find_map(|table| {
        (payload.get(table) == Some(&psb::token::ARRAY_OPEN)).then_some(())?;
        let (declared_count, after_count) = psb::compact_int(payload, table + 1);
        (payload.get(after_count) == Some(&psb::token::ENTITY_REF)).then_some(())?;
        let reference_start = after_count + 1;
        let (class, after_reference) = psb::reference_id(payload, reference_start).ok()?;
        (class == table_class
            && payload.get(after_reference..after_reference + 2) == Some(&[0xfb, 0xe2]))
        .then(|| {
            (
                table,
                declared_count,
                after_reference + 2,
                payload[reference_start..after_reference].to_vec(),
            )
        })
    })?;
    (payload.get(cursor) == Some(&psb::token::ENTITY_REF)).then_some(())?;
    let (_, after_row_class) = psb::reference_id(payload, cursor + 1).ok()?;
    cursor = after_row_class;

    let mut separator = vec![0xf3, psb::token::ENTITY_REF];
    separator.extend_from_slice(&reference_bytes);
    separator.push(0xe2);
    let mut rows = Vec::new();
    let row_limit = usize::try_from(declared_count).unwrap_or(usize::MAX);
    while cursor < end && rows.len() < row_limit {
        let row_end = find_bytes(payload, &separator, cursor, end).unwrap_or(end);
        let Some(row) = positional_dimension(payload, cursor, row_end, cache) else {
            break;
        };
        rows.push(row);
        if rows.len() == row_limit {
            break;
        }
        if payload.get(row_end..row_end + separator.len()) != Some(separator.as_slice()) {
            break;
        }
        cursor = row_end + separator.len();
    }
    Some(FeatureDimensionTable {
        declared_count,
        entity_ref: Some(table_class),
        rows,
        offset: table,
    })
}

fn self_described_positional_dimension_table(
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> Option<FeatureDimensionTable> {
    let mut candidates = Vec::new();
    for table in start..end {
        if payload.get(table) != Some(&psb::token::ARRAY_OPEN) {
            continue;
        }
        let (declared_count, after_count) = psb::compact_int(payload, table + 1);
        if payload.get(after_count) != Some(&psb::token::ENTITY_REF) {
            continue;
        }
        let Ok((table_class, after_reference)) = psb::reference_id(payload, after_count + 1) else {
            continue;
        };
        if payload.get(after_reference..after_reference + 2) != Some(&[0xfb, 0xe2]) {
            continue;
        }
        let Some(candidate) = positional_dimension_table(payload, table, end, table_class, cache)
        else {
            continue;
        };
        if candidate.offset == table
            && declared_count > 1
            && candidate.declared_count == declared_count
            && usize::try_from(declared_count).ok() == Some(candidate.rows.len())
            && candidate
                .rows
                .iter()
                .all(|row| matches!(row.dimension_type, 0x01..=0x05 | 0x0a))
        {
            candidates.push(candidate);
        }
    }
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}

fn feature_skamps(payload: &[u8], start: usize, end: usize) -> Vec<FeatureSkamp> {
    let Some(table) = find_bytes(payload, b"skamp_ptr\0", start, end) else {
        return Vec::new();
    };
    let mut cursor = table + b"skamp_ptr\0".len();
    if payload
        .get(cursor)
        .is_some_and(|byte| matches!(byte, 0xf1 | 0xf3))
    {
        cursor += 1;
    } else if payload.get(cursor..cursor + 2) == Some(&[0xf4, 0x05]) {
        cursor += 2;
    }
    if payload.get(cursor) != Some(&psb::token::ARRAY_OPEN) {
        return Vec::new();
    }
    let (declared_count, next) = psb::compact_int(payload, cursor + 1);
    cursor = next;
    let class_start = cursor;
    let Ok((_, next)) = psb::reference_id(payload, cursor + 1) else {
        return Vec::new();
    };
    let class_encoding = &payload[class_start..next];
    cursor = next;
    if payload.get(cursor..cursor + 2) != Some(&[psb::token::ARRAY_CLOSE, 0xe2]) {
        return Vec::new();
    }
    cursor += 2;
    let mut trailer = Vec::with_capacity(class_encoding.len() + 2);
    trailer.push(0xf3);
    trailer.extend_from_slice(class_encoding);
    trailer.push(0xe2);
    let Some(prototype_end) = find_bytes(payload, &trailer, cursor, end) else {
        return Vec::new();
    };
    let named_item = (|| {
        Some(FeatureSkampItem {
            entity_id: named_compact_int(payload, b"ent_id\0", cursor, prototype_end)?,
            sense: named_compact_int(payload, b"sense\0", cursor, prototype_end)?,
        })
    })();
    let Some(items_label) = find_bytes(payload, b"items\0", cursor, prototype_end) else {
        return Vec::new();
    };
    let mut item_cursor = items_label + b"items\0".len();
    if payload.get(item_cursor) != Some(&psb::token::ARRAY_OPEN) {
        return Vec::new();
    }
    let (prototype_item_count, after_count) = psb::compact_int(payload, item_cursor + 1);
    item_cursor = after_count;
    let item_class_start = item_cursor;
    let Ok((_, after_item_class)) = psb::reference_id(payload, item_cursor + 1) else {
        return Vec::new();
    };
    let item_class_encoding = &payload[item_class_start..after_item_class];
    let mut item_close = Vec::with_capacity(item_class_encoding.len() + 2);
    item_close.push(0xf1);
    item_close.extend_from_slice(item_class_encoding);
    item_close.push(0xe2);
    let Some(named_item_end) = find_bytes(payload, &item_close, after_item_class, prototype_end)
    else {
        return Vec::new();
    };
    item_cursor = named_item_end + item_close.len();
    let mut prototype_items = named_item.into_iter().collect::<Vec<_>>();
    while prototype_items.len() < usize::try_from(prototype_item_count).unwrap_or(usize::MAX) {
        let (Some(entity_id), next) = segment_int(payload, item_cursor) else {
            return Vec::new();
        };
        item_cursor = next;
        let (Some(sense), next) = segment_int(payload, item_cursor) else {
            return Vec::new();
        };
        item_cursor = next;
        prototype_items.push(FeatureSkampItem { entity_id, sense });
    }
    if item_cursor != prototype_end {
        return Vec::new();
    }
    let Some(prototype) = (|| {
        Some(FeatureSkamp {
            id: named_compact_int(payload, b"id\0", cursor, prototype_end)?,
            kind: named_compact_int(payload, b"type\0", cursor, prototype_end)?,
            flags: named_compact_int(payload, b"flags\0", cursor, prototype_end)?,
            status: named_compact_int(payload, b"status\0", cursor, prototype_end)?,
            items: prototype_items,
            offset: cursor,
        })
    })() else {
        return Vec::new();
    };
    let mut rows = vec![prototype];
    cursor = prototype_end + trailer.len();
    'rows: while rows.len() < usize::try_from(declared_count).unwrap_or(usize::MAX) {
        let row_offset = cursor;
        let Some(id) = next_solver_int(payload, &mut cursor) else {
            break;
        };
        let Some(kind) = next_solver_int(payload, &mut cursor) else {
            break;
        };
        let Some(flags) = next_solver_int(payload, &mut cursor) else {
            break;
        };
        let Some(status) = next_solver_int(payload, &mut cursor) else {
            break;
        };
        if payload.get(cursor) != Some(&psb::token::ARRAY_OPEN) {
            break;
        }
        let (item_count, next) = psb::compact_int(payload, cursor + 1);
        cursor = next;
        let Ok((_, next)) = psb::reference_id(payload, cursor + 1) else {
            break;
        };
        cursor = next;
        if payload.get(cursor..cursor + 2) != Some(&[psb::token::ARRAY_CLOSE, 0xe2]) {
            break;
        }
        cursor += 2;
        let mut items = Vec::new();
        while items.len() < usize::try_from(item_count).unwrap_or(usize::MAX) {
            if !items.is_empty() && payload.get(cursor) == Some(&0xe2) {
                cursor += 1;
            }
            if payload.get(cursor) == Some(&psb::token::ENTITY_REF) {
                let Ok((_, next)) = psb::reference_id(payload, cursor + 1) else {
                    break 'rows;
                };
                cursor = next;
            }
            let Some(entity_id) = next_solver_int(payload, &mut cursor) else {
                break 'rows;
            };
            let Some(sense) = next_solver_int(payload, &mut cursor) else {
                break 'rows;
            };
            items.push(FeatureSkampItem { entity_id, sense });
            if payload.get(cursor) == Some(&0xf1) {
                let Ok((_, next)) = psb::reference_id(payload, cursor + 2) else {
                    break 'rows;
                };
                cursor = next;
                if payload.get(cursor) != Some(&0xe2) {
                    break 'rows;
                }
                cursor += 1;
            }
        }
        if payload.get(cursor..cursor + trailer.len()) == Some(trailer.as_slice()) {
            cursor += trailer.len();
        } else if payload.get(cursor) == Some(&0xe2) {
            cursor += 1;
        } else if payload.get(cursor) == Some(&0xe0) {
            // The final row is terminated by the following named table.
        } else {
            break;
        }
        rows.push(FeatureSkamp {
            id,
            kind,
            flags,
            status,
            items,
            offset: row_offset,
        });
    }
    rows
}

fn named_array_class(payload: &[u8], label: &[u8], start: usize, end: usize) -> Option<u32> {
    let label = find_bytes(payload, label, start, end)? + label.len();
    let array =
        (label..end).find(|offset| payload.get(*offset) == Some(&psb::token::ARRAY_OPEN))?;
    let (_, after_count) = psb::compact_int(payload, array + 1);
    (payload.get(after_count) == Some(&psb::token::ENTITY_REF)).then_some(())?;
    psb::reference_id(payload, after_count + 1)
        .ok()
        .map(|(class, _)| class)
}

fn named_solver_table_header(
    payload: &[u8],
    label: &[u8],
    start: usize,
    end: usize,
) -> Option<FeatureSolverTableHeader> {
    let offset = find_bytes(payload, label, start, end)?;
    let mut cursor = offset + label.len();
    if payload
        .get(cursor)
        .is_some_and(|byte| matches!(byte, 0xf1 | 0xf3))
    {
        cursor += 1;
    } else if payload
        .get(cursor..cursor + 2)
        .is_some_and(|wrapper| matches!(wrapper, [0xf4, 0x04 | 0x05]))
    {
        cursor += 2;
    }
    (payload.get(cursor) == Some(&psb::token::ARRAY_OPEN)).then_some(())?;
    let (declared_count, after_count) = psb::compact_int(payload, cursor + 1);
    (payload.get(after_count) == Some(&psb::token::ENTITY_REF)).then_some(())?;
    let (entity_ref, _) = psb::reference_id(payload, after_count + 1).ok()?;
    Some(FeatureSolverTableHeader {
        declared_count,
        entity_ref,
        offset,
    })
}

fn positional_solver_table_header(
    payload: &[u8],
    start: usize,
    end: usize,
    table_class: u32,
) -> Option<FeatureSolverTableHeader> {
    let (offset, declared_count, _, _) = positional_array_header(payload, start, end, table_class)?;
    Some(FeatureSolverTableHeader {
        declared_count,
        entity_ref: table_class,
        offset,
    })
}

fn positional_array_header(
    payload: &[u8],
    start: usize,
    end: usize,
    table_class: u32,
) -> Option<(usize, u32, usize, Vec<u8>)> {
    let candidates = (start..end)
        .filter_map(|offset| {
            (payload.get(offset) == Some(&psb::token::ARRAY_OPEN)).then_some(())?;
            let (count, after_count) = psb::compact_int(payload, offset + 1);
            (payload.get(after_count) == Some(&psb::token::ENTITY_REF)).then_some(())?;
            let reference_start = after_count + 1;
            let (class, after_class) = psb::reference_id(payload, reference_start).ok()?;
            (class == table_class
                && payload.get(after_class..after_class + 2) == Some(&[0xfb, 0xe2]))
            .then(|| {
                (
                    offset,
                    count,
                    after_class + 2,
                    payload[after_count..after_class].to_vec(),
                )
            })
        })
        .collect::<Vec<_>>();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}

fn consume_positional_separator(
    payload: &[u8],
    cursor: usize,
    end: usize,
    class_encoding: &[u8],
    class_prefixes: &[u8],
) -> Option<usize> {
    if payload.get(cursor) == Some(&0xe2) {
        return Some(cursor + 1);
    }
    let length = class_encoding.len() + 2;
    (cursor + length <= end
        && payload
            .get(cursor)
            .is_some_and(|prefix| class_prefixes.contains(prefix))
        && payload.get(cursor + 1..cursor + 1 + class_encoding.len()) == Some(class_encoding)
        && payload.get(cursor + length - 1) == Some(&0xe2))
    .then_some(cursor + length)
}

fn positional_feature_skamps(
    payload: &[u8],
    start: usize,
    end: usize,
    table_class: u32,
) -> Vec<FeatureSkamp> {
    let Some((_, count, mut cursor, table_class_encoding)) =
        positional_array_header(payload, start, end, table_class)
    else {
        return Vec::new();
    };
    if payload.get(cursor) != Some(&psb::token::ENTITY_REF) {
        return Vec::new();
    }
    let Ok((_, after_row_class)) = psb::reference_id(payload, cursor + 1) else {
        return Vec::new();
    };
    cursor = after_row_class;
    let mut rows = Vec::new();
    'rows: while rows.len() < usize::try_from(count).unwrap_or(usize::MAX) {
        let row_offset = cursor;
        let Some(id) = next_solver_int(payload, &mut cursor) else {
            break;
        };
        let Some(kind) = next_solver_int(payload, &mut cursor) else {
            break;
        };
        let Some(flags) = next_solver_int(payload, &mut cursor) else {
            break;
        };
        let Some(status) = next_solver_int(payload, &mut cursor) else {
            break;
        };
        if payload.get(cursor) != Some(&psb::token::ARRAY_OPEN) {
            break;
        }
        let (item_count, after_item_count) = psb::compact_int(payload, cursor + 1);
        if payload.get(after_item_count) != Some(&psb::token::ENTITY_REF) {
            break;
        }
        let item_class_start = after_item_count + 1;
        let Ok((_, after_item_class)) = psb::reference_id(payload, item_class_start) else {
            break;
        };
        let item_class_encoding = &payload[after_item_count..after_item_class];
        if payload.get(after_item_class..after_item_class + 2) != Some(&[0xfb, 0xe2])
            || payload.get(after_item_class + 2) != Some(&psb::token::ENTITY_REF)
        {
            break;
        }
        let Ok((_, after_item_row_class)) = psb::reference_id(payload, after_item_class + 3) else {
            break;
        };
        cursor = after_item_row_class;
        let mut items = Vec::new();
        while items.len() < usize::try_from(item_count).unwrap_or(usize::MAX) {
            let Some(entity_id) = next_solver_int(payload, &mut cursor) else {
                break 'rows;
            };
            let Some(sense) = next_solver_int(payload, &mut cursor) else {
                break 'rows;
            };
            items.push(FeatureSkampItem { entity_id, sense });
            if items.len() < usize::try_from(item_count).unwrap_or(usize::MAX) {
                let Some(next) = consume_positional_separator(
                    payload,
                    cursor,
                    end,
                    item_class_encoding,
                    &[0xf1],
                ) else {
                    break 'rows;
                };
                cursor = next;
            }
        }
        let row = FeatureSkamp {
            id,
            kind,
            flags,
            status,
            items,
            offset: row_offset,
        };
        if rows.len() + 1 < usize::try_from(count).unwrap_or(usize::MAX) {
            let Some(next) =
                consume_positional_separator(payload, cursor, end, &table_class_encoding, &[0xf3])
            else {
                break;
            };
            cursor = next;
        }
        rows.push(row);
    }
    rows
}

fn feature_relation_triples(
    payload: &[u8],
    start: usize,
    end: usize,
) -> Vec<FeatureRelationTriple> {
    let Some(table) = find_bytes(payload, b"triples_ptr\0", start, end) else {
        return Vec::new();
    };
    let mut cursor = table + b"triples_ptr\0".len();
    if payload.get(cursor..cursor + 2) == Some(&[0xf4, 0x04]) {
        cursor += 2;
    }
    if payload.get(cursor) != Some(&psb::token::ARRAY_OPEN) {
        return Vec::new();
    }
    let (declared_count, next) = psb::compact_int(payload, cursor + 1);
    cursor = next;
    let Ok((_, next)) = psb::reference_id(payload, cursor + 1) else {
        return Vec::new();
    };
    cursor = next;
    if payload.get(cursor..cursor + 2) != Some(&[psb::token::ARRAY_CLOSE, 0xe2]) {
        return Vec::new();
    }
    cursor += 2;
    let Some(close) = find_bytes(payload, &[0xf1, psb::token::ENTITY_REF], cursor, end) else {
        return Vec::new();
    };
    let prototype = FeatureRelationTriple {
        relation_id: named_compact_int(payload, b"rel_id\0", cursor, close),
        equation_id: named_compact_int(payload, b"eqn_id\0", cursor, close),
        skamp_id: named_compact_int(payload, b"skamp_id\0", cursor, close),
        offset: cursor,
    };
    let Ok((_, next)) = psb::reference_id(payload, close + 2) else {
        return Vec::new();
    };
    cursor = next;
    if payload.get(cursor) != Some(&0xe2) {
        return Vec::new();
    }
    cursor += 1;
    let mut rows = vec![prototype];
    while rows.len() < usize::try_from(declared_count).unwrap_or(usize::MAX) {
        let row_offset = cursor;
        let relation_id = next_solver_int(payload, &mut cursor);
        let equation_id = next_solver_int(payload, &mut cursor);
        let skamp_id = next_solver_int(payload, &mut cursor);
        let terminal_named_boundary = rows.len() + 1
            == usize::try_from(declared_count).unwrap_or(usize::MAX)
            && payload.get(cursor).is_some_and(|byte| *byte >= 0xe0);
        if payload.get(cursor) != Some(&0xe2) && !terminal_named_boundary {
            break;
        }
        if !terminal_named_boundary {
            cursor += 1;
        }
        rows.push(FeatureRelationTriple {
            relation_id,
            equation_id,
            skamp_id,
            offset: row_offset,
        });
    }
    rows
}

fn positional_relation_triples(
    payload: &[u8],
    start: usize,
    end: usize,
    table_class: u32,
) -> Vec<FeatureRelationTriple> {
    let Some((_, count, mut cursor, class_encoding)) =
        positional_array_header(payload, start, end, table_class)
    else {
        return Vec::new();
    };
    if payload.get(cursor) != Some(&psb::token::ENTITY_REF) {
        return Vec::new();
    }
    let Ok((_, after_row_class)) = psb::reference_id(payload, cursor + 1) else {
        return Vec::new();
    };
    cursor = after_row_class;
    let mut rows = Vec::new();
    while rows.len() < usize::try_from(count).unwrap_or(usize::MAX) {
        let offset = cursor;
        let before_relation = cursor;
        let relation_id = next_solver_int(payload, &mut cursor);
        if cursor <= before_relation {
            break;
        }
        let before_equation = cursor;
        let equation_id = next_solver_int(payload, &mut cursor);
        if cursor <= before_equation {
            break;
        }
        let before_skamp = cursor;
        let skamp_id = next_solver_int(payload, &mut cursor);
        if cursor <= before_skamp {
            break;
        }
        let row = FeatureRelationTriple {
            relation_id,
            equation_id,
            skamp_id,
            offset,
        };
        if rows.len() + 1 < usize::try_from(count).unwrap_or(usize::MAX) {
            let Some(next) =
                consume_positional_separator(payload, cursor, end, &class_encoding, &[0xf1])
            else {
                break;
            };
            cursor = next;
        }
        rows.push(row);
    }
    rows
}

fn relation_operand_vectors(bytes: &[u8]) -> Option<[[Option<u32>; 4]; 3]> {
    let mut values = Vec::with_capacity(12);
    let mut cursor = 0;
    while cursor < bytes.len() && values.len() < 12 {
        match bytes[cursor] {
            0xe4 => {
                values.push(Some(1));
                cursor += 1;
            }
            0xe5 => {
                values.extend([Some(0); 2]);
                cursor += 1;
            }
            0xe6 => {
                values.extend([Some(0); 3]);
                cursor += 1;
            }
            0xf6 => {
                values.push(None);
                cursor += 1;
            }
            _ => {
                let value = next_solver_int(bytes, &mut cursor)?;
                values.push(Some(value));
            }
        }
    }
    if cursor != bytes.len() || values.len() != 12 {
        return None;
    }
    let mut chunks = values.chunks_exact(4);
    let result = [
        chunks.next()?.try_into().ok()?,
        chunks.next()?.try_into().ok()?,
        chunks.next()?.try_into().ok()?,
    ];
    chunks.next().is_none().then_some(result)
}

fn relation_table(payload: &[u8], start: usize, end: usize) -> Option<FeatureRelationTable> {
    let table = find_bytes(payload, b"relat_ptr\0", start, end)?;
    let mut cursor = table + b"relat_ptr\0".len();
    if payload.get(cursor..cursor + 2) == Some(&[0xf4, 0x04]) {
        cursor += 2;
    }
    (payload.get(cursor) == Some(&psb::token::ARRAY_OPEN)).then_some(())?;
    let (declared_count, next) = psb::compact_int(payload, cursor + 1);
    cursor = next;
    let entity_ref = if payload.get(cursor) == Some(&psb::token::ENTITY_REF) {
        let (value, next) = psb::reference_id(payload, cursor + 1).ok()?;
        cursor = next;
        Some(value)
    } else {
        None
    };
    if payload.get(cursor) == Some(&psb::token::ARRAY_CLOSE) {
        cursor += 1;
    }
    if payload.get(cursor) == Some(&0xe2) {
        cursor += 1;
    }
    let rows_end = [b"skamp_ptr\0".as_slice(), b"triples_ptr\0"]
        .into_iter()
        .filter_map(|label| find_bytes(payload, label, cursor, end))
        .min()
        .unwrap_or(end);
    let rows_start = (|| {
        let close = find_bytes(payload, &[0xf1, psb::token::ENTITY_REF], cursor, rows_end)?;
        let (_, after_ref) = psb::reference_id(payload, close + 2).ok()?;
        (payload.get(after_ref) == Some(&0xe2)).then_some(after_ref + 1)
    })();
    let rows = rows_start.map_or_else(Vec::new, |rows_start| {
        positional_relation_rows(
            payload,
            rows_start,
            rows_end,
            declared_count.saturating_sub(2),
        )
    });
    Some(FeatureRelationTable {
        declared_count,
        entity_ref,
        rows,
        skamps: feature_skamps(payload, start, end),
        skamp_header: named_solver_table_header(payload, b"skamp_ptr\0", start, end),
        triples: feature_relation_triples(payload, start, end),
        triples_header: named_solver_table_header(payload, b"triples_ptr\0", start, end),
        offset: table,
    })
}

fn positional_relation_rows(
    payload: &[u8],
    mut cursor: usize,
    end: usize,
    row_count: u32,
) -> Vec<FeatureRelation> {
    if cursor > end || end > payload.len() {
        return Vec::new();
    }
    let mut rows = Vec::new();
    for _ in 0..row_count {
        let Some(row_end) = payload[cursor..end]
            .iter()
            .position(|byte| *byte == 0xe2)
            .map(|relative| relative + cursor)
        else {
            break;
        };
        let (relation_id, after_id) = psb::compact_int(payload, cursor);
        if after_id <= cursor || after_id >= row_end {
            break;
        }
        let (used, after_used) = psb::compact_int(payload, after_id);
        if after_used <= after_id || after_used >= row_end {
            break;
        }
        let mut suffixes = Vec::new();
        for suffix_start in after_used..row_end {
            let (sign, after_sign) = psb::compact_int(payload, suffix_start);
            let (dimension_id, after_dimension) = psb::compact_int(payload, after_sign);
            let (relation_type, after_type) = psb::compact_int(payload, after_dimension);
            if after_sign > suffix_start
                && after_dimension > after_sign
                && after_type > after_dimension
                && after_type == row_end
            {
                suffixes.push((suffix_start, sign, dimension_id, relation_type));
            }
        }
        let [(suffix_start, sign, dimension_id, relation_type)] = suffixes.as_slice() else {
            break;
        };
        let operands = payload[after_used..*suffix_start].to_vec();
        rows.push(FeatureRelation {
            relation_id,
            used,
            operand_vectors: relation_operand_vectors(&operands),
            operands,
            sign: *sign,
            dimension_id: *dimension_id,
            relation_type: *relation_type,
            body: payload[cursor..row_end].to_vec(),
            offset: cursor,
        });
        cursor = row_end + 1;
    }
    rows
}

fn positional_relation_table(
    payload: &[u8],
    start: usize,
    end: usize,
    table_class: u32,
) -> Option<FeatureRelationTable> {
    let (table, declared_count, cursor, reference_bytes) = (start..end).find_map(|table| {
        (payload.get(table) == Some(&psb::token::ARRAY_OPEN)).then_some(())?;
        let (declared_count, after_count) = psb::compact_int(payload, table + 1);
        (payload.get(after_count) == Some(&psb::token::ENTITY_REF)).then_some(())?;
        let reference_start = after_count + 1;
        let (class, after_reference) = psb::reference_id(payload, reference_start).ok()?;
        (class == table_class
            && payload.get(after_reference..after_reference + 2) == Some(&[0xfb, 0xe2]))
        .then(|| {
            (
                table,
                declared_count,
                after_reference + 2,
                payload[reference_start..after_reference].to_vec(),
            )
        })
    })?;
    let mut prototype_separator = vec![0xf1, psb::token::ENTITY_REF];
    prototype_separator.extend_from_slice(&reference_bytes);
    prototype_separator.push(0xe2);
    let rows_start = (|| {
        (payload.get(cursor) == Some(&psb::token::ENTITY_REF)).then_some(())?;
        let (_, prototype) = psb::reference_id(payload, cursor + 1).ok()?;
        let prototype_end = find_bytes(payload, &prototype_separator, prototype, end)?;
        Some(prototype_end + prototype_separator.len())
    })();
    let rows = rows_start.map_or_else(Vec::new, |rows_start| {
        positional_relation_rows(payload, rows_start, end, declared_count.saturating_sub(2))
    });
    Some(FeatureRelationTable {
        declared_count,
        entity_ref: Some(table_class),
        rows,
        skamps: Vec::new(),
        skamp_header: None,
        triples: Vec::new(),
        triples_header: None,
        offset: table,
    })
}

fn saved_section_scalar(
    payload: &[u8],
    offset: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> (Option<f64>, usize) {
    let Some(&prefix) = payload.get(offset).filter(|_| offset < end) else {
        return (None, offset);
    };
    if prefix == 0x18
        && payload
            .get(offset + 1)
            .is_some_and(|next| matches!(next, 0x18 | 0xe0 | 0xe3 | 0xf0 | 0xf1))
    {
        return (Some(0.0), offset + 1);
    }
    if matches!(prefix, 0x90 | 0xd7) && offset + 7 <= end {
        return (None, offset + 7);
    }
    if prefix == 0x41 && offset + 8 <= end {
        let mut raw = [0; 8];
        raw[0] = 0x3f;
        raw[1..].copy_from_slice(&payload[offset + 1..offset + 8]);
        return (Some(f64::from_be_bytes(raw)), offset + 8);
    }
    if prefix == 0x2d && offset + 8 <= end {
        let mut raw = [0; 8];
        raw[0] = 0x40;
        raw[1..].copy_from_slice(&payload[offset + 1..offset + 8]);
        return (Some(f64::from_be_bytes(raw)), offset + 8);
    }
    if matches!(prefix, 0x74 | 0x75) && offset + 7 <= end {
        let mut raw = [0; 8];
        raw[0] = 0x3f;
        raw[1] = prefix.wrapping_sub(0x8b);
        raw[2..].copy_from_slice(&payload[offset + 1..offset + 7]);
        return (Some(f64::from_be_bytes(raw)), offset + 7);
    }
    if prefix == 0x99 && offset + 7 <= end {
        let mut raw = [0; 8];
        raw[..2].copy_from_slice(&[0xc0, 0x0e]);
        raw[2..].copy_from_slice(&payload[offset + 1..offset + 7]);
        return (Some(f64::from_be_bytes(raw)), offset + 7);
    }
    if prefix == 0xdd && offset + 7 <= end {
        let mut raw = [0; 8];
        raw[..2].copy_from_slice(&[0x40, 0x0c]);
        raw[2..].copy_from_slice(&payload[offset + 1..offset + 7]);
        return (Some(f64::from_be_bytes(raw)), offset + 7);
    }
    let supplied_head = match prefix {
        0xb3 => Some([0xbf, 0xe0]),
        0xcb => Some([0xbf, 0xf8]),
        0xd6 => Some([0xc0, 0x04]),
        _ => None,
    };
    if let Some(head) = supplied_head.filter(|_| offset + 7 <= end) {
        let mut raw = [0; 8];
        raw[..2].copy_from_slice(&head);
        raw[2..].copy_from_slice(&payload[offset + 1..offset + 7]);
        return (Some(f64::from_be_bytes(raw)), offset + 7);
    }
    if prefix == 0xd5 && offset + 7 <= end {
        let mut raw = [0; 8];
        raw[0] = 0xbf;
        raw[1..7].copy_from_slice(&payload[offset + 1..offset + 7]);
        return (Some(f64::from_be_bytes(raw)), offset + 7);
    }
    scalar::decode_in_lane(payload, offset, cache)
        .filter(|(_, next)| *next <= end)
        .map_or((None, offset + 1), |(value, next)| (Some(value), next))
}

fn saved_line_block(
    payload: &[u8],
    mut cursor: usize,
    segment_end: usize,
    cache: &scalar::ScalarCache,
) -> Vec<FeatureSavedEntity> {
    if payload.get(cursor) == Some(&0xf1) {
        cursor = payload[cursor..segment_end]
            .iter()
            .position(|byte| *byte == 0xe3)
            .map_or(segment_end, |relative| cursor + relative + 1);
    }
    let mut entities = Vec::new();
    while cursor < segment_end {
        if payload.get(cursor) == Some(&0xe3) {
            cursor += 1;
        }
        let point_label = b"\xe0\x00entity(point)\0";
        if payload.get(cursor..cursor + point_label.len()) == Some(point_label) {
            let Some(close) = find_bytes(
                payload,
                &[0xf1, psb::token::ENTITY_REF],
                cursor + point_label.len(),
                segment_end,
            ) else {
                break;
            };
            let Ok((_, after_reference)) = psb::reference_id(payload, close + 2) else {
                break;
            };
            if payload.get(after_reference) != Some(&0xe3) {
                break;
            }
            cursor = after_reference + 1;
            continue;
        }
        if payload.get(cursor) == Some(&psb::token::NAMED_RECORD)
            || payload.get(cursor..cursor + 2) == Some(&[0xf1, 0xe1])
        {
            break;
        }
        let record_offset = cursor;
        let mut references = Vec::new();
        let mut attributes = Vec::new();
        loop {
            if payload.get(cursor) == Some(&psb::token::ENTITY_REF) {
                let Ok((reference, next)) = psb::reference_id(payload, cursor + 1) else {
                    break;
                };
                references.push(reference);
                cursor = next;
            } else if payload
                .get(cursor..cursor + 2)
                .is_some_and(|bytes| matches!(bytes, [0xf0 | 0xf1, 0xf7]))
            {
                let Ok((reference, next)) = psb::reference_id(payload, cursor + 2) else {
                    break;
                };
                references.push(reference);
                cursor = next;
            } else if payload.get(cursor) == Some(&0xeb) {
                let Some(bytes) = payload.get(cursor + 1..cursor + 6) else {
                    break;
                };
                let mut attribute = [0; 5];
                attribute.copy_from_slice(bytes);
                attributes.push(attribute);
                cursor += 6;
            } else {
                break;
            }
        }
        let (Some(entity_id), next) = segment_int(payload, cursor) else {
            cursor += 1;
            continue;
        };
        if payload.get(next) != Some(&0xe2) {
            cursor += 1;
            continue;
        }
        cursor = next + 1;
        let mut values = Vec::with_capacity(6);
        while cursor < segment_end && values.len() < 6 {
            if payload.get(cursor) == Some(&0xe3)
                || payload.get(cursor) == Some(&psb::token::NAMED_RECORD)
            {
                break;
            }
            if payload.get(cursor..cursor + 2) == Some(&[0x18, 0xe5]) {
                values.extend([Some(0.0), Some(1.0), Some(0.0)]);
                cursor += 2;
                continue;
            }
            if payload.get(cursor) == Some(&psb::token::ENTITY_REF) {
                let Ok((reference, next)) = psb::reference_id(payload, cursor + 1) else {
                    break;
                };
                references.push(reference);
                cursor = next;
                continue;
            }
            if payload
                .get(cursor..cursor + 2)
                .is_some_and(|bytes| matches!(bytes, [0xf0 | 0xf1, 0xf7]))
            {
                let Ok((reference, next)) = psb::reference_id(payload, cursor + 2) else {
                    break;
                };
                references.push(reference);
                cursor = next;
                continue;
            }
            if payload.get(cursor) == Some(&0xeb) {
                let Some(bytes) = payload.get(cursor + 1..cursor + 6) else {
                    break;
                };
                let mut attribute = [0; 5];
                attribute.copy_from_slice(bytes);
                attributes.push(attribute);
                cursor += 6;
                continue;
            }
            if payload.get(cursor) == Some(&0xe2) {
                cursor += 1;
                continue;
            }
            let (value, next) = saved_section_scalar(payload, cursor, segment_end, cache);
            if next <= cursor {
                break;
            }
            values.push(value);
            cursor = next;
        }
        loop {
            if payload
                .get(cursor)
                .is_some_and(|prefix| matches!(prefix, 0x0f | 0x18 | 0xe6))
            {
                cursor += 1;
                continue;
            }
            if payload
                .get(cursor)
                .is_some_and(|prefix| matches!(prefix, 0x82..=0x8f))
                && cursor + 6 <= segment_end
            {
                cursor += 6;
                continue;
            }
            let reference_start = match payload.get(cursor..cursor + 2) {
                Some([0xf0 | 0xf1, 0xf7]) => Some(cursor + 2),
                _ if payload.get(cursor) == Some(&psb::token::ENTITY_REF) => Some(cursor + 1),
                _ => None,
            };
            let Some(reference_start) = reference_start else {
                break;
            };
            let Ok((reference, next)) = psb::reference_id(payload, reference_start) else {
                break;
            };
            references.push(reference);
            cursor = next;
        }
        let row_separator = payload.get(cursor) == Some(&0xe3);
        let named_boundary = payload.get(cursor) == Some(&psb::token::NAMED_RECORD);
        let section_boundary = cursor == segment_end;
        if !row_separator && !named_boundary && !section_boundary {
            cursor = record_offset + 1;
            continue;
        }
        if row_separator {
            cursor += 1;
        }
        values.resize(6, None);
        entities.push(FeatureSavedEntity::Line(FeatureSavedLine {
            entity_id,
            references,
            attributes,
            endpoints: [
                [values[0], values[1], values[2]],
                [values[3], values[4], values[5]],
            ],
            offset: record_offset,
        }));
    }
    entities
}

fn saved_line_entities(
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> Vec<FeatureSavedEntity> {
    let label = b"\xe0\x00entity(line)\0";
    let mut entities = Vec::new();
    let mut search = start;
    while let Some(label_offset) = find_bytes(payload, label, search, end) {
        let body_start = label_offset + label.len();
        let body_end = [
            b"\xe0\x00entity(arc)\0".as_slice(),
            b"\xe0\x00entity(circle)\0".as_slice(),
            b"\xe0\x00entity(dummy_ent)\0".as_slice(),
        ]
        .into_iter()
        .filter_map(|next_label| find_bytes(payload, next_label, body_start, end))
        .min()
        .unwrap_or(end);
        entities.extend(saved_line_block(payload, body_start, body_end, cache));
        search = body_end;
    }
    entities
}

fn saved_named_scalars<const N: usize>(
    payload: &[u8],
    field: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> Option<[Option<f64>; N]> {
    let mut label = vec![0xe0, 0x02];
    label.extend_from_slice(field);
    label.push(0);
    let mut cursor = find_bytes(payload, &label, start, end)? + label.len();
    while payload
        .get(cursor)
        .is_some_and(|byte| matches!(byte, 0xf1..=0xf3))
    {
        cursor += 1;
    }
    if payload.get(cursor) == Some(&psb::token::ARRAY_OPEN) {
        let (count, next) = psb::compact_int(payload, cursor + 1);
        (usize::try_from(count).ok()? == N).then_some(())?;
        cursor = next;
    }
    if N == 3 && payload.get(cursor..cursor + 2) == Some(&[0x18, 0xe5]) {
        return Some(std::array::from_fn(|index| {
            Some(if index == 1 { 1.0 } else { 0.0 })
        }));
    }
    let mut values = [None; N];
    for value in &mut values {
        let (decoded, next) = saved_section_scalar(payload, cursor, end, cache);
        (next > cursor).then_some(())?;
        *value = decoded;
        cursor = next;
    }
    Some(values)
}

fn saved_entity_id(payload: &[u8], start: usize, end: usize) -> Option<u32> {
    named_compact_int(payload, b"\xe0\x01id\0", start, end)
}

fn saved_arc_scalar(
    payload: &[u8],
    offset: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> (Option<f64>, usize) {
    if payload.get(offset) == Some(&0x18)
        && payload.get(offset + 1).is_some_and(|next| {
            matches!(
                next,
                0x28 | 0x5e | 0x60 | 0x64 | 0x9b
                    ..=0xa0 | 0xad | 0xcc | 0xd0 | 0xd2 | 0xd5 | 0xde | 0xdf
            )
        })
    {
        return (Some(0.0), offset + 1);
    }
    if payload.get(offset) == Some(&0x28) && offset + 8 <= end {
        let mut raw = [0; 8];
        raw[0] = 0x3f;
        raw[1..].copy_from_slice(&payload[offset + 1..offset + 8]);
        return (Some(f64::from_be_bytes(raw)), offset + 8);
    }
    let arc_dict = match payload.get(offset).copied() {
        Some(0x9b) => Some([0x40, 0x10]),
        Some(0x9c) => Some([0x40, 0x11]),
        Some(0x9d) => Some([0x40, 0x12]),
        Some(0x9e) => Some([0x40, 0x13]),
        Some(0x9f) => Some([0x40, 0x14]),
        Some(0xa0) => Some([0x40, 0x15]),
        Some(0x5e) => Some([0x3f, 0xd3]),
        Some(0x60) => Some([0x3f, 0xd5]),
        Some(0x64) => Some([0x3f, 0xd9]),
        Some(0xad) => Some([0x3f, 0xd9]),
        Some(0xcc) => Some([0xbf, 0xf9]),
        Some(0xd0) => Some([0xbf, 0xfe]),
        Some(0xd2) => Some([0xc0, 0x00]),
        Some(0xd5) => Some([0xc0, 0x03]),
        Some(0xde) => Some([0xc0, 0x10]),
        Some(0xdf) => Some([0xc0, 0x11]),
        _ => None,
    };
    if let (Some(head), Some(tail)) = (arc_dict, payload.get(offset + 1..offset + 7)) {
        let mut raw = [0; 8];
        raw[..2].copy_from_slice(&head);
        raw[2..].copy_from_slice(tail);
        return (Some(f64::from_be_bytes(raw)), offset + 7);
    }
    let decoded = saved_section_scalar(payload, offset, end, cache);
    if decoded.1 > offset + 1 || decoded.0.is_some() {
        return decoded;
    }
    if payload
        .get(offset)
        .is_some_and(|prefix| matches!(prefix, 0x80..=0xdf))
        && offset + 7 <= end
    {
        return (None, offset + 7);
    }
    decoded
}

fn saved_positional_generated_entities(
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
    order_table: Option<&FeatureOrderTable>,
    segments: Option<&FeatureSegmentTable>,
) -> Vec<FeatureSavedEntity> {
    let (Some(order_table), Some(segments)) = (order_table, segments) else {
        return Vec::new();
    };
    let generated_segments = order_table
        .rows
        .iter()
        .filter_map(|row| {
            (order_table.internal_id(row.external_id) == Some(row.internal_id)
                && order_table.external_id(row.internal_id) == Some(row.external_id))
            .then_some(())?;
            let segment = segments.segment(row.external_id)?;
            Some((row.internal_id, segment))
        })
        .collect::<BTreeMap<_, _>>();
    let mut starts = Vec::new();
    for separator in start..end {
        if payload.get(separator) != Some(&0xe3) {
            continue;
        }
        let row_start = separator + 1;
        let (Some(entity_id), after_id) = segment_int(payload, row_start) else {
            continue;
        };
        if !generated_segments.contains_key(&entity_id) {
            continue;
        }
        let header_end = after_id.saturating_add(24).min(end);
        if after_id > header_end {
            continue;
        }
        if payload[after_id..header_end].contains(&0xe2) {
            starts.push(row_start);
        }
    }
    starts.sort_unstable();
    starts.dedup();

    let mut entities = Vec::new();
    for (index, row_start) in starts.iter().copied().enumerate() {
        let row_end = starts
            .get(index + 1)
            .map_or(end, |next| next.saturating_sub(1));
        let (Some(entity_id), after_id) = segment_int(payload, row_start) else {
            continue;
        };
        let segment = generated_segments[&entity_id];
        let value_count = match segment.kind {
            FeatureSegmentKind::Line => 6,
            FeatureSegmentKind::Arc => 12,
            FeatureSegmentKind::Point => continue,
        };
        if after_id > row_end {
            continue;
        }
        let Some(header_size) = payload[after_id..row_end]
            .iter()
            .position(|byte| *byte == 0xe2)
        else {
            continue;
        };
        let mut cursor = after_id + header_size + 1;
        let mut values = Vec::with_capacity(value_count);
        while cursor < row_end && values.len() < value_count {
            if payload.get(cursor) == Some(&0xe3) {
                break;
            }
            if payload.get(cursor..cursor + 2) == Some(&[0x18, 0xe5]) {
                values.extend([Some(0.0), Some(1.0), Some(0.0)]);
                cursor += 2;
                continue;
            }
            if payload.get(cursor) == Some(&psb::token::ENTITY_REF) {
                let Ok((_, next)) = psb::reference_id(payload, cursor + 1) else {
                    break;
                };
                cursor = next;
                continue;
            }
            if payload
                .get(cursor..cursor + 2)
                .is_some_and(|bytes| matches!(bytes, [0xf0 | 0xf1, 0xf7]))
            {
                let Ok((_, next)) = psb::reference_id(payload, cursor + 2) else {
                    break;
                };
                cursor = next;
                continue;
            }
            if payload.get(cursor) == Some(&0xeb) {
                cursor += 6;
                continue;
            }
            if matches!(payload.get(cursor), Some(0xf6)) {
                cursor += 1;
                continue;
            }
            let (value, next) = saved_arc_scalar(payload, cursor, row_end, cache);
            if next <= cursor {
                break;
            }
            values.push(value);
            cursor = next;
        }
        if values.len() != value_count {
            if segment.kind != FeatureSegmentKind::Arc
                || values.len() > value_count
                || (cursor != row_end && payload.get(cursor) != Some(&0xe3))
            {
                continue;
            }
            values.resize(value_count, None);
        }
        match segment.kind {
            FeatureSegmentKind::Line => {
                let endpoints = [
                    [values[0], values[1], values[2]],
                    [values[3], values[4], values[5]],
                ];
                let orientation_matches = match (
                    segment.vertical_horizontal,
                    endpoints[0][0],
                    endpoints[0][1],
                    endpoints[1][0],
                    endpoints[1][1],
                ) {
                    (Some(0), Some(first), _, Some(second), _) => {
                        let scale = first.abs().max(second.abs()).max(1.0);
                        (first - second).abs() <= 1e-9 * scale
                    }
                    (Some(1), _, Some(first), _, Some(second)) => {
                        let scale = first.abs().max(second.abs()).max(1.0);
                        (first - second).abs() <= 1e-9 * scale
                    }
                    _ => false,
                };
                if orientation_matches {
                    entities.push(FeatureSavedEntity::Line(FeatureSavedLine {
                        entity_id,
                        references: Vec::new(),
                        attributes: Vec::new(),
                        endpoints,
                        offset: row_start,
                    }));
                }
            }
            FeatureSegmentKind::Arc => {
                entities.push(FeatureSavedEntity::Arc(FeatureSavedArc {
                    entity_id,
                    center: [values[0], values[1], values[2]],
                    radius: values[3],
                    endpoints: [
                        [values[4], values[5], values[6]],
                        [values[7], values[8], values[9]],
                    ],
                    parameters: [values[10], values[11]],
                    offset: row_start,
                }));
            }
            FeatureSegmentKind::Point => {}
        }
    }
    entities
}

fn saved_circular_entities(
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
    order_table: Option<&FeatureOrderTable>,
    segments: Option<&FeatureSegmentTable>,
) -> Vec<FeatureSavedEntity> {
    let mut entities = Vec::new();
    for (kind, label) in [
        ("arc", b"\xe0\x00entity(arc)\0".as_slice()),
        ("circle", b"\xe0\x00entity(circle)\0".as_slice()),
    ] {
        let mut search = start;
        while let Some(entity_offset) = find_bytes(payload, label, search, end) {
            let body_start = entity_offset + label.len();
            let body_end = find_bytes(payload, b"\xe0\x00entity(", body_start, end).unwrap_or(end);
            let Some(entity_id) = saved_entity_id(payload, body_start, body_end) else {
                search = body_end;
                continue;
            };
            let center = saved_named_scalars::<3>(payload, b"center", body_start, body_end, cache)
                .unwrap_or([None; 3]);
            let radius = saved_named_scalars::<1>(payload, b"radius", body_start, body_end, cache)
                .unwrap_or([None])[0];
            if kind == "arc" {
                let first = saved_named_scalars::<3>(payload, b"end1", body_start, body_end, cache)
                    .unwrap_or([None; 3]);
                let second =
                    saved_named_scalars::<3>(payload, b"end2", body_start, body_end, cache)
                        .unwrap_or([None; 3]);
                let start_parameter =
                    saved_named_scalars::<1>(payload, b"t0", body_start, body_end, cache)
                        .unwrap_or([None])[0];
                let end_parameter =
                    saved_named_scalars::<1>(payload, b"t1", body_start, body_end, cache)
                        .unwrap_or([None])[0];
                entities.push(FeatureSavedEntity::Arc(FeatureSavedArc {
                    entity_id,
                    center,
                    radius,
                    endpoints: [first, second],
                    parameters: [start_parameter, end_parameter],
                    offset: entity_offset,
                }));
                entities.extend(saved_positional_generated_entities(
                    payload,
                    body_start,
                    body_end,
                    cache,
                    order_table,
                    segments,
                ));
            } else {
                entities.push(FeatureSavedEntity::Circle(FeatureSavedCircle {
                    entity_id,
                    center,
                    radius,
                    offset: entity_offset,
                }));
            }
            search = body_end;
        }
    }
    entities
}

fn saved_dummy_entities(payload: &[u8], start: usize, end: usize) -> Vec<FeatureSavedEntity> {
    let label = b"\xe0\x00entity(dummy_ent)\0";
    let mut entities = Vec::new();
    let mut search = start;
    while let Some(entity_offset) = find_bytes(payload, label, search, end) {
        let body_start = entity_offset + label.len();
        let body_end = find_bytes(payload, b"\xe0\x00entity(", body_start, end).unwrap_or(end);
        entities.push(FeatureSavedEntity::Dummy(FeatureSavedDummy {
            entity_id: saved_entity_id(payload, body_start, body_end),
            offset: entity_offset,
        }));
        search = body_end;
    }
    entities
}

fn saved_spline_entities(
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> Vec<FeatureSavedEntity> {
    const LABEL: &[u8] = b"\xe0\x00save_entity_ptr(spline)\0";
    const POINTS: &[u8] = b"\xe0\x02i_pnts\0\xf9";
    let mut entities = Vec::new();
    let mut search = start;
    while let Some(entity_offset) = find_bytes(payload, LABEL, search, end) {
        let body_start = entity_offset + LABEL.len();
        let body_end = find_bytes(payload, LABEL, body_start, end).unwrap_or(end);
        let points_label = find_bytes(payload, POINTS, body_start, body_end);
        let entity_id_end = points_label.unwrap_or(body_end);
        let mut declared_point_count = None;
        let mut point_count = None;
        let mut points = Vec::new();
        let mut fields_start = body_start;
        if let Some(points_label) = points_label {
            let extents_start = points_label + POINTS.len();
            let (declared, dimensions_end) = psb::compact_int(payload, extents_start);
            let (coordinate_count, mut cursor) = psb::compact_int(payload, dimensions_end);
            if dimensions_end > extents_start && cursor > dimensions_end && coordinate_count == 3 {
                declared_point_count = Some(declared);
                point_count = usize::try_from(declared).ok().filter(|point_count| {
                    point_count.saturating_mul(3)
                        <= body_end.saturating_sub(cursor).saturating_mul(16).max(12)
                });
                if let Some(point_count) = point_count {
                    points.reserve(point_count);
                    for _ in 0..point_count {
                        let mut point = [0.0; 3];
                        let mut next_cursor = cursor;
                        let mut complete = true;
                        for coordinate in &mut point {
                            let Some((value, next)) =
                                scalar::decode_in_lane(payload, next_cursor, cache)
                                    .filter(|(_, next)| *next <= body_end)
                            else {
                                complete = false;
                                break;
                            };
                            *coordinate = value;
                            next_cursor = next;
                        }
                        if !complete {
                            break;
                        }
                        points.push(point);
                        cursor = next_cursor;
                    }
                    fields_start = cursor;
                }
            }
        }
        let endpoint_tangents = find_bytes(
            payload,
            b"\xe0\x02end_tangts\0\xf9\x02\x03",
            fields_start,
            body_end,
        )
        .and_then(|label| {
            let mut at = label + b"\xe0\x02end_tangts\0\xf9\x02\x03".len();
            let mut tangents = [[0.0; 3]; 2];
            for tangent in &mut tangents {
                for coordinate in tangent {
                    let (value, next) = scalar::decode_in_lane(payload, at, cache)?;
                    (next <= body_end).then_some(())?;
                    *coordinate = value;
                    at = next;
                }
            }
            Some(tangents)
        });
        let parameters = point_count.and_then(|point_count| {
            find_bytes(payload, b"\xe0\x02params\0\xf8", fields_start, body_end).and_then(|label| {
                let count_at = label + b"\xe0\x02params\0\xf8".len();
                let (count, mut at) = psb::compact_int(payload, count_at);
                (usize::try_from(count).ok() == Some(point_count) && at > count_at).then_some(())?;
                let mut values = Vec::with_capacity(point_count);
                for _ in 0..count {
                    let (value, next) = saved_spline_parameter(payload, at, cache)?;
                    (next <= body_end).then_some(())?;
                    values.push(value);
                    at = next;
                }
                Some(values)
            })
        });
        entities.push(FeatureSavedEntity::Spline(FeatureSavedSpline {
            entity_id: saved_entity_id(payload, body_start, entity_id_end),
            declared_point_count,
            interpolation_points: points,
            endpoint_tangents,
            parameters,
            offset: entity_offset,
        }));
        search = body_start;
    }
    entities
}

fn saved_spline_parameter(
    payload: &[u8],
    offset: usize,
    cache: &scalar::ScalarCache,
) -> Option<(f64, usize)> {
    let prefix = *payload.get(offset)?;
    if prefix == 0x18
        && payload
            .get(offset + 1)
            .is_some_and(|next| matches!(next, 0x2d | 0x6d | 0x85 | 0x93 | 0x9e))
    {
        return Some((0.0, offset + 1));
    }
    if matches!(prefix, 0x6d | 0x85 | 0x93 | 0x9e) {
        let tail = payload.get(offset + 1..offset + 7)?;
        let second = prefix.wrapping_sub(0x8b);
        let mut raw = [0; 8];
        raw[0] = if second >= 0x80 { 0x3f } else { 0x40 };
        raw[1] = second;
        raw[2..].copy_from_slice(tail);
        return Some((f64::from_be_bytes(raw), offset + 7));
    }
    if prefix == 0x2d {
        let tail = payload.get(offset + 1..offset + 8)?;
        let mut raw = [0; 8];
        raw[0] = 0x40;
        raw[1..].copy_from_slice(tail);
        return Some((f64::from_be_bytes(raw), offset + 8));
    }
    scalar::decode_in_lane(payload, offset, cache)
}

fn saved_entity_offset(entity: &FeatureSavedEntity) -> usize {
    match entity {
        FeatureSavedEntity::Line(entity) => entity.offset,
        FeatureSavedEntity::Arc(entity) => entity.offset,
        FeatureSavedEntity::Circle(entity) => entity.offset,
        FeatureSavedEntity::Spline(entity) => entity.offset,
        FeatureSavedEntity::Dummy(entity) => entity.offset,
    }
}

fn saved_section(
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
    order_table: Option<&FeatureOrderTable>,
    segments: Option<&FeatureSegmentTable>,
) -> Option<FeatureSavedSection> {
    let table = find_bytes(payload, b"\xe0\x00p_saved_result\0", start, end)?;
    let table_end = find_bytes(payload, b"\xe0\x02local_sys\0", table, end)
        .or_else(|| find_bytes(payload, b"\xe0\x00rigid_data\0", table, end))
        .unwrap_or(end);
    let mut entities = saved_line_entities(payload, table, table_end, cache);
    entities.extend(saved_circular_entities(
        payload,
        table,
        table_end,
        cache,
        order_table,
        segments,
    ));
    entities.extend(saved_dummy_entities(payload, table, table_end));
    entities.extend(saved_spline_entities(payload, start, end, cache));
    entities.sort_by_key(saved_entity_offset);
    Some(FeatureSavedSection {
        entities,
        offset: table,
    })
}

fn positional_saved_section(
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
    order_table: Option<&FeatureOrderTable>,
    segments: Option<&FeatureSegmentTable>,
) -> Option<FeatureSavedSection> {
    let mut entities =
        saved_positional_generated_entities(payload, start, end, cache, order_table, segments);
    entities.sort_by_key(saved_entity_offset);
    let offset = entities.first().map(saved_entity_offset)?;
    Some(FeatureSavedSection { entities, offset })
}

fn field_value(payload: &[u8]) -> FeatureFieldValue {
    if payload.is_empty() {
        return FeatureFieldValue::Empty;
    }
    if payload[0] == psb::token::SCALAR_BODY {
        let (dimensions, dimensions_end) = psb::compact_int(payload, 1);
        let (count, values_start) = psb::compact_int(payload, dimensions_end);
        let slot_count = usize::try_from(dimensions).ok().and_then(|dimensions| {
            usize::try_from(count)
                .ok()
                .and_then(|count| dimensions.checked_mul(count))
        });
        let Some(slot_count) = slot_count.filter(|slot_count| {
            dimensions_end > 1
                && values_start > dimensions_end
                && *slot_count
                    <= payload
                        .len()
                        .saturating_sub(values_start)
                        .saturating_mul(16)
                        .max(12)
        }) else {
            return FeatureFieldValue::Raw(payload.to_vec());
        };
        let cache = scalar::ScalarCache::from_section(payload);
        let decoded_values = decode_exact_scalars(&payload[values_start..], slot_count, &cache);
        return FeatureFieldValue::ScalarArray {
            dimensions,
            count,
            body: payload[values_start..].to_vec(),
            decoded_values,
        };
    }
    if payload[0] == psb::token::ENTITY_REF {
        if let Ok((entity_id, end)) = psb::reference_id(payload, 1) {
            let terminated = end + 1 == payload.len() && payload[end] == psb::token::ARRAY_CLOSE;
            if end == payload.len() || terminated {
                return FeatureFieldValue::EntityReference {
                    entity_id,
                    terminated,
                };
            }
        }
    }
    if payload[0] == psb::token::ARRAY_OPEN {
        let (count, mut cursor) = psb::compact_int(payload, 1);
        let mut values = Vec::new();
        for _ in 0..count {
            let (value, next) = psb::compact_int(payload, cursor);
            if next == cursor {
                return FeatureFieldValue::Raw(payload.to_vec());
            }
            values.push(value);
            cursor = next;
        }
        if cursor == payload.len()
            || cursor + 1 == payload.len() && payload[cursor] == psb::token::ARRAY_CLOSE
        {
            return FeatureFieldValue::CompactIntArray(values);
        }
    }
    let (value, end) = psb::compact_int(payload, 0);
    if end == payload.len() {
        FeatureFieldValue::CompactInt(value)
    } else {
        FeatureFieldValue::Raw(payload.to_vec())
    }
}

/// Decode named fields and their context-independent value wrappers inside
/// procedural choice spans.
pub fn choice_fields(choices: &[FeatureChoice]) -> Vec<FeatureChoiceField> {
    let mut fields = Vec::new();
    for choice in choices {
        let mut headers = Vec::new();
        for offset in 0..choice.payload.len().saturating_sub(2) {
            if choice.payload[offset] != psb::token::NAMED_RECORD {
                continue;
            }
            let Some(nul) = choice.payload[offset + 2..]
                .iter()
                .position(|&byte| byte == 0)
                .map(|relative| offset + 2 + relative)
            else {
                continue;
            };
            if choice.payload[offset + 2..nul]
                .iter()
                .all(u8::is_ascii_graphic)
            {
                headers.push((offset, nul + 1));
            }
        }
        for (index, &(header, value_start)) in headers.iter().enumerate() {
            let end = headers
                .get(index + 1)
                .map_or(choice.payload.len(), |hit| hit.0);
            if value_start > end {
                continue;
            }
            fields.push(FeatureChoiceField {
                feature_id: choice.feature_id,
                choice_label: choice.label.clone(),
                name: String::from_utf8_lossy(&choice.payload[header + 2..value_start - 1])
                    .into_owned(),
                type_byte: choice.payload[header + 1],
                value: field_value(&choice.payload[value_start..end]),
                offset: choice.payload_offset + header,
            });
        }
    }
    fields.sort_by_key(|field| field.offset);
    fields
}

/// Decode generated-geometry table headers from known feature rows.
pub fn geometry_tables(rows: &[FeatureRow]) -> Vec<FeatureGeometryTable> {
    const FIELDS: &[(&[u8], FeatureGeometryTableKind)] = &[
        (b"edg_id_tab_ptr", FeatureGeometryTableKind::EdgeIds),
        (b"lo_id_tab_ptr", FeatureGeometryTableKind::LoopIds),
        (b"bnd_type", FeatureGeometryTableKind::Boundaries),
        (b"used_bodies", FeatureGeometryTableKind::UsedBodies),
        (b"geom_lists", FeatureGeometryTableKind::GeometryLists),
        (b"dtm_id_tab", FeatureGeometryTableKind::DatumIds),
    ];
    let mut tables = Vec::new();
    let mut datum_class_by_stream = BTreeMap::<usize, u32>::new();
    for row in rows {
        for &(label, kind) in FIELDS {
            let needle = [label, b"\0"].concat();
            let mut from = 0;
            while let Some(relative) = row.body.get(from..).and_then(|tail| {
                tail.windows(needle.len())
                    .position(|window| window == needle)
            }) {
                let offset = from + relative;
                from = offset + needle.len();
                let Some((count, entity_class, entry_ids)) =
                    geometry_table_at(&row.body, offset + needle.len(), kind)
                else {
                    continue;
                };
                tables.push(FeatureGeometryTable {
                    feature_id: row.feature_id,
                    kind,
                    count,
                    entity_class,
                    entry_ids,
                    offset: row.body_offset + offset,
                });
                if kind == FeatureGeometryTableKind::DatumIds {
                    datum_class_by_stream.insert(row.stream_offset, entity_class);
                }
            }
        }
        let Some(&entity_class) = datum_class_by_stream.get(&row.stream_offset) else {
            continue;
        };
        for cursor in 0..row.body.len() {
            let Some((count, entry_ids)) =
                positional_datum_geometry_table_at(&row.body, cursor, entity_class)
            else {
                continue;
            };
            tables.push(FeatureGeometryTable {
                feature_id: row.feature_id,
                kind: FeatureGeometryTableKind::DatumIds,
                count,
                entity_class,
                entry_ids: Some(entry_ids),
                offset: row.body_offset + cursor,
            });
        }
    }
    tables.sort_by_key(|table| table.offset);
    tables
}

fn positional_datum_geometry_table_at(
    body: &[u8],
    cursor: usize,
    entity_class: u32,
) -> Option<(u32, Vec<u32>)> {
    (body.get(cursor) == Some(&psb::token::ARRAY_OPEN)).then_some(())?;
    let (count, after_count) = psb::compact_int(body, cursor + 1);
    (after_count > cursor + 1 && body.get(after_count) == Some(&psb::token::ENTITY_REF))
        .then_some(())?;
    let (stored_class, mut cursor) = psb::reference_id(body, after_count + 1).ok()?;
    (stored_class == entity_class).then_some(())?;
    if body.get(cursor) == Some(&psb::token::ARRAY_CLOSE) {
        cursor += 1;
    }
    if body.get(cursor) == Some(&0xe2) {
        cursor += 1;
    }

    let capacity = bounded_len(u64::from(count), 1, body.len().saturating_sub(cursor))?;
    let entry_class = entity_class.checked_add(1)?;
    let mut entry_ids = Vec::with_capacity(capacity);
    for index in 0..count {
        if index == 0 {
            (body.get(cursor) == Some(&psb::token::ENTITY_REF)).then_some(())?;
            let (stored_entry_class, after_class) = psb::reference_id(body, cursor + 1).ok()?;
            (stored_entry_class == entry_class).then_some(())?;
            cursor = after_class;
        } else {
            (body
                .get(cursor)
                .is_some_and(|byte| matches!(byte, 0xf1 | 0xf2)))
            .then_some(())?;
            (body.get(cursor + 1) == Some(&psb::token::ENTITY_REF)).then_some(())?;
            let (continuation_class, after_class) = psb::reference_id(body, cursor + 2).ok()?;
            (continuation_class == entity_class && body.get(after_class) == Some(&0xe2))
                .then_some(())?;
            cursor = after_class + 1;
        }
        let (entry_id, after_id) = psb::reference_id(body, cursor).ok()?;
        entry_ids.push(entry_id);
        cursor = after_id;
        if body.get(cursor) == Some(&0xf6) {
            cursor += 1;
        } else {
            let (_, after_dimension) = psb::reference_id(body, cursor).ok()?;
            cursor = after_dimension;
        }
    }
    Some((count, entry_ids))
}

fn geometry_table_at(
    body: &[u8],
    mut cursor: usize,
    kind: FeatureGeometryTableKind,
) -> Option<(u32, u32, Option<Vec<u32>>)> {
    if body
        .get(cursor)
        .is_some_and(|byte| matches!(byte, 0xf1 | 0xf2))
    {
        cursor += 1;
    }
    if body.get(cursor) != Some(&psb::token::ARRAY_OPEN) {
        return None;
    }
    let (count, after_count) = psb::compact_int(body, cursor + 1);
    if after_count == cursor + 1 || body.get(after_count) != Some(&psb::token::ENTITY_REF) {
        return None;
    }
    let (entity_class, mut after_class) = psb::reference_id(body, after_count + 1).ok()?;
    if body.get(after_class) == Some(&0xfb) {
        after_class += 1;
    }
    if body.get(after_class) == Some(&0xe2) {
        after_class += 1;
    }
    let entry_ids = if kind == FeatureGeometryTableKind::DatumIds {
        let mut entries = Vec::new();
        let mut entry_cursor = after_class;
        for _ in 0..count {
            const ENTRY: &[u8] = b"\xe0\x01dtm_id\0";
            if body.get(entry_cursor..entry_cursor + ENTRY.len()) != Some(ENTRY) {
                entries.clear();
                break;
            }
            let (entry, next) = psb::compact_int(body, entry_cursor + ENTRY.len());
            if next == entry_cursor + ENTRY.len() {
                entries.clear();
                break;
            }
            entries.push(entry);
            entry_cursor = next;
        }
        (entries.len() == usize::try_from(count).unwrap_or(usize::MAX)).then_some(entries)
    } else {
        None
    };
    Some((count, entity_class, entry_ids))
}

/// Decode complete named affected-ID arrays from known feature rows.
pub fn affected_ids(rows: &[FeatureRow]) -> Vec<FeatureAffectedIds> {
    const FIELDS: &[(&[u8], AffectedIdKind)] = &[
        (b"geoms_affected", AffectedIdKind::Geometry),
        (b"edgs_affected", AffectedIdKind::Edges),
        (b"strong_parents", AffectedIdKind::StrongParents),
        (b"parent_table", AffectedIdKind::Parents),
        (b"contours", AffectedIdKind::Contours),
    ];
    let mut result = Vec::new();
    for row in rows {
        for &(label, kind) in FIELDS {
            let needle = [label, b"\0"].concat();
            let mut from = 0;
            while let Some(relative) = row.body.get(from..).and_then(|tail| {
                tail.windows(needle.len())
                    .position(|window| window == needle)
            }) {
                let label_offset = from + relative;
                from = label_offset + needle.len();
                if label_offset < 2
                    || row.body[label_offset - 2] != psb::token::NAMED_RECORD
                    || row.body.get(from) != Some(&psb::token::ARRAY_OPEN)
                {
                    continue;
                }
                let (count, mut cursor) = psb::compact_int(&row.body, from + 1);
                if cursor == from + 1 {
                    continue;
                }
                // Each id is a compact int of at least one byte, so the count
                // cannot exceed the unread bytes of the row body.
                let Some(capacity) =
                    bounded_len(u64::from(count), 1, row.body.len().saturating_sub(cursor))
                else {
                    continue;
                };
                let mut ids = Vec::with_capacity(capacity);
                for _ in 0..count {
                    let (id, next) = psb::compact_int(&row.body, cursor);
                    if next == cursor {
                        ids.clear();
                        break;
                    }
                    ids.push(id);
                    cursor = next;
                }
                if ids.len() == count as usize {
                    result.push(FeatureAffectedIds {
                        feature_id: row.feature_id,
                        kind,
                        ids,
                        offset: row.body_offset + label_offset - 2,
                    });
                }
            }
        }
    }
    result.sort_by_key(|record| record.offset);
    result
}

fn skip_replay_field_label(run: &[u8], cursor: usize, expected: &[u8]) -> Option<usize> {
    if run.get(cursor) != Some(&psb::token::NAMED_RECORD) {
        return Some(cursor);
    }
    let name_end = run
        .get(cursor + 2..)?
        .iter()
        .position(|byte| *byte == 0)
        .map(|relative| cursor + 2 + relative)?;
    (run.get(cursor + 2..name_end) == Some(expected)).then_some(name_end + 1)
}

fn replay_extent(
    run: &[u8],
    cursor: usize,
    field_name: &[u8],
    inherited: Option<u32>,
) -> Option<(u32, ReplayExtentSource, usize)> {
    let cursor = skip_replay_field_label(run, cursor, field_name)?;
    if run.get(cursor) == Some(&psb::token::ARRAY_OPEN) {
        let (count, after) = psb::compact_int(run, cursor + 1);
        (after > cursor + 1).then_some((count, ReplayExtentSource::Explicit, after))
    } else {
        inherited.map(|count| (count, ReplayExtentSource::Inherited, cursor))
    }
}

fn skip_replay_position_reference(run: &[u8], cursor: usize) -> Option<usize> {
    if run.get(cursor) != Some(&psb::token::ENTITY_REF) {
        return Some(cursor);
    }
    let (_, after) = psb::reference_id(run, cursor + 1).ok()?;
    (run.get(after) == Some(&psb::token::ARRAY_OPEN)).then_some(after)
}

fn replay_ids(run: &[u8], count: u32, mut cursor: usize) -> Option<(Vec<u32>, usize)> {
    // Each id is a compact int of at least one byte, so the count cannot exceed
    // the unread bytes of the run.
    bounded_len(u64::from(count), 1, run.len().saturating_sub(cursor))?;
    let mut ids = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let (id, after) = psb::compact_int(run, cursor);
        if after == cursor {
            return None;
        }
        ids.push(id);
        cursor = after;
    }
    Some((ids, cursor))
}

struct ReplayAffectedPair {
    geometry_ids: Vec<u32>,
    edge_ids: Vec<u32>,
    geometry_extent: ReplayExtentSource,
    edge_extent: ReplayExtentSource,
    consumed: usize,
}

fn replay_affected_pair(run: &[u8], extents: [Option<u32>; 2]) -> Option<ReplayAffectedPair> {
    let (geometry_count, geometry_extent, cursor) =
        replay_extent(run, 0, b"geoms_affected", extents[0])?;
    let (geometry_ids, cursor) = replay_ids(run, geometry_count, cursor)?;
    let cursor = skip_replay_position_reference(run, cursor)?;
    let (edge_count, edge_extent, cursor) =
        replay_extent(run, cursor, b"edgs_affected", extents[1])?;
    let (edge_ids, cursor) = replay_ids(run, edge_count, cursor)?;
    Some(ReplayAffectedPair {
        geometry_ids,
        edge_ids,
        geometry_extent,
        edge_extent,
        consumed: cursor,
    })
}

fn unique_unanchored_replay_pair(
    row: &FeatureRow,
    extents: [Option<u32>; 2],
) -> Option<(ReplayAffectedPair, usize)> {
    let mut candidates = Vec::new();
    for suffix in row
        .body
        .windows(2)
        .enumerate()
        .filter_map(|(offset, window)| (window == [0xe1, 0xe1]).then_some(offset))
    {
        let (row_id, after_id) = psb::compact_int(&row.body, suffix + 2);
        if after_id == suffix + 2 || row.body.get(after_id) != Some(&psb::token::COMPOUND_CLOSE) {
            continue;
        }
        let selector_start = if row.body.get(after_id + 1) == Some(&psb::token::COMPOUND_CLOSE) {
            after_id + 2
        } else if row.body.get(after_id + 1) == Some(&psb::token::ENTITY_REF) {
            let Ok((_, after_reference)) = psb::reference_id(&row.body, after_id + 2) else {
                continue;
            };
            if row.body.get(after_reference) != Some(&psb::token::COMPOUND_CLOSE) {
                continue;
            }
            after_reference + 1
        } else {
            continue;
        };
        let (_, after_selector) = psb::compact_int(&row.body, selector_start);
        let (repeated_row_id, after_repeated_id) = psb::compact_int(&row.body, after_selector);
        if after_selector == selector_start
            || after_repeated_id == after_selector
            || repeated_row_id != row_id
            || row.body.get(after_repeated_id..after_repeated_id + 4)
                != Some(&[0x00, 0xe1, 0x00, psb::token::COMPOUND_CLOSE])
        {
            continue;
        }
        for start in 1..suffix {
            if row.body[start - 1] != psb::token::COMPOUND_CLOSE {
                continue;
            }
            let Some(pair) = replay_affected_pair(&row.body[start..suffix], extents) else {
                continue;
            };
            if pair.consumed == suffix - start {
                candidates.push((pair, start));
            }
        }
    }
    (candidates.len() == 1).then_some(())?;
    candidates.pop()
}

/// Decode the two affected-ID array positions in class-913 and class-914 replay rows.
///
/// Array extents are stateful within one `AllFeatur` stream and schema class.
/// An omitted `f8` opener reuses the preceding extent at the same array
/// position.
pub fn replay_affected_ids(rows: &[FeatureRow]) -> Vec<FeatureReplayAffectedIds> {
    const ANCHOR_PREFIX: &[u8] = &[0xf1, 0xf7, 0x42];
    const ANCHOR_SUFFIX: &[u8] = &[0x80, 0x01, 0xe3];
    const ANCHOR_LEN: usize = ANCHOR_PREFIX.len() + 1 + ANCHOR_SUFFIX.len();
    const TERMINATOR: &[u8] = &[0xf5, 0x96, 0x92];
    let mut result = Vec::new();
    let mut extents = BTreeMap::<(usize, u32), [Option<u32>; 2]>::new();
    for row in rows {
        let Some(schema_class @ (913 | 914)) = row.root_schema_class else {
            continue;
        };
        let anchor = row.body.windows(ANCHOR_LEN).rposition(|window| {
            window.starts_with(ANCHOR_PREFIX)
                && matches!(window[ANCHOR_PREFIX.len()], 0xc8 | 0xd8)
                && window.ends_with(ANCHOR_SUFFIX)
        });
        let state = extents
            .entry((row.stream_offset, schema_class))
            .or_default();
        let (pair, source_offset) = if let Some(anchor) = anchor {
            let run_start = anchor + ANCHOR_LEN;
            let Some(term_relative) = row.body[run_start..]
                .windows(TERMINATOR.len())
                .position(|window| window == TERMINATOR)
            else {
                continue;
            };
            let run = &row.body[run_start..run_start + term_relative];
            let Some(pair) = replay_affected_pair(run, *state) else {
                continue;
            };
            (pair, anchor)
        } else {
            let Some(pair) = unique_unanchored_replay_pair(row, *state) else {
                continue;
            };
            pair
        };
        let ReplayAffectedPair {
            geometry_ids,
            edge_ids,
            geometry_extent,
            edge_extent,
            ..
        } = pair;
        state[0] = Some(geometry_ids.len() as u32);
        state[1] = Some(edge_ids.len() as u32);
        result.push(FeatureReplayAffectedIds {
            feature_id: row.feature_id,
            geometry_ids,
            edge_ids,
            geometry_extent,
            edge_extent,
            offset: row.body_offset + source_offset,
        });
    }
    result.sort_by_key(|record| record.offset);
    result
}

/// Decode named `direction` and `direction2` compact integers inside
/// `lo_restore` records.
pub fn loop_restore_directions(rows: &[FeatureRow]) -> Vec<FeatureLoopRestoreDirection> {
    const FIELDS: &[(&[u8], LoopRestoreDirectionLane)] = &[
        (b"direction", LoopRestoreDirectionLane::Primary),
        (b"direction2", LoopRestoreDirectionLane::Secondary),
    ];
    let mut result = Vec::new();
    for row in rows {
        for &(label, lane) in FIELDS {
            let needle = [label, b"\0"].concat();
            let mut from = 0;
            while let Some(relative) = row.body.get(from..).and_then(|tail| {
                tail.windows(needle.len())
                    .position(|window| window == needle)
            }) {
                let label_offset = from + relative;
                from = label_offset + needle.len();
                if label_offset < 2
                    || row.body[label_offset - 2] != psb::token::NAMED_RECORD
                    || row.body[label_offset - 1] != 1
                    || !row.body[..label_offset - 2]
                        .windows(b"lo_restore\0".len())
                        .any(|window| window == b"lo_restore\0")
                {
                    continue;
                }
                let (value, after) = psb::compact_int(&row.body, from);
                if after == from {
                    continue;
                }
                result.push(FeatureLoopRestoreDirection {
                    feature_id: row.feature_id,
                    lane,
                    value,
                    offset: row.body_offset + label_offset - 2,
                });
            }
        }
    }
    result.sort_by_key(|record| record.offset);
    result
}

/// Decode complete ordered `lo_hist` rosters paired with named loop tables.
pub fn loop_history_entries(
    rows: &[FeatureRow],
    geometry_tables: &[FeatureGeometryTable],
) -> Vec<FeatureLoopHistoryEntry> {
    const LABEL: &[u8] = b"\xe0\x01lo_hist\0";
    const RECORD_WIDTH: u32 = 6;
    let mut result = Vec::new();
    for table in geometry_tables
        .iter()
        .filter(|table| table.kind == FeatureGeometryTableKind::LoopIds)
    {
        let Some(row) = rows.iter().find(|row| {
            row.feature_id == table.feature_id
                && table.offset >= row.body_offset
                && table.offset < row.body_offset.saturating_add(row.body.len())
        }) else {
            continue;
        };
        let table_offset = table.offset - row.body_offset;
        let Some(label_offset) = row.body[table_offset..]
            .windows(LABEL.len())
            .position(|window| window == LABEL)
            .map(|offset| table_offset + offset)
        else {
            continue;
        };
        let label_stream_offset = row.body_offset + label_offset;
        if geometry_tables.iter().any(|other| {
            other.kind == FeatureGeometryTableKind::LoopIds
                && other.feature_id == table.feature_id
                && other.offset > table.offset
                && other.offset < label_stream_offset
        }) {
            continue;
        }
        let array_offset = label_offset + LABEL.len();
        if row.body.get(array_offset) != Some(&psb::token::ARRAY_OPEN) {
            continue;
        }
        let (width, roster_offset) = psb::compact_int(&row.body, array_offset + 1);
        if width != RECORD_WIDTH || roster_offset == array_offset + 1 {
            continue;
        }
        let Ok(count) = usize::try_from(table.count) else {
            continue;
        };
        let Some(entries) = loop_history_roster(&row.body, roster_offset, count) else {
            continue;
        };
        result.extend((0..table.count).zip(entries).map(|(ordinal, entry)| {
            FeatureLoopHistoryEntry {
                feature_id: row.feature_id,
                ordinal,
                loop_id: entry.loop_id,
                field_bytes: entry.field_bytes,
                boundary: entry.boundary,
                offset: row.body_offset + entry.offset,
                end_offset: row.body_offset + entry.end_offset,
            }
        }));
    }
    result.sort_by_key(|entry| entry.offset);
    result
}

fn loop_history_roster(
    body: &[u8],
    mut cursor: usize,
    count: usize,
) -> Option<Vec<ParsedLoopHistoryEntry>> {
    const FIELD_COUNT: usize = 4;
    (count > 0 && count <= body.len().saturating_sub(cursor) / 2).then_some(())?;
    let mut entries = Vec::with_capacity(count);
    for index in 0..count {
        let offset = cursor;
        let (loop_id, after_id) = psb::compact_int(body, cursor);
        (after_id > cursor && body[cursor] <= 0xbf).then_some(())?;
        cursor = after_id;
        let mut field_bytes = Vec::with_capacity(FIELD_COUNT + 1);
        for _ in 0..FIELD_COUNT {
            let token = psb::token_at(body, cursor)?;
            (!matches!(
                token.kind,
                psb::TokenKind::CompoundClose | psb::TokenKind::Truncated(_)
            ))
            .then_some(())?;
            field_bytes.push(
                body.get(cursor..cursor.checked_add(token.length)?)?
                    .to_vec(),
            );
            cursor = cursor.checked_add(token.length)?;
        }
        let boundary = if body.get(cursor) == Some(&0xe3) {
            cursor += 1;
            FeatureLoopHistoryBoundary::CompoundClose
        } else if body
            .get(cursor)
            .is_some_and(|byte| matches!(byte, 0xf1 | 0xf2))
        {
            let marker = body[cursor];
            (body.get(cursor + 1) == Some(&psb::token::ENTITY_REF)).then_some(())?;
            let (reference, after_reference) = psb::reference_id(body, cursor + 2).ok()?;
            (body.get(after_reference) == Some(&0xe3)).then_some(())?;
            cursor = after_reference + 1;
            if marker == 0xf1 {
                FeatureLoopHistoryBoundary::ReferenceContinue(reference)
            } else {
                FeatureLoopHistoryBoundary::ReferenceFinal(reference)
            }
        } else {
            (index + 1 == count).then_some(())?;
            let token = psb::token_at(body, cursor)?;
            if token.kind != psb::TokenKind::NamedRecord {
                (!matches!(
                    token.kind,
                    psb::TokenKind::CompoundClose | psb::TokenKind::Truncated(_)
                ))
                .then_some(())?;
                field_bytes.push(
                    body.get(cursor..cursor.checked_add(token.length)?)?
                        .to_vec(),
                );
                cursor = cursor.checked_add(token.length)?;
                matches!(
                    psb::token_at(body, cursor).map(|token| token.kind),
                    Some(psb::TokenKind::NamedRecord)
                )
                .then_some(())?;
            }
            FeatureLoopHistoryBoundary::NamedRecord
        };
        entries.push(ParsedLoopHistoryEntry {
            loop_id,
            field_bytes,
            boundary,
            offset,
            end_offset: cursor,
        });
    }
    Some(entries)
}

struct ParsedLoopHistoryEntry {
    loop_id: u32,
    field_bytes: Vec<Vec<u8>>,
    boundary: FeatureLoopHistoryBoundary,
    offset: usize,
    end_offset: usize,
}

/// Decode full-turn rotational termination from the positional
/// `param_choice_ptr` body of section-sweep feature rows.
pub fn revolution_extents(rows: &[FeatureRow]) -> Vec<FeatureRevolutionExtent> {
    const PARAMETER_CHOICE_PREFIX: &[u8] = &[0x83, 0xdf, 0xf6, 0xe3];
    const FULL_TURN_CHOICES: &[u8] = &[
        0x00, 0x00, 0xea, 0x44, 0x00, 0x00, 0xf6, 0xf6, 0xf6, 0x00, 0x00, 0x00, 0x00,
    ];
    let mut result = Vec::new();
    for row in rows {
        if !matches!(row.root_schema_class, Some(916 | 917)) {
            continue;
        }
        let Some(schema_end) = (0..row.body.len().min(20)).find_map(|offset| {
            if row.body.get(offset..offset + 2) != Some(&[0xe3, 0xf6]) {
                return None;
            }
            let (schema_class, after) = psb::compact_int(&row.body, offset + 2);
            (Some(schema_class) == row.root_schema_class && row.body.get(after) == Some(&0xe1))
                .then_some(after + 1)
        }) else {
            continue;
        };
        if row.body.get(schema_end) != Some(&2) {
            continue;
        }
        let Some(choice_start) = row.body[schema_end + 1..row.body.len().min(64)]
            .windows(PARAMETER_CHOICE_PREFIX.len())
            .position(|window| window == PARAMETER_CHOICE_PREFIX)
            .map(|relative| schema_end + 1 + relative + PARAMETER_CHOICE_PREFIX.len())
        else {
            continue;
        };
        if row
            .body
            .get(choice_start..choice_start + FULL_TURN_CHOICES.len())
            != Some(FULL_TURN_CHOICES)
        {
            continue;
        }
        result.push(FeatureRevolutionExtent {
            feature_id: row.feature_id,
            kind: FeatureRevolutionExtentKind::FullTurn,
            offset: row.body_offset + choice_start + 2,
        });
    }
    result.sort_by_key(|record| record.offset);
    result
}

/// Decode full-turn termination stored inside an owned DEPDB section
/// definition. The owning current-state recipe must independently select a
/// rotational sweep.
pub fn definition_revolution_extents(
    definitions: &[FeatureDefinition],
    operations: &[FeatureOperation],
) -> Vec<FeatureRevolutionExtent> {
    const FULL_TURN: &[u8] = &[
        0x83, 0xdf, 0xf6, 0xe3, 0x00, 0x00, 0xea, 0x44, 0x00, 0x00, 0xf6, 0xf6, 0xf6, 0x00, 0x00,
        0x00, 0x00,
    ];
    let mut result = Vec::new();
    for definition in definitions {
        let Some(feature_id) = definition.owner_feature_id else {
            continue;
        };
        let recipe_matches = operations.iter().any(|operation| {
            operation.feature_id == feature_id
                && operation
                    .recipe
                    .is_some_and(|recipe| recipe.kind() == FeatureRecipeKind::Revolve)
        });
        if !recipe_matches {
            continue;
        }
        let offsets = definition
            .body
            .windows(FULL_TURN.len())
            .enumerate()
            .filter_map(|(offset, window)| (window == FULL_TURN).then_some(offset))
            .collect::<Vec<_>>();
        result.extend(offsets.into_iter().map(|offset| FeatureRevolutionExtent {
            feature_id,
            kind: FeatureRevolutionExtentKind::FullTurn,
            offset: definition.offset + offset + 6,
        }));
    }
    result.sort_by_key(|record| record.offset);
    result
}

fn definitions_in_ranges(
    payload: &[u8],
    starts: &[(usize, u32, Option<u32>, bool)],
) -> Vec<FeatureDefinition> {
    let cache = scalar::ScalarCache::from_section(payload);
    let mut result = Vec::new();
    let mut replay_dimension_class = None;
    let mut replay_variable_class = None;
    let mut replay_relation_class = None;
    let mut replay_skamp_class = None;
    let mut replay_triples_class = None;
    let mut replay_trim_entity_classes: Option<TrimTableClasses> = None;
    let mut replay_trim_vertex_classes: Option<TrimTableClasses> = None;
    let mut replay_order_class = None;
    for (index, &(start, id, owner_override, positional)) in starts.iter().enumerate() {
        let end = starts
            .get(index + 1)
            .map_or(payload.len(), |&(offset, _, _, _)| offset);
        let schema_end = starts[index + 1..]
            .iter()
            .find(|(_, _, _, positional)| !positional)
            .map_or(payload.len(), |(offset, _, _, _)| *offset);
        let mut parameter_frames = Vec::new();
        for &(label, kind) in &[
            (
                b"local_sys".as_slice(),
                FeatureParameterFrameKind::LocalSystem,
            ),
            (b"transf".as_slice(), FeatureParameterFrameKind::Transform),
        ] {
            let needle = [label, b"\0\xf9\x04\x03"].concat();
            let mut from = start;
            while let Some(relative) = payload[from..end]
                .windows(needle.len())
                .position(|window| window == needle)
            {
                let field_offset = from + relative;
                let body_start = field_offset + needle.len();
                let body_end = payload[body_start..end]
                    .windows(1)
                    .position(|window| window[0] == psb::token::NAMED_RECORD)
                    .map_or(end, |relative| body_start + relative);
                let body = payload[body_start..body_end].to_vec();
                parameter_frames.push(FeatureParameterFrame {
                    kind,
                    decoded_values: scalar::decode_feature_local_system_slots(&body, &cache)
                        .map(|slots| slots.to_vec()),
                    body,
                    offset: field_offset,
                });
                from = body_start;
            }
        }
        parameter_frames.sort_by_key(|frame| frame.offset);
        let mut outlines = Vec::new();
        if let Some(info) = find_bytes(payload, b"\xe0\x00feat_outl_info\0", start, end) {
            if let Some(label) = find_bytes(payload, b"outline\0\xf9\x02\x03", info, end) {
                let scalar_start = label + b"outline\0\xf9\x02\x03".len();
                outlines.push(FeatureOutline {
                    phase: OutlinePhase::PreRollback,
                    local_values: decode_optional_scalars(payload, scalar_start, 6, &cache),
                    offset: label,
                });
            }
            for &(label, phase) in &[
                (
                    b"\xe0\x00post_roll_back\0".as_slice(),
                    OutlinePhase::PostRollback,
                ),
                (b"\xe0\x00post_regen\0".as_slice(), OutlinePhase::PostRegen),
            ] {
                let Some(label_offset) = find_bytes(payload, label, info, end) else {
                    continue;
                };
                let framing = label_offset + label.len();
                if payload.get(framing..framing + 2) != Some(&[0xe3, psb::token::ENTITY_REF]) {
                    continue;
                }
                let Ok((_, after_ref)) = psb::reference_id(payload, framing + 2) else {
                    continue;
                };
                if payload.get(after_ref..after_ref + 3) != Some(&[0xf5, 0x96, 0x92])
                    || after_ref + 4 > end
                {
                    continue;
                }
                outlines.push(FeatureOutline {
                    phase,
                    local_values: decode_optional_scalars(payload, after_ref + 4, 6, &cache),
                    offset: label_offset,
                });
            }
        }
        outlines.sort_by_key(|outline| outline.offset);
        let variables = variable_table(payload, start, end, &cache).or_else(|| {
            positional
                .then(|| {
                    positional_variable_table(payload, start, end, replay_variable_class?, &cache)
                })
                .flatten()
        });
        if !positional {
            replay_variable_class = variables.as_ref().and_then(|table| table.entity_ref);
        }
        let segments = segment_table(payload, start, end).or_else(|| {
            positional
                .then(|| positional_segment_table(payload, start, end))
                .flatten()
        });
        let trim_entities = trim_entity_table(payload, start, end).or_else(|| {
            if positional {
                positional_trim_entity_table(
                    payload,
                    start,
                    end,
                    replay_trim_entity_classes?,
                    replay_trim_vertex_classes.map(|classes| classes.table),
                )
            } else {
                None
            }
        });
        if !positional {
            replay_trim_entity_classes =
                trim_table_header(payload, b"ent_tab\0", start, end).map(|header| header.classes);
        }
        let trim_vertices =
            trim_vertex_table(payload, start, end, segments.as_ref(), variables.as_ref()).or_else(
                || {
                    if positional {
                        positional_trim_vertex_table(
                            payload,
                            start,
                            end,
                            replay_trim_vertex_classes?,
                            segments.as_ref(),
                            variables.as_ref(),
                        )
                    } else {
                        None
                    }
                },
            );
        if !positional {
            replay_trim_vertex_classes =
                trim_table_header(payload, b"vert_tab\0", start, end).map(|header| header.classes);
        }
        let order_table = order_table(payload, start, end).or_else(|| {
            positional
                .then(|| positional_order_table(payload, start, end, replay_order_class?))
                .flatten()
        });
        if !positional {
            replay_order_class = order_table.as_ref().and_then(|table| table.entity_ref);
        }
        let section_3d = section_3d(payload, start, end).or_else(|| {
            positional
                .then(|| positional_section_3d(payload, start, end))
                .flatten()
        });
        let dimensions = dimension_table(payload, start, end, &cache).or_else(|| {
            positional.then_some(()).and_then(|()| {
                replay_dimension_class
                    .and_then(|table_class| {
                        positional_dimension_table(payload, start, end, table_class, &cache)
                    })
                    .or_else(|| {
                        self_described_positional_dimension_table(payload, start, end, &cache)
                    })
            })
        });
        if !positional {
            replay_dimension_class = dimensions.as_ref().and_then(|table| table.entity_ref);
        }
        let mut relations = relation_table(payload, start, end).or_else(|| {
            positional
                .then(|| positional_relation_table(payload, start, end, replay_relation_class?))
                .flatten()
        });
        if !positional {
            replay_relation_class = relations.as_ref().and_then(|table| table.entity_ref);
            replay_skamp_class = named_array_class(payload, b"skamp_ptr\0", start, schema_end);
            replay_triples_class = named_array_class(payload, b"triples_ptr\0", start, schema_end);
        } else if let Some(table) = &mut relations {
            table.skamps = replay_skamp_class.map_or_else(Vec::new, |table_class| {
                positional_feature_skamps(payload, start, end, table_class)
            });
            table.skamp_header = replay_skamp_class.and_then(|table_class| {
                positional_solver_table_header(payload, start, end, table_class)
            });
            table.triples = replay_triples_class.map_or_else(Vec::new, |table_class| {
                positional_relation_triples(payload, start, end, table_class)
            });
            table.triples_header = replay_triples_class.and_then(|table_class| {
                positional_solver_table_header(payload, start, end, table_class)
            });
        }
        let saved_section = saved_section(
            payload,
            start,
            end,
            &cache,
            order_table.as_ref(),
            segments.as_ref(),
        )
        .or_else(|| {
            if positional {
                positional_saved_section(
                    payload,
                    start,
                    end,
                    &cache,
                    order_table.as_ref(),
                    segments.as_ref(),
                )
            } else {
                None
            }
        });
        let owner_feature_id = owner_override.or_else(|| {
            let ids = contextual_references(payload, start, end, b"feat_id", b"gsec2d_ptr")
                .into_iter()
                .map(|(_, id)| id)
                .collect::<BTreeSet<_>>();
            ids.first().copied().filter(|_| ids.len() == 1)
        });
        result.push(FeatureDefinition {
            id,
            owner_feature_id,
            body: payload[start..end].to_vec(),
            parameter_frames,
            outlines,
            variables,
            segments,
            trim_entities,
            trim_vertices,
            order_table,
            section_3d,
            dimensions,
            relations,
            saved_section,
            offset: start,
        });
    }
    result
}

fn contextual_references(
    payload: &[u8],
    start: usize,
    end: usize,
    field: &[u8],
    following_record: &[u8],
) -> Vec<(usize, u32)> {
    let needle = [&[psb::token::NAMED_RECORD, 1][..], field, &[0]].concat();
    payload[start..end]
        .windows(needle.len())
        .enumerate()
        .filter_map(|(relative, window)| {
            if window != needle {
                return None;
            }
            let record_start = start + relative;
            let value_start = record_start + needle.len();
            let (value, after_value) = psb::reference_id(payload, value_start).ok()?;
            let following_end = after_value.checked_add(3 + following_record.len())?;
            (following_end <= end
                && payload.get(after_value..after_value + 2)
                    == Some(&[psb::token::NAMED_RECORD, 0])
                && payload.get(after_value + 2..following_end - 1) == Some(following_record)
                && payload.get(following_end - 1) == Some(&0))
            .then_some((record_start, value))
        })
        .collect()
}

/// Decode `FeatDefs` feature-definition records and their `f9 04 03`
/// definition-space parameter frames.
fn definition_starts(payload: &[u8]) -> Vec<(usize, u32, Option<u32>, bool)> {
    const PREFIX: &[u8] = b"feat_defs_";
    let mut starts = Vec::new();
    for offset in 0..payload.len() {
        if payload.get(offset..offset + PREFIX.len()) != Some(PREFIX) {
            continue;
        }
        let digits_start = offset + PREFIX.len();
        let Some(nul_relative) = payload[digits_start..].iter().position(|&byte| byte == 0) else {
            continue;
        };
        let digits = &payload[digits_start..digits_start + nul_relative];
        if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
            continue;
        }
        let Ok(id) = String::from_utf8_lossy(digits).parse::<u32>() else {
            continue;
        };
        starts.push((offset, id, None, false));
    }
    starts.sort_unstable_by_key(|&(offset, _, _, _)| offset);
    let labeled_starts = starts.clone();
    for (index, &(start, _, _, _)) in labeled_starts.iter().enumerate() {
        let end = labeled_starts
            .get(index + 1)
            .map_or(payload.len(), |&(offset, _, _, _)| offset);
        for (offset, owner) in
            contextual_references(payload, start, end, b"feat_id", b"ref_model_info")
        {
            starts.push((offset, owner, Some(owner), true));
        }
    }
    starts.sort_unstable_by_key(|&(offset, _, _, _)| offset);
    starts.dedup_by_key(|entry| entry.0);
    starts
}

fn depdb_gsec2d_starts(payload: &[u8]) -> Vec<(usize, u32, Option<u32>, bool)> {
    const GSEC: &[u8] = b"gsec2d_ptr\0";
    const NAME: &[u8] = b"name\0S2D";
    payload
        .windows(GSEC.len())
        .enumerate()
        .filter_map(|(start, window)| {
            (window == GSEC).then_some(())?;
            let search_end = start.saturating_add(128).min(payload.len());
            let digits_start = find_bytes(payload, NAME, start, search_end)? + NAME.len();
            let digits_end = payload[digits_start..search_end]
                .iter()
                .position(|byte| *byte == 0)?
                + digits_start;
            let digits = payload.get(digits_start..digits_end)?;
            if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
                return None;
            }
            let id = String::from_utf8_lossy(digits).parse::<u32>().ok()?;
            Some((start, id, None, false))
        })
        .collect()
}

/// Decode `FeatDefs` feature-definition records and their `f9 04 03`
/// definition-space parameter frames.
pub fn definitions(payload: &[u8]) -> Vec<FeatureDefinition> {
    let mut starts = definition_starts(payload);
    let retained_offsets = starts
        .iter()
        .map(|(offset, _, _, _)| *offset)
        .collect::<BTreeSet<_>>();
    let replay_markers = s2d_replay_starts(payload);
    let claimed_markers = claimed_s2d_replay_markers(payload, &starts, &replay_markers);
    let replay_starts = replay_markers
        .into_iter()
        .filter(|offset| !claimed_markers.contains(offset))
        .map(|offset| (offset, inherited_definition_id(&starts, offset), None, true))
        .collect::<Vec<_>>();
    starts.extend(replay_starts);
    starts.sort_unstable_by_key(|&(offset, _, _, _)| offset);
    starts.dedup_by_key(|entry| entry.0);
    definitions_in_ranges(payload, &starts)
        .into_iter()
        .filter(|definition| retained_offsets.contains(&definition.offset))
        .collect()
}

/// Decode labelled and positional feature definitions embedded directly in a
/// DEPDB section. A labelled `gsec2d_ptr` definition supplies the table schema
/// for its following positional `S2D` instances.
pub fn depdb_definitions(payload: &[u8]) -> Vec<FeatureDefinition> {
    let mut starts = definition_starts(payload);
    starts.extend(depdb_gsec2d_starts(payload));
    let replay_markers = s2d_replay_starts(payload);
    let claimed_markers = claimed_s2d_replay_markers(payload, &starts, &replay_markers);
    let replay_starts = replay_markers
        .into_iter()
        .filter(|offset| !claimed_markers.contains(offset))
        .map(|offset| (offset, inherited_definition_id(&starts, offset), None, true))
        .collect::<Vec<_>>();
    starts.extend(replay_starts);
    starts.sort_unstable_by_key(|&(offset, _, _, _)| offset);
    starts.dedup_by_key(|entry| entry.0);
    definitions_in_ranges(payload, &starts)
}

fn s2d_replay_starts(payload: &[u8]) -> Vec<usize> {
    const PREFIX: &[u8] = b"\xe3S2D";
    payload
        .windows(PREFIX.len())
        .enumerate()
        .filter_map(|(offset, window)| {
            if window != PREFIX {
                return None;
            }
            let suffix = payload.get(offset + PREFIX.len()..)?;
            let nul = suffix.iter().take(12).position(|byte| *byte == 0)?;
            (nul > 0 && suffix[..nul].iter().all(u8::is_ascii_digit)).then_some(offset)
        })
        .collect()
}

fn inherited_definition_id(
    starts: &[(usize, u32, Option<u32>, bool)],
    replay_offset: usize,
) -> u32 {
    starts
        .iter()
        .filter(|(offset, _, _, positional)| !positional && *offset < replay_offset)
        .max_by_key(|(offset, _, _, _)| *offset)
        .map_or(0, |(_, id, _, _)| *id)
}

fn claimed_s2d_replay_markers(
    payload: &[u8],
    starts: &[(usize, u32, Option<u32>, bool)],
    replay_markers: &[usize],
) -> BTreeSet<usize> {
    starts
        .iter()
        .enumerate()
        .filter(|(_, (_, _, _, positional))| *positional)
        .filter_map(|(index, (start, _, _, _))| {
            let end = starts
                .get(index + 1)
                .map_or(payload.len(), |(offset, _, _, _)| *offset);
            replay_markers
                .iter()
                .copied()
                .find(|marker| marker >= start && *marker < end)
        })
        .collect()
}

/// Decode unlabeled positional `S2D` replay instances without assigning an
/// owner. Ownership remains absent unless an independent entity join proves it.
pub fn positional_replay_definitions(payload: &[u8]) -> Vec<FeatureDefinition> {
    let mut starts = definition_starts(payload);
    let replay_markers = s2d_replay_starts(payload);
    let claimed_markers = claimed_s2d_replay_markers(payload, &starts, &replay_markers);
    let pending_offsets = replay_markers
        .into_iter()
        .filter(|offset| !claimed_markers.contains(offset))
        .collect::<BTreeSet<_>>();
    let replay_starts = pending_offsets
        .iter()
        .copied()
        .map(|offset| (offset, inherited_definition_id(&starts, offset), None, true))
        .collect::<Vec<_>>();
    starts.extend(replay_starts);
    starts.sort_unstable_by_key(|&(offset, _, _, _)| offset);
    starts.dedup_by_key(|entry| entry.0);
    definitions_in_ranges(payload, &starts)
        .into_iter()
        .filter(|definition| pending_offsets.contains(&definition.offset))
        .collect()
}

/// Decode one standalone DEPDB `gsec2d_ptr` section whose owner is established
/// by the section's unique procedural-recipe record.
pub fn depdb_section_definition(
    payload: &[u8],
    owner_feature_id: u32,
) -> Option<FeatureDefinition> {
    const GSEC: &[u8] = b"gsec2d_ptr\0";
    const NAME: &[u8] = b"name\0S2D";
    const PREFIX: &[u8] = b"feat_defs_";
    let starts = payload
        .windows(GSEC.len())
        .enumerate()
        .filter_map(|(offset, window)| (window == GSEC).then_some(offset))
        .collect::<Vec<_>>();
    let [start] = starts.as_slice() else {
        return None;
    };
    let name_search_end = start.saturating_add(128).min(payload.len());
    let name = find_bytes(payload, NAME, *start, name_search_end)? + NAME.len();
    let name_end = payload[name..name_search_end]
        .iter()
        .position(|byte| *byte == 0)?
        + name;
    let digits = payload.get(name..name_end)?;
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let section_id = String::from_utf8_lossy(digits).parse::<u32>().ok()?;
    let end =
        find_bytes(payload, PREFIX, *start + GSEC.len(), payload.len()).unwrap_or(payload.len());
    definitions_in_ranges(
        &payload[..end],
        &[(*start, section_id, Some(owner_feature_id), true)],
    )
    .pop()
}

/// Bind an owner omitted by `feat_id` through the section's unique generated
/// datum entry. An explicit canonical `feat_id` remains authoritative.
pub fn bind_definition_owners(
    definitions: &mut [FeatureDefinition],
    geometry_tables: &[FeatureGeometryTable],
) {
    for definition in definitions
        .iter_mut()
        .filter(|definition| definition.owner_feature_id.is_none())
    {
        let Some(sketch_plane) = definition
            .section_3d
            .as_ref()
            .and_then(|section| section.sketch_plane_entity_id)
        else {
            continue;
        };
        let owners = geometry_tables
            .iter()
            .filter(|table| table.kind == FeatureGeometryTableKind::DatumIds)
            .filter(|table| {
                table
                    .entry_ids
                    .as_ref()
                    .is_some_and(|ids| ids.contains(&sketch_plane))
            })
            .map(|table| table.feature_id)
            .collect::<BTreeSet<_>>();
        if let [owner] = owners.into_iter().collect::<Vec<_>>().as_slice() {
            definition.owner_feature_id = Some(*owner);
        }
    }
}

/// Bind instantiated saved sections through the exact set of trimmed section
/// entities copied into the owning feature's generated-entity table. Schema
/// identifiers remain unchanged; only the omitted canonical owner is filled.
pub fn bind_trimmed_definition_owners(
    definitions: &mut [FeatureDefinition],
    entity_tables: &[FeatureEntityTable],
) {
    let claimed_owner_ids = definitions
        .iter()
        .filter_map(|definition| definition.owner_feature_id)
        .collect::<BTreeSet<_>>();
    let candidates = definitions
        .iter()
        .map(|definition| {
            let external_ids = definition
                .trim_entities
                .as_ref()
                .map(|table| {
                    table
                        .rows
                        .iter()
                        .map(|row| row.external_id)
                        .collect::<BTreeSet<_>>()
                })
                .unwrap_or_default();
            if definition.owner_feature_id.is_some() || external_ids.is_empty() {
                return BTreeSet::new();
            }
            entity_tables
                .iter()
                .filter_map(|table| {
                    let owner = table.feature_id?;
                    if claimed_owner_ids.contains(&owner) {
                        return None;
                    }
                    let source_ids = table
                        .entries
                        .iter()
                        .filter_map(|entry| entry.source_entity_id)
                        .collect::<BTreeSet<_>>();
                    (source_ids == external_ids).then_some(owner)
                })
                .collect::<BTreeSet<_>>()
        })
        .collect::<Vec<_>>();
    let mut owner_candidate_counts = BTreeMap::new();
    for owner in candidates.iter().flat_map(|owners| owners.iter()) {
        *owner_candidate_counts.entry(*owner).or_insert(0usize) += 1;
    }
    for (definition, owners) in definitions.iter_mut().zip(candidates) {
        let Some(owner) = owners
            .first()
            .copied()
            .filter(|_| owners.len() == 1)
            .filter(|owner| owner_candidate_counts.get(owner) == Some(&1))
        else {
            continue;
        };
        definition.owner_feature_id = Some(owner);
    }
}

/// Bind unlabeled positional definitions through exact section-entity IDs in
/// the owning generated-entity table. Empty and non-unique joins remain
/// unbound.
pub fn bind_replay_definition_owners(
    definitions: &mut [FeatureDefinition],
    entity_tables: &[FeatureEntityTable],
    claimed_owner_ids: &BTreeSet<u32>,
) {
    let candidates = definitions
        .iter()
        .map(|definition| {
            let external_ids = definition
                .order_table
                .as_ref()
                .map(|table| {
                    table
                        .rows
                        .iter()
                        .map(|row| row.external_id)
                        .collect::<BTreeSet<_>>()
                })
                .unwrap_or_default();
            if definition.owner_feature_id.is_some() || external_ids.is_empty() {
                return BTreeSet::new();
            }
            entity_tables
                .iter()
                .filter_map(|table| {
                    let owner = table.feature_id?;
                    if claimed_owner_ids.contains(&owner) {
                        return None;
                    }
                    let source_ids = table
                        .entries
                        .iter()
                        .filter_map(|entry| entry.source_entity_id)
                        .collect::<BTreeSet<_>>();
                    (!source_ids.is_empty() && source_ids.is_subset(&external_ids)).then_some(owner)
                })
                .collect::<BTreeSet<_>>()
        })
        .collect::<Vec<_>>();
    let mut owner_candidate_counts = BTreeMap::new();
    for owner in candidates.iter().flat_map(|owners| owners.iter()) {
        *owner_candidate_counts.entry(*owner).or_insert(0usize) += 1;
    }
    for (definition, owners) in definitions.iter_mut().zip(candidates) {
        let Some(owner) = owners
            .first()
            .copied()
            .filter(|_| owners.len() == 1)
            .filter(|owner| owner_candidate_counts.get(owner) == Some(&1))
        else {
            continue;
        };
        definition.id = owner;
        definition.owner_feature_id = Some(owner);
    }
}

/// Bind a DEPDB section through the consecutive recipe, internal datum, and
/// sketch-plane identifier chain. Repeated definitions for one plane remain
/// unowned because the current regeneration snapshot is not established.
pub fn bind_depdb_section_owners(
    definitions: &mut [FeatureDefinition],
    operations: &[FeatureOperation],
    depdb_ranges: &[(usize, usize)],
) {
    let in_depdb = |offset: usize| {
        depdb_ranges
            .iter()
            .any(|(start, end)| offset >= *start && offset < *end)
    };
    let claimed_owner_ids = definitions
        .iter()
        .filter_map(|definition| definition.owner_feature_id)
        .collect::<BTreeSet<_>>();
    let mut definitions_per_plane = BTreeMap::new();
    for plane_id in definitions.iter().filter_map(|definition| {
        (definition.owner_feature_id.is_none() && in_depdb(definition.offset))
            .then_some(definition.section_3d.as_ref()?.sketch_plane_entity_id?)
    }) {
        *definitions_per_plane.entry(plane_id).or_insert(0usize) += 1;
    }
    let mut ordered_operations = operations.iter().collect::<Vec<_>>();
    ordered_operations.sort_by_key(|operation| operation.offset);
    for definition in definitions
        .iter_mut()
        .filter(|definition| definition.owner_feature_id.is_none())
    {
        let Some(plane_id) = definition
            .section_3d
            .as_ref()
            .and_then(|section| section.sketch_plane_entity_id)
            .filter(|plane_id| *plane_id >= 2)
        else {
            continue;
        };
        if definitions_per_plane.get(&plane_id) != Some(&1) {
            continue;
        }
        let owner_id = plane_id - 2;
        let datum_id = plane_id - 1;
        if claimed_owner_ids.contains(&owner_id) {
            continue;
        }
        let matches = ordered_operations
            .windows(2)
            .filter(|pair| {
                pair[0].feature_id == owner_id
                    && pair[0].recipe.is_some()
                    && pair[1].feature_id == datum_id
                    && pair[1].recipe.is_none()
            })
            .count();
        if matches == 1 {
            if definition.id == 0 {
                definition.id = owner_id;
            }
            definition.owner_feature_id = Some(owner_id);
        }
    }
}

/// Decode the implicit named-record entity table and every canonical `f7`
/// reference, preserving both source context and unresolved target IDs.
pub fn entity_graph(payload: &[u8]) -> (Vec<FeatureEntity>, Vec<FeatureEntityReference>) {
    let tokens = psb::tokens(payload);
    let Some(root) = tokens.first() else {
        return (Vec::new(), Vec::new());
    };
    let root_name = payload.get(2..root.length.saturating_sub(1));
    if root.offset != 0
        || root.kind != psb::TokenKind::NamedRecord
        || payload.get(1) != Some(&0)
        || root_name != Some(b"Sld_Features".as_slice())
    {
        return (Vec::new(), Vec::new());
    }
    let mut entities = Vec::new();
    for token in &tokens {
        if token.kind != psb::TokenKind::NamedRecord || token.length < 3 {
            continue;
        }
        let name_start = token.offset + 2;
        let name_end = token.offset + token.length - 1;
        entities.push(FeatureEntity {
            entity_id: entities.len() as u32,
            type_byte: payload[token.offset + 1],
            name: String::from_utf8_lossy(&payload[name_start..name_end]).into_owned(),
            offset: token.offset,
        });
    }
    let entity_count = entities.len() as u32;
    let entity_by_offset = entities
        .iter()
        .map(|entity| (entity.offset, entity.entity_id))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut source = None;
    let mut references = Vec::new();
    for token in tokens {
        if token.kind == psb::TokenKind::NamedRecord {
            source = entity_by_offset.get(&token.offset).copied();
        } else if token.kind == psb::TokenKind::EntityReference {
            let Ok((target_entity_id, _)) = psb::reference_id(payload, token.offset + 1) else {
                continue;
            };
            references.push(FeatureEntityReference {
                source_entity_id: source,
                target_entity_id,
                target_resolved: target_entity_id < entity_count,
                offset: token.offset,
            });
        }
    }
    (entities, references)
}

fn read_entries(
    payload: &[u8],
    body_start: usize,
    count: u32,
) -> Option<Vec<FeatureEntityTableEntry>> {
    let count = usize::try_from(count).ok()?;
    let remaining = payload.len().checked_sub(body_start)?;
    (count <= remaining / 2).then_some(())?;
    let mut entries = Vec::with_capacity(count);
    let mut cursor = body_start;
    for index in 0..count {
        let prefixed_class = (payload.get(cursor) == Some(&psb::token::ENTITY_REF))
            .then(|| psb::reference_id(payload, cursor + 1).ok())
            .flatten();
        let prefixed = prefixed_class.is_some();
        if let Some((_, after_class)) = prefixed_class {
            cursor = after_class;
        }
        let offset = cursor;
        let (id, after) = psb::reference_id(payload, cursor).ok()?;
        let (class_id, after_class) = psb::reference_id(payload, after).ok().or_else(|| {
            (index == 0)
                .then_some(prefixed_class)
                .flatten()
                .map(|(class_id, _)| (class_id, after))
        })?;
        let (source_entity_id, body_start) = if class_id == 200 {
            match psb::reference_id(payload, after_class) {
                Ok((order, after_order)) => (Some(order), after_order),
                Err(_) => (None, after_class),
            }
        } else {
            (None, after_class)
        };
        let terminal_table_separator = (index + 1 == count
            && class_id == 200
            && matches!(payload.get(body_start), Some(0x00 | 0x01))
            && payload.get(body_start + 1..body_start + 3)
                == Some(&[0xf2, psb::token::ENTITY_REF]))
        .then_some(body_start + 1);
        let end_offset = if let Some(end_offset) = terminal_table_separator {
            end_offset
        } else {
            body_start
                + payload
                    .get(body_start..)?
                    .iter()
                    .position(|&byte| byte == 0xe3)?
                + 1
        };
        entries.push(FeatureEntityTableEntry {
            entity_id: id,
            class_id,
            source_entity_id,
            prefixed,
            offset,
            end_offset,
        });
        cursor = end_offset;
    }
    Some(entries)
}

/// Decode valid `AllFeatur` mixed generated-entity tables.
///
/// `feature_ids` must come from byte-decoded geometry ownership; no owner is
/// inferred from a table's neighbouring bytes or entity contents.
pub fn entity_tables(
    payload: &[u8],
    feature_ids: &BTreeSet<u32>,
    surface_ids: &BTreeSet<u32>,
) -> Vec<FeatureEntityTable> {
    let spans = row_spans(payload, feature_ids);
    let mut tables = Vec::new();
    for offset in 0..payload.len() {
        if payload[offset] != psb::token::ARRAY_OPEN {
            continue;
        }
        let (count, after_count) = psb::compact_int(payload, offset + 1);
        if count == 0 || payload.get(after_count) != Some(&psb::token::ENTITY_REF) {
            continue;
        }
        let Ok((table_class_id, after_table_class)) = psb::reference_id(payload, after_count + 1)
        else {
            continue;
        };
        if payload.get(after_table_class..after_table_class + 2) != Some(&[0xfb, 0xe3]) {
            continue;
        }
        let Some(&(_, row_end, feature_id)) = spans
            .iter()
            .find(|&&(start, end, _)| start <= offset && offset < end)
        else {
            continue;
        };
        let Some(entries) = read_entries(&payload[..row_end], after_table_class + 2, count) else {
            continue;
        };
        let entry_ids = entries
            .iter()
            .map(|entry| entry.entity_id)
            .collect::<Vec<_>>();
        let surface_ids = entry_ids
            .iter()
            .copied()
            .filter(|id| surface_ids.contains(id))
            .collect::<Vec<_>>();
        let non_surface_entity_ids = entry_ids
            .iter()
            .copied()
            .filter(|id| !surface_ids.contains(id))
            .collect();
        tables.push(FeatureEntityTable {
            feature_id: Some(feature_id),
            table_class_id,
            entry_ids,
            entries,
            surface_ids,
            non_surface_entity_ids,
            offset,
        });
    }
    tables
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn final_generated_entry_may_terminate_at_the_table_separator() {
        let payload = [10, 0x80, 200, 4, 0, 0xe3, 11, 0x80, 200, 7, 1, 0xf2, 0xf7];
        let entries = read_entries(&payload, 0, 2).expect("complete generated table");

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].source_entity_id, Some(4));
        assert_eq!(entries[0].end_offset, 6);
        assert_eq!(entries[1].source_entity_id, Some(7));
        assert_eq!(entries[1].end_offset, 11);
    }

    #[test]
    fn generated_table_prototype_uses_its_prefixed_entry_class() {
        let payload = [0xf7, 30, 20, 0xe4, 0xe3, 11, 0x80, 200, 7, 1, 0xe3];
        let entries = read_entries(&payload, 0, 2).expect("prototype and positional entry");

        assert_eq!(entries[0].entity_id, 20);
        assert_eq!(entries[0].class_id, 30);
        assert!(entries[0].prefixed);
        assert_eq!(entries[0].end_offset, 5);
        assert_eq!(entries[1].class_id, 200);
        assert_eq!(entries[1].source_entity_id, Some(7));

        let misplaced = [10, 30, 0, 0xe3, 0xf7, 31, 20, 0xe4, 0xe3];
        assert!(read_entries(&misplaced, 0, 2).is_none());
    }

    #[test]
    fn choice_fields_ignore_overlapping_headers() {
        let choices = [FeatureChoice {
            feature_id: 7,
            label: "choice".into(),
            type_byte: None,
            payload: vec![psb::token::NAMED_RECORD, psb::token::NAMED_RECORD, b'a', 0],
            payload_offset: 100,
            offset: 90,
        }];

        let fields = choice_fields(&choices);
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].offset, 101);
        assert_eq!(fields[0].name, "");
    }

    #[test]
    fn positional_datum_table_replays_the_named_stream_schema() {
        let row = |feature_id, stream_offset, body: Vec<u8>| FeatureRow {
            feature_id,
            header: [0xeb, 0x04],
            root_schema_class: Some(917),
            stream_offset,
            body,
            body_offset: feature_id as usize * 100,
            offset: feature_id as usize * 100 - 2,
        };
        let rows = [
            row(
                1,
                10,
                b"\xe0\x00dtm_id_tab\0\xf2\xf8\x01\xf7\x57\xfb\xe2\
                  \xe0\x01dtm_id\0\x2a\xe0\x01dim_id\0\xf6"
                    .to_vec(),
            ),
            row(
                2,
                10,
                vec![
                    0x00, 0xf8, 0x02, 0xf7, 0x57, 0xfb, 0xe2, 0xf7, 0x58, 0x80, 0x91, 0xf6, 0xf1,
                    0xf7, 0x57, 0xe2, 0x80, 0x92, 0xf6, 0xe3,
                ],
            ),
            row(
                3,
                11,
                vec![
                    0xf8, 0x01, 0xf7, 0x57, 0xfb, 0xe2, 0xf7, 0x58, 0x2b, 0xf6, 0xe3,
                ],
            ),
        ];

        let decoded = geometry_tables(&rows);

        assert_eq!(decoded.len(), 2);
        assert_eq!(decoded[0].feature_id, 1);
        assert_eq!(decoded[0].entry_ids, Some(vec![42]));
        assert_eq!(decoded[1].feature_id, 2);
        assert_eq!(decoded[1].count, 2);
        assert_eq!(decoded[1].entity_class, 87);
        assert_eq!(decoded[1].entry_ids, Some(vec![145, 146]));
        assert_eq!(decoded[1].offset, 201);
    }

    #[test]
    fn loop_history_roster_uses_declared_loop_count_and_stored_order() {
        let mut body = b"\xe0\x00lo_id_tab_ptr\0\xf8\x03\xf7\x60\xfb\xe3\
                         \xe0\x01lo_hist\0\xf8\x06"
            .to_vec();
        let first_offset = body.len();
        body.extend_from_slice(&[42, 1, 0xf6, 0xe5, 2, 0xf1, 0xf7, 96, 0xe3]);
        let second_offset = body.len();
        body.extend_from_slice(&[43, 3, 0xf6, 0xe5, 4, 0xe3]);
        let third_offset = body.len();
        body.extend_from_slice(&[44, 5, 6, 0xe4, 0xf6, 7]);
        let named_boundary_offset = body.len();
        body.extend_from_slice(b"\xe0\x00next\0");
        let rows = [FeatureRow {
            feature_id: 7,
            header: [0xeb, 0x04],
            root_schema_class: Some(917),
            stream_offset: 10,
            body,
            body_offset: 1_000,
            offset: 998,
        }];

        let tables = geometry_tables(&rows);
        let entries = loop_history_entries(&rows, &tables);

        assert_eq!(entries.len(), 3);
        assert_eq!(
            entries
                .iter()
                .map(|entry| (
                    entry.feature_id,
                    entry.ordinal,
                    entry.loop_id,
                    entry.offset,
                    entry.end_offset,
                ))
                .collect::<Vec<_>>(),
            vec![
                (7, 0, 42, 1_000 + first_offset, 1_000 + second_offset),
                (7, 1, 43, 1_000 + second_offset, 1_000 + third_offset),
                (
                    7,
                    2,
                    44,
                    1_000 + third_offset,
                    1_000 + named_boundary_offset
                ),
            ]
        );
        assert_eq!(
            entries[0].field_bytes,
            vec![vec![1], vec![0xf6], vec![0xe5], vec![2]]
        );
        assert_eq!(
            entries[0].boundary,
            FeatureLoopHistoryBoundary::ReferenceContinue(96)
        );
        assert_eq!(
            entries[1].boundary,
            FeatureLoopHistoryBoundary::CompoundClose
        );
        assert_eq!(entries[2].field_bytes.len(), 5);
        assert_eq!(entries[2].boundary, FeatureLoopHistoryBoundary::NamedRecord);
    }

    #[test]
    fn loop_history_roster_rejects_incomplete_and_early_boundaries() {
        assert!(loop_history_roster(&[1, 2, 0xe3], 0, 1).is_none());
        assert!(loop_history_roster(&[1, 2, 3, 4, 5, 0xe3], 0, 2).is_none());
        let direct_named = loop_history_roster(b"\x01\x02\xf6\xe5\x03\xe0\x00next\0", 0, 1)
            .expect("direct named boundary");
        assert_eq!(direct_named.len(), 1);
        assert_eq!(direct_named[0].loop_id, 1);
        assert_eq!(direct_named[0].offset, 0);
        assert_eq!(direct_named[0].end_offset, 5);
        assert_eq!(
            direct_named[0].boundary,
            FeatureLoopHistoryBoundary::NamedRecord
        );

        let body = b"\xe0\x00lo_id_tab_ptr\0\xf8\x01\xf7\x60\xfb\xe3\
                     \xe0\x01lo_hist\0\xf8\x05\x2a\xe3"
            .to_vec();
        let rows = [FeatureRow {
            feature_id: 7,
            header: [0xeb, 0x04],
            root_schema_class: Some(917),
            stream_offset: 10,
            body,
            body_offset: 1_000,
            offset: 998,
        }];
        assert!(loop_history_entries(&rows, &geometry_tables(&rows)).is_empty());
    }

    #[test]
    fn entity_graph_requires_the_solid_features_root() {
        let packed_lookalike = b"\xe0\x00SlV\xff\0\xf7\x01";
        assert_eq!(entity_graph(packed_lookalike), (Vec::new(), Vec::new()));

        let payload = b"\xe0\x00Sld_Features\0\xe0\x00first_feat_ptr\0\xf7\x00";
        let (entities, references) = entity_graph(payload);
        assert_eq!(entities.len(), 2);
        assert_eq!(entities[0].name, "Sld_Features");
        assert_eq!(references.len(), 1);
        assert_eq!(references[0].source_entity_id, Some(1));
        assert!(references[0].target_resolved);
    }

    #[test]
    fn generated_entity_entries_accept_variable_schema_classes() {
        let payload = [
            0xf7, 0x50, 0x0d, 0x80, 0xcc, 0x00, 0xe4, 0xf1, 0xf7, 0x4f, 0xe3, 0x12, 0x80, 0xcb,
            0x00, 0xe4, 0xe3,
        ];

        let entries = read_entries(&payload, 0, 2).expect("generated entity entries");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].entity_id, 13);
        assert_eq!(entries[0].class_id, 204);
        assert!(entries[0].prefixed);
        assert_eq!(entries[1].entity_id, 18);
        assert_eq!(entries[1].class_id, 203);
        assert!(!entries[1].prefixed);
    }

    fn replay_row(feature_id: u32, operands: &[u8]) -> FeatureRow {
        let mut body = vec![0xf1, 0xf7, 0x42, 0xd8, 0x80, 0x01, 0xe3];
        body.extend_from_slice(operands);
        body.extend_from_slice(&[0xf5, 0x96, 0x92]);
        FeatureRow {
            feature_id,
            header: [0xeb, 0x04],
            root_schema_class: Some(913),
            stream_offset: 100,
            body,
            body_offset: 200,
            offset: 190,
        }
    }

    fn unanchored_replay_row(
        feature_id: u32,
        row_id: u8,
        suffix_reference: Option<u8>,
        operands: &[u8],
    ) -> FeatureRow {
        let mut row = replay_row(feature_id, operands);
        row.body.clear();
        row.body.push(psb::token::COMPOUND_CLOSE);
        row.body.extend_from_slice(operands);
        row.body
            .extend_from_slice(&[0xe1, 0xe1, row_id, psb::token::COMPOUND_CLOSE]);
        if let Some(reference) = suffix_reference {
            row.body.extend_from_slice(&[
                psb::token::ENTITY_REF,
                reference,
                psb::token::COMPOUND_CLOSE,
            ]);
        } else {
            row.body.push(psb::token::COMPOUND_CLOSE);
        }
        row.body
            .extend_from_slice(&[3, row_id, 0x00, 0xe1, 0x00, psb::token::COMPOUND_CLOSE]);
        row
    }

    #[test]
    fn positional_round_replay_inherits_each_array_extent() {
        let mut rows = [
            replay_row(1, &[0xf8, 2, 10, 11, 0xf7, 42, 0xf8, 3, 20, 21, 22]),
            replay_row(2, &[12, 13, 23, 24, 25]),
            replay_row(3, &[0xf8, 1, 14, 26, 27, 28]),
        ];
        rows[0].body[3] = 0xc8;

        let decoded = replay_affected_ids(&rows);

        assert_eq!(decoded.len(), 3);
        assert_eq!(decoded[0].geometry_ids, vec![10, 11]);
        assert_eq!(decoded[0].edge_ids, vec![20, 21, 22]);
        assert_eq!(decoded[1].geometry_ids, vec![12, 13]);
        assert_eq!(decoded[1].edge_ids, vec![23, 24, 25]);
        assert_eq!(decoded[1].geometry_extent, ReplayExtentSource::Inherited);
        assert_eq!(decoded[1].edge_extent, ReplayExtentSource::Inherited);
        assert_eq!(decoded[2].geometry_ids, vec![14]);
        assert_eq!(decoded[2].edge_ids, vec![26, 27, 28]);
        assert_eq!(decoded[2].geometry_extent, ReplayExtentSource::Explicit);
        assert_eq!(decoded[2].edge_extent, ReplayExtentSource::Inherited);
    }

    #[test]
    fn positional_round_replay_uses_repeated_row_id_suffix() {
        let rows = [
            unanchored_replay_row(1, 40, None, &[0xf8, 2, 10, 11, 0xf8, 2, 20, 21]),
            unanchored_replay_row(2, 41, None, &[12, 13, 22, 23]),
        ];

        let decoded = replay_affected_ids(&rows);

        assert_eq!(decoded.len(), 2);
        assert_eq!(decoded[0].geometry_ids, vec![10, 11]);
        assert_eq!(decoded[0].edge_ids, vec![20, 21]);
        assert_eq!(decoded[1].geometry_ids, vec![12, 13]);
        assert_eq!(decoded[1].edge_ids, vec![22, 23]);
        assert_eq!(decoded[1].geometry_extent, ReplayExtentSource::Inherited);
        assert_eq!(decoded[1].edge_extent, ReplayExtentSource::Inherited);
    }

    #[test]
    fn positional_chamfer_replay_uses_referenced_row_id_suffix() {
        let mut row = unanchored_replay_row(1, 40, Some(74), &[0xf8, 2, 10, 11, 0xf8, 2, 20, 21]);
        row.root_schema_class = Some(914);

        let decoded = replay_affected_ids(&[row]);

        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].geometry_ids, vec![10, 11]);
        assert_eq!(decoded[0].edge_ids, vec![20, 21]);
    }

    #[test]
    fn radius_dimension_type_uses_model_length_units() {
        assert_eq!(dimension_unit(0x03), DimensionUnit::Millimeters);
    }

    #[test]
    fn binds_missing_definition_owner_from_unique_generated_datum_table() {
        let mut definitions = [FeatureDefinition {
            id: 917,
            owner_feature_id: None,
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: Some(FeatureSection3d {
                sketch_plane_entity_id: Some(12),
                sketch_plane_flip: None,
                reference_plane_entity_ids: Vec::new(),
                reference_plane_datum_geometry_id: None,
                orientation: FeatureSectionOrientation::default(),
                dimension_ids: Vec::new(),
                offset: 1,
            }),
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 0,
        }];
        bind_definition_owners(
            &mut definitions,
            &[FeatureGeometryTable {
                feature_id: 10,
                kind: FeatureGeometryTableKind::DatumIds,
                count: 1,
                entity_class: 87,
                entry_ids: Some(vec![12]),
                offset: 2,
            }],
        );

        assert_eq!(definitions[0].owner_feature_id, Some(10));
    }

    fn pending_replay(external_ids: &[u32]) -> FeatureDefinition {
        FeatureDefinition {
            id: 0,
            owner_feature_id: None,
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: Some(FeatureOrderTable {
                declared_count: external_ids.len() as u32,
                has_prototype: false,
                entity_ref: Some(1),
                rows: external_ids
                    .iter()
                    .enumerate()
                    .map(|(index, external_id)| FeatureOrderRow {
                        external_id: *external_id,
                        internal_id: index as u32 + 1,
                        bitmask: 0,
                        offset: index,
                    })
                    .collect(),
                offset: 0,
            }),
            section_3d: None,
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 0,
        }
    }

    fn pending_trimmed_definition(external_ids: &[u32]) -> FeatureDefinition {
        let mut definition = pending_replay(&[]);
        definition.id = 917;
        definition.trim_entities = Some(FeatureTrimEntityTable {
            declared_count: Some(external_ids.len() as u32),
            entity_ref: Some(1),
            entry_ref: None,
            buckets: Vec::new(),
            rows: external_ids
                .iter()
                .enumerate()
                .map(|(index, external_id)| FeatureTrimEntity {
                    external_id: *external_id,
                    kind: TrimEntityKind::Line,
                    mode: Some(0),
                    vertices: [index as u32, index as u32 + 1],
                    center_vertex: None,
                    offset: index,
                })
                .collect(),
            solved_external_ids: external_ids.to_vec(),
            offset: 0,
        });
        definition
    }

    fn generated_entity_table(owner: u32, source_ids: &[u32]) -> FeatureEntityTable {
        FeatureEntityTable {
            feature_id: Some(owner),
            table_class_id: 80,
            entry_ids: Vec::new(),
            entries: source_ids
                .iter()
                .enumerate()
                .map(|(index, source_id)| FeatureEntityTableEntry {
                    entity_id: index as u32 + 1,
                    class_id: 200,
                    source_entity_id: Some(*source_id),
                    prefixed: true,
                    offset: index,
                    end_offset: index + 1,
                })
                .collect(),
            surface_ids: Vec::new(),
            non_surface_entity_ids: Vec::new(),
            offset: 0,
        }
    }

    #[test]
    fn binds_replay_owner_from_unique_source_entity_subset() {
        let mut definitions = [pending_replay(&[10, 11, 12])];
        bind_replay_definition_owners(
            &mut definitions,
            &[generated_entity_table(42, &[10, 12])],
            &BTreeSet::new(),
        );

        assert_eq!(definitions[0].id, 42);
        assert_eq!(definitions[0].owner_feature_id, Some(42));
    }

    #[test]
    fn binds_saved_section_owner_from_exact_trimmed_entity_set() {
        let mut definitions = [pending_trimmed_definition(&[9, 10, 11, 14, 21])];
        bind_trimmed_definition_owners(
            &mut definitions,
            &[generated_entity_table(667, &[14, 21, 11, 10, 9])],
        );

        assert_eq!(definitions[0].id, 917);
        assert_eq!(definitions[0].owner_feature_id, Some(667));
    }

    #[test]
    fn withholds_saved_section_owner_for_partial_or_reused_entity_sets() {
        let mut partial = [pending_trimmed_definition(&[9, 10, 11])];
        bind_trimmed_definition_owners(&mut partial, &[generated_entity_table(667, &[9, 10])]);
        assert_eq!(partial[0].owner_feature_id, None);

        let mut reused = [
            pending_trimmed_definition(&[9, 10]),
            pending_trimmed_definition(&[9, 10]),
        ];
        bind_trimmed_definition_owners(&mut reused, &[generated_entity_table(667, &[9, 10])]);
        assert!(reused
            .iter()
            .all(|definition| definition.owner_feature_id.is_none()));
    }

    #[test]
    fn withholds_replay_owner_for_empty_or_ambiguous_source_joins() {
        let mut empty = [pending_replay(&[10])];
        bind_replay_definition_owners(
            &mut empty,
            &[generated_entity_table(42, &[])],
            &BTreeSet::new(),
        );
        assert_eq!(empty[0].owner_feature_id, None);

        let mut ambiguous = [pending_replay(&[10, 11])];
        bind_replay_definition_owners(
            &mut ambiguous,
            &[
                generated_entity_table(42, &[10]),
                generated_entity_table(43, &[11]),
            ],
            &BTreeSet::new(),
        );
        assert_eq!(ambiguous[0].owner_feature_id, None);

        let mut repeated_owner = [pending_replay(&[10]), pending_replay(&[10, 11])];
        bind_replay_definition_owners(
            &mut repeated_owner,
            &[generated_entity_table(42, &[10])],
            &BTreeSet::new(),
        );
        assert!(repeated_owner
            .iter()
            .all(|definition| definition.owner_feature_id.is_none()));

        let mut claimed = [pending_replay(&[10])];
        bind_replay_definition_owners(
            &mut claimed,
            &[generated_entity_table(42, &[10])],
            &BTreeSet::from([42]),
        );
        assert_eq!(claimed[0].owner_feature_id, None);
    }

    fn operation(
        feature_id: u32,
        recipe: Option<FeatureRecipe>,
        offset: usize,
    ) -> FeatureOperation {
        FeatureOperation {
            feature_id,
            kind: String::new(),
            display_name_stored: false,
            stored_name: None,
            stored_name_bytes: None,
            identifier_keyword: None,
            stored_name_prefix: None,
            recipe,
            root_schema_class: None,
            parent_feature_id: None,
            offset,
            state_offset: offset,
        }
    }

    #[test]
    fn binds_unique_depdb_section_from_recipe_datum_plane_chain() {
        let mut definition = pending_replay(&[]);
        definition.section_3d = Some(FeatureSection3d {
            sketch_plane_entity_id: Some(249),
            sketch_plane_flip: None,
            reference_plane_entity_ids: Vec::new(),
            reference_plane_datum_geometry_id: None,
            orientation: FeatureSectionOrientation::default(),
            dimension_ids: Vec::new(),
            offset: 0,
        });
        let operations = [
            operation(247, Some(FeatureRecipe::ProtrudeRevolve), 10),
            operation(248, None, 20),
        ];

        bind_depdb_section_owners(
            std::slice::from_mut(&mut definition),
            &operations,
            &[(0, usize::MAX)],
        );

        assert_eq!(definition.id, 247);
        assert_eq!(definition.owner_feature_id, Some(247));
    }

    #[test]
    fn depdb_owner_binding_preserves_stored_definition_identifier() {
        let mut definition = pending_replay(&[]);
        definition.id = 2;
        definition.section_3d = Some(FeatureSection3d {
            sketch_plane_entity_id: Some(249),
            sketch_plane_flip: None,
            reference_plane_entity_ids: Vec::new(),
            reference_plane_datum_geometry_id: None,
            orientation: FeatureSectionOrientation::default(),
            dimension_ids: Vec::new(),
            offset: 0,
        });
        let operations = [
            operation(247, Some(FeatureRecipe::ProtrudeRevolve), 10),
            operation(248, None, 20),
        ];

        bind_depdb_section_owners(
            std::slice::from_mut(&mut definition),
            &operations,
            &[(0, usize::MAX)],
        );

        assert_eq!(definition.id, 2);
        assert_eq!(definition.owner_feature_id, Some(247));
    }

    #[test]
    fn decodes_owned_depdb_full_turn_for_rotational_recipe() {
        let mut definition = pending_replay(&[]);
        definition.id = 247;
        definition.owner_feature_id = Some(247);
        definition.offset = 100;
        definition.body = vec![
            0x83, 0xdf, 0xf6, 0xe3, 0x00, 0x00, 0xea, 0x44, 0x00, 0x00, 0xf6, 0xf6, 0xf6, 0x00,
            0x00, 0x00, 0x00,
        ];
        let revolve = operation(247, Some(FeatureRecipe::ProtrudeRevolve), 10);

        let decoded = definition_revolution_extents(&[definition.clone()], &[revolve]);

        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].feature_id, 247);
        assert_eq!(decoded[0].kind, FeatureRevolutionExtentKind::FullTurn);
        assert_eq!(decoded[0].offset, 106);

        let extrude = operation(247, Some(FeatureRecipe::ProtrudeExtrude), 10);
        assert!(definition_revolution_extents(&[definition], &[extrude]).is_empty());
    }

    #[test]
    fn preserves_repeated_identical_depdb_full_turn_states() {
        let sequence = [
            0x83, 0xdf, 0xf6, 0xe3, 0x00, 0x00, 0xea, 0x44, 0x00, 0x00, 0xf6, 0xf6, 0xf6, 0x00,
            0x00, 0x00, 0x00,
        ];
        let mut definition = pending_replay(&[]);
        definition.id = 247;
        definition.owner_feature_id = Some(247);
        definition.offset = 100;
        definition.body.extend(sequence);
        definition.body.extend([0xe7, 0x04, 0x00, 0xe1]);
        definition.body.extend(sequence);
        let revolve = operation(247, Some(FeatureRecipe::ProtrudeRevolve), 10);

        let decoded = definition_revolution_extents(&[definition], &[revolve]);

        assert_eq!(decoded.len(), 2);
        assert_eq!(decoded[0].offset, 106);
        assert_eq!(decoded[1].offset, 127);
        assert!(decoded
            .iter()
            .all(|extent| extent.kind == FeatureRevolutionExtentKind::FullTurn));
    }

    #[test]
    fn withholds_depdb_owner_for_repeated_plane_or_nonconsecutive_datum() {
        let section = FeatureSection3d {
            sketch_plane_entity_id: Some(249),
            sketch_plane_flip: None,
            reference_plane_entity_ids: Vec::new(),
            reference_plane_datum_geometry_id: None,
            orientation: FeatureSectionOrientation::default(),
            dimension_ids: Vec::new(),
            offset: 0,
        };
        let mut repeated = [pending_replay(&[]), pending_replay(&[])];
        for definition in &mut repeated {
            definition.section_3d = Some(section.clone());
        }
        let consecutive = [
            operation(247, Some(FeatureRecipe::ProtrudeRevolve), 10),
            operation(248, None, 20),
        ];
        bind_depdb_section_owners(&mut repeated, &consecutive, &[(0, usize::MAX)]);
        assert!(repeated
            .iter()
            .all(|definition| definition.owner_feature_id.is_none()));

        let mut separated = pending_replay(&[]);
        separated.section_3d = Some(section);
        let operations = [
            operation(247, Some(FeatureRecipe::ProtrudeRevolve), 10),
            operation(900, None, 15),
            operation(248, None, 20),
        ];
        bind_depdb_section_owners(
            std::slice::from_mut(&mut separated),
            &operations,
            &[(0, usize::MAX)],
        );
        assert_eq!(separated.owner_feature_id, None);

        let mut claimed = pending_replay(&[]);
        claimed.id = 247;
        claimed.owner_feature_id = Some(247);
        let mut candidate = pending_replay(&[]);
        candidate.section_3d = Some(FeatureSection3d {
            sketch_plane_entity_id: Some(249),
            sketch_plane_flip: None,
            reference_plane_entity_ids: Vec::new(),
            reference_plane_datum_geometry_id: None,
            orientation: FeatureSectionOrientation::default(),
            dimension_ids: Vec::new(),
            offset: 0,
        });
        let mut definitions = [claimed, candidate];
        bind_depdb_section_owners(&mut definitions, &consecutive, &[(0, usize::MAX)]);
        assert_eq!(definitions[1].owner_feature_id, None);
    }

    #[test]
    fn positional_replays_exclude_the_contextually_owned_instance() {
        let payload = b"feat_defs_917\0template\0\xe0\x01feat_id\0\x2a\
            \xe0\x00ref_model_info\0\xe3S2D0004\0owned\
            \xe3S2D0004\0pending";

        let decoded = positional_replay_definitions(payload);

        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].id, 917);
        assert_eq!(decoded[0].owner_feature_id, None);
        assert!(decoded[0].body.starts_with(b"\xe3S2D0004\0"));
        assert!(decoded[0].body.ends_with(b"pending"));
    }

    #[test]
    fn unlabeled_replay_boundary_ends_the_preceding_definition() {
        let payload = b"feat_defs_917\0template\xe3S2D0004\0replay";

        let decoded = definitions(payload);

        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].body, b"feat_defs_917\0template");
    }

    #[test]
    fn positional_saved_section_starts_an_owned_definition() {
        let payload = b"feat_defs_917\0\xe0\x01feat_id\0\x28\xe0\x00gsec2d_ptr\0\
            template\0\xe0\x01feat_id\0\x2a\xe0\x00ref_model_info\0\
            \xe0\x00name\0S2D0004\0saved";

        let decoded = definitions(payload);

        assert_eq!(decoded.len(), 2);
        assert_eq!(decoded[0].id, 917);
        assert_eq!(decoded[0].owner_feature_id, Some(40));
        assert_eq!(decoded[0].body.last(), Some(&0));
        assert_eq!(decoded[1].id, 42);
        assert_eq!(decoded[1].owner_feature_id, Some(42));
        assert!(decoded[1].body.starts_with(b"\xe0\x01feat_id\0"));
        assert!(decoded[1].body.ends_with(b"saved"));
    }

    #[test]
    fn positional_saved_section_replays_its_segment_table() {
        let mut payload = b"feat_defs_917\0template\0\xe0\x01feat_id\0\x2a\
            \xe0\x00ref_model_info\0\xe3S2D0004\0\xf8\x02\xf7\x01\xfb\xe2\
            \xf2\xf7\x01\xe2"
            .to_vec();
        payload.extend_from_slice(&[2, 0, 0, 0, 7, 8, 0xf6, 0, 0, 0xf6, 0xf6, 42, 0xe2, 0xe3]);
        payload.extend_from_slice(&[3, 0, 0, 0, 8, 9, 10, 1, 0, 11, 12, 43, 0xe2]);

        let decoded = definitions(&payload);
        let segments = decoded[1].segments.as_ref().expect("positional segtab");

        assert_eq!(segments.declared_count, 2);
        assert_eq!(segments.entity_ref, Some(1));
        assert_eq!(segments.rows.len(), 2);
        assert!(segments.is_complete());
        assert_eq!(segments.rows[0].point_ids, [7, 8]);
        assert_eq!(segments.rows[1].kind, FeatureSegmentKind::Arc);
        assert_eq!(segments.rows[1].center_id, Some(10));
        assert_eq!(segments.rows[1].external_id, 43);
    }

    #[test]
    fn positional_segment_table_stops_at_the_next_s2d_record() {
        let mut payload = b"\xe3S2D0004\0\xf8\x03\xf7\x01\xfb\xe2\xf2\xf7\x01\xe2".to_vec();
        payload.extend_from_slice(&[2, 0, 0, 0, 7, 8, 0xf6, 0, 0, 0xf6, 0xf6, 42, 0xe2, 0xe3]);
        payload.extend_from_slice(&[3, 0, 0, 0, 8, 9, 10, 1, 0, 11, 12, 43, 0xe2]);
        payload.extend_from_slice(b"\xe3S2D0004\0");
        payload.extend_from_slice(&[2, 0, 0, 0, 1, 2, 0xf6, 0, 0, 0xf6, 0xf6, 42, 0xe2]);

        let segments = positional_segment_table(&payload, 0, payload.len())
            .expect("first positional segment table");

        assert_eq!(segments.declared_count, 3);
        assert_eq!(segments.rows.len(), 2);
        assert!(!segments.is_complete());
        assert!(segments.segment(42).is_none());
        assert_eq!(segments.rows[0].external_id, 42);
        assert_eq!(segments.rows[1].external_id, 43);
    }

    #[test]
    fn segment_tables_retain_extents_without_decoded_rows() {
        let named = b"segtab_ptr\0\xf8\x02\xf7\x01\xfb\xe2\xf2\xf7\x01\xe2";
        let segments = segment_table(named, 0, named.len()).expect("named segtab header");
        assert_eq!(segments.declared_count, 2);
        assert_eq!(segments.entity_ref, Some(1));
        assert!(segments.rows.is_empty());
        assert!(!segments.is_complete());

        let positional = b"\xf8\x02\xf7\x01\xfb\xe2\xf2\xf7\x01\xe2";
        let segments = segment_table_body(positional, 0, 0, positional.len())
            .expect("positional segtab header");
        assert_eq!(segments.declared_count, 2);
        assert_eq!(segments.entity_ref, Some(1));
        assert!(segments.rows.is_empty());
        assert!(!segments.is_complete());
    }

    #[test]
    fn segment_tables_retain_fully_framed_opaque_families() {
        let mut payload = b"segtab_ptr\0\xf8\x02\xf7\x01\xfb\xe2\
            type\0\xc0\x80\x01dir\0\xf8\x03\x00\xe5\xe4\
            pointid\0\xf8\x02\xf6\xe4cntrid\0\x00arcorient\0\x00\
            verhor\0\x00radius\0\xf6radius2\0\xf6ext_id\0\x04\
            \xf2\xf7\x01\xe2"
            .to_vec();
        payload.extend_from_slice(&[0x19, 0, 1, 0, 10, 11, 0xf6, 0, 0, 0xf6, 0xf6, 1, 0xe2]);

        let segments = segment_table(&payload, 0, payload.len()).expect("segment table");

        assert!(segments.is_complete());
        assert!(segments.rows.is_empty());
        assert_eq!(segments.opaque_rows.len(), 2);
        assert_eq!(segments.opaque_rows[0].kind, 1);
        assert_eq!(segments.opaque_rows[0].point_ids, [None, Some(1)]);
        assert_eq!(segments.opaque_rows[0].external_id, 4);
        assert_eq!(segments.opaque_rows[1].kind, 25);
        assert_eq!(segments.opaque_rows[1].point_ids, [Some(10), Some(11)]);
        assert_eq!(segments.opaque_rows[1].external_id, 1);

        let malformed_known = [
            0xf8, 1, 0xf7, 1, 0xfb, 0xe2, 0xf2, 0xf7, 1, 0xe2, 2, 0, 1, 0, 10, 0xf6, 0xf6, 0, 0,
            0xf6, 0xf6, 1, 0xe2,
        ];
        let segments = segment_table_body(&malformed_known, 0, 0, malformed_known.len())
            .expect("malformed known segment table");
        assert!(!segments.is_complete());
        assert!(segments.rows.is_empty());
        assert!(segments.opaque_rows.is_empty());
    }

    #[test]
    fn segment_rows_expand_compact_slots_and_accept_the_c1_type_wrapper() {
        let payload = [
            0xf8, 1, 0xf7, 1, 0xfb, 0xe2, 0xf2, 0xf7, 1, 0xe2, 0xc1, 0x00, 2, 0xe5, 0xe4, 9, 11,
            0xf6, 3, 0, 0xe6, 0xe2,
        ];

        let segments = segment_table_body(&payload, 0, 0, payload.len()).expect("segment table");

        assert!(segments.is_complete());
        assert_eq!(segments.rows.len(), 1);
        assert_eq!(segments.rows[0].kind, FeatureSegmentKind::Line);
        assert_eq!(segments.rows[0].directions, [Some(0), Some(0), Some(1)]);
        assert_eq!(segments.rows[0].point_ids, [9, 11]);
        assert_eq!(segments.rows[0].external_id, 0);
    }

    #[test]
    fn positional_dimension_table_uses_the_inherited_table_class() {
        let mut payload = b"prefix\xf8\x02\xf7\x58\xfb\xe2\xf7\x59".to_vec();
        payload.extend_from_slice(&[2, 0x46, 0x08, 0, 0, 0, 0, 0, 0, 0, 0x18, 43]);
        payload.extend_from_slice(b"\xf3\xf7\x58\xe2");
        payload.extend_from_slice(&[10, 0x60, 0xc8, 0x1e, 0x15, 0xd4, 0xaf, 0x9f, 0, 0x18, 44]);
        let cache = scalar::ScalarCache::from_section(&payload);

        let dimensions = positional_dimension_table(&payload, 0, payload.len(), 88, &cache)
            .expect("positional dimtab");

        assert_eq!(dimensions.declared_count, 2);
        assert_eq!(dimensions.entity_ref, Some(88));
        assert_eq!(dimensions.rows.len(), 2);
        assert_eq!(dimensions.rows[0].value, Some(3.0));
        assert_eq!(dimensions.rows[0].external_id, 43);
        assert_eq!(dimensions.rows[1].dimension_type, 10);
        assert_eq!(dimensions.rows[1].external_id, 44);
    }

    #[test]
    fn positional_dimension_table_is_self_describing_when_multiple_rows_close() {
        let mut payload = b"prefix\xf8\x04\xf7\x58\xfb\xe2\xf7\x59".to_vec();
        for (index, row) in [
            [1, 0xe4, 0, 0x18, 2],
            [2, 0x0e, 0, 0x18, 0],
            [2, 0xe4, 0, 0x18, 3],
            [2, 0xe4, 0, 0x18, 1],
        ]
        .into_iter()
        .enumerate()
        {
            payload.extend_from_slice(&row);
            if index < 3 {
                payload.extend_from_slice(b"\xf3\xf7\x58\xe2");
            }
        }
        let cache = scalar::ScalarCache::from_section(&payload);

        let dimensions =
            self_described_positional_dimension_table(&payload, 0, payload.len(), &cache)
                .expect("self-described dimension table");

        assert_eq!(dimensions.entity_ref, Some(88));
        assert_eq!(dimensions.rows.len(), 4);
        assert_eq!(dimensions.rows[0].external_id, 2);
        assert_eq!(dimensions.rows[1].value, Some(-0.5));
    }

    #[test]
    fn one_row_positional_table_does_not_self_identify_as_dimensions() {
        let payload = b"\xf8\x01\xf7\x58\xfb\xe2\xf7\x59\x01\xe4\x00\x18\x02";
        assert_eq!(
            self_described_positional_dimension_table(
                payload,
                0,
                payload.len(),
                &scalar::ScalarCache::default(),
            ),
            None
        );
    }

    #[test]
    fn positional_dimension_table_retains_bounded_opaque_values() {
        let mut payload = b"prefix\xf8\x03\xf7\x58\xfb\xe2\xf7\x59".to_vec();
        payload.extend_from_slice(&[2, 0x46, 0x08, 0, 0, 0, 0, 0, 0, 0, 0x18, 43]);
        payload.extend_from_slice(b"\xf3\xf7\x58\xe2");
        payload.extend_from_slice(&[1, 0x00, 0x04, 0xa6, 0, 0x18, 44]);
        payload.extend_from_slice(b"\xf3\xf7\x58\xe2");
        payload.extend_from_slice(&[5, 0x0d, 0, 0x18, 45]);
        let cache = scalar::ScalarCache::from_section(&payload);

        let dimensions = positional_dimension_table(&payload, 0, payload.len(), 88, &cache)
            .expect("positional dimtab");

        assert_eq!(dimensions.rows.len(), 3);
        assert_eq!(dimensions.rows[1].value, None);
        assert_eq!(
            dimensions.rows[1].unresolved_value_token.as_deref(),
            Some(&[0x00, 0x04, 0xa6][..])
        );
        assert_eq!(dimensions.rows[1].external_id, 44);
        assert_eq!(dimensions.rows[2].value, Some(-1.0));
        assert_eq!(dimensions.rows[2].external_id, 45);
    }

    #[test]
    fn positional_dimensions_decode_the_positive_dict_lattice_and_bounded_opaque_forms() {
        let positive = [1, 0x53, 0xa1, 0xca, 0xc0, 0x83, 0x12, 0x6f, 0, 0x18, 46];
        let opaque_three = [1, 0x00, 0x04, 0xa6, 0, 0x18, 47];
        let opaque_four = [1, 0x01, 0x04, 0xfe, 0xf2, 0, 0x18, 48];
        let zero = [2, 0x18, 0, 0x18, 49];
        let negative_half = [1, 0x0e, 0, 0x18, 50];
        let cache = scalar::ScalarCache::default();

        let positive_row = positional_dimension(&positive, 0, positive.len(), &cache)
            .expect("positive dictionary dimension");
        assert_eq!(
            positive_row.value,
            Some(f64::from_be_bytes([
                0x3f, 0xc8, 0xa1, 0xca, 0xc0, 0x83, 0x12, 0x6f,
            ]))
        );
        assert_eq!(positive_row.direction_byte, 0);
        assert_eq!(positive_row.auxiliary_value, Some(0.0));
        assert_eq!(positive_row.external_id, 46);
        for (body, external_id, token) in [
            (&opaque_three[..], 47, &[0x00, 0x04, 0xa6][..]),
            (&opaque_four[..], 48, &[0x01, 0x04, 0xfe, 0xf2][..]),
        ] {
            let row = positional_dimension(body, 0, body.len(), &cache)
                .expect("bounded opaque dimension");
            assert_eq!(row.value, None);
            assert_eq!(row.unresolved_value_token.as_deref(), Some(token));
            assert_eq!(row.external_id, external_id);
        }
        let zero_row = positional_dimension(&zero, 0, zero.len(), &cache).expect("zero dimension");
        assert_eq!(zero_row.value, Some(0.0));
        assert_eq!(zero_row.external_id, 49);
        let negative_half_row =
            positional_dimension(&negative_half, 0, negative_half.len(), &cache)
                .expect("negative half dimension");
        assert_eq!(negative_half_row.value, Some(-0.5));
        assert_eq!(negative_half_row.external_id, 50);
    }

    #[test]
    fn positional_dimension_seven_byte_positive_value_preserves_field_alignment() {
        let body = [2, 0x31, 0x60, 0x07, 0x53, 0x93, 0xb5, 0xe5, 0, 0x18, 27];
        let row = positional_dimension(&body, 0, body.len(), &scalar::ScalarCache::default())
            .expect("seven-byte positive dimension");

        assert_eq!(
            row.value,
            Some(f64::from_be_bytes([
                0x40, 0x60, 0x07, 0x53, 0x93, 0xb5, 0xe5, 0,
            ]))
        );
        assert_eq!(row.direction_byte, 0);
        assert_eq!(row.auxiliary_value, Some(0.0));
        assert_eq!(row.external_id, 27);
    }

    #[test]
    fn dimension_tables_retain_extents_without_decoded_rows() {
        let named = b"dimtab_ptr\0\xf8\x02\xf7\x58\xfb\xe2";
        let cache = scalar::ScalarCache::from_section(named);
        let dimensions =
            dimension_table(named, 0, named.len(), &cache).expect("named dimtab header");
        assert_eq!(dimensions.declared_count, 2);
        assert_eq!(dimensions.entity_ref, Some(88));
        assert!(dimensions.rows.is_empty());

        let positional = b"\xf8\x02\xf7\x58\xfb\xe2\xf7\x59";
        let cache = scalar::ScalarCache::from_section(positional);
        let dimensions = positional_dimension_table(positional, 0, positional.len(), 88, &cache)
            .expect("positional dimtab header");
        assert_eq!(dimensions.declared_count, 2);
        assert_eq!(dimensions.entity_ref, Some(88));
        assert!(dimensions.rows.is_empty());
    }

    #[test]
    fn positional_definition_inherits_the_labeled_dimension_table_class() {
        let mut payload = b"feat_defs_917\0dimtab_ptr\0\xf8\x01\xf7\x58\xfb\xe2\
            type\0\x01value\0\xe4direct\0\x00aux_value\0\x18ext_id\0\x04\
            \xe0\x01feat_id\0\x2a\xe0\x00ref_model_info\0\xe3S2D0004\0\
            \xf8\x01\xf7\x58\xfb\xe2\xf7\x59"
            .to_vec();
        payload.extend_from_slice(&[2, 0x46, 0x08, 0, 0, 0, 0, 0, 0, 0, 0x18, 43]);

        let decoded = definitions(&payload);
        let dimensions = decoded[1].dimensions.as_ref().expect("positional dimtab");

        assert_eq!(decoded[1].owner_feature_id, Some(42));
        assert_eq!(dimensions.entity_ref, Some(88));
        assert_eq!(dimensions.rows.len(), 1);
        assert_eq!(dimensions.rows[0].value, Some(3.0));
        assert_eq!(dimensions.rows[0].external_id, 43);
    }

    #[test]
    fn depdb_gsec2d_definition_anchors_positional_table_replay() {
        let mut payload = b"gsec2d_ptr\0\xe0\x0aname\0S2D0002\0\
            dimtab_ptr\0\xf8\x01\xf7\x58\xfb\xe2\
            type\0\x01value\0\xe4direct\0\x00aux_value\0\x18ext_id\0\x04\
            \xe3S2D0003\0\xf8\x01\xf7\x58\xfb\xe2\xf7\x59"
            .to_vec();
        payload.extend_from_slice(&[2, 0x46, 0x08, 0, 0, 0, 0, 0, 0, 0, 0x18, 43]);

        let decoded = depdb_definitions(&payload);
        let dimensions = decoded[1].dimensions.as_ref().expect("positional dimtab");

        assert_eq!(decoded.len(), 2);
        assert_eq!(decoded[0].id, 2);
        assert_eq!(decoded[1].id, 2);
        assert!(decoded
            .iter()
            .all(|definition| definition.owner_feature_id.is_none()));
        assert_eq!(dimensions.entity_ref, Some(88));
        assert_eq!(dimensions.rows.len(), 1);
        assert_eq!(dimensions.rows[0].value, Some(3.0));
        assert_eq!(dimensions.rows[0].external_id, 43);
    }

    #[test]
    fn positional_variable_table_joins_coordinate_rows() {
        let payload = b"prefix\xf8\x02\xf7\x77\xfb\xe2\xf7\x78\
            \x01\x07\x18\x18\x01\x00\x09\xf1\xf7\x77\xe2\
            \x02\x07\x18\x18\x01\x00\x0a";
        let cache = scalar::ScalarCache::from_section(payload);

        let variables = positional_variable_table(payload, 0, payload.len(), 119, &cache)
            .expect("positional var_arr");

        assert_eq!(variables.declared_count, 2);
        assert_eq!(variables.entity_ref, Some(119));
        assert_eq!(variables.rows.len(), 2);
        assert!(variables.is_complete());
        assert_eq!(variables.rows[0].uvar_id, Some(9));
        assert_eq!(variables.rows[1].uvar_id, Some(10));
        assert_eq!(variables.points.len(), 1);
        assert_eq!(variables.points[0].point_id, 7);
        assert_eq!(variables.points[0].u, Some(0.0));
        assert_eq!(variables.points[0].v, Some(0.0));
    }

    #[test]
    fn variable_tables_retain_extents_without_decoded_rows() {
        let named = b"var_arr\0\xf8\x02\xf7\x77\xfb\xe2\xf1\xf7\x77\xe2";
        let cache = scalar::ScalarCache::from_section(named);
        let variables =
            variable_table(named, 0, named.len(), &cache).expect("named var_arr header");
        assert_eq!(variables.declared_count, 2);
        assert_eq!(variables.entity_ref, Some(119));
        assert!(variables.rows.is_empty());
        assert!(variables.points.is_empty());
        assert!(!variables.is_complete());

        let positional = b"\xf8\x02\xf7\x77\xfb\xe2\xf7\x78";
        let cache = scalar::ScalarCache::from_section(positional);
        let variables = positional_variable_table(positional, 0, positional.len(), 119, &cache)
            .expect("positional var_arr header");
        assert_eq!(variables.declared_count, 2);
        assert_eq!(variables.entity_ref, Some(119));
        assert!(variables.rows.is_empty());
        assert!(variables.points.is_empty());
        assert!(!variables.is_complete());
    }

    #[test]
    fn variable_table_withholds_duplicate_coordinate_identities() {
        let row = |variable_type, value, offset| FeatureVariableRow {
            variable_type,
            key: 7,
            value: Some(value),
            guess: None,
            known: None,
            homogeneity: None,
            uvar_id: None,
            dimension_driven: false,
            offset,
        };
        let table = variable_table_from_rows(
            3,
            Some(119),
            vec![row(1, 2.0, 10), row(1, 2.0, 20), row(2, 3.0, 30)],
            5,
        );

        assert_eq!(table.rows.len(), 3);
        assert_eq!(table.points.len(), 1);
        assert_eq!(table.points[0].point_id, 7);
        assert_eq!(table.points[0].u, None);
        assert_eq!(table.points[0].v, Some(3.0));
    }

    #[test]
    fn radius_variables_do_not_create_section_points() {
        let row = |variable_type, key, value, offset| FeatureVariableRow {
            variable_type,
            key,
            value: Some(value),
            guess: None,
            known: None,
            homogeneity: None,
            uvar_id: None,
            dimension_driven: false,
            offset,
        };
        let table = variable_table_from_rows(
            3,
            Some(119),
            vec![row(1, 7, 2.0, 10), row(2, 7, 3.0, 20), row(3, 99, 4.0, 30)],
            5,
        );

        assert_eq!(table.points.len(), 1);
        assert_eq!(table.points[0].point_id, 7);
        let (points, ambiguous) = table.reconciled_points();
        assert_eq!(points.get(&7), Some(&[Some(2.0), Some(3.0)]));
        assert!(!points.contains_key(&99));
        assert!(ambiguous.is_empty());
    }

    #[test]
    fn variable_coordinate_7e_and_c6_are_the_f3_dict_sign_pair() {
        let positive = [0x7e, 0x6b, 0x37, 0x21, 0xad, 0xb3, 0xb7];
        let negative = [0xc6, 0x6b, 0x37, 0x21, 0xad, 0xb3, 0xb7];
        let cache = scalar::ScalarCache::from_section(&positive);

        assert_eq!(
            decode_variable_scalar(&positive, 0, positive.len(), &cache),
            (
                Some(f64::from_be_bytes([
                    0x3f, 0xf3, 0x6b, 0x37, 0x21, 0xad, 0xb3, 0xb7
                ])),
                7,
                false
            )
        );
        assert_eq!(
            decode_variable_scalar(&negative, 0, negative.len(), &cache),
            (
                Some(f64::from_be_bytes([
                    0xbf, 0xf3, 0x6b, 0x37, 0x21, 0xad, 0xb3, 0xb7
                ])),
                7,
                false
            )
        );
    }

    #[test]
    fn positional_gsec3d_decodes_placement_and_reference_rows() {
        let payload = b"prefix\x07S2D0004\0\x01\xf6\xe1\xf6\x82\x01\xf6\
            \xf8\x02\xf7\x39\xfb\xe2\xf7\x3a\
            \x06\x05\xf6\x03\xf6\x00\xe3tail\xf2\xf7\x39\xe2\
            \x07\x05\xf6\x04\xf6\x01";

        let section = positional_section_3d(payload, 0, payload.len()).expect("positional gsec3d");

        assert_eq!(section.sketch_plane_entity_id, Some(513));
        assert_eq!(section.sketch_plane_flip, None);
        assert_eq!(section.reference_plane_entity_ids, vec![6, 7]);
        assert_eq!(section.reference_plane_datum_geometry_id, None);
        assert_eq!(section.orientation.section_flip, Some(BinaryFlag::Set));
        assert_eq!(section.orientation.reference_type, Some(5));
        assert_eq!(section.orientation.segment_id, Some(3));
        assert_eq!(section.orientation.reference_flip, Some(BinaryFlag::Clear));
    }

    #[test]
    fn positional_gsec3d_retains_its_header_without_a_body() {
        let payload = b"prefix\x07S2D0004\0";

        let section = positional_section_3d(payload, 0, payload.len()).expect("positional gsec3d");

        assert_eq!(section.offset, 6);
        assert_eq!(section.sketch_plane_entity_id, None);
        assert!(section.reference_plane_entity_ids.is_empty());
        assert_eq!(section.orientation, FeatureSectionOrientation::default());
    }

    #[test]
    fn positional_gsec3d_retains_placement_and_complete_reference_prefix() {
        let payload = b"prefix\x07S2D0004\0\x01\xf6\xe1\xf6\x82\x01\xf6\
            \xf8\x02\xf7\x39\xfb\xe2\xf7\x3a\
            \x06\x05\xf6\x03\xf6\x00\xe3tail\xf2\xf7\x39\xe2\x07";

        let section = positional_section_3d(payload, 0, payload.len()).expect("positional gsec3d");

        assert_eq!(section.sketch_plane_entity_id, Some(513));
        assert_eq!(section.reference_plane_entity_ids, [6]);
        assert_eq!(section.orientation.section_flip, Some(BinaryFlag::Set));
        assert_eq!(section.orientation.reference_type, Some(5));
        assert_eq!(section.orientation.segment_id, Some(3));
        assert_eq!(section.orientation.reference_flip, Some(BinaryFlag::Clear));
    }

    #[test]
    fn positional_relation_table_replays_rows_after_its_prototype() {
        let payload = b"prefix\xf8\x03\xf7\x64\xfb\xe2\xf7\x65\
            prototype\xf1\xf7\x64\xe2\
            \x08\x00\x03\x0f\xf6\xe4\x01\xe4\x00\xe4\x0f\x10\x0f\x18\x00\xf6\x00\xe2";

        let relations = positional_relation_table(payload, 0, payload.len(), 100)
            .expect("positional relat_ptr");

        assert_eq!(relations.declared_count, 3);
        assert_eq!(relations.entity_ref, Some(100));
        assert_eq!(relations.rows.len(), 1);
        assert_eq!(relations.rows[0].relation_id, 8);
        assert_eq!(relations.rows[0].used, 0);
        assert_eq!(relations.rows[0].sign, 0);
        assert_eq!(relations.rows[0].dimension_id, 246);
        assert_eq!(relations.rows[0].relation_type, 0);
        assert!(relations.rows[0].operand_vectors.is_some());
    }

    #[test]
    fn relation_table_retains_solver_children_after_an_invalid_row() {
        let payload = b"relat_ptr\0\xf4\x04\xf8\x03\xf7\x6a\xfb\xe2\
            schema\xf1\xf7\x6a\xe2invalid\
            skamp_ptr\0\xf3\xf8\x01\xf7\x6b\xfb\xe2\
            \xe0\x01id\0\x05\xe0\x01type\0\x02\xe0\x01flags\0\x03\
            \xe0\x01status\0\x04\xe0\x00items\0\xf8\x01\xf7\x6c\xfb\xe2\
            \xe0\x01ent_id\0\x2a\xe0\x01sense\0\x01\xf1\xf7\x6c\xe2\
            \xf3\xf7\x6b\xe2";

        let relations = relation_table(payload, 0, payload.len()).expect("relat_ptr header");

        assert_eq!(relations.declared_count, 3);
        assert_eq!(relations.entity_ref, Some(106));
        assert!(relations.rows.is_empty());
        assert_eq!(relations.skamps.len(), 1);
        assert_eq!(relations.skamps[0].id, 5);
    }

    #[test]
    fn relation_tables_retain_extents_without_their_prototypes() {
        let named = b"relat_ptr\0\xf8\x03\xf7\x64\xfb\xe2";
        let relations = relation_table(named, 0, named.len()).expect("named relat_ptr header");
        assert_eq!(relations.declared_count, 3);
        assert_eq!(relations.entity_ref, Some(100));
        assert!(relations.rows.is_empty());

        let positional = b"\xf8\x03\xf7\x64\xfb\xe2";

        let relations = positional_relation_table(positional, 0, positional.len(), 100)
            .expect("positional relat_ptr header");

        assert_eq!(relations.declared_count, 3);
        assert_eq!(relations.entity_ref, Some(100));
        assert!(relations.rows.is_empty());
    }

    #[test]
    fn positional_skamp_table_replays_counted_nested_items() {
        let payload = b"\xf8\x02\xf7\x58\xfb\xe2\xf7\x59\
            \x01\x00\x00\x23\xf8\x02\xf7\x60\xfb\xe2\xf7\x61\
            \x06\x03\xf1\xf7\x60\xe2\x07\x02\xf3\xf7\x58\xe2\
            \x02\x01\x00\x23\xf8\x01\xf7\x60\xfb\xe2\xf7\x61\x08\x00";

        let skamps = positional_feature_skamps(payload, 0, payload.len(), 88);

        assert_eq!(skamps.len(), 2);
        assert_eq!(skamps[0].id, 1);
        assert_eq!(skamps[0].kind, 0);
        assert_eq!(skamps[0].items.len(), 2);
        assert_eq!(skamps[0].items[0].entity_id, 6);
        assert_eq!(skamps[0].items[1].sense, 2);
        assert_eq!(skamps[1].kind, 1);
        assert_eq!(skamps[1].items[0].entity_id, 8);
    }

    #[test]
    fn positional_solver_tables_retain_complete_prefix_rows() {
        let skamps = b"\xf8\x02\xf7\x58\xfb\xe2\xf7\x59\
            \x01\x00\x00\x23\xf8\x02\xf7\x60\xfb\xe2\xf7\x61\
            \x06\x03\xf1\xf7\x60\xe2\x07\x02\xf3\xf7\x58\xe2";
        let rows = positional_feature_skamps(skamps, 0, skamps.len(), 88);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, 1);

        let triples = b"\xf8\x02\xf7\x64\xfb\xe2\xf7\x65\
            \x01\xf6\x04\xf1\xf7\x64\xe2";
        let rows = positional_relation_triples(triples, 0, triples.len(), 100);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].relation_id, Some(1));
    }

    #[test]
    fn solver_header_does_not_adopt_a_later_array() {
        let payload = b"skamp_ptr\0opaque\xf8\x02\xf7\x58\xfb\xe2";

        assert!(named_solver_table_header(payload, b"skamp_ptr\0", 0, payload.len()).is_none());
    }

    #[test]
    fn named_solver_tables_retain_complete_prefix_rows() {
        let skamps = b"skamp_ptr\0\xf3\xf8\x02\xf7\x6b\xfb\xe2\
            \xe0\x01id\0\x05\xe0\x01type\0\x02\xe0\x01flags\0\x03\
            \xe0\x01status\0\x04\xe0\x00items\0\xf8\x01\xf7\x6c\xfb\xe2\
            \xe0\x01ent_id\0\x2a\xe0\x01sense\0\x01\xf1\xf7\x6c\xe2\
            \xf3\xf7\x6b\xe2invalid";
        let rows = feature_skamps(skamps, 0, skamps.len());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, 5);

        let triples = b"triples_ptr\0\xf4\x04\xf8\x02\xf7\x6d\xfb\xe2\
            \xe0\x01rel_id\0\x07\xe0\x01eqn_id\0\x08\
            \xe0\x01skamp_id\0\x05\xf1\xf7\x6d\xe2\x01\x02\x03";
        let rows = feature_relation_triples(triples, 0, triples.len());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].relation_id, Some(7));
    }

    #[test]
    fn positional_triples_replay_nullable_relation_joins() {
        let payload = b"\xf8\x02\xf7\x64\xfb\xe2\xf7\x65\
            \x01\xf6\x04\xf1\xf7\x64\xe2\x02\xf6\x05";

        let triples = positional_relation_triples(payload, 0, payload.len(), 100);

        assert_eq!(triples.len(), 2);
        assert_eq!(triples[0].relation_id, Some(1));
        assert_eq!(triples[0].equation_id, None);
        assert_eq!(triples[0].skamp_id, Some(4));
        assert_eq!(triples[1].relation_id, Some(2));
        assert_eq!(triples[1].skamp_id, Some(5));
    }

    #[test]
    fn positional_trim_entity_table_decodes_without_segments() {
        let payload = b"prefix\xf8\x07\xf7\x42\xfb\xe2\xf7\x43\x00\xe3\
            \x09\x00\x03\x04\xf6\x00\
            \xf4\x04\xf7\x42\xe2\x01\xf8\x13\xf7\x44\xfb\xe2";
        let entities = positional_trim_entity_table(
            payload,
            0,
            payload.len(),
            TrimTableClasses {
                table: 66,
                bucket: 67,
                entry: 67,
            },
            Some(68),
        )
        .expect("positional ent_tab");

        assert_eq!(entities.declared_count, Some(7));
        assert_eq!(entities.entity_ref, Some(66));
        assert_eq!(entities.entry_ref, Some(67));
        assert_eq!(entities.solved_external_ids, vec![9]);
        assert_eq!(entities.rows[0].vertices, [3, 4]);
        assert_eq!(entities.rows[0].kind, TrimEntityKind::Line);
    }

    #[test]
    fn positional_trim_entity_table_retains_an_empty_extent() {
        let payload = b"prefix\xf8\x00\xf7\x42\xfb\xe2\
            \xf8\x01\xf7\x44\xfb\xe2";

        let entities = positional_trim_entity_table(
            payload,
            0,
            payload.len(),
            TrimTableClasses {
                table: 66,
                bucket: 67,
                entry: 67,
            },
            Some(68),
        )
        .expect("empty positional ent_tab");

        assert_eq!(entities.declared_count, Some(0));
        assert_eq!(entities.entity_ref, Some(66));
        assert_eq!(entities.entry_ref, Some(67));
        assert!(entities.rows.is_empty());
        assert!(entities.solved_external_ids.is_empty());
    }

    #[test]
    fn positional_trim_entity_table_withholds_rows_without_the_entry_class() {
        let payload = b"prefix\xf8\x01\xf7\x42\xfb\xe2\
            \x00\xe3\x09\x00\x03\x04\xf6\x00";

        let entities = positional_trim_entity_table(
            payload,
            0,
            payload.len(),
            TrimTableClasses {
                table: 66,
                bucket: 67,
                entry: 67,
            },
            None,
        )
        .expect("positional ent_tab header");

        assert_eq!(entities.declared_count, Some(1));
        assert!(entities.rows.is_empty());
        assert!(entities.solved_external_ids.is_empty());
    }

    #[test]
    fn positional_order_table_replays_prototype_and_following_rows() {
        let payload = b"prefix\xf8\x03\xf7\x42\xfb\xe2\xf7\x43\
            \x09\x01\x00\xf1\xf7\x42\xe2\
            \x0a\x02\x01\xe2\x0b\x03\x00";

        let order =
            positional_order_table(payload, 0, payload.len(), 66).expect("positional order_table");

        assert_eq!(order.declared_count, 3);
        assert!(order.has_prototype);
        assert!(order.is_complete());
        assert_eq!(order.entity_ref, Some(66));
        assert_eq!(order.rows.len(), 2);
        assert_eq!(order.rows[0].external_id, 10);
        assert_eq!(order.rows[0].internal_id, 2);
        assert_eq!(order.rows[0].bitmask, 1);
        assert_eq!(order.rows[1].external_id, 11);
        assert_eq!(order.internal_id(10), Some(2));
        assert_eq!(order.external_id(2), Some(10));

        let mut duplicate_external = order.clone();
        duplicate_external.declared_count += 1;
        duplicate_external.rows.push(FeatureOrderRow {
            external_id: 10,
            internal_id: 4,
            bitmask: 0,
            offset: 20,
        });
        assert_eq!(duplicate_external.internal_id(10), None);
        assert_eq!(duplicate_external.external_id(2), None);
        let mut duplicate_internal = order;
        duplicate_internal.declared_count += 1;
        duplicate_internal.rows.push(FeatureOrderRow {
            external_id: 12,
            internal_id: 2,
            bitmask: 0,
            offset: 21,
        });
        assert_eq!(duplicate_internal.external_id(2), None);
        assert_eq!(duplicate_internal.internal_id(10), None);
    }

    #[test]
    fn named_order_table_replays_prototype_and_following_rows() {
        let payload = b"order_table\0\xf8\x03\xf7\x42\xfb\xe2\
            \xe0\x01ext_id\0\x09\xe0\x01int_id\0\x01\
            \xe0\x01bitmask\0\x00\xf1\xf7\x42\xe2\
            \x0a\x02\x01\xe2\x0b\x03\x00";

        let order = order_table(payload, 0, payload.len()).expect("named order_table");

        assert_eq!(order.declared_count, 3);
        assert!(order.has_prototype);
        assert!(order.is_complete());
        assert_eq!(order.entity_ref, Some(66));
        assert_eq!(order.rows.len(), 2);
        assert_eq!(order.external_id(2), Some(10));
        assert_eq!(order.internal_id(11), Some(3));
    }

    #[test]
    fn order_tables_retain_extents_without_decoded_rows() {
        let named = b"order_table\0\xf8\x02\xf7\x42\xfb\xe2\xf1\xf7\x42\xe2";
        let order = order_table(named, 0, named.len()).expect("named order_table header");
        assert_eq!(order.declared_count, 2);
        assert!(!order.has_prototype);
        assert!(!order.is_complete());
        assert_eq!(order.entity_ref, Some(66));
        assert!(order.rows.is_empty());

        let positional = b"\xf8\x02\xf7\x42\xfb\xe2";
        let order = positional_order_table(positional, 0, positional.len(), 66)
            .expect("positional order_table header");
        assert_eq!(order.declared_count, 2);
        assert!(!order.has_prototype);
        assert!(!order.is_complete());
        assert_eq!(order.entity_ref, Some(66));
        assert!(order.rows.is_empty());
    }

    #[test]
    fn incomplete_order_tables_do_not_resolve_identifiers() {
        let named = b"order_table\0\xf8\x02\xf7\x42\xfb\xe2\
            \xf1\xf7\x42\xe2\x0a\x02\x00";
        let order = order_table(named, 0, named.len()).expect("named order_table");
        assert_eq!(order.rows.len(), 1);
        assert!(!order.is_complete());
        assert_eq!(order.internal_id(10), None);
        assert_eq!(order.external_id(2), None);

        let positional = b"\xf8\x02\xf7\x42\xfb\xe2";
        let order = positional_order_table(positional, 0, positional.len(), 66)
            .expect("positional order_table");
        assert!(!order.is_complete());
        assert_eq!(order.internal_id(10), None);
    }

    #[test]
    fn positional_trim_vertex_table_is_independent_of_entity_rows() {
        let payload = b"prefix\xf8\x13\xf7\x44\xfb\xe2\xf7\x45\
            \x01\x02\x03\x00\xe2";
        let vertices = positional_trim_vertex_table(
            payload,
            0,
            payload.len(),
            TrimTableClasses {
                table: 68,
                bucket: 69,
                entry: 69,
            },
            None,
            None,
        )
        .expect("positional vert_tab");

        assert_eq!(vertices.declared_count, Some(19));
        assert_eq!(vertices.entity_ref, Some(68));
        assert_eq!(vertices.entry_ref, Some(69));
        assert_eq!(vertices.rows.len(), 1);
        assert_eq!(vertices.rows[0].vertex_id, 3);
        assert_eq!(vertices.rows[0].entities, [1, 2]);
    }

    #[test]
    fn positional_trim_vertex_table_retains_an_empty_extent() {
        let payload = b"prefix\xf8\x00\xf7\x44\xfb\xe2";

        let vertices = positional_trim_vertex_table(
            payload,
            0,
            payload.len(),
            TrimTableClasses {
                table: 68,
                bucket: 69,
                entry: 69,
            },
            None,
            None,
        )
        .expect("empty positional vert_tab");

        assert_eq!(vertices.declared_count, Some(0));
        assert_eq!(vertices.entity_ref, Some(68));
        assert_eq!(vertices.entry_ref, Some(69));
        assert!(vertices.rows.is_empty());
    }

    #[test]
    fn trim_vertex_uses_unique_shared_point_for_mixed_curves() {
        let segment = |kind, point_ids, external_id| FeatureSegment {
            kind,
            directions: [None; 3],
            point_ids,
            center_id: (kind == FeatureSegmentKind::Arc).then_some(4),
            arc_orientation: (kind == FeatureSegmentKind::Arc).then_some(0),
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id,
            offset: 0,
        };
        let segments = FeatureSegmentTable {
            declared_count: 2,
            entity_ref: None,
            rows: vec![
                segment(FeatureSegmentKind::Line, [1, 2], 9),
                segment(FeatureSegmentKind::Arc, [2, 3], 10),
            ],
            opaque_rows: Vec::new(),
            offset: 0,
        };
        let variables = FeatureVariableTable {
            declared_count: 0,
            entity_ref: None,
            rows: Vec::new(),
            points: vec![FeatureSectionPoint {
                point_id: 2,
                u: Some(3.0),
                v: Some(4.0),
            }],
            offset: 0,
        };

        assert_eq!(
            entity_intersection([9, 10], Some(&segments), Some(&variables)),
            Some([3.0, 4.0])
        );

        let mut duplicate_segments = segments.clone();
        duplicate_segments.rows.push(segments.rows[0].clone());
        assert!(duplicate_segments.segment(9).is_none());
        assert!(
            entity_intersection([9, 10], Some(&duplicate_segments), Some(&variables)).is_none()
        );

        let mut duplicate_points = variables.clone();
        duplicate_points.points.push(variables.points[0].clone());
        assert_eq!(
            duplicate_points.reconciled_points().0.get(&2),
            Some(&[Some(3.0), Some(4.0)])
        );
        assert_eq!(
            entity_intersection([9, 10], Some(&segments), Some(&duplicate_points)),
            Some([3.0, 4.0])
        );
        duplicate_points.points[1].u = Some(5.0);
        assert!(duplicate_points.reconciled_points().1.contains(&2));
        assert!(entity_intersection([9, 10], Some(&segments), Some(&duplicate_points)).is_none());
        let row = |variable_type, value, offset| FeatureVariableRow {
            variable_type,
            key: 2,
            value: Some(value),
            guess: None,
            known: None,
            homogeneity: None,
            uvar_id: None,
            dimension_driven: false,
            offset,
        };
        let mut repeated_raw = variables.clone();
        repeated_raw.points[0] = FeatureSectionPoint {
            point_id: 2,
            u: None,
            v: None,
        };
        repeated_raw.rows = vec![row(1, 3.0, 30), row(1, 3.0, 31), row(2, 4.0, 32)];
        assert_eq!(
            repeated_raw.reconciled_points().0.get(&2),
            Some(&[Some(3.0), Some(4.0)])
        );
        repeated_raw.rows[1].value = Some(5.0);
        assert!(repeated_raw.reconciled_points().1.contains(&2));
    }

    #[test]
    fn trim_vertex_template_identifies_table_and_entry_classes() {
        let payload = b"vert_tab\0\xf8\x13\xf7\x44\xfb\xe2\
            attrs\0\xf1\xf7\x46\xe3bucket_xar\0\xf8\x01\xf7\x46\xfb\xe3\
            \xf7\x45\x09\x0a\x03\x00";

        assert_eq!(
            trim_table_header(payload, b"vert_tab\0", 0, payload.len()),
            Some(TrimTableHeader {
                declared_count: 19,
                classes: TrimTableClasses {
                    table: 68,
                    bucket: 70,
                    entry: 69,
                },
            })
        );
    }

    #[test]
    fn trim_buckets_require_the_complete_declared_sequence_and_counts() {
        let payload = b"bucket_index\0\x00bucket_xar\0\xf8\x01\xf7\x43\xfb\xe3\
            \xf7\x44\x09\x0a\x03\x00\xe2\x01\xf8\x01\xf7\x43\xfb\xe3\
            \xf7\x44\x09\x0a\x03\x00\xe2\x02\xf1\xf7\x42\xe2\x03\xe2\
            \x04\xf0\xf7\x43\xf8\x01\xf7\x43\xfb\xe3\xf7\x44\x0b\x0c\
            \x05\x00\xe2\x05\xf8\x01\xf7\x43\xfb\xe3\xf7\x44\x0d\x0e\
            \x06\x00\xe2\x06\xe0\x00next\0";
        let header = TrimTableHeader {
            declared_count: 7,
            classes: TrimTableClasses {
                table: 66,
                bucket: 67,
                entry: 68,
            },
        };

        assert_eq!(
            trim_buckets(payload, 0, payload.len(), header, TrimEntryKind::Vertex)
                .iter()
                .map(|bucket| (
                    bucket.index,
                    bucket.declared_entry_count,
                    bucket.decoded_entry_count
                ))
                .collect::<Vec<_>>(),
            (0..7)
                .zip([1, 1, 0, 0, 1, 1, 0])
                .map(|(index, count)| (index, count, count))
                .collect::<Vec<_>>()
        );
        let truncated = payload
            .windows(2)
            .position(|bytes| bytes == [0xe2, 0x06])
            .expect("last bucket index");
        assert_eq!(
            trim_buckets(payload, 0, truncated, header, TrimEntryKind::Vertex)
                .iter()
                .map(|bucket| bucket.index)
                .collect::<Vec<_>>(),
            (0..6).collect::<Vec<_>>()
        );
    }

    #[test]
    fn trim_bucket_completeness_rejects_missing_and_extra_vertex_entries() {
        let header = TrimTableHeader {
            declared_count: 1,
            classes: TrimTableClasses {
                table: 66,
                bucket: 67,
                entry: 68,
            },
        };
        let missing = b"bucket_index\0\x00bucket_xar\0\xf8\x02\xf7\x43\xfb\xe3\
            \xf7\x44\x01\x02\x03\x00\xe0";
        let buckets = trim_buckets(missing, 0, missing.len(), header, TrimEntryKind::Vertex);
        assert_eq!(buckets[0].declared_entry_count, 2);
        assert_eq!(buckets[0].decoded_entry_count, 1);
        assert!(!buckets[0].is_complete());

        let extra = b"bucket_index\0\x00bucket_xar\0\xf8\x01\xf7\x43\xfb\xe3\
            \xf7\x44\x01\x02\x03\x00\xe3\x04\x05\x06\x00\xe0";
        let buckets = trim_buckets(extra, 0, extra.len(), header, TrimEntryKind::Vertex);
        assert_eq!(buckets[0].declared_entry_count, 1);
        assert_eq!(buckets[0].decoded_entry_count, 2);
        assert!(!buckets[0].is_complete());
    }

    #[test]
    fn trim_vertex_entries_retain_variable_incident_entity_counts() {
        let counted = b"\xf8\x03\x0a\x0b\x0c\x07\x00";
        assert_eq!(
            trim_vertex_entry(counted, 0, counted.len()),
            Some((vec![10, 11, 12], 7, counted.len()))
        );
        let direct = b"\x0a\x0b\x0c\x07\x00";
        assert_eq!(
            trim_vertex_entry(direct, 0, direct.len()),
            Some((vec![10, 11, 12], 7, direct.len()))
        );
    }

    #[test]
    fn trim_entity_bucket_counts_the_named_prototype_and_complete_bodies() {
        let payload = b"bucket_index\0\x00bucket_xar\0\xf8\x02\xf7\x43\xfb\xe3\
            entry_ptr(entity_entry)\0\xe3xid\0\x00ent_mode\0\x00start_vtx\0\xf6\
            end_vtx\0\xf6center_vtx\0\xf6pers_attribs\0\x00\
            \xf4\x04\xf7\x42\xe2\xe3\
            \x09\x00\x03\x04\xf6\x00\xe0";
        let header = TrimTableHeader {
            declared_count: 1,
            classes: TrimTableClasses {
                table: 66,
                bucket: 67,
                entry: 68,
            },
        };
        let buckets = trim_buckets(payload, 0, payload.len(), header, TrimEntryKind::Entity);
        assert_eq!(buckets[0].decoded_entry_count, 2);
        assert!(buckets[0].is_complete());

        let truncated = payload.len() - 2;
        let buckets = trim_buckets(payload, 0, truncated, header, TrimEntryKind::Entity);
        assert_eq!(buckets[0].decoded_entry_count, 1);
        assert!(!buckets[0].is_complete());
    }

    #[test]
    fn decodes_var_arr_dictionary_sign_pairs() {
        let cache = scalar::ScalarCache::default();
        let cases = [
            (
                [0x97, 0xc3, 0x95, 0x81, 0x06, 0x24, 0xdc],
                3.595_499_999_999_999_5,
            ),
            (
                [0xdd, 0xc3, 0x95, 0x81, 0x06, 0x24, 0xdc],
                -3.595_499_999_999_999_5,
            ),
            (
                [0x80, 0x58, 0x23, 0x8b, 0x27, 0x55, 0x6f],
                1.334_018_271_988_806_7,
            ),
            (
                [0xc8, 0x58, 0x23, 0x8b, 0x27, 0x55, 0x6f],
                -1.334_018_271_988_806_7,
            ),
        ];
        for (bytes, expected) in cases {
            let (value, next, dimension_driven) =
                decode_variable_scalar(&bytes, 0, bytes.len(), &cache);
            assert_eq!(value, Some(expected));
            assert_eq!(next, bytes.len());
            assert!(!dimension_driven);
        }
    }

    #[test]
    fn decodes_var_arr_negative_subunit_form() {
        let bytes = [0xd5, 0xd9, 0x52, 0xa4, 0x85, 0x40, 0x39];
        let (value, next, dimension_driven) =
            decode_variable_scalar(&bytes, 0, bytes.len(), &scalar::ScalarCache::default());

        assert_eq!(value, Some(-0.395_669_107_559_015_74));
        assert_eq!(next, bytes.len());
        assert!(!dimension_driven);
    }

    #[test]
    fn var_arr_world_coordinate_2d_is_positive() {
        let bytes = [0x2d, 0x34, 0x43, 0xf5, 0x12, 0xe8, 0x00, 0x45];
        let (value, next, dimension_driven) = decode_section_coordinate_scalar(
            &bytes,
            0,
            bytes.len(),
            &scalar::ScalarCache::default(),
        );

        assert_eq!(value, Some(20.265_458_280_220_873));
        assert_eq!(next, bytes.len());
        assert!(!dimension_driven);
        assert_eq!(
            decode_variable_scalar(&bytes, 0, bytes.len(), &scalar::ScalarCache::default()).0,
            Some(-20.265_458_280_220_873)
        );
    }

    #[test]
    fn saved_section_world_coordinate_2d_is_positive() {
        let bytes = [0x2d, 0x52, 0xa4, 0x0d, 0xb4, 0x1f, 0x70, 0xed];

        assert_eq!(
            saved_section_scalar(&bytes, 0, bytes.len(), &scalar::ScalarCache::default()),
            (Some(74.563_336_401_657_31), bytes.len())
        );
    }

    #[test]
    fn decodes_var_arr_positional_dict_lattice() {
        for (bytes, head) in [
            ([0x64, 1, 2, 3, 4, 5, 6], [0x3f, 0xd9]),
            ([0x69, 1, 2, 3, 4, 5, 6], [0x3f, 0xde]),
            ([0x9c, 1, 2, 3, 4, 5, 6], [0x40, 0x11]),
            ([0x9d, 1, 2, 3, 4, 5, 6], [0x40, 0x12]),
            ([0x9f, 1, 2, 3, 4, 5, 6], [0x40, 0x14]),
            ([0xa0, 1, 2, 3, 4, 5, 6], [0x40, 0x15]),
            ([0xad, 1, 2, 3, 4, 5, 6], [0x3f, 0xd9]),
            ([0xb3, 1, 2, 3, 4, 5, 6], [0xbf, 0xe0]),
            ([0xcb, 1, 2, 3, 4, 5, 6], [0xbf, 0xf8]),
            ([0xcc, 1, 2, 3, 4, 5, 6], [0xbf, 0xf9]),
            ([0xd0, 1, 2, 3, 4, 5, 6], [0xbf, 0xfe]),
            ([0xd2, 1, 2, 3, 4, 5, 6], [0xc0, 0x00]),
            ([0xd6, 1, 2, 3, 4, 5, 6], [0xc0, 0x04]),
        ] {
            let (value, next, dimension_driven) =
                decode_variable_scalar(&bytes, 0, bytes.len(), &scalar::ScalarCache::default());
            assert_eq!(
                value,
                Some(f64::from_be_bytes([
                    head[0], head[1], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6],
                ]))
            );
            assert_eq!(next, bytes.len());
            assert!(!dimension_driven);
        }
        let bytes = [0x28, 1, 2, 3, 4, 5, 6, 7];
        assert_eq!(
            decode_variable_scalar(&bytes, 0, bytes.len(), &scalar::ScalarCache::default()),
            (
                Some(f64::from_be_bytes([0x3f, 1, 2, 3, 4, 5, 6, 7])),
                bytes.len(),
                false,
            )
        );
    }

    #[test]
    fn saved_line_accepts_bare_entity_reference_before_coordinates() {
        let payload = b"\xe0\0entity(line)\0\x05\xe2\xf7\x2a\
            \x2f\x20\0\x2f\x20\0\x2f\x20\0\
            \x2f\x20\0\x2f\x20\0\x2f\x20\0\xf1\xf7\x2b\xe3";
        let entities =
            saved_line_entities(payload, 0, payload.len(), &scalar::ScalarCache::default());

        assert_eq!(entities.len(), 1);
        let FeatureSavedEntity::Line(line) = &entities[0] else {
            panic!("expected saved line");
        };
        assert_eq!(line.entity_id, 5);
        assert_eq!(line.references, [42, 43]);
        assert_eq!(line.endpoints, [[Some(8.0); 3]; 2]);
    }

    #[test]
    fn saved_line_expands_compact_basis_triple() {
        let payload = b"\xe0\0entity(line)\0\x05\xe2\x18\xe5\x2f\x20\0\x2f\x20\0\x2f\x20\0\xe3";
        let entities =
            saved_line_entities(payload, 0, payload.len(), &scalar::ScalarCache::default());
        let FeatureSavedEntity::Line(line) = &entities[0] else {
            panic!("expected saved line");
        };
        assert_eq!(
            line.endpoints,
            [
                [Some(0.0), Some(1.0), Some(0.0)],
                [Some(8.0), Some(8.0), Some(8.0)]
            ]
        );
    }

    #[test]
    fn saved_line_replay_continues_after_point_prototype() {
        let scalar_triple = b"\x2f\x20\0\x2f\x20\0\x2f\x20\0";
        let mut payload = b"\xe0\0entity(line)\0\x05\xe2".to_vec();
        payload.extend_from_slice(scalar_triple);
        payload.extend_from_slice(scalar_triple);
        payload.push(0xe3);
        payload.extend_from_slice(b"\xe0\0entity(point)\0\xe0\x01id\0\x04\xf1\xf7\x2a\xe3\x06\xe2");
        payload.extend_from_slice(scalar_triple);
        payload.extend_from_slice(scalar_triple);
        payload.extend_from_slice(b"\xe0\0entity(arc)\0");

        let entities =
            saved_line_entities(&payload, 0, payload.len(), &scalar::ScalarCache::default());

        assert_eq!(entities.len(), 2);
        assert_eq!(
            entities
                .iter()
                .filter_map(|entity| match entity {
                    FeatureSavedEntity::Line(line) => Some(line.entity_id),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            [5, 6]
        );
    }

    #[test]
    fn saved_line_accepts_named_record_boundary() {
        let payload = b"\xe0\0entity(line)\0\x03\xe2\xf1\xf7\x80\xc4\
            \x48\x20\0\x46\x15\xff\xff\xff\xff\xff\x8f\x18\
            \x48\x1e\0\x46\x15\xff\xff\xff\xff\xff\x8f\x18\x8a\x01\x02\x03\x04\x05\x0f\
            \xe0\0entity(point)\0\xf1\xf7\x2a\xe3\xe0\0entity(arc)\0";
        let entities =
            saved_line_entities(payload, 0, payload.len(), &scalar::ScalarCache::default());

        assert_eq!(entities.len(), 1);
        let FeatureSavedEntity::Line(line) = &entities[0] else {
            panic!("expected saved line");
        };
        assert_eq!(line.entity_id, 3);
        assert_eq!(line.references, [196]);
    }

    #[test]
    fn saved_line_retains_its_identity_and_coordinate_prefix() {
        let payload = b"\xe0\0entity(line)\0\x07\xe2\x0f\x0f\x0f\
            \xe0\0entity(arc)\0";

        let entities =
            saved_line_entities(payload, 0, payload.len(), &scalar::ScalarCache::default());

        let [FeatureSavedEntity::Line(line)] = entities.as_slice() else {
            panic!("saved line");
        };
        assert_eq!(line.entity_id, 7);
        assert_eq!(
            line.endpoints,
            [[Some(0.0), Some(0.0), Some(0.0)], [None; 3]]
        );
    }

    #[test]
    fn saved_section_retains_an_empty_named_table() {
        let payload = b"\xe0\0p_saved_result\0\xe0\x02local_sys\0";

        let section = saved_section(
            payload,
            0,
            payload.len(),
            &scalar::ScalarCache::default(),
            None,
            None,
        )
        .expect("saved section header");

        assert_eq!(section.offset, 0);
        assert!(section.entities.is_empty());
    }

    #[test]
    fn saved_section_41_form_occupies_eight_bytes() {
        let bytes = [0x41, 0xfd, 0x6b, 0xf1, 0xa1, 0xc2, 0x1f, 0xf0];
        let (value, next) =
            saved_section_scalar(&bytes, 0, bytes.len(), &scalar::ScalarCache::default());
        assert_eq!(next, bytes.len());
        assert_eq!(
            value,
            Some(f64::from_be_bytes([
                0x3f, 0xfd, 0x6b, 0xf1, 0xa1, 0xc2, 0x1f, 0xf0
            ]))
        );
    }

    #[test]
    fn saved_section_zero_does_not_consume_named_record_opener() {
        let mut section = Vec::new();
        for index in 0_u16..=224 {
            section.extend_from_slice(&[0x46, 0x08, (index >> 8) as u8, index as u8, 0, 0, 0, 0]);
        }
        let cache = scalar::ScalarCache::from_section(&section);

        assert_eq!(
            saved_section_scalar(&[0x18, 0xe0], 0, 2, &cache),
            (Some(0.0), 1)
        );
    }

    #[test]
    fn saved_section_consecutive_zero_slots_remain_distinct() {
        let cache = scalar::ScalarCache::default();
        let bytes = [0x18, 0x18, 0x81, 0, 0, 0, 0, 0, 0];
        assert_eq!(
            saved_section_scalar(&bytes, 0, bytes.len(), &cache),
            (Some(0.0), 1)
        );
        assert_eq!(
            saved_section_scalar(&bytes, 1, bytes.len(), &cache),
            (Some(0.0), 2)
        );
    }

    #[test]
    fn saved_section_dd_form_supplies_ieee_high_bytes() {
        let bytes = [0xdd, 0xe6, 0x8a, 0x84, 0x79, 0xd0, 0x62];
        assert_eq!(
            saved_section_scalar(&bytes, 0, bytes.len(), &scalar::ScalarCache::default()),
            (
                Some(f64::from_be_bytes([
                    0x40, 0x0c, 0xe6, 0x8a, 0x84, 0x79, 0xd0, 0x62,
                ])),
                7,
            )
        );
    }

    #[test]
    fn saved_section_negative_dict_forms_supply_ieee_high_bytes() {
        for (bytes, head) in [
            ([0xb3, 1, 2, 3, 4, 5, 6], [0xbf, 0xe0]),
            ([0xcb, 1, 2, 3, 4, 5, 6], [0xbf, 0xf8]),
            ([0xd6, 1, 2, 3, 4, 5, 6], [0xc0, 0x04]),
        ] {
            assert_eq!(
                saved_section_scalar(&bytes, 0, bytes.len(), &scalar::ScalarCache::default()),
                (
                    Some(f64::from_be_bytes([
                        head[0], head[1], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5],
                        bytes[6],
                    ])),
                    7,
                )
            );
        }
    }

    #[test]
    fn saved_arc_negative_dict_forms_supply_ieee_high_bytes() {
        for (bytes, head) in [
            ([0x9b, 1, 2, 3, 4, 5, 6], [0x40, 0x10]),
            ([0x9c, 1, 2, 3, 4, 5, 6], [0x40, 0x11]),
            ([0x9d, 1, 2, 3, 4, 5, 6], [0x40, 0x12]),
            ([0x9e, 1, 2, 3, 4, 5, 6], [0x40, 0x13]),
            ([0x9f, 1, 2, 3, 4, 5, 6], [0x40, 0x14]),
            ([0xa0, 1, 2, 3, 4, 5, 6], [0x40, 0x15]),
            ([0x5e, 1, 2, 3, 4, 5, 6], [0x3f, 0xd3]),
            ([0x60, 1, 2, 3, 4, 5, 6], [0x3f, 0xd5]),
            ([0x64, 1, 2, 3, 4, 5, 6], [0x3f, 0xd9]),
            ([0xad, 1, 2, 3, 4, 5, 6], [0x3f, 0xd9]),
            ([0xcc, 1, 2, 3, 4, 5, 6], [0xbf, 0xf9]),
            ([0xd0, 1, 2, 3, 4, 5, 6], [0xbf, 0xfe]),
            ([0xd2, 1, 2, 3, 4, 5, 6], [0xc0, 0x00]),
            ([0xd5, 1, 2, 3, 4, 5, 6], [0xc0, 0x03]),
            ([0xde, 1, 2, 3, 4, 5, 6], [0xc0, 0x10]),
            ([0xdf, 1, 2, 3, 4, 5, 6], [0xc0, 0x11]),
        ] {
            let expected = f64::from_be_bytes([
                head[0], head[1], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6],
            ]);
            assert_eq!(
                saved_arc_scalar(&bytes, 0, bytes.len(), &scalar::ScalarCache::default()),
                (Some(expected), 7)
            );
        }
        let d5 = [0xd5, 1, 2, 3, 4, 5, 6];
        assert_eq!(
            saved_section_scalar(&d5, 0, d5.len(), &scalar::ScalarCache::default()),
            (Some(f64::from_be_bytes([0xbf, 1, 2, 3, 4, 5, 6, 0])), 7)
        );
    }

    #[test]
    fn saved_arc_28_form_supplies_ieee_high_byte() {
        let bytes = [0x28, 1, 2, 3, 4, 5, 6, 7];
        assert_eq!(
            saved_arc_scalar(&bytes, 0, bytes.len(), &scalar::ScalarCache::default()),
            (Some(f64::from_be_bytes([0x3f, 1, 2, 3, 4, 5, 6, 7])), 8)
        );
    }

    #[test]
    fn saved_arc_zero_does_not_consume_arc_scalar_opener() {
        let bytes = [0x18, 0x5e, 1, 2, 3, 4, 5, 6];
        let cache = scalar::ScalarCache::default();
        assert_eq!(
            saved_arc_scalar(&bytes, 0, bytes.len(), &cache),
            (Some(0.0), 1)
        );
        assert_eq!(
            saved_arc_scalar(&bytes, 1, bytes.len(), &cache),
            (Some(f64::from_be_bytes([0x3f, 0xd3, 1, 2, 3, 4, 5, 6])), 8)
        );
    }

    #[test]
    fn saved_circular_entities_retain_ids_and_independent_fields() {
        let payload = b"\xe0\x00entity(arc)\0\
            \xe0\x01id\0\x07\xe0\x02center\0\x0f\x0f\x0f\
            \xe0\x00entity(circle)\0\
            \xe0\x01id\0\x08\xe0\x02radius\0\x0f";

        let entities = saved_circular_entities(
            payload,
            0,
            payload.len(),
            &scalar::ScalarCache::default(),
            None,
            None,
        );

        let [FeatureSavedEntity::Arc(arc), FeatureSavedEntity::Circle(circle)] =
            entities.as_slice()
        else {
            panic!("saved circular entities");
        };
        assert_eq!(arc.entity_id, 7);
        assert_eq!(arc.center, [Some(0.0); 3]);
        assert_eq!(arc.radius, None);
        assert_eq!(arc.endpoints, [[None; 3]; 2]);
        assert_eq!(arc.parameters, [None; 2]);
        assert_eq!(circle.entity_id, 8);
        assert_eq!(circle.center, [None; 3]);
        assert_eq!(circle.radius, Some(0.0));
    }

    #[test]
    fn saved_arc_replay_uses_order_table_row_boundaries() {
        let mut payload = vec![0xe3, 7, 0xe2];
        payload.extend([0x0f; 12]);
        payload.push(0xe3);
        let order = FeatureOrderTable {
            declared_count: 1,
            has_prototype: false,
            entity_ref: None,
            rows: vec![FeatureOrderRow {
                external_id: 42,
                internal_id: 7,
                bitmask: 0,
                offset: 0,
            }],
            offset: 0,
        };
        let segments = FeatureSegmentTable {
            declared_count: 1,
            entity_ref: None,
            rows: vec![FeatureSegment {
                kind: FeatureSegmentKind::Arc,
                directions: [None; 3],
                point_ids: [1, 2],
                center_id: Some(3),
                arc_orientation: Some(0),
                vertical_horizontal: None,
                radius_ref: None,
                radius2_ref: None,
                external_id: 42,
                offset: 0,
            }],
            opaque_rows: Vec::new(),
            offset: 0,
        };

        let entities = saved_positional_generated_entities(
            &payload,
            0,
            payload.len(),
            &scalar::ScalarCache::default(),
            Some(&order),
            Some(&segments),
        );

        assert_eq!(entities.len(), 1);
        let FeatureSavedEntity::Arc(arc) = &entities[0] else {
            panic!("expected saved arc");
        };
        assert_eq!(arc.entity_id, 7);
        assert_eq!(arc.center, [Some(0.0); 3]);
        assert_eq!(arc.radius, Some(0.0));
        let section = positional_saved_section(
            &payload,
            0,
            payload.len(),
            &scalar::ScalarCache::default(),
            Some(&order),
            Some(&segments),
        )
        .expect("positional saved section");
        assert_eq!(section.entities.len(), 1);
        assert_eq!(section.offset, 1);
    }

    #[test]
    fn saved_arc_replay_retains_a_structurally_terminated_scalar_prefix() {
        let mut payload = vec![0xe3, 7, 0xe2];
        payload.extend([0x0f; 6]);
        payload.push(0xe3);
        let order = FeatureOrderTable {
            declared_count: 1,
            has_prototype: false,
            entity_ref: None,
            rows: vec![FeatureOrderRow {
                external_id: 42,
                internal_id: 7,
                bitmask: 0,
                offset: 0,
            }],
            offset: 0,
        };
        let segments = FeatureSegmentTable {
            declared_count: 1,
            entity_ref: None,
            rows: vec![FeatureSegment {
                kind: FeatureSegmentKind::Arc,
                directions: [None; 3],
                point_ids: [1, 2],
                center_id: Some(3),
                arc_orientation: Some(0),
                vertical_horizontal: None,
                radius_ref: None,
                radius2_ref: None,
                external_id: 42,
                offset: 0,
            }],
            opaque_rows: Vec::new(),
            offset: 0,
        };

        let entities = saved_positional_generated_entities(
            &payload,
            0,
            payload.len(),
            &scalar::ScalarCache::default(),
            Some(&order),
            Some(&segments),
        );

        let [FeatureSavedEntity::Arc(arc)] = entities.as_slice() else {
            panic!("expected saved arc");
        };
        assert_eq!(arc.entity_id, 7);
        assert_eq!(arc.center, [Some(0.0); 3]);
        assert_eq!(arc.radius, Some(0.0));
        assert_eq!(arc.endpoints[0], [Some(0.0), Some(0.0), None]);
        assert_eq!(arc.endpoints[1], [None; 3]);
        assert_eq!(arc.parameters, [None; 2]);
    }

    #[test]
    fn saved_generated_line_requires_its_orientation_invariant() {
        let payload = [0xe3, 8, 0xe2, 0x0f, 0x0f, 0x0f, 0xe4, 0x0f, 0x0f, 0xe3];
        let order = FeatureOrderTable {
            declared_count: 1,
            has_prototype: false,
            entity_ref: None,
            rows: vec![FeatureOrderRow {
                external_id: 43,
                internal_id: 8,
                bitmask: 0,
                offset: 0,
            }],
            offset: 0,
        };
        let segments = FeatureSegmentTable {
            declared_count: 1,
            entity_ref: None,
            rows: vec![FeatureSegment {
                kind: FeatureSegmentKind::Line,
                directions: [None; 3],
                point_ids: [1, 2],
                center_id: None,
                arc_orientation: Some(0),
                vertical_horizontal: Some(1),
                radius_ref: None,
                radius2_ref: None,
                external_id: 43,
                offset: 0,
            }],
            opaque_rows: Vec::new(),
            offset: 0,
        };

        let entities = saved_positional_generated_entities(
            &payload,
            0,
            payload.len(),
            &scalar::ScalarCache::default(),
            Some(&order),
            Some(&segments),
        );

        assert_eq!(entities.len(), 1);
        let FeatureSavedEntity::Line(line) = &entities[0] else {
            panic!("expected saved line");
        };
        assert_eq!(line.entity_id, 8);
        assert_eq!(line.endpoints[0], [Some(0.0); 3]);
        assert_eq!(line.endpoints[1], [Some(1.0), Some(0.0), Some(0.0)]);
    }

    #[test]
    fn decodes_mdlstatus_recipe_discriminators_within_their_records() {
        let payload = b"\xe3icon\0protextrude\0Protrusion id 40\0\xe2\xe3\
            icon\0protrevolve\0Revolve id 41\0\xe2\xe3\
            icon\0cutextrude\0Cut id 42\0\xe2\xe3\
            icon\0cutrevolve\0Cut id 43\0\xe2\xe3Datum Plane id 44\0\xe3K\xc3\xb6rper ID 45\0";
        let operations = operations(payload);
        assert_eq!(operations.len(), 6);
        assert_eq!(operations[0].recipe, Some(FeatureRecipe::ProtrudeExtrude));
        assert_eq!(operations[1].recipe, Some(FeatureRecipe::ProtrudeRevolve));
        assert_eq!(operations[2].recipe, Some(FeatureRecipe::CutExtrude));
        assert_eq!(operations[3].recipe, Some(FeatureRecipe::CutRevolve));
        assert_eq!(operations[4].recipe, None);
        assert_eq!(operations[5].kind, "Körper");
        assert_eq!(operations[5].feature_id, 45);
    }

    #[test]
    fn binds_depdb_recipe_records_to_compact_feature_ids() {
        let payload = b"\xe3K\xc3\xb6rper ID 247\0\xe3\
            \xf7\x3b\x80\xf7\x83\x95\xf6\x20Drehen 1\0\xf6\0protrevolve\0\
            \xe3Body ID 8053\0\xe3\
            \xf7\x50\x9f\x75\x83\x95\xf6\x9f\x73Profile 1\0\xf6\0protextrude\0";

        let operations = operations(payload);
        assert_eq!(operations.len(), 2);
        assert_eq!(operations[0].feature_id, 247);
        assert_eq!(operations[0].recipe, Some(FeatureRecipe::ProtrudeRevolve));
        assert_eq!(operations[0].root_schema_class, Some(917));
        assert_eq!(operations[0].parent_feature_id, Some(32));
        assert_eq!(operations[1].feature_id, 8053);
        assert_eq!(operations[1].recipe, Some(FeatureRecipe::ProtrudeExtrude));
        assert_eq!(operations[1].root_schema_class, Some(917));
        assert_eq!(operations[1].parent_feature_id, Some(8051));
    }

    #[test]
    fn preserves_competing_depdb_recipe_bindings() {
        let payload = b"\xf7\x50\x9f\x75\x83\x95\xf6\x9f\x73Profile 1\0\xf6\0protextrude\0\
            \xf7\x50\x9f\x75\x83\x94\xf6\x9f\x73Profile 2\0\xf6\0cutextrude\0";

        let states = operation_states(payload);
        assert_eq!(states.len(), 2);
        assert_eq!(states[0].feature_id, 8053);
        assert_eq!(states[0].recipe, Some(FeatureRecipe::ProtrudeExtrude));
        assert_eq!(states[0].root_schema_class, Some(917));
        assert_eq!(states[1].feature_id, 8053);
        assert_eq!(states[1].recipe, Some(FeatureRecipe::CutExtrude));
        assert_eq!(states[1].root_schema_class, Some(916));

        let current = operations(payload);
        assert_eq!(current.len(), 1);
        assert_eq!(current[0], states[1]);

        let repeated = b"\xf7\x50\x9f\x75\x83\x95\xf6\x9f\x73Profile 1\0\xf6\0protextrude\0\
            \xf7\x50\x9f\x75\x83\x95\xf6\x9f\x73Profile 2\0\xf6\0protextrude\0";
        let repeated_states = operation_states(repeated);
        assert_eq!(repeated_states.len(), 2);
        assert_eq!(repeated_states[0].recipe, repeated_states[1].recipe);
        assert_ne!(repeated_states[0].offset, repeated_states[1].offset);
    }

    #[test]
    fn promotes_depdb_recipe_without_operation_display_name() {
        let payload = b"\xe3\
            \xf7\x50\x9f\x75\x83\x95\xf6\x9f\x73Profile 1\0\xf6\0protextrude\0";

        let operations = operations(payload);
        assert_eq!(operations.len(), 1);
        assert_eq!(operations[0].feature_id, 8053);
        assert_eq!(operations[0].kind, "Extrude");
        assert_eq!(operations[0].recipe, Some(FeatureRecipe::ProtrudeExtrude));
        assert_eq!(operations[0].root_schema_class, Some(917));
        assert_eq!(operations[0].parent_feature_id, Some(8051));
        assert_eq!(operations[0].offset, 1);
    }

    #[test]
    fn decodes_count_bounded_saved_spline_interpolation_points() {
        let payload = b"\xe0\x00save_entity_ptr(spline)\0\xe3\
            \xe0\x01id\0\x07\
            \xe0\x02i_pnts\0\xf9\x02\x03\
            \xe4\x0f\x0d\x0f\xe4\x0f\
            \xe0\x02end_tangts\0\xf9\x02\x03\
            \xe4\x0f\x0f\xe4\x0f\x0f\
            \xe0\x02params\0\xf8\x02\x0f\xe4\
            \xe0\x01tan_cond\0\x00";

        let entities =
            saved_spline_entities(payload, 0, payload.len(), &scalar::ScalarCache::default());
        let [FeatureSavedEntity::Spline(spline)] = entities.as_slice() else {
            panic!("saved spline");
        };
        assert_eq!(spline.entity_id, Some(7));
        assert_eq!(spline.declared_point_count, Some(2));
        assert_eq!(
            spline.interpolation_points,
            [[1.0, 0.0, -1.0], [0.0, 1.0, 0.0]]
        );
        assert_eq!(
            spline.endpoint_tangents,
            Some([[1.0, 0.0, 0.0], [1.0, 0.0, 0.0]])
        );
        assert_eq!(spline.parameters, Some(vec![0.0, 1.0]));
    }

    #[test]
    fn decodes_compact_saved_spline_point_count() {
        let mut payload = b"\xe0\x00save_entity_ptr(spline)\0\xe3\
            \xe0\x01id\0\x07\
            \xe0\x02i_pnts\0\xf9\x80\x88\x03"
            .to_vec();
        payload.extend(std::iter::repeat_n(0x0f, 136 * 3));

        let entities =
            saved_spline_entities(&payload, 0, payload.len(), &scalar::ScalarCache::default());
        let [FeatureSavedEntity::Spline(spline)] = entities.as_slice() else {
            panic!("saved spline");
        };
        assert_eq!(spline.declared_point_count, Some(136));
        assert_eq!(spline.interpolation_points.len(), 136);
        assert!(spline
            .interpolation_points
            .iter()
            .all(|point| *point == [0.0; 3]));
    }

    #[test]
    fn saved_spline_retains_its_declared_count_and_complete_point_prefix() {
        let payload = b"\xe0\x00save_entity_ptr(spline)\0\xe3\
            \xe0\x01id\0\x07\
            \xe0\x02i_pnts\0\xf9\x02\x03\
            \x0f\x0f\x0f\xe0\x01tan_cond\0\x00";

        let entities =
            saved_spline_entities(payload, 0, payload.len(), &scalar::ScalarCache::default());
        let [FeatureSavedEntity::Spline(spline)] = entities.as_slice() else {
            panic!("saved spline");
        };

        assert_eq!(spline.entity_id, Some(7));
        assert_eq!(spline.declared_point_count, Some(2));
        assert_eq!(spline.interpolation_points, [[0.0; 3]]);
        assert_eq!(spline.endpoint_tangents, None);
        assert_eq!(spline.parameters, None);
    }

    #[test]
    fn saved_spline_retains_its_identity_without_a_point_table() {
        let payload = b"\xe0\x00save_entity_ptr(spline)\0\xe3\xe0\x01id\0\x07";

        let entities =
            saved_spline_entities(payload, 0, payload.len(), &scalar::ScalarCache::default());
        let [FeatureSavedEntity::Spline(spline)] = entities.as_slice() else {
            panic!("saved spline");
        };

        assert_eq!(spline.entity_id, Some(7));
        assert_eq!(spline.declared_point_count, None);
        assert!(spline.interpolation_points.is_empty());
    }

    #[test]
    fn decodes_compact_feature_scalar_array_extents() {
        let mut payload = vec![psb::token::SCALAR_BODY, 0x80, 0x88, 0x03];
        payload.extend(std::iter::repeat_n(0x0f, 136 * 3));

        let FeatureFieldValue::ScalarArray {
            dimensions,
            count,
            body,
            decoded_values,
        } = field_value(&payload)
        else {
            panic!("scalar array");
        };
        assert_eq!(dimensions, 136);
        assert_eq!(count, 3);
        assert_eq!(body.len(), 408);
        assert_eq!(decoded_values, Some(vec![0.0; 408]));
    }

    #[test]
    fn decodes_saved_spline_chord_parameter_lane() {
        let body = [
            0x18, 0x6d, 0x31, 0xd2, 0x2a, 0x7f, 0x68, 0x39, 0x85, 0x06, 0x5f, 0x25, 0x83, 0xf4,
            0x6c, 0x93, 0xd8, 0xd4, 0xfb, 0x45, 0xbc, 0x38, 0x9e, 0x51, 0xef, 0x1e, 0x96, 0xe2,
            0x6c, 0x2d, 0x1a, 0xfc, 0x59, 0x51, 0xbd, 0x0a, 0x38,
        ];
        let cache = scalar::ScalarCache::default();
        let expected = [
            0.0,
            0.568_581_660_273_827_7,
            1.626_555_582_565_994_3,
            3.105_874_980_035_448_4,
            4.830_013_730_963_952,
            6.746_434_476_054_269,
        ];
        let mut cursor = 0;
        for expected in expected {
            let (value, next) = saved_spline_parameter(&body, cursor, &cache).expect("parameter");
            assert_eq!(value, expected);
            cursor = next;
        }
        assert_eq!(cursor, body.len());
    }

    #[test]
    fn decodes_zero_offset_positional_placement_instruction() {
        let payload = b"place_instruction_ptrs\0\xf8\x03\xf7\x0b\xfb\xe3\
            \xf1\xf7\x0b\xe3\xc0\x4e\x9f\x18\xf6\xf6\x02\xf6\x00\x00\x00\xe6";
        let rows = placement_instruction_rows(payload, 1000);
        let [row] = rows.as_slice() else {
            panic!("placement row");
        };
        assert_eq!(row.kind, 20_127);
        assert!(row.zero_offset);
        assert_eq!(row.dimension_id, None);
        assert_eq!(row.reference_id, None);
        assert_eq!(row.geometry1_id, Some(2));
        assert_eq!(row.geometry2_id, None);
        assert_eq!([row.member1, row.member2], [0, 0]);
        assert_eq!(row.offset, 1029);
    }

    #[test]
    fn model_reference_entry_joins_feature_name_to_feature_id() {
        let payload = b"\0\xf7\x71\x2a\x05\x29Datum Plane id 41\0\x2a\x2a\x10\0\
            \xf7\x71\x30\x05\x2fBroken\0\x30\x31";

        assert_eq!(
            reference_names(payload),
            [FeatureReferenceName {
                feature_id: 41,
                name: "Datum Plane id 41".to_string(),
                name_bytes: b"Datum Plane id 41".to_vec(),
                own_reference_id: 42,
                reference_type: 5,
                offset: 1,
            }]
        );
    }
}
