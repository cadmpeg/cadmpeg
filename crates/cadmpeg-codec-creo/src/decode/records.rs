// SPDX-License-Identifier: Apache-2.0
//! Record shadow-layer structs and their `ContainerScan` mappers, moved
//! verbatim from `decode.rs`.

#[allow(clippy::wildcard_imports)]
use super::*;

#[derive(Serialize)]
pub(super) struct CreoSketchRecord {
    pub(super) id: String,
    pub(super) definition_id: u32,
    pub(super) owner_feature_id: Option<u32>,
    pub(super) source_section: String,
    pub(super) offset: usize,
    pub(super) section_3d: Option<CreoSketchSection3d>,
    pub(super) table_headers: Vec<CreoSketchTableHeader>,
    pub(super) section_points: Vec<CreoSketchSectionPoint>,
    pub(super) solved_external_ids: Vec<u32>,
    pub(super) variables: Vec<CreoSketchVariable>,
    pub(super) segments: Vec<CreoSketchSegment>,
    pub(super) opaque_segments: Vec<CreoSketchOpaqueSegment>,
    pub(super) trim_entities: Vec<CreoSketchTrimEntity>,
    pub(super) trim_vertices: Vec<CreoSketchTrimVertex>,
    pub(super) order_rows: Vec<CreoSketchOrderRow>,
    pub(super) saved_entities: Vec<CreoSketchSavedEntity>,
    pub(super) dimensions: Vec<CreoSketchDimension>,
    pub(super) relations: Vec<CreoSketchRelation>,
    pub(super) skamps: Vec<CreoSketchSkamp>,
    pub(super) relation_triples: Vec<CreoSketchRelationTriple>,
}

