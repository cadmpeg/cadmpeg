// SPDX-License-Identifier: Apache-2.0
//! Versioned `native.iges` physical cards and entity records.

use crate::card::{CardScan, ScannedLine, Section};
use crate::decode_resource::{
    collect_result_vec, format_retained, insert_optional_btree_map, insert_optional_btree_set,
    reserve_vec, reserve_vec_growth,
};
use crate::directory::{DirectoryEntry, QuarantinedDirectoryRecord, SourceStatus, UseFlag};
use crate::entities::drawing::drawing_property_value;
use crate::entities::geometry::{
    resolve_transform, BoundaryEndpoint, BoundaryVertexDerivation, TransformResolutionError,
};
use crate::entities::structure::{
    array_base_type, flow_join_target_valid, placement_affine, signal_string_geometry_target,
    PlacementRejection,
};
use crate::global::{RealPrecision, ResolvedGlobal};
use crate::graph::expectation::{ExpectationLabel, ReferenceExpectation};
use crate::graph::{ParameterResolver, ReferenceEdge, ReferenceKind};
use crate::parameter::{
    connect_node_layout, signal_string_layout, text_node_layout, DefaultTailCount, MacroDataError,
    OverdeclaredCount, ParameterRecord, QuarantinedParameterRecord, ResolvedGroups, TextNodeLayout,
    Token, TokenValue, TrailingPointerAnalysis,
};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;
use serde::{Serialize, Serializer};
use std::collections::{BTreeMap, BTreeSet};

mod annotations;
mod fem;

pub(crate) const MAX_PRODUCT_OCCURRENCES: usize = 100_000;
pub(crate) const MAX_PRODUCT_OCCURRENCE_DEPTH: usize = 64;
const DEFAULT_DIMENSION_DISPLAY_CHARACTER_SET: i64 = 1;
const DEFAULT_DIMENSION_DISPLAY_WITNESS_LINE_ANGLE_RAD: f64 = std::f64::consts::FRAC_PI_2;
const DEFAULT_DIMENSION_TOLERANCE_PLACEMENT: i64 = 2;
const DEFAULT_DIMENSION_UNITS_CHARACTER_SET: i64 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProductOccurrenceLimits {
    output: usize,
    depth: usize,
}

impl ProductOccurrenceLimits {
    pub(crate) const fn new(output: usize, depth: usize) -> Self {
        Self { output, depth }
    }
}

struct NativeCard<'a> {
    index: usize,
    line: &'a ScannedLine,
}

struct CardId(usize);

impl std::fmt::Display for CardId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "iges:physical:card#{}", self.0)
    }
}

impl Serialize for CardId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

struct QuarantinedId {
    section: &'static str,
    sequence: u32,
}

impl std::fmt::Display for QuarantinedId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "iges:quarantine:{}#{}",
            self.section, self.sequence
        )
    }
}

impl Serialize for QuarantinedId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl Serialize for NativeCard<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Wire<'a> {
            id: CardId,
            offset: u64,
            payload: &'a [u8],
            line_ending: &'a [u8],
            section: Option<Section>,
            sequence: Option<u32>,
        }
        let (section, sequence) = match self.line {
            ScannedLine::Card {
                section, sequence, ..
            } => (Some(*section), Some(*sequence)),
            ScannedLine::Trailing(_) => (None, None),
        };
        let line = self.line.physical();
        Wire {
            id: CardId(
                self.index
                    .checked_add(1)
                    .ok_or_else(|| serde::ser::Error::custom("IGES card index exceeds usize"))?,
            ),
            offset: line.offset,
            payload: &line.payload,
            line_ending: line.line_ending(),
            section,
            sequence,
        }
        .serialize(serializer)
    }
}

