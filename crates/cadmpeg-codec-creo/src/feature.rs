// SPDX-License-Identifier: Apache-2.0
//! Structural `AllFeatur` feature-to-generated-entity bindings.
//!
//! A mixed generated-entity table is `f8 <count> f7 1d fb e3`, followed by
//! exactly `<count>` compact entity identifiers, each terminated by `e3`.
//! `f7 1e` may prefix an entry. The table belongs to an `AllFeatur` row only
//! when its byte offset is bounded by that row's known feature-id header.

use std::collections::{BTreeMap, BTreeSet};

use crate::psb;
use crate::scalar;

/// Procedural recipe discriminator stored in a feature-state record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureRecipeKind {
    /// Linear section sweep named `protextrude`.
    Extrude,
    /// Rotational section sweep named `protrevolve`.
    Revolve,
}

/// Feature-operation family named by a feature-state record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureOperation {
    /// Numeric feature identifier following `id` in the stored name.
    pub feature_id: u32,
    /// Stored operation-family name.
    pub kind: String,
    /// Whether `kind` came from a stored `<Kind> id <N>` display name.
    pub display_name_stored: bool,
    /// Optional one-byte state prefix immediately preceding the family name.
    pub status_prefix: Option<u8>,
    /// Procedural recipe name stored in the same current-state record.
    pub recipe: Option<FeatureRecipeKind>,
    /// Root feature-definition schema class from a DEPDB recipe prefix.
    pub root_schema_class: Option<u32>,
    /// Previous or parent feature identifier from a DEPDB recipe prefix.
    pub parent_feature_id: Option<u32>,
    /// Byte offset of the operation name in the original stream.
    pub offset: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FeatureRecipeBinding {
    kind: FeatureRecipeKind,
    root_schema_class: u32,
    parent_feature_id: u32,
    offset: usize,
}

fn recipe_bindings(payload: &[u8]) -> BTreeMap<u32, FeatureRecipeBinding> {
    const RECIPES: &[(&[u8], FeatureRecipeKind)] = &[
        (b"protextrude\0", FeatureRecipeKind::Extrude),
        (b"protrevolve\0", FeatureRecipeKind::Revolve),
    ];
    let mut bindings = BTreeMap::new();
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
        if let Some((_, kind)) = RECIPES
            .iter()
            .find(|(name, _)| payload.get(recipe_start..recipe_start + name.len()) == Some(*name))
        {
            bindings.insert(
                feature_id,
                FeatureRecipeBinding {
                    kind: *kind,
                    root_schema_class: schema_class,
                    parent_feature_id,
                    offset: marker,
                },
            );
        }
    }
    bindings
}

/// Decode NUL-terminated `<Kind> id <N>` operation names and their bounded
/// procedural-recipe records from one feature-state namespace.
pub fn operations(payload: &[u8]) -> Vec<FeatureOperation> {
    const SEPARATORS: &[&[u8]] = &[b" id ", b" ID "];
    let family_byte = |byte: u8| {
        byte.is_ascii_alphanumeric()
            || byte >= 0x80
            || matches!(byte, b' ' | b'_' | b'-' | b'/' | b'(' | b')')
    };
    let bound_recipes = recipe_bindings(payload);
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
        let family = &payload[offset..separator];
        if family.is_empty() || family.first() == Some(&b' ') || family.last() == Some(&b' ') {
            continue;
        }
        let (status_prefix, family) = match family {
            [prefix @ (b'x' | b'y'), first, ..] if first.is_ascii_uppercase() => {
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
        let bound_recipe = bound_recipes.get(&feature_id).copied();
        let recipe = bound_recipe.map(|binding| binding.kind).or_else(|| {
            if record
                .windows(b"protextrude\0".len())
                .any(|window| window == b"protextrude\0")
            {
                Some(FeatureRecipeKind::Extrude)
            } else if record
                .windows(b"protrevolve\0".len())
                .any(|window| window == b"protrevolve\0")
            {
                Some(FeatureRecipeKind::Revolve)
            } else {
                None
            }
        });
        result.push(FeatureOperation {
            feature_id,
            kind: String::from_utf8_lossy(family).into_owned(),
            display_name_stored: true,
            status_prefix,
            recipe,
            root_schema_class: bound_recipe.map(|binding| binding.root_schema_class),
            parent_feature_id: bound_recipe.map(|binding| binding.parent_feature_id),
            offset,
        });
    }
    for (feature_id, binding) in &bound_recipes {
        if result
            .iter()
            .any(|operation| operation.feature_id == *feature_id)
        {
            continue;
        }
        result.push(FeatureOperation {
            feature_id: *feature_id,
            kind: match binding.kind {
                FeatureRecipeKind::Extrude => "Extrude",
                FeatureRecipeKind::Revolve => "Revolve",
            }
            .to_string(),
            display_name_stored: false,
            status_prefix: None,
            recipe: Some(binding.kind),
            root_schema_class: Some(binding.root_schema_class),
            parent_feature_id: Some(binding.parent_feature_id),
            offset: binding.offset,
        });
    }
    result.sort_by_key(|operation| operation.offset);
    let mut current = result
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
    /// Byte offset immediately after the record's structural `e3` close.
    pub end_offset: usize,
}

/// One byte-bounded positional `AllFeatur` row for a known geometry owner.
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
        dimensions: u8,
        /// Number of scalar tuples from the wrapper.
        count: u8,
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

/// Which named direction lane supplied a recipe byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectionLane {
    /// `direction`.
    Primary,
    /// `direction2`.
    Secondary,
}

/// Interpretation permitted by the direction byte itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectionValue {
    /// Defined boolean side flag (`00` or `01`).
    SideFlag(bool),
    /// Any other raw byte; no side semantics are assigned.
    Raw(u8),
}

/// One named recipe direction byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureDirectionByte {
    /// Owning feature identifier.
    pub feature_id: u32,
    /// Primary or secondary direction lane.
    pub lane: DirectionLane,
    /// Byte interpretation.
    pub value: DirectionValue,
    /// Byte offset of the named field header in the original stream.
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
    /// Coordinate class: `1` is section `u`, `2` is section `v`.
    pub variable_type: u32,
    /// Point or solver-variable key.
    pub key: u32,
    /// Solved value when the scalar token is defined inline.
    pub value: Option<f64>,
    /// Pre-solve estimate when defined inline.
    pub guess: Option<f64>,
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

/// Defining-sketch segment table from one feature definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureSegmentTable {
    /// Count declared by the `f8` opener.
    pub declared_count: u32,
    /// Entity-table reference following the opener.
    pub entity_ref: Option<u32>,
    /// Fully aligned line and arc rows.
    pub rows: Vec<FeatureSegment>,
    /// Byte offset of the `segtab_ptr` label in the original stream.
    pub offset: usize,
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

/// Solved/trimmed entity graph for one feature definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureTrimEntityTable {
    /// Complete positional rows in stored order.
    pub rows: Vec<FeatureTrimEntity>,
    /// Sorted external IDs present in the trimmed profile.
    pub solved_external_ids: Vec<u32>,
    /// Byte offset of the `ent_tab` label in the original stream.
    pub offset: usize,
}