#[derive(Serialize)]
pub(super) struct CreoFeatureDefinitionRecord {
    pub(super) id: String,
    pub(super) definition_id: u32,
    pub(super) owner_feature_id: Option<u32>,
    pub(super) source_section: String,
    pub(super) body: Vec<u8>,
    pub(super) parameter_frames: Vec<CreoFeatureParameterFrame>,
    pub(super) outlines: Vec<CreoFeatureOutline>,
    pub(super) offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoCurveExpressionRecord {
    pub(super) id: String,
    pub(super) entity_id: u32,
    pub(super) backup: bool,
    pub(super) local_system: Option<CreoCurveExpressionLocalSystem>,
    pub(super) lines: Vec<CreoCurveExpressionLine>,
    pub(super) assignments: Vec<CreoCurveExpressionAssignment>,
    pub(super) prohibited_constructs: Vec<String>,
}

#[derive(Serialize)]
pub(super) struct CreoFeatureReferenceNameRecord {
    pub(super) id: String,
    pub(super) owner_feature_id: u32,
    pub(super) name: String,
    pub(super) name_bytes: Vec<u8>,
    pub(super) own_reference_id: u32,
    pub(super) reference_type: u32,
    pub(super) offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoFamilyTableRecord {
    pub(super) id: &'static str,
    pub(super) pointer_kind: &'static str,
    pub(super) table_entity_id: Option<u32>,
    pub(super) offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoFeatureEntityRecord {
    pub(super) id: String,
    pub(super) entity_id: u32,
    pub(super) type_byte: u8,
    pub(super) name: String,
    pub(super) offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoFeatureEntityReferenceRecord {
    pub(super) id: String,
    pub(super) source_entity_id: Option<u32>,
    pub(super) target_entity_id: u32,
    pub(super) target_resolved: bool,
    pub(super) offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoFeatureEntityTableRecord {
    pub(super) id: String,
    pub(super) owner_feature_id: Option<u32>,
    pub(super) table_class_id: u32,
    pub(super) entry_ids: Vec<u32>,
    pub(super) entries: Vec<CreoFeatureEntityTableEntryRecord>,
    pub(super) surface_ids: Vec<u32>,
    pub(super) non_surface_entity_ids: Vec<u32>,
    pub(super) offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoFeatureEntityTableEntryRecord {
    pub(super) entity_id: u32,
    pub(super) class_id: u32,
    pub(super) source_entity_id: Option<u32>,
    pub(super) prefixed: bool,
    pub(super) offset: usize,
    pub(super) end_offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoFeatureGeometryTableRecord {
    pub(super) id: String,
    pub(super) owner_feature_id: u32,
    pub(super) kind: &'static str,
    pub(super) declared_count: u32,
    pub(super) entity_class_id: u32,
    pub(super) entry_ids: Option<Vec<u32>>,
    pub(super) offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoFeatureAffectedIdsRecord {
    pub(super) id: String,
    pub(super) owner_feature_id: u32,
    pub(super) kind: &'static str,
    pub(super) ids: Vec<u32>,
    pub(super) offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoFeatureReplayAffectedIdsRecord {
    pub(super) id: String,
    pub(super) owner_feature_id: u32,
    pub(super) geometry_ids: Vec<u32>,
    pub(super) edge_ids: Vec<u32>,
    pub(super) geometry_extent: &'static str,
    pub(super) edge_extent: &'static str,
    pub(super) offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoFeatureLoopRestoreDirectionRecord {
    pub(super) id: String,
    pub(super) owner_feature_id: u32,
    pub(super) lane: &'static str,
    pub(super) value: u32,
    pub(super) offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoFeatureRevolutionExtentRecord {
    pub(super) id: String,
    pub(super) owner_feature_id: u32,
    pub(super) kind: &'static str,
    pub(super) angle_radians: f64,
    pub(super) offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoFeatureChoiceRecord {
    pub(super) id: String,
    pub(super) owner_feature_id: u32,
    pub(super) label: String,
    pub(super) type_byte: Option<u8>,
    pub(super) payload: Vec<u8>,
    pub(super) payload_offset: usize,
    pub(super) offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoFeatureRowRecord {
    pub(super) id: String,
    pub(super) owner_feature_id: u32,
    pub(super) header: [u8; 2],
    pub(super) root_schema_class: Option<u32>,
    pub(super) stream_offset: usize,
    pub(super) body: Vec<u8>,
    pub(super) body_offset: usize,
    pub(super) offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoFeatureChoiceFieldRecord {
    pub(super) id: String,
    pub(super) owner_feature_id: u32,
    pub(super) choice_label: String,
    pub(super) name: String,
    pub(super) type_byte: u8,
    pub(super) value: CreoFeatureFieldValue,
    pub(super) offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoHalfEdgeRecord {
    pub(super) id: String,
    pub(super) curve_id: u32,
    pub(super) side: u8,
    pub(super) face_id: u32,
    pub(super) next: Option<CreoHalfEdgeRef>,
    pub(super) offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoLoopRecord {
    pub(super) id: String,
    pub(super) face_id: u32,
    pub(super) half_edges: Vec<CreoHalfEdgeRef>,
}

#[derive(Serialize)]
pub(super) struct CreoTopologicalVertexRecord {
    pub(super) id: String,
    pub(super) vertex_id: u32,
    pub(super) half_edges: Vec<CreoHalfEdgeRef>,
}

#[derive(Serialize)]
pub(super) struct CreoHalfEdgeVertexIncidenceRecord {
    pub(super) id: String,
    pub(super) half_edge: CreoHalfEdgeRef,
    pub(super) start_vertex_id: u32,
    pub(super) end_vertex_id: Option<u32>,
}

#[derive(Serialize)]
pub(super) struct CreoFaceComponentRecord {
    pub(super) id: String,
    pub(super) face_ids: Vec<u32>,
    pub(super) curve_ids: Vec<u32>,
}

#[derive(Serialize)]
pub(super) struct CreoExpandedSectionRecord {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) source_offset: usize,
    pub(super) compressed_length: usize,
    pub(super) expanded_length: usize,
    pub(super) sha256: String,
}

#[derive(Serialize)]
pub(super) struct CreoDoubleXarTableRecord {
    pub(super) id: String,
    pub(super) section_name: String,
    pub(super) section_source_offset: usize,
    pub(super) expanded_offset: usize,
    pub(super) count: u32,
    pub(super) entries: Vec<CreoDoubleXarEntryRecord>,
}

#[derive(Serialize)]
pub(super) struct CreoDoubleXarEntryRecord {
    pub(super) index: u32,
    pub(super) raw: Vec<u8>,
    pub(super) value: Option<f64>,
    pub(super) kind: &'static str,
}

#[derive(Serialize)]
pub(super) struct CreoPrimitiveScalarArrayRecord {
    pub(super) id: String,
    pub(super) field: String,
    pub(super) expanded_offset: usize,
    pub(super) count: u32,
    pub(super) values: Vec<f64>,
}

#[derive(Debug, Serialize)]
pub(super) struct CreoReferenceLineRecord {
    pub(super) id: String,
    pub(super) family: &'static str,
    pub(super) entity_id: Option<u32>,
    pub(super) start: [f64; 3],
    pub(super) end: [f64; 3],
    pub(super) original_length: Option<f64>,
    pub(super) offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoReferenceCircleRecord {
    pub(super) id: String,
    pub(super) entity_id: u32,
    pub(super) center: [f64; 3],
    pub(super) center_source: &'static str,
    pub(super) radius: f64,
    pub(super) axis: [f64; 3],
    pub(super) endpoints: [[f64; 3]; 2],
    pub(super) offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoReferenceConicRecord {
    pub(super) id: String,
    pub(super) entity_id: u32,
    pub(super) type_id: u32,
    pub(super) flip: u32,
    pub(super) endpoints: [[f64; 3]; 2],
    pub(super) parameter_interval: [Option<f64>; 2],
    pub(super) coefficients: [f64; 2],
    pub(super) local_system: Option<[f64; 12]>,
    pub(super) body: Vec<u8>,
    pub(super) offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoReferenceEllipseRecord {
    pub(super) id: String,
    pub(super) source_conic_id: String,
    pub(super) source_entity_id: u32,
    pub(super) center: [f64; 3],
    pub(super) axis: [f64; 3],
    pub(super) major_direction: [f64; 3],
    pub(super) major_radius: f64,
    pub(super) minor_radius: f64,
    pub(super) offset: usize,
}

pub(super) fn expanded_section_records(scan: &ContainerScan) -> Vec<CreoExpandedSectionRecord> {
    scan.framing
        .expanded_sections
        .iter()
        .map(|section| CreoExpandedSectionRecord {
            id: format!(
                "creo:container:expanded_section#{}:{}",
                section.name, section.source_offset
            ),
            name: section.name.clone(),
            source_offset: section.source_offset,
            compressed_length: section.compressed_length,
            expanded_length: section.data.len(),
            sha256: sha256_hex(&section.data),
        })
        .collect()
}

#[derive(Serialize)]
pub(super) struct CreoFcCurveCoordinateRecord {
    pub(super) id: String,
    pub(super) curve_id: u32,
    pub(super) subtype: u8,
    pub(super) body: Vec<u8>,
    pub(super) values_mm: Vec<f64>,
    pub(super) tokens: Vec<CreoFcCurveCoordinateToken>,
    pub(super) opaque_spans: Vec<CreoFcCurveOpaqueSpan>,
    pub(super) offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoPrototypePcurveRecord {
    pub(super) id: String,
    pub(super) curve_id: u32,
    pub(super) face_0_endpoints: [[f64; 2]; 2],
    pub(super) face_1_endpoints: [[f64; 2]; 2],
    pub(super) offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoCurvePrototypeTopologyRecord {
    pub(super) id: String,
    pub(super) curve_id: u32,
    pub(super) faces: [u32; 2],
    pub(super) next_edges: [u32; 2],
    pub(super) offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoCurvePrototypeRecord {
    pub(super) id: String,
    pub(super) curve_id: u32,
    pub(super) type_byte: u8,
    pub(super) generating_feature_id: Option<u32>,
    pub(super) offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoPlaneLocalSystemRecord {
    pub(super) id: String,
    pub(super) surface_id: u32,
    pub(super) body: Vec<u8>,
    pub(super) slots: Vec<Option<f64>>,
    pub(super) origin: Option<[f64; 3]>,
    pub(super) u_axis: Option<[f64; 3]>,
    pub(super) normal: Option<[f64; 3]>,
    pub(super) classification: &'static str,
    pub(super) row_offset: usize,
    pub(super) offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoPlaneEnvelopeRecord {
    pub(super) id: String,
    pub(super) surface_id: u32,
    pub(super) body: Vec<u8>,
    pub(super) envelope: CreoPlaneEnvelope,
    pub(super) corner_coordinate_equal: [Option<bool>; 3],
    pub(super) scalar_tokens: Vec<Vec<u8>>,
    pub(super) row_offset: usize,
    pub(super) offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoOutlinePlaneRecord {
    pub(super) id: String,
    pub(super) surface_id: u32,
    pub(super) origin: [f64; 3],
    pub(super) normal: [f64; 3],
    pub(super) u_axis: [f64; 3],
    pub(super) offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoDatumPlaneRecord {
    pub(super) id: String,
    pub(super) datum_id: u32,
    pub(super) owner_feature_id: u32,
    pub(super) normal: [f64; 3],
    pub(super) plane_offset: f64,
    pub(super) corners: [[Option<f64>; 3]; 2],
    pub(super) offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoFeatureSectionTransformRecord {
    pub(super) id: String,
    pub(super) definition_id: u32,
    pub(super) owner_feature_id: Option<u32>,
    pub(super) origin: [f64; 3],
    pub(super) u_axis: [f64; 3],
    pub(super) v_axis: [f64; 3],
    pub(super) normal: [f64; 3],
    pub(super) offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoFeaturePlacementInstructionRecord {
    pub(super) id: String,
    pub(super) definition_id: u32,
    pub(super) owner_feature_id: Option<u32>,
    pub(super) instruction_type: u32,
    pub(super) zero_offset: bool,
    pub(super) dimension_id: Option<u32>,
    pub(super) reference_id: Option<u32>,
    pub(super) geometry1_id: Option<u32>,
    pub(super) geometry2_id: Option<u32>,
    pub(super) member1: u32,
    pub(super) member2: u32,
    pub(super) offset: usize,
    pub(super) source_section: String,
}

pub(super) fn feature_entity_records(scan: &ContainerScan) -> Vec<CreoFeatureEntityRecord> {
    scan.features
        .entities
        .iter()
        .map(|entity| CreoFeatureEntityRecord {
            id: format!("creo:allfeatur:entity#{}", entity.entity_id),
            entity_id: entity.entity_id,
            type_byte: entity.type_byte,
            name: entity.name.clone(),
            offset: entity.offset,
        })
        .collect()
}

pub(super) fn feature_entity_reference_records(
    scan: &ContainerScan,
) -> Vec<CreoFeatureEntityReferenceRecord> {
    scan.features
        .entity_references
        .iter()
        .map(|reference| CreoFeatureEntityReferenceRecord {
            id: format!("creo:allfeatur:entity_reference#{}", reference.offset),
            source_entity_id: reference.source_entity_id,
            target_entity_id: reference.target_entity_id,
            target_resolved: reference.target_resolved,
            offset: reference.offset,
        })
        .collect()
}

pub(super) fn feature_entity_table_records(
    scan: &ContainerScan,
) -> Vec<CreoFeatureEntityTableRecord> {
    scan.features
        .entity_tables
        .iter()
        .map(|table| CreoFeatureEntityTableRecord {
            id: format!("creo:allfeatur:entity_table#{}", table.offset),
            owner_feature_id: table.feature_id,
            table_class_id: table.table_class_id,
            entry_ids: table.entry_ids.clone(),
            entries: table
                .entries
                .iter()
                .map(|entry| CreoFeatureEntityTableEntryRecord {
                    entity_id: entry.entity_id,
                    class_id: entry.class_id,
                    source_entity_id: entry.source_entity_id,
                    prefixed: entry.prefixed,
                    offset: entry.offset,
                    end_offset: entry.end_offset,
                })
                .collect(),
            surface_ids: table.surface_ids.clone(),
            non_surface_entity_ids: table.non_surface_entity_ids.clone(),
            offset: table.offset,
        })
        .collect()
}

pub(super) fn feature_geometry_table_records(
    scan: &ContainerScan,
) -> Vec<CreoFeatureGeometryTableRecord> {
    scan.features
        .geometry_tables
        .iter()
        .map(|table| CreoFeatureGeometryTableRecord {
            id: format!("creo:feature:geometry_table#{}", table.offset),
            owner_feature_id: table.feature_id,
            kind: match table.kind {
                crate::feature::FeatureGeometryTableKind::EdgeIds => "edge_ids",
                crate::feature::FeatureGeometryTableKind::LoopIds => "loop_ids",
                crate::feature::FeatureGeometryTableKind::Boundaries => "boundaries",
                crate::feature::FeatureGeometryTableKind::UsedBodies => "used_bodies",
                crate::feature::FeatureGeometryTableKind::GeometryLists => "geometry_lists",
                crate::feature::FeatureGeometryTableKind::DatumIds => "datum_ids",
            },
            declared_count: table.count,
            entity_class_id: table.entity_class,
            entry_ids: table.entry_ids.clone(),
            offset: table.offset,
            source_section: source_section(scan, table.offset),
        })
        .collect()
}

pub(super) fn feature_affected_id_records(
    scan: &ContainerScan,
) -> Vec<CreoFeatureAffectedIdsRecord> {
    scan.features
        .affected_ids
        .iter()
        .map(|record| CreoFeatureAffectedIdsRecord {
            id: format!("creo:feature:affected_ids#{}", record.offset),
            owner_feature_id: record.feature_id,
            kind: affected_kind(record.kind),
            ids: record.ids.clone(),
            offset: record.offset,
            source_section: source_section(scan, record.offset),
        })
        .collect()
}

pub(super) fn feature_replay_affected_id_records(
    scan: &ContainerScan,
) -> Vec<CreoFeatureReplayAffectedIdsRecord> {
    scan.features
        .replay_affected_ids
        .iter()
        .map(|record| CreoFeatureReplayAffectedIdsRecord {
            id: format!("creo:feature:replay_affected_ids#{}", record.offset),
            owner_feature_id: record.feature_id,
            geometry_ids: record.geometry_ids.clone(),
            edge_ids: record.edge_ids.clone(),
            geometry_extent: extent_source(record.geometry_extent),
            edge_extent: extent_source(record.edge_extent),
            offset: record.offset,
            source_section: source_section(scan, record.offset),
        })
        .collect()
}

pub(super) fn feature_loop_restore_direction_records(
    scan: &ContainerScan,
) -> Vec<CreoFeatureLoopRestoreDirectionRecord> {
    scan.features
        .loop_restore_directions
        .iter()
        .map(|record| CreoFeatureLoopRestoreDirectionRecord {
            id: format!("creo:feature:loop_restore_direction#{}", record.offset),
            owner_feature_id: record.feature_id,
            lane: match record.lane {
                crate::feature::LoopRestoreDirectionLane::Primary => "primary",
                crate::feature::LoopRestoreDirectionLane::Secondary => "secondary",
            },
            value: record.value,
            offset: record.offset,
            source_section: source_section(scan, record.offset),
        })
        .collect()
}

pub(super) fn feature_revolution_extent_records(
    scan: &ContainerScan,
) -> Vec<CreoFeatureRevolutionExtentRecord> {
    scan.features
        .revolution_extents
        .iter()
        .map(|record| CreoFeatureRevolutionExtentRecord {
            id: format!("creo:feature:revolution_extent#{}", record.offset),
            owner_feature_id: record.feature_id,
            kind: match record.kind {
                crate::feature::FeatureRevolutionExtentKind::FullTurn => "full_turn",
            },
            angle_radians: std::f64::consts::TAU,
            offset: record.offset,
            source_section: source_section(scan, record.offset),
        })
        .collect()
}

pub(super) fn feature_choice_records(scan: &ContainerScan) -> Vec<CreoFeatureChoiceRecord> {
    scan.features
        .choices
        .iter()
        .map(|choice| CreoFeatureChoiceRecord {
            id: format!("creo:feature:choice#{}", choice.offset),
            owner_feature_id: choice.feature_id,
            label: choice.label.clone(),
            type_byte: choice.type_byte,
            payload: choice.payload.clone(),
            payload_offset: choice.payload_offset,
            offset: choice.offset,
            source_section: source_section(scan, choice.offset),
        })
        .collect()
}

pub(super) fn feature_row_records(scan: &ContainerScan) -> Vec<CreoFeatureRowRecord> {
    scan.features
        .rows
        .iter()
        .map(|row| CreoFeatureRowRecord {
            id: format!("creo:allfeatur:feature_row#{}", row.offset),
            owner_feature_id: row.feature_id,
            header: row.header,
            root_schema_class: row.root_schema_class,
            stream_offset: row.stream_offset,
            body: row.body.clone(),
            body_offset: row.body_offset,
            offset: row.offset,
            source_section: source_section(scan, row.offset),
        })
        .collect()
}

pub(super) fn depdb_recipe_row_records(scan: &ContainerScan) -> Vec<CreoFeatureRowRecord> {
    scan.features
        .depdb_recipe_rows
        .iter()
        .map(|row| CreoFeatureRowRecord {
            id: format!("creo:depdb:recipe_row#{}", row.offset),
            owner_feature_id: row.feature_id,
            header: row.header,
            root_schema_class: row.root_schema_class,
            stream_offset: row.stream_offset,
            body: row.body.clone(),
            body_offset: row.body_offset,
            offset: row.offset,
            source_section: source_section(scan, row.offset),
        })
        .collect()
}

pub(super) fn feature_choice_field_records(
    scan: &ContainerScan,
) -> Vec<CreoFeatureChoiceFieldRecord> {
    scan.features
        .choice_fields
        .iter()
        .map(|field| CreoFeatureChoiceFieldRecord {
            id: format!("creo:feature:choice_field#{}", field.offset),
            owner_feature_id: field.feature_id,
            choice_label: field.choice_label.clone(),
            name: field.name.clone(),
            type_byte: field.type_byte,
            value: match &field.value {
                crate::feature::FeatureFieldValue::Empty => CreoFeatureFieldValue::Empty,
                crate::feature::FeatureFieldValue::CompactInt(value) => {
                    CreoFeatureFieldValue::CompactInt { value: *value }
                }
                crate::feature::FeatureFieldValue::CompactIntArray(values) => {
                    CreoFeatureFieldValue::CompactIntArray {
                        values: values.clone(),
                    }
                }
                crate::feature::FeatureFieldValue::EntityReference {
                    entity_id,
                    terminated,
                } => CreoFeatureFieldValue::EntityReference {
                    entity_id: *entity_id,
                    terminated: *terminated,
                },
                crate::feature::FeatureFieldValue::ScalarArray {
                    dimensions,
                    count,
                    body,
                    decoded_values,
                } => CreoFeatureFieldValue::ScalarArray {
                    dimensions: *dimensions,
                    count: *count,
                    body: body.clone(),
                    decoded_values: decoded_values.clone(),
                },
                crate::feature::FeatureFieldValue::Raw(bytes) => CreoFeatureFieldValue::Raw {
                    bytes: bytes.clone(),
                },
            },
            offset: field.offset,
            source_section: source_section(scan, field.offset),
        })
        .collect()
}

pub(super) fn half_edge_records(scan: &ContainerScan) -> Vec<CreoHalfEdgeRecord> {
    let topology_rows = scan
        .curves
        .topology_rows
        .iter()
        .map(|row| (row.id, row))
        .collect::<BTreeMap<_, _>>();
    scan.topology
        .half_edges
        .iter()
        .filter_map(|edge| {
            let row = topology_rows.get(&edge.id.curve_id)?;
            Some(CreoHalfEdgeRecord {
                id: format!(
                    "creo:topology:half_edge#{}:{}",
                    edge.id.curve_id, edge.id.side
                ),
                curve_id: edge.id.curve_id,
                side: edge.id.side,
                face_id: edge.face_id,
                next: edge.next.map(half_edge_ref),
                offset: row.offset,
                source_section: source_section(scan, row.offset),
            })
        })
        .collect()
}

pub(super) fn loop_records(scan: &ContainerScan) -> Vec<CreoLoopRecord> {
    scan.topology
        .loops
        .iter()
        .enumerate()
        .map(|(index, record)| CreoLoopRecord {
            id: format!("creo:topology:loop#{}", index + 1),
            face_id: record.face_id,
            half_edges: record
                .half_edges
                .iter()
                .copied()
                .map(half_edge_ref)
                .collect(),
        })
        .collect()
}

pub(super) fn topological_vertex_records(scan: &ContainerScan) -> Vec<CreoTopologicalVertexRecord> {
    scan.topology
        .vertices
        .iter()
        .map(|record| CreoTopologicalVertexRecord {
            id: format!("creo:topology:vertex#{}", record.id),
            vertex_id: record.id,
            half_edges: record
                .half_edges
                .iter()
                .copied()
                .map(half_edge_ref)
                .collect(),
        })
        .collect()
}

pub(super) fn half_edge_vertex_incidence_records(
    scan: &ContainerScan,
) -> Vec<CreoHalfEdgeVertexIncidenceRecord> {
    scan.topology
        .half_edge_vertex_incidence
        .iter()
        .map(|record| CreoHalfEdgeVertexIncidenceRecord {
            id: format!(
                "creo:topology:half_edge_vertex_incidence#{}:{}",
                record.half_edge.curve_id, record.half_edge.side
            ),
            half_edge: half_edge_ref(record.half_edge),
            start_vertex_id: record.start_vertex_id,
            end_vertex_id: record.end_vertex_id,
        })
        .collect()
}

pub(super) fn face_component_records(scan: &ContainerScan) -> Vec<CreoFaceComponentRecord> {
    scan.topology
        .face_components
        .iter()
        .enumerate()
        .map(|(index, record)| CreoFaceComponentRecord {
            id: format!("creo:topology:face_component#{}", index + 1),
            face_ids: record.face_ids.clone(),
            curve_ids: record.curve_ids.clone(),
        })
        .collect()
}

pub(super) fn fc_curve_coordinate_records(
    scan: &ContainerScan,
) -> Vec<CreoFcCurveCoordinateRecord> {
    scan.curves
        .fc_coordinates
        .iter()
        .map(|record| CreoFcCurveCoordinateRecord {
            id: format!("creo:curve:fc_coordinates#{}", record.curve_id),
            curve_id: record.curve_id,
            subtype: record.subtype,
            body: record.body.clone(),
            values_mm: record.values_mm.clone(),
            tokens: record
                .tokens
                .iter()
                .map(|token| CreoFcCurveCoordinateToken {
                    value_mm: token.value_mm,
                    raw: token.raw.clone(),
                    offset: token.offset,
                    length: token.length,
                })
                .collect(),
            opaque_spans: record
                .opaque_spans
                .iter()
                .map(|span| CreoFcCurveOpaqueSpan {
                    raw: span.raw.clone(),
                    offset: span.offset,
                    length: span.length,
                })
                .collect(),
            offset: record.offset,
            source_section: source_section(scan, record.offset),
        })
        .collect()
}

pub(super) fn prototype_pcurve_records(scan: &ContainerScan) -> Vec<CreoPrototypePcurveRecord> {
    scan.curves
        .prototype_pcurves
        .iter()
        .map(|record| CreoPrototypePcurveRecord {
            id: format!("creo:curve:prototype_pcurve#{}", record.curve_id),
            curve_id: record.curve_id,
            face_0_endpoints: record.face_0_endpoints,
            face_1_endpoints: record.face_1_endpoints,
            offset: record.offset,
            source_section: source_section(scan, record.offset),
        })
        .collect()
}

pub(super) fn curve_prototype_topology_records(
    scan: &ContainerScan,
) -> Vec<CreoCurvePrototypeTopologyRecord> {
    scan.curves
        .prototype_topology
        .iter()
        .map(|record| CreoCurvePrototypeTopologyRecord {
            id: format!("creo:curve:prototype_topology#{}", record.curve_id),
            curve_id: record.curve_id,
            faces: record.faces,
            next_edges: record.next_edges,
            offset: record.offset,
            source_section: source_section(scan, record.offset),
        })
        .collect()
}

pub(super) fn curve_prototype_records(
    scan: &ContainerScan,
    prototypes: &[crate::curve::CurvePrototype],
    id_prefix: &str,
) -> Vec<CreoCurvePrototypeRecord> {
    prototypes
        .iter()
        .map(|record| CreoCurvePrototypeRecord {
            id: format!("{id_prefix}#{}:{}", record.offset, record.id),
            curve_id: record.id,
            type_byte: record.type_byte,
            generating_feature_id: record.feature_id,
            offset: record.offset,
            source_section: source_section(scan, record.offset),
        })
        .collect()
}

pub(super) fn plane_local_system_records(
    scan: &ContainerScan,
    systems: &[crate::surface::PlaneLocalSystem],
    id_prefix: &str,
) -> Vec<CreoPlaneLocalSystemRecord> {
    systems
        .iter()
        .map(|record| CreoPlaneLocalSystemRecord {
            id: format!("{id_prefix}#{}:{}", record.offset, record.surface_id),
            surface_id: record.surface_id,
            body: record.body.clone(),
            slots: record.slots.clone(),
            origin: record.origin,
            u_axis: record.u_axis,
            normal: record.normal,
            classification: match record.classification {
                crate::surface::LocalSystemClassification::Simple => "simple",
                crate::surface::LocalSystemClassification::Unclassified => "unclassified",
            },
            row_offset: record.row_offset,
            offset: record.offset,
            source_section: source_section(scan, record.offset),
        })
        .collect()
}

pub(super) fn plane_envelope_records(
    scan: &ContainerScan,
    envelopes: &[crate::surface::PlaneEnvelopeRecord],
    id_prefix: &str,
) -> Vec<CreoPlaneEnvelopeRecord> {
    envelopes
        .iter()
        .map(|record| CreoPlaneEnvelopeRecord {
            id: format!("{id_prefix}#{}:{}", record.offset, record.surface_id),
            surface_id: record.surface_id,
            body: record.body.clone(),
            envelope: match &record.envelope {
                crate::surface::PlaneEnvelope::Standard {
                    bounds_2d,
                    corners_3d,
                } => CreoPlaneEnvelope::Standard {
                    bounds_2d: *bounds_2d,
                    corners_3d: *corners_3d,
                },
                crate::surface::PlaneEnvelope::Compact { prefix, corners_3d } => {
                    CreoPlaneEnvelope::Compact {
                        prefix: *prefix,
                        corners_3d: *corners_3d,
                    }
                }
            },
            corner_coordinate_equal: record.corner_coordinate_equal,
            scalar_tokens: record.scalar_tokens.clone(),
            row_offset: record.row_offset,
            offset: record.offset,
            source_section: source_section(scan, record.offset),
        })
        .collect()
}

pub(super) fn outline_plane_records(
    scan: &ContainerScan,
    planes: &[crate::surface::OutlinePlane],
    id_prefix: &str,
) -> Vec<CreoOutlinePlaneRecord> {
    planes
        .iter()
        .map(|record| CreoOutlinePlaneRecord {
            id: format!("{id_prefix}#{}:{}", record.offset, record.surface_id),
            surface_id: record.surface_id,
            origin: record.origin,
            normal: record.normal,
            u_axis: record.u_axis,
            offset: record.offset,
            source_section: source_section(scan, record.offset),
        })
        .collect()
}

pub(super) fn datum_plane_records(scan: &ContainerScan) -> Vec<CreoDatumPlaneRecord> {
    scan.planes
        .datums
        .iter()
        .map(|record| CreoDatumPlaneRecord {
            id: format!(
                "creo:datum:plane#{}:{}",
                record.offset_in_payload, record.id
            ),
            datum_id: record.id,
            owner_feature_id: record.feature_id,
            normal: record.normal,
            plane_offset: record.offset,
            corners: record.corners,
            offset: record.offset_in_payload,
            source_section: source_section(scan, record.offset_in_payload),
        })
        .collect()
}

pub(super) fn feature_section_transform_records(
    scan: &ContainerScan,
) -> Vec<CreoFeatureSectionTransformRecord> {
    let mut records = scan
        .features
        .section_transforms
        .iter()
        .map(|record| CreoFeatureSectionTransformRecord {
            id: format!(
                "creo:feature:section_transform#{}:{}",
                record.definition_id, record.offset
            ),
            definition_id: record.definition_id,
            owner_feature_id: record.feature_id,
            origin: record.origin,
            u_axis: record.u_axis,
            v_axis: record.v_axis,
            normal: record.normal,
            offset: record.offset,
            source_section: source_section(scan, record.offset),
        })
        .collect::<Vec<_>>();
    records.sort_by(|left, right| left.id.cmp(&right.id));
    records.dedup_by(|left, right| left.id == right.id);
    records
}

pub(super) fn feature_placement_instruction_records(
    scan: &ContainerScan,
) -> Vec<CreoFeaturePlacementInstructionRecord> {
    scan.features
        .definitions
        .iter()
        .flat_map(|definition| {
            crate::feature::placement_instructions(definition)
                .into_iter()
                .map(|instruction| CreoFeaturePlacementInstructionRecord {
                    id: format!(
                        "creo:featdefs:placement_instruction#{}:{}",
                        definition.id, instruction.offset
                    ),
                    definition_id: definition.id,
                    owner_feature_id: definition.owner_feature_id,
                    instruction_type: instruction.kind,
                    zero_offset: instruction.zero_offset,
                    dimension_id: instruction.dimension_id,
                    reference_id: instruction.reference_id,
                    geometry1_id: instruction.geometry1_id,
                    geometry2_id: instruction.geometry2_id,
                    member1: instruction.member1,
                    member2: instruction.member2,
                    offset: instruction.offset,
                    source_section: source_section(scan, instruction.offset),
                })
        })
        .collect()
}

#[derive(Serialize)]
pub(super) struct CreoSurfaceParameterRecord {
    pub(super) id: String,
    pub(super) surface_id: u32,
    pub(super) surface_type_byte: u8,
    pub(super) surface_family: &'static str,
    pub(super) boundary: &'static str,
    pub(super) body: Vec<u8>,
    pub(super) slots: Vec<CreoSurfaceParameterSlot>,
    pub(super) opaque_spans: Vec<CreoSurfaceParameterOpaqueSpan>,
    pub(super) scalar_frames: Vec<CreoSurfaceParameterScalarFrame>,
    pub(super) terminal_scalar_frame: Option<CreoSurfaceParameterScalarFrame>,
    pub(super) tabulated_cylinder_frame: Option<CreoTabulatedCylinderFrame>,
    pub(super) positional_cylinder_frame: Option<CreoPositionalCylinderFrame>,
    pub(super) split_cylinder_outline_bounds: Option<[[f64; 2]; 2]>,
    pub(super) positional_cone_frame: Option<CreoPositionalConeFrame>,
    pub(super) positional_torus_frame: Option<CreoPositionalTorusFrame>,
    pub(super) torus_outline_frame: Option<CreoTorusOutlineFrame>,
    pub(super) type26_five_coordinate_envelope: Option<CreoType26FiveCoordinateEnvelope>,
    pub(super) type26_split_coordinate_envelope: Option<CreoType26SplitCoordinateEnvelope>,
    pub(super) torus_radius_overrides: Option<CreoTorusRadiusOverrides>,
    pub(super) cone_half_angle_override: Option<CreoConeHalfAngleOverride>,
    pub(super) extrusion_direction: Option<[f64; 3]>,
    pub(super) row_offset: usize,
    pub(super) body_offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoSurfaceRowRecord {
    pub(super) id: String,
    pub(super) surface_id: u32,
    pub(super) type_byte: u8,
    pub(super) surface_family: &'static str,
    pub(super) surface_variant: Option<&'static str>,
    pub(super) feature_id: u32,
    pub(super) reversed: bool,
    pub(super) boundary_type: u8,
    pub(super) next_surface: u32,
    pub(super) offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoSurfacePrototypeRecord {
    pub(super) id: String,
    pub(super) declared_family: String,
    pub(super) family: String,
    pub(super) parameters: Vec<CreoSurfaceNamedParameterRecord>,
    pub(super) offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoSurfaceNamedParameterRecord {
    pub(super) name: String,
    pub(super) value_kind: &'static str,
    pub(super) compact_values: Vec<u32>,
    pub(super) scalar_dimensions: Option<u32>,
    pub(super) scalar_count: Option<u32>,
    pub(super) scalar_values: Vec<Option<f64>>,
    pub(super) scalar_tokens: Vec<Vec<u8>>,
    pub(super) opaque: Vec<u8>,
    pub(super) body: Vec<u8>,
    pub(super) offset: usize,
    pub(super) value_offset: usize,
}

#[derive(Serialize)]
pub(super) struct CreoCurveParameterRecord {
    pub(super) id: String,
    pub(super) curve_id: u32,
    pub(super) type_byte: u8,
    pub(super) body: Vec<u8>,
    pub(super) scalar_values: Vec<f64>,
    pub(super) scalar_tokens: Vec<CreoCurveParameterScalar>,
    pub(super) skipped_references: Vec<u32>,
    pub(super) references: Vec<CreoCurveParameterReference>,
    pub(super) opaque_spans: Vec<CreoCurveParameterOpaqueSpan>,
    pub(super) suffix: &'static str,
    pub(super) suffix_candidate_count: Option<usize>,
    pub(super) offset: usize,
    pub(super) body_offset: usize,
    pub(super) suffix_offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoCurveTopologyRowRecord {
    pub(super) id: String,
    pub(super) curve_id: u32,
    pub(super) type_byte: u8,
    pub(super) feature_id: u32,
    pub(super) directions: [u8; 2],
    pub(super) faces: [u32; 2],
    pub(super) next_edges: [u32; 2],
    pub(super) offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoCrossSectionCurveRowRecord {
    pub(super) id: String,
    pub(super) curve_id: u32,
    pub(super) type_byte: u8,
    pub(super) feature_id: u32,
    pub(super) directions: [u8; 2],
    pub(super) suffix: [u32; 4],
    pub(super) body: Vec<u8>,
    pub(super) scalar_values: Vec<f64>,
    pub(super) scalar_tokens: Vec<CreoCurveParameterScalar>,
    pub(super) references: Vec<CreoCurveParameterReference>,
    pub(super) opaque_spans: Vec<CreoCurveParameterOpaqueSpan>,
    pub(super) offset: usize,
    pub(super) source_section: String,
}

#[derive(Serialize)]
pub(super) struct CreoTabulatedCylinderCurveReplayRecord {
    pub(super) id: String,
    pub(super) surface_id: u32,
    pub(super) curve_id: u32,
    pub(super) curve_type: u8,
    pub(super) flip: u8,
    pub(super) tangent_condition: u8,
    pub(super) degree: u8,
    pub(super) parameter_body: Vec<u8>,
    pub(super) control_point_ids: [u32; 4],
    pub(super) successor_reference: u32,
    pub(super) control_point_bodies: [Vec<u8>; 4],
    pub(super) control_points: [Option<[f64; 2]>; 4],
    pub(super) terminal_reference: u32,
    pub(super) offset: usize,
    pub(super) surface_row_offset: usize,
    pub(super) source_section: String,
}

pub(super) fn surface_row_records(
    scan: &ContainerScan,
    rows: &[crate::surface::SurfaceRow],
    namespace: &str,
) -> Vec<CreoSurfaceRowRecord> {
    rows.iter()
        .map(|row| CreoSurfaceRowRecord {
            id: format!("creo:{namespace}:surface_row#{}", row.id),
            surface_id: row.id,
            type_byte: row.type_byte,
            surface_family: surface_family(row.kind),
            surface_variant: surface_variant(row.type_byte),
            feature_id: row.feature_id,
            reversed: row.reversed,
            boundary_type: row.boundary_type,
            next_surface: row.next_surface,
            offset: row.offset,
            source_section: source_section(scan, row.offset),
        })
        .collect()
}

pub(super) fn surface_prototype_records(
    scan: &ContainerScan,
    records: &[crate::surface::SurfacePrototypeRecord],
    id_namespace: &str,
) -> Vec<CreoSurfacePrototypeRecord> {
    records
        .iter()
        .map(|record| CreoSurfacePrototypeRecord {
            id: format!("creo:{id_namespace}:surface_prototype#{}", record.offset),
            declared_family: record.declared_family.clone(),
            family: surface_prototype_family_name(&record.family),
            parameters: record
                .parameters
                .iter()
                .map(surface_named_parameter_record)
                .collect(),
            offset: record.offset,
            source_section: source_section(scan, record.offset),
        })
        .collect()
}

pub(super) fn curve_parameter_records(
    scan: &ContainerScan,
    records: &[crate::curve::CurveParameterRecord],
    id_namespace: &str,
) -> Vec<CreoCurveParameterRecord> {
    records
        .iter()
        .map(|record| {
            let (suffix, suffix_candidate_count) = match record.suffix {
                crate::curve::CurveSuffixStatus::Unique => ("unique", None),
                crate::curve::CurveSuffixStatus::Ambiguous { candidate_count } => {
                    ("ambiguous", Some(candidate_count))
                }
            };
            CreoCurveParameterRecord {
                id: format!("creo:{id_namespace}:curve_parameter#{}", record.curve_id),
                curve_id: record.curve_id,
                type_byte: record.type_byte,
                body: record.body.clone(),
                scalar_values: record.scalar_values.clone(),
                scalar_tokens: record
                    .scalar_tokens
                    .iter()
                    .map(|token| CreoCurveParameterScalar {
                        value: token.value,
                        raw: token.raw.clone(),
                        offset: token.offset,
                        length: token.length,
                    })
                    .collect(),
                skipped_references: record.skipped_references.clone(),
                references: record
                    .references
                    .iter()
                    .map(|reference| CreoCurveParameterReference {
                        entity_id: reference.entity_id,
                        offset: reference.offset,
                        length: reference.length,
                    })
                    .collect(),
                opaque_spans: record
                    .opaque_spans
                    .iter()
                    .map(|span| CreoCurveParameterOpaqueSpan {
                        raw: span.raw.clone(),
                        offset: span.offset,
                        length: span.length,
                    })
                    .collect(),
                suffix,
                suffix_candidate_count,
                offset: record.offset,
                body_offset: record.body_offset,
                suffix_offset: record.suffix_offset,
                source_section: source_section(scan, record.offset),
            }
        })
        .collect()
}

pub(super) fn cross_section_curve_row_records(
    scan: &ContainerScan,
) -> Vec<CreoCrossSectionCurveRowRecord> {
    scan.curves
        .cross_section_rows
        .iter()
        .map(|row| CreoCrossSectionCurveRowRecord {
            id: format!("creo:cross_section_geometry:curve_row#{}", row.id),
            curve_id: row.id,
            type_byte: row.type_byte,
            feature_id: row.feature_id,
            directions: row.directions,
            suffix: row.suffix,
            body: row.body.clone(),
            scalar_values: row.scalar_tokens.iter().map(|token| token.value).collect(),
            scalar_tokens: row
                .scalar_tokens
                .iter()
                .map(|token| CreoCurveParameterScalar {
                    value: token.value,
                    raw: token.raw.clone(),
                    offset: token.offset,
                    length: token.length,
                })
                .collect(),
            references: row
                .references
                .iter()
                .map(|reference| CreoCurveParameterReference {
                    entity_id: reference.entity_id,
                    offset: reference.offset,
                    length: reference.length,
                })
                .collect(),
            opaque_spans: row
                .opaque_spans
                .iter()
                .map(|span| CreoCurveParameterOpaqueSpan {
                    raw: span.raw.clone(),
                    offset: span.offset,
                    length: span.length,
                })
                .collect(),
            offset: row.offset,
            source_section: source_section(scan, row.offset),
        })
        .collect()
}

pub(super) fn curve_topology_row_records(
    scan: &ContainerScan,
    rows: &[crate::curve::CurveTopologyRow],
    id_namespace: &str,
) -> Vec<CreoCurveTopologyRowRecord> {
    rows.iter()
        .map(|row| CreoCurveTopologyRowRecord {
            id: format!("creo:{id_namespace}:curve_topology#{}", row.id),
            curve_id: row.id,
            type_byte: row.type_byte,
            feature_id: row.feature_id,
            directions: row.directions,
            faces: row.faces,
            next_edges: row.next_edges,
            offset: row.offset,
            source_section: source_section(scan, row.offset),
        })
        .collect()
}

pub(super) fn tabulated_cylinder_curve_replay_records(
    scan: &ContainerScan,
) -> Vec<CreoTabulatedCylinderCurveReplayRecord> {
    scan.curves
        .tabulated_cylinder_replays
        .iter()
        .map(|record| CreoTabulatedCylinderCurveReplayRecord {
            id: format!(
                "creo:visibgeom:tabulated_cylinder_curve_replay#{}",
                record.surface_id
            ),
            surface_id: record.surface_id,
            curve_id: record.curve_id,
            curve_type: record.curve_type,
            flip: record.flip,
            tangent_condition: record.tangent_condition,
            degree: record.degree,
            parameter_body: record.parameter_body.clone(),
            control_point_ids: record.control_point_ids,
            successor_reference: record.successor_reference,
            control_point_bodies: record.control_point_bodies.clone(),
            control_points: record.control_points,
            terminal_reference: record.terminal_reference,
            offset: record.offset,
            surface_row_offset: record.surface_row_offset,
            source_section: source_section(scan, record.offset),
        })
        .collect()
}

pub(super) fn surface_parameter_records(
    scan: &ContainerScan,
    rows: &[crate::surface::SurfaceRow],
    parameters: &[crate::surface::SurfaceParameterRecord],
    namespace: &str,
) -> Vec<CreoSurfaceParameterRecord> {
    parameters
        .iter()
        .filter_map(|record| {
            let row = crate::surface::unique_surface_row(rows, record.surface_id)?;
            let surface_family = surface_family(row.kind);
            let boundary = match record.boundary {
                crate::surface::SurfaceBodyBoundary::CompoundClose => "compound_close",
                crate::surface::SurfaceBodyBoundary::NextRow => "next_row",
                crate::surface::SurfaceBodyBoundary::NamedRecord => "named_record",
                crate::surface::SurfaceBodyBoundary::SectionEnd => "section_end",
            };
            let source_section = source_section(scan, record.body_offset);
            Some(CreoSurfaceParameterRecord {
                id: format!("creo:{namespace}:surface_parameter#{}", record.surface_id),
                surface_id: record.surface_id,
                surface_type_byte: row.type_byte,
                surface_family,
                boundary,
                body: record.body.clone(),
                slots: record
                    .scalar_tokens
                    .iter()
                    .map(|slot| CreoSurfaceParameterSlot {
                        value: slot.value,
                        raw: slot.raw.clone(),
                        offset: slot.offset,
                        length: slot.length,
                    })
                    .collect(),
                opaque_spans: record
                    .opaque_spans
                    .iter()
                    .map(|span| CreoSurfaceParameterOpaqueSpan {
                        raw: span.raw.clone(),
                        offset: span.offset,
                        length: span.length,
                    })
                    .collect(),
                scalar_frames: record
                    .scalar_frames
                    .iter()
                    .map(|frame| CreoSurfaceParameterScalarFrame {
                        offset: frame.offset,
                        slots: frame
                            .slots
                            .iter()
                            .map(|slot| CreoSurfaceParameterSlot {
                                value: slot.value,
                                raw: slot.raw.clone(),
                                offset: slot.offset,
                                length: slot.length,
                            })
                            .collect(),
                    })
                    .collect(),
                terminal_scalar_frame: record.terminal_scalar_frame.as_ref().map(|frame| {
                    CreoSurfaceParameterScalarFrame {
                        offset: frame.offset,
                        slots: frame
                            .slots
                            .iter()
                            .map(|slot| CreoSurfaceParameterSlot {
                                value: slot.value,
                                raw: slot.raw.clone(),
                                offset: slot.offset,
                                length: slot.length,
                            })
                            .collect(),
                    }
                }),
                tabulated_cylinder_frame: record.tabulated_cylinder_frame.map(|frame| {
                    CreoTabulatedCylinderFrame {
                        values: frame.values,
                        prefixes: frame.prefixes,
                    }
                }),
                positional_cylinder_frame: record.positional_cylinder_frame.map(|frame| {
                    CreoPositionalCylinderFrame {
                        origin: frame.origin,
                        axis: frame.axis,
                        ref_direction: frame.ref_direction,
                        radius: frame.radius,
                        length: frame.length,
                    }
                }),
                split_cylinder_outline_bounds: record.split_cylinder_outline_bounds,
                positional_cone_frame: record.positional_cone_frame.map(|frame| {
                    CreoPositionalConeFrame {
                        apex: frame.apex,
                        axis: frame.axis,
                        ref_direction: frame.ref_direction,
                        half_angle: frame.half_angle,
                    }
                }),
                positional_torus_frame: record.positional_torus_frame.map(|frame| {
                    CreoPositionalTorusFrame {
                        center: frame.center,
                        axis: frame.axis,
                        ref_direction: frame.ref_direction,
                        major_radius: frame.major_radius,
                        minor_radius: frame.minor_radius,
                    }
                }),
                torus_outline_frame: record.torus_outline_frame(row.type_byte).map(|frame| {
                    CreoTorusOutlineFrame {
                        values: frame.values,
                        selector: frame.selector,
                        offset: frame.offset,
                    }
                }),
                type26_five_coordinate_envelope: record
                    .type26_five_coordinate_envelope(row.type_byte)
                    .map(|envelope| CreoType26FiveCoordinateEnvelope {
                        values: envelope.values,
                        offset: envelope.offset,
                    }),
                type26_split_coordinate_envelope: record
                    .type26_split_coordinate_envelope(row.type_byte)
                    .map(|envelope| CreoType26SplitCoordinateEnvelope {
                        values: envelope.values,
                        offset: envelope.offset,
                    }),
                torus_radius_overrides: record.torus_radius_overrides(row.type_byte).map(
                    |overrides| CreoTorusRadiusOverrides {
                        radius1: overrides.radius1,
                        radius2: overrides.radius2,
                        radius2_encoding: match overrides.radius2_encoding {
                            crate::surface::TorusRadius2Encoding::Direct => "direct",
                            crate::surface::TorusRadius2Encoding::OuterRingDifference => {
                                "outer_ring_difference"
                            }
                        },
                        offset: overrides.offset,
                    },
                ),
                cone_half_angle_override: record.cone_half_angle_override(row.type_byte).map(
                    |half_angle| CreoConeHalfAngleOverride {
                        radians: half_angle.radians,
                        offset: half_angle.offset,
                    },
                ),
                extrusion_direction: (row.kind == crate::surface::SurfaceKind::Extrusion)
                    .then(|| record.extrusion_direction(row.type_byte))
                    .flatten(),
                row_offset: record.offset,
                body_offset: record.body_offset,
                source_section,
            })
        })
        .collect()
}

pub(super) fn feature_operation_state_records(
    scan: &ContainerScan,
) -> Vec<CreoFeatureOperationState> {
    let current_offsets = scan
        .features
        .operations
        .iter()
        .map(|state| (state.feature_id, state.offset))
        .collect::<BTreeMap<_, _>>();
    let mut ordinals = BTreeMap::<u32, usize>::new();
    scan.features
        .operation_states
        .iter()
        .map(|state| {
            let state_ordinal = *ordinals.entry(state.feature_id).or_default();
            ordinals.insert(state.feature_id, state_ordinal + 1);
            CreoFeatureOperationState {
                id: format!(
                    "creo:mdlstatus:feature_state#{}:{state_ordinal}",
                    state.feature_id
                ),
                feature_id: state.feature_id,
                state_ordinal,
                current: current_offsets.get(&state.feature_id) == Some(&state.offset),
                family: state.kind.clone(),
                display_name_stored: state.display_name_stored,
                stored_name: state.stored_name.clone(),
                stored_name_bytes: state.stored_name_bytes.clone(),
                identifier_keyword: state.identifier_keyword.clone(),
                stored_name_prefix: state
                    .stored_name_prefix
                    .map(|prefix| char::from(prefix).to_string()),
                recipe: state.recipe.map(crate::feature::FeatureRecipe::name),
                root_schema_class: state.root_schema_class,
                parent_feature_id: state.parent_feature_id,
                offset: state.offset,
                state_offset: state.state_offset,
            }
        })
        .collect()
}

pub(super) fn feature_reference_name_records(
    scan: &ContainerScan,
) -> Vec<CreoFeatureReferenceNameRecord> {
    scan.features
        .reference_names
        .iter()
        .map(|record| CreoFeatureReferenceNameRecord {
            id: format!("creo:mdlrefinfo:feature_name#{}", record.offset),
            owner_feature_id: record.feature_id,
            name: record.name.clone(),
            name_bytes: record.name_bytes.clone(),
            own_reference_id: record.own_reference_id,
            reference_type: record.reference_type,
            offset: record.offset,
        })
        .collect()
}

#[derive(Serialize)]
pub(super) struct CreoPcurveEndpointRecord {
    pub(super) id: String,
    pub(super) curve_id: u32,
    pub(super) faces: [u32; 2],
    pub(super) face_0_endpoints: [[f64; 2]; 2],
    pub(super) face_1_endpoints: [[f64; 2]; 2],
    pub(super) source_form: &'static str,
}

pub(super) fn pcurve_endpoint_records(
    scan: &ContainerScan,
) -> Vec<(CreoPcurveEndpointRecord, usize)> {
    let mut records = scan
        .curves
        .pcurves
        .iter()
        .map(|pcurve| {
            (
                CreoPcurveEndpointRecord {
                    id: format!("creo:visibgeom:pcurve_endpoints#{}", pcurve.curve_id),
                    curve_id: pcurve.curve_id,
                    faces: pcurve.faces,
                    face_0_endpoints: pcurve.face_0_endpoints,
                    face_1_endpoints: pcurve.face_1_endpoints,
                    source_form: "positional",
                },
                pcurve.offset,
            )
        })
        .collect::<Vec<_>>();
    records.extend(scan.curves.bound_prototype_pcurves.iter().map(|pcurve| {
        (
            CreoPcurveEndpointRecord {
                id: format!(
                    "creo:visibgeom:prototype_pcurve_endpoints#{}",
                    pcurve.curve_id
                ),
                curve_id: pcurve.curve_id,
                faces: pcurve.faces,
                face_0_endpoints: pcurve.face_0_endpoints,
                face_1_endpoints: pcurve.face_1_endpoints,
                source_form: "prototype",
            },
            pcurve.offset,
        )
    }));
    records.sort_by_key(|(_, offset)| *offset);
    records
}

pub(super) fn curve_expression_records(scan: &ContainerScan) -> Vec<CreoCurveExpressionRecord> {
    scan.curves
        .expressions
        .iter()
        .map(|record| CreoCurveExpressionRecord {
            id: curve_expression_record_id(record),
            entity_id: record.entity_id,
            backup: record.backup,
            local_system: record.local_system.as_ref().map(|frame| {
                CreoCurveExpressionLocalSystem {
                    dimensions: frame.dimensions,
                    count: frame.count,
                    body: frame.body.clone(),
                    explicit_slots: frame.explicit_slots,
                    offset: frame.offset,
                }
            }),
            lines: record
                .lines
                .iter()
                .map(|line| CreoCurveExpressionLine {
                    text: line.text.clone(),
                    offset: line.offset,
                })
                .collect(),
            assignments: record
                .assignments
                .iter()
                .map(|assignment| CreoCurveExpressionAssignment {
                    name: assignment.name.clone(),
                    declared_unit: assignment.declared_unit.clone(),
                    expression: assignment.expression.clone(),
                    dependencies: assignment.dependencies.clone(),
                    value: assignment.value.clone(),
                    activation: assignment.activation.token(),
                    offset: assignment.offset,
                })
                .collect(),
            prohibited_constructs: record.prohibited_constructs.clone(),
        })
        .collect()
}

pub(super) fn sketch_records(scan: &ContainerScan) -> Vec<CreoSketchRecord> {
    scan.features
        .definitions
        .iter()
        .filter(|definition| feature_definition_has_sketch_design(definition))
        .map(|definition| CreoSketchRecord {
            id: feature_sketch_record_id_in_scan(scan, definition),
            definition_id: definition.id,
            owner_feature_id: definition.owner_feature_id,
            source_section: source_section(scan, definition.offset),
            offset: definition.offset,
            section_3d: definition
                .section_3d
                .as_ref()
                .map(|section| CreoSketchSection3d {
                    sketch_plane_entity_id: section.sketch_plane_entity_id,
                    sketch_plane_flip: section.sketch_plane_flip.map(binary_flag_value),
                    reference_plane_entity_ids: section.reference_plane_entity_ids.clone(),
                    reference_plane_datum_geometry_id: section.reference_plane_datum_geometry_id,
                    orientation: CreoSketchSectionOrientation {
                        section_flip: section.orientation.section_flip.map(binary_flag_value),
                        reference_type: section.orientation.reference_type,
                        segment_id: section.orientation.segment_id,
                        reference_flip: section.orientation.reference_flip.map(binary_flag_value),
                    },
                    dimension_ids: section.dimension_ids.clone(),
                    offset: section.offset,
                }),
            table_headers: sketch_table_headers(definition),
            section_points: sketch_section_point_records(definition),
            solved_external_ids: definition
                .trim_entities
                .as_ref()
                .map_or_else(Vec::new, |table| table.solved_external_ids.clone()),
            variables: definition
                .variables
                .iter()
                .flat_map(|table| &table.rows)
                .map(|row| CreoSketchVariable {
                    variable_type: row.variable_type,
                    key: row.key,
                    value: row.value,
                    guess: row.guess,
                    known: row.known,
                    homogeneity: row.homogeneity,
                    uvar_id: row.uvar_id,
                    dimension_driven: row.dimension_driven,
                    offset: row.offset,
                })
                .collect(),
            segments: definition
                .segments
                .iter()
                .flat_map(|table| &table.rows)
                .map(|segment| CreoSketchSegment {
                    external_id: segment.external_id,
                    kind: match segment.kind {
                        crate::feature::FeatureSegmentKind::Line => "line",
                        crate::feature::FeatureSegmentKind::Arc => "arc",
                        crate::feature::FeatureSegmentKind::Point => "point",
                    },
                    point_ids: segment.point_ids,
                    center_id: segment.center_id,
                    directions: segment.directions,
                    arc_orientation: segment.arc_orientation,
                    vertical_horizontal_constraint: segment.vertical_horizontal,
                    radius_dimension_id: segment.radius_ref,
                    secondary_radius_dimension_id: segment.radius2_ref,
                    offset: segment.offset,
                })
                .collect(),
            opaque_segments: definition
                .segments
                .iter()
                .flat_map(|table| &table.opaque_rows)
                .map(|segment| CreoSketchOpaqueSegment {
                    external_id: segment.external_id,
                    kind: segment.kind,
                    point_ids: segment.point_ids,
                    center_id: segment.center_id,
                    directions: segment.directions,
                    arc_orientation: segment.arc_orientation,
                    vertical_horizontal_constraint: segment.vertical_horizontal,
                    radius_dimension_id: segment.radius_ref,
                    secondary_radius_dimension_id: segment.radius2_ref,
                    offset: segment.offset,
                })
                .collect(),
            trim_entities: definition
                .trim_entities
                .iter()
                .flat_map(|table| &table.rows)
                .map(|entity| CreoSketchTrimEntity {
                    external_id: entity.external_id,
                    mode: entity.mode,
                    vertices: entity.vertices,
                    center_vertex: entity.center_vertex,
                    kind: match entity.kind {
                        crate::feature::TrimEntityKind::Line => "line",
                        crate::feature::TrimEntityKind::Arc => "arc",
                    },
                    offset: entity.offset,
                })
                .collect(),
            trim_vertices: definition
                .trim_vertices
                .iter()
                .flat_map(|table| &table.rows)
                .map(|vertex| CreoSketchTrimVertex {
                    vertex_id: vertex.vertex_id,
                    entities: vertex.entities.clone(),
                    section_coordinates: vertex.section_coordinates,
                    offset: vertex.offset,
                })
                .collect(),
            order_rows: definition
                .order_table
                .iter()
                .flat_map(|table| &table.rows)
                .map(|row| CreoSketchOrderRow {
                    external_id: row.external_id,
                    internal_id: row.internal_id,
                    bitmask: row.bitmask,
                    offset: row.offset,
                })
                .collect(),
            saved_entities: definition
                .saved_section
                .iter()
                .flat_map(|section| &section.entities)
                .map(|entity| match entity {
                    crate::feature::FeatureSavedEntity::Line(line) => CreoSketchSavedEntity::Line {
                        entity_id: line.entity_id,
                        references: line.references.clone(),
                        attributes: line.attributes.clone(),
                        endpoints: line.endpoints,
                        offset: line.offset,
                    },
                    crate::feature::FeatureSavedEntity::Arc(arc) => CreoSketchSavedEntity::Arc {
                        entity_id: arc.entity_id,
                        center: arc.center,
                        radius: arc.radius,
                        endpoints: arc.endpoints,
                        parameters: arc.parameters,
                        offset: arc.offset,
                    },
                    crate::feature::FeatureSavedEntity::Circle(circle) => {
                        CreoSketchSavedEntity::Circle {
                            entity_id: circle.entity_id,
                            center: circle.center,
                            radius: circle.radius,
                            offset: circle.offset,
                        }
                    }
                    crate::feature::FeatureSavedEntity::Spline(spline) => {
                        CreoSketchSavedEntity::Spline {
                            entity_id: spline.entity_id,
                            declared_point_count: spline.declared_point_count,
                            interpolation_points: spline.interpolation_points.clone(),
                            endpoint_tangents: spline.endpoint_tangents,
                            parameters: spline.parameters.clone(),
                            offset: spline.offset,
                        }
                    }
                    crate::feature::FeatureSavedEntity::Dummy(dummy) => {
                        CreoSketchSavedEntity::Dummy {
                            entity_id: dummy.entity_id,
                            offset: dummy.offset,
                        }
                    }
                })
                .collect(),
            dimensions: definition
                .dimensions
                .iter()
                .flat_map(|table| &table.rows)
                .map(|dimension| CreoSketchDimension {
                    external_id: dimension.external_id,
                    dimension_type: dimension.dimension_type,
                    value: dimension.value,
                    unresolved_value_token: dimension.unresolved_value_token.clone(),
                    unit: match dimension.value_unit {
                        crate::feature::DimensionUnit::Radians => "radians",
                        crate::feature::DimensionUnit::Millimeters => "millimeters",
                        crate::feature::DimensionUnit::SchemaDefined => "schema_defined",
                    },
                    direction_byte: dimension.direction_byte,
                    auxiliary_value: dimension.auxiliary_value,
                    offset: dimension.offset,
                })
                .collect(),
            relations: definition
                .relations
                .iter()
                .flat_map(|table| &table.rows)
                .map(|relation| CreoSketchRelation {
                    relation_id: relation.relation_id,
                    used: relation.used,
                    operands: relation.operands.clone(),
                    operand_vectors: relation.operand_vectors,
                    sign: relation.sign,
                    dimension_id: relation.dimension_id,
                    relation_type: relation.relation_type,
                    body: relation.body.clone(),
                    offset: relation.offset,
                })
                .collect(),
            skamps: definition
                .relations
                .iter()
                .flat_map(|table| &table.skamps)
                .map(|skamp| CreoSketchSkamp {
                    id: skamp.id,
                    kind: skamp.kind,
                    flags: skamp.flags,
                    status: skamp.status,
                    items: skamp
                        .items
                        .iter()
                        .map(|item| CreoSketchSkampItem {
                            entity_id: item.entity_id,
                            sense: item.sense,
                        })
                        .collect(),
                    offset: skamp.offset,
                })
                .collect(),
            relation_triples: definition
                .relations
                .iter()
                .flat_map(|table| &table.triples)
                .map(|triple| CreoSketchRelationTriple {
                    relation: triple.relation_id,
                    equation: triple.equation_id,
                    skamp: triple.skamp_id,
                    offset: triple.offset,
                })
                .collect(),
        })
        .collect()
}

pub(super) fn sketch_section_point_records(
    definition: &crate::feature::FeatureDefinition,
) -> Vec<CreoSketchSectionPoint> {
    let Some(variables) = &definition.variables else {
        return Vec::new();
    };
    let (points, ambiguous) = variables.reconciled_points();
    points
        .keys()
        .copied()
        .chain(ambiguous.iter().copied())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|point_id| {
            let [u, v] = points.get(&point_id).copied().unwrap_or([None; 2]);
            let state = if ambiguous.contains(&point_id) {
                "conflicting"
            } else {
                match (u.is_some(), v.is_some()) {
                    (true, true) => "resolved",
                    (true, false) | (false, true) => "partial",
                    (false, false) => "unresolved",
                }
            };
            CreoSketchSectionPoint {
                point_id,
                u,
                v,
                state,
            }
        })
        .collect()
}

pub(super) fn feature_definition_records(scan: &ContainerScan) -> Vec<CreoFeatureDefinitionRecord> {
    scan.features
        .definitions
        .iter()
        .map(|definition| CreoFeatureDefinitionRecord {
            id: feature_definition_record_id(scan, definition),
            definition_id: definition.id,
            owner_feature_id: definition.owner_feature_id,
            source_section: source_section(scan, definition.offset),
            body: definition.body.clone(),
            parameter_frames: definition
                .parameter_frames
                .iter()
                .map(|frame| CreoFeatureParameterFrame {
                    kind: match frame.kind {
                        crate::feature::FeatureParameterFrameKind::LocalSystem => "local_system",
                        crate::feature::FeatureParameterFrameKind::Transform => "transform",
                    },
                    body: frame.body.clone(),
                    decoded_values: frame.decoded_values.clone(),
                    offset: frame.offset,
                })
                .collect(),
            outlines: definition
                .outlines
                .iter()
                .map(|outline| CreoFeatureOutline {
                    phase: match outline.phase {
                        crate::feature::OutlinePhase::PreRollback => "pre_rollback",
                        crate::feature::OutlinePhase::PostRollback => "post_rollback",
                        crate::feature::OutlinePhase::PostRegen => "post_regen",
                    },
                    local_values: outline.local_values.clone(),
                    offset: outline.offset,
                })
                .collect(),
            offset: definition.offset,
        })
        .collect()
}