enum NativeQuarantinedRecord<'a> {
    Directory(&'a QuarantinedDirectoryRecord),
    Parameter(&'a QuarantinedParameterRecord),
}

impl Serialize for NativeQuarantinedRecord<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Wire<'a, D: Serialize> {
            id: QuarantinedId,
            section: &'static str,
            sequence: u32,
            source_offset: u64,
            cards: usize,
            bytes: &'a [u8],
            defect: D,
        }
        match self {
            Self::Directory(record) => Wire {
                id: QuarantinedId {
                    section: "directory",
                    sequence: record.sequence,
                },
                section: "directory-entry",
                sequence: record.sequence,
                source_offset: record.source_offset,
                cards: record.cards(),
                bytes: &record.bytes,
                defect: record.defect,
            }
            .serialize(serializer),
            Self::Parameter(record) => Wire {
                id: QuarantinedId {
                    section: "parameter",
                    sequence: record.sequence,
                },
                section: "parameter-data",
                sequence: record.sequence,
                source_offset: record.source_offset(),
                cards: record.cards(),
                bytes: record.bytes(),
                defect: record.defect,
            }
            .serialize(serializer),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeDirection {
    id: String,
    source_entity: String,
    components: Vec<Option<f64>>,
    physically_dependent: bool,
    has_transform: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeFlash {
    id: String,
    source_entity: String,
    form: i64,
    reference_point: [Option<f64>; 2],
    dimension_1: Option<f64>,
    dimension_2: Option<f64>,
    rotation: Option<f64>,
    reference_entity: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeTransformation {
    id: String,
    source_entity: String,
    form: i64,
    coefficients: Vec<Option<f64>>,
    parent: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeCopiousData {
    id: String,
    source_entity: String,
    form: i64,
    interpretation: Option<i64>,
    declared_tuple_count: Option<i64>,
    common_z: Option<f64>,
    tuples: Vec<Vec<Option<f64>>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeBoundaryVertexEndpoint {
    edge: String,
    endpoint: BoundaryEndpoint,
    position: [f64; 3],
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeBoundaryVertex {
    id: String,
    source_entity: String,
    vertex: String,
    representative: [f64; 3],
    tolerance: f64,
    sewn: bool,
    source_endpoints: Vec<NativeBoundaryVertexEndpoint>,
}

fn copious_tuple_layout(form: i64, interpretation: Option<i64>) -> Option<(usize, usize)> {
    let expected = match form {
        1 | 11 | 20 | 21 | 31..=38 | 40 | 63 => 1,
        2 | 12 => 2,
        3 | 13 => 3,
        _ => return None,
    };
    match (expected, interpretation) {
        (1, Some(1)) => Some((4, 2)),
        (2, Some(2)) => Some((3, 3)),
        (3, Some(3)) => Some((3, 6)),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeColorDefinition {
    id: String,
    source_entity: String,
    red_percent: Option<f64>,
    green_percent: Option<f64>,
    blue_percent: Option<f64>,
    name: Option<Vec<u8>>,
    fallback_color_number: i64,
}

#[derive(Debug, Clone, PartialEq)]
struct NativeDisplayAttributes {
    id: String,
    source_entity: String,
    visible: bool,
    line_font: DisplayRef,
    level: DisplayRef,
    view: i64,
    line_weight_number: i64,
    line_weight_mm: Option<f64>,
    color: DisplayRef,
}

#[derive(Debug, Clone, PartialEq)]
enum DisplayRef {
    Number(u64),
    Definition {
        pointer: i64,
        target: Option<String>,
    },
}

impl DisplayRef {
    fn definition(&self) -> Option<&str> {
        match self {
            Self::Number(_) => None,
            Self::Definition { target, .. } => target.as_deref(),
        }
    }
}

impl Serialize for DisplayRef {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Number(number) => serializer.serialize_u64(*number),
            Self::Definition { pointer, .. } => serializer.serialize_i64(*pointer),
        }
    }
}

impl Serialize for NativeDisplayAttributes {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Wire<'a> {
            id: &'a str,
            source_entity: &'a str,
            visible: bool,
            line_font_number: &'a DisplayRef,
            line_font_definition: Option<&'a str>,
            level_number: &'a DisplayRef,
            level_definition: Option<&'a str>,
            view: i64,
            line_weight_number: i64,
            line_weight_mm: Option<f64>,
            color_number: &'a DisplayRef,
            color_definition: Option<&'a str>,
        }
        Wire {
            id: &self.id,
            source_entity: &self.source_entity,
            visible: self.visible,
            line_font_number: &self.line_font,
            line_font_definition: self.line_font.definition(),
            level_number: &self.level,
            level_definition: self.level.definition(),
            view: self.view,
            line_weight_number: self.line_weight_number,
            line_weight_mm: self.line_weight_mm,
            color_number: &self.color,
            color_definition: self.color.definition(),
        }
        .serialize(serializer)
    }
}

fn resolve_display_ref(
    ctx: &DecodeContext<'_>,
    references: &BTreeMap<u32, Vec<ReferenceEdge>>,
    source_sequence: u32,
    pointer: i64,
    kind: ReferenceKind,
    arena: &str,
) -> Result<DisplayRef, CodecError> {
    if pointer >= 0 {
        return Ok(DisplayRef::Number(pointer.unsigned_abs()));
    }
    let target = references
        .get(&source_sequence)
        .and_then(|references| {
            references
                .iter()
                .find_map(|reference| reference.resolved_target_sequence_for(kind))
        })
        .map(|sequence| {
            format_retained(
                ctx,
                format_args!("iges:presentation:{arena}#D{sequence}"),
                "iges native display definition",
            )
        })
        .transpose()?;
    Ok(DisplayRef::Definition { pointer, target })
}

fn resolved_label_display_definition(
    ctx: &DecodeContext<'_>,
    references: &BTreeMap<u32, Vec<ReferenceEdge>>,
    source_sequence: u32,
    pointer: i64,
) -> Result<Option<String>, CodecError> {
    (pointer > 0)
        .then(|| {
            references
                .get(&source_sequence)?
                .iter()
                .find_map(|reference| {
                    reference.resolved_target_sequence_for(ReferenceKind::LabelDisplay)
                })
        })
        .flatten()
        .map(|sequence| {
            format_retained(
                ctx,
                format_args!("iges:structure:associativity#D{sequence}"),
                "iges native label display definition",
            )
        })
        .transpose()
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum NativeLineFontDefinition {
    Template {
        id: String,
        source_entity: String,
        fallback_line_font_number: i64,
        tangent_oriented: Option<bool>,
        template: Option<String>,
        spacing: Option<f64>,
        scale: Option<f64>,
    },
    VisibleBlankPattern {
        id: String,
        source_entity: String,
        fallback_line_font_number: i64,
        segment_count: Option<i64>,
        lengths: Vec<Option<f64>>,
        hexadecimal_pattern: Option<Vec<u8>>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeTextDisplayTemplate {
    id: String,
    source_entity: String,
    form: i64,
    character_box: [Option<f64>; 2],
    font_code: Option<i64>,
    font_definition: Option<String>,
    slant_angle: Option<f64>,
    rotation_angle: Option<f64>,
    mirror: Option<i64>,
    vertical: Option<i64>,
    origin_or_increment: [Option<f64>; 3],
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeGlyphMotion {
    pen_up: Option<bool>,
    point: [Option<i64>; 2],
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeGlyph {
    character_code: Option<i64>,
    next_origin: [Option<i64>; 2],
    declared_motion_count: Option<i64>,
    motions: Vec<NativeGlyphMotion>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeTextFontDefinition {
    id: String,
    source_entity: String,
    font_code: Option<i64>,
    name: Option<Vec<u8>>,
    supersedes_code: Option<i64>,
    supersedes_definition: Option<String>,
    grid_units_per_text_height: Option<i64>,
    declared_character_count: Option<i64>,
    characters: Vec<NativeGlyph>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeDefinitionLevels {
    id: String,
    source_entity: String,
    declared_count: Option<i64>,
    levels: Vec<Option<i64>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum PrimitiveSolidKind {
    Block,
    RightAngularWedge,
    RightCircularCylinder,
    RightCircularConeFrustum,
    Sphere,
    Torus,
    Ellipsoid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum ProceduralSolidKind {
    Revolution,
    LinearExtrusion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum ExternalReferenceKind {
    ExternalDefinition,
    ExternalFileDefinition,
    ExternalLogical,
    NativeDefinition,
    NativeLibraryDefinition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum ProductPropertyKind {
    ReferenceDesignator,
    Name,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum ViewProjection {
    OrthographicParallel,
    Perspective,
}

impl ExternalReferenceKind {
    const fn form(self) -> i64 {
        match self {
            Self::ExternalDefinition => 0,
            Self::ExternalFileDefinition => 1,
            Self::ExternalLogical => 2,
            Self::NativeDefinition => 3,
            Self::NativeLibraryDefinition => 4,
        }
    }
}

impl ProductPropertyKind {
    const fn form(self) -> i64 {
        match self {
            Self::ReferenceDesignator => 7,
            Self::Name => 15,
        }
    }
}

impl ViewProjection {
    const fn form(self) -> i64 {
        match self {
            Self::OrthographicParallel => 0,
            Self::Perspective => 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativePrimitiveSolid {
    id: String,
    source_entity: String,
    kind: PrimitiveSolidKind,
    dimensions: BTreeMap<String, Option<f64>>,
    origin: [Option<f64>; 3],
    x_axis: Option<[Option<f64>; 3]>,
    z_axis: Option<[Option<f64>; 3]>,
    transformation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeProceduralSolid {
    id: String,
    source_entity: String,
    kind: ProceduralSolidKind,
    form: i64,
    profile: Option<String>,
    amount: Option<f64>,
    origin: Option<[Option<f64>; 3]>,
    direction: [Option<f64>; 3],
    transformation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum NativeBooleanTerm {
    Operand { entity: Option<String>, raw: i64 },
    Operation { operation: i64 },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeBooleanTree {
    id: String,
    source_entity: String,
    form: i64,
    declared_length: Option<i64>,
    terms: Vec<NativeBooleanTerm>,
    transformation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeSelectedComponent {
    id: String,
    source_entity: String,
    boolean_tree: Option<String>,
    selection_point: [Option<f64>; 3],
    transformation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeAssemblyItem {
    item: Option<String>,
    transformation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeSolidAssembly {
    id: String,
    source_entity: String,
    form: i64,
    declared_count: Option<i64>,
    items: Vec<NativeAssemblyItem>,
    transformation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeVoidShell {
    shell: Option<String>,
    orientation: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeManifoldSolid {
    id: String,
    source_entity: String,
    shell: Option<String>,
    shell_orientation: Option<i64>,
    declared_void_count: Option<i64>,
    voids: Vec<NativeVoidShell>,
    transformation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeSolidInstance {
    id: String,
    source_entity: String,
    form: i64,
    solid: Option<String>,
    transformation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeSubfigureDefinition {
    id: String,
    source_entity: String,
    depth: Option<i64>,
    name: Option<Vec<u8>>,
    declared_member_count: Option<i64>,
    members: Vec<Option<String>>,
    transformation: Option<String>,
    label_display: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeSubfigureInstance {
    id: String,
    source_entity: String,
    definition: Option<String>,
    translation: [Option<f64>; 3],
    scale: Option<f64>,
    transformation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeNetworkDefinition {
    id: String,
    source_entity: String,
    depth: Option<i64>,
    name: Option<Vec<u8>>,
    declared_member_count: Option<i64>,
    members: Vec<Option<String>>,
    type_flag: Option<i64>,
    primary_reference_designator: Option<Vec<u8>>,
    display_template: Option<String>,
    declared_connect_point_count: Option<i64>,
    connect_points: Vec<Option<String>>,
    transformation: Option<String>,
    label_display: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeNetworkInstance {
    id: String,
    source_entity: String,
    definition: Option<String>,
    translation: [Option<f64>; 3],
    scale: [Option<f64>; 3],
    type_flag: Option<i64>,
    primary_reference_designator: Option<Vec<u8>>,
    display_template: Option<String>,
    declared_connect_point_count: Option<i64>,
    connect_points: Vec<Option<String>>,
    transformation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeConnectPoint {
    id: String,
    source_entity: String,
    position: [Option<f64>; 3],
    display_geometry: Option<String>,
    type_flag: Option<i64>,
    function_flag: Option<i64>,
    function_identifier: Option<Vec<u8>>,
    identifier_display_template: Option<String>,
    function_name: Option<Vec<u8>>,
    name_display_template: Option<String>,
    identifier: Option<i64>,
    function_code: Option<i64>,
    swap_flag: Option<i64>,
    owner: Option<String>,
    transformation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeRectangularArray {
    id: String,
    source_entity: String,
    base: Option<String>,
    scale: Option<f64>,
    origin: [Option<f64>; 3],
    columns: Option<i64>,
    rows: Option<i64>,
    column_spacing: Option<f64>,
    row_spacing: Option<f64>,
    rotation: Option<f64>,
    do_dont_flag: Option<i64>,
    positions: Vec<Option<i64>>,
    transformation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeCircularArray {
    id: String,
    source_entity: String,
    base: Option<String>,
    location_count: Option<i64>,
    center: [Option<f64>; 3],
    radius: Option<f64>,
    start_angle: Option<f64>,
    delta_angle: Option<f64>,
    do_dont_flag: Option<i64>,
    positions: Vec<Option<i64>>,
    transformation: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
struct NativeExternalReference {
    id: String,
    source_entity: String,
    reference_kind: ExternalReferenceKind,
    file_identifier: Option<Vec<u8>>,
    symbolic_name: Option<Vec<u8>>,
    library_name: Option<Vec<u8>>,
}

impl Serialize for NativeExternalReference {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Wire<'a> {
            id: &'a str,
            source_entity: &'a str,
            form: i64,
            reference_kind: &'a ExternalReferenceKind,
            file_identifier: &'a Option<Vec<u8>>,
            symbolic_name: &'a Option<Vec<u8>>,
            library_name: &'a Option<Vec<u8>>,
            resolution_state: &'static str,
        }
        Wire {
            id: &self.id,
            source_entity: &self.source_entity,
            form: self.reference_kind.form(),
            reference_kind: &self.reference_kind,
            file_identifier: &self.file_identifier,
            symbolic_name: &self.symbolic_name,
            library_name: &self.library_name,
            resolution_state: "not_attempted",
        }
        .serialize(serializer)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeGroup {
    id: String,
    source_entity: String,
    ordered: bool,
    back_pointers_required: bool,
    declared_member_count: Option<i64>,
    members: Vec<Option<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeAssociativityClassDefinition {
    back_pointers_required: Option<bool>,
    ordered: Option<bool>,
    declared_item_count: Option<i64>,
    item_types: Vec<Option<i64>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeLabelPlacement {
    view: Option<String>,
    text_location: [Option<f64>; 3],
    leader: Option<String>,
    label_level: Option<i64>,
    entity: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeExternalIndexEntry {
    symbolic_name: Option<Vec<u8>>,
    entity: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeDimensionGeometryItem {
    geometry: Option<String>,
    location_flag: Option<i64>,
    point: [Option<f64>; 3],
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum NativeAssociativity {
    Definition {
        id: String,
        source_entity: String,
        associativity_form: i64,
        declared_class_count: Option<i64>,
        classes: Vec<NativeAssociativityClassDefinition>,
    },
    LabelDisplay {
        id: String,
        source_entity: String,
        declared_count: Option<i64>,
        placements: Vec<NativeLabelPlacement>,
    },
    ViewList {
        id: String,
        source_entity: String,
        declared_visible_count: Option<i64>,
        view: Option<String>,
        visible_entities: Vec<Option<String>>,
    },
    SingleParent {
        id: String,
        source_entity: String,
        declared_child_count: Option<i64>,
        parent: Option<String>,
        children: Vec<Option<String>>,
    },
    ExternalReferenceIndex {
        id: String,
        source_entity: String,
        declared_count: Option<i64>,
        entries: Vec<NativeExternalIndexEntry>,
    },
    LegacySignalString {
        id: String,
        source_entity: String,
        declared_signal_name_count: Option<i64>,
        declared_connection_count: Option<i64>,
        declared_schematic_count: Option<i64>,
        declared_physical_count: Option<i64>,
        signal_names: Vec<Option<Vec<u8>>>,
        connections: Vec<Option<String>>,
        schematic_entities: Vec<Option<String>>,
        physical_entities: Vec<Option<String>>,
    },
    LegacyTextNode {
        id: String,
        source_entity: String,
        declared_geometry_count: Option<i64>,
        declared_text_description_count: Option<i64>,
        geometry: Vec<Option<String>>,
        box_width: Option<f64>,
        box_height: Option<f64>,
        font_characteristic: Option<i64>,
        font_definition: Option<String>,
        slant_angle: Option<f64>,
        rotation_angle: Option<f64>,
        mirror_flag: Option<i64>,
        rotate_internal_flag: Option<i64>,
    },
    LegacyConnectNode {
        id: String,
        source_entity: String,
        declared_point_count: Option<i64>,
        declared_data_count: Option<i64>,
        points: Vec<Option<String>>,
        data: Vec<TokenValue>,
    },
    DimensionedGeometry {
        id: String,
        source_entity: String,
        declared_geometry_count: Option<i64>,
        dimension: Option<String>,
        geometry: Vec<Option<String>>,
    },
    Planar {
        id: String,
        source_entity: String,
        declared_entity_count: Option<i64>,
        plane_transform: Option<String>,
        entities: Vec<Option<String>>,
    },
    Flow {
        id: String,
        source_entity: String,
        form: i64,
        declared_associated_flow_count: Option<i64>,
        declared_connection_count: Option<i64>,
        declared_join_count: Option<i64>,
        declared_name_count: Option<i64>,
        declared_name_display_count: Option<i64>,
        declared_continuation_count: Option<i64>,
        type_flag: Option<i64>,
        function_flag: Option<i64>,
        associated_flows: Vec<Option<String>>,
        connections: Vec<Option<String>>,
        joins: Vec<Option<String>>,
        names: Vec<Option<Vec<u8>>>,
        name_displays: Vec<Option<String>>,
        continuations: Vec<Option<String>>,
    },
    RecalculableDimension {
        id: String,
        source_entity: String,
        declared_geometry_count: Option<i64>,
        dimension: Option<String>,
        orientation_flag: Option<i64>,
        angle: Option<f64>,
        geometry: Vec<NativeDimensionGeometryItem>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeAttributeValue {
    value: TokenValue,
    display_template: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeAttributeDefinition {
    attribute_type: Option<i64>,
    value_data_type: Option<i64>,
    declared_value_count: Option<i64>,
    values: Vec<NativeAttributeValue>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeAttributeTableDefinition {
    id: String,
    source_entity: String,
    form: i64,
    name: Option<Vec<u8>>,
    attribute_list_type: Option<i64>,
    declared_attribute_count: Option<i64>,
    attributes: Vec<NativeAttributeDefinition>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeAttributeTableInstance {
    id: String,
    source_entity: String,
    form: i64,
    definition: Option<String>,
    declared_row_count: Option<i64>,
    rows: Vec<Vec<TokenValue>>,
}

#[derive(Debug, Clone, PartialEq)]
struct NativeProductProperty {
    id: String,
    source_entity: String,
    property_kind: ProductPropertyKind,
    value: Option<Vec<u8>>,
    owners: Vec<String>,
}

impl Serialize for NativeProductProperty {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Wire<'a> {
            id: &'a String,
            source_entity: &'a String,
            form: i64,
            property_kind: &'a ProductPropertyKind,
            value: &'a Option<Vec<u8>>,
            owners: &'a Vec<String>,
        }
        Wire {
            id: &self.id,
            source_entity: &self.source_entity,
            form: self.property_kind.form(),
            property_kind: &self.property_kind,
            value: &self.value,
            owners: &self.owners,
        }
        .serialize(serializer)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "property_kind", rename_all = "snake_case")]
enum NativePropertyValue {
    RegionRestriction {
        electrical_vias: Option<i64>,
        electrical_components: Option<i64>,
        electrical_circuitry: Option<i64>,
    },
    LevelFunction {
        function_code: Option<i64>,
        description: Option<Vec<u8>>,
    },
    RegionFill {
        fill_code: Option<i64>,
        obsolete_pointer: Option<i64>,
    },
    LineWidening {
        width: Option<f64>,
        cornering: Option<i64>,
        extension_flag: Option<i64>,
        justification: Option<i64>,
        extension: Option<f64>,
    },
    DrilledHole {
        drill_diameter: Option<f64>,
        finished_diameter: Option<f64>,
        plated: Option<i64>,
        lower_layer: Option<i64>,
        upper_layer: Option<i64>,
    },
    ReferenceDesignator {
        value: Option<Vec<u8>>,
    },
    PinNumber {
        value: Option<Vec<u8>>,
    },
    PartNumber {
        generic: Option<Vec<u8>>,
        military: Option<Vec<u8>>,
        vendor: Option<Vec<u8>>,
        internal: Option<Vec<u8>>,
    },
    Hierarchy {
        line_font: Option<i64>,
        view: Option<i64>,
        level: Option<i64>,
        blank: Option<i64>,
        line_weight: Option<i64>,
        color: Option<i64>,
    },
    ExternalReferenceFileList {
        names: Vec<Option<Vec<u8>>>,
    },
    NominalSize {
        size: Option<f64>,
        name: Option<Vec<u8>>,
        standard: Option<Vec<u8>>,
    },
    FlowLineSpecification {
        values: Vec<Option<Vec<u8>>>,
    },
    Name {
        value: Option<Vec<u8>>,
    },
    IntercharacterSpacing {
        percent: Option<f64>,
    },
    LineFont {
        pattern_code: Option<i64>,
    },
    Highlight {
        highlighted: Option<bool>,
    },
    Pick {
        pickable: Option<bool>,
    },
    UniformRectangularGrid {
        finite: Option<bool>,
        lines: Option<bool>,
        weighted: Option<bool>,
        origin: [Option<f64>; 2],
        spacing: [Option<f64>; 2],
        counts: [Option<i64>; 2],
    },
    AssociativityGroupType {
        associativity_type: Option<i64>,
        name: Option<Vec<u8>>,
    },
    LevelToLepLayerMap {
        definitions: Vec<NativeLepLayerDefinition>,
    },
    LepArtworkStackup {
        identification: Option<Vec<u8>>,
        levels: Vec<Option<i64>>,
    },
    LepDrilledHole {
        drill_diameter: Option<f64>,
        finished_diameter: Option<f64>,
        function_code: Option<i64>,
    },
    TabularData {
        property_type: Option<i64>,
        declared_dependent_count: Option<i64>,
        independent_variables: Vec<NativeIndependentVariable>,
        dependent_values: Vec<Option<f64>>,
    },
    GenericData {
        name: Option<Vec<u8>>,
        values: Vec<NativeGenericPropertyValue>,
    },
    DimensionUnits {
        secondary_position: Option<i64>,
        units_indicator: Option<i64>,
        character_set: Option<i64>,
        suffix: Option<Vec<u8>>,
        fraction_flag: Option<i64>,
        precision: Option<i64>,
    },
    DimensionTolerance {
        secondary_flag: Option<i64>,
        tolerance_type: Option<i64>,
        placement: Option<i64>,
        upper: Option<f64>,
        lower: Option<f64>,
        suppress_plus: Option<bool>,
        fraction_flag: Option<i64>,
        precision: Option<i64>,
    },
    DimensionDisplayData {
        dimension_type: Option<i64>,
        label_position: Option<i64>,
        declared_character_set: Option<i64>,
        character_set: Option<i64>,
        label: Option<Vec<u8>>,
        decimal_symbol: Option<i64>,
        declared_witness_line_angle: Option<f64>,
        witness_line_angle: Option<f64>,
        text_alignment: Option<i64>,
        text_level: Option<i64>,
        text_placement: Option<i64>,
        arrow_orientation: Option<i64>,
        initial_value: Option<f64>,
        supplemental_notes: Vec<NativeSupplementalNote>,
    },
    BasicDimension {
        corners: Vec<[Option<f64>; 2]>,
    },
    DrawingSheetApproval {
        name: Option<Vec<u8>>,
        organization: Option<Vec<u8>>,
        date: Option<Vec<u8>>,
    },
    DrawingSheetId {
        sheet_number: Option<i64>,
        revision: Option<Vec<u8>>,
    },
    Underscore {
        ranges: Vec<NativeTextScoreRange>,
    },
    Overscore {
        ranges: Vec<NativeTextScoreRange>,
    },
    Closure {
        u: Option<i64>,
        v: Option<i64>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeTextScoreRange {
    text_index: Option<i64>,
    first_character: Option<i64>,
    last_character: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeSupplementalNote {
    position: Option<i64>,
    first_text: Option<i64>,
    last_text: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeIndependentVariable {
    variable_type: Option<i64>,
    declared_value_count: Option<i64>,
    values: Vec<Option<f64>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeGenericPropertyValue {
    data_type: Option<i64>,
    value: TokenValue,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeLepLayerDefinition {
    exchange_level: Option<i64>,
    native_identifier: Option<Vec<u8>>,
    physical_layer: Option<i64>,
    functional_identifier: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeProperty {
    id: String,
    source_entity: String,
    form: i64,
    declared_value_count: Option<i64>,
    owners: Vec<String>,
    #[serde(flatten)]
    value: NativePropertyValue,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeUnitDefinition {
    unit_type: Option<Vec<u8>>,
    unit_value: Option<Vec<u8>>,
    scale_factor: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeUnitsData {
    id: String,
    source_entity: String,
    declared_count: Option<i64>,
    units: Vec<NativeUnitDefinition>,
    owners: Vec<String>,
}

/// The product-occurrence record and the one route that builds it.
///
/// The fields are private to this module, so `role` and `instance_path` cannot
/// be set independently: [`NativeProductOccurrence::new`] derives both from the
/// instance path and the optional member, and the id from the same path.
mod occurrence {
    use super::{collect_result_vec, format_retained, CodecError, DecodeContext};
    use serde::{Serialize, Serializer};

    /// What one product occurrence record is: the assembly root, a nested
    /// occurrence, or one member of an occurrence's definition.
    #[derive(Debug, Clone, PartialEq, Eq)]
    enum OccurrenceRole {
        Root,
        Nested,
        Member(String),
    }

    impl OccurrenceRole {
        const fn is_root(&self) -> bool {
            matches!(self, Self::Root)
        }

        fn member(&self) -> Option<&str> {
            match self {
                Self::Root | Self::Nested => None,
                Self::Member(member) => Some(member),
            }
        }
    }

    #[derive(Debug, Clone, PartialEq)]
    pub(super) struct NativeProductOccurrence {
        id: String,
        role: OccurrenceRole,
        source_instance: String,
        definition: String,
        neutral_links: Vec<String>,
        instance_path: Vec<String>,
        local_transform: [[f64; 4]; 3],
        world_transform: [[f64; 4]; 3],
    }

    struct OccurrencePath<'a>(&'a [u32]);

    impl std::fmt::Display for OccurrencePath<'_> {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            for (index, sequence) in self.0.iter().enumerate() {
                if index != 0 {
                    formatter.write_str("/")?;
                }
                write!(formatter, "{sequence}")?;
            }
            Ok(())
        }
    }

    impl NativeProductOccurrence {
        /// The occurrence reached by `path`, or one member of its definition.
        ///
        /// The assembly root is the occurrence at the head of the path that
        /// names no member; every other record is nested or a member.
        pub(super) fn new(
            ctx: &DecodeContext<'_>,
            path: &[u32],
            member: Option<u32>,
            instance_sequence: u32,
            definition_sequence: u32,
            neutral_links: Vec<String>,
            frames: ([[f64; 4]; 3], [[f64; 4]; 3]),
        ) -> Result<Self, CodecError> {
            let (local_transform, world_transform) = frames;
            let path_key = OccurrencePath(path);
            let (id, role) = match member {
                Some(member) => (
                    format_retained(
                        ctx,
                        format_args!("iges:product:occurrence#{path_key}/D{member}"),
                        "iges native occurrence id",
                    )?,
                    OccurrenceRole::Member(format_retained(
                        ctx,
                        format_args!("iges:entity:directory#{member}"),
                        "iges native occurrence member",
                    )?),
                ),
                None if path.len() == 1 => (
                    format_retained(
                        ctx,
                        format_args!("iges:product:occurrence#{path_key}"),
                        "iges native occurrence id",
                    )?,
                    OccurrenceRole::Root,
                ),
                None => (
                    format_retained(
                        ctx,
                        format_args!("iges:product:occurrence#{path_key}"),
                        "iges native occurrence id",
                    )?,
                    OccurrenceRole::Nested,
                ),
            };
            Ok(Self {
                id,
                role,
                source_instance: format_retained(
                    ctx,
                    format_args!("iges:entity:directory#{instance_sequence}"),
                    "iges native occurrence instance",
                )?,
                definition: format_retained(
                    ctx,
                    format_args!("iges:entity:directory#{definition_sequence}"),
                    "iges native occurrence definition",
                )?,
                neutral_links,
                instance_path: collect_result_vec(
                    ctx,
                    path.len(),
                    "iges native occurrence path slots",
                    |index| {
                        format_retained(
                            ctx,
                            format_args!("iges:entity:directory#{}", path[index]),
                            "iges native occurrence path entry",
                        )
                    },
                )?,
                local_transform,
                world_transform,
            })
        }
    }

    impl Serialize for NativeProductOccurrence {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            #[derive(Serialize)]
            struct Wire<'a> {
                id: &'a str,
                root: bool,
                source_instance: &'a str,
                definition: &'a str,
                member: Option<&'a str>,
                neutral_links: &'a [String],
                instance_path: &'a [String],
                local_transform: [[f64; 4]; 3],
                world_transform: [[f64; 4]; 3],
            }
            Wire {
                id: &self.id,
                root: self.role.is_root(),
                source_instance: &self.source_instance,
                definition: &self.definition,
                member: self.role.member(),
                neutral_links: &self.neutral_links,
                instance_path: &self.instance_path,
                local_transform: self.local_transform,
                world_transform: self.world_transform,
            }
            .serialize(serializer)
        }
    }
}

use occurrence::NativeProductOccurrence;

#[derive(Debug, Clone, PartialEq, Eq)]
struct NativeProductOccurrenceExpansion {
    id: String,
    output_limit: usize,
    depth_limit: usize,
    emitted: usize,
    issues: Vec<ProductOccurrenceIssue>,
}

impl Serialize for NativeProductOccurrenceExpansion {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Wire<'a> {
            id: &'a str,
            output_limit: usize,
            depth_limit: usize,
            emitted: usize,
            truncated: bool,
            issues: &'a [ProductOccurrenceIssue],
        }
        Wire {
            id: &self.id,
            output_limit: self.output_limit,
            depth_limit: self.depth_limit,
            emitted: self.emitted,
            truncated: !self.issues.is_empty(),
            issues: &self.issues,
        }
        .serialize(serializer)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum ProductOccurrenceIssue {
    OutputLimit,
    DepthLimit,
    MalformedDefinition,
    MalformedPlacement,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProductOccurrenceExpansion {
    pub(crate) output_truncated_at: Option<u32>,
    pub(crate) depth_truncated_at: Option<u32>,
    pub(crate) malformed_definition_sequences: Vec<u32>,
    pub(crate) malformed_placement_sequences: Vec<u32>,
}

/// The two quarantine lists a decode carries into the native store.
#[derive(Debug, Clone, Copy)]
pub(crate) struct QuarantinedRecords<'a> {
    pub(crate) directory: &'a [QuarantinedDirectoryRecord],
    pub(crate) parameters: &'a [QuarantinedParameterRecord],
}

pub(crate) struct AmbiguousParameterBoundary {
    pub(crate) sequence: u32,
    pub(crate) ambiguity: ParameterBoundaryAmbiguity,
}

pub(crate) enum ParameterBoundaryAmbiguity {
    EquallyValid(usize),
    Structural(usize),
}

/// Why one Type 422 attribute-table instance states no readable row grid.
///
/// Each variant is a refusal that names the lane the instance failed in, not a
/// default: no row is read and the instance's loss states which count could
/// not be used. None of them stands for a table that holds no row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnstatableAttributeTable {
    /// The definition record does not state how many attributes the table has.
    AttributeCount,
    /// The definition record does not state a usable value count for one
    /// attribute: the count field lies past the record's parameter end, or the
    /// record states it as a negative integer, a real or a string.
    ValueCount { attribute: usize },
    /// The per-attribute value counts sum past what `usize` can state.
    ValueTotal,
    /// The instance record does not state a row count.
    RowCount,
    /// The instance record's primary parameters do not hold the rows it
    /// declares.
    RowsNotHeld { declared: usize, available: usize },
}

impl UnstatableAttributeTable {
    /// The clause naming this refusal in the instance's loss message.
    pub(crate) fn reason(self) -> String {
        match self {
            Self::AttributeCount => {
                "states no attribute count in its attribute-table definition".to_owned()
            }
            Self::ValueCount { attribute } => format!(
                "states no value count for attribute {attribute} in its attribute-table definition"
            ),
            Self::ValueTotal => {
                "states per-attribute value counts that sum past an addressable row".to_owned()
            }
            Self::RowCount => "states no row count".to_owned(),
            Self::RowsNotHeld {
                declared,
                available,
            } => format!(
                "declares {declared} attribute rows; its Parameter Data record holds {available} values"
            ),
        }
    }
}

/// The admitted value grid of one Type 422 attribute-table instance.
///
/// `values` holds exactly `rows * values_per_row` tokens of the instance
/// record's primary parameters.
struct AttributeTableRows<'a> {
    values: &'a [crate::parameter::Token],
    values_per_row: std::num::NonZeroUsize,
}

/// The value grid one Type 422 attribute-table instance states.
///
/// Every count on this route is read from the file: the attribute count and
/// the per-attribute value counts from the Type 322 definition record, the row
/// count from the instance record itself. A count the record does not state,
/// states as a negative integer, a real or a string, or states past what the
/// record holds is refused by name, never read as a zero count standing for an
/// empty table. `Ok(None)` states the two tables the format itself says hold no
/// value: one that resolves to no attribute-table definition, and one whose
/// attributes together state no value.
fn attribute_table_rows<'a>(
    form: i64,
    record: &'a ParameterRecord,
    instance_end: usize,
    definition: Option<(&ParameterRecord, usize, usize)>,
    value_start: usize,
) -> Result<Option<AttributeTableRows<'a>>, UnstatableAttributeTable> {
    let Some((definition_record, stride, definition_end)) = definition else {
        return Ok(None);
    };
    let Some(attribute_count) =
        definition_record.count_with_stride_before(3, stride, definition_end)
    else {
        return Err(UnstatableAttributeTable::AttributeCount);
    };
    let mut values_per_row = 0_usize;
    for attribute in 0..attribute_count {
        let count_index = 6 + attribute * 3;
        let declared = match definition_record.value(count_index) {
            // The definition omits this attribute's count field, or the field
            // lies past the record's own parameter end. `integer_or` states
            // the format's default of one value for the first and states
            // nothing for the second.
            None | Some(TokenValue::Omitted) => definition_record
                .integer_or(count_index, 1)
                .ok_or(UnstatableAttributeTable::ValueCount { attribute })?,
            Some(TokenValue::Integer(value)) => *value,
            Some(TokenValue::Real(_) | TokenValue::String(_)) => {
                return Err(UnstatableAttributeTable::ValueCount { attribute })
            }
        };
        let declared = usize::try_from(declared)
            .map_err(|_| UnstatableAttributeTable::ValueCount { attribute })?;
        values_per_row = values_per_row
            .checked_add(declared)
            .ok_or(UnstatableAttributeTable::ValueTotal)?;
    }
    let Some(values_per_row) = std::num::NonZeroUsize::new(values_per_row) else {
        return Ok(None);
    };
    let declared_rows = if form == 0 {
        // A Form 0 instance states one row of the definition's values.
        1
    } else {
        let declared = record
            .integer(1)
            .ok_or(UnstatableAttributeTable::RowCount)?;
        usize::try_from(declared).map_err(|_| UnstatableAttributeTable::RowCount)?
    };
    let primary = match record.tokens().get(value_start..instance_end) {
        Some(primary) => primary,
        // The record's primary parameters end before its first value, so it
        // states no attribute value.
        None => &[],
    };
    let admitted = declared_rows
        .checked_mul(values_per_row.get())
        .and_then(|required| primary.get(..required));
    let Some(values) = admitted else {
        return Err(UnstatableAttributeTable::RowsNotHeld {
            declared: declared_rows,
            available: primary.len(),
        });
    };
    Ok(Some(AttributeTableRows {
        values,
        values_per_row,
    }))
}

/// Collects at most one overdeclared-count verdict per Directory Entry. The
/// first verdict a record earns is the one its loss reports.
#[derive(Default)]
struct OverdeclaredCounts(BTreeMap<u32, OverdeclaredCount>);

impl OverdeclaredCounts {
    fn counted_tail(
        &mut self,
        sequence: u32,
        record: Option<&ParameterRecord>,
        end: usize,
        index: usize,
        stride: usize,
    ) -> usize {
        self.admit(
            sequence,
            record.map_or(DefaultTailCount::Unreadable, |record| {
                record.count_with_stride_before_default_tail(index, stride, end)
            }),
        )
    }

    fn counted_tail_at(
        &mut self,
        sequence: u32,
        record: Option<&ParameterRecord>,
        end: usize,
        index: usize,
        item_start: usize,
        stride: usize,
    ) -> usize {
        self.admit(
            sequence,
            record.map_or(DefaultTailCount::Unreadable, |record| {
                record.count_with_stride_before_default_tail_at(index, item_start, stride, end)
            }),
        )
    }

    fn counted_complete(
        &mut self,
        sequence: u32,
        record: Option<&ParameterRecord>,
        end: usize,
        index: usize,
        item_start: usize,
        stride: usize,
    ) -> usize {
        self.admit(
            sequence,
            record.map_or(DefaultTailCount::Unreadable, |record| {
                record.count_with_stride_at_complete(index, item_start, stride, end)
            }),
        )
    }

    fn admit(&mut self, sequence: u32, verdict: DefaultTailCount) -> usize {
        match verdict {
            DefaultTailCount::Held(count) => count,
            DefaultTailCount::Overdeclared(count) => {
                self.0.entry(sequence).or_insert(count);
                0
            }
            DefaultTailCount::Unreadable => 0,
        }
    }
}

pub(crate) struct NativeStoreResult {
    pub(crate) occurrence_expansion: ProductOccurrenceExpansion,
    pub(crate) ambiguous_parameter_boundaries: Vec<AmbiguousParameterBoundary>,
    pub(crate) overdeclared_counts: BTreeMap<u32, OverdeclaredCount>,
    pub(crate) unstatable_attribute_tables: BTreeMap<u32, UnstatableAttributeTable>,
}

#[derive(Debug, Clone, PartialEq)]
struct NativeView {
    id: String,
    source_entity: String,
    view_number: Option<i64>,
    scale: Option<f64>,
    geometry: ViewGeometry,
}

#[derive(Debug, Clone, PartialEq)]
enum ViewGeometry {
    Orthographic {
        model_to_view: Option<String>,
        clipping_planes: Vec<Option<String>>,
    },
    Perspective(Box<PerspectiveViewGeometry>),
}

#[derive(Debug, Clone, PartialEq)]
struct PerspectiveViewGeometry {
    view_plane_normal: [Option<f64>; 3],
    view_reference_point: [Option<f64>; 3],
    center_of_projection: [Option<f64>; 3],
    view_up: [Option<f64>; 3],
    view_plane_distance: Option<f64>,
    clipping_window: [Option<f64>; 4],
    depth_clipping: Option<i64>,
    depth_range: [Option<f64>; 2],
}

impl ViewGeometry {
    fn projection(&self) -> ViewProjection {
        match self {
            Self::Orthographic { .. } => ViewProjection::OrthographicParallel,
            Self::Perspective(_) => ViewProjection::Perspective,
        }
    }
}

impl Serialize for NativeView {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct OrthographicWire<'a> {
            id: &'a str,
            source_entity: &'a str,
            form: i64,
            projection: ViewProjection,
            view_number: Option<i64>,
            scale: Option<f64>,
            model_to_view: &'a Option<String>,
            clipping_planes: &'a [Option<String>],
            view_plane_normal: Option<[Option<f64>; 3]>,
            view_reference_point: Option<[Option<f64>; 3]>,
            center_of_projection: Option<[Option<f64>; 3]>,
            view_up: Option<[Option<f64>; 3]>,
            view_plane_distance: Option<f64>,
            clipping_window: Option<[Option<f64>; 4]>,
            depth_clipping: Option<i64>,
            depth_range: Option<[Option<f64>; 2]>,
        }
        #[derive(Serialize)]
        struct PerspectiveWire<'a> {
            id: &'a str,
            source_entity: &'a str,
            form: i64,
            projection: ViewProjection,
            view_number: Option<i64>,
            scale: Option<f64>,
            model_to_view: Option<&'a str>,
            clipping_planes: &'a [Option<String>],
            view_plane_normal: [Option<f64>; 3],
            view_reference_point: [Option<f64>; 3],
            center_of_projection: [Option<f64>; 3],
            view_up: [Option<f64>; 3],
            view_plane_distance: Option<f64>,
            clipping_window: [Option<f64>; 4],
            depth_clipping: Option<i64>,
            depth_range: [Option<f64>; 2],
        }
        let projection = self.geometry.projection();
        match &self.geometry {
            ViewGeometry::Orthographic {
                model_to_view,
                clipping_planes,
            } => OrthographicWire {
                id: &self.id,
                source_entity: &self.source_entity,
                form: projection.form(),
                projection,
                view_number: self.view_number,
                scale: self.scale,
                model_to_view,
                clipping_planes,
                view_plane_normal: None,
                view_reference_point: None,
                center_of_projection: None,
                view_up: None,
                view_plane_distance: None,
                clipping_window: None,
                depth_clipping: None,
                depth_range: None,
            }
            .serialize(serializer),
            ViewGeometry::Perspective(geometry) => PerspectiveWire {
                id: &self.id,
                source_entity: &self.source_entity,
                form: projection.form(),
                projection,
                view_number: self.view_number,
                scale: self.scale,
                model_to_view: None,
                clipping_planes: &[],
                view_plane_normal: geometry.view_plane_normal,
                view_reference_point: geometry.view_reference_point,
                center_of_projection: geometry.center_of_projection,
                view_up: geometry.view_up,
                view_plane_distance: geometry.view_plane_distance,
                clipping_window: geometry.clipping_window,
                depth_clipping: geometry.depth_clipping,
                depth_range: geometry.depth_range,
            }
            .serialize(serializer),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeViewDisplay {
    view: Option<String>,
    #[serde(flatten)]
    style: ViewDisplayStyle,
}

#[derive(Debug, Clone, PartialEq)]
enum ViewDisplayStyle {
    Inherited,
    Overrides {
        line_font: Option<i64>,
        line_font_definition: Option<String>,
        color: Option<i64>,
        line_weight: Option<i64>,
    },
}

impl Serialize for ViewDisplayStyle {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Wire<'a> {
            line_font: Option<i64>,
            line_font_definition: Option<&'a str>,
            color: Option<i64>,
            line_weight: Option<i64>,
        }
        let wire = match self {
            Self::Inherited => Wire {
                line_font: None,
                line_font_definition: None,
                color: None,
                line_weight: None,
            },
            Self::Overrides {
                line_font,
                line_font_definition,
                color,
                line_weight,
            } => Wire {
                line_font: *line_font,
                line_font_definition: line_font_definition.as_deref(),
                color: *color,
                line_weight: *line_weight,
            },
        };
        wire.serialize(serializer)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeViewVisibility {
    id: String,
    source_entity: String,
    form: i64,
    declared_view_count: Option<i64>,
    displays: Vec<NativeViewDisplay>,
    declared_entity_count: Option<i64>,
    entities: Vec<Option<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeSegmentDisplay {
    view: Option<String>,
    breakpoint: Option<f64>,
    display_flag: Option<i64>,
    color: TokenValue,
    line_font: TokenValue,
    line_weight: TokenValue,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeSegmentedVisibility {
    id: String,
    source_entity: String,
    declared_block_count: Option<i64>,
    blocks: Vec<NativeSegmentDisplay>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeDrawingView {
    view: Option<String>,
    origin: [Option<f64>; 2],
    rotation: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeDrawing {
    id: String,
    source_entity: String,
    form: i64,
    declared_view_count: Option<i64>,
    views: Vec<NativeDrawingView>,
    declared_annotation_count: Option<i64>,
    annotations: Vec<Option<String>>,
    name_property: Option<String>,
    name: Option<Vec<u8>>,
    size: Option<[Option<f64>; 2]>,
    units_flag: Option<i64>,
    units_name: Option<Vec<u8>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    ambiguous_property_forms: Vec<i64>,
}

fn choose_drawing_property(
    trailing: Option<&crate::parameter::ResolvedGroups>,
    form: i64,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
) -> (Option<u32>, bool) {
    let mut selected = None;
    let mut first_value = None;
    let mut conflicting = false;
    for sequence in trailing
        .into_iter()
        .flat_map(crate::parameter::ResolvedGroups::properties)
    {
        if !entries
            .get(sequence)
            .is_some_and(|entry| entry.entity_type == 406 && entry.form == form)
        {
            continue;
        }
        let Some(value) = records
            .get(sequence)
            .and_then(|record| drawing_property_value(form, record))
        else {
            continue;
        };
        if first_value.as_ref().is_some_and(|first| *first != value) {
            conflicting = true;
        } else if first_value.is_none() {
            first_value = Some(value);
        }
        selected = Some(selected.map_or(*sequence, |current: u32| current.min(*sequence)));
    }
    ((!conflicting).then_some(selected).flatten(), conflicting)
}

#[derive(Clone)]
struct OccurrenceDefinition {
    members: Vec<u32>,
    transform: Transform,
}

// The wire adapter receives the optional field by reference, including its absence.
#[allow(clippy::ref_option)]
fn serialize_parameter_record<S: Serializer>(
    record: &Option<NativeParameterRecord>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    #[derive(Serialize)]
    struct Wire<'a> {
        parameter_line_start: Option<u32>,
        parameter_line_end: Option<u32>,
        parameter_bytes: &'a [u8],
        parameters: &'a [Token],
        comment: &'a [u8],
    }
    let record = record.as_ref();
    Wire {
        parameter_line_start: record.map(|record| record.lines.start),
        parameter_line_end: record.map(|record| record.lines.end),
        parameter_bytes: record.map_or(&[], |record| record.bytes.as_slice()),
        parameters: record.map_or(&[], |record| record.parameters.as_slice()),
        comment: record.map_or(&[], |record| record.comment.as_slice()),
    }
    .serialize(serializer)
}

#[derive(Debug, Clone, PartialEq)]
struct NativeParameterRecord {
    lines: std::ops::Range<u32>,
    bytes: Vec<u8>,
    parameters: Vec<Token>,
    comment: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeEntity {
    id: String,
    directory_sequence: u32,
    entity_type: i64,
    form: i64,
    parameter_start: i64,
    parameter_line_count: i64,
    structure: i64,
    line_font: i64,
    level: i64,
    view: i64,
    transform: i64,
    label_display: i64,
    #[serde(flatten)]
    status: SourceStatus,
    line_weight: i64,
    color: i64,
    reserved: [[u8; 8]; 2],
    label: [u8; 8],
    subscript: i64,
    #[serde(flatten, serialize_with = "serialize_parameter_record")]
    parameter_record: Option<NativeParameterRecord>,
    association_links: Vec<String>,
    property_links: Vec<String>,
    links: Vec<String>,
    references: Vec<ReferenceEdge>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct NativeMacroDefinition {
    id: String,
    source_entity: String,
    defined_entity_type: i64,
    macro_statement: Vec<u8>,
    language_statements: Vec<Vec<u8>>,
    end_statement: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeMacroInstance {
    id: String,
    source_entity: String,
    entity_type: i64,
    form: i64,
    macro_definition: Option<String>,
    macro_library: Option<String>,
    parameters: Vec<Token>,
}

fn binary_integer(value: Option<i64>) -> Option<bool> {
    match value? {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

fn member_affine(
    entry: &DirectoryEntry,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
    length_factor: f64,
    precision: RealPrecision,
    ctx: &DecodeContext<'_>,
) -> Result<Transform, TransformResolutionError> {
    if entry.transform == 0 {
        return Ok(Transform::identity());
    }
    resolve_transform(
        entry.transform,
        entries,
        records,
        length_factor,
        precision,
        &mut std::collections::BTreeSet::new(),
        Some(ctx),
    )
}

struct OccurrenceExpansion<'a, 'ctx> {
    entries: &'a BTreeMap<u32, &'a DirectoryEntry>,
    records: &'a BTreeMap<u32, &'a ParameterRecord>,
    definitions: &'a BTreeMap<u32, OccurrenceDefinition>,
    neutral_links: &'a BTreeMap<u32, Vec<String>>,
    length_factor: f64,
    precision: RealPrecision,
    output_limit: usize,
    depth_limit: usize,
    ctx: &'a DecodeContext<'ctx>,
}

impl OccurrenceExpansion<'_, '_> {
    fn record_malformed(
        &self,
        malformed: &mut BTreeSet<u32>,
        sequence: u32,
    ) -> Result<(), CodecError> {
        insert_optional_btree_set(
            Some(self.ctx),
            malformed,
            sequence,
            "iges malformed occurrence placement nodes",
        )?;
        Ok(())
    }

    fn expand(
        &self,
        instance_sequence: u32,
        parent: Transform,
        path: &mut Vec<u32>,
        occurrences: &mut Vec<NativeProductOccurrence>,
        depth_truncated_at: &mut Option<u32>,
        malformed_placement_sequences: &mut std::collections::BTreeSet<u32>,
    ) -> Result<Option<u32>, CodecError> {
        let _depth = self.ctx.enter_nested("iges_product_occurrence")?;
        if occurrences.len() >= self.output_limit {
            return Ok(Some(instance_sequence));
        }
        if path.len() >= self.depth_limit {
            if depth_truncated_at.is_none() {
                *depth_truncated_at = Some(instance_sequence);
            }
            return Ok(None);
        }
        if path.contains(&instance_sequence) {
            return Ok(None);
        }
        let (Some(instance), Some(record)) = (
            self.entries.get(&instance_sequence).copied(),
            self.records.get(&instance_sequence).copied(),
        ) else {
            self.record_malformed(malformed_placement_sequences, instance_sequence)?;
            return Ok(None);
        };
        let (definition_sequence, local) = match placement_affine(
            instance,
            record,
            self.entries,
            self.records,
            self.length_factor,
            self.precision,
            Some(self.ctx),
        ) {
            Ok(placement) => placement,
            Err(error) => {
                error.non_resource()?;
                self.record_malformed(malformed_placement_sequences, instance_sequence)?;
                return Ok(None);
            }
        };
        let Some(definition) = self.definitions.get(&definition_sequence) else {
            self.record_malformed(malformed_placement_sequences, instance_sequence)?;
            return Ok(None);
        };
        let Ok(definition_world) = parent
            .compose(local)
            .and_then(|world| world.compose(definition.transform))
        else {
            self.record_malformed(malformed_placement_sequences, instance_sequence)?;
            return Ok(None);
        };
        reserve_vec_growth(self.ctx, path, 1, "iges occurrence expansion path slots")?;
        path.push(instance_sequence);
        reserve_vec_growth(self.ctx, occurrences, 1, "iges_product_occurrences")?;
        occurrences.push(NativeProductOccurrence::new(
            self.ctx,
            path,
            None,
            instance_sequence,
            definition_sequence,
            Vec::new(),
            (local.affine_rows(), definition_world.affine_rows()),
        )?);
        for member in &definition.members {
            if occurrences.len() >= self.output_limit {
                path.pop();
                return Ok(Some(instance_sequence));
            }
            if self
                .entries
                .get(member)
                .is_some_and(|entry| matches!(entry.entity_type, 408 | 420))
            {
                if let Some(source_sequence) = self.expand(
                    *member,
                    definition_world,
                    path,
                    occurrences,
                    depth_truncated_at,
                    malformed_placement_sequences,
                )? {
                    path.pop();
                    return Ok(Some(source_sequence));
                }
                continue;
            }
            let Some(member_entry) = self.entries.get(member).copied() else {
                self.record_malformed(malformed_placement_sequences, instance_sequence)?;
                continue;
            };
            let member_local = match member_affine(
                member_entry,
                self.entries,
                self.records,
                self.length_factor,
                self.precision,
                self.ctx,
            ) {
                Ok(transform) => transform,
                Err(error) => {
                    error.non_resource()?;
                    self.record_malformed(malformed_placement_sequences, *member)?;
                    continue;
                }
            };
            let Ok(member_world) = definition_world.compose(member_local) else {
                self.record_malformed(malformed_placement_sequences, *member)?;
                continue;
            };
            reserve_vec_growth(self.ctx, occurrences, 1, "iges_product_occurrences")?;
            let neutral_links = self
                .neutral_links
                .get(member)
                .map_or(Ok(Vec::new()), |links| {
                    collect_result_vec(
                        self.ctx,
                        links.len(),
                        "iges occurrence neutral link copy slots",
                        |index| {
                            format_retained(
                                self.ctx,
                                format_args!("{}", links[index]),
                                "iges occurrence neutral link copy",
                            )
                        },
                    )
                })?;
            occurrences.push(NativeProductOccurrence::new(
                self.ctx,
                path,
                Some(*member),
                instance_sequence,
                definition_sequence,
                neutral_links,
                (member_local.affine_rows(), member_world.affine_rows()),
            )?);
        }
        path.pop();
        Ok(None)
    }
}

fn charge_native_entities(ctx: &DecodeContext<'_>, count: u64) -> Result<(), CodecError> {
    ctx.charge_entities(count, "iges_native_entities")
}

fn copy_native_tokens(ctx: &DecodeContext<'_>, tokens: &[Token]) -> Result<Vec<Token>, CodecError> {
    let mut copies = reserve_vec(ctx, tokens.len(), "iges native token slots")?;
    for token in tokens {
        let value = match &token.value {
            TokenValue::String(bytes) => {
                TokenValue::String(ctx.copy_retained(bytes, "iges native token bytes")?)
            }
            value => value.clone(),
        };
        copies.push(Token {
            value,
            span: token.span.clone(),
        });
    }
    Ok(copies)
}

fn copy_native_token_value(
    ctx: &DecodeContext<'_>,
    value: &TokenValue,
) -> Result<TokenValue, CodecError> {
    match value {
        TokenValue::String(bytes) => Ok(TokenValue::String(
            ctx.copy_retained(bytes, "iges native token value bytes")?,
        )),
        value => Ok(value.clone()),
    }
}

fn copy_native_string(
    ctx: &DecodeContext<'_>,
    bytes: Option<&[u8]>,
    operation: &'static str,
) -> Result<Option<Vec<u8>>, CodecError> {
    bytes
        .map(|bytes| ctx.copy_retained(bytes, operation))
        .transpose()
}

fn native_entity_ids(
    ctx: &DecodeContext<'_>,
    sequences: impl IntoIterator<Item = u32>,
    operation: &'static str,
) -> Result<Vec<String>, CodecError> {
    let mut ids = Vec::new();
    for sequence in sequences {
        reserve_vec_growth(ctx, &mut ids, 1, operation)?;
        ids.push(format_retained(
            ctx,
            format_args!("iges:entity:directory#{sequence}"),
            "iges native linked entity id",
        )?);
    }
    Ok(ids)
}

fn push_occurrence_neutral_link(
    ctx: &DecodeContext<'_>,
    links: &mut BTreeMap<u32, Vec<String>>,
    sequence: u32,
    id: &str,
) -> Result<(), CodecError> {
    if !links.contains_key(&sequence) {
        insert_optional_btree_map(
            Some(ctx),
            links,
            sequence,
            Vec::new(),
            "iges occurrence neutral link map nodes",
        )?;
    }
    if let Some(group) = links.get_mut(&sequence) {
        reserve_vec_growth(ctx, group, 1, "iges occurrence neutral link slots")?;
        group.push(format_retained(
            ctx,
            format_args!("{id}"),
            "iges occurrence neutral link id",
        )?);
    }
    Ok(())
}

struct ColonsAsUnderscores<'a>(&'a str);

impl std::fmt::Display for ColonsAsUnderscores<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (index, part) in self.0.split(':').enumerate() {
            if index != 0 {
                formatter.write_str("_")?;
            }
            formatter.write_str(part)?;
        }
        Ok(())
    }
}

fn copy_native_parameter_record(
    ctx: &DecodeContext<'_>,
    record: &ParameterRecord,
) -> Result<NativeParameterRecord, CodecError> {
    Ok(NativeParameterRecord {
        lines: record.line_range.clone(),
        bytes: ctx.copy_retained(&record.bytes, "iges native parameter bytes")?,
        parameters: copy_native_tokens(ctx, record.tokens())?,
        comment: ctx.copy_retained(&record.comment, "iges native parameter comment")?,
    })
}

fn collect_native_items<I, T>(
    ctx: &DecodeContext<'_>,
    entries: I,
    operation: &'static str,
    mut build: impl FnMut(I::Item) -> Result<T, CodecError>,
) -> Result<Vec<T>, CodecError>
where
    I: Iterator + Clone,
{
    let mut values = reserve_vec(ctx, entries.clone().count(), operation)?;
    for entry in entries {
        values.push(build(entry)?);
    }
    Ok(values)
}

struct NativeInputIndexes<'a> {
    quarantined_directory_records: Vec<NativeQuarantinedRecord<'a>>,
    quarantined_parameter_records: Vec<NativeQuarantinedRecord<'a>>,
    cards: Vec<NativeCard<'a>>,
    by_directory: BTreeMap<u32, &'a ParameterRecord>,
    entries: BTreeMap<u32, &'a DirectoryEntry>,
}

fn index_native_inputs<'a>(
    scan: &'a CardScan<'_>,
    directory: &'a [DirectoryEntry],
    parameters: &'a [ParameterRecord],
    quarantine: QuarantinedRecords<'a>,
    ctx: &DecodeContext<'_>,
) -> Result<NativeInputIndexes<'a>, CodecError> {
    let mut quarantined_directory_records = reserve_vec(
        ctx,
        quarantine.directory.len(),
        "iges native quarantined directory slots",
    )?;
    quarantined_directory_records.extend(
        quarantine
            .directory
            .iter()
            .map(NativeQuarantinedRecord::Directory),
    );
    let mut quarantined_parameter_records = reserve_vec(
        ctx,
        quarantine.parameters.len(),
        "iges native quarantined parameter slots",
    )?;
    quarantined_parameter_records.extend(
        quarantine
            .parameters
            .iter()
            .map(NativeQuarantinedRecord::Parameter),
    );
    let mut cards = reserve_vec(ctx, scan.lines.len(), "iges native card slots")?;
    cards.extend(
        scan.lines
            .iter()
            .enumerate()
            .map(|(index, line)| NativeCard { index, line }),
    );
    let mut by_directory = BTreeMap::new();
    for record in parameters {
        insert_optional_btree_map(
            Some(ctx),
            &mut by_directory,
            record.directory_sequence,
            record,
            "iges native parameter index",
        )?;
    }
    let mut entries = BTreeMap::new();
    for entry in directory {
        insert_optional_btree_map(
            Some(ctx),
            &mut entries,
            entry.sequence,
            entry,
            "iges native directory index",
        )?;
    }
    Ok(NativeInputIndexes {
        quarantined_directory_records,
        quarantined_parameter_records,
        cards,
        by_directory,
        entries,
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn store(
    ir: &mut CadIr,
    scan: &CardScan,
    directory: &[DirectoryEntry],
    parameters: &[ParameterRecord],
    trailing_pointer_analysis: &BTreeMap<u32, TrailingPointerAnalysis>,
    quarantine: QuarantinedRecords<'_>,
    structure_admitted: Option<&crate::entities::geometry::Projection>,
    sequences: &crate::entities::geometry::SourceSequences,
    boundary_vertex_derivations: &[BoundaryVertexDerivation],
    references: &mut BTreeMap<u32, Vec<ReferenceEdge>>,
    global: &ResolvedGlobal,
    limits: ProductOccurrenceLimits,
    ctx: &DecodeContext<'_>,
) -> Result<NativeStoreResult, CodecError> {
    charge_native_entities(ctx, scan.lines.len() as u64)?;
    let NativeInputIndexes {
        quarantined_directory_records,
        quarantined_parameter_records,
        cards,
        by_directory,
        entries,
    } = index_native_inputs(scan, directory, parameters, quarantine, ctx)?;
    let mut macro_definitions = Vec::new();
    for entry in directory.iter().filter(|entry| entry.entity_type == 306) {
        let Some(record) = by_directory.get(&entry.sequence).copied() else {
            continue;
        };
        let data = match crate::parameter::macro_parameter_data_with_context(
            &record.bytes,
            global.parameter_delimiter,
            global.record_delimiter,
            Some(ctx),
        ) {
            Ok(data) => data,
            Err(MacroDataError::Defect(_, _)) => continue,
            Err(MacroDataError::Refusal(error)) => return Err(error),
        };
        let Some(first) = data.statement_spans.first() else {
            continue;
        };
        let Some(last) = data.statement_spans.last() else {
            continue;
        };
        let Some(language_end) = data.statement_spans.len().checked_sub(1) else {
            continue;
        };
        let Some(language_spans) = data.statement_spans.get(1..language_end) else {
            continue;
        };
        let mut language_statements = Vec::new();
        for span in language_spans {
            reserve_vec_growth(
                ctx,
                &mut language_statements,
                1,
                "iges native macro statements",
            )?;
            language_statements.push(ctx.copy_retained(
                &record.bytes[span.clone()],
                "iges native macro statement bytes",
            )?);
        }
        reserve_vec_growth(
            ctx,
            &mut macro_definitions,
            1,
            "iges native macro definitions",
        )?;
        macro_definitions.push(NativeMacroDefinition {
            id: format_retained(
                ctx,
                format_args!("iges:native:macro-definition#D{}", entry.sequence),
                "iges native macro definition id",
            )?,
            source_entity: format_retained(
                ctx,
                format_args!("iges:entity:directory#{}", entry.sequence),
                "iges native macro source id",
            )?,
            defined_entity_type: data.defined_entity_type,
            macro_statement: ctx.copy_retained(
                &record.bytes[first.clone()],
                "iges native macro header bytes",
            )?,
            language_statements,
            end_statement: ctx.copy_retained(
                &record.bytes[last.clone()],
                "iges native macro terminator bytes",
            )?,
        });
    }
    let mut macro_instances = Vec::new();
    for entry in directory
        .iter()
        .filter(|entry| crate::profile::macro_instance_type(entry.entity_type))
    {
        let Some(record) = by_directory.get(&entry.sequence).copied() else {
            continue;
        };
        reserve_vec_growth(
            ctx,
            &mut macro_instances,
            1,
            "iges native macro instance slots",
        )?;
        let structure_sequence =
            crate::graph::resolved_structure_sequence(references, entry.sequence);
        let macro_definition = structure_sequence
            .filter(|sequence| {
                entries
                    .get(sequence)
                    .is_some_and(|target| target.entity_type == 306)
            })
            .map(|sequence| {
                format_retained(
                    ctx,
                    format_args!("iges:entity:directory#{sequence}"),
                    "iges native macro definition reference",
                )
            })
            .transpose()?;
        let macro_library = structure_sequence
            .filter(|sequence| {
                entries
                    .get(sequence)
                    .is_some_and(|target| target.entity_type == 416)
            })
            .map(|sequence| {
                format_retained(
                    ctx,
                    format_args!("iges:entity:directory#{sequence}"),
                    "iges native macro library reference",
                )
            })
            .transpose()?;
        macro_instances.push(NativeMacroInstance {
            id: format_retained(
                ctx,
                format_args!("iges:native:macro-instance#D{}", entry.sequence),
                "iges native macro instance id",
            )?,
            source_entity: format_retained(
                ctx,
                format_args!("iges:entity:directory#{}", entry.sequence),
                "iges native macro instance source",
            )?,
            entity_type: entry.entity_type,
            form: entry.form,
            macro_definition,
            macro_library,
            parameters: copy_native_tokens(ctx, record.tokens().get(1..).unwrap_or(&[]))?,
        });
    }
    // The native reading boundary is the retained trailing-group boundary
    // clamped to the entity's primary layout.
    let clamped_primary_end = |sequence: u32, record: &ParameterRecord| {
        trailing_pointer_analysis
            .get(&sequence)
            .and_then(|analysis| match analysis {
                TrailingPointerAnalysis::Unambiguous(groups) => Some(groups),
                _ => None,
            })
            .map_or(record.parameter_end(), |groups| groups.token_start)
            .min(
                crate::parameter::entity_primary_end_for_global_table(
                    record,
                    &entries,
                    global.global_table(),
                )
                .unwrap_or(record.parameter_end()),
            )
    };
    let parameter_resolver = ParameterResolver::new(directory, ctx)?;
    let mut overdeclared_counts = OverdeclaredCounts::default();
    let mut unstatable_attribute_tables = BTreeMap::new();
    let mut required_back_pointer_members = std::collections::BTreeSet::new();
    for group in directory
        .iter()
        .filter(|entry| entry.entity_type == 402 && matches!(entry.form, 1 | 14))
    {
        let record = by_directory.get(&group.sequence).copied();
        let count = record
            .and_then(|record| {
                record.count_with_stride_before(1, 1, clamped_primary_end(group.sequence, record))
            })
            .unwrap_or_default();
        for index in 0..count {
            if let Some(sequence) = record
                .and_then(|record| record.integer(2 + index))
                .and_then(|value| u32::try_from(value).ok())
                .filter(|sequence| sequence % 2 == 1 && entries.contains_key(sequence))
            {
                insert_optional_btree_set(
                    Some(ctx),
                    &mut required_back_pointer_members,
                    sequence,
                    "iges native required back-pointer member",
                )?;
            }
        }
    }
    let mut ambiguous_parameter_boundaries = Vec::new();
    for sequence in by_directory.keys() {
        let Some(analysis) = trailing_pointer_analysis.get(sequence) else {
            continue;
        };
        let TrailingPointerAnalysis::Ambiguous { candidates, valid } = analysis else {
            continue;
        };
        let ambiguity = if *valid > 1 {
            ParameterBoundaryAmbiguity::EquallyValid(*valid)
        } else if required_back_pointer_members.contains(sequence) && *candidates > 1 {
            ParameterBoundaryAmbiguity::Structural(*candidates)
        } else {
            continue;
        };
        reserve_vec_growth(
            ctx,
            &mut ambiguous_parameter_boundaries,
            1,
            "iges native ambiguous boundary slots",
        )?;
        ambiguous_parameter_boundaries.push(AmbiguousParameterBoundary {
            sequence: *sequence,
            ambiguity,
        });
    }
    charge_native_entities(ctx, directory.len() as u64)?;
    let mut entities =
        collect_result_vec(ctx, directory.len(), "iges native entity slots", |index| {
            let entry = &directory[index];
            let parameters = by_directory.get(&entry.sequence).copied();
            let trailing = trailing_pointer_analysis
                .get(&entry.sequence)
                .and_then(|analysis| match analysis {
                    TrailingPointerAnalysis::Unambiguous(groups) => Some(groups),
                    _ => None,
                });
            let invalid_trailing = (trailing.is_none()
                && required_back_pointer_members.contains(&entry.sequence))
            .then(|| {
                trailing_pointer_analysis
                    .get(&entry.sequence)
                    .and_then(|analysis| match analysis {
                        TrailingPointerAnalysis::SingleInvalid(groups) => Some(groups),
                        _ => None,
                    })
            })
            .flatten();
            for (token_index, raw_pointer) in trailing
                .into_iter()
                .flat_map(|groups| {
                    groups
                        .associations()
                        .iter()
                        .enumerate()
                        .map(|(index, sequence)| {
                            (groups.token_start + 1 + index, i64::from(*sequence))
                        })
                })
                .chain(invalid_trailing.into_iter().flat_map(|groups| {
                    groups
                        .association_pointers
                        .iter()
                        .map(|pointer| (pointer.token_index, pointer.raw_pointer))
                }))
            {
                let _association = parameter_resolver.resolve_any_of(
                    entry.sequence,
                    token_index,
                    raw_pointer,
                    (212, 312, &[402]),
                    |target| matches!(target.entity_type, 212 | 312 | 402),
                )?;
            }
            for (token_index, raw_pointer) in trailing
                .into_iter()
                .flat_map(|groups| {
                    groups
                        .properties()
                        .iter()
                        .enumerate()
                        .map(|(index, sequence)| {
                            (
                                groups.token_start + groups.associations().len() + 2 + index,
                                i64::from(*sequence),
                            )
                        })
                })
                .chain(invalid_trailing.into_iter().flat_map(|groups| {
                    groups
                        .property_pointers
                        .iter()
                        .map(|pointer| (pointer.token_index, pointer.raw_pointer))
                }))
            {
                let _property = parameter_resolver.resolve_any_of(
                    entry.sequence,
                    token_index,
                    raw_pointer,
                    (316, 322, &[406, 422]),
                    |target| matches!(target.entity_type, 316 | 322 | 406 | 422),
                )?;
            }
            let association_links = native_entity_ids(
                ctx,
                trailing
                    .into_iter()
                    .flat_map(ResolvedGroups::associations)
                    .copied(),
                "iges native association link slots",
            )?;
            let property_links = native_entity_ids(
                ctx,
                trailing
                    .into_iter()
                    .flat_map(ResolvedGroups::properties)
                    .copied(),
                "iges native property link slots",
            )?;
            Ok(NativeEntity {
                id: format_retained(
                    ctx,
                    format_args!("iges:entity:directory#{}", entry.sequence),
                    "iges native entity id",
                )?,
                directory_sequence: entry.sequence,
                entity_type: entry.entity_type,
                form: entry.form,
                parameter_start: entry.parameter_start,
                parameter_line_count: entry.parameter_line_count,
                structure: entry.structure,
                line_font: entry.line_font,
                level: entry.level,
                view: entry.view,
                transform: entry.transform,
                label_display: entry.label_display,
                status: entry.status,
                line_weight: entry.line_weight,
                color: entry.color,
                reserved: entry.reserved,
                label: entry.label,
                subscript: entry.subscript,
                parameter_record: parameters
                    .map(|record| copy_native_parameter_record(ctx, record))
                    .transpose()?,
                association_links,
                property_links,
                links: native_entity_ids(
                    ctx,
                    references
                        .get(&entry.sequence)
                        .into_iter()
                        .flatten()
                        .filter_map(ReferenceEdge::target_sequence),
                    "iges native reference link slots",
                )?,
                references: match references.get(&entry.sequence) {
                    Some(edges) => {
                        let mut copies =
                            reserve_vec(ctx, edges.len(), "iges native reference slots")?;
                        for edge in edges {
                            copies.push(edge.copy_for_native(ctx)?);
                        }
                        copies
                    }
                    None => Vec::new(),
                },
            })
        })?;
    let directions = collect_native_items(
        ctx,
        directory
            .iter()
            .filter(|entry| entry.entity_type == 123 && entry.form == 0),
        "iges native direction slots",
        |entry| {
            let parameters = by_directory.get(&entry.sequence).copied();
            Ok(NativeDirection {
                id: format_retained(
                    ctx,
                    format_args!("iges:native:direction#D{}", entry.sequence),
                    "iges native direction id",
                )?,
                source_entity: format_retained(
                    ctx,
                    format_args!("iges:entity:directory#{}", entry.sequence),
                    "iges native direction source",
                )?,
                components: collect_result_vec(
                    ctx,
                    3,
                    "iges native direction components",
                    |index| Ok(parameters.and_then(|record| record.number(index + 1))),
                )?,
                physically_dependent: entry.status.is_physically_dependent(),
                has_transform: entry.transform != 0,
            })
        },
    )?;
    let flashes = collect_native_items(
        ctx,
        directory
            .iter()
            .filter(|entry| entry.entity_type == 125 && matches!(entry.form, 0..=4)),
        "iges native flash slots",
        |entry| {
            let parameters = by_directory.get(&entry.sequence).copied();
            let reference_entity = parameters
                .and_then(|record| record.integer_or(6, 0))
                .map(|sequence| parameter_resolver.resolve_any(entry.sequence, 6, sequence))
                .transpose()?
                .flatten()
                .map(|sequence| {
                    format_retained(
                        ctx,
                        format_args!("iges:entity:directory#{sequence}"),
                        "iges native flash reference",
                    )
                })
                .transpose()?;
            Ok(NativeFlash {
                id: format_retained(
                    ctx,
                    format_args!("iges:native:flash#D{}", entry.sequence),
                    "iges native flash id",
                )?,
                source_entity: format_retained(
                    ctx,
                    format_args!("iges:entity:directory#{}", entry.sequence),
                    "iges native flash source",
                )?,
                form: entry.form,
                reference_point: [
                    parameters.and_then(|record| record.number(1)),
                    parameters.and_then(|record| record.number(2)),
                ],
                dimension_1: parameters.and_then(|record| record.number_or(3, 0.0)),
                dimension_2: parameters.and_then(|record| record.number_or(4, 0.0)),
                rotation: parameters.and_then(|record| record.number_or(5, 0.0)),
                reference_entity,
            })
        },
    )?;
    let transforms = collect_native_items(
        ctx,
        directory
            .iter()
            .filter(|entry| entry.entity_type == 124 && matches!(entry.form, 0 | 1 | 10 | 11 | 12)),
        "iges native transformation slots",
        |entry| {
            let parameters = by_directory.get(&entry.sequence).copied();
            Ok(NativeTransformation {
                id: format_retained(
                    ctx,
                    format_args!("iges:native:transformation#D{}", entry.sequence),
                    "iges native transformation id",
                )?,
                source_entity: format_retained(
                    ctx,
                    format_args!("iges:entity:directory#{}", entry.sequence),
                    "iges native transformation source",
                )?,
                form: entry.form,
                coefficients: collect_result_vec(
                    ctx,
                    12,
                    "iges native transformation coefficients",
                    |index| Ok(parameters.and_then(|record| record.number(index + 1))),
                )?,
                parent: (entry.transform > 0)
                    .then(|| {
                        format_retained(
                            ctx,
                            format_args!("iges:native:transformation#D{}", entry.transform),
                            "iges native transformation parent",
                        )
                    })
                    .transpose()?,
            })
        },
    )?;
    let copious_data = collect_native_items(
        ctx,
        directory.iter().filter(|entry| entry.entity_type == 106),
        "iges native copious data slots",
        |entry| {
            let parameters = by_directory.get(&entry.sequence).copied();
            let interpretation = parameters.and_then(|record| record.integer(1));
            let declared_tuple_count = parameters.and_then(|record| record.integer(2));
            let layout = copious_tuple_layout(entry.form, interpretation);
            let common_z = (layout == Some((4, 2)))
                .then(|| parameters.and_then(|record| record.number(3)))
                .flatten();
            let tuples = match (layout, parameters) {
                (Some((start, width)), Some(record)) => {
                    let end = clamped_primary_end(entry.sequence, record);
                    let count = overdeclared_counts.counted_tail_at(
                        entry.sequence,
                        Some(record),
                        end,
                        2,
                        start,
                        width,
                    );
                    collect_result_vec(ctx, count, "iges native copious tuple slots", |tuple| {
                        collect_result_vec(
                            ctx,
                            width,
                            "iges native copious component slots",
                            |component| {
                                Ok(tuple
                                    .checked_mul(width)
                                    .and_then(|offset| offset.checked_add(start))
                                    .and_then(|offset| offset.checked_add(component))
                                    .and_then(|index| record.number(index)))
                            },
                        )
                    })?
                }
                _ => Vec::new(),
            };
            Ok(NativeCopiousData {
                id: format_retained(
                    ctx,
                    format_args!("iges:native:copious-data#D{}", entry.sequence),
                    "iges native copious data id",
                )?,
                source_entity: format_retained(
                    ctx,
                    format_args!("iges:entity:directory#{}", entry.sequence),
                    "iges native copious data source",
                )?,
                form: entry.form,
                interpretation,
                declared_tuple_count,
                common_z,
                tuples,
            })
        },
    )?;
    let colors = collect_native_items(
        ctx,
        directory
            .iter()
            .filter(|entry| entry.entity_type == 314 && entry.form == 0),
        "iges native color slots",
        |entry| {
            let parameters = by_directory.get(&entry.sequence).copied();
            Ok(NativeColorDefinition {
                id: format_retained(
                    ctx,
                    format_args!("iges:presentation:color#D{}", entry.sequence),
                    "iges native color id",
                )?,
                source_entity: format_retained(
                    ctx,
                    format_args!("iges:entity:directory#{}", entry.sequence),
                    "iges native color source",
                )?,
                red_percent: parameters.and_then(|record| record.number(1)),
                green_percent: parameters.and_then(|record| record.number(2)),
                blue_percent: parameters.and_then(|record| record.number(3)),
                name: parameters
                    .and_then(|record| record.string(4))
                    .map(|bytes| ctx.copy_retained(bytes, "iges native color name"))
                    .transpose()?,
                fallback_color_number: entry.color,
            })
        },
    )?;
    let display_attributes = collect_native_items(
        ctx,
        directory.iter(),
        "iges native display attribute slots",
        |entry| {
            Ok(NativeDisplayAttributes {
                id: format_retained(
                    ctx,
                    format_args!("iges:presentation:display-attributes#D{}", entry.sequence),
                    "iges native display id",
                )?,
                source_entity: format_retained(
                    ctx,
                    format_args!("iges:entity:directory#{}", entry.sequence),
                    "iges native display source",
                )?,
                visible: entry.status.is_visible(),
                line_font: resolve_display_ref(
                    ctx,
                    references,
                    entry.sequence,
                    entry.line_font,
                    ReferenceKind::LineFont,
                    "line-font",
                )?,
                level: resolve_display_ref(
                    ctx,
                    references,
                    entry.sequence,
                    entry.level,
                    ReferenceKind::Level,
                    "definition-levels",
                )?,
                view: entry.view,
                line_weight_number: entry.line_weight,
                line_weight_mm: global
                    .length_context()
                    .and_then(|context| context.line_weight_mm(entry.line_weight)),
                color: resolve_display_ref(
                    ctx,
                    references,
                    entry.sequence,
                    entry.color,
                    ReferenceKind::Color,
                    "color",
                )?,
            })
        },
    )?;
    let line_fonts = collect_native_items(
        ctx,
        directory
            .iter()
            .filter(|entry| entry.entity_type == 304 && matches!(entry.form, 1 | 2)),
        "iges native line font slots",
        |entry| {
            let parameters = by_directory.get(&entry.sequence).copied();
            Ok(if entry.form == 1 {
                NativeLineFontDefinition::Template {
                    id: format_retained(
                        ctx,
                        format_args!("iges:presentation:line-font#D{}", entry.sequence),
                        "iges native line font id",
                    )?,
                    source_entity: format_retained(
                        ctx,
                        format_args!("iges:entity:directory#{}", entry.sequence),
                        "iges native line font source",
                    )?,
                    fallback_line_font_number: entry.line_font,
                    tangent_oriented: binary_integer(
                        parameters.and_then(|record| record.integer(1)),
                    ),
                    template: parameters
                        .and_then(|record| record.integer(2))
                        .map(|sequence| {
                            parameter_resolver.resolve_type(entry.sequence, 2, sequence, 308, &[0])
                        })
                        .transpose()?
                        .flatten()
                        .map(|sequence| {
                            format_retained(
                                ctx,
                                format_args!("iges:entity:directory#{sequence}"),
                                "iges native line font template",
                            )
                        })
                        .transpose()?,
                    spacing: parameters.and_then(|record| record.number(3)),
                    scale: parameters.and_then(|record| record.number(4)),
                }
            } else {
                // A Form 2 line font states the visible/blank lengths before the
                // final hexadecimal pattern token, so the length run ends one
                // token earlier. A record with no primary token states no
                // length run, so the run is empty. No source-stated value is
                // floored here.
                let pattern_end = parameters
                    .map(|record| clamped_primary_end(entry.sequence, record))
                    .filter(|end| *end > 0)
                    .map_or(0, |end| end - 1);
                let count =
                    overdeclared_counts.counted_tail(entry.sequence, parameters, pattern_end, 1, 1);
                let declared = parameters.and_then(|record| record.integer(1));
                let held = declared.and_then(|value| usize::try_from(value).ok()) == Some(count);
                NativeLineFontDefinition::VisibleBlankPattern {
                    id: format_retained(
                        ctx,
                        format_args!("iges:presentation:line-font#D{}", entry.sequence),
                        "iges native line font id",
                    )?,
                    source_entity: format_retained(
                        ctx,
                        format_args!("iges:entity:directory#{}", entry.sequence),
                        "iges native line font source",
                    )?,
                    fallback_line_font_number: entry.line_font,
                    segment_count: declared,
                    lengths: collect_result_vec(
                        ctx,
                        count,
                        "iges native line font lengths",
                        |index| Ok(parameters.and_then(|record| record.number(2 + index))),
                    )?,
                    hexadecimal_pattern: held
                        .then(|| parameters.and_then(|record| record.string(2 + count)))
                        .flatten()
                        .map(|bytes| ctx.copy_retained(bytes, "iges native line font pattern"))
                        .transpose()?,
                }
            })
        },
    )?;
    let text_templates = collect_native_items(
        ctx,
        directory
            .iter()
            .filter(|entry| entry.entity_type == 312 && matches!(entry.form, 0..=1)),
        "iges native text template slots",
        |entry| {
            let record = by_directory.get(&entry.sequence).copied();
            let font_code = record.and_then(|record| record.integer(3));
            Ok(NativeTextDisplayTemplate {
                id: format_retained(
                    ctx,
                    format_args!("iges:presentation:text-template#D{}", entry.sequence),
                    "iges native text template id",
                )?,
                source_entity: format_retained(
                    ctx,
                    format_args!("iges:entity:directory#{}", entry.sequence),
                    "iges native text template source",
                )?,
                form: entry.form,
                character_box: [
                    record.and_then(|record| record.number(1)),
                    record.and_then(|record| record.number(2)),
                ],
                font_code,
                font_definition: font_code
                    .filter(|value| *value < 0)
                    .map(|value| {
                        parameter_resolver.resolve_negative_type(
                            entry.sequence,
                            3,
                            value,
                            310,
                            &[0],
                        )
                    })
                    .transpose()?
                    .flatten()
                    .map(|sequence| {
                        format_retained(
                            ctx,
                            format_args!("iges:presentation:text-font#D{sequence}"),
                            "iges native text template font",
                        )
                    })
                    .transpose()?,
                slant_angle: record.and_then(|record| record.number(4)),
                rotation_angle: record.and_then(|record| record.number(5)),
                mirror: record.and_then(|record| record.integer(6)),
                vertical: record.and_then(|record| record.integer(7)),
                origin_or_increment: [
                    record.and_then(|record| record.number(8)),
                    record.and_then(|record| record.number(9)),
                    record.and_then(|record| record.number(10)),
                ],
            })
        },
    )?;
    let text_fonts = collect_native_items(
        ctx,
        directory
            .iter()
            .filter(|entry| entry.entity_type == 310 && entry.form == 0),
        "iges native text font slots",
        |entry| {
            let record = by_directory.get(&entry.sequence).copied();
            let count = record
                .and_then(|record| {
                    record.count_with_stride_before(
                        5,
                        1,
                        clamped_primary_end(entry.sequence, record),
                    )
                })
                .unwrap_or_default();
            let supersedes_code = record.and_then(|record| record.integer(3));
            let mut cursor = 6_usize;
            let mut characters = reserve_vec(ctx, count, "iges native text font glyph slots")?;
            let mut malformed = false;
            for _ in 0..count {
                let Some(record) = record else {
                    malformed = true;
                    break;
                };
                let Some(count_index) = cursor.checked_add(3) else {
                    malformed = true;
                    break;
                };
                let declared_motion_count = record.integer(count_index);
                let motion_count = record.count_with_stride_before(
                    count_index,
                    3,
                    clamped_primary_end(entry.sequence, record),
                );
                let Some(motion_count) = motion_count else {
                    malformed = true;
                    break;
                };
                let Some(next) = motion_count
                    .checked_mul(3)
                    .and_then(|width| cursor.checked_add(4 + width))
                else {
                    malformed = true;
                    break;
                };
                let motions = collect_result_vec(
                    ctx,
                    motion_count,
                    "iges native text font motion slots",
                    |offset| {
                        let start = cursor + 4 + offset * 3;
                        Ok(NativeGlyphMotion {
                            pen_up: record.integer(start).map(|value| value == 1),
                            point: [record.integer(start + 1), record.integer(start + 2)],
                        })
                    },
                )?;
                characters.push(NativeGlyph {
                    character_code: record.integer(cursor),
                    next_origin: [record.integer(cursor + 1), record.integer(cursor + 2)],
                    declared_motion_count,
                    motions,
                });
                cursor = next;
            }
            if malformed {
                characters.clear();
            }
            Ok(NativeTextFontDefinition {
                id: format_retained(
                    ctx,
                    format_args!("iges:presentation:text-font#D{}", entry.sequence),
                    "iges native text font id",
                )?,
                source_entity: format_retained(
                    ctx,
                    format_args!("iges:entity:directory#{}", entry.sequence),
                    "iges native text font source",
                )?,
                font_code: record.and_then(|record| record.integer(1)),
                name: record
                    .and_then(|record| record.string(2))
                    .map(|bytes| ctx.copy_retained(bytes, "iges native text font name"))
                    .transpose()?,
                supersedes_code,
                supersedes_definition: supersedes_code
                    .filter(|value| *value < 0)
                    .map(|value| {
                        parameter_resolver.resolve_negative_type(
                            entry.sequence,
                            3,
                            value,
                            310,
                            &[0],
                        )
                    })
                    .transpose()?
                    .flatten()
                    .map(|sequence| {
                        format_retained(
                            ctx,
                            format_args!("iges:presentation:text-font#D{sequence}"),
                            "iges native text font superseded",
                        )
                    })
                    .transpose()?,
                grid_units_per_text_height: record.and_then(|record| record.integer(4)),
                declared_character_count: record.and_then(|record| record.integer(5)),
                characters,
            })
        },
    )?;
    let definition_levels = collect_native_items(
        ctx,
        directory
            .iter()
            .filter(|entry| entry.entity_type == 406 && entry.form == 1),
        "iges native definition level slots",
        |entry| {
            let parameters = by_directory.get(&entry.sequence).copied();
            let end = parameters.map_or(0, |record| clamped_primary_end(entry.sequence, record));
            let count = overdeclared_counts.counted_tail(entry.sequence, parameters, end, 1, 1);
            Ok(NativeDefinitionLevels {
                id: format_retained(
                    ctx,
                    format_args!("iges:presentation:definition-levels#D{}", entry.sequence),
                    "iges native definition levels id",
                )?,
                source_entity: format_retained(
                    ctx,
                    format_args!("iges:entity:directory#{}", entry.sequence),
                    "iges native definition levels source",
                )?,
                declared_count: parameters.and_then(|record| record.integer(1)),
                levels: collect_result_vec(
                    ctx,
                    count,
                    "iges native definition level values",
                    |index| Ok(parameters.and_then(|record| record.integer(2 + index))),
                )?,
            })
        },
    )?;
    let mut primitive_solids = Vec::new();
    for entry in directory
        .iter()
        .filter(|entry| matches!(entry.entity_type, 150 | 152 | 154 | 156 | 158 | 160 | 168))
    {
        let record = by_directory.get(&entry.sequence).copied();
        let number = |index| record.and_then(|record| record.number(index));
        let (kind, dimension_names, origin_start, x_axis_start, z_axis_start): (
            PrimitiveSolidKind,
            &[&str],
            usize,
            Option<usize>,
            Option<usize>,
        ) = match entry.entity_type {
            150 => (
                PrimitiveSolidKind::Block,
                &["x_length", "y_length", "z_length"],
                4,
                Some(7),
                Some(10),
            ),
            152 => (
                PrimitiveSolidKind::RightAngularWedge,
                &["x_length", "y_length", "z_length", "top_x_length"],
                5,
                Some(8),
                Some(11),
            ),
            154 => (
                PrimitiveSolidKind::RightCircularCylinder,
                &["height", "radius"],
                3,
                None,
                Some(6),
            ),
            156 => (
                PrimitiveSolidKind::RightCircularConeFrustum,
                &["height", "large_radius", "small_radius"],
                4,
                None,
                Some(7),
            ),
            158 => (PrimitiveSolidKind::Sphere, &["radius"], 2, None, None),
            160 => (
                PrimitiveSolidKind::Torus,
                &["major_radius", "minor_radius"],
                3,
                None,
                Some(6),
            ),
            168 => (
                PrimitiveSolidKind::Ellipsoid,
                &["x_radius", "y_radius", "z_radius"],
                4,
                Some(7),
                Some(10),
            ),
            _ => continue,
        };
        reserve_vec_growth(
            ctx,
            &mut primitive_solids,
            1,
            "iges native primitive solid slots",
        )?;
        let mut dimensions = BTreeMap::new();
        for (index, name) in dimension_names.iter().enumerate() {
            let key = format_retained(
                ctx,
                format_args!("{name}"),
                "iges native primitive dimension name",
            )?;
            insert_optional_btree_map(
                Some(ctx),
                &mut dimensions,
                key,
                number(index + 1),
                "iges native primitive dimension node",
            )?;
        }
        let axis = |start: usize| [number(start), number(start + 1), number(start + 2)];
        primitive_solids.push(NativePrimitiveSolid {
            id: format_retained(
                ctx,
                format_args!("iges:solid:primitive#D{}", entry.sequence),
                "iges native primitive solid id",
            )?,
            source_entity: format_retained(
                ctx,
                format_args!("iges:entity:directory#{}", entry.sequence),
                "iges native primitive solid source",
            )?,
            kind,
            dimensions,
            origin: axis(origin_start),
            x_axis: x_axis_start.map(axis),
            z_axis: z_axis_start.map(axis),
            transformation: (entry.transform > 0)
                .then(|| {
                    format_retained(
                        ctx,
                        format_args!("iges:native:transformation#D{}", entry.transform),
                        "iges native primitive transformation",
                    )
                })
                .transpose()?,
        });
    }
    let procedural_solids = collect_native_items(
        ctx,
        directory
            .iter()
            .filter(|entry| matches!(entry.entity_type, 162 | 164)),
        "iges native procedural solid slots",
        |entry| {
            let record = by_directory.get(&entry.sequence).copied();
            let number = |index| record.and_then(|record| record.number(index));
            let axis = |start: usize| [number(start), number(start + 1), number(start + 2)];
            let revolution = entry.entity_type == 162;
            Ok(NativeProceduralSolid {
                id: format_retained(
                    ctx,
                    format_args!("iges:solid:procedural#D{}", entry.sequence),
                    "iges native procedural solid id",
                )?,
                source_entity: format_retained(
                    ctx,
                    format_args!("iges:entity:directory#{}", entry.sequence),
                    "iges native procedural solid source",
                )?,
                kind: if revolution {
                    ProceduralSolidKind::Revolution
                } else {
                    ProceduralSolidKind::LinearExtrusion
                },
                form: entry.form,
                profile: record
                    .and_then(|record| record.integer(1))
                    .map(|sequence| {
                        parameter_resolver.resolve(
                            entry.sequence,
                            1,
                            sequence,
                            ReferenceExpectation::Named(ExpectationLabel::CurveEntity),
                            |target| {
                                matches!(
                                    target.entity_type,
                                    100 | 102 | 104 | 106 | 110 | 112 | 126 | 130
                                )
                            },
                        )
                    })
                    .transpose()?
                    .flatten()
                    .map(|sequence| {
                        format_retained(
                            ctx,
                            format_args!("iges:entity:directory#{sequence}"),
                            "iges native procedural profile",
                        )
                    })
                    .transpose()?,
                amount: number(2),
                origin: revolution.then(|| axis(3)),
                direction: axis(if revolution { 6 } else { 3 }),
                transformation: (entry.transform > 0)
                    .then(|| {
                        format_retained(
                            ctx,
                            format_args!("iges:native:transformation#D{}", entry.transform),
                            "iges native procedural transformation",
                        )
                    })
                    .transpose()?,
            })
        },
    )?;
    let boolean_trees = collect_native_items(
        ctx,
        directory
            .iter()
            .filter(|entry| entry.entity_type == 180 && matches!(entry.form, 0 | 1)),
        "iges native boolean tree slots",
        |entry| {
            let record = by_directory.get(&entry.sequence).copied();
            let end = record.map_or(0, |record| clamped_primary_end(entry.sequence, record));
            let count = overdeclared_counts.counted_tail(entry.sequence, record, end, 1, 1);
            let mut terms = Vec::new();
            for index in 0..count {
                let Some(value) = record.and_then(|record| record.integer(2 + index)) else {
                    continue;
                };
                reserve_vec_growth(ctx, &mut terms, 1, "iges native boolean term slots")?;
                terms.push(if value < 0 {
                    NativeBooleanTerm::Operand {
                        entity: parameter_resolver
                            .resolve_negative(
                                entry.sequence,
                                2 + index,
                                value,
                                if entry.form == 1 {
                                    ReferenceExpectation::Named(
                                        ExpectationLabel::ConstructiveSolidOrType186,
                                    )
                                } else {
                                    ReferenceExpectation::Named(ExpectationLabel::ConstructiveSolid)
                                },
                                |target| {
                                    matches!(
                                        target.entity_type,
                                        150 | 152
                                            | 154
                                            | 156
                                            | 158
                                            | 160
                                            | 162
                                            | 164
                                            | 168
                                            | 180
                                            | 430
                                    ) || (entry.form == 1 && target.entity_type == 186)
                                },
                            )?
                            .map(|sequence| {
                                format_retained(
                                    ctx,
                                    format_args!("iges:entity:directory#{sequence}"),
                                    "iges native boolean operand",
                                )
                            })
                            .transpose()?,
                        raw: value,
                    }
                } else {
                    NativeBooleanTerm::Operation { operation: value }
                });
            }
            Ok(NativeBooleanTree {
                id: format_retained(
                    ctx,
                    format_args!("iges:solid:boolean-tree#D{}", entry.sequence),
                    "iges native boolean tree id",
                )?,
                source_entity: format_retained(
                    ctx,
                    format_args!("iges:entity:directory#{}", entry.sequence),
                    "iges native boolean tree source",
                )?,
                form: entry.form,
                declared_length: record.and_then(|record| record.integer(1)),
                terms,
                transformation: (entry.transform > 0)
                    .then(|| {
                        format_retained(
                            ctx,
                            format_args!("iges:native:transformation#D{}", entry.transform),
                            "iges native boolean transformation",
                        )
                    })
                    .transpose()?,
            })
        },
    )?;
    let selected_components = collect_native_items(
        ctx,
        directory
            .iter()
            .filter(|entry| entry.entity_type == 182 && entry.form == 0),
        "iges native selected component slots",
        |entry| {
            let record = by_directory.get(&entry.sequence).copied();
            Ok(NativeSelectedComponent {
                id: format_retained(
                    ctx,
                    format_args!("iges:solid:selected-component#D{}", entry.sequence),
                    "iges native selected component id",
                )?,
                source_entity: format_retained(
                    ctx,
                    format_args!("iges:entity:directory#{}", entry.sequence),
                    "iges native selected component source",
                )?,
                boolean_tree: record
                    .and_then(|record| record.integer(1))
                    .map(|sequence| {
                        parameter_resolver.resolve_type(entry.sequence, 1, sequence, 180, &[0, 1])
                    })
                    .transpose()?
                    .flatten()
                    .map(|sequence| {
                        format_retained(
                            ctx,
                            format_args!("iges:solid:boolean-tree#D{sequence}"),
                            "iges native selected boolean tree",
                        )
                    })
                    .transpose()?,
                selection_point: [
                    record.and_then(|record| record.number(2)),
                    record.and_then(|record| record.number(3)),
                    record.and_then(|record| record.number(4)),
                ],
                transformation: (entry.transform > 0)
                    .then(|| {
                        format_retained(
                            ctx,
                            format_args!("iges:native:transformation#D{}", entry.transform),
                            "iges native selected transformation",
                        )
                    })
                    .transpose()?,
            })
        },
    )?;
    let solid_assemblies = collect_native_items(
        ctx,
        directory
            .iter()
            .filter(|entry| entry.entity_type == 184 && matches!(entry.form, 0 | 1)),
        "iges native solid assembly slots",
        |entry| {
            let record = by_directory.get(&entry.sequence).copied();
            let end = record.map_or(0, |record| clamped_primary_end(entry.sequence, record));
            let count = overdeclared_counts.counted_complete(entry.sequence, record, end, 1, 2, 2);
            Ok(NativeSolidAssembly {
                id: format_retained(
                    ctx,
                    format_args!("iges:product:solid-assembly#D{}", entry.sequence),
                    "iges native solid assembly id",
                )?,
                source_entity: format_retained(
                    ctx,
                    format_args!("iges:entity:directory#{}", entry.sequence),
                    "iges native solid assembly source",
                )?,
                form: entry.form,
                declared_count: record.and_then(|record| record.integer(1)),
                items: collect_result_vec(
                    ctx,
                    count,
                    "iges native solid assembly item slots",
                    |index| {
                        Ok(NativeAssemblyItem {
                            item: record
                                .and_then(|record| record.integer(2 + index))
                                .map(|sequence| {
                                    parameter_resolver.resolve(
                                        entry.sequence,
                                        2 + index,
                                        sequence,
                                        if entry.form == 1 {
                                            ReferenceExpectation::Named(
                                                ExpectationLabel::ConstructiveSolidOrType186,
                                            )
                                        } else {
                                            ReferenceExpectation::Named(
                                                ExpectationLabel::ConstructiveSolid,
                                            )
                                        },
                                        |target| {
                                            matches!(
                                                target.entity_type,
                                                150 | 152
                                                    | 154
                                                    | 156
                                                    | 158
                                                    | 160
                                                    | 162
                                                    | 164
                                                    | 168
                                                    | 180
                                                    | 184
                                                    | 430
                                            ) || (entry.form == 1 && target.entity_type == 186)
                                        },
                                    )
                                })
                                .transpose()?
                                .flatten()
                                .map(|sequence| {
                                    format_retained(
                                        ctx,
                                        format_args!("iges:entity:directory#{sequence}"),
                                        "iges native solid assembly member",
                                    )
                                })
                                .transpose()?,
                            transformation: record
                                .and_then(|record| record.integer(2 + count + index))
                                .filter(|sequence| *sequence != 0)
                                .map(|sequence| {
                                    parameter_resolver.resolve_type(
                                        entry.sequence,
                                        2 + count + index,
                                        sequence,
                                        124,
                                        &[],
                                    )
                                })
                                .transpose()?
                                .flatten()
                                .map(|sequence| {
                                    format_retained(
                                        ctx,
                                        format_args!("iges:native:transformation#D{sequence}"),
                                        "iges native solid assembly item transform",
                                    )
                                })
                                .transpose()?,
                        })
                    },
                )?,
                transformation: (entry.transform > 0)
                    .then(|| {
                        format_retained(
                            ctx,
                            format_args!("iges:native:transformation#D{}", entry.transform),
                            "iges native solid assembly transformation",
                        )
                    })
                    .transpose()?,
            })
        },
    )?;
    // IGES 5.3 §4.49 lays out Type 186 as SHELL at parameter index 1, the
    // orientation flag at 2, the void count N at 3, and one (VOID, VOF) pair
    // per void shell from index 4. §4.147 forbids an MSBO from pointing at a
    // Form 2 open shell, so the outer shell and every void resolve strictly
    // against Type 514 Form 1.
    let manifold_solids = collect_native_items(
        ctx,
        directory
            .iter()
            .filter(|entry| entry.entity_type == 186 && entry.form == 0),
        "iges native manifold solid slots",
        |entry| {
            let record = by_directory.get(&entry.sequence).copied();
            let end = record.map_or(0, |record| clamped_primary_end(entry.sequence, record));
            let count = overdeclared_counts.counted_tail(entry.sequence, record, end, 3, 2);
            let closed_shell = |index: usize| -> Result<Option<String>, CodecError> {
                record
                    .and_then(|record| record.integer(index))
                    .map(|sequence| {
                        parameter_resolver.resolve_type(entry.sequence, index, sequence, 514, &[1])
                    })
                    .transpose()?
                    .flatten()
                    .map(|sequence| {
                        format_retained(
                            ctx,
                            format_args!("iges:entity:directory#{sequence}"),
                            "iges native manifold shell",
                        )
                    })
                    .transpose()
            };
            Ok(NativeManifoldSolid {
                id: format_retained(
                    ctx,
                    format_args!("iges:solid:manifold-brep#D{}", entry.sequence),
                    "iges native manifold solid id",
                )?,
                source_entity: format_retained(
                    ctx,
                    format_args!("iges:entity:directory#{}", entry.sequence),
                    "iges native manifold solid source",
                )?,
                shell: closed_shell(1)?,
                shell_orientation: record.and_then(|record| record.integer(2)),
                declared_void_count: record.and_then(|record| record.integer(3)),
                // Struct fields evaluate in written order, so the outer shell
                // records its reference edge before any void pair records its
                // own, pinning the serialized edge order to ascending
                // parameter index.
                voids: collect_result_vec(
                    ctx,
                    count,
                    "iges native manifold void slots",
                    |index| {
                        Ok(NativeVoidShell {
                            shell: closed_shell(4 + index * 2)?,
                            orientation: record.and_then(|record| record.integer(5 + index * 2)),
                        })
                    },
                )?,
                transformation: (entry.transform > 0)
                    .then(|| {
                        format_retained(
                            ctx,
                            format_args!("iges:native:transformation#D{}", entry.transform),
                            "iges native manifold transformation",
                        )
                    })
                    .transpose()?,
            })
        },
    )?;
    let solid_instances = collect_native_items(
        ctx,
        directory
            .iter()
            .filter(|entry| entry.entity_type == 430 && matches!(entry.form, 0 | 1)),
        "iges native solid instance slots",
        |entry| {
            let record = by_directory.get(&entry.sequence).copied();
            Ok(NativeSolidInstance {
                id: format_retained(
                    ctx,
                    format_args!("iges:product:solid-instance#D{}", entry.sequence),
                    "iges native solid instance id",
                )?,
                source_entity: format_retained(
                    ctx,
                    format_args!("iges:entity:directory#{}", entry.sequence),
                    "iges native solid instance source",
                )?,
                form: entry.form,
                solid: record
                    .and_then(|record| record.integer(1))
                    .map(|sequence| {
                        if entry.form == 1 {
                            parameter_resolver.resolve_type(entry.sequence, 1, sequence, 186, &[])
                        } else {
                            parameter_resolver.resolve(
                                entry.sequence,
                                1,
                                sequence,
                                ReferenceExpectation::Named(ExpectationLabel::ConstructiveSolid),
                                |target| {
                                    matches!(
                                        target.entity_type,
                                        150 | 152
                                            | 154
                                            | 156
                                            | 158
                                            | 160
                                            | 162
                                            | 164
                                            | 168
                                            | 180
                                            | 184
                                            | 430
                                    )
                                },
                            )
                        }
                    })
                    .transpose()?
                    .flatten()
                    .map(|sequence| {
                        format_retained(
                            ctx,
                            format_args!("iges:entity:directory#{sequence}"),
                            "iges native solid instance target",
                        )
                    })
                    .transpose()?,
                transformation: (entry.transform > 0)
                    .then(|| {
                        format_retained(
                            ctx,
                            format_args!("iges:native:transformation#D{}", entry.transform),
                            "iges native solid instance transformation",
                        )
                    })
                    .transpose()?,
            })
        },
    )?;
    let subfigure_definitions = collect_native_items(
        ctx,
        directory
            .iter()
            .filter(|entry| entry.entity_type == 308 && entry.form == 0),
        "iges native subfigure definition slots",
        |entry| {
            let record = by_directory.get(&entry.sequence).copied();
            let end = record.map_or(0, |record| clamped_primary_end(entry.sequence, record));
            let count = overdeclared_counts.counted_tail(entry.sequence, record, end, 3, 1);
            Ok(NativeSubfigureDefinition {
                id: format_retained(
                    ctx,
                    format_args!("iges:product:subfigure-definition#D{}", entry.sequence),
                    "iges native subfigure definition id",
                )?,
                source_entity: format_retained(
                    ctx,
                    format_args!("iges:entity:directory#{}", entry.sequence),
                    "iges native subfigure definition source",
                )?,
                depth: record.and_then(|record| record.integer(1)),
                name: record
                    .and_then(|record| record.string(2))
                    .map(|bytes| ctx.copy_retained(bytes, "iges native subfigure name"))
                    .transpose()?,
                declared_member_count: record.and_then(|record| record.integer(3)),
                members: collect_result_vec(
                    ctx,
                    count,
                    "iges native subfigure member slots",
                    |index| {
                        record
                            .and_then(|record| record.integer(4 + index))
                            .map(|sequence| {
                                parameter_resolver.resolve_any(entry.sequence, 4 + index, sequence)
                            })
                            .transpose()?
                            .flatten()
                            .map(|sequence| {
                                format_retained(
                                    ctx,
                                    format_args!("iges:entity:directory#{sequence}"),
                                    "iges native subfigure member",
                                )
                            })
                            .transpose()
                    },
                )?,
                transformation: (entry.transform > 0)
                    .then(|| {
                        format_retained(
                            ctx,
                            format_args!("iges:native:transformation#D{}", entry.transform),
                            "iges native subfigure definition transform",
                        )
                    })
                    .transpose()?,
                label_display: resolved_label_display_definition(
                    ctx,
                    references,
                    entry.sequence,
                    entry.label_display,
                )?,
            })
        },
    )?;
    let subfigure_instances = collect_native_items(
        ctx,
        directory
            .iter()
            .filter(|entry| entry.entity_type == 408 && entry.form == 0),
        "iges native subfigure instance slots",
        |entry| {
            let record = by_directory.get(&entry.sequence).copied();
            Ok(NativeSubfigureInstance {
                id: format_retained(
                    ctx,
                    format_args!("iges:product:subfigure-instance#D{}", entry.sequence),
                    "iges native subfigure instance id",
                )?,
                source_entity: format_retained(
                    ctx,
                    format_args!("iges:entity:directory#{}", entry.sequence),
                    "iges native subfigure instance source",
                )?,
                definition: record
                    .and_then(|record| record.integer(1))
                    .map(|sequence| {
                        parameter_resolver.resolve_type(entry.sequence, 1, sequence, 308, &[0])
                    })
                    .transpose()?
                    .flatten()
                    .map(|sequence| {
                        format_retained(
                            ctx,
                            format_args!("iges:product:subfigure-definition#D{sequence}"),
                            "iges native subfigure instance definition",
                        )
                    })
                    .transpose()?,
                translation: [
                    record.and_then(|record| record.number(2)),
                    record.and_then(|record| record.number(3)),
                    record.and_then(|record| record.number(4)),
                ],
                scale: record.and_then(|record| record.number(5)),
                transformation: (entry.transform > 0)
                    .then(|| {
                        format_retained(
                            ctx,
                            format_args!("iges:native:transformation#D{}", entry.transform),
                            "iges native subfigure instance transform",
                        )
                    })
                    .transpose()?,
            })
        },
    )?;
    let network_definitions = collect_native_items(
        ctx,
        directory
            .iter()
            .filter(|entry| entry.entity_type == 320 && entry.form == 0),
        "iges native network definition slots",
        |entry| {
            let record = by_directory.get(&entry.sequence).copied();
            let member_count = record.and_then(|record| {
                let end = clamped_primary_end(entry.sequence, record);
                let count = record.count_with_stride_before(3, 1, end)?;
                let type_flag = 4 + count;
                let primary_reference_designator = type_flag + 1;
                let display_template = type_flag + 2;
                let connect_count = type_flag + 3;
                (record.integer(type_flag).is_some()
                    && record
                        .string_or_empty(primary_reference_designator)
                        .is_some()
                    && record.integer_or(display_template, 0).is_some()
                    && record.integer(connect_count).is_some())
                .then_some(count)
            });
            let connect_count_index = member_count.map(|count| 7 + count);
            let connect_count = record.zip(connect_count_index).and_then(|(record, index)| {
                record.count_with_stride_before(
                    index,
                    1,
                    clamped_primary_end(entry.sequence, record),
                )
            });
            Ok(NativeNetworkDefinition {
                id: format_retained(
                    ctx,
                    format_args!("iges:product:network-definition#D{}", entry.sequence),
                    "iges native network definition id",
                )?,
                source_entity: format_retained(
                    ctx,
                    format_args!("iges:entity:directory#{}", entry.sequence),
                    "iges native network definition source",
                )?,
                depth: record.and_then(|record| record.integer(1)),
                name: record
                    .and_then(|record| record.string(2))
                    .map(|bytes| ctx.copy_retained(bytes, "iges native network name"))
                    .transpose()?,
                declared_member_count: record.and_then(|record| record.integer(3)),
                members: if let Some(member_count) = member_count {
                    collect_result_vec(
                        ctx,
                        member_count,
                        "iges native network member slots",
                        |index| {
                            record
                                .and_then(|record| record.integer(4 + index))
                                .map(|sequence| {
                                    parameter_resolver.resolve_any(
                                        entry.sequence,
                                        4 + index,
                                        sequence,
                                    )
                                })
                                .transpose()?
                                .flatten()
                                .map(|sequence| {
                                    format_retained(
                                        ctx,
                                        format_args!("iges:entity:directory#{sequence}"),
                                        "iges native network member",
                                    )
                                })
                                .transpose()
                        },
                    )?
                } else {
                    Vec::new()
                },
                type_flag: member_count.and_then(|member_count| {
                    record.and_then(|record| record.integer(4 + member_count))
                }),
                primary_reference_designator: member_count
                    .and_then(|member_count| {
                        record.and_then(|record| record.string_or_empty(5 + member_count))
                    })
                    .filter(|value| !value.is_empty())
                    .map(|bytes| ctx.copy_retained(bytes, "iges native network designator"))
                    .transpose()?,
                display_template: member_count
                    .and_then(|member_count| {
                        record
                            .and_then(|record| record.integer_or(6 + member_count, 0))
                            .filter(|sequence| *sequence != 0)
                            .map(|sequence| (6 + member_count, sequence))
                    })
                    .map(|(index, sequence)| {
                        parameter_resolver.resolve_type(
                            entry.sequence,
                            index,
                            sequence,
                            312,
                            &[0, 1],
                        )
                    })
                    .transpose()?
                    .flatten()
                    .map(|sequence| {
                        format_retained(
                            ctx,
                            format_args!("iges:entity:directory#{sequence}"),
                            "iges native network template",
                        )
                    })
                    .transpose()?,
                declared_connect_point_count: member_count.and_then(|member_count| {
                    record.and_then(|record| record.integer(7 + member_count))
                }),
                connect_points: if let Some((member_count, connect_count)) =
                    member_count.zip(connect_count)
                {
                    collect_result_vec(
                        ctx,
                        connect_count,
                        "iges native network connect point slots",
                        |index| {
                            record
                                .and_then(|record| record.integer(8 + member_count + index))
                                .filter(|sequence| *sequence != 0)
                                .map(|sequence| {
                                    parameter_resolver.resolve_type(
                                        entry.sequence,
                                        8 + member_count + index,
                                        sequence,
                                        132,
                                        &[0],
                                    )
                                })
                                .transpose()?
                                .flatten()
                                .map(|sequence| {
                                    format_retained(
                                        ctx,
                                        format_args!("iges:entity:directory#{sequence}"),
                                        "iges native network connect point",
                                    )
                                })
                                .transpose()
                        },
                    )?
                } else {
                    Vec::new()
                },
                transformation: (entry.transform > 0)
                    .then(|| {
                        format_retained(
                            ctx,
                            format_args!("iges:native:transformation#D{}", entry.transform),
                            "iges native network definition transform",
                        )
                    })
                    .transpose()?,
                label_display: resolved_label_display_definition(
                    ctx,
                    references,
                    entry.sequence,
                    entry.label_display,
                )?,
            })
        },
    )?;
    let network_instances = collect_native_items(
        ctx,
        directory
            .iter()
            .filter(|entry| entry.entity_type == 420 && entry.form == 0),
        "iges native network instance slots",
        |entry| {
            let record = by_directory.get(&entry.sequence).copied();
            let end = record.map_or(0, |record| clamped_primary_end(entry.sequence, record));
            let connect_count =
                overdeclared_counts.counted_tail(entry.sequence, record, end, 11, 1);
            Ok(NativeNetworkInstance {
                id: format_retained(
                    ctx,
                    format_args!("iges:product:network-instance#D{}", entry.sequence),
                    "iges native network instance id",
                )?,
                source_entity: format_retained(
                    ctx,
                    format_args!("iges:entity:directory#{}", entry.sequence),
                    "iges native network instance source",
                )?,
                definition: record
                    .and_then(|record| record.integer(1))
                    .map(|sequence| {
                        parameter_resolver.resolve_type(entry.sequence, 1, sequence, 320, &[0])
                    })
                    .transpose()?
                    .flatten()
                    .map(|sequence| {
                        format_retained(
                            ctx,
                            format_args!("iges:product:network-definition#D{sequence}"),
                            "iges native network instance definition",
                        )
                    })
                    .transpose()?,
                translation: [
                    record.and_then(|record| record.number(2)),
                    record.and_then(|record| record.number(3)),
                    record.and_then(|record| record.number(4)),
                ],
                scale: [
                    record.and_then(|record| record.number(5)),
                    record.and_then(|record| record.number(6)),
                    record.and_then(|record| record.number(7)),
                ],
                type_flag: record.and_then(|record| record.integer(8)),
                primary_reference_designator: record
                    .and_then(|record| record.string_or_empty(9))
                    .filter(|value| !value.is_empty())
                    .map(|bytes| {
                        ctx.copy_retained(bytes, "iges native network instance designator")
                    })
                    .transpose()?,
                display_template: record
                    .and_then(|record| record.integer_or(10, 0))
                    .filter(|sequence| *sequence != 0)
                    .map(|sequence| {
                        parameter_resolver.resolve_type(entry.sequence, 10, sequence, 312, &[0, 1])
                    })
                    .transpose()?
                    .flatten()
                    .map(|sequence| {
                        format_retained(
                            ctx,
                            format_args!("iges:entity:directory#{sequence}"),
                            "iges native network instance template",
                        )
                    })
                    .transpose()?,
                declared_connect_point_count: record.and_then(|record| record.integer(11)),
                connect_points: collect_result_vec(
                    ctx,
                    connect_count,
                    "iges native network instance connect point slots",
                    |index| {
                        record
                            .and_then(|record| record.integer(12 + index))
                            .filter(|sequence| *sequence != 0)
                            .map(|sequence| {
                                parameter_resolver.resolve_type(
                                    entry.sequence,
                                    12 + index,
                                    sequence,
                                    132,
                                    &[0],
                                )
                            })
                            .transpose()?
                            .flatten()
                            .map(|sequence| {
                                format_retained(
                                    ctx,
                                    format_args!("iges:entity:directory#{sequence}"),
                                    "iges native network instance connect point",
                                )
                            })
                            .transpose()
                    },
                )?,
                transformation: (entry.transform > 0)
                    .then(|| {
                        format_retained(
                            ctx,
                            format_args!("iges:native:transformation#D{}", entry.transform),
                            "iges native network instance transform",
                        )
                    })
                    .transpose()?,
            })
        },
    )?;
    let connect_points = collect_native_items(
        ctx,
        directory
            .iter()
            .filter(|entry| entry.entity_type == 132 && entry.form == 0),
        "iges native connect point slots",
        |entry| {
            let record = by_directory.get(&entry.sequence).copied();
            let optional_entity_link = |index| -> Result<Option<String>, CodecError> {
                record
                    .and_then(|record| record.integer(index))
                    .filter(|sequence| *sequence != 0)
                    .map(|sequence| parameter_resolver.resolve_any(entry.sequence, index, sequence))
                    .transpose()?
                    .flatten()
                    .map(|sequence| {
                        format_retained(
                            ctx,
                            format_args!("iges:entity:directory#{sequence}"),
                            "iges native connect point link",
                        )
                    })
                    .transpose()
            };
            let optional_template_link = |index| -> Result<Option<String>, CodecError> {
                record
                    .and_then(|record| record.integer(index))
                    .filter(|sequence| *sequence != 0)
                    .map(|sequence| {
                        parameter_resolver.resolve_type(
                            entry.sequence,
                            index,
                            sequence,
                            312,
                            &[0, 1],
                        )
                    })
                    .transpose()?
                    .flatten()
                    .map(|sequence| {
                        format_retained(
                            ctx,
                            format_args!("iges:entity:directory#{sequence}"),
                            "iges native connect point template",
                        )
                    })
                    .transpose()
            };
            Ok(NativeConnectPoint {
                id: format_retained(
                    ctx,
                    format_args!("iges:product:connect-point#D{}", entry.sequence),
                    "iges native connect point id",
                )?,
                source_entity: format_retained(
                    ctx,
                    format_args!("iges:entity:directory#{}", entry.sequence),
                    "iges native connect point source",
                )?,
                position: [
                    record.and_then(|record| record.number(1)),
                    record.and_then(|record| record.number(2)),
                    record.and_then(|record| record.number(3)),
                ],
                display_geometry: optional_entity_link(4)?,
                type_flag: record.and_then(|record| record.integer(5)),
                function_flag: record.and_then(|record| record.integer(6)),
                function_identifier: record
                    .and_then(|record| record.string(7))
                    .map(|bytes| {
                        ctx.copy_retained(bytes, "iges native connect function identifier")
                    })
                    .transpose()?,
                identifier_display_template: optional_template_link(8)?,
                function_name: record
                    .and_then(|record| record.string(9))
                    .map(|bytes| ctx.copy_retained(bytes, "iges native connect function name"))
                    .transpose()?,
                name_display_template: optional_template_link(10)?,
                identifier: record.and_then(|record| record.integer(11)),
                function_code: record.and_then(|record| record.integer(12)),
                swap_flag: record.and_then(|record| record.integer(13)),
                owner: record
                    .and_then(|record| record.integer(14))
                    .filter(|sequence| *sequence != 0)
                    .map(|sequence| {
                        parameter_resolver.resolve(
                            entry.sequence,
                            14,
                            sequence,
                            ReferenceExpectation::AnyOf {
                                first: 320,
                                second: 420,
                                rest: vec![],
                            },
                            |target| matches!(target.entity_type, 320 | 420),
                        )
                    })
                    .transpose()?
                    .flatten()
                    .map(|sequence| {
                        format_retained(
                            ctx,
                            format_args!("iges:entity:directory#{sequence}"),
                            "iges native connect point owner",
                        )
                    })
                    .transpose()?,
                transformation: (entry.transform > 0)
                    .then(|| {
                        format_retained(
                            ctx,
                            format_args!("iges:native:transformation#D{}", entry.transform),
                            "iges native connect point transform",
                        )
                    })
                    .transpose()?,
            })
        },
    )?;
    let rectangular_arrays = collect_native_items(
        ctx,
        directory
            .iter()
            .filter(|entry| entry.entity_type == 412 && entry.form == 0),
        "iges native rectangular array slots",
        |entry| {
            let record = by_directory.get(&entry.sequence).copied();
            let end = record.map_or(0, |record| clamped_primary_end(entry.sequence, record));
            let count = overdeclared_counts.counted_tail_at(entry.sequence, record, end, 11, 13, 1);
            Ok(NativeRectangularArray {
                id: format_retained(
                    ctx,
                    format_args!("iges:product:rectangular-array#D{}", entry.sequence),
                    "iges native rectangular array id",
                )?,
                source_entity: format_retained(
                    ctx,
                    format_args!("iges:entity:directory#{}", entry.sequence),
                    "iges native rectangular array source",
                )?,
                base: record
                    .and_then(|record| record.integer(1))
                    .map(|sequence| {
                        parameter_resolver.resolve(
                            entry.sequence,
                            1,
                            sequence,
                            ReferenceExpectation::Named(ExpectationLabel::ArrayBaseEntity),
                            |target| array_base_type(target.entity_type, target.form),
                        )
                    })
                    .transpose()?
                    .flatten()
                    .map(|sequence| {
                        format_retained(
                            ctx,
                            format_args!("iges:entity:directory#{sequence}"),
                            "iges native rectangular array base",
                        )
                    })
                    .transpose()?,
                scale: record.and_then(|record| record.number(2)),
                origin: [
                    record.and_then(|record| record.number(3)),
                    record.and_then(|record| record.number(4)),
                    record.and_then(|record| record.number(5)),
                ],
                columns: record.and_then(|record| record.integer(6)),
                rows: record.and_then(|record| record.integer(7)),
                column_spacing: record.and_then(|record| record.number(8)),
                row_spacing: record.and_then(|record| record.number(9)),
                rotation: record.and_then(|record| record.number(10)),
                do_dont_flag: record.and_then(|record| record.integer(12)),
                positions: collect_result_vec(
                    ctx,
                    count,
                    "iges native rectangular position slots",
                    |index| Ok(record.and_then(|record| record.integer(13 + index))),
                )?,
                transformation: (entry.transform > 0)
                    .then(|| {
                        format_retained(
                            ctx,
                            format_args!("iges:native:transformation#D{}", entry.transform),
                            "iges native rectangular array transform",
                        )
                    })
                    .transpose()?,
            })
        },
    )?;
    let circular_arrays = collect_native_items(
        ctx,
        directory
            .iter()
            .filter(|entry| entry.entity_type == 414 && entry.form == 0),
        "iges native circular array slots",
        |entry| {
            let record = by_directory.get(&entry.sequence).copied();
            let end = record.map_or(0, |record| clamped_primary_end(entry.sequence, record));
            let count = overdeclared_counts.counted_tail_at(entry.sequence, record, end, 9, 11, 1);
            Ok(NativeCircularArray {
                id: format_retained(
                    ctx,
                    format_args!("iges:product:circular-array#D{}", entry.sequence),
                    "iges native circular array id",
                )?,
                source_entity: format_retained(
                    ctx,
                    format_args!("iges:entity:directory#{}", entry.sequence),
                    "iges native circular array source",
                )?,
                base: record
                    .and_then(|record| record.integer(1))
                    .map(|sequence| {
                        parameter_resolver.resolve(
                            entry.sequence,
                            1,
                            sequence,
                            ReferenceExpectation::Named(ExpectationLabel::ArrayBaseEntity),
                            |target| array_base_type(target.entity_type, target.form),
                        )
                    })
                    .transpose()?
                    .flatten()
                    .map(|sequence| {
                        format_retained(
                            ctx,
                            format_args!("iges:entity:directory#{sequence}"),
                            "iges native circular array base",
                        )
                    })
                    .transpose()?,
                location_count: record.and_then(|record| record.integer(2)),
                center: [
                    record.and_then(|record| record.number(3)),
                    record.and_then(|record| record.number(4)),
                    record.and_then(|record| record.number(5)),
                ],
                radius: record.and_then(|record| record.number(6)),
                start_angle: record.and_then(|record| record.number(7)),
                delta_angle: record.and_then(|record| record.number(8)),
                do_dont_flag: record.and_then(|record| record.integer(10)),
                positions: collect_result_vec(
                    ctx,
                    count,
                    "iges native circular position slots",
                    |index| Ok(record.and_then(|record| record.integer(11 + index))),
                )?,
                transformation: (entry.transform > 0)
                    .then(|| {
                        format_retained(
                            ctx,
                            format_args!("iges:native:transformation#D{}", entry.transform),
                            "iges native circular array transform",
                        )
                    })
                    .transpose()?,
            })
        },
    )?;
    let mut external_references = Vec::new();
    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 416 && matches!(entry.form, 0..=4))
    {
        let record = by_directory.get(&entry.sequence).copied();
        let (reference_kind, file_index, symbolic_index, library_index) = match entry.form {
            0 => (
                ExternalReferenceKind::ExternalDefinition,
                Some(1),
                Some(2),
                None,
            ),
            1 => (
                ExternalReferenceKind::ExternalFileDefinition,
                Some(1),
                None,
                None,
            ),
            2 => (
                ExternalReferenceKind::ExternalLogical,
                Some(1),
                Some(2),
                None,
            ),
            3 => (ExternalReferenceKind::NativeDefinition, None, Some(1), None),
            4 => (
                ExternalReferenceKind::NativeLibraryDefinition,
                None,
                Some(2),
                Some(1),
            ),
            _ => continue,
        };
        reserve_vec_growth(
            ctx,
            &mut external_references,
            1,
            "iges native external reference slots",
        )?;
        external_references.push(NativeExternalReference {
            id: format_retained(
                ctx,
                format_args!("iges:product:external-reference#D{}", entry.sequence),
                "iges native external reference id",
            )?,
            source_entity: format_retained(
                ctx,
                format_args!("iges:entity:directory#{}", entry.sequence),
                "iges native external reference source",
            )?,
            reference_kind,
            file_identifier: file_index
                .and_then(|index| {
                    record.and_then(|record| record.string(index)).map(|bytes| {
                        ctx.copy_retained(bytes, "iges native external file identifier")
                    })
                })
                .transpose()?,
            symbolic_name: symbolic_index
                .and_then(|index| {
                    record
                        .and_then(|record| record.string(index))
                        .map(|bytes| ctx.copy_retained(bytes, "iges native external symbolic name"))
                })
                .transpose()?,
            library_name: library_index
                .and_then(|index| {
                    record
                        .and_then(|record| record.string(index))
                        .map(|bytes| ctx.copy_retained(bytes, "iges native external library name"))
                })
                .transpose()?,
        });
    }
    let groups = collect_native_items(
        ctx,
        directory
            .iter()
            .filter(|entry| entry.entity_type == 402 && matches!(entry.form, 1 | 7 | 14 | 15)),
        "iges native group slots",
        |entry| {
            let record = by_directory.get(&entry.sequence).copied();
            let end = record.map_or(0, |record| clamped_primary_end(entry.sequence, record));
            let count = overdeclared_counts.counted_tail(entry.sequence, record, end, 1, 1);
            Ok(NativeGroup {
                id: format_retained(
                    ctx,
                    format_args!("iges:product:group#D{}", entry.sequence),
                    "iges native group id",
                )?,
                source_entity: format_retained(
                    ctx,
                    format_args!("iges:entity:directory#{}", entry.sequence),
                    "iges native group source",
                )?,
                ordered: matches!(entry.form, 14 | 15),
                back_pointers_required: matches!(entry.form, 1 | 14),
                declared_member_count: record.and_then(|record| record.integer(1)),
                members: collect_result_vec(
                    ctx,
                    count,
                    "iges native group member slots",
                    |index| {
                        record
                            .and_then(|record| record.integer(2 + index))
                            .map(|sequence| {
                                parameter_resolver.resolve_any(entry.sequence, 2 + index, sequence)
                            })
                            .transpose()?
                            .flatten()
                            .map(|sequence| {
                                format_retained(
                                    ctx,
                                    format_args!("iges:entity:directory#{sequence}"),
                                    "iges native group member",
                                )
                            })
                            .transpose()
                    },
                )?,
            })
        },
    )?;
    let mut associativities = collect_native_items(
        ctx,
        directory.iter().filter(|entry| entry.entity_type == 302),
        "iges native associativity definition slots",
        |entry| {
            let record = by_directory.get(&entry.sequence).copied();
            let class_count = record.and_then(|record| {
                record.count_with_stride_before(1, 1, clamped_primary_end(entry.sequence, record))
            });
            let end = record.map_or(0, |record| clamped_primary_end(entry.sequence, record));
            let mut cursor = 2_usize;
            let classes = if let Some(class_count) = class_count {
                let mut classes =
                    reserve_vec(ctx, class_count, "iges native associativity classes")?;
                let mut complete = true;
                for _ in 0..class_count {
                    let Some(record) = record else {
                        complete = false;
                        break;
                    };
                    let Some(count_index) = cursor.checked_add(2) else {
                        complete = false;
                        break;
                    };
                    let Some(item_count) = record.count_with_stride_before(count_index, 1, end)
                    else {
                        complete = false;
                        break;
                    };
                    let Some(next) = cursor.checked_add(3 + item_count) else {
                        complete = false;
                        break;
                    };
                    classes.push(NativeAssociativityClassDefinition {
                        back_pointers_required: record.integer(cursor).map(|value| value == 1),
                        ordered: record.integer(cursor + 1).map(|value| value == 1),
                        declared_item_count: record.integer(cursor + 2),
                        item_types: collect_result_vec(
                            ctx,
                            item_count,
                            "iges native associativity class item types",
                            |offset| Ok(record.integer(cursor + 3 + offset)),
                        )?,
                    });
                    cursor = next;
                }
                if complete {
                    classes
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            };
            Ok(NativeAssociativity::Definition {
                id: format_retained(
                    ctx,
                    format_args!("iges:structure:associativity#D{}", entry.sequence),
                    "iges native associativity definition id",
                )?,
                source_entity: format_retained(
                    ctx,
                    format_args!("iges:entity:directory#{}", entry.sequence),
                    "iges native associativity definition source",
                )?,
                associativity_form: entry.form,
                declared_class_count: record.and_then(|record| record.integer(1)),
                classes,
            })
        },
    )?;
    directory
        .iter()
        .filter(|entry| {
            entry.entity_type == 402
                && matches!(
                    entry.form,
                    2 | 5 | 6 | 8 | 9 | 10 | 11 | 12 | 13 | 16 | 18 | 20 | 21
                )
        })
        .try_for_each(|entry| -> Result<(), CodecError> {
            reserve_vec_growth(ctx, &mut associativities, 1, "iges native associativities")?;
            let record = by_directory.get(&entry.sequence).copied();
            let id = format_retained(
                ctx,
                format_args!("iges:structure:associativity#D{}", entry.sequence),
                "iges native associativity id",
            )?;
            let source_entity = format_retained(
                ctx,
                format_args!("iges:entity:directory#{}", entry.sequence),
                "iges native associativity source",
            )?;
            let entity_link = |index| -> Result<Option<String>, CodecError> {
                record
                    .and_then(|record| record.integer(index))
                    .filter(|sequence| *sequence != 0)
                    .map(|sequence| parameter_resolver.resolve_any(entry.sequence, index, sequence))
                    .transpose()?
                    .flatten()
                    .map(|sequence| {
                        format_retained(
                            ctx,
                            format_args!("iges:entity:directory#{sequence}"),
                            "iges native associativity link",
                        )
                    })
                    .transpose()
            };
            let association = match entry.form {
                5 => {
                    let end =
                        record.map_or(0, |record| clamped_primary_end(entry.sequence, record));
                    let count = overdeclared_counts.counted_tail(entry.sequence, record, end, 1, 7);
                    NativeAssociativity::LabelDisplay {
                        id,
                        source_entity,
                        declared_count: record.and_then(|record| record.integer(1)),
                        placements: collect_result_vec(
                            ctx,
                            count,
                            "iges native label placement slots",
                            |offset| {
                                let start = 2 + offset * 7;
                                Ok(NativeLabelPlacement {
                                    view: record
                                        .and_then(|record| record.integer(start))
                                        .map(|sequence| {
                                            parameter_resolver.resolve_type(
                                                entry.sequence,
                                                start,
                                                sequence,
                                                410,
                                                &[0, 1],
                                            )
                                        })
                                        .transpose()?
                                        .flatten()
                                        .map(|sequence| {
                                            format_retained(
                                                ctx,
                                                format_args!("iges:entity:directory#{sequence}"),
                                                "iges native label placement view",
                                            )
                                        })
                                        .transpose()?,
                                    text_location: [
                                        record.and_then(|record| record.number(start + 1)),
                                        record.and_then(|record| record.number(start + 2)),
                                        record.and_then(|record| record.number(start + 3)),
                                    ],
                                    leader: record
                                        .and_then(|record| record.integer(start + 4))
                                        .map(|sequence| {
                                            parameter_resolver.resolve_type(
                                                entry.sequence,
                                                start + 4,
                                                sequence,
                                                214,
                                                &[],
                                            )
                                        })
                                        .transpose()?
                                        .flatten()
                                        .map(|sequence| {
                                            format_retained(
                                                ctx,
                                                format_args!("iges:entity:directory#{sequence}"),
                                                "iges native label placement leader",
                                            )
                                        })
                                        .transpose()?,
                                    label_level: record
                                        .and_then(|record| record.integer(start + 5)),
                                    entity: entity_link(start + 6)?,
                                })
                            },
                        )?,
                    }
                }
                6 => {
                    let end =
                        record.map_or(0, |record| clamped_primary_end(entry.sequence, record));
                    let count =
                        overdeclared_counts.counted_tail_at(entry.sequence, record, end, 2, 4, 1);
                    NativeAssociativity::ViewList {
                        id,
                        source_entity,
                        declared_visible_count: record.and_then(|record| record.integer(2)),
                        view: record
                            .and_then(|record| record.integer(3))
                            .map(|sequence| {
                                parameter_resolver.resolve_type(
                                    entry.sequence,
                                    3,
                                    sequence,
                                    410,
                                    &[0, 1],
                                )
                            })
                            .transpose()?
                            .flatten()
                            .map(|sequence| {
                                format_retained(
                                    ctx,
                                    format_args!("iges:entity:directory#{sequence}"),
                                    "iges native view list view",
                                )
                            })
                            .transpose()?,
                        visible_entities: collect_result_vec(
                            ctx,
                            count,
                            "iges native visible entity slots",
                            |offset| entity_link(4 + offset),
                        )?,
                    }
                }
                9 => {
                    let end =
                        record.map_or(0, |record| clamped_primary_end(entry.sequence, record));
                    let count =
                        overdeclared_counts.counted_tail_at(entry.sequence, record, end, 2, 4, 1);
                    NativeAssociativity::SingleParent {
                        id,
                        source_entity,
                        declared_child_count: record.and_then(|record| record.integer(2)),
                        parent: entity_link(3)?,
                        children: collect_result_vec(
                            ctx,
                            count,
                            "iges native single-parent child slots",
                            |offset| entity_link(4 + offset),
                        )?,
                    }
                }
                2 | 12 => {
                    let end =
                        record.map_or(0, |record| clamped_primary_end(entry.sequence, record));
                    let count = overdeclared_counts.counted_tail(entry.sequence, record, end, 1, 2);
                    NativeAssociativity::ExternalReferenceIndex {
                        id,
                        source_entity,
                        declared_count: record.and_then(|record| record.integer(1)),
                        entries: collect_result_vec(
                            ctx,
                            count,
                            "iges native external index entries",
                            |offset| -> Result<NativeExternalIndexEntry, CodecError> {
                                let start = 2 + offset * 2;
                                Ok(NativeExternalIndexEntry {
                                    symbolic_name: record
                                        .and_then(|record| record.string(start))
                                        .map(|bytes| {
                                            ctx.copy_retained(
                                                bytes,
                                                "iges native external index name",
                                            )
                                        })
                                        .transpose()?,
                                    entity: entity_link(start + 1)?,
                                })
                            },
                        )?,
                    }
                }
                8 => {
                    let fields = record.and_then(|record| {
                        signal_string_layout(record).map(|layout| (record, layout))
                    });
                    let (signal_names, connections, schematic_entities, physical_entities) =
                        match fields {
                            Some((record, layout)) => {
                                let connection_indices = layout.connections();
                                let connections = collect_result_vec(
                                    ctx,
                                    connection_indices.len(),
                                    "iges native signal connection slots",
                                    |offset| {
                                        let index = connection_indices.start + offset;
                                        record
                                            .integer(index)
                                            .map(|sequence| {
                                                parameter_resolver.resolve_type(
                                                    entry.sequence,
                                                    index,
                                                    sequence,
                                                    402,
                                                    &[11],
                                                )
                                            })
                                            .transpose()?
                                            .flatten()
                                            .map(|sequence| {
                                                format_retained(
                                                    ctx,
                                                    format_args!(
                                                        "iges:entity:directory#{sequence}"
                                                    ),
                                                    "iges native signal connection",
                                                )
                                            })
                                            .transpose()
                                    },
                                )?;
                                let geometry_links = |indices: std::ops::Range<usize>| -> Result<
                                    Vec<Option<String>>,
                                    CodecError,
                                > {
                                    collect_result_vec(
                                        ctx,
                                        indices.len(),
                                        "iges native signal geometry slots",
                                        |offset| {
                                            let index = indices.start + offset;
                                            record
                                                .integer(index)
                                                .map(|sequence| {
                                                    parameter_resolver.resolve(
                                                        entry.sequence,
                                                        index,
                                                        sequence,
                                                        ReferenceExpectation::Named(
                                                            ExpectationLabel::SignalStringGeometry,
                                                        ),
                                                        |target| {
                                                            signal_string_geometry_target(
                                                                target.entity_type,
                                                                target.form,
                                                            )
                                                        },
                                                    )
                                                })
                                                .transpose()?
                                                .flatten()
                                                .map(|sequence| {
                                                    format_retained(
                                                        ctx,
                                                        format_args!(
                                                            "iges:entity:directory#{sequence}"
                                                        ),
                                                        "iges native signal geometry",
                                                    )
                                                })
                                                .transpose()
                                        },
                                    )
                                };
                                let name_indices = layout.signal_names();
                                (
                                    collect_result_vec(
                                        ctx,
                                        name_indices.len(),
                                        "iges native signal name slots",
                                        |offset| {
                                            record
                                                .string(name_indices.start + offset)
                                                .map(|bytes| {
                                                    ctx.copy_retained(
                                                        bytes,
                                                        "iges native signal name",
                                                    )
                                                })
                                                .transpose()
                                        },
                                    )?,
                                    connections,
                                    geometry_links(layout.schematic())?,
                                    geometry_links(layout.physical())?,
                                )
                            }
                            None => (Vec::new(), Vec::new(), Vec::new(), Vec::new()),
                        };
                    NativeAssociativity::LegacySignalString {
                        id,
                        source_entity,
                        declared_signal_name_count: record.and_then(|record| record.integer(1)),
                        declared_connection_count: record.and_then(|record| record.integer(2)),
                        declared_schematic_count: record.and_then(|record| record.integer(3)),
                        declared_physical_count: record.and_then(|record| record.integer(4)),
                        signal_names,
                        connections,
                        schematic_entities,
                        physical_entities,
                    }
                }
                10 => {
                    let layout = record.and_then(text_node_layout);
                    let description_start = layout.as_ref().map(TextNodeLayout::description_start);
                    let font_characteristic = description_start.and_then(|index| {
                        record.and_then(|record| record.integer_or(index + 2, 1))
                    });
                    let font_definition = description_start
                        .zip(font_characteristic)
                        .filter(|(_, value)| *value < 0)
                        .map(|(index, value)| {
                            parameter_resolver.resolve_negative(
                                entry.sequence,
                                index + 2,
                                value,
                                ReferenceExpectation::Named(
                                    ExpectationLabel::Type310Form0FontDefinition,
                                ),
                                |target| target.entity_type == 310 && target.form == 0,
                            )
                        })
                        .transpose()?
                        .flatten()
                        .map(|sequence| {
                            format_retained(
                                ctx,
                                format_args!("iges:entity:directory#{sequence}"),
                                "iges native text font definition",
                            )
                        })
                        .transpose()?;
                    let geometry_indices = layout
                        .as_ref()
                        .map(TextNodeLayout::geometry)
                        .unwrap_or(0..0);
                    NativeAssociativity::LegacyTextNode {
                        id,
                        source_entity,
                        declared_geometry_count: record.and_then(|record| record.integer(1)),
                        declared_text_description_count: record
                            .and_then(|record| record.integer(2)),
                        geometry: collect_result_vec(
                            ctx,
                            geometry_indices.len(),
                            "iges native text node geometry slots",
                            |offset| {
                                let index = geometry_indices.start + offset;
                                record
                                    .and_then(|record| record.integer(index))
                                    .map(|sequence| {
                                        parameter_resolver.resolve_type(
                                            entry.sequence,
                                            index,
                                            sequence,
                                            116,
                                            &[0],
                                        )
                                    })
                                    .transpose()?
                                    .flatten()
                                    .map(|sequence| {
                                        format_retained(
                                            ctx,
                                            format_args!("iges:entity:directory#{sequence}"),
                                            "iges native text node geometry",
                                        )
                                    })
                                    .transpose()
                            },
                        )?,
                        box_width: description_start.and_then(|index| {
                            record.and_then(|record| record.number_or(index, 0.0))
                        }),
                        box_height: description_start.and_then(|index| {
                            record.and_then(|record| record.number_or(index + 1, 0.0))
                        }),
                        font_characteristic,
                        font_definition,
                        slant_angle: description_start.and_then(|index| {
                            record.and_then(|record| {
                                record.number_or(index + 3, std::f64::consts::FRAC_PI_2)
                            })
                        }),
                        rotation_angle: description_start.and_then(|index| {
                            record.and_then(|record| record.number_or(index + 4, 0.0))
                        }),
                        mirror_flag: description_start.and_then(|index| {
                            record.and_then(|record| record.integer_or(index + 5, 0))
                        }),
                        rotate_internal_flag: description_start.and_then(|index| {
                            record.and_then(|record| record.integer_or(index + 6, 0))
                        }),
                    }
                }
                11 => {
                    let fields = record.and_then(|record| {
                        connect_node_layout(record).map(|layout| (record, layout))
                    });
                    let (points, data) = match fields {
                        Some((record, layout)) => {
                            let point_indices = layout.points();
                            let points = collect_result_vec(
                                ctx,
                                point_indices.len(),
                                "iges native connect node point slots",
                                |offset| {
                                    let index = point_indices.start + offset;
                                    record
                                        .integer(index)
                                        .map(|sequence| {
                                            parameter_resolver.resolve_type(
                                                entry.sequence,
                                                index,
                                                sequence,
                                                116,
                                                &[0],
                                            )
                                        })
                                        .transpose()?
                                        .flatten()
                                        .map(|sequence| {
                                            format_retained(
                                                ctx,
                                                format_args!("iges:entity:directory#{sequence}"),
                                                "iges native connect node point",
                                            )
                                        })
                                        .transpose()
                                },
                            )?;
                            let data_indices = layout.data();
                            let data = collect_result_vec(
                                ctx,
                                data_indices.len(),
                                "iges native connect node data slots",
                                |offset| {
                                    record
                                        .token(data_indices.start + offset)
                                        .map_or(Ok(TokenValue::Omitted), |item| {
                                            copy_native_token_value(ctx, &item.value)
                                        })
                                },
                            )?;
                            (points, data)
                        }
                        None => (Vec::new(), Vec::new()),
                    };
                    NativeAssociativity::LegacyConnectNode {
                        id,
                        source_entity,
                        declared_point_count: record.and_then(|record| record.integer(1)),
                        declared_data_count: record.and_then(|record| record.integer(2)),
                        points,
                        data,
                    }
                }
                13 => {
                    let end =
                        record.map_or(0, |record| clamped_primary_end(entry.sequence, record));
                    let count =
                        overdeclared_counts.counted_tail_at(entry.sequence, record, end, 2, 4, 1);
                    NativeAssociativity::DimensionedGeometry {
                        id,
                        source_entity,
                        declared_geometry_count: record.and_then(|record| record.integer(2)),
                        dimension: record
                            .and_then(|record| record.integer(3))
                            .map(|sequence| {
                                parameter_resolver.resolve(
                                    entry.sequence,
                                    3,
                                    sequence,
                                    ReferenceExpectation::Named(ExpectationLabel::DimensionEntity),
                                    |target| {
                                        matches!(
                                            target.entity_type,
                                            202 | 206 | 216 | 218 | 220 | 222
                                        )
                                    },
                                )
                            })
                            .transpose()?
                            .flatten()
                            .map(|sequence| {
                                format_retained(
                                    ctx,
                                    format_args!("iges:entity:directory#{sequence}"),
                                    "iges native dimension geometry dimension",
                                )
                            })
                            .transpose()?,
                        geometry: collect_result_vec(
                            ctx,
                            count,
                            "iges native dimension geometry slots",
                            |offset| entity_link(4 + offset),
                        )?,
                    }
                }
                16 => {
                    let end =
                        record.map_or(0, |record| clamped_primary_end(entry.sequence, record));
                    let count =
                        overdeclared_counts.counted_tail_at(entry.sequence, record, end, 2, 4, 1);
                    NativeAssociativity::Planar {
                        id,
                        source_entity,
                        declared_entity_count: record.and_then(|record| record.integer(2)),
                        plane_transform: record
                            .and_then(|record| record.integer(3))
                            .filter(|sequence| *sequence != 0)
                            .map(|sequence| {
                                parameter_resolver.resolve_type(
                                    entry.sequence,
                                    3,
                                    sequence,
                                    124,
                                    &[0],
                                )
                            })
                            .transpose()?
                            .flatten()
                            .map(|sequence| {
                                format_retained(
                                    ctx,
                                    format_args!("iges:native:transformation#D{sequence}"),
                                    "iges native planar transformation",
                                )
                            })
                            .transpose()?,
                        entities: collect_result_vec(
                            ctx,
                            count,
                            "iges native planar entity slots",
                            |offset| entity_link(4 + offset),
                        )?,
                    }
                }
                18 | 20 => {
                    let end =
                        record.map_or(0, |record| clamped_primary_end(entry.sequence, record));
                    let first_list_index = if entry.form == 18 { 10 } else { 9 };
                    let count_options: [Option<usize>; 6] = std::array::from_fn(|offset| {
                        let index = offset + 2;
                        record.and_then(|record| record.count_with_stride_before(index, 1, end))
                    });
                    let complete = record.is_some()
                        && count_options.iter().all(Option::is_some)
                        && count_options
                            .iter()
                            .flatten()
                            .try_fold(0_usize, |total, count| total.checked_add(*count))
                            .is_some_and(|total| total <= end.saturating_sub(first_list_index));
                    let counts = if complete {
                        count_options.map(Option::unwrap_or_default)
                    } else {
                        [0; 6]
                    };
                    let flow_links = |start, count| -> Result<Vec<Option<String>>, CodecError> {
                        collect_result_vec(ctx, count, "iges native flow link slots", |offset| {
                            let index = start + offset;
                            record
                                .and_then(|record| record.integer(index))
                                .map(|sequence| {
                                    parameter_resolver.resolve(
                                        entry.sequence,
                                        index,
                                        sequence,
                                        ReferenceExpectation::Named(
                                            ExpectationLabel::MatchingFlowAssociativity,
                                        ),
                                        |target| {
                                            target.entity_type == 402 && target.form == entry.form
                                        },
                                    )
                                })
                                .transpose()?
                                .flatten()
                                .map(|sequence| {
                                    format_retained(
                                        ctx,
                                        format_args!("iges:entity:directory#{sequence}"),
                                        "iges native flow link",
                                    )
                                })
                                .transpose()
                        })
                    };
                    let mut cursor = first_list_index;
                    let associated_flows = flow_links(cursor, counts[0])?;
                    cursor += counts[0];
                    let connections = collect_result_vec(
                        ctx,
                        counts[1],
                        "iges native flow connection slots",
                        |offset| {
                            let index = cursor + offset;
                            record
                                .and_then(|record| record.integer(index))
                                .map(|sequence| {
                                    parameter_resolver.resolve(
                                        entry.sequence,
                                        index,
                                        sequence,
                                        if entry.form == 18 {
                                            ReferenceExpectation::Named(
                                                ExpectationLabel::Type132OrGroup,
                                            )
                                        } else {
                                            ReferenceExpectation::Type {
                                                entity_type: 132,
                                                forms: vec![],
                                            }
                                        },
                                        |target| {
                                            target.entity_type == 132
                                                || (entry.form == 18
                                                    && target.entity_type == 402
                                                    && matches!(target.form, 1 | 7 | 14 | 15))
                                        },
                                    )
                                })
                                .transpose()?
                                .flatten()
                                .map(|sequence| {
                                    format_retained(
                                        ctx,
                                        format_args!("iges:entity:directory#{sequence}"),
                                        "iges native flow connection",
                                    )
                                })
                                .transpose()
                        },
                    )?;
                    cursor += counts[1];
                    let joins = collect_result_vec(
                        ctx,
                        counts[2],
                        "iges native flow join slots",
                        |offset| {
                            let index = cursor + offset;
                            record
                                .and_then(|record| record.integer(index))
                                .map(|sequence| {
                                    parameter_resolver.resolve(
                                        entry.sequence,
                                        index,
                                        sequence,
                                        ReferenceExpectation::Named(
                                            ExpectationLabel::NonAssociativityOrType402Form7,
                                        ),
                                        |target| {
                                            flow_join_target_valid(target, global.global_table())
                                        },
                                    )
                                })
                                .transpose()?
                                .flatten()
                                .map(|sequence| {
                                    format_retained(
                                        ctx,
                                        format_args!("iges:entity:directory#{sequence}"),
                                        "iges native flow join",
                                    )
                                })
                                .transpose()
                        },
                    )?;
                    cursor += counts[2];
                    let names = collect_result_vec(
                        ctx,
                        counts[3],
                        "iges native flow name slots",
                        |offset| {
                            record
                                .and_then(|record| record.string(cursor + offset))
                                .map(|bytes| ctx.copy_retained(bytes, "iges native flow name"))
                                .transpose()
                        },
                    )?;
                    cursor += counts[3];
                    let name_displays = collect_result_vec(
                        ctx,
                        counts[4],
                        "iges native flow name display slots",
                        |offset| {
                            let index = cursor + offset;
                            record
                                .and_then(|record| record.integer(index))
                                .map(|sequence| {
                                    parameter_resolver.resolve(
                                        entry.sequence,
                                        index,
                                        sequence,
                                        if entry.form == 18 {
                                            ReferenceExpectation::AnyOf {
                                                first: 312,
                                                second: 212,
                                                rest: vec![],
                                            }
                                        } else {
                                            ReferenceExpectation::Type {
                                                entity_type: 312,
                                                forms: vec![],
                                            }
                                        },
                                        |target| {
                                            target.entity_type == 312
                                                || (entry.form == 18 && target.entity_type == 212)
                                        },
                                    )
                                })
                                .transpose()?
                                .flatten()
                                .map(|sequence| {
                                    format_retained(
                                        ctx,
                                        format_args!("iges:entity:directory#{sequence}"),
                                        "iges native flow name display",
                                    )
                                })
                                .transpose()
                        },
                    )?;
                    cursor += counts[4];
                    let continuations = collect_result_vec(
                        ctx,
                        counts[5],
                        "iges native flow continuation slots",
                        |offset| {
                            let index = cursor + offset;
                            record
                                .and_then(|record| record.integer(index))
                                .filter(|sequence| *sequence != 0)
                                .map(|sequence| {
                                    if entry.form == 18 {
                                        parameter_resolver.resolve_type(
                                            entry.sequence,
                                            index,
                                            sequence,
                                            402,
                                            &[11, 18],
                                        )
                                    } else {
                                        parameter_resolver.resolve_type(
                                            entry.sequence,
                                            index,
                                            sequence,
                                            402,
                                            &[20],
                                        )
                                    }
                                })
                                .transpose()?
                                .flatten()
                                .map(|sequence| {
                                    format_retained(
                                        ctx,
                                        format_args!("iges:entity:directory#{sequence}"),
                                        "iges native flow continuation",
                                    )
                                })
                                .transpose()
                        },
                    )?;
                    NativeAssociativity::Flow {
                        id,
                        source_entity,
                        form: entry.form,
                        declared_associated_flow_count: record.and_then(|record| record.integer(2)),
                        declared_connection_count: record.and_then(|record| record.integer(3)),
                        declared_join_count: record.and_then(|record| record.integer(4)),
                        declared_name_count: record.and_then(|record| record.integer(5)),
                        declared_name_display_count: record.and_then(|record| record.integer(6)),
                        declared_continuation_count: record.and_then(|record| record.integer(7)),
                        type_flag: record.and_then(|record| record.integer(8)),
                        function_flag: (entry.form == 18)
                            .then(|| record.and_then(|record| record.integer(9)))
                            .flatten(),
                        associated_flows,
                        connections,
                        joins,
                        names,
                        name_displays,
                        continuations,
                    }
                }
                21 => {
                    let end =
                        record.map_or(0, |record| clamped_primary_end(entry.sequence, record));
                    let count =
                        overdeclared_counts.counted_tail_at(entry.sequence, record, end, 2, 6, 5);
                    NativeAssociativity::RecalculableDimension {
                        id,
                        source_entity,
                        declared_geometry_count: record.and_then(|record| record.integer(2)),
                        dimension: record
                            .and_then(|record| record.integer(3))
                            .map(|sequence| {
                                parameter_resolver.resolve(
                                    entry.sequence,
                                    3,
                                    sequence,
                                    ReferenceExpectation::Named(ExpectationLabel::DimensionEntity),
                                    |target| {
                                        matches!(
                                            target.entity_type,
                                            202 | 206 | 216 | 218 | 220 | 222
                                        )
                                    },
                                )
                            })
                            .transpose()?
                            .flatten()
                            .map(|sequence| {
                                format_retained(
                                    ctx,
                                    format_args!("iges:entity:directory#{sequence}"),
                                    "iges native recalculable dimension",
                                )
                            })
                            .transpose()?,
                        orientation_flag: record.and_then(|record| record.integer(4)),
                        angle: record.and_then(|record| record.number(5)),
                        geometry: collect_result_vec(
                            ctx,
                            count,
                            "iges native recalculable geometry slots",
                            |offset| -> Result<NativeDimensionGeometryItem, CodecError> {
                                let start = 6 + offset * 5;
                                Ok(NativeDimensionGeometryItem {
                                    geometry: entity_link(start)?,
                                    location_flag: record
                                        .and_then(|record| record.integer(start + 1)),
                                    point: [
                                        record.and_then(|record| record.number(start + 2)),
                                        record.and_then(|record| record.number(start + 3)),
                                        record.and_then(|record| record.number(start + 4)),
                                    ],
                                })
                            },
                        )?,
                    }
                }
                _ => return Ok(()),
            };
            associativities.push(association);
            Ok(())
        })?;
    let attribute_table_definitions = collect_native_items(
        ctx,
        directory
            .iter()
            .filter(|entry| entry.entity_type == 322 && matches!(entry.form, 0..=2)),
        "iges native attribute definition slots",
        |entry| {
            let record = by_directory.get(&entry.sequence).copied();
            let end = record.map_or(0, |record| clamped_primary_end(entry.sequence, record));
            let count = if entry.form == 0 {
                Some(overdeclared_counts.counted_tail(entry.sequence, record, end, 3, 3))
            } else {
                record.and_then(|record| record.count_with_stride_before(3, 1, end))
            };
            let mut cursor = 4;
            let mut attributes = reserve_vec(
                ctx,
                count.unwrap_or_default(),
                "iges native attribute definition attributes",
            )?;
            let mut complete = true;
            if let Some(count) = count {
                for _ in 0..count {
                    let Some(record) = record else {
                        complete = false;
                        break;
                    };
                    let attribute_type = record.integer(cursor);
                    let value_data_type = record.integer(cursor + 1);
                    let declared_value_count = record.integer(cursor + 2);
                    let stride = if entry.form == 2 { 2 } else { 1 };
                    let value_count = if entry.form == 0 {
                        Some(0)
                    } else {
                        match record.value(cursor + 2) {
                            Some(TokenValue::Omitted) => {
                                (stride <= end.saturating_sub(cursor + 3)).then_some(1)
                            }
                            Some(TokenValue::Integer(_)) => {
                                record.count_with_stride_before(cursor + 2, stride, end)
                            }
                            None | Some(TokenValue::Real(_) | TokenValue::String(_)) => None,
                        }
                    };
                    let Some(value_start) = cursor.checked_add(3) else {
                        complete = false;
                        break;
                    };
                    let Some(value_count) = value_count else {
                        complete = false;
                        break;
                    };
                    let Some(next) = value_count
                        .checked_mul(stride)
                        .and_then(|width| value_start.checked_add(width))
                        .filter(|next| *next <= end)
                    else {
                        complete = false;
                        break;
                    };
                    let mut values =
                        reserve_vec(ctx, value_count, "iges native attribute definition values")?;
                    if entry.form != 0 {
                        for offset in 0..value_count {
                            let value_index = value_start + offset * stride;
                            let value = record
                                .tokens()
                                .get(value_index)
                                .map_or(Ok(TokenValue::Omitted), |token| {
                                    copy_native_token_value(ctx, &token.value)
                                })?;
                            let display_template = (entry.form == 2)
                                .then(|| record.integer(value_index + 1))
                                .flatten()
                                .filter(|sequence| *sequence != 0)
                                .map(|sequence| {
                                    parameter_resolver.resolve_type(
                                        entry.sequence,
                                        value_index + 1,
                                        sequence,
                                        312,
                                        &[0, 1],
                                    )
                                })
                                .transpose()?
                                .flatten()
                                .map(|sequence| {
                                    format_retained(
                                        ctx,
                                        format_args!("iges:entity:directory#{sequence}"),
                                        "iges native attribute display template",
                                    )
                                })
                                .transpose()?;
                            values.push(NativeAttributeValue {
                                value,
                                display_template,
                            });
                        }
                    }
                    attributes.push(NativeAttributeDefinition {
                        attribute_type,
                        value_data_type,
                        declared_value_count,
                        values,
                    });
                    cursor = next;
                }
            }
            if !complete {
                attributes.clear();
            }
            Ok(NativeAttributeTableDefinition {
                id: format_retained(
                    ctx,
                    format_args!("iges:product:attribute-definition#D{}", entry.sequence),
                    "iges native attribute definition id",
                )?,
                source_entity: format_retained(
                    ctx,
                    format_args!("iges:entity:directory#{}", entry.sequence),
                    "iges native attribute definition source",
                )?,
                form: entry.form,
                name: copy_native_string(
                    ctx,
                    record.and_then(|record| record.string(1)),
                    "iges native attribute definition name",
                )?,
                attribute_list_type: record.and_then(|record| record.integer(2)),
                declared_attribute_count: record.and_then(|record| record.integer(3)),
                attributes,
            })
        },
    )?;
    let attribute_table_instances = collect_native_items(
        ctx,
        directory
            .iter()
            .filter(|entry| entry.entity_type == 422 && matches!(entry.form, 0..=1)),
        "iges native attribute instance slots",
        |entry| {
            let record = by_directory.get(&entry.sequence).copied();
            let definition_sequence =
                crate::graph::resolved_structure_sequence(references, entry.sequence);
            let definition_record =
                definition_sequence.and_then(|sequence| by_directory.get(&sequence).copied());
            let definition = definition_sequence
                .and_then(|sequence| Some((sequence, *entries.get(&sequence)?)))
                .zip(definition_record)
                .map(|((sequence, definition_entry), definition_record)| {
                    let stride = if definition_entry.form == 0 { 3 } else { 1 };
                    (
                        definition_record,
                        stride,
                        clamped_primary_end(sequence, definition_record),
                    )
                });
            let value_start = if entry.form == 0 { 1 } else { 2 };
            let grid = match record {
                Some(record) => attribute_table_rows(
                    entry.form,
                    record,
                    clamped_primary_end(entry.sequence, record),
                    definition,
                    value_start,
                ),
                // The instance has no Parameter Data record at all, which its
                // own quarantine loss already names.
                None => Ok(None),
            };
            let rows = match grid {
                Ok(Some(grid)) => {
                    let row_width = grid.values_per_row.get();
                    collect_result_vec(
                        ctx,
                        grid.values.len() / row_width,
                        "iges native attribute instance row slots",
                        |index| {
                            let row = &grid.values[index * row_width..(index + 1) * row_width];
                            collect_result_vec(
                                ctx,
                                row.len(),
                                "iges native attribute instance value slots",
                                |offset| copy_native_token_value(ctx, &row[offset].value),
                            )
                        },
                    )?
                }
                Ok(None) => Vec::new(),
                Err(refusal) => {
                    insert_optional_btree_map(
                        Some(ctx),
                        &mut unstatable_attribute_tables,
                        entry.sequence,
                        refusal,
                        "iges unstatable attribute table nodes",
                    )?;
                    Vec::new()
                }
            };
            Ok(NativeAttributeTableInstance {
                id: format_retained(
                    ctx,
                    format_args!("iges:product:attribute-instance#D{}", entry.sequence),
                    "iges native attribute instance id",
                )?,
                source_entity: format_retained(
                    ctx,
                    format_args!("iges:entity:directory#{}", entry.sequence),
                    "iges native attribute instance source",
                )?,
                form: entry.form,
                definition: definition_sequence
                    .map(|sequence| {
                        format_retained(
                            ctx,
                            format_args!("iges:product:attribute-definition#D{sequence}"),
                            "iges native attribute instance definition",
                        )
                    })
                    .transpose()?,
                declared_row_count: (entry.form == 1)
                    .then(|| record.and_then(|record| record.integer(1)))
                    .flatten(),
                rows,
            })
        },
    )?;
    let product_properties = collect_native_items(
        ctx,
        directory
            .iter()
            .filter(|entry| entry.entity_type == 406 && matches!(entry.form, 7 | 15)),
        "iges native product property slots",
        |entry| {
            let record = by_directory.get(&entry.sequence).copied();
            Ok(NativeProductProperty {
                id: format_retained(
                    ctx,
                    format_args!("iges:product:property#D{}", entry.sequence),
                    "iges native product property id",
                )?,
                source_entity: format_retained(
                    ctx,
                    format_args!("iges:entity:directory#{}", entry.sequence),
                    "iges native product property source",
                )?,
                property_kind: if entry.form == 7 {
                    ProductPropertyKind::ReferenceDesignator
                } else {
                    ProductPropertyKind::Name
                },
                value: copy_native_string(
                    ctx,
                    record.and_then(|record| record.string(2)),
                    "iges native product property value",
                )?,
                owners: native_entity_ids(
                    ctx,
                    by_directory
                        .iter()
                        .filter(|(sequence, _owner_record)| {
                            **sequence != entry.sequence
                                && trailing_pointer_analysis
                                    .get(sequence)
                                    .and_then(|analysis| match analysis {
                                        TrailingPointerAnalysis::Unambiguous(groups) => {
                                            Some(groups)
                                        }
                                        _ => None,
                                    })
                                    .is_some_and(|groups| {
                                        groups.properties().contains(&entry.sequence)
                                    })
                        })
                        .map(|(sequence, _)| *sequence),
                    "iges native product property owner slots",
                )?,
            })
        },
    )?;
    let mut properties = Vec::new();
    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 406 && matches!(entry.form, 2..=15 | 18..=36))
    {
        let Some(record) = by_directory.get(&entry.sequence).copied() else {
            continue;
        };
        reserve_vec_growth(ctx, &mut properties, 1, "iges native property slots")?;
        let end = clamped_primary_end(entry.sequence, record);
        let mut counted = |index, stride| {
            overdeclared_counts.counted_tail(entry.sequence, Some(record), end, index, stride)
        };
        let string =
            |index| copy_native_string(ctx, record.string(index), "iges native property string");
        let strings = |start: usize, count: usize| -> Result<Vec<Option<Vec<u8>>>, CodecError> {
            collect_result_vec(ctx, count, "iges native property string slots", |offset| {
                string(start + offset)
            })
        };
        let value = match entry.form {
            2 => NativePropertyValue::RegionRestriction {
                electrical_vias: record.integer(2),
                electrical_components: record.integer(3),
                electrical_circuitry: record.integer(4),
            },
            3 => NativePropertyValue::LevelFunction {
                function_code: record.integer(2),
                description: string(3)?,
            },
            4 => NativePropertyValue::RegionFill {
                fill_code: record.integer(2),
                obsolete_pointer: record.integer(3),
            },
            5 => NativePropertyValue::LineWidening {
                width: record.number(2),
                cornering: record.integer(3),
                extension_flag: record.integer(4),
                justification: record.integer(5),
                extension: record.number(6),
            },
            6 => NativePropertyValue::DrilledHole {
                drill_diameter: record.number(2),
                finished_diameter: record.number(3),
                plated: record.integer(4),
                lower_layer: record.integer(5),
                upper_layer: record.integer(6),
            },
            7 => NativePropertyValue::ReferenceDesignator { value: string(2)? },
            8 => NativePropertyValue::PinNumber { value: string(2)? },
            9 => NativePropertyValue::PartNumber {
                generic: string(2)?,
                military: string(3)?,
                vendor: string(4)?,
                internal: string(5)?,
            },
            10 => NativePropertyValue::Hierarchy {
                line_font: record.integer(2),
                view: record.integer(3),
                level: record.integer(4),
                blank: record.integer(5),
                line_weight: record.integer(6),
                color: record.integer(7),
            },
            11 => {
                let dependent_count = record
                    .integer(3)
                    .and_then(|value| usize::try_from(value).ok());
                let independent_count = record
                    .integer(4)
                    .and_then(|value| usize::try_from(value).ok());
                let (independent_variables, dependent_values) =
                    match (dependent_count, independent_count) {
                        (Some(dependent_count), Some(independent_count)) if dependent_count > 0 => {
                            let header_end = independent_count
                                .checked_mul(2)
                                .and_then(|width| 5_usize.checked_add(width))
                                .filter(|header_end| *header_end <= end);
                            match header_end {
                                Some(header_end) => {
                                    let count_start = 5 + independent_count;
                                    let mut cursor = header_end;
                                    let mut point_count = 1_usize;
                                    let mut independent_variables = reserve_vec(
                                        ctx,
                                        independent_count,
                                        "iges native tabular independent variables",
                                    )?;
                                    let mut valid = true;
                                    for offset in 0..independent_count {
                                        let declared_value_count =
                                            record.integer(count_start + offset);
                                        let Some(value_count) = declared_value_count
                                            .and_then(|value| usize::try_from(value).ok())
                                            .filter(|value_count| *value_count > 0)
                                        else {
                                            valid = false;
                                            break;
                                        };
                                        let Some(next) = cursor
                                            .checked_add(value_count)
                                            .filter(|next| *next <= end)
                                        else {
                                            valid = false;
                                            break;
                                        };
                                        point_count = match point_count.checked_mul(value_count) {
                                            Some(point_count) => point_count,
                                            None => {
                                                valid = false;
                                                break;
                                            }
                                        };
                                        independent_variables.push(NativeIndependentVariable {
                                            variable_type: record.integer(5 + offset),
                                            declared_value_count,
                                            values: collect_result_vec(
                                                ctx,
                                                value_count,
                                                "iges native tabular independent values",
                                                |index| Ok(record.number(cursor + index)),
                                            )?,
                                        });
                                        cursor = next;
                                    }
                                    let dependent_value_count = valid
                                        .then(|| dependent_count.checked_mul(point_count))
                                        .flatten()
                                        .filter(|count| *count <= end.saturating_sub(cursor));
                                    match dependent_value_count {
                                        Some(count) => (
                                            independent_variables,
                                            collect_result_vec(
                                                ctx,
                                                count,
                                                "iges native tabular dependent values",
                                                |offset| Ok(record.number(cursor + offset)),
                                            )?,
                                        ),
                                        None => (Vec::new(), Vec::new()),
                                    }
                                }
                                None => (Vec::new(), Vec::new()),
                            }
                        }
                        _ => (Vec::new(), Vec::new()),
                    };
                NativePropertyValue::TabularData {
                    property_type: record.integer(2),
                    declared_dependent_count: record.integer(3),
                    independent_variables,
                    dependent_values,
                }
            }
            12 => NativePropertyValue::ExternalReferenceFileList {
                names: strings(2, counted(1, 1))?,
            },
            13 => NativePropertyValue::NominalSize {
                size: record.number(2),
                name: string(3)?,
                standard: string(4)?,
            },
            14 => NativePropertyValue::FlowLineSpecification {
                values: strings(2, counted(1, 1))?,
            },
            15 => NativePropertyValue::Name { value: string(2)? },
            18 => NativePropertyValue::IntercharacterSpacing {
                percent: record.number(2),
            },
            19 => NativePropertyValue::LineFont {
                pattern_code: record.integer(2),
            },
            20 => NativePropertyValue::Highlight {
                highlighted: binary_integer(record.integer(2)),
            },
            21 => NativePropertyValue::Pick {
                pickable: binary_integer(record.integer(2)).map(|value| !value),
            },
            22 => NativePropertyValue::UniformRectangularGrid {
                finite: binary_integer(record.integer(2)),
                lines: binary_integer(record.integer(3)),
                weighted: binary_integer(record.integer(4)).map(|value| !value),
                origin: [record.number(5), record.number(6)],
                spacing: [record.number(7), record.number(8)],
                counts: [record.integer(9), record.integer(10)],
            },
            23 => NativePropertyValue::AssociativityGroupType {
                associativity_type: record.integer(2),
                name: string(3)?,
            },
            24 => {
                let definition_count = counted(2, 4);
                NativePropertyValue::LevelToLepLayerMap {
                    definitions: collect_result_vec(
                        ctx,
                        definition_count,
                        "iges native layer definition slots",
                        |offset| {
                            let start = 3 + offset * 4;
                            Ok(NativeLepLayerDefinition {
                                exchange_level: record.integer(start),
                                native_identifier: string(start + 1)?,
                                physical_layer: record.integer(start + 2),
                                functional_identifier: string(start + 3)?,
                            })
                        },
                    )?,
                }
            }
            25 => {
                let level_count = counted(3, 1);
                NativePropertyValue::LepArtworkStackup {
                    identification: string(2)?,
                    levels: collect_result_vec(
                        ctx,
                        level_count,
                        "iges native artwork level slots",
                        |offset| Ok(record.integer(4 + offset)),
                    )?,
                }
            }
            26 => NativePropertyValue::LepDrilledHole {
                drill_diameter: record.number(2),
                finished_diameter: record.number(3),
                function_code: record.integer(4),
            },
            27 => {
                let value_count = counted(3, 2);
                NativePropertyValue::GenericData {
                    name: string(2)?,
                    values: collect_result_vec(
                        ctx,
                        value_count,
                        "iges native generic property value slots",
                        |offset| {
                            let index = 4 + offset * 2;
                            Ok(NativeGenericPropertyValue {
                                data_type: record.integer(index),
                                value: record
                                    .tokens()
                                    .get(index + 1)
                                    .map_or(Ok(TokenValue::Omitted), |token| {
                                        copy_native_token_value(ctx, &token.value)
                                    })?,
                            })
                        },
                    )?,
                }
            }
            28 => NativePropertyValue::DimensionUnits {
                secondary_position: record.integer(2),
                units_indicator: record.integer(3),
                character_set: record.integer_or(4, DEFAULT_DIMENSION_UNITS_CHARACTER_SET),
                suffix: string(5)?,
                fraction_flag: record.integer(6),
                precision: record.integer(7),
            },
            29 => NativePropertyValue::DimensionTolerance {
                secondary_flag: record.integer(2),
                tolerance_type: record.integer(3),
                placement: record.integer_or(4, DEFAULT_DIMENSION_TOLERANCE_PLACEMENT),
                upper: record.number(5),
                lower: record.number(6),
                suppress_plus: binary_integer(record.integer(7)),
                fraction_flag: record.integer(8),
                precision: record.integer(9),
            },
            30 => {
                let note_count = counted(13, 3);
                NativePropertyValue::DimensionDisplayData {
                    dimension_type: record.integer(2),
                    label_position: record.integer(3),
                    declared_character_set: record.integer(4),
                    character_set: record.integer_or(4, DEFAULT_DIMENSION_DISPLAY_CHARACTER_SET),
                    label: string(5)?,
                    decimal_symbol: record.integer(6),
                    declared_witness_line_angle: record.number(7),
                    witness_line_angle: record
                        .number_or(7, DEFAULT_DIMENSION_DISPLAY_WITNESS_LINE_ANGLE_RAD),
                    text_alignment: record.integer(8),
                    text_level: record.integer(9),
                    text_placement: record.integer(10),
                    arrow_orientation: record.integer(11),
                    initial_value: record.number(12),
                    supplemental_notes: collect_result_vec(
                        ctx,
                        note_count,
                        "iges native supplemental note slots",
                        |offset| {
                            let start = 14 + offset * 3;
                            Ok(NativeSupplementalNote {
                                position: record.integer(start),
                                first_text: record.integer(start + 1),
                                last_text: record.integer(start + 2),
                            })
                        },
                    )?,
                }
            }
            31 => NativePropertyValue::BasicDimension {
                corners: collect_result_vec(
                    ctx,
                    4,
                    "iges native basic dimension corners",
                    |offset| Ok([record.number(2 + offset * 2), record.number(3 + offset * 2)]),
                )?,
            },
            32 => NativePropertyValue::DrawingSheetApproval {
                name: string(2)?,
                organization: string(3)?,
                date: string(4)?,
            },
            33 => NativePropertyValue::DrawingSheetId {
                sheet_number: record.integer(2),
                revision: string(3)?,
            },
            34 | 35 => {
                let range_count = counted(2, 3);
                let ranges = collect_result_vec(
                    ctx,
                    range_count,
                    "iges native text score range slots",
                    |offset| {
                        let start = 3 + offset * 3;
                        Ok(NativeTextScoreRange {
                            text_index: record.integer(start),
                            first_character: record.integer(start + 1),
                            last_character: record.integer(start + 2),
                        })
                    },
                )?;
                if entry.form == 34 {
                    NativePropertyValue::Underscore { ranges }
                } else {
                    NativePropertyValue::Overscore { ranges }
                }
            }
            36 => NativePropertyValue::Closure {
                u: record.integer(2),
                v: record.integer(3),
            },
            _ => continue,
        };
        properties.push(NativeProperty {
            id: format_retained(
                ctx,
                format_args!("iges:application:property#D{}", entry.sequence),
                "iges native property id",
            )?,
            source_entity: format_retained(
                ctx,
                format_args!("iges:entity:directory#{}", entry.sequence),
                "iges native property source",
            )?,
            form: entry.form,
            declared_value_count: record.integer(1),
            owners: native_entity_ids(
                ctx,
                by_directory
                    .iter()
                    .filter(|(sequence, _owner)| {
                        **sequence != entry.sequence
                            && trailing_pointer_analysis
                                .get(sequence)
                                .and_then(|analysis| match analysis {
                                    TrailingPointerAnalysis::Unambiguous(groups) => Some(groups),
                                    _ => None,
                                })
                                .is_some_and(|groups| groups.properties().contains(&entry.sequence))
                    })
                    .map(|(sequence, _)| *sequence),
                "iges native property owner slots",
            )?,
            value,
        });
    }
    let units_data = collect_native_items(
        ctx,
        directory
            .iter()
            .filter(|entry| entry.entity_type == 316 && entry.form == 0),
        "iges native units data slots",
        |entry| {
            let record = by_directory.get(&entry.sequence).copied();
            let end = record.map_or(0, |record| clamped_primary_end(entry.sequence, record));
            let count = overdeclared_counts.counted_tail(entry.sequence, record, end, 1, 3);
            let owners = native_entity_ids(
                ctx,
                by_directory
                    .iter()
                    .filter(|(sequence, _owner)| {
                        trailing_pointer_analysis
                            .get(sequence)
                            .and_then(|analysis| match analysis {
                                TrailingPointerAnalysis::Unambiguous(groups) => Some(groups),
                                _ => None,
                            })
                            .is_some_and(|groups| groups.properties().contains(&entry.sequence))
                    })
                    .map(|(sequence, _)| *sequence),
                "iges native units owner slots",
            )?;
            Ok(NativeUnitsData {
                id: format_retained(
                    ctx,
                    format_args!("iges:metadata:units-data#D{}", entry.sequence),
                    "iges native units data id",
                )?,
                source_entity: format_retained(
                    ctx,
                    format_args!("iges:entity:directory#{}", entry.sequence),
                    "iges native units data source",
                )?,
                declared_count: record.and_then(|record| record.integer(1)),
                units: collect_result_vec(
                    ctx,
                    count,
                    "iges native unit definition slots",
                    |offset| {
                        let start = 2 + offset * 3;
                        Ok(NativeUnitDefinition {
                            unit_type: copy_native_string(
                                ctx,
                                record.and_then(|record| record.string(start)),
                                "iges native unit type",
                            )?,
                            unit_value: copy_native_string(
                                ctx,
                                record.and_then(|record| record.string(start + 1)),
                                "iges native unit value",
                            )?,
                            scale_factor: record.and_then(|record| record.number(start + 2)),
                        })
                    },
                )?,
                owners,
            })
        },
    )?;
    let views = collect_native_items(
        ctx,
        directory
            .iter()
            .filter(|entry| entry.entity_type == 410 && matches!(entry.form, 0 | 1)),
        "iges native view slots",
        |entry| {
            let record = by_directory.get(&entry.sequence).copied();
            let vector = |start| {
                [
                    record.and_then(|record| record.number(start)),
                    record.and_then(|record| record.number(start + 1)),
                    record.and_then(|record| record.number(start + 2)),
                ]
            };
            Ok(NativeView {
                id: format_retained(
                    ctx,
                    format_args!("iges:presentation:view#D{}", entry.sequence),
                    "iges native view id",
                )?,
                source_entity: format_retained(
                    ctx,
                    format_args!("iges:entity:directory#{}", entry.sequence),
                    "iges native view source",
                )?,
                view_number: record.and_then(|record| record.integer(1)),
                scale: record.and_then(|record| record.number(2)),
                geometry: if entry.form == 0 {
                    ViewGeometry::Orthographic {
                        model_to_view: (entry.transform > 0)
                            .then(|| {
                                format_retained(
                                    ctx,
                                    format_args!("iges:native:transformation#D{}", entry.transform),
                                    "iges native view transformation",
                                )
                            })
                            .transpose()?,
                        clipping_planes: collect_result_vec(
                            ctx,
                            6,
                            "iges native view clipping plane slots",
                            |offset| {
                                let index = offset + 3;
                                record
                                    .and_then(|record| record.integer(index))
                                    .filter(|sequence| *sequence != 0)
                                    .map(|sequence| {
                                        parameter_resolver.resolve_type(
                                            entry.sequence,
                                            index,
                                            sequence,
                                            108,
                                            &[],
                                        )
                                    })
                                    .transpose()?
                                    .flatten()
                                    .map(|sequence| {
                                        format_retained(
                                            ctx,
                                            format_args!("iges:entity:directory#{sequence}"),
                                            "iges native view clipping plane",
                                        )
                                    })
                                    .transpose()
                            },
                        )?,
                    }
                } else {
                    ctx.charge_collection_items(1, "iges native perspective view geometry")?;
                    ViewGeometry::Perspective(Box::new(PerspectiveViewGeometry {
                        view_plane_normal: vector(3),
                        view_reference_point: vector(6),
                        center_of_projection: vector(9),
                        view_up: vector(12),
                        view_plane_distance: record.and_then(|record| record.number(15)),
                        clipping_window: std::array::from_fn(|index| {
                            record.and_then(|record| record.number(16 + index))
                        }),
                        depth_clipping: record.and_then(|record| record.integer(20)),
                        depth_range: std::array::from_fn(|index| {
                            record.and_then(|record| record.number(21 + index))
                        }),
                    }))
                },
            })
        },
    )?;
    let view_visibility = collect_native_items(
        ctx,
        directory
            .iter()
            .filter(|entry| entry.entity_type == 402 && matches!(entry.form, 3 | 4)),
        "iges native view visibility slots",
        |entry| {
            let record = by_directory.get(&entry.sequence).copied();
            let width = if entry.form == 3 { 1 } else { 5 };
            let end = record.map_or(0, |record| clamped_primary_end(entry.sequence, record));
            let view_count = record
                .and_then(|record| record.count_with_stride_before(1, width, end))
                .and_then(|view_count| {
                    let entity_count = record.and_then(|record| {
                        crate::parameter::view_visibility_entity_count(
                            record,
                            global.global_table(),
                        )
                    })?;
                    let entity_start = 3_usize.checked_add(view_count.checked_mul(width)?)?;
                    let finish = entity_start.checked_add(entity_count)?;
                    (finish <= end).then_some((view_count, entity_count))
                })
                .unwrap_or_default();
            let (view_count, entity_count) = view_count;
            Ok(NativeViewVisibility {
                id: format_retained(
                    ctx,
                    format_args!("iges:presentation:view-visibility#D{}", entry.sequence),
                    "iges native view visibility id",
                )?,
                source_entity: format_retained(
                    ctx,
                    format_args!("iges:entity:directory#{}", entry.sequence),
                    "iges native view visibility source",
                )?,
                form: entry.form,
                // Both counts sit at fixed Parameter indices 1 and 2 for every
                // form, so retention is unconditional; only the entity list's
                // start moves with the view count.
                declared_view_count: record.and_then(|record| record.integer(1)),
                displays: collect_result_vec(
                    ctx,
                    view_count,
                    "iges native view display slots",
                    |index| {
                        let start = 3 + index * width;
                        if let Some(color) = (entry.form == 4)
                            .then(|| record.and_then(|record| record.integer(start + 3)))
                            .flatten()
                            .filter(|value| *value < 0)
                        {
                            let _color_reference = parameter_resolver.resolve_negative_type(
                                entry.sequence,
                                start + 3,
                                color,
                                314,
                                &[0],
                            )?;
                        }
                        Ok(NativeViewDisplay {
                            view: record
                                .and_then(|record| record.integer(start))
                                .map(|sequence| {
                                    parameter_resolver.resolve_type(
                                        entry.sequence,
                                        start,
                                        sequence,
                                        410,
                                        &[0, 1],
                                    )
                                })
                                .transpose()?
                                .flatten()
                                .map(|sequence| {
                                    format_retained(
                                        ctx,
                                        format_args!("iges:presentation:view#D{sequence}"),
                                        "iges native view display view",
                                    )
                                })
                                .transpose()?,
                            style: if entry.form == 4 {
                                ViewDisplayStyle::Overrides {
                                    line_font: record.and_then(|record| record.integer(start + 1)),
                                    line_font_definition: record
                                        .and_then(|record| record.integer(start + 2))
                                        .filter(|sequence| *sequence != 0)
                                        .map(|sequence| {
                                            parameter_resolver.resolve_type(
                                                entry.sequence,
                                                start + 2,
                                                sequence,
                                                304,
                                                &[1, 2],
                                            )
                                        })
                                        .transpose()?
                                        .flatten()
                                        .map(|sequence| {
                                            format_retained(
                                                ctx,
                                                format_args!(
                                                    "iges:presentation:line-font#D{sequence}"
                                                ),
                                                "iges native view display line font",
                                            )
                                        })
                                        .transpose()?,
                                    color: record.and_then(|record| record.integer(start + 3)),
                                    line_weight: record
                                        .and_then(|record| record.integer(start + 4)),
                                }
                            } else {
                                ViewDisplayStyle::Inherited
                            },
                        })
                    },
                )?,
                declared_entity_count: record.and_then(|record| record.integer(2)),
                entities: collect_result_vec(
                    ctx,
                    entity_count,
                    "iges native visible entity slots",
                    |index| {
                        record
                            .and_then(|record| record.integer(3 + view_count * width + index))
                            .map(|sequence| {
                                parameter_resolver.resolve_any(
                                    entry.sequence,
                                    3 + view_count * width + index,
                                    sequence,
                                )
                            })
                            .transpose()?
                            .flatten()
                            .map(|sequence| {
                                format_retained(
                                    ctx,
                                    format_args!("iges:entity:directory#{sequence}"),
                                    "iges native visible entity",
                                )
                            })
                            .transpose()
                    },
                )?,
            })
        },
    )?;
    let segmented_visibility = collect_native_items(
        ctx,
        directory
            .iter()
            .filter(|entry| entry.entity_type == 402 && entry.form == 19),
        "iges native segmented visibility slots",
        |entry| {
            let record = by_directory.get(&entry.sequence).copied();
            let end = record.map_or(0, |record| clamped_primary_end(entry.sequence, record));
            let count = overdeclared_counts.counted_tail(entry.sequence, record, end, 1, 6);
            let value = |index| -> Result<TokenValue, CodecError> {
                record
                    .and_then(|record| record.token(index))
                    .map_or(Ok(TokenValue::Omitted), |token| {
                        copy_native_token_value(ctx, &token.value)
                    })
            };
            Ok(NativeSegmentedVisibility {
                id: format_retained(
                    ctx,
                    format_args!("iges:presentation:segmented-visibility#D{}", entry.sequence),
                    "iges native segmented visibility id",
                )?,
                source_entity: format_retained(
                    ctx,
                    format_args!("iges:entity:directory#{}", entry.sequence),
                    "iges native segmented visibility source",
                )?,
                declared_block_count: record.and_then(|record| record.integer(1)),
                blocks: collect_result_vec(
                    ctx,
                    count,
                    "iges native segment display slots",
                    |index| {
                        let start = 2 + index * 6;
                        if let Some(color) = record
                            .and_then(|record| record.integer(start + 3))
                            .filter(|value| *value < 0)
                        {
                            let _color_reference = parameter_resolver.resolve_negative_type(
                                entry.sequence,
                                start + 3,
                                color,
                                314,
                                &[0],
                            )?;
                        }
                        if let Some(line_font) = record
                            .and_then(|record| record.integer(start + 4))
                            .filter(|value| *value < 0)
                        {
                            let _line_font_reference = parameter_resolver.resolve_negative_type(
                                entry.sequence,
                                start + 4,
                                line_font,
                                304,
                                &[1, 2],
                            )?;
                        }
                        Ok(NativeSegmentDisplay {
                            view: record
                                .and_then(|record| record.integer(start))
                                .map(|sequence| {
                                    parameter_resolver.resolve_type(
                                        entry.sequence,
                                        start,
                                        sequence,
                                        410,
                                        &[0, 1],
                                    )
                                })
                                .transpose()?
                                .flatten()
                                .map(|sequence| {
                                    format_retained(
                                        ctx,
                                        format_args!("iges:presentation:view#D{sequence}"),
                                        "iges native segment display view",
                                    )
                                })
                                .transpose()?,
                            breakpoint: record.and_then(|record| record.number(start + 1)),
                            display_flag: record.and_then(|record| record.integer(start + 2)),
                            color: value(start + 3)?,
                            line_font: value(start + 4)?,
                            line_weight: value(start + 5)?,
                        })
                    },
                )?,
            })
        },
    )?;
    let drawings = collect_native_items(
        ctx,
        directory
            .iter()
            .filter(|entry| entry.entity_type == 404 && matches!(entry.form, 0 | 1)),
        "iges native drawing slots",
        |entry| {
            let record = by_directory.get(&entry.sequence).copied();
            let width = if entry.form == 0 { 3 } else { 4 };
            let end = record.map_or(0, |record| clamped_primary_end(entry.sequence, record));
            let counts = record
                .and_then(|record| record.count_with_stride_before(1, width, end))
                .and_then(|view_count| {
                    let annotation_count_index =
                        2_usize.checked_add(view_count.checked_mul(width)?)?;
                    let annotation_count = record.and_then(|record| {
                        record.count_with_stride_before(annotation_count_index, 1, end)
                    })?;
                    let finish = annotation_count_index
                        .checked_add(1)?
                        .checked_add(annotation_count)?;
                    (finish <= end).then_some((view_count, annotation_count))
                })
                .unwrap_or_default();
            let (view_count, annotation_count) = counts;
            let annotation_count_index = 2 + view_count * width;
            let declared_view_count = record.and_then(|record| record.integer(1));
            // The annotation count's slot is fixed by the DECLARED view count
            // under the Type 404 table, not the admitted `view_count`: on the
            // refusal path `view_count` is 0 and index 2 holds the first view
            // pointer, so deriving from the admitted count would retain a view
            // pointer as a count. When the chain succeeds the two coincide.
            let declared_annotation_count = declared_view_count
                .and_then(|count| usize::try_from(count).ok())
                .and_then(|count| count.checked_mul(width))
                .and_then(|span| span.checked_add(2))
                .zip(record)
                .and_then(|(index, record)| record.integer(index));
            let trailing = trailing_pointer_analysis
                .get(&entry.sequence)
                .and_then(|analysis| match analysis {
                    TrailingPointerAnalysis::Unambiguous(groups) => Some(groups),
                    _ => None,
                });
            let (name_property, name_ambiguous) =
                choose_drawing_property(trailing, 15, &entries, &by_directory);
            let (size_property, size_ambiguous) =
                choose_drawing_property(trailing, 16, &entries, &by_directory);
            let (units_property, units_ambiguous) =
                choose_drawing_property(trailing, 17, &entries, &by_directory);
            let ambiguous_property_forms = collect_native_items(
                ctx,
                [
                    (15, name_ambiguous),
                    (16, size_ambiguous),
                    (17, units_ambiguous),
                ]
                .into_iter()
                .filter_map(|(form, ambiguous)| ambiguous.then_some(form)),
                "iges native drawing ambiguous property slots",
                Ok,
            )?;
            Ok(NativeDrawing {
                id: format_retained(
                    ctx,
                    format_args!("iges:presentation:drawing#D{}", entry.sequence),
                    "iges native drawing id",
                )?,
                source_entity: format_retained(
                    ctx,
                    format_args!("iges:entity:directory#{}", entry.sequence),
                    "iges native drawing source",
                )?,
                form: entry.form,
                declared_view_count,
                views: collect_result_vec(
                    ctx,
                    view_count,
                    "iges native drawing view slots",
                    |index| {
                        let start = 2 + index * width;
                        Ok(NativeDrawingView {
                            view: record
                                .and_then(|record| record.integer(start))
                                .map(|sequence| {
                                    parameter_resolver.resolve_type(
                                        entry.sequence,
                                        start,
                                        sequence,
                                        410,
                                        &[0, 1],
                                    )
                                })
                                .transpose()?
                                .flatten()
                                .map(|sequence| {
                                    format_retained(
                                        ctx,
                                        format_args!("iges:presentation:view#D{sequence}"),
                                        "iges native drawing view",
                                    )
                                })
                                .transpose()?,
                            origin: [
                                record.and_then(|record| record.number(start + 1)),
                                record.and_then(|record| record.number(start + 2)),
                            ],
                            rotation: (entry.form == 1)
                                .then(|| record.and_then(|record| record.number(start + 3)))
                                .flatten(),
                        })
                    },
                )?,
                declared_annotation_count,
                annotations: collect_result_vec(
                    ctx,
                    annotation_count,
                    "iges native drawing annotation slots",
                    |index| {
                        record
                            .and_then(|record| record.integer(annotation_count_index + 1 + index))
                            .map(|sequence| {
                                parameter_resolver.resolve(
                                    entry.sequence,
                                    annotation_count_index + 1 + index,
                                    sequence,
                                    ReferenceExpectation::Named(
                                        ExpectationLabel::DrawingSpaceAnnotation,
                                    ),
                                    |target| {
                                        target.status.use_flag(global.global_table())
                                            == Some(UseFlag::Annotation)
                                            && target.status.is_physically_dependent()
                                    },
                                )
                            })
                            .transpose()?
                            .flatten()
                            .map(|sequence| {
                                format_retained(
                                    ctx,
                                    format_args!("iges:entity:directory#{sequence}"),
                                    "iges native drawing annotation",
                                )
                            })
                            .transpose()
                    },
                )?,
                name_property: name_property
                    .map(|sequence| {
                        format_retained(
                            ctx,
                            format_args!("iges:product:property#D{sequence}"),
                            "iges native drawing name property",
                        )
                    })
                    .transpose()?,
                name: copy_native_string(
                    ctx,
                    name_property
                        .and_then(|sequence| by_directory.get(&sequence))
                        .and_then(|record| record.string(2)),
                    "iges native drawing name",
                )?,
                size: size_property.and_then(|sequence| {
                    let record = by_directory.get(&sequence)?;
                    Some([record.number(2), record.number(3)])
                }),
                units_flag: units_property
                    .and_then(|sequence| by_directory.get(&sequence))
                    .and_then(|record| record.integer(2)),
                units_name: copy_native_string(
                    ctx,
                    units_property
                        .and_then(|sequence| by_directory.get(&sequence))
                        .and_then(|record| record.string(3)),
                    "iges native drawing units name",
                )?,
                ambiguous_property_forms,
            })
        },
    )?;
    let annotations = annotations::build(
        directory,
        (&by_directory, &entries),
        &parameter_resolver,
        &clamped_primary_end,
        &mut overdeclared_counts,
        global.global_table(),
        ctx,
    )?;
    let fem_entities = fem::build(directory, &by_directory, &parameter_resolver, ctx)?;
    // Scan every definition for root-inference diagnostics, then restrict the
    // map consumed by expansion to definitions admitted by structure.
    let occurrence_length_factor = global
        .length_context()
        .map(|context| context.length_factor_mm());
    let mut malformed_definition_sequences = Vec::new();
    let mut all_occurrence_definitions = BTreeMap::new();
    for entry in directory
        .iter()
        .filter(|entry| matches!(entry.entity_type, 308 | 320) && entry.form == 0)
    {
        let Some(record) = by_directory.get(&entry.sequence).copied() else {
            reserve_vec_growth(
                ctx,
                &mut malformed_definition_sequences,
                1,
                "iges malformed occurrence definitions",
            )?;
            malformed_definition_sequences.push(entry.sequence);
            continue;
        };
        let Some(count) =
            record.count_with_stride_before(3, 1, clamped_primary_end(entry.sequence, record))
        else {
            reserve_vec_growth(
                ctx,
                &mut malformed_definition_sequences,
                1,
                "iges malformed occurrence definitions",
            )?;
            malformed_definition_sequences.push(entry.sequence);
            if !all_occurrence_definitions.contains_key(&entry.sequence) {
                ctx.charge_collection_items(1, "iges occurrence definition map")?;
            }
            all_occurrence_definitions.insert(
                entry.sequence,
                OccurrenceDefinition {
                    members: Vec::new(),
                    transform: Transform::identity(),
                },
            );
            continue;
        };
        let mut malformed = false;
        let mut members = Vec::new();
        for index in 0..count {
            let member = record
                .integer(4 + index)
                .and_then(|value| u32::try_from(value).ok())
                .filter(|sequence| sequence % 2 == 1 && entries.contains_key(sequence));
            match member {
                Some(member) => {
                    reserve_vec_growth(ctx, &mut members, 1, "iges occurrence definition members")?;
                    members.push(member);
                }
                None => malformed = true,
            }
        }
        let transform = if let Some(length_factor) = occurrence_length_factor {
            match resolve_transform(
                entry.transform,
                &entries,
                &by_directory,
                length_factor,
                global.real_precision(),
                &mut BTreeSet::new(),
                Some(ctx),
            ) {
                Ok(transform) => transform,
                Err(error) => {
                    error.non_resource()?;
                    malformed = true;
                    Transform::identity()
                }
            }
        } else {
            Transform::identity()
        };
        if malformed {
            reserve_vec_growth(
                ctx,
                &mut malformed_definition_sequences,
                1,
                "iges malformed occurrence definitions",
            )?;
            malformed_definition_sequences.push(entry.sequence);
        }
        if !all_occurrence_definitions.contains_key(&entry.sequence) {
            ctx.charge_collection_items(1, "iges occurrence definition map")?;
        }
        all_occurrence_definitions
            .insert(entry.sequence, OccurrenceDefinition { members, transform });
    }
    // Keep parseable member lists as containment evidence even when semantic
    // structure admission rejects their definitions. A rejected definition is
    // not traversed below, but one of its admitted child instances must not be
    // promoted to a root. Container-only decode passes None and retains every
    // parseable structure record for expansion.
    let mut contained_instances = BTreeSet::new();
    for sequence in all_occurrence_definitions
        .values()
        .flat_map(|definition| definition.members.iter().copied())
        .filter(|sequence| {
            entries
                .get(sequence)
                .is_some_and(|entry| matches!(entry.entity_type, 408 | 420))
        })
    {
        insert_optional_btree_set(
            Some(ctx),
            &mut contained_instances,
            sequence,
            "iges contained occurrence instances",
        )?;
    }
    let mut occurrence_definitions = BTreeMap::new();
    for (sequence, definition) in all_occurrence_definitions
        .into_iter()
        .filter(|(sequence, _)| {
            structure_admitted.is_none_or(|admitted| admitted.decoded.contains(sequence))
        })
    {
        insert_optional_btree_map(
            Some(ctx),
            &mut occurrence_definitions,
            sequence,
            definition,
            "iges admitted occurrence definition nodes",
        )?;
    }
    let mut occurrence_neutral_links = BTreeMap::<u32, Vec<String>>::new();
    for curve in &ir.model.curves {
        if let Some(sequence) = curve
            .source_object
            .as_ref()
            .filter(|source| source.format == cadmpeg_ir::CodecFormat::Iges)
            .and_then(|_| sequences.curve(&curve.id))
        {
            push_occurrence_neutral_link(
                ctx,
                &mut occurrence_neutral_links,
                sequence,
                curve.id.as_str(),
            )?;
        }
    }
    for surface in &ir.model.surfaces {
        if let Some(sequence) = surface
            .source_object
            .as_ref()
            .filter(|source| source.format == cadmpeg_ir::CodecFormat::Iges)
            .and_then(|_| sequences.surface(&surface.id))
        {
            push_occurrence_neutral_link(
                ctx,
                &mut occurrence_neutral_links,
                sequence,
                surface.id.as_str(),
            )?;
        }
    }
    for body in &ir.model.bodies {
        if let Some(sequence) = sequences.body_neutral_form(&body.id) {
            push_occurrence_neutral_link(
                ctx,
                &mut occurrence_neutral_links,
                sequence,
                body.id.as_str(),
            )?;
        }
    }
    for point in &ir.model.points {
        if let Some(sequence) = sequences.point(&point.id) {
            push_occurrence_neutral_link(
                ctx,
                &mut occurrence_neutral_links,
                sequence,
                point.id.as_str(),
            )?;
        }
    }
    let mut product_occurrences = Vec::new();
    let mut output_truncated_at = None;
    let mut depth_truncated_at = None;
    let mut malformed_placement_sequences = std::collections::BTreeSet::new();
    if let Some(length_factor) = occurrence_length_factor {
        if let Some(admission) = structure_admitted {
            for sequence in admission.placement_rejections.iter().filter_map(
                |(sequence, reason)| match reason {
                    PlacementRejection::MissingRecord
                    | PlacementRejection::InvalidDefinition
                    | PlacementRejection::InvalidPlacement => Some(*sequence),
                    PlacementRejection::InvalidMetadata { definition } => {
                        (!occurrence_definitions.contains_key(definition)).then_some(*sequence)
                    }
                },
            ) {
                insert_optional_btree_set(
                    Some(ctx),
                    &mut malformed_placement_sequences,
                    sequence,
                    "iges malformed occurrence placement nodes",
                )?;
            }
        }
        let expansion = OccurrenceExpansion {
            entries: &entries,
            records: &by_directory,
            definitions: &occurrence_definitions,
            neutral_links: &occurrence_neutral_links,
            length_factor,
            precision: global.real_precision(),
            output_limit: limits.output,
            depth_limit: limits.depth,
            ctx,
        };
        if malformed_definition_sequences.is_empty() {
            for root in directory.iter().filter(|entry| {
                matches!(entry.entity_type, 408 | 420)
                    && entry.form == 0
                    && structure_admitted
                        .is_none_or(|admitted| admitted.decoded.contains(&entry.sequence))
                    && !contained_instances.contains(&entry.sequence)
            }) {
                if let Some(source_sequence) = expansion.expand(
                    root.sequence,
                    Transform::identity(),
                    &mut Vec::new(),
                    &mut product_occurrences,
                    &mut depth_truncated_at,
                    &mut malformed_placement_sequences,
                )? {
                    output_truncated_at = Some(source_sequence);
                    break;
                }
            }
        }
    }
    let issues = collect_native_items(
        ctx,
        [
            output_truncated_at
                .is_some()
                .then_some(ProductOccurrenceIssue::OutputLimit),
            depth_truncated_at
                .is_some()
                .then_some(ProductOccurrenceIssue::DepthLimit),
            (!malformed_definition_sequences.is_empty())
                .then_some(ProductOccurrenceIssue::MalformedDefinition),
            (!malformed_placement_sequences.is_empty())
                .then_some(ProductOccurrenceIssue::MalformedPlacement),
        ]
        .into_iter()
        .flatten(),
        "iges occurrence issue slots",
        Ok,
    )?;
    let product_occurrence_expansion = [NativeProductOccurrenceExpansion {
        id: format_retained(
            ctx,
            format_args!("iges:product:occurrence-expansion#state"),
            "iges occurrence expansion state id",
        )?,
        output_limit: limits.output,
        depth_limit: limits.depth,
        emitted: product_occurrences.len(),
        issues,
    }];
    let boundary_vertex_sewing = collect_native_items(
        ctx,
        boundary_vertex_derivations.iter(),
        "iges boundary vertex sewing slots",
        |derivation| {
            Ok(NativeBoundaryVertex {
                id: format_retained(
                    ctx,
                    format_args!(
                        "iges:topology:boundary-vertex#{}",
                        ColonsAsUnderscores(
                            derivation
                                .vertex
                                .as_str()
                                .strip_prefix("iges:model:vertex#")
                                .unwrap_or(derivation.vertex.as_str())
                        )
                    ),
                    "iges boundary vertex sewing id",
                )?,
                source_entity: format_retained(
                    ctx,
                    format_args!("{}", derivation.source_entity),
                    "iges boundary vertex sewing source",
                )?,
                vertex: format_retained(
                    ctx,
                    format_args!("{}", derivation.vertex.as_str()),
                    "iges boundary vertex sewing vertex",
                )?,
                representative: [
                    derivation.representative.x,
                    derivation.representative.y,
                    derivation.representative.z,
                ],
                tolerance: derivation.tolerance,
                sewn: derivation
                    .source_endpoints
                    .iter()
                    .any(|endpoint| endpoint.position != derivation.representative),
                source_endpoints: collect_native_items(
                    ctx,
                    derivation.source_endpoints.iter(),
                    "iges boundary vertex endpoint slots",
                    |endpoint| {
                        Ok(NativeBoundaryVertexEndpoint {
                            edge: format_retained(
                                ctx,
                                format_args!("{}", endpoint.edge),
                                "iges boundary vertex endpoint edge",
                            )?,
                            endpoint: endpoint.endpoint,
                            position: [
                                endpoint.position.x,
                                endpoint.position.y,
                                endpoint.position.z,
                            ],
                        })
                    },
                )?,
            })
        },
    )?;
    parameter_resolver.append_to(references)?;
    for entity in &mut entities {
        entity.links = native_entity_ids(
            ctx,
            references
                .get(&entity.directory_sequence)
                .into_iter()
                .flatten()
                .filter_map(ReferenceEdge::target_sequence),
            "iges resolved native reference link slots",
        )?;
        entity.references = match references.get(&entity.directory_sequence) {
            Some(edges) => {
                let mut copies =
                    reserve_vec(ctx, edges.len(), "iges resolved native reference slots")?;
                for edge in edges {
                    copies.push(edge.copy_for_native(ctx)?);
                }
                copies
            }
            None => Vec::new(),
        };
    }
    let native_entity_count = [
        directions.len(),
        flashes.len(),
        transforms.len(),
        copious_data.len(),
        colors.len(),
        display_attributes.len(),
        line_fonts.len(),
        text_templates.len(),
        text_fonts.len(),
        definition_levels.len(),
        primitive_solids.len(),
        procedural_solids.len(),
        boolean_trees.len(),
        selected_components.len(),
        solid_assemblies.len(),
        manifold_solids.len(),
        solid_instances.len(),
        subfigure_definitions.len(),
        subfigure_instances.len(),
        network_definitions.len(),
        network_instances.len(),
        connect_points.len(),
        rectangular_arrays.len(),
        circular_arrays.len(),
        external_references.len(),
        groups.len(),
        associativities.len(),
        attribute_table_definitions.len(),
        attribute_table_instances.len(),
        product_properties.len(),
        properties.len(),
        units_data.len(),
        views.len(),
        view_visibility.len(),
        segmented_visibility.len(),
        drawings.len(),
        annotations.len(),
        fem_entities.len(),
        boundary_vertex_sewing.len(),
        product_occurrences.len(),
        product_occurrence_expansion.len(),
        macro_definitions.len(),
        macro_instances.len(),
        quarantined_directory_records.len(),
        quarantined_parameter_records.len(),
    ]
    .into_iter()
    .fold(0_u64, |total, count| total.saturating_add(count as u64));
    ctx.charge_entities(native_entity_count, "iges_native_entities")?;
    let namespace = ir.native.namespace_mut("iges");
    namespace.set_arena_from(ctx, "cards", cards)?;
    namespace.set_arena_from(ctx, "entities", entities)?;
    namespace.set_arena_from(ctx, "directions", directions)?;
    namespace.set_arena_from(ctx, "flashes", flashes)?;
    namespace.set_arena_from(ctx, "transformations", transforms)?;
    namespace.set_arena_from(ctx, "copious_data", copious_data)?;
    namespace.set_arena_from(ctx, "colors", colors)?;
    namespace.set_arena_from(ctx, "display_attributes", display_attributes)?;
    namespace.set_arena_from(ctx, "line_fonts", line_fonts)?;
    namespace.set_arena_from(ctx, "text_templates", text_templates)?;
    namespace.set_arena_from(ctx, "text_fonts", text_fonts)?;
    namespace.set_arena_from(ctx, "definition_levels", definition_levels)?;
    namespace.set_arena_from(ctx, "primitive_solids", primitive_solids)?;
    namespace.set_arena_from(ctx, "procedural_solids", procedural_solids)?;
    namespace.set_arena_from(ctx, "boolean_trees", boolean_trees)?;
    namespace.set_arena_from(ctx, "selected_components", selected_components)?;
    namespace.set_arena_from(ctx, "solid_assemblies", solid_assemblies)?;
    namespace.set_arena_from(ctx, "manifold_solids", manifold_solids)?;
    namespace.set_arena_from(ctx, "solid_instances", solid_instances)?;
    namespace.set_arena_from(ctx, "subfigure_definitions", subfigure_definitions)?;
    namespace.set_arena_from(ctx, "subfigure_instances", subfigure_instances)?;
    namespace.set_arena_from(ctx, "network_definitions", network_definitions)?;
    namespace.set_arena_from(ctx, "network_instances", network_instances)?;
    namespace.set_arena_from(ctx, "connect_points", connect_points)?;
    namespace.set_arena_from(ctx, "rectangular_arrays", rectangular_arrays)?;
    namespace.set_arena_from(ctx, "circular_arrays", circular_arrays)?;
    namespace.set_arena_from(ctx, "external_references", external_references)?;
    namespace.set_arena_from(ctx, "groups", groups)?;
    namespace.set_arena_from(ctx, "associativities", associativities)?;
    namespace.set_arena_from(
        ctx,
        "attribute_table_definitions",
        attribute_table_definitions,
    )?;
    namespace.set_arena_from(ctx, "attribute_table_instances", attribute_table_instances)?;
    namespace.set_arena_from(ctx, "product_properties", product_properties)?;
    namespace.set_arena_from(ctx, "properties", properties)?;
    namespace.set_arena_from(ctx, "units_data", units_data)?;
    namespace.set_arena_from(ctx, "views", views)?;
    namespace.set_arena_from(ctx, "view_visibility", view_visibility)?;
    namespace.set_arena_from(ctx, "segmented_visibility", segmented_visibility)?;
    namespace.set_arena_from(ctx, "drawings", drawings)?;
    namespace.set_arena_from(ctx, "annotations", annotations)?;
    namespace.set_arena_from(ctx, "fem_entities", fem_entities)?;
    if !boundary_vertex_sewing.is_empty() {
        namespace.set_arena_from(ctx, "boundary_vertex_sewing", boundary_vertex_sewing)?;
    }
    namespace.set_arena_from(ctx, "product_occurrences", product_occurrences)?;
    namespace.set_arena_from(
        ctx,
        "product_occurrence_expansion",
        product_occurrence_expansion,
    )?;
    if !macro_definitions.is_empty() {
        namespace.set_arena_from(ctx, "macro_definitions", macro_definitions)?;
    }
    if !macro_instances.is_empty() {
        namespace.set_arena_from(ctx, "macro_instances", macro_instances)?;
    }
    namespace.set_arena_from(
        ctx,
        "quarantined_directory_records",
        quarantined_directory_records,
    )?;
    namespace.set_arena_from(
        ctx,
        "quarantined_parameter_records",
        quarantined_parameter_records,
    )?;
    Ok(NativeStoreResult {
        occurrence_expansion: ProductOccurrenceExpansion {
            output_truncated_at,
            depth_truncated_at,
            malformed_definition_sequences,
            malformed_placement_sequences: {
                let mut sequences = reserve_vec(
                    ctx,
                    malformed_placement_sequences.len(),
                    "iges malformed occurrence placement result slots",
                )?;
                sequences.extend(malformed_placement_sequences);
                sequences
            },
        },
        ambiguous_parameter_boundaries,
        overdeclared_counts: overdeclared_counts.0,
        unstatable_attribute_tables,
    })
}

#[cfg(test)]
mod tests;