/// One solved trim vertex and the two trimmed entities incident to it.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureTrimVertex {
    /// Vertex identifier shared with `ent_tab` endpoint and center fields.
    pub vertex_id: u32,
    /// Distinct `ent_tab` external entity identifiers meeting at the vertex.
    pub entities: [u32; 2],
    /// Solved section-frame coordinates for a nonparallel line-line junction.
    pub section_coordinates: Option<[f64; 2]>,
    /// Byte offset of the positional triple in the original stream.
    pub offset: usize,
}

/// Solved trim-vertex adjacency table for one feature definition.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureTrimVertexTable {
    /// Complete validated vertex rows in stored order.
    pub rows: Vec<FeatureTrimVertex>,
    /// Byte offset of the `vert_tab` label in the original stream.
    pub offset: usize,
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
    /// Entity-table class reference following the opener.
    pub entity_ref: Option<u32>,
    /// Complete positional triples in stored order.
    pub rows: Vec<FeatureOrderRow>,
    /// Byte offset of the `order_table` label in the original stream.
    pub offset: usize,
}

impl FeatureOrderTable {
    /// Resolve a generated-entity position to its section entity identifier.
    pub fn external_id(&self, internal_id: u32) -> Option<u32> {
        self.rows
            .iter()
            .find(|row| row.internal_id == internal_id)
            .map(|row| row.external_id)
    }

