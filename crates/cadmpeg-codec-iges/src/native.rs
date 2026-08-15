// SPDX-License-Identifier: Apache-2.0
//! Versioned `native.iges` physical cards and entity records.

use crate::card::CardScan;
use crate::directory::DirectoryEntry;
use crate::entities::annotation::{parameterized_curve_type, section_boundary_type};
use crate::entities::geometry::{resolve_transform, Affine};
use crate::entities::structure::array_base_type;
use crate::global::{Global, RealPrecision};
use crate::graph::{ParameterResolver, ReferenceEdge};
use crate::parameter::{trailing_pointer_groups, ParameterRecord, Token, TokenValue};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::CadIr;
use serde::Serialize;
use std::collections::BTreeMap;

pub(crate) const MAX_PRODUCT_OCCURRENCES: usize = 100_000;
pub(crate) const MAX_PRODUCT_OCCURRENCE_DEPTH: usize = 64;
const DEFAULT_DIMENSION_DISPLAY_CHARACTER_SET: i64 = 1;
const DEFAULT_DIMENSION_DISPLAY_WITNESS_LINE_ANGLE_RAD: f64 = std::f64::consts::FRAC_PI_2;

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct NativeCard {
    id: String,
    offset: u64,
    payload: Vec<u8>,
    line_ending: Vec<u8>,
    section: Option<String>,
    sequence: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
enum NativeTokenValue {
    Omitted,
    Integer(i64),
    Real(f64),
    String(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeToken {
    start: usize,
    end: usize,
    value: NativeTokenValue,
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
struct NativeColorDefinition {
    id: String,
    source_entity: String,
    red_percent: Option<f64>,
    green_percent: Option<f64>,
    blue_percent: Option<f64>,
    name: Option<Vec<u8>>,
    fallback_color_number: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeDisplayAttributes {
    id: String,
    source_entity: String,
    visible: bool,
    line_font_number: i64,
    line_font_definition: Option<String>,
    level_number: i64,
    level_definition: Option<String>,
    view: i64,
    line_weight_number: i64,
    line_weight_mm: Option<f64>,
    color_number: i64,
    color_definition: Option<String>,
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

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativePrimitiveSolid {
    id: String,
    source_entity: String,
    kind: String,
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
    kind: String,
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

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeExternalReference {
    id: String,
    source_entity: String,
    form: i64,
    reference_kind: String,
    file_identifier: Option<Vec<u8>>,
    symbolic_name: Option<Vec<u8>>,
    library_name: Option<Vec<u8>>,
    resolution_state: String,
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
    value: NativeTokenValue,
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
    rows: Vec<Vec<NativeTokenValue>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeProductProperty {
    id: String,
    source_entity: String,
    form: i64,
    property_kind: String,
    value: Option<Vec<u8>>,
    owners: Vec<String>,
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
    value: NativeTokenValue,
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

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeProductOccurrence {
    id: String,
    source_instance: String,
    definition: String,
    member: Option<String>,
    neutral_links: Vec<String>,
    instance_path: Vec<String>,
    local_transform: [[f64; 4]; 3],
    world_transform: [[f64; 4]; 3],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct NativeProductOccurrenceExpansion {
    id: String,
    output_limit: usize,
    depth_limit: usize,
    emitted: usize,
    truncated: bool,
    issues: Vec<ProductOccurrenceIssue>,
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

pub(crate) struct NativeStoreResult {
    pub(crate) occurrence_expansion: ProductOccurrenceExpansion,
    pub(crate) ambiguous_parameter_sequences: Vec<(u32, usize)>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeView {
    id: String,
    source_entity: String,
    form: i64,
    projection: String,
    view_number: Option<i64>,
    scale: Option<f64>,
    model_to_view: Option<String>,
    clipping_planes: Vec<Option<String>>,
    view_plane_normal: Option<[Option<f64>; 3]>,
    view_reference_point: Option<[Option<f64>; 3]>,
    center_of_projection: Option<[Option<f64>; 3]>,
    view_up: Option<[Option<f64>; 3]>,
    view_plane_distance: Option<f64>,
    clipping_window: Option<[Option<f64>; 4]>,
    depth_clipping: Option<i64>,
    depth_range: Option<[Option<f64>; 2]>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeViewDisplay {
    view: Option<String>,
    line_font: Option<i64>,
    line_font_definition: Option<String>,
    color: Option<i64>,
    line_weight: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeViewVisibility {
    id: String,
    source_entity: String,
    form: i64,
    displays: Vec<NativeViewDisplay>,
    entities: Vec<Option<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeSegmentDisplay {
    view: Option<String>,
    breakpoint: Option<f64>,
    display_flag: Option<i64>,
    color: NativeTokenValue,
    line_font: NativeTokenValue,
    line_weight: NativeTokenValue,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeSegmentedVisibility {
    id: String,
    source_entity: String,
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
    views: Vec<NativeDrawingView>,
    annotations: Vec<Option<String>>,
    name_property: Option<String>,
    name: Option<Vec<u8>>,
    size: Option<[Option<f64>; 2]>,
    units_flag: Option<i64>,
    units_name: Option<Vec<u8>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    ambiguous_property_forms: Vec<i64>,
}

#[derive(Debug, PartialEq)]
enum DrawingPropertyValue {
    Name(Vec<u8>),
    Size([f64; 2]),
    Units(i64, Vec<u8>),
}

fn drawing_property_value(form: i64, record: &ParameterRecord) -> Option<DrawingPropertyValue> {
    match form {
        15 => (record.integer(1) == Some(1))
            .then(|| record.string(2).filter(|value| !value.is_empty()))
            .flatten()
            .map(|value| DrawingPropertyValue::Name(value.to_vec())),
        16 => {
            if record.integer(1) != Some(2) {
                return None;
            }
            let size = [record.number(2)?, record.number(3)?];
            size.iter()
                .all(|value| value.is_finite() && *value > 0.0)
                .then_some(DrawingPropertyValue::Size(size))
        }
        17 => {
            let units = record.integer(2).filter(|value| (1..=11).contains(value))?;
            let name = record.string(3).filter(|value| !value.is_empty())?;
            (record.integer(1) == Some(2))
                .then_some(DrawingPropertyValue::Units(units, name.to_vec()))
        }
        _ => None,
    }
}

fn drawing_property_candidates(
    trailing: Option<&crate::parameter::TrailingPointerGroups>,
    form: i64,
    entries: &BTreeMap<u32, &DirectoryEntry>,
) -> Vec<u32> {
    trailing
        .into_iter()
        .flat_map(|groups| groups.properties.iter().copied())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .filter(|sequence| {
            entries
                .get(sequence)
                .is_some_and(|entry| entry.entity_type == 406 && entry.form == form)
        })
        .collect()
}

fn choose_drawing_property(
    candidates: &[u32],
    form: i64,
    records: &BTreeMap<u32, &ParameterRecord>,
) -> (Option<u32>, bool) {
    let valid = candidates
        .iter()
        .filter_map(|sequence| {
            records
                .get(sequence)
                .and_then(|record| drawing_property_value(form, record))
                .map(|value| (*sequence, value))
        })
        .collect::<Vec<_>>();
    let conflicting = valid.len() > 1 && valid.windows(2).any(|pair| pair[0].1 != pair[1].1);
    let selected = (!conflicting)
        .then(|| valid.first().map(|(sequence, _)| *sequence))
        .flatten();
    (selected, conflicting)
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeTextRun {
    declared_character_count: Option<i64>,
    text: Option<Vec<u8>>,
    box_size: [Option<f64>; 2],
    font_code: Option<i64>,
    font_definition: Option<String>,
    slant_angle: Option<f64>,
    rotation_angle: Option<f64>,
    mirror: Option<i64>,
    vertical: Option<i64>,
    start: [Option<f64>; 3],
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct NativeNewTextRun {
    fixed_or_variable: Option<i64>,
    character_size: [Option<f64>; 2],
    character_spacing: Option<f64>,
    line_spacing: Option<f64>,
    font_style: Option<i64>,
    character_angle: Option<f64>,
    control_codes: Option<Vec<u8>>,
    text: NativeTextRun,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum NativeAnnotation {
    GeneralNote {
        id: String,
        source_entity: String,
        declared_string_count: Option<i64>,
        strings: Vec<NativeTextRun>,
        transformation: Option<String>,
    },
    NewGeneralNote {
        id: String,
        source_entity: String,
        containment_size: [Option<f64>; 2],
        justification: Option<i64>,
        containment_origin: [Option<f64>; 3],
        containment_angle: Option<f64>,
        baseline_origin: [Option<f64>; 3],
        normal_interline_spacing: Option<f64>,
        declared_string_count: Option<i64>,
        strings: Vec<NativeNewTextRun>,
        transformation: Option<String>,
    },
    Leader {
        id: String,
        source_entity: String,
        form: i64,
        declared_segment_count: Option<i64>,
        arrowhead_size: [Option<f64>; 2],
        arrowhead: [Option<f64>; 3],
        segment_tails: Vec<[Option<f64>; 3]>,
        transformation: Option<String>,
    },
    AngularDimension {
        id: String,
        source_entity: String,
        note: Option<String>,
        witnesses: [Option<String>; 2],
        vertex: [Option<f64>; 2],
        radius: Option<f64>,
        leaders: [Option<String>; 2],
        transformation: Option<String>,
    },
    CurveDimension {
        id: String,
        source_entity: String,
        note: Option<String>,
        curves: [Option<String>; 2],
        leaders: [Option<String>; 2],
        witnesses: [Option<String>; 2],
        transformation: Option<String>,
    },
    DiameterDimension {
        id: String,
        source_entity: String,
        note: Option<String>,
        leaders: [Option<String>; 2],
        center: [Option<f64>; 2],
        transformation: Option<String>,
    },
    FlagNote {
        id: String,
        source_entity: String,
        origin: [Option<f64>; 3],
        rotation: Option<f64>,
        note: Option<String>,
        declared_leader_count: Option<i64>,
        leaders: Vec<Option<String>>,
        transformation: Option<String>,
    },
    GeneralLabel {
        id: String,
        source_entity: String,
        note: Option<String>,
        declared_leader_count: Option<i64>,
        leaders: Vec<Option<String>>,
        transformation: Option<String>,
    },
    LinearDimension {
        id: String,
        source_entity: String,
        form: i64,
        note: Option<String>,
        leaders: [Option<String>; 2],
        witnesses: [Option<String>; 2],
        transformation: Option<String>,
    },
    OrdinateDimension {
        id: String,
        source_entity: String,
        form: i64,
        note: Option<String>,
        ordinate: Option<String>,
        supplemental_leader: Option<String>,
        transformation: Option<String>,
    },
    PointDimension {
        id: String,
        source_entity: String,
        note: Option<String>,
        leader: Option<String>,
        enclosure: Option<String>,
        transformation: Option<String>,
    },
    RadiusDimension {
        id: String,
        source_entity: String,
        form: i64,
        note: Option<String>,
        leaders: [Option<String>; 2],
        center: [Option<f64>; 2],
        transformation: Option<String>,
    },
    GeneralSymbol {
        id: String,
        source_entity: String,
        note: Option<String>,
        geometry: Vec<Option<String>>,
        leaders: Vec<Option<String>>,
        transformation: Option<String>,
    },
    SectionedArea {
        id: String,
        source_entity: String,
        boundary: Option<String>,
        fill_pattern: Option<i64>,
        pattern_anchor: [Option<f64>; 3],
        pattern_spacing: Option<f64>,
        pattern_angle: Option<f64>,
        islands: Vec<Option<String>>,
        transformation: Option<String>,
    },
}

#[derive(Clone)]
struct OccurrenceDefinition {
    members: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct NativeEntity {
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
    blank_status: u8,
    subordinate_status: u8,
    use_flag: u8,
    hierarchy_status: u8,
    line_weight: i64,
    color: i64,
    reserved: Vec<Vec<u8>>,
    label: Vec<u8>,
    subscript: i64,
    parameter_line_start: Option<u32>,
    parameter_line_end: Option<u32>,
    parameter_bytes: Vec<u8>,
    parameters: Vec<NativeToken>,
    association_links: Vec<String>,
    property_links: Vec<String>,
    comment: Vec<u8>,
    links: Vec<String>,
    references: Vec<ReferenceEdge>,
}

fn token(token: &Token) -> NativeToken {
    NativeToken {
        start: token.span.start,
        end: token.span.end,
        value: match &token.value {
            TokenValue::Omitted => NativeTokenValue::Omitted,
            TokenValue::Integer(value) => NativeTokenValue::Integer(*value),
            TokenValue::Real(value) => NativeTokenValue::Real(*value),
            TokenValue::String(value) => NativeTokenValue::String(value.clone()),
        },
    }
}

fn binary_integer(value: Option<i64>) -> Option<bool> {
    match value? {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

fn model_id_directory_sequence(id: &str, prefix: &str) -> Option<u32> {
    let suffix = id.strip_prefix(prefix)?;
    let digits = suffix
        .as_bytes()
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    (digits > 0)
        .then(|| suffix[..digits].parse::<u32>().ok())
        .flatten()
}

fn placement_affine(
    instance: &DirectoryEntry,
    record: &ParameterRecord,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
    length_factor: f64,
    precision: RealPrecision,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<(u32, Affine), ()> {
    let definition = u32::try_from(record.integer(1).ok_or(())?).map_err(|_| ())?;
    let translation_component = |index| {
        record
            .number_or(index, 0.0)
            .filter(|value| value.is_finite())
            .ok_or(())
    };
    let scale_component = |index, default| {
        record
            .number_or(index, default)
            .filter(|value| value.is_finite() && *value > 0.0)
            .ok_or(())
    };
    let x_scale = scale_component(5, 1.0)?;
    let scales = if instance.entity_type == 420 {
        [
            x_scale,
            scale_component(6, x_scale)?,
            scale_component(7, x_scale)?,
        ]
    } else {
        [x_scale; 3]
    };
    let translation = Affine {
        rows: [
            [1.0, 0.0, 0.0, translation_component(2)? * length_factor],
            [0.0, 1.0, 0.0, translation_component(3)? * length_factor],
            [0.0, 0.0, 1.0, translation_component(4)? * length_factor],
        ],
    };
    let scale = Affine {
        rows: [
            [scales[0], 0.0, 0.0, 0.0],
            [0.0, scales[1], 0.0, 0.0],
            [0.0, 0.0, scales[2], 0.0],
        ],
    };
    let directory = if instance.transform == 0 {
        Affine::IDENTITY
    } else {
        resolve_transform(
            instance.transform,
            entries,
            records,
            length_factor,
            precision,
            &mut std::collections::BTreeSet::new(),
            ctx,
        )
        .map_err(|_| ())?
    };
    Ok((definition, directory.compose(translation.compose(scale))))
}

fn member_affine(
    entry: &DirectoryEntry,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
    length_factor: f64,
    precision: RealPrecision,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<Affine, ()> {
    if entry.transform == 0 {
        return Ok(Affine::IDENTITY);
    }
    resolve_transform(
        entry.transform,
        entries,
        records,
        length_factor,
        precision,
        &mut std::collections::BTreeSet::new(),
        ctx,
    )
    .map_err(|_| ())
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
    ctx: Option<&'a DecodeContext<'ctx>>,
}

impl OccurrenceExpansion<'_, '_> {
    fn expand(
        &self,
        instance_sequence: u32,
        parent: Affine,
        path: &mut Vec<u32>,
        occurrences: &mut Vec<NativeProductOccurrence>,
        depth_truncated_at: &mut Option<u32>,
        malformed_placement_sequences: &mut std::collections::BTreeSet<u32>,
    ) -> Result<Option<u32>, CodecError> {
        let _depth = self
            .ctx
            .map(|ctx| ctx.enter_nested("iges_product_occurrence", None))
            .transpose()?;
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
            return Ok(None);
        };
        let Ok((definition_sequence, local)) = placement_affine(
            instance,
            record,
            self.entries,
            self.records,
            self.length_factor,
            self.precision,
            self.ctx,
        ) else {
            malformed_placement_sequences.insert(instance_sequence);
            return Ok(None);
        };
        let Some(definition) = self.definitions.get(&definition_sequence) else {
            return Ok(None);
        };
        let world = parent.compose(local);
        path.push(instance_sequence);
        let path_ids = path
            .iter()
            .map(|sequence| format!("iges:entity:directory#{sequence}"))
            .collect::<Vec<_>>();
        let path_key = path
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join("/");
        if let Some(ctx) = self.ctx {
            ctx.charge_collection_items(1, "iges_product_occurrences")?;
        }
        occurrences.push(NativeProductOccurrence {
            id: format!("iges:product:occurrence#{path_key}"),
            source_instance: format!("iges:entity:directory#{instance_sequence}"),
            definition: format!("iges:entity:directory#{definition_sequence}"),
            member: None,
            neutral_links: Vec::new(),
            instance_path: path_ids.clone(),
            local_transform: local.rows,
            world_transform: world.rows,
        });
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
                    world,
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
                continue;
            };
            let Ok(member_local) = member_affine(
                member_entry,
                self.entries,
                self.records,
                self.length_factor,
                self.precision,
                self.ctx,
            ) else {
                malformed_placement_sequences.insert(*member);
                continue;
            };
            if let Some(ctx) = self.ctx {
                ctx.charge_collection_items(1, "iges_product_occurrences")?;
            }
            occurrences.push(NativeProductOccurrence {
                id: format!("iges:product:occurrence#{path_key}/D{member}"),
                source_instance: format!("iges:entity:directory#{instance_sequence}"),
                definition: format!("iges:entity:directory#{definition_sequence}"),
                member: Some(format!("iges:entity:directory#{member}")),
                neutral_links: self.neutral_links.get(member).cloned().unwrap_or_default(),
                instance_path: path_ids.clone(),
                local_transform: member_local.rows,
                world_transform: world.compose(member_local).rows,
            });
        }
        path.pop();
        Ok(None)
    }
}

fn charge_native_entities(ctx: Option<&DecodeContext<'_>>, count: u64) -> Result<(), CodecError> {
    ctx.map_or(Ok(()), |ctx| {
        ctx.charge_entities(count, "iges_native_entities")
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn store(
    ir: &mut CadIr,
    scan: &CardScan,
    directory: &[DirectoryEntry],
    parameters: &[ParameterRecord],
    references: &mut BTreeMap<u32, Vec<ReferenceEdge>>,
    global: &Global,
    limits: ProductOccurrenceLimits,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<NativeStoreResult, CodecError> {
    charge_native_entities(ctx, scan.lines.len() as u64)?;
    let cards = scan
        .lines
        .iter()
        .enumerate()
        .map(|(index, line)| NativeCard {
            id: format!("iges:physical:card#{}", index + 1),
            offset: line.offset,
            payload: line.payload.clone(),
            line_ending: line.line_ending().to_vec(),
            section: line
                .section
                .map(|section| format!("{section:?}").to_lowercase()),
            sequence: line.sequence,
        })
        .collect::<Vec<_>>();
    let by_directory = parameters
        .iter()
        .map(|record| (record.directory_sequence, record))
        .collect::<BTreeMap<_, _>>();
    let entries = directory
        .iter()
        .map(|entry| (entry.sequence, entry))
        .collect::<BTreeMap<_, _>>();
    let trailing_by_directory = by_directory
        .iter()
        .map(|(sequence, record)| (*sequence, trailing_pointer_groups(record, &entries)))
        .collect::<BTreeMap<_, _>>();
    let parameter_resolver = ParameterResolver::new(directory);
    let mut required_back_pointer_members = std::collections::BTreeSet::new();
    for group in directory
        .iter()
        .filter(|entry| entry.entity_type == 402 && matches!(entry.form, 1 | 14))
    {
        let record = by_directory.get(&group.sequence).copied();
        let count = record
            .and_then(|record| record.count(1))
            .unwrap_or_default();
        for index in 0..count {
            if let Some(sequence) = record
                .and_then(|record| record.integer(2 + index))
                .and_then(|value| u32::try_from(value).ok())
                .filter(|sequence| sequence % 2 == 1 && entries.contains_key(sequence))
            {
                required_back_pointer_members.insert(sequence);
            }
        }
    }
    let ambiguous_parameter_sequences = required_back_pointer_members
        .iter()
        .filter_map(|sequence| {
            let record = by_directory.get(sequence).copied()?;
            let analysis = crate::parameter::analyze_trailing_pointer_groups(record, &entries);
            (analysis.candidate_count > 1 && analysis.groups.is_none())
                .then_some((*sequence, analysis.candidate_count))
        })
        .collect::<Vec<_>>();
    charge_native_entities(ctx, directory.len() as u64)?;
    let mut entities = directory
        .iter()
        .map(|entry| {
            let parameters = by_directory.get(&entry.sequence).copied();
            let trailing = trailing_by_directory
                .get(&entry.sequence)
                .and_then(|groups| groups.as_ref());
            let invalid_trailing = (trailing.is_none()
                && required_back_pointer_members.contains(&entry.sequence))
            .then(|| {
                parameters
                    .and_then(|record| {
                        crate::parameter::analyze_trailing_pointer_groups(record, &entries).groups
                    })
                    .filter(|groups| !groups.fully_valid)
            })
            .flatten();
            let edge_trailing = trailing.or(invalid_trailing.as_ref());
            let resolved_associations = edge_trailing
                .as_ref()
                .into_iter()
                .flat_map(|groups| groups.association_pointers.iter())
                .filter_map(|pointer| {
                    parameter_resolver.resolve(
                        entry.sequence,
                        pointer.token_index,
                        pointer.raw_pointer,
                        "type-212-or-type-312-or-type-402",
                        |target| matches!(target.entity_type, 212 | 312 | 402),
                    )
                })
                .map(|sequence| format!("iges:entity:directory#{sequence}"))
                .collect::<Vec<_>>();
            let resolved_properties = edge_trailing
                .as_ref()
                .into_iter()
                .flat_map(|groups| groups.property_pointers.iter())
                .filter_map(|pointer| {
                    parameter_resolver.resolve(
                        entry.sequence,
                        pointer.token_index,
                        pointer.raw_pointer,
                        "type-316-or-type-322-or-type-406-or-type-422",
                        |target| matches!(target.entity_type, 316 | 322 | 406 | 422),
                    )
                })
                .map(|sequence| format!("iges:entity:directory#{sequence}"))
                .collect::<Vec<_>>();
            let association_links = if trailing.is_some() {
                resolved_associations
            } else {
                Vec::new()
            };
            let property_links = if trailing.is_some() {
                resolved_properties
            } else {
                Vec::new()
            };
            NativeEntity {
                id: format!("iges:entity:directory#{}", entry.sequence),
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
                blank_status: entry.status.blank,
                subordinate_status: entry.status.subordinate,
                use_flag: entry.status.use_flag,
                hierarchy_status: entry.status.hierarchy,
                line_weight: entry.line_weight,
                color: entry.color,
                reserved: entry.reserved.iter().map(|value| value.to_vec()).collect(),
                label: entry.label.to_vec(),
                subscript: entry.subscript,
                parameter_line_start: parameters.map(|record| record.line_range.start),
                parameter_line_end: parameters.map(|record| record.line_range.end),
                parameter_bytes: parameters
                    .map(|record| record.bytes.clone())
                    .unwrap_or_default(),
                parameters: parameters
                    .into_iter()
                    .flat_map(|record| record.tokens.iter().map(token))
                    .collect(),
                association_links,
                property_links,
                comment: parameters
                    .map(|record| record.comment.clone())
                    .unwrap_or_default(),
                links: references
                    .get(&entry.sequence)
                    .into_iter()
                    .flatten()
                    .filter_map(ReferenceEdge::target)
                    .map(str::to_owned)
                    .collect(),
                references: references.get(&entry.sequence).cloned().unwrap_or_default(),
            }
        })
        .collect::<Vec<_>>();
    let directions = directory
        .iter()
        .filter(|entry| entry.entity_type == 123 && entry.form == 0)
        .map(|entry| {
            let parameters = by_directory.get(&entry.sequence).copied();
            NativeDirection {
                id: format!("iges:native:direction#D{}", entry.sequence),
                source_entity: format!("iges:entity:directory#{}", entry.sequence),
                components: (1..=3)
                    .map(|index| parameters.and_then(|record| record.number(index)))
                    .collect(),
                physically_dependent: entry.status.is_physically_dependent(),
                has_transform: entry.transform != 0,
            }
        })
        .collect::<Vec<_>>();
    let transforms = directory
        .iter()
        .filter(|entry| entry.entity_type == 124 && matches!(entry.form, 0 | 1 | 10 | 11 | 12))
        .map(|entry| {
            let parameters = by_directory.get(&entry.sequence).copied();
            NativeTransformation {
                id: format!("iges:native:transformation#D{}", entry.sequence),
                source_entity: format!("iges:entity:directory#{}", entry.sequence),
                form: entry.form,
                coefficients: (1..=12)
                    .map(|index| parameters.and_then(|record| record.number(index)))
                    .collect(),
                parent: (entry.transform > 0)
                    .then(|| format!("iges:native:transformation#D{}", entry.transform)),
            }
        })
        .collect::<Vec<_>>();
    let copious_data = directory
        .iter()
        .filter(|entry| entry.entity_type == 106)
        .map(|entry| {
            let parameters = by_directory.get(&entry.sequence).copied();
            let interpretation = parameters.and_then(|record| record.integer(1));
            let declared_tuple_count = parameters.and_then(|record| record.integer(2));
            let common_z = (interpretation == Some(1))
                .then(|| parameters.and_then(|record| record.number(3)))
                .flatten();
            let (start, width) = match interpretation {
                Some(1) => (4, 2),
                Some(2) => (3, 3),
                Some(3) => (3, 6),
                _ => (3, 1),
            };
            let count = declared_tuple_count
                .and_then(|value| usize::try_from(value).ok())
                .unwrap_or_default();
            let tuples = parameters
                .map(|record| {
                    let available = record.tokens.len().saturating_sub(start) / width;
                    (0..count.min(available))
                        .map(|tuple| {
                            (0..width)
                                .map(|component| {
                                    tuple
                                        .checked_mul(width)
                                        .and_then(|offset| offset.checked_add(start))
                                        .and_then(|offset| offset.checked_add(component))
                                        .and_then(|index| record.number(index))
                                })
                                .collect()
                        })
                        .collect()
                })
                .unwrap_or_default();
            NativeCopiousData {
                id: format!("iges:native:copious-data#D{}", entry.sequence),
                source_entity: format!("iges:entity:directory#{}", entry.sequence),
                form: entry.form,
                interpretation,
                declared_tuple_count,
                common_z,
                tuples,
            }
        })
        .collect::<Vec<_>>();
    let colors = directory
        .iter()
        .filter(|entry| entry.entity_type == 314 && entry.form == 0)
        .map(|entry| {
            let parameters = by_directory.get(&entry.sequence).copied();
            NativeColorDefinition {
                id: format!("iges:presentation:color#D{}", entry.sequence),
                source_entity: format!("iges:entity:directory#{}", entry.sequence),
                red_percent: parameters.and_then(|record| record.number(1)),
                green_percent: parameters.and_then(|record| record.number(2)),
                blue_percent: parameters.and_then(|record| record.number(3)),
                name: parameters
                    .and_then(|record| record.string(4))
                    .map(<[u8]>::to_vec),
                fallback_color_number: entry.color,
            }
        })
        .collect::<Vec<_>>();
    let display_attributes = directory
        .iter()
        .map(|entry| NativeDisplayAttributes {
            id: format!("iges:presentation:display-attributes#D{}", entry.sequence),
            source_entity: format!("iges:entity:directory#{}", entry.sequence),
            visible: entry.status.blank == 0,
            line_font_number: entry.line_font,
            line_font_definition: (entry.line_font < 0).then(|| {
                format!(
                    "iges:presentation:line-font#D{}",
                    entry.line_font.unsigned_abs()
                )
            }),
            level_number: entry.level,
            level_definition: (entry.level < 0).then(|| {
                format!(
                    "iges:presentation:definition-levels#D{}",
                    entry.level.unsigned_abs()
                )
            }),
            view: entry.view,
            line_weight_number: entry.line_weight,
            line_weight_mm: global.line_weight_mm(entry.line_weight),
            color_number: entry.color,
            color_definition: (entry.color < 0)
                .then(|| format!("iges:presentation:color#D{}", entry.color.unsigned_abs())),
        })
        .collect::<Vec<_>>();
    let line_fonts = directory
        .iter()
        .filter(|entry| entry.entity_type == 304 && matches!(entry.form, 1 | 2))
        .map(|entry| {
            let parameters = by_directory.get(&entry.sequence).copied();
            if entry.form == 1 {
                NativeLineFontDefinition::Template {
                    id: format!("iges:presentation:line-font#D{}", entry.sequence),
                    source_entity: format!("iges:entity:directory#{}", entry.sequence),
                    fallback_line_font_number: entry.line_font,
                    tangent_oriented: binary_integer(
                        parameters.and_then(|record| record.integer(1)),
                    ),
                    template: parameters
                        .and_then(|record| record.integer(2))
                        .and_then(|sequence| {
                            parameter_resolver.resolve_type(entry.sequence, 2, sequence, 308, &[0])
                        })
                        .map(|sequence| format!("iges:entity:directory#{sequence}")),
                    spacing: parameters.and_then(|record| record.number(3)),
                    scale: parameters.and_then(|record| record.number(4)),
                }
            } else {
                let count = parameters
                    .and_then(|record| record.count(1))
                    .unwrap_or_default();
                NativeLineFontDefinition::VisibleBlankPattern {
                    id: format!("iges:presentation:line-font#D{}", entry.sequence),
                    source_entity: format!("iges:entity:directory#{}", entry.sequence),
                    fallback_line_font_number: entry.line_font,
                    segment_count: parameters.and_then(|record| record.integer(1)),
                    lengths: (0..count)
                        .map(|index| parameters.and_then(|record| record.number(2 + index)))
                        .collect(),
                    hexadecimal_pattern: parameters
                        .and_then(|record| record.string(2 + count))
                        .map(<[u8]>::to_vec),
                }
            }
        })
        .collect::<Vec<_>>();
    let text_templates = directory
        .iter()
        .filter(|entry| entry.entity_type == 312 && matches!(entry.form, 0..=1))
        .map(|entry| {
            let record = by_directory.get(&entry.sequence).copied();
            let font_code = record.and_then(|record| record.integer(3));
            NativeTextDisplayTemplate {
                id: format!("iges:presentation:text-template#D{}", entry.sequence),
                source_entity: format!("iges:entity:directory#{}", entry.sequence),
                form: entry.form,
                character_box: [
                    record.and_then(|record| record.number(1)),
                    record.and_then(|record| record.number(2)),
                ],
                font_code,
                font_definition: font_code
                    .filter(|value| *value < 0)
                    .and_then(|value| {
                        parameter_resolver.resolve_negative(
                            entry.sequence,
                            3,
                            value,
                            "type-310-form-0",
                            |target| target.entity_type == 310 && target.form == 0,
                        )
                    })
                    .map(|sequence| format!("iges:presentation:text-font#D{sequence}")),
                slant_angle: record.and_then(|record| record.number(4)),
                rotation_angle: record.and_then(|record| record.number(5)),
                mirror: record.and_then(|record| record.integer(6)),
                vertical: record.and_then(|record| record.integer(7)),
                origin_or_increment: [
                    record.and_then(|record| record.number(8)),
                    record.and_then(|record| record.number(9)),
                    record.and_then(|record| record.number(10)),
                ],
            }
        })
        .collect::<Vec<_>>();
    let text_fonts = directory
        .iter()
        .filter(|entry| entry.entity_type == 310 && entry.form == 0)
        .map(|entry| {
            let record = by_directory.get(&entry.sequence).copied();
            let count = record
                .and_then(|record| record.count(5))
                .unwrap_or_default();
            let supersedes_code = record.and_then(|record| record.integer(3));
            let mut cursor = 6_usize;
            let mut characters = Vec::with_capacity(count);
            for _ in 0..count {
                let Some(record) = record else { break };
                let Some(count_index) = cursor.checked_add(3) else {
                    break;
                };
                let declared_motion_count = record.integer(count_index);
                let motion_count = record.count_with_stride(count_index, 3);
                let motions = motion_count
                    .into_iter()
                    .flat_map(|count| 0..count)
                    .map(|offset| {
                        let start = cursor + 4 + offset * 3;
                        NativeGlyphMotion {
                            pen_up: record.integer(start).map(|value| value == 1),
                            point: [record.integer(start + 1), record.integer(start + 2)],
                        }
                    })
                    .collect();
                characters.push(NativeGlyph {
                    character_code: record.integer(cursor),
                    next_origin: [record.integer(cursor + 1), record.integer(cursor + 2)],
                    declared_motion_count,
                    motions,
                });
                let Some(motion_count) = motion_count else {
                    break;
                };
                let Some(next) = motion_count
                    .checked_mul(3)
                    .and_then(|width| cursor.checked_add(4 + width))
                else {
                    break;
                };
                cursor = next;
            }
            NativeTextFontDefinition {
                id: format!("iges:presentation:text-font#D{}", entry.sequence),
                source_entity: format!("iges:entity:directory#{}", entry.sequence),
                font_code: record.and_then(|record| record.integer(1)),
                name: record
                    .and_then(|record| record.string(2))
                    .map(<[u8]>::to_vec),
                supersedes_code,
                supersedes_definition: supersedes_code
                    .filter(|value| *value < 0)
                    .and_then(|value| {
                        parameter_resolver.resolve_negative(
                            entry.sequence,
                            3,
                            value,
                            "type-310-form-0",
                            |target| target.entity_type == 310 && target.form == 0,
                        )
                    })
                    .map(|sequence| format!("iges:presentation:text-font#D{sequence}")),
                grid_units_per_text_height: record.and_then(|record| record.integer(4)),
                declared_character_count: record.and_then(|record| record.integer(5)),
                characters,
            }
        })
        .collect::<Vec<_>>();
    let definition_levels = directory
        .iter()
        .filter(|entry| entry.entity_type == 406 && entry.form == 1)
        .map(|entry| {
            let parameters = by_directory.get(&entry.sequence).copied();
            let count = parameters
                .and_then(|record| record.count(1))
                .unwrap_or_default();
            NativeDefinitionLevels {
                id: format!("iges:presentation:definition-levels#D{}", entry.sequence),
                source_entity: format!("iges:entity:directory#{}", entry.sequence),
                declared_count: parameters.and_then(|record| record.integer(1)),
                levels: (0..count)
                    .map(|index| parameters.and_then(|record| record.integer(2 + index)))
                    .collect(),
            }
        })
        .collect::<Vec<_>>();
    let primitive_solids = directory
        .iter()
        .filter(|entry| matches!(entry.entity_type, 150 | 152 | 154 | 156 | 158 | 160 | 168))
        .filter_map(|entry| {
            let record = by_directory.get(&entry.sequence).copied();
            let number = |index| record.and_then(|record| record.number(index));
            let (kind, dimension_names, origin_start, x_axis_start, z_axis_start) =
                match entry.entity_type {
                    150 => (
                        "block",
                        vec!["x_length", "y_length", "z_length"],
                        4,
                        Some(7),
                        Some(10),
                    ),
                    152 => (
                        "right_angular_wedge",
                        vec!["x_length", "y_length", "z_length", "top_x_length"],
                        5,
                        Some(8),
                        Some(11),
                    ),
                    154 => (
                        "right_circular_cylinder",
                        vec!["height", "radius"],
                        3,
                        None,
                        Some(6),
                    ),
                    156 => (
                        "right_circular_cone_frustum",
                        vec!["height", "large_radius", "small_radius"],
                        4,
                        None,
                        Some(7),
                    ),
                    158 => ("sphere", vec!["radius"], 2, None, None),
                    160 => (
                        "torus",
                        vec!["major_radius", "minor_radius"],
                        3,
                        None,
                        Some(6),
                    ),
                    168 => (
                        "ellipsoid",
                        vec!["x_radius", "y_radius", "z_radius"],
                        4,
                        Some(7),
                        Some(10),
                    ),
                    _ => return None,
                };
            let dimensions = dimension_names
                .into_iter()
                .enumerate()
                .map(|(index, name)| (name.to_owned(), number(index + 1)))
                .collect();
            let axis = |start: usize| [number(start), number(start + 1), number(start + 2)];
            Some(NativePrimitiveSolid {
                id: format!("iges:solid:primitive#D{}", entry.sequence),
                source_entity: format!("iges:entity:directory#{}", entry.sequence),
                kind: kind.into(),
                dimensions,
                origin: axis(origin_start),
                x_axis: x_axis_start.map(axis),
                z_axis: z_axis_start.map(axis),
                transformation: (entry.transform > 0)
                    .then(|| format!("iges:native:transformation#D{}", entry.transform)),
            })
        })
        .collect::<Vec<_>>();
    let procedural_solids = directory
        .iter()
        .filter(|entry| matches!(entry.entity_type, 162 | 164))
        .map(|entry| {
            let record = by_directory.get(&entry.sequence).copied();
            let number = |index| record.and_then(|record| record.number(index));
            let axis = |start: usize| [number(start), number(start + 1), number(start + 2)];
            let revolution = entry.entity_type == 162;
            NativeProceduralSolid {
                id: format!("iges:solid:procedural#D{}", entry.sequence),
                source_entity: format!("iges:entity:directory#{}", entry.sequence),
                kind: if revolution {
                    "revolution".into()
                } else {
                    "linear_extrusion".into()
                },
                form: entry.form,
                profile: record
                    .and_then(|record| record.integer(1))
                    .and_then(|sequence| {
                        parameter_resolver.resolve(
                            entry.sequence,
                            1,
                            sequence,
                            "curve-entity",
                            |target| {
                                matches!(
                                    target.entity_type,
                                    100 | 102 | 104 | 106 | 110 | 112 | 126 | 130
                                )
                            },
                        )
                    })
                    .map(|sequence| format!("iges:entity:directory#{sequence}")),
                amount: number(2),
                origin: revolution.then(|| axis(3)),
                direction: axis(if revolution { 6 } else { 3 }),
                transformation: (entry.transform > 0)
                    .then(|| format!("iges:native:transformation#D{}", entry.transform)),
            }
        })
        .collect::<Vec<_>>();
    let boolean_trees = directory
        .iter()
        .filter(|entry| entry.entity_type == 180 && matches!(entry.form, 0 | 1))
        .map(|entry| {
            let record = by_directory.get(&entry.sequence).copied();
            let count = record
                .and_then(|record| record.count(1))
                .unwrap_or_default();
            let terms = (0..count)
                .filter_map(|index| {
                    record
                        .and_then(|record| record.integer(2 + index))
                        .map(|value| (index, value))
                })
                .map(|(index, value)| {
                    if value < 0 {
                        NativeBooleanTerm::Operand {
                            entity: parameter_resolver
                                .resolve_negative(
                                    entry.sequence,
                                    2 + index,
                                    value,
                                    if entry.form == 1 {
                                        "constructive-solid-or-type-186"
                                    } else {
                                        "constructive-solid"
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
                                )
                                .map(|sequence| format!("iges:entity:directory#{sequence}")),
                            raw: value,
                        }
                    } else {
                        NativeBooleanTerm::Operation { operation: value }
                    }
                })
                .collect();
            NativeBooleanTree {
                id: format!("iges:solid:boolean-tree#D{}", entry.sequence),
                source_entity: format!("iges:entity:directory#{}", entry.sequence),
                form: entry.form,
                declared_length: record.and_then(|record| record.integer(1)),
                terms,
                transformation: (entry.transform > 0)
                    .then(|| format!("iges:native:transformation#D{}", entry.transform)),
            }
        })
        .collect::<Vec<_>>();
    let selected_components = directory
        .iter()
        .filter(|entry| entry.entity_type == 182 && entry.form == 0)
        .map(|entry| {
            let record = by_directory.get(&entry.sequence).copied();
            NativeSelectedComponent {
                id: format!("iges:solid:selected-component#D{}", entry.sequence),
                source_entity: format!("iges:entity:directory#{}", entry.sequence),
                boolean_tree: record
                    .and_then(|record| record.integer(1))
                    .and_then(|sequence| {
                        parameter_resolver.resolve(
                            entry.sequence,
                            1,
                            sequence,
                            "type-180-form-0-or-1",
                            |target| target.entity_type == 180 && matches!(target.form, 0 | 1),
                        )
                    })
                    .map(|sequence| format!("iges:solid:boolean-tree#D{sequence}")),
                selection_point: [
                    record.and_then(|record| record.number(2)),
                    record.and_then(|record| record.number(3)),
                    record.and_then(|record| record.number(4)),
                ],
                transformation: (entry.transform > 0)
                    .then(|| format!("iges:native:transformation#D{}", entry.transform)),
            }
        })
        .collect::<Vec<_>>();
    let solid_assemblies = directory
        .iter()
        .filter(|entry| entry.entity_type == 184 && matches!(entry.form, 0 | 1))
        .map(|entry| {
            let record = by_directory.get(&entry.sequence).copied();
            let count = record
                .and_then(|record| record.count(1))
                .unwrap_or_default();
            NativeSolidAssembly {
                id: format!("iges:product:solid-assembly#D{}", entry.sequence),
                source_entity: format!("iges:entity:directory#{}", entry.sequence),
                form: entry.form,
                declared_count: record.and_then(|record| record.integer(1)),
                items: (0..count)
                    .map(|index| NativeAssemblyItem {
                        item: record
                            .and_then(|record| record.integer(2 + index))
                            .and_then(|sequence| {
                                parameter_resolver.resolve(
                                    entry.sequence,
                                    2 + index,
                                    sequence,
                                    if entry.form == 1 {
                                        "constructive-solid-or-type-186"
                                    } else {
                                        "constructive-solid"
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
                            .map(|sequence| format!("iges:entity:directory#{sequence}")),
                        transformation: record
                            .and_then(|record| record.integer(2 + count + index))
                            .filter(|sequence| *sequence != 0)
                            .and_then(|sequence| {
                                parameter_resolver.resolve_type(
                                    entry.sequence,
                                    2 + count + index,
                                    sequence,
                                    124,
                                    &[],
                                )
                            })
                            .map(|sequence| format!("iges:native:transformation#D{sequence}")),
                    })
                    .collect(),
                transformation: (entry.transform > 0)
                    .then(|| format!("iges:native:transformation#D{}", entry.transform)),
            }
        })
        .collect::<Vec<_>>();
    let solid_instances = directory
        .iter()
        .filter(|entry| entry.entity_type == 430 && matches!(entry.form, 0 | 1))
        .map(|entry| {
            let record = by_directory.get(&entry.sequence).copied();
            NativeSolidInstance {
                id: format!("iges:product:solid-instance#D{}", entry.sequence),
                source_entity: format!("iges:entity:directory#{}", entry.sequence),
                form: entry.form,
                solid: record
                    .and_then(|record| record.integer(1))
                    .and_then(|sequence| {
                        parameter_resolver.resolve(
                            entry.sequence,
                            1,
                            sequence,
                            if entry.form == 1 {
                                "type-186"
                            } else {
                                "constructive-solid"
                            },
                            |target| {
                                if entry.form == 1 {
                                    target.entity_type == 186
                                } else {
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
                                }
                            },
                        )
                    })
                    .map(|sequence| format!("iges:entity:directory#{sequence}")),
                transformation: (entry.transform > 0)
                    .then(|| format!("iges:native:transformation#D{}", entry.transform)),
            }
        })
        .collect::<Vec<_>>();
    let subfigure_definitions = directory
        .iter()
        .filter(|entry| entry.entity_type == 308 && entry.form == 0)
        .map(|entry| {
            let record = by_directory.get(&entry.sequence).copied();
            let count = record
                .and_then(|record| record.count(3))
                .unwrap_or_default();
            NativeSubfigureDefinition {
                id: format!("iges:product:subfigure-definition#D{}", entry.sequence),
                source_entity: format!("iges:entity:directory#{}", entry.sequence),
                depth: record.and_then(|record| record.integer(1)),
                name: record
                    .and_then(|record| record.string(2))
                    .map(<[u8]>::to_vec),
                declared_member_count: record.and_then(|record| record.integer(3)),
                members: (0..count)
                    .map(|index| {
                        record
                            .and_then(|record| record.integer(4 + index))
                            .and_then(|sequence| {
                                parameter_resolver.resolve_any(entry.sequence, 4 + index, sequence)
                            })
                            .map(|sequence| format!("iges:entity:directory#{sequence}"))
                    })
                    .collect(),
            }
        })
        .collect::<Vec<_>>();
    let subfigure_instances = directory
        .iter()
        .filter(|entry| entry.entity_type == 408 && entry.form == 0)
        .map(|entry| {
            let record = by_directory.get(&entry.sequence).copied();
            NativeSubfigureInstance {
                id: format!("iges:product:subfigure-instance#D{}", entry.sequence),
                source_entity: format!("iges:entity:directory#{}", entry.sequence),
                definition: record
                    .and_then(|record| record.integer(1))
                    .and_then(|sequence| {
                        parameter_resolver.resolve_type(entry.sequence, 1, sequence, 308, &[0])
                    })
                    .map(|sequence| format!("iges:product:subfigure-definition#D{sequence}")),
                translation: [
                    record.and_then(|record| record.number(2)),
                    record.and_then(|record| record.number(3)),
                    record.and_then(|record| record.number(4)),
                ],
                scale: record.and_then(|record| record.number(5)),
                transformation: (entry.transform > 0)
                    .then(|| format!("iges:native:transformation#D{}", entry.transform)),
            }
        })
        .collect::<Vec<_>>();
    let network_definitions = directory
        .iter()
        .filter(|entry| entry.entity_type == 320 && entry.form == 0)
        .map(|entry| {
            let record = by_directory.get(&entry.sequence).copied();
            let member_count = record
                .and_then(|record| record.count(3))
                .unwrap_or_default();
            let connect_count_index = 7 + member_count;
            let connect_count = record
                .and_then(|record| record.count(connect_count_index))
                .unwrap_or_default();
            NativeNetworkDefinition {
                id: format!("iges:product:network-definition#D{}", entry.sequence),
                source_entity: format!("iges:entity:directory#{}", entry.sequence),
                depth: record.and_then(|record| record.integer(1)),
                name: record
                    .and_then(|record| record.string(2))
                    .map(<[u8]>::to_vec),
                declared_member_count: record.and_then(|record| record.integer(3)),
                members: (0..member_count)
                    .map(|index| {
                        record
                            .and_then(|record| record.integer(4 + index))
                            .and_then(|sequence| {
                                parameter_resolver.resolve_any(entry.sequence, 4 + index, sequence)
                            })
                            .map(|sequence| format!("iges:entity:directory#{sequence}"))
                    })
                    .collect(),
                type_flag: record.and_then(|record| record.integer(4 + member_count)),
                primary_reference_designator: record
                    .and_then(|record| record.string(5 + member_count))
                    .map(<[u8]>::to_vec),
                display_template: record
                    .and_then(|record| record.integer(6 + member_count))
                    .filter(|sequence| *sequence != 0)
                    .and_then(|sequence| {
                        parameter_resolver.resolve_type(
                            entry.sequence,
                            6 + member_count,
                            sequence,
                            312,
                            &[0, 1],
                        )
                    })
                    .map(|sequence| format!("iges:entity:directory#{sequence}")),
                declared_connect_point_count: record
                    .and_then(|record| record.integer(connect_count_index)),
                connect_points: (0..connect_count)
                    .map(|index| {
                        record
                            .and_then(|record| record.integer(8 + member_count + index))
                            .filter(|sequence| *sequence != 0)
                            .and_then(|sequence| {
                                parameter_resolver.resolve_type(
                                    entry.sequence,
                                    8 + member_count + index,
                                    sequence,
                                    132,
                                    &[0],
                                )
                            })
                            .map(|sequence| format!("iges:entity:directory#{sequence}"))
                    })
                    .collect(),
            }
        })
        .collect::<Vec<_>>();
    let network_instances = directory
        .iter()
        .filter(|entry| entry.entity_type == 420 && entry.form == 0)
        .map(|entry| {
            let record = by_directory.get(&entry.sequence).copied();
            let connect_count = record
                .and_then(|record| record.count(11))
                .unwrap_or_default();
            NativeNetworkInstance {
                id: format!("iges:product:network-instance#D{}", entry.sequence),
                source_entity: format!("iges:entity:directory#{}", entry.sequence),
                definition: record
                    .and_then(|record| record.integer(1))
                    .and_then(|sequence| {
                        parameter_resolver.resolve_type(entry.sequence, 1, sequence, 320, &[0])
                    })
                    .map(|sequence| format!("iges:product:network-definition#D{sequence}")),
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
                    .and_then(|record| record.string(9))
                    .map(<[u8]>::to_vec),
                display_template: record
                    .and_then(|record| record.integer(10))
                    .filter(|sequence| *sequence != 0)
                    .and_then(|sequence| {
                        parameter_resolver.resolve_type(entry.sequence, 10, sequence, 312, &[0, 1])
                    })
                    .map(|sequence| format!("iges:entity:directory#{sequence}")),
                declared_connect_point_count: record.and_then(|record| record.integer(11)),
                connect_points: (0..connect_count)
                    .map(|index| {
                        record
                            .and_then(|record| record.integer(12 + index))
                            .filter(|sequence| *sequence != 0)
                            .and_then(|sequence| {
                                parameter_resolver.resolve_type(
                                    entry.sequence,
                                    12 + index,
                                    sequence,
                                    132,
                                    &[0],
                                )
                            })
                            .map(|sequence| format!("iges:entity:directory#{sequence}"))
                    })
                    .collect(),
                transformation: (entry.transform > 0)
                    .then(|| format!("iges:native:transformation#D{}", entry.transform)),
            }
        })
        .collect::<Vec<_>>();
    let connect_points = directory
        .iter()
        .filter(|entry| entry.entity_type == 132 && entry.form == 0)
        .map(|entry| {
            let record = by_directory.get(&entry.sequence).copied();
            let optional_entity_link = |index| {
                record
                    .and_then(|record| record.integer(index))
                    .filter(|sequence| *sequence != 0)
                    .and_then(|sequence| {
                        parameter_resolver.resolve_any(entry.sequence, index, sequence)
                    })
                    .map(|sequence| format!("iges:entity:directory#{sequence}"))
            };
            let optional_template_link = |index| {
                record
                    .and_then(|record| record.integer(index))
                    .filter(|sequence| *sequence != 0)
                    .and_then(|sequence| {
                        parameter_resolver.resolve_type(
                            entry.sequence,
                            index,
                            sequence,
                            312,
                            &[0, 1],
                        )
                    })
                    .map(|sequence| format!("iges:entity:directory#{sequence}"))
            };
            NativeConnectPoint {
                id: format!("iges:product:connect-point#D{}", entry.sequence),
                source_entity: format!("iges:entity:directory#{}", entry.sequence),
                position: [
                    record.and_then(|record| record.number(1)),
                    record.and_then(|record| record.number(2)),
                    record.and_then(|record| record.number(3)),
                ],
                display_geometry: optional_entity_link(4),
                type_flag: record.and_then(|record| record.integer(5)),
                function_flag: record.and_then(|record| record.integer(6)),
                function_identifier: record
                    .and_then(|record| record.string(7))
                    .map(<[u8]>::to_vec),
                identifier_display_template: optional_template_link(8),
                function_name: record
                    .and_then(|record| record.string(9))
                    .map(<[u8]>::to_vec),
                name_display_template: optional_template_link(10),
                identifier: record.and_then(|record| record.integer(11)),
                function_code: record.and_then(|record| record.integer(12)),
                swap_flag: record.and_then(|record| record.integer(13)),
                owner: record
                    .and_then(|record| record.integer(14))
                    .filter(|sequence| *sequence != 0)
                    .and_then(|sequence| {
                        parameter_resolver.resolve(
                            entry.sequence,
                            14,
                            sequence,
                            "type-320-or-type-420",
                            |target| matches!(target.entity_type, 320 | 420),
                        )
                    })
                    .map(|sequence| format!("iges:entity:directory#{sequence}")),
                transformation: (entry.transform > 0)
                    .then(|| format!("iges:native:transformation#D{}", entry.transform)),
            }
        })
        .collect::<Vec<_>>();
    let rectangular_arrays = directory
        .iter()
        .filter(|entry| entry.entity_type == 412 && entry.form == 0)
        .map(|entry| {
            let record = by_directory.get(&entry.sequence).copied();
            let count = record
                .and_then(|record| record.count(11))
                .unwrap_or_default();
            NativeRectangularArray {
                id: format!("iges:product:rectangular-array#D{}", entry.sequence),
                source_entity: format!("iges:entity:directory#{}", entry.sequence),
                base: record
                    .and_then(|record| record.integer(1))
                    .and_then(|sequence| {
                        parameter_resolver.resolve(
                            entry.sequence,
                            1,
                            sequence,
                            "array-base-entity",
                            |target| array_base_type(target.entity_type, target.form),
                        )
                    })
                    .map(|sequence| format!("iges:entity:directory#{sequence}")),
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
                positions: (0..count)
                    .map(|index| record.and_then(|record| record.integer(13 + index)))
                    .collect(),
                transformation: (entry.transform > 0)
                    .then(|| format!("iges:native:transformation#D{}", entry.transform)),
            }
        })
        .collect::<Vec<_>>();
    let circular_arrays = directory
        .iter()
        .filter(|entry| entry.entity_type == 414 && entry.form == 0)
        .map(|entry| {
            let record = by_directory.get(&entry.sequence).copied();
            let count = record
                .and_then(|record| record.count(9))
                .unwrap_or_default();
            NativeCircularArray {
                id: format!("iges:product:circular-array#D{}", entry.sequence),
                source_entity: format!("iges:entity:directory#{}", entry.sequence),
                base: record
                    .and_then(|record| record.integer(1))
                    .and_then(|sequence| {
                        parameter_resolver.resolve(
                            entry.sequence,
                            1,
                            sequence,
                            "array-base-entity",
                            |target| array_base_type(target.entity_type, target.form),
                        )
                    })
                    .map(|sequence| format!("iges:entity:directory#{sequence}")),
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
                positions: (0..count)
                    .map(|index| record.and_then(|record| record.integer(11 + index)))
                    .collect(),
                transformation: (entry.transform > 0)
                    .then(|| format!("iges:native:transformation#D{}", entry.transform)),
            }
        })
        .collect::<Vec<_>>();
    let external_references = directory
        .iter()
        .filter(|entry| entry.entity_type == 416 && matches!(entry.form, 0..=4))
        .filter_map(|entry| {
            let record = by_directory.get(&entry.sequence).copied();
            let (reference_kind, file_index, symbolic_index, library_index) = match entry.form {
                0 => ("external_definition", Some(1), Some(2), None),
                1 => ("external_file_definition", Some(1), None, None),
                2 => ("external_logical", Some(1), Some(2), None),
                3 => ("native_definition", None, Some(1), None),
                4 => ("native_library_definition", None, Some(2), Some(1)),
                _ => return None,
            };
            Some(NativeExternalReference {
                id: format!("iges:product:external-reference#D{}", entry.sequence),
                source_entity: format!("iges:entity:directory#{}", entry.sequence),
                form: entry.form,
                reference_kind: reference_kind.into(),
                file_identifier: file_index.and_then(|index| {
                    record
                        .and_then(|record| record.string(index))
                        .map(<[u8]>::to_vec)
                }),
                symbolic_name: symbolic_index.and_then(|index| {
                    record
                        .and_then(|record| record.string(index))
                        .map(<[u8]>::to_vec)
                }),
                library_name: library_index.and_then(|index| {
                    record
                        .and_then(|record| record.string(index))
                        .map(<[u8]>::to_vec)
                }),
                resolution_state: "not_attempted".into(),
            })
        })
        .collect::<Vec<_>>();
    let groups = directory
        .iter()
        .filter(|entry| entry.entity_type == 402 && matches!(entry.form, 1 | 7 | 14 | 15))
        .map(|entry| {
            let record = by_directory.get(&entry.sequence).copied();
            let count = record
                .and_then(|record| record.count(1))
                .unwrap_or_default();
            NativeGroup {
                id: format!("iges:product:group#D{}", entry.sequence),
                source_entity: format!("iges:entity:directory#{}", entry.sequence),
                ordered: matches!(entry.form, 14 | 15),
                back_pointers_required: matches!(entry.form, 1 | 14),
                declared_member_count: record.and_then(|record| record.integer(1)),
                members: (0..count)
                    .map(|index| {
                        record
                            .and_then(|record| record.integer(2 + index))
                            .and_then(|sequence| {
                                parameter_resolver.resolve_any(entry.sequence, 2 + index, sequence)
                            })
                            .map(|sequence| format!("iges:entity:directory#{sequence}"))
                    })
                    .collect(),
            }
        })
        .collect::<Vec<_>>();
    let mut associativities = directory
        .iter()
        .filter(|entry| entry.entity_type == 302)
        .map(|entry| {
            let record = by_directory.get(&entry.sequence).copied();
            let class_count = record
                .and_then(|record| record.count(1))
                .unwrap_or_default();
            let mut cursor = 2_usize;
            let mut classes = Vec::with_capacity(class_count);
            for _ in 0..class_count {
                let Some(record) = record else { break };
                let Some(count_index) = cursor.checked_add(2) else {
                    break;
                };
                let item_count = record.count(count_index);
                classes.push(NativeAssociativityClassDefinition {
                    back_pointers_required: record.integer(cursor).map(|value| value == 1),
                    ordered: record.integer(cursor + 1).map(|value| value == 1),
                    declared_item_count: record.integer(cursor + 2),
                    item_types: item_count
                        .into_iter()
                        .flat_map(|count| 0..count)
                        .map(|offset| record.integer(cursor + 3 + offset))
                        .collect(),
                });
                let Some(item_count) = item_count else { break };
                let Some(next) = cursor.checked_add(3 + item_count) else {
                    break;
                };
                cursor = next;
            }
            NativeAssociativity::Definition {
                id: format!("iges:structure:associativity#D{}", entry.sequence),
                source_entity: format!("iges:entity:directory#{}", entry.sequence),
                associativity_form: entry.form,
                declared_class_count: record.and_then(|record| record.integer(1)),
                classes,
            }
        })
        .collect::<Vec<_>>();
    associativities.extend(
        directory
            .iter()
            .filter(|entry| {
                entry.entity_type == 402
                    && matches!(entry.form, 5 | 6 | 9 | 12 | 13 | 16 | 18 | 20 | 21)
            })
            .filter_map(|entry| {
                let record = by_directory.get(&entry.sequence).copied();
                let id = format!("iges:structure:associativity#D{}", entry.sequence);
                let source_entity = format!("iges:entity:directory#{}", entry.sequence);
                let entity_link = |index| {
                    record
                        .and_then(|record| record.integer(index))
                        .filter(|sequence| *sequence != 0)
                        .and_then(|sequence| {
                            parameter_resolver.resolve_any(entry.sequence, index, sequence)
                        })
                        .map(|sequence| format!("iges:entity:directory#{sequence}"))
                };
                Some(match entry.form {
                    5 => {
                        let count = record
                            .and_then(|record| record.count(1))
                            .unwrap_or_default();
                        NativeAssociativity::LabelDisplay {
                            id,
                            source_entity,
                            declared_count: record.and_then(|record| record.integer(1)),
                            placements: (0..count)
                                .map(|offset| {
                                    let start = 2 + offset * 7;
                                    NativeLabelPlacement {
                                        view: record
                                            .and_then(|record| record.integer(start))
                                            .and_then(|sequence| {
                                                parameter_resolver.resolve_type(
                                                    entry.sequence,
                                                    start,
                                                    sequence,
                                                    410,
                                                    &[0, 1],
                                                )
                                            })
                                            .map(|sequence| {
                                                format!("iges:entity:directory#{sequence}")
                                            }),
                                        text_location: [
                                            record.and_then(|record| record.number(start + 1)),
                                            record.and_then(|record| record.number(start + 2)),
                                            record.and_then(|record| record.number(start + 3)),
                                        ],
                                        leader: record
                                            .and_then(|record| record.integer(start + 4))
                                            .and_then(|sequence| {
                                                parameter_resolver.resolve_type(
                                                    entry.sequence,
                                                    start + 4,
                                                    sequence,
                                                    214,
                                                    &[],
                                                )
                                            })
                                            .map(|sequence| {
                                                format!("iges:entity:directory#{sequence}")
                                            }),
                                        label_level: record
                                            .and_then(|record| record.integer(start + 5)),
                                        entity: entity_link(start + 6),
                                    }
                                })
                                .collect(),
                        }
                    }
                    6 => {
                        let count = record
                            .and_then(|record| record.count(1))
                            .unwrap_or_default();
                        NativeAssociativity::ViewList {
                            id,
                            source_entity,
                            declared_visible_count: record.and_then(|record| record.integer(1)),
                            view: record
                                .and_then(|record| record.integer(2))
                                .and_then(|sequence| {
                                    parameter_resolver.resolve_type(
                                        entry.sequence,
                                        2,
                                        sequence,
                                        410,
                                        &[0, 1],
                                    )
                                })
                                .map(|sequence| format!("iges:entity:directory#{sequence}")),
                            visible_entities: (0..count)
                                .map(|offset| entity_link(3 + offset))
                                .collect(),
                        }
                    }
                    9 => {
                        let count = record
                            .and_then(|record| record.count(2))
                            .unwrap_or_default();
                        NativeAssociativity::SingleParent {
                            id,
                            source_entity,
                            declared_child_count: record.and_then(|record| record.integer(2)),
                            parent: entity_link(3),
                            children: (0..count).map(|offset| entity_link(4 + offset)).collect(),
                        }
                    }
                    12 => {
                        let count = record
                            .and_then(|record| record.count(1))
                            .unwrap_or_default();
                        NativeAssociativity::ExternalReferenceIndex {
                            id,
                            source_entity,
                            declared_count: record.and_then(|record| record.integer(1)),
                            entries: (0..count)
                                .map(|offset| {
                                    let start = 2 + offset * 2;
                                    NativeExternalIndexEntry {
                                        symbolic_name: record
                                            .and_then(|record| record.string(start))
                                            .map(<[u8]>::to_vec),
                                        entity: entity_link(start + 1),
                                    }
                                })
                                .collect(),
                        }
                    }
                    13 => {
                        let count = record
                            .and_then(|record| record.count(2))
                            .unwrap_or_default();
                        NativeAssociativity::DimensionedGeometry {
                            id,
                            source_entity,
                            declared_geometry_count: record.and_then(|record| record.integer(2)),
                            dimension: record
                                .and_then(|record| record.integer(3))
                                .and_then(|sequence| {
                                    parameter_resolver.resolve(
                                        entry.sequence,
                                        3,
                                        sequence,
                                        "dimension-entity",
                                        |target| {
                                            matches!(
                                                target.entity_type,
                                                202 | 206 | 216 | 218 | 220 | 222
                                            )
                                        },
                                    )
                                })
                                .map(|sequence| format!("iges:entity:directory#{sequence}")),
                            geometry: (0..count).map(|offset| entity_link(4 + offset)).collect(),
                        }
                    }
                    16 => {
                        let count = record
                            .and_then(|record| record.count(2))
                            .unwrap_or_default();
                        NativeAssociativity::Planar {
                            id,
                            source_entity,
                            declared_entity_count: record.and_then(|record| record.integer(2)),
                            plane_transform: record
                                .and_then(|record| record.integer(3))
                                .filter(|sequence| *sequence != 0)
                                .and_then(|sequence| {
                                    parameter_resolver.resolve_type(
                                        entry.sequence,
                                        3,
                                        sequence,
                                        124,
                                        &[0],
                                    )
                                })
                                .map(|sequence| format!("iges:native:transformation#D{sequence}")),
                            entities: (0..count).map(|offset| entity_link(4 + offset)).collect(),
                        }
                    }
                    18 | 20 => {
                        let counts = (2..=7)
                            .map(|index| {
                                record
                                    .and_then(|record| record.count(index))
                                    .unwrap_or_default()
                            })
                            .collect::<Vec<_>>();
                        let flow_links = |start, count| {
                            (0..count)
                                .map(|offset| {
                                    let index = start + offset;
                                    record
                                        .and_then(|record| record.integer(index))
                                        .and_then(|sequence| {
                                            parameter_resolver.resolve(
                                                entry.sequence,
                                                index,
                                                sequence,
                                                "matching-flow-associativity",
                                                |target| {
                                                    target.entity_type == 402
                                                        && target.form == entry.form
                                                },
                                            )
                                        })
                                        .map(|sequence| format!("iges:entity:directory#{sequence}"))
                                })
                                .collect::<Vec<_>>()
                        };
                        let mut cursor = if entry.form == 18 { 10 } else { 9 };
                        let associated_flows = flow_links(cursor, counts[0]);
                        cursor += counts[0];
                        let connections = (0..counts[1])
                            .map(|offset| {
                                let index = cursor + offset;
                                record
                                    .and_then(|record| record.integer(index))
                                    .and_then(|sequence| {
                                        parameter_resolver.resolve(
                                            entry.sequence,
                                            index,
                                            sequence,
                                            if entry.form == 18 {
                                                "type-132-or-group"
                                            } else {
                                                "type-132"
                                            },
                                            |target| {
                                                target.entity_type == 132
                                                    || (entry.form == 18
                                                        && target.entity_type == 402
                                                        && matches!(target.form, 1 | 7 | 14 | 15))
                                            },
                                        )
                                    })
                                    .map(|sequence| format!("iges:entity:directory#{sequence}"))
                            })
                            .collect();
                        cursor += counts[1];
                        let joins = (0..counts[2])
                            .map(|offset| {
                                let index = cursor + offset;
                                record
                                    .and_then(|record| record.integer(index))
                                    .and_then(|sequence| {
                                        parameter_resolver.resolve(
                                            entry.sequence,
                                            index,
                                            sequence,
                                            "non-associativity-or-type-402-form-7",
                                            |target| target.entity_type != 402 || target.form == 7,
                                        )
                                    })
                                    .map(|sequence| format!("iges:entity:directory#{sequence}"))
                            })
                            .collect();
                        cursor += counts[2];
                        let names = (0..counts[3])
                            .map(|offset| {
                                record
                                    .and_then(|record| record.string(cursor + offset))
                                    .map(<[u8]>::to_vec)
                            })
                            .collect::<Vec<_>>();
                        cursor += counts[3];
                        let name_displays = (0..counts[4])
                            .map(|offset| {
                                let index = cursor + offset;
                                record
                                    .and_then(|record| record.integer(index))
                                    .and_then(|sequence| {
                                        parameter_resolver.resolve(
                                            entry.sequence,
                                            index,
                                            sequence,
                                            if entry.form == 18 {
                                                "type-312-or-type-212"
                                            } else {
                                                "type-312"
                                            },
                                            |target| {
                                                target.entity_type == 312
                                                    || (entry.form == 18
                                                        && target.entity_type == 212)
                                            },
                                        )
                                    })
                                    .map(|sequence| format!("iges:entity:directory#{sequence}"))
                            })
                            .collect();
                        cursor += counts[4];
                        let continuations = (0..counts[5])
                            .map(|offset| {
                                let index = cursor + offset;
                                record
                                    .and_then(|record| record.integer(index))
                                    .filter(|sequence| *sequence != 0)
                                    .and_then(|sequence| {
                                        parameter_resolver.resolve(
                                            entry.sequence,
                                            index,
                                            sequence,
                                            if entry.form == 18 {
                                                "type-402-form-11-or-18"
                                            } else {
                                                "type-402-form-20"
                                            },
                                            |target| {
                                                target.entity_type == 402
                                                    && if entry.form == 18 {
                                                        matches!(target.form, 11 | 18)
                                                    } else {
                                                        target.form == 20
                                                    }
                                            },
                                        )
                                    })
                                    .map(|sequence| format!("iges:entity:directory#{sequence}"))
                            })
                            .collect();
                        NativeAssociativity::Flow {
                            id,
                            source_entity,
                            form: entry.form,
                            declared_associated_flow_count: record
                                .and_then(|record| record.integer(2)),
                            declared_connection_count: record.and_then(|record| record.integer(3)),
                            declared_join_count: record.and_then(|record| record.integer(4)),
                            declared_name_count: record.and_then(|record| record.integer(5)),
                            declared_name_display_count: record
                                .and_then(|record| record.integer(6)),
                            declared_continuation_count: record
                                .and_then(|record| record.integer(7)),
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
                        let count = record
                            .and_then(|record| record.count(2))
                            .unwrap_or_default();
                        NativeAssociativity::RecalculableDimension {
                            id,
                            source_entity,
                            declared_geometry_count: record.and_then(|record| record.integer(2)),
                            dimension: record
                                .and_then(|record| record.integer(3))
                                .and_then(|sequence| {
                                    parameter_resolver.resolve(
                                        entry.sequence,
                                        3,
                                        sequence,
                                        "dimension-entity",
                                        |target| {
                                            matches!(
                                                target.entity_type,
                                                202 | 206 | 216 | 218 | 220 | 222
                                            )
                                        },
                                    )
                                })
                                .map(|sequence| format!("iges:entity:directory#{sequence}")),
                            orientation_flag: record.and_then(|record| record.integer(4)),
                            angle: record.and_then(|record| record.number(5)),
                            geometry: (0..count)
                                .map(|offset| {
                                    let start = 6 + offset * 5;
                                    NativeDimensionGeometryItem {
                                        geometry: entity_link(start),
                                        location_flag: record
                                            .and_then(|record| record.integer(start + 1)),
                                        point: [
                                            record.and_then(|record| record.number(start + 2)),
                                            record.and_then(|record| record.number(start + 3)),
                                            record.and_then(|record| record.number(start + 4)),
                                        ],
                                    }
                                })
                                .collect(),
                        }
                    }
                    _ => return None,
                })
            }),
    );
    let attribute_table_definitions = directory
        .iter()
        .filter(|entry| entry.entity_type == 322 && matches!(entry.form, 0..=2))
        .map(|entry| {
            let record = by_directory.get(&entry.sequence).copied();
            let count = record
                .and_then(|record| record.count(3))
                .unwrap_or_default();
            let mut cursor = 4;
            let mut attributes = Vec::with_capacity(count);
            for _ in 0..count {
                let Some(record) = record else { break };
                let attribute_type = record.integer(cursor);
                let value_data_type = record.integer(cursor + 1);
                let declared_value_count = record.integer(cursor + 2);
                let stride = if entry.form == 2 { 2 } else { 1 };
                let value_count = if entry.form == 0 {
                    Some(0)
                } else {
                    match record.tokens.get(cursor + 2).map(|token| &token.value) {
                        Some(TokenValue::Omitted) => {
                            (stride <= record.tokens.len().saturating_sub(cursor + 3)).then_some(1)
                        }
                        Some(TokenValue::Integer(_)) => {
                            record.count_with_stride(cursor + 2, stride)
                        }
                        None | Some(TokenValue::Real(_) | TokenValue::String(_)) => None,
                    }
                };
                let Some(value_start) = cursor.checked_add(3) else {
                    break;
                };
                let mut values = Vec::with_capacity(value_count.unwrap_or_default());
                if entry.form != 0 {
                    for offset in 0..value_count.unwrap_or_default() {
                        let value_index = value_start + offset * stride;
                        let value = record
                            .tokens
                            .get(value_index)
                            .map(token)
                            .map_or(NativeTokenValue::Omitted, |token| token.value);
                        let display_template = (entry.form == 2)
                            .then(|| record.integer(value_index + 1))
                            .flatten()
                            .filter(|sequence| *sequence != 0)
                            .and_then(|sequence| {
                                parameter_resolver.resolve_type(
                                    entry.sequence,
                                    value_index + 1,
                                    sequence,
                                    312,
                                    &[0, 1],
                                )
                            })
                            .map(|sequence| format!("iges:entity:directory#{sequence}"));
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
                let Some(value_count) = value_count else {
                    break;
                };
                let Some(next) = value_count
                    .checked_mul(stride)
                    .and_then(|width| value_start.checked_add(width))
                else {
                    break;
                };
                cursor = next;
            }
            NativeAttributeTableDefinition {
                id: format!("iges:product:attribute-definition#D{}", entry.sequence),
                source_entity: format!("iges:entity:directory#{}", entry.sequence),
                form: entry.form,
                name: record
                    .and_then(|record| record.string(1))
                    .map(<[u8]>::to_vec),
                attribute_list_type: record.and_then(|record| record.integer(2)),
                declared_attribute_count: record.and_then(|record| record.integer(3)),
                attributes,
            }
        })
        .collect::<Vec<_>>();
    let attribute_table_instances = directory
        .iter()
        .filter(|entry| entry.entity_type == 422 && matches!(entry.form, 0..=1))
        .map(|entry| {
            let record = by_directory.get(&entry.sequence).copied();
            let definition_sequence =
                crate::graph::resolved_structure_sequence(references, entry.sequence);
            let definition_record =
                definition_sequence.and_then(|sequence| by_directory.get(&sequence).copied());
            let attribute_count = definition_record
                .and_then(|record| record.count(3))
                .unwrap_or_default();
            let values_per_row = (0..attribute_count)
                .try_fold(0_usize, |total, index| {
                    let count_index = 6 + index * 3;
                    let count = match definition_record
                        .and_then(|record| record.tokens.get(count_index))
                        .map(|token| &token.value)
                    {
                        None | Some(TokenValue::Omitted) => 1,
                        Some(TokenValue::Integer(value)) => {
                            usize::try_from(*value).unwrap_or_default()
                        }
                        Some(TokenValue::Real(_) | TokenValue::String(_)) => 0,
                    };
                    total.checked_add(count)
                })
                .unwrap_or_default();
            let row_count = if entry.form == 0 {
                usize::from(values_per_row > 0)
            } else {
                record
                    .and_then(|record| record.count(1))
                    .unwrap_or_default()
            };
            let value_start = if entry.form == 0 { 1 } else { 2 };
            let row_count = record.map_or(0, |record| {
                let available = record.tokens.len().saturating_sub(value_start);
                if values_per_row == 0 || row_count > available / values_per_row {
                    0
                } else {
                    row_count
                }
            });
            let rows = (0..row_count)
                .map(|row| {
                    (0..values_per_row)
                        .map(|column| {
                            record
                                .and_then(|record| {
                                    record
                                        .tokens
                                        .get(value_start + row * values_per_row + column)
                                })
                                .map(token)
                                .map_or(NativeTokenValue::Omitted, |token| token.value)
                        })
                        .collect()
                })
                .collect();
            NativeAttributeTableInstance {
                id: format!("iges:product:attribute-instance#D{}", entry.sequence),
                source_entity: format!("iges:entity:directory#{}", entry.sequence),
                form: entry.form,
                definition: definition_sequence
                    .map(|sequence| format!("iges:product:attribute-definition#D{sequence}")),
                declared_row_count: (entry.form == 1)
                    .then(|| record.and_then(|record| record.integer(1)))
                    .flatten(),
                rows,
            }
        })
        .collect::<Vec<_>>();
    let product_properties = directory
        .iter()
        .filter(|entry| entry.entity_type == 406 && matches!(entry.form, 7 | 15))
        .map(|entry| {
            let record = by_directory.get(&entry.sequence).copied();
            NativeProductProperty {
                id: format!("iges:product:property#D{}", entry.sequence),
                source_entity: format!("iges:entity:directory#{}", entry.sequence),
                form: entry.form,
                property_kind: if entry.form == 7 {
                    "reference_designator".into()
                } else {
                    "name".into()
                },
                value: record
                    .and_then(|record| record.string(2))
                    .map(<[u8]>::to_vec),
                owners: by_directory
                    .iter()
                    .filter(|(sequence, _owner_record)| {
                        **sequence != entry.sequence
                            && trailing_by_directory
                                .get(sequence)
                                .and_then(|groups| groups.as_ref())
                                .is_some_and(|groups| groups.properties.contains(&entry.sequence))
                    })
                    .map(|(sequence, _)| format!("iges:entity:directory#{sequence}"))
                    .collect(),
            }
        })
        .collect::<Vec<_>>();
    let properties = directory
        .iter()
        .filter(|entry| entry.entity_type == 406 && matches!(entry.form, 2 | 3 | 5..=15 | 18..=36))
        .filter_map(|entry| {
            let record = by_directory.get(&entry.sequence).copied()?;
            let bounded_count = |index| {
                record
                    .integer(index)
                    .and_then(|value| usize::try_from(value).ok())
                    .filter(|count| *count <= record.tokens.len())
                    .unwrap_or_default()
            };
            let count = bounded_count(1);
            let strings = |start: usize, count: usize| {
                (0..count)
                    .map(|offset| record.string(start + offset).map(<[u8]>::to_vec))
                    .collect::<Vec<_>>()
            };
            let value = match entry.form {
                2 => NativePropertyValue::RegionRestriction {
                    electrical_vias: record.integer(2),
                    electrical_components: record.integer(3),
                    electrical_circuitry: record.integer(4),
                },
                3 => NativePropertyValue::LevelFunction {
                    function_code: record.integer(2),
                    description: record.string(3).map(<[u8]>::to_vec),
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
                7 => NativePropertyValue::ReferenceDesignator {
                    value: record.string(2).map(<[u8]>::to_vec),
                },
                8 => NativePropertyValue::PinNumber {
                    value: record.string(2).map(<[u8]>::to_vec),
                },
                9 => NativePropertyValue::PartNumber {
                    generic: record.string(2).map(<[u8]>::to_vec),
                    military: record.string(3).map(<[u8]>::to_vec),
                    vendor: record.string(4).map(<[u8]>::to_vec),
                    internal: record.string(5).map(<[u8]>::to_vec),
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
                    let dependent_count = bounded_count(3);
                    let independent_count = bounded_count(4);
                    let counts = (0..independent_count)
                        .map(|offset| bounded_count(5 + independent_count + offset))
                        .collect::<Vec<_>>();
                    let mut cursor = 5 + independent_count * 2;
                    let independent_variables = counts
                        .iter()
                        .enumerate()
                        .map(|(offset, count)| {
                            let values = (0..*count)
                                .map(|index| record.number(cursor + index))
                                .collect();
                            cursor += count;
                            NativeIndependentVariable {
                                variable_type: record.integer(5 + offset),
                                declared_value_count: record
                                    .integer(5 + independent_count + offset),
                                values,
                            }
                        })
                        .collect();
                    let point_count = counts
                        .iter()
                        .try_fold(1_usize, |product, count| product.checked_mul(*count))
                        .filter(|count| *count <= record.tokens.len())
                        .unwrap_or_default()
                        .max(1);
                    let dependent_value_count = dependent_count
                        .checked_mul(point_count)
                        .filter(|count| *count <= record.tokens.len())
                        .unwrap_or_default();
                    NativePropertyValue::TabularData {
                        property_type: record.integer(2),
                        declared_dependent_count: record.integer(3),
                        independent_variables,
                        dependent_values: (0..dependent_value_count)
                            .map(|offset| record.number(cursor + offset))
                            .collect(),
                    }
                }
                12 => NativePropertyValue::ExternalReferenceFileList {
                    names: strings(2, count),
                },
                13 => NativePropertyValue::NominalSize {
                    size: record.number(2),
                    name: record.string(3).map(<[u8]>::to_vec),
                    standard: record.string(4).map(<[u8]>::to_vec),
                },
                14 => NativePropertyValue::FlowLineSpecification {
                    values: strings(2, count),
                },
                15 => NativePropertyValue::Name {
                    value: record.string(2).map(<[u8]>::to_vec),
                },
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
                    name: record.string(3).map(<[u8]>::to_vec),
                },
                24 => {
                    let definition_count = bounded_count(2);
                    NativePropertyValue::LevelToLepLayerMap {
                        definitions: (0..definition_count)
                            .map(|offset| {
                                let start = 3 + offset * 4;
                                NativeLepLayerDefinition {
                                    exchange_level: record.integer(start),
                                    native_identifier: record.string(start + 1).map(<[u8]>::to_vec),
                                    physical_layer: record.integer(start + 2),
                                    functional_identifier: record
                                        .string(start + 3)
                                        .map(<[u8]>::to_vec),
                                }
                            })
                            .collect(),
                    }
                }
                25 => {
                    let level_count = bounded_count(3);
                    NativePropertyValue::LepArtworkStackup {
                        identification: record.string(2).map(<[u8]>::to_vec),
                        levels: (0..level_count)
                            .map(|offset| record.integer(4 + offset))
                            .collect(),
                    }
                }
                26 => NativePropertyValue::LepDrilledHole {
                    drill_diameter: record.number(2),
                    finished_diameter: record.number(3),
                    function_code: record.integer(4),
                },
                27 => {
                    let value_count = bounded_count(3);
                    NativePropertyValue::GenericData {
                        name: record.string(2).map(<[u8]>::to_vec),
                        values: (0..value_count)
                            .map(|offset| {
                                let index = 4 + offset * 2;
                                NativeGenericPropertyValue {
                                    data_type: record.integer(index),
                                    value: record
                                        .tokens
                                        .get(index + 1)
                                        .map(token)
                                        .map_or(NativeTokenValue::Omitted, |token| token.value),
                                }
                            })
                            .collect(),
                    }
                }
                28 => NativePropertyValue::DimensionUnits {
                    secondary_position: record.integer(2),
                    units_indicator: record.integer(3),
                    character_set: record.integer(4),
                    suffix: record.string(5).map(<[u8]>::to_vec),
                    fraction_flag: record.integer(6),
                    precision: record.integer(7),
                },
                29 => NativePropertyValue::DimensionTolerance {
                    secondary_flag: record.integer(2),
                    tolerance_type: record.integer(3),
                    placement: record.integer(4),
                    upper: record.number(5),
                    lower: record.number(6),
                    suppress_plus: binary_integer(record.integer(7)),
                    fraction_flag: record.integer(8),
                    precision: record.integer(9),
                },
                30 => {
                    let note_count = bounded_count(13);
                    NativePropertyValue::DimensionDisplayData {
                        dimension_type: record.integer(2),
                        label_position: record.integer(3),
                        declared_character_set: record.integer(4),
                        character_set: record
                            .integer_or(4, DEFAULT_DIMENSION_DISPLAY_CHARACTER_SET),
                        label: record.string(5).map(<[u8]>::to_vec),
                        decimal_symbol: record.integer(6),
                        declared_witness_line_angle: record.number(7),
                        witness_line_angle: record
                            .number_or(7, DEFAULT_DIMENSION_DISPLAY_WITNESS_LINE_ANGLE_RAD),
                        text_alignment: record.integer(8),
                        text_level: record.integer(9),
                        text_placement: record.integer(10),
                        arrow_orientation: record.integer(11),
                        initial_value: record.number(12),
                        supplemental_notes: (0..note_count)
                            .map(|offset| {
                                let start = 14 + offset * 3;
                                NativeSupplementalNote {
                                    position: record.integer(start),
                                    first_text: record.integer(start + 1),
                                    last_text: record.integer(start + 2),
                                }
                            })
                            .collect(),
                    }
                }
                31 => NativePropertyValue::BasicDimension {
                    corners: (0..4)
                        .map(|offset| {
                            [record.number(2 + offset * 2), record.number(3 + offset * 2)]
                        })
                        .collect(),
                },
                32 => NativePropertyValue::DrawingSheetApproval {
                    name: record.string(2).map(<[u8]>::to_vec),
                    organization: record.string(3).map(<[u8]>::to_vec),
                    date: record.string(4).map(<[u8]>::to_vec),
                },
                33 => NativePropertyValue::DrawingSheetId {
                    sheet_number: record.integer(2),
                    revision: record.string(3).map(<[u8]>::to_vec),
                },
                34 | 35 => {
                    let range_count = bounded_count(2);
                    let ranges = (0..range_count)
                        .map(|offset| {
                            let start = 3 + offset * 3;
                            NativeTextScoreRange {
                                text_index: record.integer(start),
                                first_character: record.integer(start + 1),
                                last_character: record.integer(start + 2),
                            }
                        })
                        .collect();
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
                _ => return None,
            };
            Some(NativeProperty {
                id: format!("iges:application:property#D{}", entry.sequence),
                source_entity: format!("iges:entity:directory#{}", entry.sequence),
                form: entry.form,
                declared_value_count: record.integer(1),
                owners: by_directory
                    .iter()
                    .filter(|(sequence, _owner)| {
                        **sequence != entry.sequence
                            && trailing_by_directory
                                .get(sequence)
                                .and_then(|groups| groups.as_ref())
                                .is_some_and(|groups| groups.properties.contains(&entry.sequence))
                    })
                    .map(|(sequence, _)| format!("iges:entity:directory#{sequence}"))
                    .collect(),
                value,
            })
        })
        .collect::<Vec<_>>();
    let units_data = directory
        .iter()
        .filter(|entry| entry.entity_type == 316 && entry.form == 0)
        .map(|entry| {
            let record = by_directory.get(&entry.sequence).copied();
            let count = record
                .and_then(|record| record.count(1))
                .unwrap_or_default();
            let owners = by_directory
                .iter()
                .filter(|(sequence, _owner)| {
                    trailing_by_directory
                        .get(sequence)
                        .and_then(|groups| groups.as_ref())
                        .is_some_and(|groups| groups.properties.contains(&entry.sequence))
                })
                .map(|(sequence, _)| format!("iges:entity:directory#{sequence}"))
                .collect();
            NativeUnitsData {
                id: format!("iges:metadata:units-data#D{}", entry.sequence),
                source_entity: format!("iges:entity:directory#{}", entry.sequence),
                declared_count: record.and_then(|record| record.integer(1)),
                units: (0..count)
                    .map(|offset| {
                        let start = 2 + offset * 3;
                        NativeUnitDefinition {
                            unit_type: record
                                .and_then(|record| record.string(start))
                                .map(<[u8]>::to_vec),
                            unit_value: record
                                .and_then(|record| record.string(start + 1))
                                .map(<[u8]>::to_vec),
                            scale_factor: record.and_then(|record| record.number(start + 2)),
                        }
                    })
                    .collect(),
                owners,
            }
        })
        .collect::<Vec<_>>();
    let views = directory
        .iter()
        .filter(|entry| entry.entity_type == 410 && matches!(entry.form, 0 | 1))
        .map(|entry| {
            let record = by_directory.get(&entry.sequence).copied();
            let vector = |start| {
                [
                    record.and_then(|record| record.number(start)),
                    record.and_then(|record| record.number(start + 1)),
                    record.and_then(|record| record.number(start + 2)),
                ]
            };
            NativeView {
                id: format!("iges:presentation:view#D{}", entry.sequence),
                source_entity: format!("iges:entity:directory#{}", entry.sequence),
                form: entry.form,
                projection: if entry.form == 0 {
                    "orthographic_parallel".into()
                } else {
                    "perspective".into()
                },
                view_number: record.and_then(|record| record.integer(1)),
                scale: record.and_then(|record| record.number(2)),
                model_to_view: (entry.form == 0 && entry.transform > 0)
                    .then(|| format!("iges:native:transformation#D{}", entry.transform)),
                clipping_planes: if entry.form == 0 {
                    (3..=8)
                        .map(|index| {
                            record
                                .and_then(|record| record.integer(index))
                                .filter(|sequence| *sequence != 0)
                                .and_then(|sequence| {
                                    parameter_resolver.resolve_type(
                                        entry.sequence,
                                        index,
                                        sequence,
                                        108,
                                        &[],
                                    )
                                })
                                .map(|sequence| format!("iges:entity:directory#{sequence}"))
                        })
                        .collect()
                } else {
                    Vec::new()
                },
                view_plane_normal: (entry.form == 1).then(|| vector(3)),
                view_reference_point: (entry.form == 1).then(|| vector(6)),
                center_of_projection: (entry.form == 1).then(|| vector(9)),
                view_up: (entry.form == 1).then(|| vector(12)),
                view_plane_distance: (entry.form == 1)
                    .then(|| record.and_then(|record| record.number(15)))
                    .flatten(),
                clipping_window: (entry.form == 1).then(|| {
                    [
                        record.and_then(|record| record.number(16)),
                        record.and_then(|record| record.number(17)),
                        record.and_then(|record| record.number(18)),
                        record.and_then(|record| record.number(19)),
                    ]
                }),
                depth_clipping: (entry.form == 1)
                    .then(|| record.and_then(|record| record.integer(20)))
                    .flatten(),
                depth_range: (entry.form == 1).then(|| {
                    [
                        record.and_then(|record| record.number(21)),
                        record.and_then(|record| record.number(22)),
                    ]
                }),
            }
        })
        .collect::<Vec<_>>();
    let view_visibility = directory
        .iter()
        .filter(|entry| entry.entity_type == 402 && matches!(entry.form, 3 | 4))
        .map(|entry| {
            let record = by_directory.get(&entry.sequence).copied();
            let view_count = record
                .and_then(|record| record.count(1))
                .unwrap_or_default();
            let entity_count = record
                .and_then(|record| record.count(2))
                .unwrap_or_default();
            let width = if entry.form == 3 { 1 } else { 5 };
            NativeViewVisibility {
                id: format!("iges:presentation:view-visibility#D{}", entry.sequence),
                source_entity: format!("iges:entity:directory#{}", entry.sequence),
                form: entry.form,
                displays: (0..view_count)
                    .map(|index| {
                        let start = 3 + index * width;
                        if let Some(color) = (entry.form == 4)
                            .then(|| record.and_then(|record| record.integer(start + 3)))
                            .flatten()
                            .filter(|value| *value < 0)
                        {
                            let _ = parameter_resolver.resolve_negative(
                                entry.sequence,
                                start + 3,
                                color,
                                "type-314-form-0",
                                |target| target.entity_type == 314 && target.form == 0,
                            );
                        }
                        NativeViewDisplay {
                            view: record
                                .and_then(|record| record.integer(start))
                                .and_then(|sequence| {
                                    parameter_resolver.resolve_type(
                                        entry.sequence,
                                        start,
                                        sequence,
                                        410,
                                        &[0, 1],
                                    )
                                })
                                .map(|sequence| format!("iges:presentation:view#D{sequence}")),
                            line_font: (entry.form == 4)
                                .then(|| record.and_then(|record| record.integer(start + 1)))
                                .flatten(),
                            line_font_definition: (entry.form == 4)
                                .then(|| record.and_then(|record| record.integer(start + 2)))
                                .flatten()
                                .filter(|sequence| *sequence != 0)
                                .and_then(|sequence| {
                                    parameter_resolver.resolve_type(
                                        entry.sequence,
                                        start + 2,
                                        sequence,
                                        304,
                                        &[1, 2],
                                    )
                                })
                                .map(|sequence| format!("iges:presentation:line-font#D{sequence}")),
                            color: (entry.form == 4)
                                .then(|| record.and_then(|record| record.integer(start + 3)))
                                .flatten(),
                            line_weight: (entry.form == 4)
                                .then(|| record.and_then(|record| record.integer(start + 4)))
                                .flatten(),
                        }
                    })
                    .collect(),
                entities: (0..entity_count)
                    .map(|index| {
                        record
                            .and_then(|record| record.integer(3 + view_count * width + index))
                            .and_then(|sequence| {
                                parameter_resolver.resolve_any(
                                    entry.sequence,
                                    3 + view_count * width + index,
                                    sequence,
                                )
                            })
                            .map(|sequence| format!("iges:entity:directory#{sequence}"))
                    })
                    .collect(),
            }
        })
        .collect::<Vec<_>>();
    let segmented_visibility = directory
        .iter()
        .filter(|entry| entry.entity_type == 402 && entry.form == 19)
        .map(|entry| {
            let record = by_directory.get(&entry.sequence).copied();
            let count = record
                .and_then(|record| record.count(1))
                .unwrap_or_default();
            let value = |index| {
                record
                    .and_then(|record| record.tokens.get(index))
                    .map(token)
                    .map_or(NativeTokenValue::Omitted, |token| token.value)
            };
            NativeSegmentedVisibility {
                id: format!("iges:presentation:segmented-visibility#D{}", entry.sequence),
                source_entity: format!("iges:entity:directory#{}", entry.sequence),
                blocks: (0..count)
                    .map(|index| {
                        let start = 2 + index * 6;
                        if let Some(color) = record
                            .and_then(|record| record.integer(start + 3))
                            .filter(|value| *value < 0)
                        {
                            let _ = parameter_resolver.resolve_negative(
                                entry.sequence,
                                start + 3,
                                color,
                                "type-314-form-0",
                                |target| target.entity_type == 314 && target.form == 0,
                            );
                        }
                        if let Some(line_font) = record
                            .and_then(|record| record.integer(start + 4))
                            .filter(|value| *value < 0)
                        {
                            let _ = parameter_resolver.resolve_negative(
                                entry.sequence,
                                start + 4,
                                line_font,
                                "type-304-form-1-or-2",
                                |target| target.entity_type == 304 && matches!(target.form, 1 | 2),
                            );
                        }
                        NativeSegmentDisplay {
                            view: record
                                .and_then(|record| record.integer(start))
                                .and_then(|sequence| {
                                    parameter_resolver.resolve_type(
                                        entry.sequence,
                                        start,
                                        sequence,
                                        410,
                                        &[0, 1],
                                    )
                                })
                                .map(|sequence| format!("iges:presentation:view#D{sequence}")),
                            breakpoint: record.and_then(|record| record.number(start + 1)),
                            display_flag: record.and_then(|record| record.integer(start + 2)),
                            color: value(start + 3),
                            line_font: value(start + 4),
                            line_weight: value(start + 5),
                        }
                    })
                    .collect(),
            }
        })
        .collect::<Vec<_>>();
    let drawings = directory
        .iter()
        .filter(|entry| entry.entity_type == 404 && matches!(entry.form, 0 | 1))
        .map(|entry| {
            let record = by_directory.get(&entry.sequence).copied();
            let view_count = record
                .and_then(|record| record.count(1))
                .unwrap_or_default();
            let width = if entry.form == 0 { 3 } else { 4 };
            let annotation_count_index = 2 + view_count * width;
            let annotation_count = record
                .and_then(|record| record.count(annotation_count_index))
                .unwrap_or_default();
            let trailing = trailing_by_directory
                .get(&entry.sequence)
                .and_then(|groups| groups.as_ref());
            let candidates = |form| drawing_property_candidates(trailing, form, &entries);
            let (name_property, name_ambiguous) =
                choose_drawing_property(&candidates(15), 15, &by_directory);
            let (size_property, size_ambiguous) =
                choose_drawing_property(&candidates(16), 16, &by_directory);
            let (units_property, units_ambiguous) =
                choose_drawing_property(&candidates(17), 17, &by_directory);
            let ambiguous_property_forms = [
                (15, name_ambiguous),
                (16, size_ambiguous),
                (17, units_ambiguous),
            ]
            .into_iter()
            .filter_map(|(form, ambiguous)| ambiguous.then_some(form))
            .collect();
            NativeDrawing {
                id: format!("iges:presentation:drawing#D{}", entry.sequence),
                source_entity: format!("iges:entity:directory#{}", entry.sequence),
                form: entry.form,
                views: (0..view_count)
                    .map(|index| {
                        let start = 2 + index * width;
                        NativeDrawingView {
                            view: record
                                .and_then(|record| record.integer(start))
                                .and_then(|sequence| {
                                    parameter_resolver.resolve_type(
                                        entry.sequence,
                                        start,
                                        sequence,
                                        410,
                                        &[0, 1],
                                    )
                                })
                                .map(|sequence| format!("iges:presentation:view#D{sequence}")),
                            origin: [
                                record.and_then(|record| record.number(start + 1)),
                                record.and_then(|record| record.number(start + 2)),
                            ],
                            rotation: (entry.form == 1)
                                .then(|| record.and_then(|record| record.number(start + 3)))
                                .flatten(),
                        }
                    })
                    .collect(),
                annotations: (0..annotation_count)
                    .map(|index| {
                        record
                            .and_then(|record| record.integer(annotation_count_index + 1 + index))
                            .and_then(|sequence| {
                                parameter_resolver.resolve(
                                    entry.sequence,
                                    annotation_count_index + 1 + index,
                                    sequence,
                                    "drawing-space-annotation",
                                    |target| {
                                        target.status.use_flag == 1
                                            && target.status.is_physically_dependent()
                                    },
                                )
                            })
                            .map(|sequence| format!("iges:entity:directory#{sequence}"))
                    })
                    .collect(),
                name_property: name_property
                    .map(|sequence| format!("iges:product:property#D{sequence}")),
                name: name_property
                    .and_then(|sequence| by_directory.get(&sequence))
                    .and_then(|record| record.string(2))
                    .map(<[u8]>::to_vec),
                size: size_property.and_then(|sequence| {
                    let record = by_directory.get(&sequence)?;
                    Some([record.number(2), record.number(3)])
                }),
                units_flag: units_property
                    .and_then(|sequence| by_directory.get(&sequence))
                    .and_then(|record| record.integer(2)),
                units_name: units_property
                    .and_then(|sequence| by_directory.get(&sequence))
                    .and_then(|record| record.string(3))
                    .map(<[u8]>::to_vec),
                ambiguous_property_forms,
            }
        })
        .collect::<Vec<_>>();
    let text_run = |source: u32, record: Option<&ParameterRecord>, start: usize| {
        let font_code = record.and_then(|record| record.integer(start + 3));
        NativeTextRun {
            declared_character_count: record.and_then(|record| record.integer(start)),
            text: record
                .and_then(|record| record.string(start + 11))
                .map(<[u8]>::to_vec),
            box_size: [
                record.and_then(|record| record.number(start + 1)),
                record.and_then(|record| record.number(start + 2)),
            ],
            font_code,
            font_definition: font_code
                .filter(|value| *value < 0)
                .and_then(|value| {
                    parameter_resolver.resolve_negative(
                        source,
                        start + 3,
                        value,
                        "type-310-form-0",
                        |target| target.entity_type == 310 && target.form == 0,
                    )
                })
                .map(|sequence| format!("iges:presentation:text-font#D{sequence}")),
            slant_angle: record.and_then(|record| record.number(start + 4)),
            rotation_angle: record.and_then(|record| record.number(start + 5)),
            mirror: record.and_then(|record| record.integer(start + 6)),
            vertical: record.and_then(|record| record.integer(start + 7)),
            start: [
                record.and_then(|record| record.number(start + 8)),
                record.and_then(|record| record.number(start + 9)),
                record.and_then(|record| record.number(start + 10)),
            ],
        }
    };
    let annotations = directory
        .iter()
        .filter(|entry| {
            (matches!(entry.entity_type, 202 | 204 | 206 | 208 | 210 | 212 | 213)
                && entry.form == 0)
                || (entry.entity_type == 214 && matches!(entry.form, 1..=12))
                || matches!(
                    (entry.entity_type, entry.form),
                    (216, 0..=2) | (218 | 222, 0..=1) | (220, 0)
                )
                || matches!((entry.entity_type, entry.form), (228 | 230, 0))
        })
        .filter_map(|entry| {
            let record = by_directory.get(&entry.sequence).copied();
            let transformation = (entry.transform > 0)
                .then(|| format!("iges:native:transformation#D{}", entry.transform));
            Some(if entry.entity_type == 212 {
                let count = record
                    .and_then(|record| record.count(1))
                    .unwrap_or_default();
                NativeAnnotation::GeneralNote {
                    id: format!("iges:presentation:annotation#D{}", entry.sequence),
                    source_entity: format!("iges:entity:directory#{}", entry.sequence),
                    declared_string_count: record.and_then(|record| record.integer(1)),
                    strings: (0..count)
                        .map(|index| text_run(entry.sequence, record, 2 + index * 12))
                        .collect(),
                    transformation,
                }
            } else if entry.entity_type == 213 {
                let count = record
                    .and_then(|record| record.count(12))
                    .unwrap_or_default();
                NativeAnnotation::NewGeneralNote {
                    id: format!("iges:presentation:annotation#D{}", entry.sequence),
                    source_entity: format!("iges:entity:directory#{}", entry.sequence),
                    containment_size: [
                        record.and_then(|record| record.number(1)),
                        record.and_then(|record| record.number(2)),
                    ],
                    justification: record.and_then(|record| record.integer(3)),
                    containment_origin: [
                        record.and_then(|record| record.number(4)),
                        record.and_then(|record| record.number(5)),
                        record.and_then(|record| record.number(6)),
                    ],
                    containment_angle: record.and_then(|record| record.number(7)),
                    baseline_origin: [
                        record.and_then(|record| record.number(8)),
                        record.and_then(|record| record.number(9)),
                        record.and_then(|record| record.number(10)),
                    ],
                    normal_interline_spacing: record.and_then(|record| record.number(11)),
                    declared_string_count: record.and_then(|record| record.integer(12)),
                    strings: (0..count)
                        .map(|index| {
                            let start = 13 + index * 20;
                            let font_code = record.and_then(|record| record.integer(start + 11));
                            NativeNewTextRun {
                                fixed_or_variable: record.and_then(|record| record.integer(start)),
                                character_size: [
                                    record.and_then(|record| record.number(start + 1)),
                                    record.and_then(|record| record.number(start + 2)),
                                ],
                                character_spacing: record
                                    .and_then(|record| record.number(start + 3)),
                                line_spacing: record.and_then(|record| record.number(start + 4)),
                                font_style: record.and_then(|record| record.integer(start + 5)),
                                character_angle: record.and_then(|record| record.number(start + 6)),
                                control_codes: record
                                    .and_then(|record| record.string(start + 7))
                                    .map(<[u8]>::to_vec),
                                text: NativeTextRun {
                                    declared_character_count: record
                                        .and_then(|record| record.integer(start + 8)),
                                    text: record
                                        .and_then(|record| record.string(start + 19))
                                        .map(<[u8]>::to_vec),
                                    box_size: [
                                        record.and_then(|record| record.number(start + 9)),
                                        record.and_then(|record| record.number(start + 10)),
                                    ],
                                    font_code,
                                    font_definition: font_code
                                        .filter(|value| *value < 0)
                                        .and_then(|value| {
                                            parameter_resolver.resolve_negative(
                                                entry.sequence,
                                                start + 11,
                                                value,
                                                "type-310-form-0",
                                                |target| {
                                                    target.entity_type == 310 && target.form == 0
                                                },
                                            )
                                        })
                                        .map(|sequence| {
                                            format!("iges:presentation:text-font#D{sequence}")
                                        }),
                                    slant_angle: record
                                        .and_then(|record| record.number(start + 12)),
                                    rotation_angle: record
                                        .and_then(|record| record.number(start + 13)),
                                    mirror: record.and_then(|record| record.integer(start + 14)),
                                    vertical: record.and_then(|record| record.integer(start + 15)),
                                    start: [
                                        record.and_then(|record| record.number(start + 16)),
                                        record.and_then(|record| record.number(start + 17)),
                                        record.and_then(|record| record.number(start + 18)),
                                    ],
                                },
                            }
                        })
                        .collect(),
                    transformation,
                }
            } else if entry.entity_type == 214 {
                let count = record
                    .and_then(|record| record.count(1))
                    .unwrap_or_default();
                let z = record.and_then(|record| record.number(4));
                NativeAnnotation::Leader {
                    id: format!("iges:presentation:annotation#D{}", entry.sequence),
                    source_entity: format!("iges:entity:directory#{}", entry.sequence),
                    form: entry.form,
                    declared_segment_count: record.and_then(|record| record.integer(1)),
                    arrowhead_size: [
                        record.and_then(|record| record.number(2)),
                        record.and_then(|record| record.number(3)),
                    ],
                    arrowhead: [
                        record.and_then(|record| record.number(5)),
                        record.and_then(|record| record.number(6)),
                        z,
                    ],
                    segment_tails: (0..count)
                        .map(|index| {
                            [
                                record.and_then(|record| record.number(7 + index * 2)),
                                record.and_then(|record| record.number(8 + index * 2)),
                                z,
                            ]
                        })
                        .collect(),
                    transformation,
                }
            } else {
                let note_link = |index| {
                    record
                        .and_then(|record| record.integer(index))
                        .filter(|sequence| *sequence != 0)
                        .and_then(|sequence| {
                            parameter_resolver.resolve_type(
                                entry.sequence,
                                index,
                                sequence,
                                212,
                                &[0],
                            )
                        })
                        .map(|sequence| format!("iges:presentation:annotation#D{sequence}"))
                };
                let leader_link = |index| {
                    record
                        .and_then(|record| record.integer(index))
                        .filter(|sequence| *sequence != 0)
                        .and_then(|sequence| {
                            parameter_resolver.resolve(
                                entry.sequence,
                                index,
                                sequence,
                                "type-214-form-1-through-12",
                                |target| target.entity_type == 214 && matches!(target.form, 1..=12),
                            )
                        })
                        .map(|sequence| format!("iges:presentation:annotation#D{sequence}"))
                };
                let witness_link = |index| {
                    record
                        .and_then(|record| record.integer(index))
                        .filter(|sequence| *sequence != 0)
                        .and_then(|sequence| {
                            parameter_resolver.resolve_type(
                                entry.sequence,
                                index,
                                sequence,
                                106,
                                &[40],
                            )
                        })
                        .map(|sequence| format!("iges:entity:directory#{sequence}"))
                };
                let curve_link = |index| {
                    record
                        .and_then(|record| record.integer(index))
                        .filter(|sequence| *sequence != 0)
                        .and_then(|sequence| {
                            parameter_resolver.resolve(
                                entry.sequence,
                                index,
                                sequence,
                                "parameterized-curve",
                                |target| {
                                    parameterized_curve_type(target)
                                        && target.status.is_physically_dependent()
                                        && target.status.use_flag == 1
                                },
                            )
                        })
                        .map(|sequence| format!("iges:entity:directory#{sequence}"))
                };
                let ordinate_link = |index| {
                    record
                        .and_then(|record| record.integer(index))
                        .filter(|sequence| *sequence != 0)
                        .and_then(|sequence| {
                            parameter_resolver.resolve(
                                entry.sequence,
                                index,
                                sequence,
                                "type-106-form-40-or-leader",
                                |target| {
                                    (target.entity_type == 106 && target.form == 40)
                                        || (target.entity_type == 214
                                            && matches!(target.form, 1..=12))
                                },
                            )
                        })
                        .map(|sequence| {
                            entries
                                .get(&sequence)
                                .filter(|target| target.entity_type == 214)
                                .map_or_else(
                                    || format!("iges:entity:directory#{sequence}"),
                                    |_| format!("iges:presentation:annotation#D{sequence}"),
                                )
                        })
                };
                let id = format!("iges:presentation:annotation#D{}", entry.sequence);
                let source_entity = format!("iges:entity:directory#{}", entry.sequence);
                match entry.entity_type {
                    202 => NativeAnnotation::AngularDimension {
                        id,
                        source_entity,
                        note: note_link(1),
                        witnesses: [witness_link(2), witness_link(3)],
                        vertex: [
                            record.and_then(|record| record.number(4)),
                            record.and_then(|record| record.number(5)),
                        ],
                        radius: record.and_then(|record| record.number(6)),
                        leaders: [leader_link(7), leader_link(8)],
                        transformation,
                    },
                    204 => NativeAnnotation::CurveDimension {
                        id,
                        source_entity,
                        note: note_link(1),
                        curves: [curve_link(2), curve_link(3)],
                        leaders: [leader_link(4), leader_link(5)],
                        witnesses: [witness_link(6), witness_link(7)],
                        transformation,
                    },
                    206 => NativeAnnotation::DiameterDimension {
                        id,
                        source_entity,
                        note: note_link(1),
                        leaders: [leader_link(2), leader_link(3)],
                        center: [
                            record.and_then(|record| record.number(4)),
                            record.and_then(|record| record.number(5)),
                        ],
                        transformation,
                    },
                    208 | 210 => {
                        let (note_index, count_index, leader_start) = if entry.entity_type == 208 {
                            (5, 6, 7)
                        } else {
                            (1, 2, 3)
                        };
                        let leader_count = record
                            .and_then(|record| record.count(count_index))
                            .unwrap_or_default();
                        let leaders = (0..leader_count)
                            .map(|offset| leader_link(leader_start + offset))
                            .collect();
                        if entry.entity_type == 208 {
                            NativeAnnotation::FlagNote {
                                id,
                                source_entity,
                                origin: [
                                    record.and_then(|record| record.number(1)),
                                    record.and_then(|record| record.number(2)),
                                    record.and_then(|record| record.number(3)),
                                ],
                                rotation: record.and_then(|record| record.number(4)),
                                note: note_link(note_index),
                                declared_leader_count: record
                                    .and_then(|record| record.integer(count_index)),
                                leaders,
                                transformation,
                            }
                        } else {
                            NativeAnnotation::GeneralLabel {
                                id,
                                source_entity,
                                note: note_link(note_index),
                                declared_leader_count: record
                                    .and_then(|record| record.integer(count_index)),
                                leaders,
                                transformation,
                            }
                        }
                    }
                    216 => NativeAnnotation::LinearDimension {
                        id,
                        source_entity,
                        form: entry.form,
                        note: note_link(1),
                        leaders: [leader_link(2), leader_link(3)],
                        witnesses: [witness_link(4), witness_link(5)],
                        transformation,
                    },
                    218 => NativeAnnotation::OrdinateDimension {
                        id,
                        source_entity,
                        form: entry.form,
                        note: note_link(1),
                        ordinate: ordinate_link(2),
                        supplemental_leader: (entry.form == 1).then(|| leader_link(3)).flatten(),
                        transformation,
                    },
                    220 => NativeAnnotation::PointDimension {
                        id,
                        source_entity,
                        note: note_link(1),
                        leader: leader_link(2),
                        enclosure: record
                            .and_then(|record| record.integer(3))
                            .filter(|sequence| *sequence != 0)
                            .and_then(|sequence| {
                                parameter_resolver.resolve(
                                    entry.sequence,
                                    3,
                                    sequence,
                                    "point-dimension-enclosure",
                                    |target| {
                                        matches!(
                                            (target.entity_type, target.form),
                                            (100 | 102, 0) | (106, 63)
                                        ) && target.status.is_physically_dependent()
                                            && target.status.use_flag == 1
                                    },
                                )
                            })
                            .map(|sequence| format!("iges:entity:directory#{sequence}")),
                        transformation,
                    },
                    222 => NativeAnnotation::RadiusDimension {
                        id,
                        source_entity,
                        form: entry.form,
                        note: note_link(1),
                        leaders: [
                            leader_link(2),
                            (entry.form == 1).then(|| leader_link(5)).flatten(),
                        ],
                        center: [
                            record.and_then(|record| record.number(3)),
                            record.and_then(|record| record.number(4)),
                        ],
                        transformation,
                    },
                    228 => {
                        let geometry_count = record
                            .and_then(|record| record.count(2))
                            .unwrap_or_default();
                        let leader_count_index = 3 + geometry_count;
                        let leader_count = record
                            .and_then(|record| record.count(leader_count_index))
                            .unwrap_or_default();
                        NativeAnnotation::GeneralSymbol {
                            id,
                            source_entity,
                            note: note_link(1),
                            geometry: (0..geometry_count)
                                .map(|offset| {
                                    let index = 3 + offset;
                                    record
                                        .and_then(|record| record.integer(index))
                                        .and_then(|sequence| {
                                            parameter_resolver.resolve(
                                                entry.sequence,
                                                index,
                                                sequence,
                                                "subordinate-annotation-geometry",
                                                |target| {
                                                    target.status.is_physically_dependent()
                                                        && target.status.use_flag == 1
                                                },
                                            )
                                        })
                                        .map(|sequence| format!("iges:entity:directory#{sequence}"))
                                })
                                .collect(),
                            leaders: (0..leader_count)
                                .map(|offset| leader_link(leader_count_index + 1 + offset))
                                .collect(),
                            transformation,
                        }
                    }
                    230 => {
                        let island_count = record
                            .and_then(|record| record.count(8))
                            .unwrap_or_default();
                        NativeAnnotation::SectionedArea {
                            id,
                            source_entity,
                            boundary: record
                                .and_then(|record| record.integer(1))
                                .and_then(|sequence| {
                                    parameter_resolver.resolve(
                                        entry.sequence,
                                        1,
                                        sequence,
                                        "section-boundary-entity",
                                        section_boundary_type,
                                    )
                                })
                                .map(|sequence| format!("iges:entity:directory#{sequence}")),
                            fill_pattern: record.and_then(|record| record.integer(2)),
                            pattern_anchor: [
                                record.and_then(|record| record.number(3)),
                                record.and_then(|record| record.number(4)),
                                record.and_then(|record| record.number(5)),
                            ],
                            pattern_spacing: record.and_then(|record| record.number(6)),
                            pattern_angle: record.and_then(|record| record.number(7)),
                            islands: (0..island_count)
                                .map(|offset| {
                                    let index = 9 + offset;
                                    record
                                        .and_then(|record| record.integer(index))
                                        .and_then(|sequence| {
                                            parameter_resolver.resolve(
                                                entry.sequence,
                                                index,
                                                sequence,
                                                "section-boundary-entity",
                                                section_boundary_type,
                                            )
                                        })
                                        .map(|sequence| format!("iges:entity:directory#{sequence}"))
                                })
                                .collect(),
                            transformation,
                        }
                    }
                    _ => return None,
                }
            })
        })
        .collect::<Vec<_>>();
    let mut malformed_definition_sequences = Vec::new();
    let occurrence_definitions = directory
        .iter()
        .filter(|entry| matches!(entry.entity_type, 308 | 320) && entry.form == 0)
        .filter_map(|entry| {
            let Some(record) = by_directory.get(&entry.sequence).copied() else {
                malformed_definition_sequences.push(entry.sequence);
                return None;
            };
            let Some(count) = record.count(3) else {
                malformed_definition_sequences.push(entry.sequence);
                return Some((
                    entry.sequence,
                    OccurrenceDefinition {
                        members: Vec::new(),
                    },
                ));
            };
            let mut malformed = false;
            let members = (0..count)
                .filter_map(|index| {
                    let member = record
                        .integer(4 + index)
                        .and_then(|value| u32::try_from(value).ok())
                        .filter(|sequence| sequence % 2 == 1 && entries.contains_key(sequence));
                    malformed |= member.is_none();
                    member
                })
                .collect();
            if malformed {
                malformed_definition_sequences.push(entry.sequence);
            }
            Some((entry.sequence, OccurrenceDefinition { members }))
        })
        .collect::<BTreeMap<_, _>>();
    let contained_instances = occurrence_definitions
        .values()
        .flat_map(|definition| definition.members.iter().copied())
        .filter(|sequence| {
            entries
                .get(sequence)
                .is_some_and(|entry| matches!(entry.entity_type, 408 | 420))
        })
        .collect::<std::collections::BTreeSet<_>>();
    let mut occurrence_neutral_links = BTreeMap::<u32, Vec<String>>::new();
    for curve in &ir.model.curves {
        if let Some(sequence) = curve
            .source_object
            .as_ref()
            .filter(|source| source.format == "iges")
            .and_then(|source| source.object_id.strip_prefix('D'))
            .and_then(|value| value.parse::<u32>().ok())
        {
            occurrence_neutral_links
                .entry(sequence)
                .or_default()
                .push(curve.id.0.clone());
        }
    }
    for surface in &ir.model.surfaces {
        if let Some(sequence) = surface
            .source_object
            .as_ref()
            .filter(|source| source.format == "iges")
            .and_then(|source| source.object_id.strip_prefix('D'))
            .and_then(|value| value.parse::<u32>().ok())
        {
            occurrence_neutral_links
                .entry(sequence)
                .or_default()
                .push(surface.id.0.clone());
        }
    }
    for body in &ir.model.bodies {
        if let Some(sequence) = model_id_directory_sequence(&body.id.0, "iges:model:body#D") {
            occurrence_neutral_links
                .entry(sequence)
                .or_default()
                .push(body.id.0.clone());
        }
    }
    for point in &ir.model.points {
        if let Some(sequence) = model_id_directory_sequence(&point.id.0, "iges:model:point#D") {
            occurrence_neutral_links
                .entry(sequence)
                .or_default()
                .push(point.id.0.clone());
        }
    }
    let mut product_occurrences = Vec::new();
    let mut output_truncated_at = None;
    let mut depth_truncated_at = None;
    let mut malformed_placement_sequences = std::collections::BTreeSet::new();
    let expansion = OccurrenceExpansion {
        entries: &entries,
        records: &by_directory,
        definitions: &occurrence_definitions,
        neutral_links: &occurrence_neutral_links,
        length_factor: global.length_factor_mm(),
        precision: global.real_precision(),
        output_limit: limits.output,
        depth_limit: limits.depth,
        ctx,
    };
    if malformed_definition_sequences.is_empty() {
        for root in directory.iter().filter(|entry| {
            matches!(entry.entity_type, 408 | 420)
                && entry.form == 0
                && !contained_instances.contains(&entry.sequence)
        }) {
            if let Some(source_sequence) = expansion.expand(
                root.sequence,
                Affine::IDENTITY,
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
    let issues = [
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
    .flatten()
    .collect::<Vec<_>>();
    let product_occurrence_expansion = [NativeProductOccurrenceExpansion {
        id: "iges:product:occurrence-expansion#state".into(),
        output_limit: limits.output,
        depth_limit: limits.depth,
        emitted: product_occurrences.len(),
        truncated: !issues.is_empty(),
        issues,
    }];
    parameter_resolver.append_to(references);
    for entity in &mut entities {
        entity.links = references
            .get(&entity.directory_sequence)
            .into_iter()
            .flatten()
            .filter_map(ReferenceEdge::target)
            .map(str::to_owned)
            .collect();
        entity.references = references
            .get(&entity.directory_sequence)
            .cloned()
            .unwrap_or_default();
    }
    let native_entity_count = [
        directions.len(),
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
        product_occurrences.len(),
        product_occurrence_expansion.len(),
    ]
    .into_iter()
    .fold(0_u64, |total, count| total.saturating_add(count as u64));
    if let Some(ctx) = ctx {
        ctx.charge_entities(native_entity_count, "iges_native_entities")?;
    }
    let namespace = ir.native.namespace_mut("iges");
    namespace.version = 2;
    namespace.set_arena_from("cards", cards)?;
    namespace.set_arena_from("entities", entities)?;
    namespace.set_arena_from("directions", directions)?;
    namespace.set_arena_from("transformations", transforms)?;
    namespace.set_arena_from("copious_data", copious_data)?;
    namespace.set_arena_from("colors", colors)?;
    namespace.set_arena_from("display_attributes", display_attributes)?;
    namespace.set_arena_from("line_fonts", line_fonts)?;
    namespace.set_arena_from("text_templates", text_templates)?;
    namespace.set_arena_from("text_fonts", text_fonts)?;
    namespace.set_arena_from("definition_levels", definition_levels)?;
    namespace.set_arena_from("primitive_solids", primitive_solids)?;
    namespace.set_arena_from("procedural_solids", procedural_solids)?;
    namespace.set_arena_from("boolean_trees", boolean_trees)?;
    namespace.set_arena_from("selected_components", selected_components)?;
    namespace.set_arena_from("solid_assemblies", solid_assemblies)?;
    namespace.set_arena_from("solid_instances", solid_instances)?;
    namespace.set_arena_from("subfigure_definitions", subfigure_definitions)?;
    namespace.set_arena_from("subfigure_instances", subfigure_instances)?;
    namespace.set_arena_from("network_definitions", network_definitions)?;
    namespace.set_arena_from("network_instances", network_instances)?;
    namespace.set_arena_from("connect_points", connect_points)?;
    namespace.set_arena_from("rectangular_arrays", rectangular_arrays)?;
    namespace.set_arena_from("circular_arrays", circular_arrays)?;
    namespace.set_arena_from("external_references", external_references)?;
    namespace.set_arena_from("groups", groups)?;
    namespace.set_arena_from("associativities", associativities)?;
    namespace.set_arena_from("attribute_table_definitions", attribute_table_definitions)?;
    namespace.set_arena_from("attribute_table_instances", attribute_table_instances)?;
    namespace.set_arena_from("product_properties", product_properties)?;
    namespace.set_arena_from("properties", properties)?;
    namespace.set_arena_from("units_data", units_data)?;
    namespace.set_arena_from("views", views)?;
    namespace.set_arena_from("view_visibility", view_visibility)?;
    namespace.set_arena_from("segmented_visibility", segmented_visibility)?;
    namespace.set_arena_from("drawings", drawings)?;
    namespace.set_arena_from("annotations", annotations)?;
    namespace.set_arena_from("product_occurrences", product_occurrences)?;
    namespace.set_arena_from("product_occurrence_expansion", product_occurrence_expansion)?;
    Ok(NativeStoreResult {
        occurrence_expansion: ProductOccurrenceExpansion {
            output_truncated_at,
            depth_truncated_at,
            malformed_definition_sequences,
            malformed_placement_sequences: malformed_placement_sequences.into_iter().collect(),
        },
        ambiguous_parameter_sequences,
    })
}

#[cfg(test)]
mod tests;
