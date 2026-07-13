// SPDX-License-Identifier: Apache-2.0
//! Conversion from a PSB container to [`CadIr`].
//!
//! Decode transfers standard datum planes as derived plane surfaces and
//! preserves each geometry section as an [`UnknownRecord`]. Source metadata
//! records the layout, namespace census, active units, and counts of decoded
//! structural rows.
//!
//! Surface and curve namespaces contain useful topology and prototype data, but
//! the placed body model is incomplete. The report therefore records blocking
//! geometry and topology losses instead of emitting a partial B-rep.

use std::collections::BTreeMap;

use cadmpeg_ir::codec::{CodecError, DecodeOptions, DecodeResult, ReadSeek};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::geometry::{Surface, SurfaceGeometry};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::ids::UnknownId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::report::{DecodeReport, LossCategory, LossNote, Severity};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::AnnotationBuilder;
use cadmpeg_ir::Exactness;

use crate::container::{self, role, ContainerScan};

/// Decode a `.prt` stream into an IR document and loss report.
///
/// The stream is read from its beginning. `options.container_only` is reflected
/// in the report, but the current decoder always performs the same structural
/// scan.
pub fn decode(
    reader: &mut dyn ReadSeek,
    options: &DecodeOptions,
) -> Result<DecodeResult, CodecError> {
    let scan = container::scan(reader)?;

    let ir = build_ir(&scan)?;
    let report = build_report(&scan, options.container_only);
    Ok(DecodeResult::new(ir, report))
}

/// Build source metadata, preserved geometry records, and datum-plane surfaces.
fn build_ir(scan: &ContainerScan) -> Result<CadIr, CodecError> {
    let mut ir = CadIr::empty(Units::default());
    let mut annotations = AnnotationBuilder::new();
    ir.source = Some(source_meta(scan));

    for section in scan.sections.iter().filter(|s| s.role == role::GEOMETRY) {
        let end = (section.offset + section.length).min(scan.data.len());
        let bytes = &scan.data[section.offset..end];
        let id = UnknownId(format!("creo:{}:section#{}", section.name, section.offset));
        annotate(
            &mut annotations,
            &id,
            &section.name,
            section.offset as u64,
            "psb_geometry_section",
            Exactness::Unknown,
        );
        ir.push_native_unknown(
            "creo",
            UnknownRecord {
                id,
                offset: section.offset as u64,
                byte_len: bytes.len() as u64,
                sha256: sha256_hex(bytes),
                data: Some(bytes.to_vec()),
                links: Vec::new(),
            },
        )?;
    }
    for plane in &scan.datum_planes {
        let id = SurfaceId(format!("creo:datum-plane#{}", plane.id));
        annotate(
            &mut annotations,
            &id,
            "ActDatums",
            plane.offset_in_payload as u64,
            "datum_plane_outline",
            Exactness::Derived,
        );
        ir.model.surfaces.push(Surface {
            id,
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(
                    plane.normal[0] * plane.offset,
                    plane.normal[1] * plane.offset,
                    plane.normal[2] * plane.offset,
                ),
                normal: Vector3::new(plane.normal[0], plane.normal[1], plane.normal[2]),
                u_axis: cadmpeg_ir::geometry::derive_reference_direction(Vector3::new(
                    plane.normal[0],
                    plane.normal[1],
                    plane.normal[2],
                )),
            },
            source_object: None,
        });
    }
    ir.annotations = annotations.build();
    Ok(ir)
}

fn annotate(
    annotations: &mut AnnotationBuilder,
    id: impl std::fmt::Display,
    source_stream: &str,
    offset: u64,
    tag: &str,
    exactness: Exactness,
) {
    let stream = annotations.stream(format!("creo:{source_stream}"));
    annotations.note(id.to_string(), stream, offset).tag(tag);
    annotations.exactness(id, exactness);
}

