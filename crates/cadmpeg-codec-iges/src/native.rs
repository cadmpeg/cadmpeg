// SPDX-License-Identifier: Apache-2.0
//! Versioned `native.iges` physical cards and entity records.

use crate::card::CardScan;
use crate::directory::DirectoryEntry;
use crate::entities::geometry::{resolve_transform, Affine};
use crate::global::Global;
use crate::graph::ReferenceEdge;
use crate::parameter::{ParameterRecord, Token, TokenValue};
use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::CadIr;
use serde::Serialize;
use std::collections::BTreeMap;

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

fn record_has_property_pointer(record: &ParameterRecord, property_sequence: u32) -> bool {
    (1..record.tokens.len()).any(|association_count_index| {
        let Some(association_count) = record
            .integer(association_count_index)
            .and_then(|value| usize::try_from(value).ok())
        else {
            return false;
        };
        let property_count_index = association_count_index + 1 + association_count;
        let Some(property_count) = record
            .integer(property_count_index)
            .and_then(|value| usize::try_from(value).ok())
        else {
            return false;
        };
        property_count_index + 1 + property_count == record.tokens.len()
            && (0..property_count).any(|index| {
                record.integer(property_count_index + 1 + index)
                    == Some(i64::from(property_sequence))
            })
    })
}

fn placement_affine(
    instance: &DirectoryEntry,
    record: &ParameterRecord,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
    length_factor: f64,
) -> Option<(u32, Affine)> {
    let definition = u32::try_from(record.integer(1)?).ok()?;
    let translation = Affine {
        rows: [
            [
                1.0,
                0.0,
                0.0,
                record.number(2).unwrap_or(0.0) * length_factor,
            ],
            [
                0.0,
                1.0,
                0.0,
                record.number(3).unwrap_or(0.0) * length_factor,
            ],
            [
                0.0,
                0.0,
                1.0,
                record.number(4).unwrap_or(0.0) * length_factor,
            ],
        ],
    };
    let x_scale = record.number(5).unwrap_or(1.0);
    let scales = if instance.entity_type == 420 {
        [
            x_scale,
            record.number(6).unwrap_or(x_scale),
            record.number(7).unwrap_or(x_scale),
        ]
    } else {
        [x_scale; 3]
    };
    let scale = Affine {
        rows: [
            [scales[0], 0.0, 0.0, 0.0],
            [0.0, scales[1], 0.0, 0.0],
            [0.0, 0.0, scales[2], 0.0],
        ],
    };
    let directory = resolve_transform(
        instance.transform,
        entries,
        records,
        length_factor,
        &mut std::collections::BTreeSet::new(),
    )
    .ok()?;
    Some((definition, directory.compose(translation.compose(scale))))
}

struct OccurrenceExpansion<'a> {
    entries: &'a BTreeMap<u32, &'a DirectoryEntry>,
    records: &'a BTreeMap<u32, &'a ParameterRecord>,
    definitions: &'a BTreeMap<u32, OccurrenceDefinition>,
    neutral_links: &'a BTreeMap<u32, Vec<String>>,
    length_factor: f64,
}

impl OccurrenceExpansion<'_> {
    fn expand(
        &self,
        instance_sequence: u32,
        parent: Affine,
        path: &mut Vec<u32>,
        occurrences: &mut Vec<NativeProductOccurrence>,
    ) {
        if path.len() >= 64 || path.contains(&instance_sequence) {
            return;
        }
        let (Some(instance), Some(record)) = (
            self.entries.get(&instance_sequence).copied(),
            self.records.get(&instance_sequence).copied(),
        ) else {
            return;
        };
        let Some((definition_sequence, local)) = placement_affine(
            instance,
            record,
            self.entries,
            self.records,
            self.length_factor,
        ) else {
            return;
        };
        let Some(definition) = self.definitions.get(&definition_sequence) else {
            return;
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
            if self
                .entries
                .get(member)
                .is_some_and(|entry| matches!(entry.entity_type, 408 | 420))
            {
                self.expand(*member, world, path, occurrences);
                continue;
            }
            let member_local = self
                .entries
                .get(member)
                .and_then(|entry| {
                    resolve_transform(
                        entry.transform,
                        self.entries,
                        self.records,
                        self.length_factor,
                        &mut std::collections::BTreeSet::new(),
                    )
                    .ok()
                })
                .unwrap_or(Affine::IDENTITY);
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
    }
}