    /// Resolve a section entity identifier to its generated-entity position.
    pub fn internal_id(&self, external_id: u32) -> Option<u32> {
        self.rows
            .iter()
            .find(|row| row.external_id == external_id)
            .map(|row| row.internal_id)
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
    /// Joins between relation, equation, and incidence identifiers.
    pub triples: Vec<FeatureRelationTriple>,
    /// Byte offset of the `relat_ptr` label in the original stream.
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
    /// Complete interpolation points in stored parameter order.
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
    /// Numeric identifier embedded in `feat_defs_<id>`, or the canonical
    /// feature owner identifier for an instantiated positional definition.
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

const TABLE_TAG: &[u8] = &[0xf7, 0x1d, 0xfb, 0xe3];
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
    starts.dedup_by_key(|(_, id)| *id);
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

/// Decode positional `AllFeatur` rows whose feature identifiers are proven by
/// geometry ownership. Unknown feature-like byte sequences remain unclaimed.
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
    let mut cursor = 0;
    let mut values = Vec::with_capacity(slot_count);
    for _ in 0..slot_count {
        let (value, next) = scalar::decode_in_lane(payload, cursor, cache)?;
        values.push(value);
        cursor = next;
    }
    (cursor == payload.len()).then_some(values)
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
    let variable_dict = match prefix {
        0x64 | 0xad => Some([0x3f, 0xd9]),
        0x69 => Some([0x3f, 0xde]),
        0x7e => Some([0x3f, 0xf3]),
        0x80 => Some([0x3f, 0xf5]),
        0x97 => Some([0x40, 0x0c]),
        0x9c => Some([0x40, 0x11]),
        0x9d => Some([0x40, 0x12]),
        0x9f => Some([0x40, 0x14]),
        0xa0 => Some([0x40, 0x15]),
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
            decode_variable_scalar(payload, value_label, close, cache);
        let guess_label = find_bytes(payload, b"guess\0", cursor, close)? + b"guess\0".len();
        let (guess, _, _) = decode_variable_scalar(payload, guess_label, close, cache);
        Some(FeatureVariableRow {
            variable_type,
            key,
            value,
            guess,
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
        let (value, next, dimension_driven) = decode_variable_scalar(payload, cursor, end, cache);
        cursor = next;
        let (guess, next, _) = decode_variable_scalar(payload, cursor, end, cache);
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
        rows.push(FeatureVariableRow {
            variable_type,
            key,
            value,
            guess,
            uvar_id: trailing.get(2).copied(),
            dimension_driven,
            offset: row_offset,
        });
        let delimiter = payload[cursor..end].iter().position(|&byte| byte == 0xe2)?;
        cursor += delimiter + 1;
    }
    variable_table_from_rows(declared_count, entity_ref, rows, table)
}

fn variable_table_from_rows(
    declared_count: u32,
    entity_ref: Option<u32>,
    rows: Vec<FeatureVariableRow>,
    offset: usize,
) -> Option<FeatureVariableTable> {
    (!rows.is_empty()).then(|| {
        let mut coordinates = BTreeMap::<u32, (Option<f64>, Option<f64>)>::new();
        for row in &rows {
            let point = coordinates.entry(row.key).or_insert((None, None));
            match row.variable_type {
                1 if row.value.is_some() => point.0 = row.value,
                2 if row.value.is_some() => point.1 = row.value,
                _ => {}
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
    })
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
    let mut rows = Vec::with_capacity(row_limit.min(1024));
    let mut prototype_separator = vec![0xf1, psb::token::ENTITY_REF];
    prototype_separator.extend_from_slice(&reference_bytes);
    prototype_separator.push(0xe2);
    while cursor < end && rows.len() < row_limit {
        let row_offset = cursor;
        let (variable_type, next) = psb::compact_int(payload, cursor);
        cursor = next;
        let (key, next) = psb::compact_int(payload, cursor);
        cursor = next;
        let (value, next, dimension_driven) = decode_variable_scalar(payload, cursor, end, cache);
        cursor = next;
        let (guess, next, _) = decode_variable_scalar(payload, cursor, end, cache);
        cursor = next;
        let mut trailing = Vec::with_capacity(3);
        while cursor < end && payload[cursor] != 0xe2 && trailing.len() < 3 {
            if payload[cursor] >= 0xc0 {
                return None;
            }
            let (field, next) = psb::compact_int(payload, cursor);
            (next > cursor).then_some(())?;
            trailing.push(field);
            cursor = next;
        }
        rows.push(FeatureVariableRow {
            variable_type,
            key,
            value,
            guess,
            uvar_id: trailing.get(2).copied(),
            dimension_driven,
            offset: row_offset,
        });
        if rows.len() < row_limit {
            if rows.len() == 1 {
                (payload.get(cursor..cursor + prototype_separator.len())
                    == Some(prototype_separator.as_slice()))
                .then_some(())?;
                cursor += prototype_separator.len();
            } else {
                (payload.get(cursor) == Some(&0xe2)).then_some(())?;
                cursor += 1;
            }
        }
    }
    (rows.len() == row_limit).then_some(())?;
    variable_table_from_rows(declared_count, Some(table_class), rows, table)
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
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            values.push(next_segment_int(payload, &mut p));
        }
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
        let kind = match kind[0]? {
            2 => FeatureSegmentKind::Line,
            3 => FeatureSegmentKind::Arc,
            5 => FeatureSegmentKind::Point,
            _ => return None,
        };
        let point0 = point_ids[0]?;
        let point1 = if kind == FeatureSegmentKind::Point {
            point0
        } else {
            point_ids[1]?
        };
        Some(FeatureSegment {
            kind,
            directions: [directions[0], directions[1], directions[2]],
            point_ids: [point0, point1],
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
    ]
    .into_iter()
    .filter_map(|label| find_bytes(payload, label, cursor, end))
    .min()
    .unwrap_or(end);
    let mut rows = named_row.into_iter().collect::<Vec<_>>();
    let first_row = cursor;
    let row_limit = usize::try_from(declared_count).unwrap_or(usize::MAX);
    while cursor < region_end && rows.len() < row_limit {
        let row_start = cursor;
        let kind_offset = if payload.get(cursor..cursor + 2) == Some(&[0xc0, 0x80]) {
            cursor + 2
        } else {
            cursor
        };
        if !matches!(payload.get(kind_offset), Some(2 | 3 | 5))
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
        let kind = match kind_raw {
            Some(2) => FeatureSegmentKind::Line,
            Some(3) => FeatureSegmentKind::Arc,
            Some(5) => FeatureSegmentKind::Point,
            _ => {
                cursor += 1;
                continue;
            }
        };
        let directions = [
            next_segment_int(payload, &mut p),
            next_segment_int(payload, &mut p),
            next_segment_int(payload, &mut p),
        ];
        let point0 = next_segment_int(payload, &mut p);
        let point1 = next_segment_int(payload, &mut p);
        let Some(point0) = point0 else {
            cursor += 1;
            continue;
        };
        let point1 = if kind == FeatureSegmentKind::Point {
            point0
        } else if let Some(point1) = point1 {
            point1
        } else {
            cursor += 1;
            continue;
        };
        let center_id = next_segment_int(payload, &mut p);
        let arc_orientation = next_segment_int(payload, &mut p);
        let verhor_flag = payload.get(p) == Some(&0xf5);
        let vertical_horizontal = next_segment_int(payload, &mut p);
        if verhor_flag {
            let _ = next_segment_int(payload, &mut p);
        }
        let radius_ref = next_segment_int(payload, &mut p);
        let radius2_ref = next_segment_int(payload, &mut p);
        let Some(external_id) = next_segment_int(payload, &mut p) else {
            cursor += 1;
            continue;
        };
        if payload.get(p) == Some(&0xe2) && point0 < 256 && point1 < 256 {
            rows.push(FeatureSegment {
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
            });
            cursor = p + 1;
        } else {
            cursor += 1;
        }
    }
    (!rows.is_empty()).then_some(FeatureSegmentTable {
        declared_count,
        entity_ref,
        rows,
        offset: table,
    })
}

fn trim_entity_table(
    payload: &[u8],
    start: usize,
    end: usize,
    segments: Option<&FeatureSegmentTable>,
) -> Option<FeatureTrimEntityTable> {
    let table = find_bytes(payload, b"ent_tab\0", start, end)?;
    let prototype = find_bytes(payload, b"entry_ptr(entity_entry)", table, end)?;
    let close = find_bytes(payload, &[0xf2, psb::token::ENTITY_REF], prototype, end)?;
    let (_, mut cursor) = psb::compact_int(payload, close + 2);
    if payload.get(cursor) == Some(&0xe3) {
        cursor += 1;
    }
    let first_row = cursor;
    let region_end = find_bytes(payload, b"vert_tab", cursor, end).unwrap_or(end);
    let valid_ids = segments.map(|table| {
        table
            .rows
            .iter()
            .map(|row| row.external_id)
            .collect::<BTreeSet<_>>()
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
            if external_id != 0
                && payload.get(p) == Some(&0)
                && valid_ids
                    .as_ref()
                    .is_none_or(|ids| ids.contains(&external_id))
                && seen.insert(external_id)
            {
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
    (!rows.is_empty()).then(|| FeatureTrimEntityTable {
        solved_external_ids: seen.into_iter().collect(),
        rows,
        offset: table,
    })
}

fn trim_table_classes(
    payload: &[u8],
    label: &[u8],
    start: usize,
    end: usize,
) -> Option<(u32, u32)> {
    let table = find_bytes(payload, label, start, end)? + label.len();
    let opener = (table..end).find(|&offset| payload[offset] == psb::token::ARRAY_OPEN)?;
    let (_, after_count) = psb::compact_int(payload, opener + 1);
    (payload.get(after_count) == Some(&psb::token::ENTITY_REF)).then_some(())?;
    let (table_class, _) = psb::reference_id(payload, after_count + 1).ok()?;
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
    Some((table_class, entry_class))
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
    table_class: u32,
    entry_class: u32,
    next_table_class: Option<u32>,
    segments: Option<&FeatureSegmentTable>,
) -> Option<FeatureTrimEntityTable> {
    let (table, declared_count, rows_start, region_end) =
        positional_table_region(payload, start, end, table_class, next_table_class)?;
    (declared_count > 0).then_some(())?;
    let valid_ids = segments.map(|table| {
        table
            .rows
            .iter()
            .map(|row| row.external_id)
            .collect::<BTreeSet<_>>()
    });
    let mut rows = Vec::new();
    let mut seen = BTreeSet::new();
    (rows_start..region_end)
        .any(|offset| {
            if payload.get(offset) != Some(&psb::token::ENTITY_REF) {
                return false;
            }
            psb::reference_id(payload, offset + 1).is_ok_and(|(class, after_reference)| {
                class == entry_class
                    && payload.get(after_reference..after_reference + 2) == Some(&[0, 0xe3])
            })
        })
        .then_some(())?;
    let mut cursor = rows_start;
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
            if external_id != 0
                && payload.get(p) == Some(&0)
                && valid_ids
                    .as_ref()
                    .is_none_or(|ids| ids.contains(&external_id))
                && seen.insert(external_id)
            {
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
    (!rows.is_empty()).then(|| FeatureTrimEntityTable {
        solved_external_ids: seen.into_iter().collect(),
        rows,
        offset: table,
    })
}

fn trim_vertex_table(
    payload: &[u8],
    start: usize,
    end: usize,
    entities: Option<&FeatureTrimEntityTable>,
    segments: Option<&FeatureSegmentTable>,
    variables: Option<&FeatureVariableTable>,
) -> Option<FeatureTrimVertexTable> {
    let table = find_bytes(payload, b"vert_tab\0", start, end)?;
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
    cursor = find_bytes(payload, &block_marker, reference_end, end)?;

    let valid_entities = entities.map(|table| {
        table
            .rows
            .iter()
            .map(|row| row.external_id)
            .collect::<BTreeSet<_>>()
    });
    let valid_vertices = entities.map(|table| {
        table
            .rows
            .iter()
            .flat_map(|row| row.vertices.into_iter().chain(row.center_vertex))
            .collect::<BTreeSet<_>>()
    });
    let mut rows = Vec::new();
    let mut seen = BTreeSet::new();
    while cursor < end {
        if payload.get(cursor..cursor + block_marker.len()) == Some(block_marker.as_slice()) {
            cursor += block_marker.len();
            let (_, next) = segment_int(payload, cursor);
            cursor = next;
            continue;
        }
        match payload[cursor] {
            psb::token::ARRAY_OPEN => {
                let (_, next) = psb::compact_int(payload, cursor + 1);
                cursor = next;
                continue;
            }
            psb::token::ENTITY_REF => {
                let Ok((_, next)) = psb::reference_id(payload, cursor + 1) else {
                    cursor += 1;
                    continue;
                };
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
        let (entity_1, next) = segment_int(payload, cursor);
        let (entity_2, next) = segment_int(payload, next);
        let (vertex_id, next) = segment_int(payload, next);
        let (Some(entity_1), Some(entity_2), Some(vertex_id)) = (entity_1, entity_2, vertex_id)
        else {
            cursor += 1;
            continue;
        };
        if payload.get(next) != Some(&0) {
            cursor += 1;
            continue;
        }
        let valid = match (&valid_entities, &valid_vertices) {
            (Some(entity_ids), Some(vertex_ids)) => {
                entity_ids.contains(&entity_1)
                    && entity_ids.contains(&entity_2)
                    && vertex_ids.contains(&vertex_id)
            }
            _ => entity_1 > 2 && entity_2 > 2,
        };
        if valid && entity_1 != entity_2 && seen.insert(vertex_id) {
            rows.push(FeatureTrimVertex {
                vertex_id,
                entities: [entity_1, entity_2],
                section_coordinates: line_intersection([entity_1, entity_2], segments, variables),
                offset: row_offset,
            });
        }
        cursor = next + 1;
    }
    (!rows.is_empty()).then_some(FeatureTrimVertexTable {
        rows,
        offset: table,
    })
}

fn positional_trim_vertex_table(
    payload: &[u8],
    start: usize,
    end: usize,
    classes: (u32, u32),
    entities: Option<&FeatureTrimEntityTable>,
    segments: Option<&FeatureSegmentTable>,
    variables: Option<&FeatureVariableTable>,
) -> Option<FeatureTrimVertexTable> {
    let (table_class, entry_class) = classes;
    let (table, declared_count, rows_start, region_end) =
        positional_table_region(payload, start, end, table_class, None)?;
    (declared_count > 0).then_some(())?;
    let valid_entities = entities.map(|table| {
        table
            .rows
            .iter()
            .map(|row| row.external_id)
            .collect::<BTreeSet<_>>()
    });
    let valid_vertices = entities.map(|table| {
        table
            .rows
            .iter()
            .flat_map(|row| row.vertices.into_iter().chain(row.center_vertex))
            .collect::<BTreeSet<_>>()
    });
    let mut rows = Vec::new();
    let mut seen = BTreeSet::new();
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
        let (entity_1, next) = segment_int(payload, row_offset);
        let (entity_2, next) = segment_int(payload, next);
        let (vertex_id, next) = segment_int(payload, next);
        let (Some(entity_1), Some(entity_2), Some(vertex_id)) = (entity_1, entity_2, vertex_id)
        else {
            cursor += 1;
            continue;
        };
        let valid = match (&valid_entities, &valid_vertices) {
            (Some(entity_ids), Some(vertex_ids)) => {
                entity_ids.contains(&entity_1)
                    && entity_ids.contains(&entity_2)
                    && vertex_ids.contains(&vertex_id)
            }
            _ => entity_1 > 2 && entity_2 > 2,
        };
        if payload.get(next) == Some(&0) && valid && entity_1 != entity_2 && seen.insert(vertex_id)
        {
            rows.push(FeatureTrimVertex {
                vertex_id,
                entities: [entity_1, entity_2],
                section_coordinates: line_intersection([entity_1, entity_2], segments, variables),
                offset: row_offset,
            });
        }
        cursor = next.saturating_add(1).max(cursor + 1);
    }
    (!rows.is_empty()).then_some(FeatureTrimVertexTable {
        rows,
        offset: table,
    })
}

fn line_intersection(
    entity_ids: [u32; 2],
    segments: Option<&FeatureSegmentTable>,
    variables: Option<&FeatureVariableTable>,
) -> Option<[f64; 2]> {
    let segments = segments?;
    let variables = variables?;
    let segment = |external_id| {
        segments.rows.iter().find(|segment| {
            segment.external_id == external_id && segment.kind == FeatureSegmentKind::Line
        })
    };
    let point = |point_id| {
        variables
            .points
            .iter()
            .find(|point| point.point_id == point_id)
            .and_then(|point| Some([point.u?, point.v?]))
    };
    let first = segment(entity_ids[0])?;
    let second = segment(entity_ids[1])?;
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
    let (_, next) = psb::reference_id(payload, close + 2).ok()?;
    cursor = next;
    if payload.get(cursor) == Some(&0xe2) {
        cursor += 1;
    }
    let mut rows = Vec::new();
    let mut external_ids = BTreeSet::new();
    let mut internal_ids = BTreeSet::new();
    let row_limit = usize::try_from(declared_count).unwrap_or(usize::MAX);
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
    (!rows.is_empty()).then_some(FeatureOrderTable {
        declared_count,
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
    let (table, declared_count, mut cursor) = (start..end).find_map(|table| {
        (payload.get(table) == Some(&psb::token::ARRAY_OPEN)).then_some(())?;
        let (declared_count, after_count) = psb::compact_int(payload, table + 1);
        (payload.get(after_count) == Some(&psb::token::ENTITY_REF)).then_some(())?;
        let (class, after_reference) = psb::reference_id(payload, after_count + 1).ok()?;
        (class == table_class
            && payload.get(after_reference..after_reference + 2) == Some(&[0xfb, 0xe2]))
        .then_some((table, declared_count, after_reference + 2))
    })?;
    (payload.get(cursor) == Some(&psb::token::ENTITY_REF)).then_some(())?;
    let (_, after_entry_class) = psb::reference_id(payload, cursor + 1).ok()?;
    cursor = after_entry_class;
    for _ in 0..3 {
        let (_, next) = segment_int(payload, cursor);
        (next > cursor).then_some(())?;
        cursor = next;
    }
    (payload.get(cursor..cursor + 2) == Some(&[0xf1, psb::token::ENTITY_REF])).then_some(())?;
    let (class, after_reference) = psb::reference_id(payload, cursor + 2).ok()?;
    (class == table_class && payload.get(after_reference) == Some(&0xe2)).then_some(())?;
    cursor = after_reference + 1;
    let row_limit = usize::try_from(declared_count.saturating_sub(1)).unwrap_or(usize::MAX);
    let mut rows = Vec::with_capacity(row_limit.min(1024));
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
            return None;
        };
        if !external_ids.insert(external_id) || !internal_ids.insert(internal_id) {
            return None;
        }
        rows.push(FeatureOrderRow {
            external_id,
            internal_id,
            bitmask,
            offset: row_offset,
        });
        cursor = next;
        if rows.len() == row_limit {
            break;
        }
        (payload.get(cursor) == Some(&0xe2)).then_some(())?;
        cursor += 1;
    }
    (rows.len() == row_limit).then_some(FeatureOrderTable {
        declared_count,
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
    payload[start..end]
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
            let mut cursor = name_end + 1;
            let section_flip = payload.get(cursor).copied().and_then(BinaryFlag::decode);
            cursor += 1;
            for _ in 0..3 {
                let (_, next) = segment_int(payload, cursor);
                (next > cursor).then_some(())?;
                cursor = next;
            }
            let (sketch_plane_entity_id, next) = segment_int(payload, cursor);
            cursor = next;
            let sketch_plane_flip = payload.get(cursor).copied().and_then(BinaryFlag::decode);
            cursor += 1;
            (payload.get(cursor) == Some(&psb::token::ARRAY_OPEN)).then_some(())?;
            let (reference_count, next) = psb::compact_int(payload, cursor + 1);
            cursor = next;
            (payload.get(cursor) == Some(&psb::token::ENTITY_REF)).then_some(())?;
            let table_reference_start = cursor + 1;
            let (_, next) = psb::reference_id(payload, table_reference_start).ok()?;
            let table_reference = payload[table_reference_start..next].to_vec();
            cursor = next;
            (payload.get(cursor..cursor + 2) == Some(&[0xfb, 0xe2])).then_some(())?;
            cursor += 2;
            (payload.get(cursor) == Some(&psb::token::ENTITY_REF)).then_some(())?;
            let (_, next) = psb::reference_id(payload, cursor + 1).ok()?;
            cursor = next;

            let mut reference_plane_entity_ids = Vec::new();
            let mut orientation = FeatureSectionOrientation {
                section_flip,
                ..FeatureSectionOrientation::default()
            };
            let row_count = usize::try_from(reference_count).ok()?;
            let mut separator = vec![0xf2, psb::token::ENTITY_REF];
            separator.extend_from_slice(&table_reference);
            separator.push(0xe2);
            for row in 0..row_count {
                let plane_id = next_segment_int(payload, &mut cursor)?;
                let reference_type = next_segment_int(payload, &mut cursor);
                let _external_reference_id = next_segment_int(payload, &mut cursor);
                let segment_id = next_segment_int(payload, &mut cursor);
                let _sub_index = next_segment_int(payload, &mut cursor);
                let reference_flip = payload.get(cursor).copied().and_then(BinaryFlag::decode);
                let (_, next) = segment_int(payload, cursor);
                cursor = next;
                reference_plane_entity_ids.push(plane_id);
                if row == 0 {
                    orientation.reference_type = reference_type;
                    orientation.segment_id = segment_id;
                    orientation.reference_flip = reference_flip;
                }
                if row + 1 < row_count {
                    let separator_at = find_bytes(payload, &separator, cursor, end)?;
                    cursor = separator_at + separator.len();
                }
            }
            Some(FeatureSection3d {
                sketch_plane_entity_id,
                sketch_plane_flip,
                reference_plane_entity_ids,
                reference_plane_datum_geometry_id: None,
                orientation,
                dimension_ids: Vec::new(),
                offset: section,
            })
        })
}

fn dimension_unit(dimension_type: u32) -> DimensionUnit {
    match dimension_type {
        0x0a => DimensionUnit::Radians,
        0x01..=0x05 => DimensionUnit::Millimeters,
        _ => DimensionUnit::SchemaDefined,
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
    let (value, after_value, _) =
        decode_variable_scalar(payload, value_label + b"value\0".len(), end, cache);
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
    let (value, cursor, _) = decode_variable_scalar(payload, cursor, end, cache);
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
    (!rows.is_empty()).then_some(FeatureDimensionTable {
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
        let row = positional_dimension(payload, cursor, row_end, cache)?;
        rows.push(row);
        if rows.len() == row_limit {
            break;
        }
        (payload.get(row_end..row_end + separator.len()) == Some(separator.as_slice()))
            .then_some(())?;
        cursor = row_end + separator.len();
    }
    (rows.len() == row_limit && !rows.is_empty()).then_some(FeatureDimensionTable {
        declared_count,
        entity_ref: Some(table_class),
        rows,
        offset: table,
    })
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
    while rows.len() < usize::try_from(declared_count).unwrap_or(usize::MAX) {
        let row_offset = cursor;
        let Some(id) = next_solver_int(payload, &mut cursor) else {
            return Vec::new();
        };
        let Some(kind) = next_solver_int(payload, &mut cursor) else {
            return Vec::new();
        };
        let Some(flags) = next_solver_int(payload, &mut cursor) else {
            return Vec::new();
        };
        let Some(status) = next_solver_int(payload, &mut cursor) else {
            return Vec::new();
        };
        if payload.get(cursor) != Some(&psb::token::ARRAY_OPEN) {
            return Vec::new();
        }
        let (item_count, next) = psb::compact_int(payload, cursor + 1);
        cursor = next;
        let Ok((_, next)) = psb::reference_id(payload, cursor + 1) else {
            return Vec::new();
        };
        cursor = next;
        if payload.get(cursor..cursor + 2) != Some(&[psb::token::ARRAY_CLOSE, 0xe2]) {
            return Vec::new();
        }
        cursor += 2;
        let mut items = Vec::new();
        while items.len() < usize::try_from(item_count).unwrap_or(usize::MAX) {
            if !items.is_empty() && payload.get(cursor) == Some(&0xe2) {
                cursor += 1;
            }
            if payload.get(cursor) == Some(&psb::token::ENTITY_REF) {
                let Ok((_, next)) = psb::reference_id(payload, cursor + 1) else {
                    return Vec::new();
                };
                cursor = next;
            }
            let Some(entity_id) = next_solver_int(payload, &mut cursor) else {
                return Vec::new();
            };
            let Some(sense) = next_solver_int(payload, &mut cursor) else {
                return Vec::new();
            };
            items.push(FeatureSkampItem { entity_id, sense });
            if payload.get(cursor) == Some(&0xf1) {
                let Ok((_, next)) = psb::reference_id(payload, cursor + 2) else {
                    return Vec::new();
                };
                cursor = next;
                if payload.get(cursor) != Some(&0xe2) {
                    return Vec::new();
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
            return Vec::new();
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
            return Vec::new();
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
    let close = find_bytes(payload, &[0xf1, psb::token::ENTITY_REF], cursor, end)?;
    let (_, after_ref) = psb::reference_id(payload, close + 2).ok()?;
    (payload.get(after_ref) == Some(&0xe2)).then_some(())?;
    cursor = after_ref + 1;

    let rows = positional_relation_rows(payload, cursor, end, declared_count.saturating_sub(2))?;
    Some(FeatureRelationTable {
        declared_count,
        entity_ref,
        rows,
        skamps: feature_skamps(payload, start, end),
        triples: feature_relation_triples(payload, start, end),
        offset: table,
    })
}

fn positional_relation_rows(
    payload: &[u8],
    mut cursor: usize,
    end: usize,
    row_count: u32,
) -> Option<Vec<FeatureRelation>> {
    let mut rows = Vec::new();
    for _ in 0..row_count {
        let row_end = payload[cursor..end].iter().position(|byte| *byte == 0xe2)? + cursor;
        let (relation_id, after_id) = psb::compact_int(payload, cursor);
        (after_id > cursor && after_id < row_end).then_some(())?;
        let (used, after_used) = psb::compact_int(payload, after_id);
        (after_used > after_id && after_used < row_end).then_some(())?;
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
            return None;
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
    Some(rows)
}

fn positional_relation_table(
    payload: &[u8],
    start: usize,
    end: usize,
    table_class: u32,
) -> Option<FeatureRelationTable> {
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
    let (_, next) = psb::reference_id(payload, cursor + 1).ok()?;
    cursor = next;
    let mut prototype_separator = vec![0xf1, psb::token::ENTITY_REF];
    prototype_separator.extend_from_slice(&reference_bytes);
    prototype_separator.push(0xe2);
    let prototype_end = find_bytes(payload, &prototype_separator, cursor, end)?;
    cursor = prototype_end + prototype_separator.len();
    let rows = positional_relation_rows(payload, cursor, end, declared_count.saturating_sub(2))?;
    Some(FeatureRelationTable {
        declared_count,
        entity_ref: Some(table_class),
        rows,
        skamps: Vec::new(),
        triples: Vec::new(),
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
        if values.len() != 6 || (!row_separator && !named_boundary) {
            cursor = record_offset + 1;
            continue;
        }
        if row_separator {
            cursor += 1;
        }
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
            let segment = segments
                .rows
                .iter()
                .find(|segment| segment.external_id == row.external_id)?;
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
        let Some(header_size) = payload[after_id..row_end]
            .iter()
            .position(|byte| *byte == 0xe2)
        else {
            continue;
        };
        let mut cursor = after_id + header_size + 1;
        let mut values = Vec::with_capacity(value_count);
        while cursor < row_end && values.len() < value_count {
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
            continue;
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
            let Some(center) =
                saved_named_scalars::<3>(payload, b"center", body_start, body_end, cache)
            else {
                search = body_end;
                continue;
            };
            let Some([radius]) =
                saved_named_scalars::<1>(payload, b"radius", body_start, body_end, cache)
            else {
                search = body_end;
                continue;
            };
            if kind == "arc" {
                let Some(first) =
                    saved_named_scalars::<3>(payload, b"end1", body_start, body_end, cache)
                else {
                    search = body_end;
                    continue;
                };
                let Some(second) =
                    saved_named_scalars::<3>(payload, b"end2", body_start, body_end, cache)
                else {
                    search = body_end;
                    continue;
                };
                let Some([start_parameter]) =
                    saved_named_scalars::<1>(payload, b"t0", body_start, body_end, cache)
                else {
                    search = body_end;
                    continue;
                };
                let Some([end_parameter]) =
                    saved_named_scalars::<1>(payload, b"t1", body_start, body_end, cache)
                else {
                    search = body_end;
                    continue;
                };
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
        let Some(points_label) = find_bytes(payload, POINTS, body_start, body_end) else {
            search = body_start;
            continue;
        };
        let dimensions = points_label + POINTS.len();
        let (Some(&point_count), Some(&coordinate_count)) =
            (payload.get(dimensions), payload.get(dimensions + 1))
        else {
            search = body_start;
            continue;
        };
        if coordinate_count != 3 {
            search = body_start;
            continue;
        }
        let mut cursor = dimensions + 2;
        let mut points = Vec::with_capacity(usize::from(point_count));
        for _ in 0..point_count {
            let mut point = [0.0; 3];
            let mut complete = true;
            for coordinate in &mut point {
                let Some((value, next)) = scalar::decode_in_lane(payload, cursor, cache) else {
                    complete = false;
                    break;
                };
                *coordinate = value;
                cursor = next;
            }
            if !complete {
                points.clear();
                break;
            }
            points.push(point);
        }
        if points.len() == usize::from(point_count) {
            let endpoint_tangents = find_bytes(
                payload,
                b"\xe0\x02end_tangts\0\xf9\x02\x03",
                cursor,
                body_end,
            )
            .and_then(|label| {
                let mut at = label + b"\xe0\x02end_tangts\0\xf9\x02\x03".len();
                let mut tangents = [[0.0; 3]; 2];
                for tangent in &mut tangents {
                    for coordinate in tangent {
                        let (value, next) = scalar::decode_in_lane(payload, at, cache)?;
                        *coordinate = value;
                        at = next;
                    }
                }
                Some(tangents)
            });
            let parameters = find_bytes(payload, b"\xe0\x02params\0\xf8", cursor, body_end)
                .and_then(|label| {
                    let count_at = label + b"\xe0\x02params\0\xf8".len();
                    let (count, mut at) = psb::compact_int(payload, count_at);
                    (count == u32::from(point_count) && at > count_at).then_some(())?;
                    let mut values = Vec::with_capacity(usize::from(point_count));
                    for _ in 0..count {
                        let (value, next) = saved_spline_parameter(payload, at, cache)?;
                        values.push(value);
                        at = next;
                    }
                    Some(values)
                });
            entities.push(FeatureSavedEntity::Spline(FeatureSavedSpline {
                entity_id: saved_entity_id(payload, body_start, points_label),
                interpolation_points: points,
                endpoint_tangents,
                parameters,
                offset: entity_offset,
            }));
        }
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
    (!entities.is_empty()).then_some(FeatureSavedSection {
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
    if payload[0] == psb::token::SCALAR_BODY && payload.len() >= 3 {
        let slot_count = usize::from(payload[1]).checked_mul(usize::from(payload[2]));
        let cache = scalar::ScalarCache::from_section(payload);
        let decoded_values =
            slot_count.and_then(|slots| decode_exact_scalars(&payload[3..], slots, &cache));
        return FeatureFieldValue::ScalarArray {
            dimensions: payload[1],
            count: payload[2],
            body: payload[3..].to_vec(),
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
            }
        }
    }
    tables.sort_by_key(|table| table.offset);
    tables
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
                let mut ids = Vec::with_capacity(count as usize);
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

fn replay_ids(run: &[u8], count: u32, mut cursor: usize) -> Option<(Vec<u32>, usize)> {
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

/// Decode the two affected-ID array positions in class-913 replay rows.
///
/// Array extents are stateful within one `AllFeatur` stream and schema class.
/// An omitted `f8` opener reuses the preceding extent at the same array
/// position.
pub fn replay_affected_ids(rows: &[FeatureRow]) -> Vec<FeatureReplayAffectedIds> {
    const ANCHOR: &[u8] = &[0xf1, 0xf7, 0x42, 0xd8, 0x80, 0x01, 0xe3];
    const TERMINATOR: &[u8] = &[0xf5, 0x96, 0x92];
    let mut result = Vec::new();
    let mut extents = BTreeMap::<usize, [Option<u32>; 2]>::new();
    for row in rows {
        if row.root_schema_class != Some(913) {
            continue;
        }
        let Some(anchor) = row
            .body
            .windows(ANCHOR.len())
            .rposition(|window| window == ANCHOR)
        else {
            continue;
        };
        let run_start = anchor + ANCHOR.len();
        let Some(term_relative) = row.body[run_start..]
            .windows(TERMINATOR.len())
            .position(|window| window == TERMINATOR)
        else {
            continue;
        };
        let run = &row.body[run_start..run_start + term_relative];
        let state = extents.entry(row.stream_offset).or_default();
        let Some((geometry_count, geometry_extent, cursor)) =
            replay_extent(run, 0, b"geoms_affected", state[0])
        else {
            continue;
        };
        let Some((geometry_ids, cursor)) = replay_ids(run, geometry_count, cursor) else {
            continue;
        };
        let Some((edge_count, edge_extent, cursor)) =
            replay_extent(run, cursor, b"edgs_affected", state[1])
        else {
            continue;
        };
        let Some((edge_ids, _)) = replay_ids(run, edge_count, cursor) else {
            continue;
        };
        state[0] = Some(geometry_count);
        state[1] = Some(edge_count);
        result.push(FeatureReplayAffectedIds {
            feature_id: row.feature_id,
            geometry_ids,
            edge_ids,
            geometry_extent,
            edge_extent,
            offset: row.body_offset + anchor,
        });
    }
    result.sort_by_key(|record| record.offset);
    result
}

/// Decode genuine named `direction` and `direction2` recipe bytes.
pub fn direction_bytes(rows: &[FeatureRow]) -> Vec<FeatureDirectionByte> {
    const FIELDS: &[(&[u8], DirectionLane)] = &[
        (b"direction", DirectionLane::Primary),
        (b"direction2", DirectionLane::Secondary),
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
                if label_offset < 2 || row.body[label_offset - 2] != psb::token::NAMED_RECORD {
                    continue;
                }
                let Some(&raw) = row.body.get(from) else {
                    continue;
                };
                let value = match raw {
                    0 => DirectionValue::SideFlag(false),
                    1 => DirectionValue::SideFlag(true),
                    value => DirectionValue::Raw(value),
                };
                result.push(FeatureDirectionByte {
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

fn definitions_in_ranges(
    payload: &[u8],
    starts: &[(usize, u32, Option<u32>)],
) -> Vec<FeatureDefinition> {
    let cache = scalar::ScalarCache::from_section(payload);
    let mut result = Vec::new();
    let mut replay_dimension_class = None;
    let mut replay_variable_class = None;
    let mut replay_relation_class = None;
    let mut replay_trim_entity_classes = None;
    let mut replay_trim_vertex_classes = None;
    let mut replay_order_class = None;
    for (index, &(start, id, owner_override)) in starts.iter().enumerate() {
        let end = starts
            .get(index + 1)
            .map_or(payload.len(), |&(offset, _, _)| offset);
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
                    decoded_values: decode_exact_scalars(&body, 12, &cache),
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
            owner_override.and_then(|_| {
                positional_variable_table(payload, start, end, replay_variable_class?, &cache)
            })
        });
        if owner_override.is_none() {
            replay_variable_class = variables.as_ref().and_then(|table| table.entity_ref);
        }
        let segments = segment_table(payload, start, end)
            .or_else(|| owner_override.and_then(|_| positional_segment_table(payload, start, end)));
        let trim_entities =
            trim_entity_table(payload, start, end, segments.as_ref()).or_else(|| {
                owner_override.and_then(|_| {
                    let (table_class, entry_class) = replay_trim_entity_classes?;
                    positional_trim_entity_table(
                        payload,
                        start,
                        end,
                        table_class,
                        entry_class,
                        replay_trim_vertex_classes.map(|(class, _)| class),
                        segments.as_ref(),
                    )
                })
            });
        if owner_override.is_none() {
            replay_trim_entity_classes = trim_table_classes(payload, b"ent_tab\0", start, end);
        }
        let trim_vertices = trim_vertex_table(
            payload,
            start,
            end,
            trim_entities.as_ref(),
            segments.as_ref(),
            variables.as_ref(),
        )
        .or_else(|| {
            owner_override.and_then(|_| {
                let (table_class, entry_class) = replay_trim_vertex_classes?;
                positional_trim_vertex_table(
                    payload,
                    start,
                    end,
                    (table_class, entry_class),
                    trim_entities.as_ref(),
                    segments.as_ref(),
                    variables.as_ref(),
                )
            })
        });
        if owner_override.is_none() {
            replay_trim_vertex_classes = trim_table_classes(payload, b"vert_tab\0", start, end);
        }
        let order_table = order_table(payload, start, end).or_else(|| {
            owner_override
                .and_then(|_| positional_order_table(payload, start, end, replay_order_class?))
        });
        if owner_override.is_none() {
            replay_order_class = order_table.as_ref().and_then(|table| table.entity_ref);
        }
        let section_3d = section_3d(payload, start, end)
            .or_else(|| owner_override.and_then(|_| positional_section_3d(payload, start, end)));
        let dimensions = dimension_table(payload, start, end, &cache).or_else(|| {
            owner_override.and_then(|_| {
                positional_dimension_table(payload, start, end, replay_dimension_class?, &cache)
            })
        });
        if owner_override.is_none() {
            replay_dimension_class = dimensions.as_ref().and_then(|table| table.entity_ref);
        }
        let relations = relation_table(payload, start, end).or_else(|| {
            owner_override.and_then(|_| {
                positional_relation_table(payload, start, end, replay_relation_class?)
            })
        });
        if owner_override.is_none() {
            replay_relation_class = relations.as_ref().and_then(|table| table.entity_ref);
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
            owner_override.and_then(|_| {
                positional_saved_section(
                    payload,
                    start,
                    end,
                    &cache,
                    order_table.as_ref(),
                    segments.as_ref(),
                )
            })
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
pub fn definitions(payload: &[u8]) -> Vec<FeatureDefinition> {
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
        starts.push((offset, id, None));
    }
    starts.sort_unstable_by_key(|&(offset, _, _)| offset);
    let labeled_starts = starts.clone();
    for (index, &(start, _, _)) in labeled_starts.iter().enumerate() {
        let end = labeled_starts
            .get(index + 1)
            .map_or(payload.len(), |&(offset, _, _)| offset);
        for (offset, owner) in
            contextual_references(payload, start, end, b"feat_id", b"ref_model_info")
        {
            starts.push((offset, owner, Some(owner)));
        }
    }
    starts.sort_unstable_by_key(|&(offset, _, _)| offset);
    starts.dedup_by_key(|entry| entry.0);
    definitions_in_ranges(payload, &starts)
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
        &[(*start, section_id, Some(owner_feature_id))],
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

/// Decode the implicit named-record entity table and every canonical `f7`
/// reference, preserving both source context and unresolved target IDs.
pub fn entity_graph(payload: &[u8]) -> (Vec<FeatureEntity>, Vec<FeatureEntityReference>) {
    let tokens = psb::tokens(payload);
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
    for _ in 0..count {
        let prefixed = payload.get(cursor..cursor + 2) == Some(&[0xf7, 0x1e]);
        if prefixed {
            cursor += 2;
        }
        let offset = cursor;
        let (id, after) = psb::reference_id(payload, cursor).ok()?;
        let (class_id, after_class) = psb::reference_id(payload, after).ok()?;
        let (source_entity_id, body_start) = if class_id == 200 {
            match psb::reference_id(payload, after_class) {
                Ok((order, after_order)) => (Some(order), after_order),
                Err(_) => (None, after_class),
            }
        } else {
            (None, after_class)
        };
        let close = payload
            .get(body_start..)?
            .iter()
            .position(|&byte| byte == 0xe3)?;
        let end_offset = body_start + close + 1;
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
        if count == 0 || payload.get(after_count..after_count + TABLE_TAG.len()) != Some(TABLE_TAG)
        {
            continue;
        }
        let Some(&(_, row_end, feature_id)) = spans
            .iter()
            .find(|&&(start, end, _)| start <= offset && offset < end)
        else {
            continue;
        };
        let Some(entries) = read_entries(&payload[..row_end], after_count + TABLE_TAG.len(), count)
        else {
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
        if surface_ids.is_empty() {
            continue;
        }
        let non_surface_entity_ids = entry_ids
            .iter()
            .copied()
            .filter(|id| !surface_ids.contains(id))
            .collect();
        tables.push(FeatureEntityTable {
            feature_id: Some(feature_id),
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

    #[test]
    fn positional_round_replay_inherits_each_array_extent() {
        let rows = [
            replay_row(1, &[0xf8, 2, 10, 11, 0xf8, 3, 20, 21, 22]),
            replay_row(2, &[12, 13, 23, 24, 25]),
            replay_row(3, &[0xf8, 1, 14, 26, 27, 28]),
        ];

        let decoded = replay_affected_ids(&rows);

        assert_eq!(decoded.len(), 3);
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
        assert_eq!(segments.rows[0].point_ids, [7, 8]);
        assert_eq!(segments.rows[1].kind, FeatureSegmentKind::Arc);
        assert_eq!(segments.rows[1].center_id, Some(10));
        assert_eq!(segments.rows[1].external_id, 43);
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
        assert_eq!(variables.rows[0].uvar_id, Some(9));
        assert_eq!(variables.rows[1].uvar_id, Some(10));
        assert_eq!(variables.points.len(), 1);
        assert_eq!(variables.points[0].point_id, 7);
        assert_eq!(variables.points[0].u, Some(0.0));
        assert_eq!(variables.points[0].v, Some(0.0));
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
    fn positional_trim_entity_table_uses_inherited_entry_class() {
        let payload = b"prefix\xf8\x07\xf7\x42\xfb\xe2\xf7\x43\x00\xe3\
            \x09\x00\x03\x04\xf6\x00\
            \xf4\x04\xf7\x42\xe2\x01\xf8\x13\xf7\x44\xfb\xe2";
        let segments = FeatureSegmentTable {
            declared_count: 1,
            entity_ref: None,
            rows: vec![FeatureSegment {
                kind: FeatureSegmentKind::Line,
                directions: [None; 3],
                point_ids: [1, 2],
                center_id: None,
                arc_orientation: None,
                vertical_horizontal: None,
                radius_ref: None,
                radius2_ref: None,
                external_id: 9,
                offset: 0,
            }],
            offset: 0,
        };

        let entities = positional_trim_entity_table(
            payload,
            0,
            payload.len(),
            66,
            67,
            Some(68),
            Some(&segments),
        )
        .expect("positional ent_tab");

        assert_eq!(entities.solved_external_ids, vec![9]);
        assert_eq!(entities.rows[0].vertices, [3, 4]);
        assert_eq!(entities.rows[0].kind, TrimEntityKind::Line);
    }

    #[test]
    fn positional_order_table_replays_prototype_and_following_rows() {
        let payload = b"prefix\xf8\x03\xf7\x42\xfb\xe2\xf7\x43\
            \x09\x01\x00\xf1\xf7\x42\xe2\
            \x0a\x02\x01\xe2\x0b\x03\x00";

        let order =
            positional_order_table(payload, 0, payload.len(), 66).expect("positional order_table");

        assert_eq!(order.declared_count, 3);
        assert_eq!(order.entity_ref, Some(66));
        assert_eq!(order.rows.len(), 2);
        assert_eq!(order.rows[0].external_id, 10);
        assert_eq!(order.rows[0].internal_id, 2);
        assert_eq!(order.rows[0].bitmask, 1);
        assert_eq!(order.rows[1].external_id, 11);
    }

    #[test]
    fn positional_trim_vertex_table_uses_inherited_entry_class() {
        let payload = b"prefix\xf8\x13\xf7\x44\xfb\xe2\xf7\x45\
            \x09\x0a\x03\x00\xe2";
        let entities = FeatureTrimEntityTable {
            rows: vec![
                FeatureTrimEntity {
                    external_id: 9,
                    mode: Some(0),
                    vertices: [3, 4],
                    center_vertex: None,
                    kind: TrimEntityKind::Line,
                    offset: 0,
                },
                FeatureTrimEntity {
                    external_id: 10,
                    mode: Some(0),
                    vertices: [3, 5],
                    center_vertex: None,
                    kind: TrimEntityKind::Line,
                    offset: 0,
                },
            ],
            solved_external_ids: vec![9, 10],
            offset: 0,
        };

        let vertices = positional_trim_vertex_table(
            payload,
            0,
            payload.len(),
            (68, 69),
            Some(&entities),
            None,
            None,
        )
        .expect("positional vert_tab");

        assert_eq!(vertices.rows.len(), 1);
        assert_eq!(vertices.rows[0].vertex_id, 3);
        assert_eq!(vertices.rows[0].entities, [9, 10]);
    }

    #[test]
    fn trim_vertex_template_identifies_table_and_entry_classes() {
        let payload = b"vert_tab\0\xf8\x13\xf7\x44\xfb\xe2\
            attrs\0\xf1\xf7\x46\xe3\xf8\x01\xf7\x46\xfb\xe3\
            \xf7\x45\x09\x0a\x03\x00";

        assert_eq!(
            trim_table_classes(payload, b"vert_tab\0", 0, payload.len()),
            Some((68, 69))
        );
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
    fn saved_arc_replay_uses_order_table_row_boundaries() {
        let mut payload = vec![0xe3, 7, 0xe2];
        payload.extend([0x0f; 12]);
        payload.push(0xe3);
        let order = FeatureOrderTable {
            declared_count: 1,
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
    fn saved_generated_line_requires_its_orientation_invariant() {
        let payload = [0xe3, 8, 0xe2, 0x0f, 0x0f, 0x0f, 0xe4, 0x0f, 0x0f, 0xe3];
        let order = FeatureOrderTable {
            declared_count: 1,
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
            icon\0protrevolve\0Revolve id 41\0\xe2\xe3Datum Plane id 42\0\xe3K\xc3\xb6rper ID 43\0";
        let operations = operations(payload);
        assert_eq!(operations.len(), 4);
        assert_eq!(operations[0].recipe, Some(FeatureRecipeKind::Extrude));
        assert_eq!(operations[1].recipe, Some(FeatureRecipeKind::Revolve));
        assert_eq!(operations[2].recipe, None);
        assert_eq!(operations[3].kind, "Körper");
        assert_eq!(operations[3].feature_id, 43);
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
        assert_eq!(operations[0].recipe, Some(FeatureRecipeKind::Revolve));
        assert_eq!(operations[0].root_schema_class, Some(917));
        assert_eq!(operations[0].parent_feature_id, Some(32));
        assert_eq!(operations[1].feature_id, 8053);
        assert_eq!(operations[1].recipe, Some(FeatureRecipeKind::Extrude));
        assert_eq!(operations[1].root_schema_class, Some(917));
        assert_eq!(operations[1].parent_feature_id, Some(8051));
    }

    #[test]
    fn promotes_depdb_recipe_without_operation_display_name() {
        let payload = b"\xe3\
            \xf7\x50\x9f\x75\x83\x95\xf6\x9f\x73Profile 1\0\xf6\0protextrude\0";

        let operations = operations(payload);
        assert_eq!(operations.len(), 1);
        assert_eq!(operations[0].feature_id, 8053);
        assert_eq!(operations[0].kind, "Extrude");
        assert_eq!(operations[0].recipe, Some(FeatureRecipeKind::Extrude));
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
}