fn source_meta(scan: &ContainerScan) -> SourceMeta {
    let mut attributes = BTreeMap::new();
    attributes.insert("version_line".to_string(), scan.version_line.clone());
    attributes.insert("layout".to_string(), scan.layout.token().to_string());
    attributes.insert("file_size".to_string(), scan.data.len().to_string());
    attributes.insert("section_count".to_string(), scan.sections.len().to_string());
    if let Some(c) = scan.census.srf_array_count {
        attributes.insert("srf_array_count".to_string(), c.to_string());
    }
    if let Some(c) = scan.census.crv_array_count {
        attributes.insert("crv_array_count".to_string(), c.to_string());
    }
    if let Some(unit) = &scan.principal_unit {
        attributes.insert("principal_unit".to_string(), unit.clone());
    }
    attributes.insert(
        "decoded_surface_row_count".to_string(),
        scan.surface_rows.len().to_string(),
    );
    attributes.insert(
        "decoded_surface_parameter_record_count".to_string(),
        scan.surface_parameters.len().to_string(),
    );
    attributes.insert(
        "decoded_plane_local_system_count".to_string(),
        scan.plane_local_systems.len().to_string(),
    );
    attributes.insert(
        "decoded_plane_envelope_count".to_string(),
        scan.plane_envelopes.len().to_string(),
    );
    attributes.insert(
        "decoded_surface_prototype_count".to_string(),
        scan.surface_prototypes.len().to_string(),
    );
    attributes.insert(
        "decoded_named_surface_prototype_count".to_string(),
        scan.surface_prototype_records.len().to_string(),
    );
    attributes.insert(
        "decoded_curve_prototype_count".to_string(),
        scan.curve_prototypes.len().to_string(),
    );
    attributes.insert(
        "decoded_curve_parameter_record_count".to_string(),
        scan.curve_parameters.len().to_string(),
    );
    attributes.insert(
        "decoded_pcurve_count".to_string(),
        scan.pcurves.len().to_string(),
    );
    attributes.insert(
        "decoded_fc_curve_control_point_record_count".to_string(),
        scan.fc_curve_control_points.len().to_string(),
    );
    attributes.insert(
        "decoded_fc05_circle_count".to_string(),
        scan.fc05_circles.len().to_string(),
    );
    attributes.insert(
        "decoded_prototype_pcurve_count".to_string(),
        scan.prototype_pcurves.len().to_string(),
    );
    attributes.insert(
        "decoded_curve_prototype_topology_count".to_string(),
        scan.curve_prototype_topology.len().to_string(),
    );
    attributes.insert(
        "decoded_bound_prototype_pcurve_count".to_string(),
        scan.bound_prototype_pcurves.len().to_string(),
    );
    attributes.insert(
        "decoded_curve_topology_row_count".to_string(),
        scan.curve_topology_rows.len().to_string(),
    );
    attributes.insert(
        "decoded_half_edge_count".to_string(),
        scan.half_edges.len().to_string(),
    );
    attributes.insert(
        "decoded_topological_vertex_count".to_string(),
        scan.topological_vertices.len().to_string(),
    );
    attributes.insert(
        "decoded_loop_count".to_string(),
        scan.loops.len().to_string(),
    );
    attributes.insert(
        "decoded_face_component_count".to_string(),
        scan.face_components.len().to_string(),
    );
    attributes.insert(
        "decoded_datum_plane_count".to_string(),
        scan.datum_planes.len().to_string(),
    );
    attributes.insert(
        "decoded_feature_count".to_string(),
        scan.feature_ids.len().to_string(),
    );
    attributes.insert(
        "decoded_feature_row_count".to_string(),
        scan.feature_rows.len().to_string(),
    );
    attributes.insert(
        "decoded_feature_choice_count".to_string(),
        scan.feature_choices.len().to_string(),
    );
    attributes.insert(
        "decoded_feature_choice_field_count".to_string(),
        scan.feature_choice_fields.len().to_string(),
    );
    attributes.insert(
        "decoded_feature_geometry_table_count".to_string(),
        scan.feature_geometry_tables.len().to_string(),
    );
    attributes.insert(
        "decoded_feature_affected_id_array_count".to_string(),
        scan.feature_affected_ids.len().to_string(),
    );
    attributes.insert(
        "decoded_feature_replay_affected_id_count".to_string(),
        scan.feature_replay_affected_ids.len().to_string(),
    );
    attributes.insert(
        "decoded_feature_direction_byte_count".to_string(),
        scan.feature_direction_bytes.len().to_string(),
    );
    attributes.insert(
        "decoded_feature_definition_count".to_string(),
        scan.feature_definitions.len().to_string(),
    );
    attributes.insert(
        "decoded_feature_outline_count".to_string(),
        scan.feature_definitions
            .iter()
            .map(|definition| definition.outlines.len())
            .sum::<usize>()
            .to_string(),
    );
    attributes.insert(
        "decoded_feature_section_point_count".to_string(),
        scan.feature_definitions
            .iter()
            .filter_map(|definition| definition.variables.as_ref())
            .map(|variables| variables.points.len())
            .sum::<usize>()
            .to_string(),
    );
    attributes.insert(
        "decoded_feature_segment_count".to_string(),
        scan.feature_definitions
            .iter()
            .filter_map(|definition| definition.segments.as_ref())
            .map(|segments| segments.rows.len())
            .sum::<usize>()
            .to_string(),
    );
    attributes.insert(
        "decoded_feature_trim_entity_count".to_string(),
        scan.feature_definitions
            .iter()
            .filter_map(|definition| definition.trim_entities.as_ref())
            .map(|entities| entities.rows.len())
            .sum::<usize>()
            .to_string(),
    );
    attributes.insert(
        "decoded_feature_trim_vertex_count".to_string(),
        scan.feature_definitions
            .iter()
            .filter_map(|definition| definition.trim_vertices.as_ref())
            .map(|vertices| vertices.rows.len())
            .sum::<usize>()
            .to_string(),
    );
    attributes.insert(
        "decoded_feature_order_entry_count".to_string(),
        scan.feature_definitions
            .iter()
            .filter_map(|definition| definition.order_table.as_ref())
            .map(|order| order.rows.len())
            .sum::<usize>()
            .to_string(),
    );
    attributes.insert(
        "decoded_feature_dimension_count".to_string(),
        scan.feature_definitions
            .iter()
            .filter_map(|definition| definition.dimensions.as_ref())
            .map(|dimensions| dimensions.rows.len())
            .sum::<usize>()
            .to_string(),
    );
    attributes.insert(
        "decoded_feature_saved_entity_count".to_string(),
        scan.feature_definitions
            .iter()
            .filter_map(|definition| definition.saved_section.as_ref())
            .map(|saved| saved.entities.len())
            .sum::<usize>()
            .to_string(),
    );
    attributes.insert(
        "decoded_feature_entity_count".to_string(),
        scan.feature_entities.len().to_string(),
    );
    attributes.insert(
        "decoded_feature_entity_reference_count".to_string(),
        scan.feature_entity_references.len().to_string(),
    );
    attributes.insert(
        "decoded_feature_entity_table_count".to_string(),
        scan.feature_entity_tables.len().to_string(),
    );
    if let Some(count) = scan.declared_body_count {
        attributes.insert("declared_body_count".to_string(), count.to_string());
    }
    if let Some(value) = scan.first_quilt_ptr {
        attributes.insert("first_quilt_ptr".to_string(), value.to_string());
    }
    SourceMeta {
        format: "creo".to_string(),
        attributes,
    }
}