pub(crate) fn store(
    ir: &mut CadIr,
    scan: &CardScan,
    directory: &[DirectoryEntry],
    parameters: &[ParameterRecord],
    references: &BTreeMap<u32, Vec<ReferenceEdge>>,
    global: &Global,
) -> Result<(), CodecError> {
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
    let entities = directory
        .iter()
        .map(|entry| {
            let parameters = by_directory.get(&entry.sequence).copied();
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
                physically_dependent: entry.status.subordinate == 1,
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
            line_font_definition: (entry.line_font < 0)
                .then(|| format!("iges:entity:directory#{}", entry.line_font.unsigned_abs())),
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
                    tangent_oriented: parameters.and_then(|record| record.integer(1)).and_then(
                        |value| match value {
                            0 => Some(false),
                            1 => Some(true),
                            _ => None,
                        },
                    ),
                    template: parameters
                        .and_then(|record| record.integer(2))
                        .map(|sequence| format!("iges:entity:directory#{sequence}")),
                    spacing: parameters.and_then(|record| record.number(3)),
                    scale: parameters.and_then(|record| record.number(4)),
                }
            } else {
                let count = parameters
                    .and_then(|record| record.integer(1))
                    .and_then(|value| usize::try_from(value).ok())
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
    let definition_levels = directory
        .iter()
        .filter(|entry| entry.entity_type == 406 && entry.form == 1)
        .map(|entry| {
            let parameters = by_directory.get(&entry.sequence).copied();
            let count = parameters
                .and_then(|record| record.integer(1))
                .and_then(|value| usize::try_from(value).ok())
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
        .map(|entry| {
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
                    _ => unreachable!("filtered primitive type"),
                };
            let dimensions = dimension_names
                .into_iter()
                .enumerate()
                .map(|(index, name)| (name.to_owned(), number(index + 1)))
                .collect();
            let axis = |start: usize| [number(start), number(start + 1), number(start + 2)];
            NativePrimitiveSolid {
                id: format!("iges:solid:primitive#D{}", entry.sequence),
                source_entity: format!("iges:entity:directory#{}", entry.sequence),
                kind: kind.into(),
                dimensions,
                origin: axis(origin_start),
                x_axis: x_axis_start.map(axis),
                z_axis: z_axis_start.map(axis),
                transformation: (entry.transform > 0)
                    .then(|| format!("iges:native:transformation#D{}", entry.transform)),
            }
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
                .and_then(|record| record.integer(1))
                .and_then(|value| usize::try_from(value).ok())
                .unwrap_or_default();
            let terms = (0..count)
                .filter_map(|index| record.and_then(|record| record.integer(2 + index)))
                .map(|value| {
                    if value < 0 {
                        NativeBooleanTerm::Operand {
                            entity: value
                                .checked_neg()
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
                .and_then(|record| record.integer(1))
                .and_then(|value| usize::try_from(value).ok())
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
                            .map(|sequence| format!("iges:entity:directory#{sequence}")),
                        transformation: record
                            .and_then(|record| record.integer(2 + count + index))
                            .filter(|sequence| *sequence != 0)
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
                .and_then(|record| record.integer(3))
                .and_then(|value| usize::try_from(value).ok())
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
                .and_then(|record| record.integer(3))
                .and_then(|value| usize::try_from(value).ok())
                .unwrap_or_default();
            let connect_count_index = 7 + member_count;
            let connect_count = record
                .and_then(|record| record.integer(connect_count_index))
                .and_then(|value| usize::try_from(value).ok())
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
                    .map(|sequence| format!("iges:entity:directory#{sequence}")),
                declared_connect_point_count: record
                    .and_then(|record| record.integer(connect_count_index)),
                connect_points: (0..connect_count)
                    .map(|index| {
                        record
                            .and_then(|record| record.integer(8 + member_count + index))
                            .filter(|sequence| *sequence != 0)
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
                .and_then(|record| record.integer(11))
                .and_then(|value| usize::try_from(value).ok())
                .unwrap_or_default();
            NativeNetworkInstance {
                id: format!("iges:product:network-instance#D{}", entry.sequence),
                source_entity: format!("iges:entity:directory#{}", entry.sequence),
                definition: record
                    .and_then(|record| record.integer(1))
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
                    .map(|sequence| format!("iges:entity:directory#{sequence}")),
                declared_connect_point_count: record.and_then(|record| record.integer(11)),
                connect_points: (0..connect_count)
                    .map(|index| {
                        record
                            .and_then(|record| record.integer(12 + index))
                            .filter(|sequence| *sequence != 0)
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
            let optional_link = |index| {
                record
                    .and_then(|record| record.integer(index))
                    .filter(|sequence| *sequence != 0)
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
                display_geometry: optional_link(4),
                type_flag: record.and_then(|record| record.integer(5)),
                function_flag: record.and_then(|record| record.integer(6)),
                function_identifier: record
                    .and_then(|record| record.string(7))
                    .map(<[u8]>::to_vec),
                identifier_display_template: optional_link(8),
                function_name: record
                    .and_then(|record| record.string(9))
                    .map(<[u8]>::to_vec),
                name_display_template: optional_link(10),
                identifier: record.and_then(|record| record.integer(11)),
                function_code: record.and_then(|record| record.integer(12)),
                swap_flag: record.and_then(|record| record.integer(13)),
                owner: optional_link(14),
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
                .and_then(|record| record.integer(11))
                .and_then(|value| usize::try_from(value).ok())
                .unwrap_or_default();
            NativeRectangularArray {
                id: format!("iges:product:rectangular-array#D{}", entry.sequence),
                source_entity: format!("iges:entity:directory#{}", entry.sequence),
                base: record
                    .and_then(|record| record.integer(1))
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
                .and_then(|record| record.integer(9))
                .and_then(|value| usize::try_from(value).ok())
                .unwrap_or_default();
            NativeCircularArray {
                id: format!("iges:product:circular-array#D{}", entry.sequence),
                source_entity: format!("iges:entity:directory#{}", entry.sequence),
                base: record
                    .and_then(|record| record.integer(1))
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
        .map(|entry| {
            let record = by_directory.get(&entry.sequence).copied();
            let (reference_kind, file_index, symbolic_index, library_index) = match entry.form {
                0 => ("external_definition", Some(1), Some(2), None),
                1 => ("external_file_definition", Some(1), None, None),
                2 => ("external_logical", Some(1), Some(2), None),
                3 => ("native_definition", None, Some(1), None),
                4 => ("native_library_definition", None, Some(2), Some(1)),
                _ => unreachable!("filtered external-reference form"),
            };
            NativeExternalReference {
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
            }
        })
        .collect::<Vec<_>>();
    let groups = directory
        .iter()
        .filter(|entry| entry.entity_type == 402 && matches!(entry.form, 1 | 7 | 14 | 15))
        .map(|entry| {
            let record = by_directory.get(&entry.sequence).copied();
            let count = record
                .and_then(|record| record.integer(1))
                .and_then(|value| usize::try_from(value).ok())
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
                            .map(|sequence| format!("iges:entity:directory#{sequence}"))
                    })
                    .collect(),
            }
        })
        .collect::<Vec<_>>();
    let attribute_table_definitions = directory
        .iter()
        .filter(|entry| entry.entity_type == 322 && matches!(entry.form, 0..=2))
        .map(|entry| {
            let record = by_directory.get(&entry.sequence).copied();
            let count = record
                .and_then(|record| record.integer(3))
                .and_then(|value| usize::try_from(value).ok())
                .unwrap_or_default();
            let mut cursor = 4;
            let mut attributes = Vec::with_capacity(count);
            for _ in 0..count {
                let attribute_type = record.and_then(|record| record.integer(cursor));
                let value_data_type = record.and_then(|record| record.integer(cursor + 1));
                let declared_value_count = record.and_then(|record| record.integer(cursor + 2));
                let value_count = match record
                    .and_then(|record| record.tokens.get(cursor + 2))
                    .map(|token| &token.value)
                {
                    None | Some(TokenValue::Omitted) => 1,
                    Some(TokenValue::Integer(value)) => usize::try_from(*value).unwrap_or_default(),
                    Some(TokenValue::Real(_) | TokenValue::String(_)) => 0,
                };
                cursor += 3;
                let mut values = Vec::with_capacity(value_count);
                if entry.form != 0 {
                    for _ in 0..value_count {
                        let value = record
                            .and_then(|record| record.tokens.get(cursor))
                            .map(token)
                            .map_or(NativeTokenValue::Omitted, |token| token.value);
                        cursor += 1;
                        let display_template = (entry.form == 2)
                            .then(|| record.and_then(|record| record.integer(cursor)))
                            .flatten()
                            .filter(|sequence| *sequence != 0)
                            .map(|sequence| format!("iges:entity:directory#{sequence}"));
                        cursor += usize::from(entry.form == 2);
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
            let definition_sequence = entry
                .structure
                .checked_neg()
                .and_then(|value| u32::try_from(value).ok());
            let definition_record =
                definition_sequence.and_then(|sequence| by_directory.get(&sequence).copied());
            let attribute_count = definition_record
                .and_then(|record| record.integer(3))
                .and_then(|value| usize::try_from(value).ok())
                .unwrap_or_default();
            let values_per_row = (0..attribute_count)
                .map(|index| {
                    let count_index = 6 + index * 3;
                    match definition_record
                        .and_then(|record| record.tokens.get(count_index))
                        .map(|token| &token.value)
                    {
                        None | Some(TokenValue::Omitted) => 1,
                        Some(TokenValue::Integer(value)) => {
                            usize::try_from(*value).unwrap_or_default()
                        }
                        Some(TokenValue::Real(_) | TokenValue::String(_)) => 0,
                    }
                })
                .sum::<usize>();
            let row_count = if entry.form == 0 {
                usize::from(values_per_row > 0)
            } else {
                record
                    .and_then(|record| record.integer(1))
                    .and_then(|value| usize::try_from(value).ok())
                    .unwrap_or_default()
            };
            let value_start = if entry.form == 0 { 1 } else { 2 };
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
                    .filter(|(sequence, owner_record)| {
                        **sequence != entry.sequence
                            && record_has_property_pointer(owner_record, entry.sequence)
                    })
                    .map(|(sequence, _)| format!("iges:entity:directory#{sequence}"))
                    .collect(),
            }
        })
        .collect::<Vec<_>>();
    let occurrence_definitions = directory
        .iter()
        .filter(|entry| matches!(entry.entity_type, 308 | 320) && entry.form == 0)
        .filter_map(|entry| {
            let record = by_directory.get(&entry.sequence).copied()?;
            let count = record
                .integer(3)
                .and_then(|value| usize::try_from(value).ok())?;
            let members = (0..count)
                .map(|index| {
                    record
                        .integer(4 + index)
                        .and_then(|value| u32::try_from(value).ok())
                })
                .collect::<Option<Vec<_>>>()?;
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
    let mut product_occurrences = Vec::new();
    if let Some(length_factor) = global.length_factor_mm() {
        let expansion = OccurrenceExpansion {
            entries: &entries,
            records: &by_directory,
            definitions: &occurrence_definitions,
            neutral_links: &occurrence_neutral_links,
            length_factor,
        };
        for root in directory.iter().filter(|entry| {
            matches!(entry.entity_type, 408 | 420)
                && entry.form == 0
                && !contained_instances.contains(&entry.sequence)
        }) {
            expansion.expand(
                root.sequence,
                Affine::IDENTITY,
                &mut Vec::new(),
                &mut product_occurrences,
            );
        }
    }
    let namespace = ir.native.namespace_mut("iges");
    namespace.version = 2;
    namespace.set_arena("cards", &cards)?;
    namespace.set_arena("entities", &entities)?;
    namespace.set_arena("directions", &directions)?;
    namespace.set_arena("transformations", &transforms)?;
    namespace.set_arena("copious_data", &copious_data)?;
    namespace.set_arena("colors", &colors)?;
    namespace.set_arena("display_attributes", &display_attributes)?;
    namespace.set_arena("line_fonts", &line_fonts)?;
    namespace.set_arena("definition_levels", &definition_levels)?;
    namespace.set_arena("primitive_solids", &primitive_solids)?;
    namespace.set_arena("procedural_solids", &procedural_solids)?;
    namespace.set_arena("boolean_trees", &boolean_trees)?;
    namespace.set_arena("selected_components", &selected_components)?;
    namespace.set_arena("solid_assemblies", &solid_assemblies)?;
    namespace.set_arena("solid_instances", &solid_instances)?;
    namespace.set_arena("subfigure_definitions", &subfigure_definitions)?;
    namespace.set_arena("subfigure_instances", &subfigure_instances)?;
    namespace.set_arena("network_definitions", &network_definitions)?;
    namespace.set_arena("network_instances", &network_instances)?;
    namespace.set_arena("connect_points", &connect_points)?;
    namespace.set_arena("rectangular_arrays", &rectangular_arrays)?;
    namespace.set_arena("circular_arrays", &circular_arrays)?;
    namespace.set_arena("external_references", &external_references)?;
    namespace.set_arena("groups", &groups)?;
    namespace.set_arena("attribute_table_definitions", &attribute_table_definitions)?;
    namespace.set_arena("attribute_table_instances", &attribute_table_instances)?;
    namespace.set_arena("product_properties", &product_properties)?;
    namespace.set_arena("product_occurrences", &product_occurrences)?;
    Ok(())
}