/// Build diagnostics for data that cannot be represented in the emitted IR.
fn build_report(scan: &ContainerScan, container_only: bool) -> DecodeReport {
    let summary = container::summarize(scan);
    let geom_sections = scan
        .sections
        .iter()
        .filter(|s| s.role == role::GEOMETRY)
        .count();

    let mut losses = Vec::new();

    if container_only {
        losses.push(LossNote {
            category: LossCategory::Geometry,
            severity: Severity::Info,
            message: "Container-only decode requested; only the container layer was read."
                .to_string(),
            provenance: None,
        });
    }

    // The namespace census: what is byte-backed and readable.
    let srf = scan
        .census
        .srf_array_count
        .map_or_else(|| "n/a".to_string(), |c| c.to_string());
    let crv = scan
        .census
        .crv_array_count
        .map_or_else(|| "n/a".to_string(), |c| c.to_string());
    losses.push(LossNote {
        category: LossCategory::Geometry,
        severity: Severity::Info,
        message: format!(
            "PSB container decoded structurally: {} section(s), {} layout, VisibGeom namespace \
             census srf_array={srf} / crv_array={crv}; {} typed surface rows, {} labeled curve \
             prototypes, {} canonical curve-topology rows, and {} closed native loops were decoded. \
             Per-instance parameter bodies remain outside the transferred IR carriers.",
            scan.sections.len(),
            scan.layout.token(),
            scan.surface_rows.len(),
            scan.curve_prototypes.len(),
            scan.curve_topology_rows.len(),
            scan.loops.len(),
        ),
        provenance: None,
    });

    // The core prototype-vs-instance limitation.
    losses.push(LossNote {
        category: LossCategory::Geometry,
        severity: Severity::Blocking,
        message: format!(
            "No model B-rep geometry was transferred. VisibGeom stores one surface prototype per family \
             (a first-instance template), not per-instance located geometry, so its prototype \
             scalars cannot be emitted as model surfaces without mislabeling most instances \
             ([spec §4.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#32-surface-prototypes)). {geom_sections} PSB geometry section(s) were preserved verbatim as unknown \
             records."
        ),
        provenance: None,
    });

    if !scan.datum_planes.is_empty() {
        losses.push(LossNote {
            category: LossCategory::Geometry,
            severity: Severity::Info,
            message: format!(
                "Transferred {} exact model-space construction datum plane carrier(s) from ActDatums; \
                 these are unbounded reference planes, not model B-rep faces.",
                scan.datum_planes.len()
            ),
            provenance: None,
        });
    }

    // The specific undecoded PSB layers that gate per-instance geometry.
    losses.push(LossNote {
        category: LossCategory::Geometry,
        severity: Severity::Blocking,
        message: "Per-instance model-space coordinates are gated behind undecoded PSB layers: the \
                  general 8-byte world-coordinate float token (only the `0x46`/`0x2d` prefixes are \
                  characterized), the `0x26` per-instance torus/sphere override region, and the \
                  round/fillet feature evaluator that generates most non-planar faces. None were \
                  decoded, so no per-instance surfaces, curves, or vertices were produced."
            .to_string(),
        provenance: None,
    });

    // Topology.
    losses.push(LossNote {
        category: LossCategory::Topology,
        severity: Severity::Blocking,
        message: "Native curve half-edges and closed loops were decoded, but the IR B-rep graph \
                  (body/region/shell/face/loop/coedge/edge/vertex) was not emitted: face-instance \
                  partitioning, surface parameter bindings, curve geometry, and vertex coordinates \
                  remain incomplete."
            .to_string(),
        provenance: None,
    });

    // Features, history, materials.
    losses.push(LossNote {
        category: LossCategory::Attribute,
        severity: Severity::Warning,
        message: "Features, feature history, datums, section recipes, materials, and display data \
                  were not transferred."
            .to_string(),
        provenance: None,
    });

    DecodeReport {
        format: "creo".to_string(),
        container_only,
        geometry_transferred: !scan.datum_planes.is_empty(),
        losses,
        notes: summary.notes,
    }
}
