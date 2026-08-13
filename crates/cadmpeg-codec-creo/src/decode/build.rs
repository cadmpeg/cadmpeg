// SPDX-License-Identifier: Apache-2.0
//! IR assembly, native arena emission, coverage, and decode report.

#[allow(clippy::wildcard_imports)]
use super::*;

pub(super) fn preserve_passthrough_sections(
    scan: &ContainerScan,
    annotations: &mut AnnotationBuilder,
) -> Vec<UnknownRecord> {
    let mut unknowns = Vec::new();
    for section in scan
        .framing
        .sections
        .iter()
        .filter(|section| section.role == role::GEOMETRY || section.role == role::THUMBNAIL)
    {
        let end = (section.offset + section.length).min(scan.framing.data.len());
        let section_bytes = &scan.framing.data[section.offset..end];
        let payload_start = section.raw_name.len().saturating_add(2);
        let raw_is_compressed = section_bytes
            .get(payload_start..)
            .is_some_and(|payload| payload.starts_with(container::UNIX_COMPRESS_MAGIC));
        let (bytes, offset, tag, exactness) = if section.role == role::THUMBNAIL {
            if raw_is_compressed {
                let Some(expanded) = container::expanded_section_for(scan, section) else {
                    continue;
                };
                let Some(marker_offset) = expanded
                    .data
                    .windows(3)
                    .position(|window| window == container::JPEG_MAGIC)
                else {
                    continue;
                };
                (
                    &expanded.data[marker_offset..],
                    expanded.source_offset,
                    "jpeg_thumbnail",
                    Exactness::Derived,
                )
            } else {
                let Some(marker_offset) = section_bytes
                    .windows(3)
                    .position(|window| window == container::JPEG_MAGIC)
                else {
                    continue;
                };
                (
                    &section_bytes[marker_offset..],
                    section.offset.saturating_add(marker_offset),
                    "jpeg_thumbnail",
                    Exactness::ByteExact,
                )
            }
        } else {
            (
                section_bytes,
                section.offset,
                "psb_geometry_section",
                Exactness::Unknown,
            )
        };
        let id = UnknownId(format!("creo:{}:section#{}", section.name, offset));
        annotate(
            annotations,
            &id,
            &section.name,
            offset as u64,
            tag,
            exactness,
        );
        unknowns.push(UnknownRecord {
            id,
            offset: offset as u64,
            byte_len: bytes.len() as u64,
            sha256: sha256_hex(bytes),
            data: Some(bytes.to_vec()),
            links: Vec::new(),
        });
    }
    unknowns
}

/// Decoded IR together with its annotations, preserved unknown records, and
/// decode-coverage counts.
pub(super) struct BuiltIr {
    pub(super) ir: CadIr,
    pub(super) annotations: cadmpeg_ir::Annotations,
    pub(super) unknowns: Vec<UnknownRecord>,
    pub(super) coverage: BTreeMap<String, usize>,
}

pub(super) fn legacy_source_stream<'a>(scan: &'a ContainerScan<'_>, offset: usize) -> &'a str {
    scan.framing
        .sections
        .iter()
        .find(|section| {
            offset >= section.offset && offset < section.offset.saturating_add(section.length)
        })
        .map_or("legacy_ascii", |section| section.name.as_str())
}

pub(super) fn emit_legacy_value_arena<T: Serialize>(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    key: &str,
    records: &[crate::legacy::ValueRecord<T>],
    tag: &str,
) -> Result<(), CodecError> {
    emit_arena(ir, annotations, key, records, |annotations, record| {
        annotate(
            annotations,
            &record.id,
            legacy_source_stream(scan, record.offset),
            record.offset as u64,
            tag,
            Exactness::ByteExact,
        );
    })
}

pub(super) fn emit_legacy_arenas(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> Result<(), CodecError> {
    let Some(legacy) = &scan.framing.legacy_ascii else {
        return Ok(());
    };
    emit_arena(
        ir,
        annotations,
        "legacy_objects",
        &legacy.persistence.objects,
        |annotations, record| {
            annotate(
                annotations,
                &record.id,
                legacy_source_stream(scan, record.offset),
                record.offset as u64,
                "legacy_type_0_object",
                Exactness::ByteExact,
            );
        },
    )?;
    emit_legacy_value_arena(
        scan,
        ir,
        annotations,
        "legacy_integer_values",
        &legacy.persistence.integer_values,
        "legacy_type_1_integer",
    )?;
    emit_legacy_value_arena(
        scan,
        ir,
        annotations,
        "legacy_real_values",
        &legacy.persistence.real_values,
        "legacy_type_2_real",
    )?;
    emit_legacy_value_arena(
        scan,
        ir,
        annotations,
        "legacy_type_3_values",
        &legacy.persistence.type_3_values,
        "legacy_type_3_value",
    )?;
    emit_legacy_value_arena(
        scan,
        ir,
        annotations,
        "legacy_type_4_values",
        &legacy.persistence.type_4_values,
        "legacy_type_4_value",
    )?;
    emit_legacy_value_arena(
        scan,
        ir,
        annotations,
        "legacy_string_values",
        &legacy.persistence.string_values,
        "legacy_type_10_string",
    )?;
    emit_legacy_value_arena(
        scan,
        ir,
        annotations,
        "legacy_type_5_values",
        &legacy.persistence.type_5_values,
        "legacy_type_5_value",
    )?;
    emit_legacy_value_arena(
        scan,
        ir,
        annotations,
        "legacy_type_6_values",
        &legacy.persistence.type_6_values,
        "legacy_type_6_value",
    )?;
    emit_legacy_value_arena(
        scan,
        ir,
        annotations,
        "legacy_type_7_values",
        &legacy.persistence.type_7_values,
        "legacy_type_7_value",
    )?;
    emit_legacy_value_arena(
        scan,
        ir,
        annotations,
        "legacy_type_9_values",
        &legacy.persistence.type_9_values,
        "legacy_type_9_value",
    )?;
    emit_legacy_value_arena(
        scan,
        ir,
        annotations,
        "legacy_type_11_values",
        &legacy.persistence.type_11_values,
        "legacy_type_11_value",
    )
}

pub(super) fn build_container_ir(scan: &ContainerScan) -> Result<BuiltIr, CodecError> {
    let mut ir = CadIr::empty(Units::default());
    let mut annotations = AnnotationBuilder::new();
    let (meta, coverage) = source_meta(scan);
    ir.source = Some(meta);
    emit_legacy_arenas(scan, &mut ir, &mut annotations)?;
    let unknowns = preserve_passthrough_sections(scan, &mut annotations);
    attach_expanded_sections(scan, &mut ir, &mut annotations)?;
    Ok(BuiltIr {
        ir,
        annotations: annotations.build(),
        unknowns,
        coverage,
    })
}

pub(super) fn face_selection_has_unresolved_operands(selection: &FaceSelection) -> bool {
    matches!(
        selection,
        FaceSelection::Unresolved
            | FaceSelection::HistoricalPartial { .. }
            | FaceSelection::Native(_)
    )
}

pub(super) fn body_selection_has_unresolved_operands(selection: &BodySelection) -> bool {
    matches!(
        selection,
        BodySelection::Unresolved | BodySelection::Native(_) | BodySelection::NativeSet(_)
    )
}

pub(super) fn edge_selection_has_unresolved_operands(selection: &EdgeSelection) -> bool {
    matches!(
        selection,
        EdgeSelection::Unresolved
            | EdgeSelection::HistoricalPartial { .. }
            | EdgeSelection::Native(_)
    )
}

pub(super) fn path_has_unresolved_operands(path: &PathRef) -> bool {
    matches!(
        path,
        PathRef::Unresolved(_) | PathRef::Native(_) | PathRef::SpatialSketchSelection { .. }
    )
}

pub(super) fn surface_boundary_has_unresolved_operands(boundary: &SurfaceBoundary) -> bool {
    match boundary {
        SurfaceBoundary::Edges(edges) => edge_selection_has_unresolved_operands(edges),
        SurfaceBoundary::Path(path) => path_has_unresolved_operands(path),
    }
}

pub(super) fn pattern_kind_has_unresolved_operands(pattern: &PatternKind) -> bool {
    match pattern {
        PatternKind::Unresolved { .. } => true,
        PatternKind::Linear { direction, .. } | PatternKind::LinearOffsets { direction, .. } => {
            direction.is_none()
        }
        PatternKind::CurveDriven { path, .. } => {
            path.as_ref().is_none_or(path_has_unresolved_operands)
        }
        PatternKind::Scale { center, .. } => {
            matches!(center, cadmpeg_ir::features::PatternScaleCenter::Native(_))
        }
        PatternKind::Composite { stages } => {
            stages.is_empty()
                || stages
                    .iter()
                    .any(|stage| pattern_kind_has_unresolved_operands(&stage.pattern))
        }
        PatternKind::Circular { .. }
        | PatternKind::CircularAngles { .. }
        | PatternKind::Mirror { .. } => false,
        PatternKind::MirrorReference { .. } => true,
    }
}

pub(super) fn termination_has_unresolved_operands(termination: &Termination) -> bool {
    match termination {
        Termination::Unresolved => true,
        Termination::ToFace { face, .. }
        | Termination::OffsetFromFace { face, .. }
        | Termination::ToShape { target: face } => face_selection_has_unresolved_operands(face),
        Termination::ToVertex { vertex } => {
            matches!(
                vertex,
                VertexSelection::Unresolved | VertexSelection::Native(_)
            )
        }
        Termination::Blind { .. }
        | Termination::ThroughAll
        | Termination::ThroughNext
        | Termination::ToFirst
        | Termination::ToLast
        | Termination::Angle { .. } => false,
    }
}

/// Build source metadata, preserved geometry records, and transferred entities.
pub(super) fn build_ir(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<BuiltIr, CodecError> {
    let mut ir = CadIr::empty(Units::default());
    let mut annotations = AnnotationBuilder::new();
    let (meta, mut coverage) = source_meta(scan);
    ir.source = Some(meta);
    emit_legacy_arenas(scan, &mut ir, &mut annotations)?;
    let unknowns = preserve_passthrough_sections(scan, &mut annotations);
    emit_reference_arenas(scan, &mut ir, &mut annotations)?;
    let line3d_id_counts =
        scan.references
            .lines
            .iter()
            .fold(BTreeMap::<u32, usize>::new(), |mut counts, line| {
                if let crate::reference::ReferenceLineKind::Line3d { entity_id, .. } = &line.kind {
                    *counts.entry(*entity_id).or_default() += 1;
                }
                counts
            });
    for line in &scan.references.lines {
        let direction = std::array::from_fn(|axis| line.end[axis] - line.start[axis]);
        let Some(direction) = normalized(direction) else {
            continue;
        };
        let (family, native_identity) = match &line.kind {
            crate::reference::ReferenceLineKind::Line => ("line", line.offset.to_string()),
            crate::reference::ReferenceLineKind::Line3d { entity_id, .. } => {
                let identity = if line3d_id_counts.get(entity_id) == Some(&1) {
                    entity_id.to_string()
                } else {
                    format!("{entity_id}@{}", line.offset)
                };
                ("line3d", identity)
            }
        };
        let prefix = format!("creo:mdl_ref_info:{family}#{native_identity}");
        let id = CurveId(prefix);
        annotate(
            &mut annotations,
            &id,
            "MdlRefInfo",
            line.offset as u64,
            "reference_line",
            Exactness::Derived,
        );
        ir.model.curves.push(Curve {
            id,
            geometry: CurveGeometry::Line {
                origin: Point3::new(line.start[0], line.start[1], line.start[2]),
                direction: Vector3::new(direction[0], direction[1], direction[2]),
            },
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("MdlRefInfo:{family}:{native_identity}"),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
    }
    let circle_id_counts =
        scan.references
            .circles
            .iter()
            .fold(BTreeMap::<u32, usize>::new(), |mut counts, circle| {
                *counts.entry(circle.entity_id).or_default() += 1;
                counts
            });
    for circle in &scan.references.circles {
        let radial = std::array::from_fn(|axis| circle.start[axis] - circle.center[axis]);
        let Some(reference) = normalized(radial) else {
            continue;
        };
        let native_identity = if circle_id_counts.get(&circle.entity_id) == Some(&1) {
            circle.entity_id.to_string()
        } else {
            format!("{}@{}", circle.entity_id, circle.offset)
        };
        let id = CurveId(format!("creo:mdl_ref_info:arc_z#{native_identity}"));
        annotate(
            &mut annotations,
            &id,
            "MdlRefInfo",
            circle.offset as u64,
            "reference_circle",
            Exactness::Derived,
        );
        ir.model.curves.push(Curve {
            id,
            geometry: CurveGeometry::Circle {
                center: Point3::new(circle.center[0], circle.center[1], circle.center[2]),
                axis: Vector3::new(circle.axis[0], circle.axis[1], circle.axis[2]),
                ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                radius: circle.radius,
            },
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("MdlRefInfo:arc_z:{native_identity}"),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
    }
    let ellipse_id_counts = scan.references.ellipses.iter().fold(
        BTreeMap::<u32, usize>::new(),
        |mut counts, ellipse| {
            *counts.entry(ellipse.source_entity_id).or_default() += 1;
            counts
        },
    );
    for ellipse in &scan.references.ellipses {
        let native_identity = if ellipse_id_counts.get(&ellipse.source_entity_id) == Some(&1) {
            ellipse.source_entity_id.to_string()
        } else {
            format!("{}@{}", ellipse.source_entity_id, ellipse.offset)
        };
        let id = CurveId(format!("creo:mdl_ref_info:conic#{native_identity}"));
        annotate(
            &mut annotations,
            &id,
            "MdlRefInfo",
            ellipse.offset as u64,
            "reference_ellipse",
            Exactness::Derived,
        );
        ir.model.curves.push(Curve {
            id,
            geometry: CurveGeometry::Ellipse {
                center: Point3::new(ellipse.center[0], ellipse.center[1], ellipse.center[2]),
                axis: Vector3::new(ellipse.axis[0], ellipse.axis[1], ellipse.axis[2]),
                major_direction: Vector3::new(
                    ellipse.major_direction[0],
                    ellipse.major_direction[1],
                    ellipse.major_direction[2],
                ),
                major_radius: ellipse.major_radius,
                minor_radius: ellipse.minor_radius,
            },
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("MdlRefInfo:conic:{native_identity}"),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
    }
    for strip in &scan.primitives.triangle_strips {
        let id = format!("creo:solid_primdata:tessellation#{}", strip.offset);
        let mut triangles = Vec::new();
        let mut base = 0u32;
        for length in &strip.strip_lengths {
            for index in 0..length.saturating_sub(2) {
                let a = base + index;
                let triangle = if index % 2 == 0 {
                    [a, a + 1, a + 2]
                } else {
                    [a, a + 2, a + 1]
                };
                triangles.push(triangle);
            }
            base += length;
        }
        annotate(
            &mut annotations,
            &id,
            "SolidPrimdata",
            strip.offset as u64,
            "display_triangle_strip",
            Exactness::Derived,
        );
        ir.model.tessellations.push(Tessellation {
            id,
            body: None,
            faces: Vec::new(),
            chordal_deflection: None,
            source_object: None,
            vertices: strip
                .positions
                .iter()
                .map(|point| Point3::new(point[0], point[1], point[2]))
                .collect(),
            triangles,
            feature_edges: Vec::new(),
            strip_lengths: strip.strip_lengths.clone(),
            normals: strip
                .normals
                .iter()
                .map(|normal| Vector3::new(normal[0], normal[1], normal[2]))
                .collect(),
            corner_normals: Vec::new(),
            triangle_groups: Vec::new(),
            texture_assignments: Vec::new(),
            channels: Vec::new(),
        });
    }
    for plane in &scan.planes.datums {
        let id = SurfaceId(format!("creo:actdatums:surface#{}", plane.id));
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
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("ActDatums:{}", plane.id),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
    }
    for (surface_id, (plane, u_axis, offset)) in placed_plane_surfaces(scan) {
        let id = SurfaceId(format!("creo:visibgeom:surface#{surface_id}"));
        if ir.model.surfaces.iter().any(|surface| surface.id == id) {
            continue;
        }
        let tag = if scan
            .planes
            .positional_frames
            .iter()
            .any(|plane| plane.surface_id == surface_id && plane.offset == offset)
        {
            "plane_positional_corner_frame"
        } else if scan
            .planes
            .outlines
            .iter()
            .any(|outline| outline.surface_id == surface_id && outline.offset == offset)
        {
            "plane_outline_held_coordinate"
        } else {
            "plane_local_system"
        };
        annotate(
            &mut annotations,
            &id,
            "VisibGeom",
            offset as u64,
            tag,
            Exactness::Derived,
        );
        ir.model.surfaces.push(Surface {
            id,
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(plane.origin[0], plane.origin[1], plane.origin[2]),
                normal: Vector3::new(plane.normal[0], plane.normal[1], plane.normal[2]),
                u_axis: Vector3::new(u_axis[0], u_axis[1], u_axis[2]),
            },
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("VisibGeom:{surface_id}"),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
    }
    let cross_section_plane_count = transfer_cross_section_planes(scan, &mut ir, &mut annotations);
    let first_instance_prototype_surface_count =
        transfer_first_instance_prototype_surfaces(scan, &mut ir, &mut annotations);
    let paired_envelope_sphere_count =
        transfer_paired_envelope_spheres(scan, &mut ir, &mut annotations);
    let positional_torus_count = transfer_positional_tori(scan, &mut ir, &mut annotations);
    let positional_line_extrusion_plane_count =
        transfer_positional_line_extrusion_planes(scan, &mut ir, &mut annotations);
    let tabulated_cylinder_spline_extrusion_count =
        transfer_tabulated_cylinder_spline_extrusions(scan, &mut ir, &mut annotations);
    transfer_fc05_cap_circles(scan, &mut ir, &mut annotations);
    transfer_cap_pair_cylinders(scan, &mut ir, &mut annotations);
    let saved_spline_curve_count = transfer_saved_spline_curves(scan, &mut ir, &mut annotations);
    let sketch_segment_coverage = transfer_sketches(scan, &mut ir, &mut annotations);
    let feature_revolution_surface_count =
        transfer_resolved_revolution_surfaces(scan, &mut ir, &mut annotations);
    let feature_revolution_vertex_orbit_curve_count =
        transfer_resolved_revolution_vertex_orbit_curves(scan, &mut ir, &mut annotations);
    let feature_extrusion_surface_count =
        transfer_feature_extrusion_surfaces(scan, &mut ir, &mut annotations);
    let feature_extrusion_vertex_orbit_curve_count =
        transfer_resolved_extrusion_vertex_orbit_curves(scan, &mut ir, &mut annotations);
    let circular_sweep_cylinder_count =
        transfer_circular_sweep_cylinders(scan, &mut ir, &mut annotations);
    let positional_cylinder_count = transfer_positional_cylinders(scan, &mut ir, &mut annotations);
    let positional_cone_count = transfer_positional_cones(scan, &mut ir, &mut annotations);
    let split_outline_cylinder_count =
        transfer_split_outline_cylinders(scan, &mut ir, &mut annotations);
    let hole_cylinder_count = transfer_hole_cylinders(scan, &mut ir, &mut annotations);
    let constrained_slot_fillet_cylinder_count =
        transfer_constrained_slot_fillet_cylinders(scan, &mut ir, &mut annotations);
    let rowless_round_cylinder_count =
        transfer_rowless_round_cylinders(scan, &mut ir, &mut annotations);
    let analytic_pcurve_carriers =
        transfer_analytic_pcurve_carriers(scan, &mut ir, &mut annotations);
    let analytic_pcurve_carrier_count = analytic_pcurve_carriers.len();
    let mut derived_intersection_curves =
        transfer_carrier_intersection_curves(scan, &mut ir, &mut annotations);
    let nurbs_boundary_curves =
        transfer_nurbs_boundary_curves(ctx, scan, &mut ir, &mut annotations)?;
    let extrusion_plane_boundary_curve_count = nurbs_boundary_curves.extrusion_plane_count;
    let extrusion_plane_section_generator_curve_count =
        nurbs_boundary_curves.extrusion_plane_section_generator_count;
    let shared_extrusion_generator_curve_count =
        nurbs_boundary_curves.shared_extrusion_generator_count;
    derived_intersection_curves.extend(nurbs_boundary_curves.ids.iter().cloned());
    let topology_bound_plane_count = transfer_topology_bound_planes(
        scan,
        &mut ir,
        &mut annotations,
        &nurbs_boundary_curves.endpoint_witnesses,
    );
    let (topological_point_count, native_topological_edge_count) = transfer_native_brep(
        scan,
        &mut ir,
        &mut annotations,
        &derived_intersection_curves,
        &analytic_pcurve_carriers,
        &nurbs_boundary_curves.endpoint_witnesses,
    );
    let feature_revolution_brep_count =
        transfer_resolved_revolution_breps(scan, &mut ir, &mut annotations);
    let feature_circular_extrusion_brep_count =
        transfer_resolved_circular_extrusion_breps(scan, &mut ir, &mut annotations);
    let feature_extrusion_brep_count =
        transfer_resolved_extrusion_breps(scan, &mut ir, &mut annotations);
    retain_unresolved_visible_carriers(scan, &mut ir, &mut annotations);
    let transferred_part_product = transfer_part_product(scan, &mut ir, &mut annotations);
    let decoded_feature_skamp_count = scan
        .features
        .definitions
        .iter()
        .filter_map(|definition| definition.relations.as_ref())
        .map(|relations| relations.skamps.len())
        .sum::<usize>();
    let missing_feature_skamp_row_count = scan
        .features
        .definitions
        .iter()
        .filter_map(|definition| definition.relations.as_ref())
        .map(|relations| {
            feature_solver_table_missing_rows(
                relations.skamp_header.as_ref(),
                relations.skamps.len(),
            )
        })
        .sum::<usize>();
    let skamp_constraint_coverage =
        design_constraint_transfer_coverage(&ir.model.sketch_constraints, ":skamp:", "creo:skamp:");
    let decoded_feature_relation_count = scan
        .features
        .definitions
        .iter()
        .filter_map(|definition| definition.relations.as_ref())
        .map(|relations| relations.rows.len())
        .sum::<usize>();
    let missing_feature_relation_row_count = scan
        .features
        .definitions
        .iter()
        .filter_map(|definition| definition.relations.as_ref())
        .map(feature_relation_table_missing_rows)
        .sum::<usize>();
    let malformed_feature_relation_table_count = scan
        .features
        .definitions
        .iter()
        .filter_map(|definition| definition.relations.as_ref())
        .filter(|relations| feature_relation_table_expected_rows(relations).is_none())
        .count();
    let decoded_feature_relation_triple_count = scan
        .features
        .definitions
        .iter()
        .filter_map(|definition| definition.relations.as_ref())
        .map(|relations| relations.triples.len())
        .sum::<usize>();
    let missing_feature_relation_triple_row_count = scan
        .features
        .definitions
        .iter()
        .filter_map(|definition| definition.relations.as_ref())
        .map(|relations| {
            feature_solver_table_missing_rows(
                relations.triples_header.as_ref(),
                relations.triples.len(),
            )
        })
        .sum::<usize>();
    let relation_constraint_coverage = design_constraint_transfer_coverage(
        &ir.model.sketch_constraints,
        ":relation:",
        "creo:relation:",
    );
    let surface_coverage = surface_transfer_coverage(
        &scan.surfaces.rows,
        &ir.model.surfaces,
        &ir.model.procedural_surfaces,
    );
    let curve_coverage = curve_transfer_coverage(&scan.curves.topology_rows, &ir.model.curves);
    {
        coverage.insert(
            "unique_visible_surface_row_count".to_string(),
            surface_coverage.unique_rows,
        );
        coverage.insert(
            "transferred_visible_surface_row_count".to_string(),
            surface_coverage.transferred_rows,
        );
        coverage.insert(
            "retained_unknown_visible_surface_row_count".to_string(),
            surface_coverage.retained_unknown_rows,
        );
        coverage.insert(
            "untransferred_visible_surface_row_count".to_string(),
            surface_coverage
                .unique_rows
                .saturating_sub(surface_coverage.transferred_rows),
        );
        coverage.insert(
            "ambiguous_visible_surface_row_count".to_string(),
            surface_coverage.ambiguous_rows,
        );
        for (family, (rows, transferred)) in &surface_coverage.by_family {
            coverage.insert(format!("visible_{family}_surface_row_count"), *rows);
            coverage.insert(
                format!("transferred_visible_{family}_surface_row_count"),
                *transferred,
            );
            coverage.insert(
                format!("untransferred_visible_{family}_surface_row_count"),
                rows.saturating_sub(*transferred),
            );
            coverage.insert(
                format!("retained_unknown_visible_{family}_surface_row_count"),
                surface_coverage
                    .unknown_by_family
                    .get(family)
                    .copied()
                    .unwrap_or_default(),
            );
        }
        coverage.insert(
            "unique_visible_curve_row_count".to_string(),
            curve_coverage.unique_rows,
        );
        coverage.insert(
            "transferred_visible_curve_row_count".to_string(),
            curve_coverage.transferred_rows,
        );
        coverage.insert(
            "retained_unknown_visible_curve_row_count".to_string(),
            curve_coverage.retained_unknown_rows,
        );
        coverage.insert(
            "untransferred_visible_curve_row_count".to_string(),
            curve_coverage
                .unique_rows
                .saturating_sub(curve_coverage.transferred_rows),
        );
        coverage.insert(
            "ambiguous_visible_curve_row_count".to_string(),
            curve_coverage.ambiguous_rows,
        );
        for (type_byte, (rows, transferred)) in &curve_coverage.by_type {
            coverage.insert(
                format!("visible_curve_type_{type_byte:02x}_row_count"),
                *rows,
            );
            coverage.insert(
                format!("transferred_visible_curve_type_{type_byte:02x}_row_count"),
                *transferred,
            );
            coverage.insert(
                format!("retained_unknown_visible_curve_type_{type_byte:02x}_row_count"),
                curve_coverage
                    .unknown_by_type
                    .get(type_byte)
                    .copied()
                    .unwrap_or_default(),
            );
        }
        coverage.insert(
            "transferred_cross_section_plane_count".to_string(),
            cross_section_plane_count,
        );
        coverage.insert(
            "transferred_first_instance_prototype_surface_count".to_string(),
            first_instance_prototype_surface_count,
        );
        coverage.insert(
            "transferred_paired_envelope_sphere_count".to_string(),
            paired_envelope_sphere_count,
        );
        coverage.insert(
            "transferred_positional_torus_count".to_string(),
            positional_torus_count,
        );
        coverage.insert(
            "transferred_positional_line_extrusion_plane_count".to_string(),
            positional_line_extrusion_plane_count,
        );
        coverage.insert(
            "transferred_tabulated_cylinder_spline_extrusion_count".to_string(),
            tabulated_cylinder_spline_extrusion_count,
        );
        coverage.insert(
            "transferred_saved_spline_curve_count".to_string(),
            saved_spline_curve_count,
        );
        coverage.insert(
            "transferred_topological_point_count".to_string(),
            topological_point_count,
        );
        coverage.insert(
            "transferred_native_topological_edge_count".to_string(),
            native_topological_edge_count,
        );
        coverage.insert(
            "transferred_analytic_pcurve_carrier_count".to_string(),
            analytic_pcurve_carrier_count,
        );
        coverage.insert(
            "transferred_extrusion_plane_boundary_curve_count".to_string(),
            extrusion_plane_boundary_curve_count,
        );
        coverage.insert(
            "transferred_extrusion_plane_section_generator_curve_count".to_string(),
            extrusion_plane_section_generator_curve_count,
        );
        coverage.insert(
            "transferred_shared_extrusion_generator_curve_count".to_string(),
            shared_extrusion_generator_curve_count,
        );
        coverage.insert(
            "transferred_topology_bound_plane_surface_count".to_string(),
            topology_bound_plane_count,
        );
        coverage.insert(
            "transferred_feature_revolution_surface_count".to_string(),
            feature_revolution_surface_count,
        );
        coverage.insert(
            "transferred_feature_revolution_vertex_orbit_curve_count".to_string(),
            feature_revolution_vertex_orbit_curve_count,
        );
        coverage.insert(
            "transferred_feature_extrusion_surface_count".to_string(),
            feature_extrusion_surface_count,
        );
        coverage.insert(
            "transferred_feature_extrusion_vertex_orbit_curve_count".to_string(),
            feature_extrusion_vertex_orbit_curve_count,
        );
        coverage.insert(
            "transferred_circular_sweep_cylinder_count".to_string(),
            circular_sweep_cylinder_count,
        );
        coverage.insert(
            "transferred_hole_cylinder_count".to_string(),
            hole_cylinder_count,
        );
        coverage.insert(
            "transferred_positional_cylinder_count".to_string(),
            positional_cylinder_count,
        );
        coverage.insert(
            "transferred_positional_cone_count".to_string(),
            positional_cone_count,
        );
        coverage.insert(
            "transferred_split_outline_cylinder_count".to_string(),
            split_outline_cylinder_count,
        );
        coverage.insert(
            "transferred_constrained_slot_fillet_cylinder_count".to_string(),
            constrained_slot_fillet_cylinder_count,
        );
        coverage.insert(
            "transferred_rowless_round_cylinder_count".to_string(),
            rowless_round_cylinder_count,
        );
        coverage.insert(
            "transferred_feature_revolution_brep_count".to_string(),
            feature_revolution_brep_count,
        );
        coverage.insert(
            "transferred_feature_circular_extrusion_brep_count".to_string(),
            feature_circular_extrusion_brep_count,
        );
        coverage.insert(
            "transferred_feature_extrusion_brep_count".to_string(),
            feature_extrusion_brep_count,
        );
        coverage.insert(
            "transferred_part_product_count".to_string(),
            usize::from(transferred_part_product),
        );
        coverage.insert(
            "decoded_feature_segment_row_count".to_string(),
            sketch_segment_coverage.decoded_rows,
        );
        coverage.insert(
            "resolved_feature_segment_geometry_count".to_string(),
            sketch_segment_coverage.resolved_geometry,
        );
        coverage.insert(
            "unresolved_feature_segment_geometry_count".to_string(),
            sketch_segment_coverage
                .decoded_rows
                .saturating_sub(sketch_segment_coverage.resolved_geometry),
        );
        for (family, (decoded, resolved)) in &sketch_segment_coverage.by_family {
            coverage.insert(format!("decoded_feature_{family}_segment_count"), *decoded);
            coverage.insert(
                format!("resolved_feature_{family}_segment_geometry_count"),
                *resolved,
            );
            coverage.insert(
                format!("unresolved_feature_{family}_segment_geometry_count"),
                decoded.saturating_sub(*resolved),
            );
        }
        coverage.insert(
            "missing_feature_segment_row_count".to_string(),
            sketch_segment_coverage.missing_rows,
        );
        coverage.insert(
            "decoded_feature_skamp_count".to_string(),
            decoded_feature_skamp_count,
        );
        coverage.insert(
            "missing_feature_skamp_row_count".to_string(),
            missing_feature_skamp_row_count,
        );
        coverage.insert(
            "transferred_feature_skamp_constraint_count".to_string(),
            skamp_constraint_coverage.transferred,
        );
        coverage.insert(
            "transferred_native_feature_skamp_constraint_count".to_string(),
            skamp_constraint_coverage.native,
        );
        coverage.insert(
            "transferred_typed_feature_skamp_constraint_count".to_string(),
            skamp_constraint_coverage.typed(),
        );
        coverage.insert(
            "active_feature_skamp_constraint_count".to_string(),
            skamp_constraint_coverage.active,
        );
        coverage.insert(
            "active_native_feature_skamp_constraint_count".to_string(),
            skamp_constraint_coverage.active_native,
        );
        coverage.insert(
            "active_typed_feature_skamp_constraint_count".to_string(),
            skamp_constraint_coverage.active_typed(),
        );
        for (kind, count) in &skamp_constraint_coverage.native_by_kind {
            coverage.insert(
                format!("transferred_native_feature_skamp_type_{kind}_constraint_count"),
                *count,
            );
        }
        for (kind, count) in &skamp_constraint_coverage.active_native_by_kind {
            coverage.insert(
                format!("active_native_feature_skamp_type_{kind}_constraint_count"),
                *count,
            );
        }
        coverage.insert(
            "decoded_feature_relation_count".to_string(),
            decoded_feature_relation_count,
        );
        coverage.insert(
            "missing_feature_relation_row_count".to_string(),
            missing_feature_relation_row_count,
        );
        coverage.insert(
            "malformed_feature_relation_table_count".to_string(),
            malformed_feature_relation_table_count,
        );
        coverage.insert(
            "decoded_feature_relation_triple_count".to_string(),
            decoded_feature_relation_triple_count,
        );
        coverage.insert(
            "missing_feature_relation_triple_row_count".to_string(),
            missing_feature_relation_triple_row_count,
        );
        coverage.insert(
            "transferred_feature_relation_constraint_count".to_string(),
            relation_constraint_coverage.transferred,
        );
        coverage.insert(
            "transferred_native_feature_relation_constraint_count".to_string(),
            relation_constraint_coverage.native,
        );
        coverage.insert(
            "transferred_typed_feature_relation_constraint_count".to_string(),
            relation_constraint_coverage.typed(),
        );
        coverage.insert(
            "active_feature_relation_constraint_count".to_string(),
            relation_constraint_coverage.active,
        );
        coverage.insert(
            "active_native_feature_relation_constraint_count".to_string(),
            relation_constraint_coverage.active_native,
        );
        coverage.insert(
            "active_typed_feature_relation_constraint_count".to_string(),
            relation_constraint_coverage.active_typed(),
        );
        for (kind, count) in &relation_constraint_coverage.native_by_kind {
            coverage.insert(
                format!("transferred_native_feature_relation_type_{kind}_constraint_count"),
                *count,
            );
        }
        for (kind, count) in &relation_constraint_coverage.active_native_by_kind {
            coverage.insert(
                format!("active_native_feature_relation_type_{kind}_constraint_count"),
                *count,
            );
        }
    }
    let prototype_feature_dependencies = surface_prototype_feature_dependencies(scan);
    let operation_feature_ids = scan
        .features
        .operations
        .iter()
        .map(|operation| operation.feature_id)
        .collect::<BTreeSet<_>>();
    for datum in &scan.planes.datums {
        if operation_feature_ids.contains(&datum.feature_id) {
            continue;
        }
        let id = IrFeatureId(format!("creo:model:feature#{}", datum.feature_id));
        if ir.model.features.iter().any(|feature| feature.id == id) {
            continue;
        }
        annotate(
            &mut annotations,
            &id,
            "ActDatums",
            datum.offset_in_payload as u64,
            "datum_plane_feature",
            Exactness::Derived,
        );
        ir.model.features.push(Feature {
            id,
            ordinal: ir.model.features.len() as u64,
            name: None,
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: if unique_feature_datum_plane(&scan.planes.datums, datum.feature_id)
                .is_some()
            {
                datum_plane_feature_definition(datum)
            } else {
                IrFeatureDefinition::DatumPlaneUnresolved
            },
            native_ref: None,
        });
    }
    let row_feature_ids = scan
        .features
        .rows
        .iter()
        .map(|row| row.feature_id)
        .collect::<BTreeSet<_>>();
    let mut geometry_generator_feature_count = 0;
    for generator in geometry_generator_features(scan) {
        let feature_id = generator.feature_id;
        let id = IrFeatureId(format!("creo:model:feature#{feature_id}"));
        if ir.model.features.iter().any(|feature| feature.id == id) {
            continue;
        }
        annotate(
            &mut annotations,
            &id,
            "VisibGeom",
            generator.offset as u64,
            "geometry_generator_feature",
            Exactness::ByteExact,
        );
        ir.model.features.push(Feature {
            id,
            ordinal: ir.model.features.len() as u64,
            name: None,
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: feature_output_bodies(scan, &ir, feature_id),
            definition: IrFeatureDefinition::StoredGeometry,
            native_ref: None,
        });
        geometry_generator_feature_count += 1;
    }
    let operation_ordinal_base = ir.model.features.len();
    for (operation_index, operation) in scan.features.operations.iter().enumerate() {
        let id = IrFeatureId(format!("creo:model:feature#{}", operation.feature_id));
        let current_operation =
            current_feature_operation(&scan.features.operations, operation.feature_id);
        let outputs = feature_output_bodies(scan, &ir, operation.feature_id);
        let mut source_properties = feature_source_properties(scan, operation.feature_id);
        if let Some(prefix) = current_operation.and_then(|operation| operation.stored_name_prefix) {
            source_properties.insert(
                "mdl_stored_name_prefix".to_string(),
                char::from(prefix).to_string(),
            );
        }
        let parameters = feature_parameters(scan, operation.feature_id);
        let schema_class = feature_schema_class(scan, operation.feature_id);
        let definition = schema_class.map_or_else(
            || {
                current_feature_recipe(&scan.features.operations, operation.feature_id)
                    .map(|_| {
                        schema_feature_definition(
                            scan,
                            &ir,
                            operation.feature_id,
                            0,
                            &operation.kind,
                        )
                    })
                    .or_else(|| {
                        current_operation.and_then(|operation| {
                            named_or_referenced_feature_definition(
                                scan,
                                &ir,
                                operation.feature_id,
                                &operation.kind,
                            )
                        })
                    })
                    .or_else(|| unbounded_feature_plane_definition(scan, &ir, operation.feature_id))
                    .unwrap_or_else(|| IrFeatureDefinition::Native {
                        kind: current_operation
                            .map_or("Native Feature", |operation| operation.kind.as_str())
                            .to_string(),
                        parameters: parameters.clone(),
                        properties: BTreeMap::new(),
                    })
            },
            |schema_class| {
                schema_feature_definition(
                    scan,
                    &ir,
                    operation.feature_id,
                    schema_class,
                    &operation.kind,
                )
            },
        );
        retain_native_feature_parameters(&mut source_properties, &definition, &parameters);
        let dependencies = feature_dependencies(
            scan,
            &ir,
            operation.feature_id,
            &prototype_feature_dependencies,
        );
        let parent = current_feature_recipe_parent(&scan.features.operations, operation.feature_id)
            .and_then(|parent_feature_id| {
                let parent = IrFeatureId(format!("creo:model:feature#{parent_feature_id}"));
                ir.model
                    .features
                    .iter()
                    .any(|feature| feature.id == parent)
                    .then_some(parent)
            });
        let operation_section = scan
            .framing
            .sections
            .iter()
            .find(|section| {
                operation.offset >= section.offset
                    && operation.offset < section.offset.saturating_add(section.length)
            })
            .map_or("MdlStatus", |section| section.name.as_str());
        let name = current_operation.and_then(|operation| {
            operation.display_name_stored.then_some(())?;
            let stored_name = operation.stored_name.as_deref()?;
            Some(
                operation
                    .stored_name_prefix
                    .and_then(|prefix| stored_name.strip_prefix(char::from(prefix)))
                    .unwrap_or(stored_name)
                    .to_string(),
            )
        });
        let source_tag = current_feature_recipe(&scan.features.operations, operation.feature_id)
            .map(|recipe| recipe.name().to_string());
        let native_ref = owning_feature_definition_ref(scan, operation.feature_id);
        if let Some(existing) = ir
            .model
            .features
            .iter_mut()
            .find(|feature| feature.id == id)
        {
            if name.is_some() {
                existing.name = name;
            }
            if existing.parent.is_none() {
                existing.parent = parent;
            }
            for dependency in dependencies {
                if !existing.dependencies.contains(&dependency) {
                    existing.dependencies.push(dependency);
                }
            }
            existing.source_properties.extend(source_properties);
            if source_tag.is_some() {
                existing.source_tag = source_tag;
            }
            if existing.native_ref.is_none() {
                existing.native_ref = native_ref;
            }
            for output in outputs {
                if !existing.outputs.contains(&output) {
                    existing.outputs.push(output);
                }
            }
            continue;
        }
        let (operation_annotation_kind, operation_exactness) = if operation.display_state_conflict {
            ("feature_operation_state_consensus", Exactness::Derived)
        } else if operation.display_name_stored {
            ("feature_operation_name", Exactness::ByteExact)
        } else {
            ("feature_recipe", Exactness::ByteExact)
        };
        annotate(
            &mut annotations,
            &id,
            operation_section,
            operation.offset as u64,
            operation_annotation_kind,
            operation_exactness,
        );
        ir.model.features.push(Feature {
            id,
            ordinal: (operation_ordinal_base + operation_index) as u64,
            name,
            suppressed: Some(false),
            parent,
            dependencies,
            source_properties,
            source_tag,
            source_text: None,
            source_content: Vec::new(),
            outputs,
            definition,
            native_ref,
        });
    }
    for feature_id in row_feature_ids {
        let id = IrFeatureId(format!("creo:model:feature#{feature_id}"));
        if ir.model.features.iter().any(|feature| feature.id == id) {
            continue;
        }
        let schema_class = feature_schema_class(scan, feature_id);
        let Some(offset) = scan
            .features
            .rows
            .iter()
            .filter(|row| row.feature_id == feature_id)
            .map(|row| row.offset)
            .min()
        else {
            continue;
        };
        let reference_name = feature_reference_name(scan, feature_id);
        let kind = reference_name.unwrap_or_else(|| {
            schema_class
                .and_then(schema_operation_kind)
                .unwrap_or("Native Feature")
        });
        annotate(
            &mut annotations,
            &id,
            "AllFeatur",
            offset as u64,
            "schema_feature_operation",
            Exactness::ByteExact,
        );
        let parameters = feature_parameters(scan, feature_id);
        let mut source_properties = feature_source_properties(scan, feature_id);
        let definition = schema_class.map_or_else(
            || {
                named_feature_definition(scan, &ir, feature_id, kind)
                    .or_else(|| unbounded_feature_plane_definition(scan, &ir, feature_id))
                    .unwrap_or_else(|| IrFeatureDefinition::Native {
                        kind: kind.to_string(),
                        parameters: parameters.clone(),
                        properties: BTreeMap::new(),
                    })
            },
            |schema_class| schema_feature_definition(scan, &ir, feature_id, schema_class, kind),
        );
        let row_schema_classes = row_feature_schema_classes(&scan.features.rows, feature_id);
        if schema_class.is_none() {
            source_properties.insert(
                "featdefs_schema_state".to_string(),
                if row_schema_classes.is_empty() {
                    "absent"
                } else {
                    "ambiguous"
                }
                .to_string(),
            );
        }
        if !row_schema_classes.is_empty() {
            source_properties.insert(
                "featdefs_row_schema_classes".to_string(),
                row_schema_classes
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(","),
            );
        }
        retain_native_feature_parameters(&mut source_properties, &definition, &parameters);
        ir.model.features.push(Feature {
            id,
            ordinal: ir.model.features.len() as u64,
            name: Some(
                reference_name.map_or_else(|| format!("{kind} id {feature_id}"), str::to_string),
            ),
            suppressed: Some(false),
            parent: None,
            dependencies: feature_dependencies(
                scan,
                &ir,
                feature_id,
                &prototype_feature_dependencies,
            ),
            source_properties,
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: feature_output_bodies(scan, &ir, feature_id),
            definition,
            native_ref: owning_feature_definition_ref(scan, feature_id),
        });
    }
    link_feature_sketch_history(scan, &mut ir);
    reconcile_feature_links(scan, &mut ir, &prototype_feature_dependencies);
    let feature_result_topology_count = emit_feature_result_topologies(scan, &mut ir);
    let feature_result_edge_count = ir
        .model
        .feature_result_topologies
        .iter()
        .map(|state| state.edges.len())
        .sum::<usize>();
    let (transferred_feature_dimension_count, dimension_parameters) =
        transfer_feature_dimensions(scan, &mut ir, &mut annotations);
    let transferred_curve_expression_parameter_count =
        transfer_curve_expression_features(scan, &mut ir, &mut annotations, &dimension_parameters);
    {
        let active_expressions = scan
            .curves
            .expressions
            .iter()
            .filter(|record| !record.backup);
        let decoded_curve_expression_assignment_count = active_expressions
            .clone()
            .map(|record| record.assignments.len())
            .sum::<usize>();
        let decoded_curve_expression_table_cell_assignment_count = active_expressions
            .clone()
            .flat_map(|record| &record.assignments)
            .filter(|assignment| {
                matches!(
                    &assignment.target,
                    crate::curve::CurveExpressionTarget::TableCell { .. }
                )
            })
            .count();
        let decoded_curve_expression_scoped_symbol_assignment_count = active_expressions
            .clone()
            .flat_map(|record| &record.assignments)
            .filter(|assignment| {
                matches!(
                    &assignment.target,
                    crate::curve::CurveExpressionTarget::ScopedSymbol { .. }
                )
            })
            .count();
        let decoded_curve_expression_system_symbol_assignment_count = active_expressions
            .clone()
            .flat_map(|record| &record.assignments)
            .filter(|assignment| {
                matches!(
                    &assignment.target,
                    crate::curve::CurveExpressionTarget::SystemSymbol { .. }
                )
            })
            .count();
        let decoded_curve_expression_function_write_assignment_count = active_expressions
            .clone()
            .flat_map(|record| &record.assignments)
            .filter(|assignment| {
                matches!(
                    &assignment.target,
                    crate::curve::CurveExpressionTarget::FunctionWrite { .. }
                )
            })
            .count();
        let evaluated_curve_expression_assignment_count = active_expressions
            .clone()
            .flat_map(|record| &record.assignments)
            .filter(|assignment| assignment.value.is_some())
            .count();
        let decoded_curve_expression_solve_block_count = active_expressions
            .clone()
            .map(|record| record.solve_blocks.len())
            .sum::<usize>();
        let decoded_curve_expression_simultaneous_equation_count = active_expressions
            .clone()
            .flat_map(|record| &record.solve_blocks)
            .map(|block| block.equations.len())
            .sum::<usize>();
        let decoded_curve_expression_solve_assignment_count = active_expressions
            .clone()
            .flat_map(|record| &record.solve_blocks)
            .map(|block| block.assignments.len())
            .sum::<usize>();
        let decoded_curve_expression_solve_variable_count = active_expressions
            .clone()
            .flat_map(|record| &record.solve_blocks)
            .map(|block| block.variables.len())
            .sum::<usize>();
        let evaluated_curve_expression_solve_block_count = active_expressions
            .clone()
            .flat_map(|record| &record.solve_blocks)
            .filter(|block| {
                !block.solutions.is_empty() && block.solutions.iter().all(Option::is_some)
            })
            .count();
        let evaluated_curve_expression_solve_variable_count = active_expressions
            .clone()
            .flat_map(|record| &record.solve_blocks)
            .flat_map(|block| &block.solutions)
            .filter(|solution| solution.is_some())
            .count();
        let unresolved_curve_expression_solve_control_count = active_expressions
            .clone()
            .filter(|record| record.unresolved_solve_control)
            .count();
        let prohibited_curve_expression_record_count = active_expressions
            .clone()
            .filter(|record| !record.prohibited_constructs.is_empty())
            .count();
        let prohibited_curve_expression_kind_count = active_expressions
            .clone()
            .map(|record| record.prohibited_constructs.len())
            .sum::<usize>();
        let activation_count = |activation| {
            active_expressions
                .clone()
                .flat_map(|record| &record.assignments)
                .filter(|assignment| assignment.activation == activation)
                .count()
        };
        coverage.insert(
            "decoded_active_curve_expression_assignment_count".to_string(),
            decoded_curve_expression_assignment_count,
        );
        coverage.insert(
            "transferred_curve_expression_parameter_count".to_string(),
            transferred_curve_expression_parameter_count,
        );
        coverage.insert(
            "decoded_active_curve_expression_table_cell_assignment_count".to_string(),
            decoded_curve_expression_table_cell_assignment_count,
        );
        coverage.insert(
            "decoded_active_curve_expression_scoped_symbol_assignment_count".to_string(),
            decoded_curve_expression_scoped_symbol_assignment_count,
        );
        coverage.insert(
            "decoded_active_curve_expression_system_symbol_assignment_count".to_string(),
            decoded_curve_expression_system_symbol_assignment_count,
        );
        coverage.insert(
            "decoded_active_curve_expression_function_write_assignment_count".to_string(),
            decoded_curve_expression_function_write_assignment_count,
        );
        coverage.insert(
            "evaluated_active_curve_expression_assignment_count".to_string(),
            evaluated_curve_expression_assignment_count,
        );
        coverage.insert(
            "decoded_active_curve_expression_solve_block_count".to_string(),
            decoded_curve_expression_solve_block_count,
        );
        coverage.insert(
            "decoded_active_curve_expression_simultaneous_equation_count".to_string(),
            decoded_curve_expression_simultaneous_equation_count,
        );
        coverage.insert(
            "decoded_active_curve_expression_solve_assignment_count".to_string(),
            decoded_curve_expression_solve_assignment_count,
        );
        coverage.insert(
            "decoded_active_curve_expression_solve_variable_count".to_string(),
            decoded_curve_expression_solve_variable_count,
        );
        coverage.insert(
            "evaluated_active_curve_expression_solve_block_count".to_string(),
            evaluated_curve_expression_solve_block_count,
        );
        coverage.insert(
            "evaluated_active_curve_expression_solve_variable_count".to_string(),
            evaluated_curve_expression_solve_variable_count,
        );
        coverage.insert(
            "unresolved_active_curve_expression_solve_control_count".to_string(),
            unresolved_curve_expression_solve_control_count,
        );
        coverage.insert(
            "prohibited_active_curve_expression_record_count".to_string(),
            prohibited_curve_expression_record_count,
        );
        coverage.insert(
            "prohibited_active_curve_expression_kind_count".to_string(),
            prohibited_curve_expression_kind_count,
        );
        for (name, activation) in [
            ("active", crate::curve::CurveExpressionActivation::Active),
            (
                "inactive",
                crate::curve::CurveExpressionActivation::Inactive,
            ),
            (
                "conditional",
                crate::curve::CurveExpressionActivation::Conditional,
            ),
        ] {
            coverage.insert(
                format!("{name}_curve_expression_assignment_count"),
                activation_count(activation),
            );
        }
        let (decoded_dimension_count, resolved_dimension_count) = scan
            .features
            .definitions
            .iter()
            .filter_map(|definition| definition.dimensions.as_ref())
            .flat_map(|table| &table.rows)
            .fold((0usize, 0usize), |(decoded, resolved), dimension| {
                (
                    decoded + 1,
                    resolved + usize::from(dimension.value.is_some()),
                )
            });
        coverage.insert(
            "decoded_feature_dimension_count".to_string(),
            decoded_dimension_count,
        );
        coverage.insert(
            "transferred_feature_dimension_parameter_count".to_string(),
            transferred_feature_dimension_count,
        );
        coverage.insert(
            "resolved_feature_dimension_value_count".to_string(),
            resolved_dimension_count,
        );
        coverage.insert(
            "unresolved_feature_dimension_value_count".to_string(),
            decoded_dimension_count.saturating_sub(resolved_dimension_count),
        );
    }
    close_sketch_constraint_parameter_references(&mut ir);
    attach_expanded_sections(scan, &mut ir, &mut annotations)?;
    emit_geometry_arenas(scan, &mut ir, &mut annotations)?;
    collect_feature_coverage(
        scan,
        &ir,
        geometry_generator_feature_count,
        feature_result_topology_count,
        feature_result_edge_count,
        &mut coverage,
    );
    Ok(BuiltIr {
        ir,
        annotations: annotations.build(),
        unknowns,
        coverage,
    })
}

/// Emit the `MdlRefInfo` reference-geometry arenas.
///
/// Reference lines, circles, conics, and ellipse carriers, each annotated
/// against the `MdlRefInfo` stream at the record offset.
pub(super) fn emit_reference_arenas(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> Result<(), CodecError> {
    emit_uniform(
        ir,
        annotations,
        "reference_lines",
        &reference_line_records(scan),
        |record| &record.id,
        |_| "MdlRefInfo",
        |record| record.offset as u64,
        "reference_line_record",
        Exactness::ByteExact,
    )?;
    emit_uniform(
        ir,
        annotations,
        "reference_circles",
        &reference_circle_records(scan),
        |record| &record.id,
        |_| "MdlRefInfo",
        |record| record.offset as u64,
        "reference_circle_record",
        Exactness::Derived,
    )?;
    emit_uniform(
        ir,
        annotations,
        "reference_conics",
        &reference_conic_records(scan),
        |record| &record.id,
        |_| "MdlRefInfo",
        |record| record.offset as u64,
        "reference_conic_record",
        Exactness::ByteExact,
    )?;
    emit_uniform(
        ir,
        annotations,
        "reference_ellipses",
        &reference_ellipse_records(scan),
        |record| &record.id,
        |_| "MdlRefInfo",
        |record| record.offset as u64,
        "reference_ellipse_carrier",
        Exactness::Derived,
    )?;
    Ok(())
}

/// Emit the surface, curve, topology, plane, and feature arenas.
///
/// Each arena is built from the scan and stored under its native key in the
/// order the source streams are read; that order fixes the annotation stream
/// numbering, so the emissions must not be reordered.
pub(super) fn emit_geometry_arenas(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> Result<(), CodecError> {
    let surface_rows = surface_row_records(scan, &scan.surfaces.rows, "visibgeom");
    emit_uniform(
        ir,
        annotations,
        "surface_rows",
        &surface_rows,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.offset as u64,
        "surface_namespace_row",
        Exactness::ByteExact,
    )?;
    let nonvisible_surface_rows =
        surface_row_records(scan, &scan.surfaces.nonvisible_rows, "novisgeom");
    emit_uniform(
        ir,
        annotations,
        "nonvisible_surface_rows",
        &nonvisible_surface_rows,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.offset as u64,
        "nonvisible_surface_namespace_row",
        Exactness::ByteExact,
    )?;
    let cross_section_surface_rows = surface_row_records(
        scan,
        &scan.surfaces.cross_section_rows,
        "cross_section_geometry",
    );
    emit_uniform(
        ir,
        annotations,
        "cross_section_surface_rows",
        &cross_section_surface_rows,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.offset as u64,
        "cross_section_surface_namespace_row",
        Exactness::ByteExact,
    )?;
    let surface_prototypes =
        surface_prototype_records(scan, &scan.surfaces.prototype_records, "visibgeom");
    emit_uniform(
        ir,
        annotations,
        "surface_prototypes",
        &surface_prototypes,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.offset as u64,
        "surface_prototype_record",
        Exactness::ByteExact,
    )?;
    let nonvisible_surface_prototypes = surface_prototype_records(
        scan,
        &scan.surfaces.nonvisible_prototype_records,
        "novisgeom",
    );
    emit_uniform(
        ir,
        annotations,
        "nonvisible_surface_prototypes",
        &nonvisible_surface_prototypes,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.offset as u64,
        "nonvisible_surface_prototype_record",
        Exactness::ByteExact,
    )?;
    let tabulated_cylinder_curve_replays = tabulated_cylinder_curve_replay_records(scan);
    emit_uniform(
        ir,
        annotations,
        "tabulated_cylinder_curve_replays",
        &tabulated_cylinder_curve_replays,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.offset as u64,
        "tabulated_cylinder_curve_replay",
        Exactness::ByteExact,
    )?;
    let curve_parameters = curve_parameter_records(scan, &scan.curves.parameters, "visibgeom");
    emit_uniform(
        ir,
        annotations,
        "curve_parameters",
        &curve_parameters,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.offset as u64,
        "curve_parameter_record",
        Exactness::ByteExact,
    )?;
    let nonvisible_curve_parameters =
        curve_parameter_records(scan, &scan.curves.nonvisible_parameters, "novisgeom");
    emit_uniform(
        ir,
        annotations,
        "nonvisible_curve_parameters",
        &nonvisible_curve_parameters,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.offset as u64,
        "nonvisible_curve_parameter_record",
        Exactness::ByteExact,
    )?;
    let fc_curve_coordinates = fc_curve_coordinate_records(scan);
    emit_uniform(
        ir,
        annotations,
        "fc_curve_coordinates",
        &fc_curve_coordinates,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.offset as u64,
        "fc_curve_coordinates",
        Exactness::ByteExact,
    )?;
    let fc05_circles = fc05_circle_records(scan);
    store_arena(ir, "fc05_circles", &fc05_circles)?;
    let fc05_cylinder_cap_pairs = fc05_cylinder_cap_pair_records(scan);
    store_arena(ir, "fc05_cylinder_cap_pairs", &fc05_cylinder_cap_pairs)?;
    let prototype_pcurves = prototype_pcurve_records(scan);
    store_arena(ir, "prototype_pcurves", &prototype_pcurves)?;
    let curve_prototype_topology = curve_prototype_topology_records(scan);
    store_arena(ir, "curve_prototype_topology", &curve_prototype_topology)?;
    let curve_prototypes =
        curve_prototype_records(scan, &scan.curves.prototypes, "creo:curve:prototype");
    emit_uniform(
        ir,
        annotations,
        "curve_prototypes",
        &curve_prototypes,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.offset as u64,
        "curve_prototype",
        Exactness::ByteExact,
    )?;
    let nonvisible_curve_prototypes = curve_prototype_records(
        scan,
        &scan.curves.nonvisible_prototypes,
        "creo:novisgeom:curve_prototype",
    );
    emit_uniform(
        ir,
        annotations,
        "nonvisible_curve_prototypes",
        &nonvisible_curve_prototypes,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.offset as u64,
        "nonvisible_curve_prototype",
        Exactness::ByteExact,
    )?;
    let cross_section_curve_prototypes = curve_prototype_records(
        scan,
        &scan.curves.cross_section_prototypes,
        "creo:cross_section_geometry:curve_prototype",
    );
    emit_uniform(
        ir,
        annotations,
        "cross_section_curve_prototypes",
        &cross_section_curve_prototypes,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.offset as u64,
        "cross_section_curve_prototype",
        Exactness::ByteExact,
    )?;
    let curve_topology_rows =
        curve_topology_row_records(scan, &scan.curves.topology_rows, "visibgeom");
    emit_uniform(
        ir,
        annotations,
        "curve_topology_rows",
        &curve_topology_rows,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.offset as u64,
        "curve_topology_row",
        Exactness::ByteExact,
    )?;
    let nonvisible_curve_topology_rows =
        curve_topology_row_records(scan, &scan.curves.nonvisible_topology_rows, "novisgeom");
    emit_uniform(
        ir,
        annotations,
        "nonvisible_curve_topology_rows",
        &nonvisible_curve_topology_rows,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.offset as u64,
        "nonvisible_curve_topology_row",
        Exactness::ByteExact,
    )?;
    let cross_section_curve_rows = cross_section_curve_row_records(scan);
    emit_uniform(
        ir,
        annotations,
        "cross_section_curve_rows",
        &cross_section_curve_rows,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.offset as u64,
        "cross_section_curve_row",
        Exactness::ByteExact,
    )?;
    let half_edges = half_edge_records(scan);
    emit_uniform(
        ir,
        annotations,
        "half_edges",
        &half_edges,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.offset as u64,
        "native_half_edge",
        Exactness::Derived,
    )?;
    let native_loops = loop_records(scan);
    store_arena(ir, "loops", &native_loops)?;
    let topological_vertices = topological_vertex_records(scan);
    store_arena(ir, "topological_vertices", &topological_vertices)?;
    let half_edge_vertex_incidence = half_edge_vertex_incidence_records(scan);
    store_arena(
        ir,
        "half_edge_vertex_incidence",
        &half_edge_vertex_incidence,
    )?;
    let face_components = face_component_records(scan);
    store_arena(ir, "face_components", &face_components)?;
    let surface_parameters = surface_parameter_records(
        scan,
        &scan.surfaces.rows,
        &scan.surfaces.parameters,
        "visibgeom",
    );
    emit_uniform(
        ir,
        annotations,
        "surface_parameters",
        &surface_parameters,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.body_offset as u64,
        "surface_parameter_frame",
        Exactness::ByteExact,
    )?;
    let nonvisible_surface_parameters = surface_parameter_records(
        scan,
        &scan.surfaces.nonvisible_rows,
        &scan.surfaces.nonvisible_parameters,
        "novisgeom",
    );
    emit_uniform(
        ir,
        annotations,
        "nonvisible_surface_parameters",
        &nonvisible_surface_parameters,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.body_offset as u64,
        "nonvisible_surface_parameter_frame",
        Exactness::ByteExact,
    )?;
    let cross_section_surface_parameters = surface_parameter_records(
        scan,
        &scan.surfaces.cross_section_rows,
        &scan.surfaces.cross_section_parameters,
        "cross_section_geometry",
    );
    emit_uniform(
        ir,
        annotations,
        "cross_section_surface_parameters",
        &cross_section_surface_parameters,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.body_offset as u64,
        "cross_section_surface_parameter_frame",
        Exactness::ByteExact,
    )?;
    let plane_local_systems = plane_local_system_records(
        scan,
        &scan.planes.local_systems,
        "creo:surface:plane_local_system",
    );
    store_arena(ir, "plane_local_systems", &plane_local_systems)?;
    let cross_section_plane_local_systems = plane_local_system_records(
        scan,
        &scan.planes.cross_section_local_systems,
        "creo:cross_section_geometry:plane_local_system",
    );
    store_arena(
        ir,
        "cross_section_plane_local_systems",
        &cross_section_plane_local_systems,
    )?;
    let plane_envelopes =
        plane_envelope_records(scan, &scan.planes.envelopes, "creo:surface:plane_envelope");
    store_arena(ir, "plane_envelopes", &plane_envelopes)?;
    let cross_section_plane_envelopes = plane_envelope_records(
        scan,
        &scan.planes.cross_section_envelopes,
        "creo:cross_section_geometry:plane_envelope",
    );
    store_arena(
        ir,
        "cross_section_plane_envelopes",
        &cross_section_plane_envelopes,
    )?;
    let outline_planes =
        outline_plane_records(scan, &scan.planes.outlines, "creo:surface:outline_plane");
    store_arena(ir, "outline_planes", &outline_planes)?;
    let positional_frame_planes = outline_plane_records(
        scan,
        &scan.planes.positional_frames,
        "creo:surface:positional_frame_plane",
    );
    store_arena(ir, "positional_frame_planes", &positional_frame_planes)?;
    let cross_section_outline_planes = outline_plane_records(
        scan,
        &scan.planes.cross_section_outlines,
        "creo:cross_section_geometry:outline_plane",
    );
    store_arena(
        ir,
        "cross_section_outline_planes",
        &cross_section_outline_planes,
    )?;
    let datum_planes = datum_plane_records(scan);
    store_arena(ir, "datum_planes", &datum_planes)?;
    let feature_section_transforms = feature_section_transform_records(scan);
    store_arena(
        ir,
        "feature_section_transforms",
        &feature_section_transforms,
    )?;
    let feature_placement_instructions = feature_placement_instruction_records(scan);
    store_arena(
        ir,
        "feature_placement_instructions",
        &feature_placement_instructions,
    )?;
    // Bespoke annotation: the arena payload drops the per-record source offset the
    // annotation needs, so the offset travels alongside each record in a tuple.
    let pcurve_endpoints = pcurve_endpoint_records(scan);
    for (record, offset) in &pcurve_endpoints {
        annotate(
            annotations,
            &record.id,
            "VisibGeom",
            *offset as u64,
            "pcurve_endpoint_frames",
            Exactness::Derived,
        );
    }
    let pcurve_endpoint_payload = pcurve_endpoints
        .iter()
        .map(|(record, _)| record)
        .collect::<Vec<_>>();
    store_arena(ir, "pcurve_endpoints", &pcurve_endpoint_payload)?;
    let feature_definitions = feature_definition_records(scan);
    emit_uniform(
        ir,
        annotations,
        "feature_definitions",
        &feature_definitions,
        |definition| &definition.id,
        |definition| &definition.source_section,
        |definition| definition.offset as u64,
        "feature_definition_record",
        Exactness::ByteExact,
    )?;
    let feature_entities = feature_entity_records(scan);
    emit_uniform(
        ir,
        annotations,
        "feature_entities",
        &feature_entities,
        |entity| &entity.id,
        |_| "AllFeatur",
        |entity| entity.offset as u64,
        "feature_entity",
        Exactness::ByteExact,
    )?;
    let feature_entity_references = feature_entity_reference_records(scan);
    emit_uniform(
        ir,
        annotations,
        "feature_entity_references",
        &feature_entity_references,
        |reference| &reference.id,
        |_| "AllFeatur",
        |reference| reference.offset as u64,
        "feature_entity_reference",
        Exactness::ByteExact,
    )?;
    let feature_entity_tables = feature_entity_table_records(scan);
    emit_uniform(
        ir,
        annotations,
        "feature_entity_tables",
        &feature_entity_tables,
        |table| &table.id,
        |_| "AllFeatur",
        |table| table.offset as u64,
        "feature_entity_table",
        Exactness::ByteExact,
    )?;
    let feature_surface_replays = feature_surface_replay_associations(scan);
    emit_uniform(
        ir,
        annotations,
        "feature_surface_replays",
        &feature_surface_replays,
        |association| &association.id,
        |_| "AllFeatur",
        |association| association.table_offset as u64,
        "feature_surface_replay_association",
        Exactness::Derived,
    )?;
    let feature_geometry_tables = feature_geometry_table_records(scan);
    emit_uniform(
        ir,
        annotations,
        "feature_geometry_tables",
        &feature_geometry_tables,
        |table| &table.id,
        |table| &table.source_section,
        |table| table.offset as u64,
        "feature_geometry_table",
        Exactness::ByteExact,
    )?;
    let feature_loop_history_entries = feature_loop_history_entry_records(scan);
    emit_uniform(
        ir,
        annotations,
        "feature_loop_history_entries",
        &feature_loop_history_entries,
        |entry| &entry.id,
        |entry| &entry.source_section,
        |entry| entry.offset as u64,
        "feature_loop_history_entry",
        Exactness::ByteExact,
    )?;
    let feature_affected_ids = feature_affected_id_records(scan);
    emit_uniform(
        ir,
        annotations,
        "feature_affected_ids",
        &feature_affected_ids,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.offset as u64,
        "feature_affected_ids",
        Exactness::ByteExact,
    )?;
    let feature_replay_affected_ids = feature_replay_affected_id_records(scan);
    emit_uniform(
        ir,
        annotations,
        "feature_replay_affected_ids",
        &feature_replay_affected_ids,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.offset as u64,
        "feature_replay_affected_ids",
        Exactness::ByteExact,
    )?;
    let surface_merge_replay_affected_ids = surface_merge_replay_affected_id_records(scan);
    emit_uniform(
        ir,
        annotations,
        "surface_merge_replay_affected_ids",
        &surface_merge_replay_affected_ids,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.offset as u64,
        "surface_merge_replay_affected_ids",
        Exactness::ByteExact,
    )?;
    let feature_loop_restore_directions = feature_loop_restore_direction_records(scan);
    emit_uniform(
        ir,
        annotations,
        "feature_loop_restore_directions",
        &feature_loop_restore_directions,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.offset as u64,
        "feature_loop_restore_direction",
        Exactness::ByteExact,
    )?;
    let feature_revolution_extents = feature_revolution_extent_records(scan);
    emit_uniform(
        ir,
        annotations,
        "feature_revolution_extents",
        &feature_revolution_extents,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.offset as u64,
        "feature_revolution_extent",
        Exactness::Derived,
    )?;
    let feature_rows = feature_row_records(scan);
    emit_uniform(
        ir,
        annotations,
        "feature_rows",
        &feature_rows,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.offset as u64,
        "feature_row",
        Exactness::ByteExact,
    )?;
    let depdb_recipe_rows = depdb_recipe_row_records(scan);
    emit_uniform(
        ir,
        annotations,
        "depdb_recipe_rows",
        &depdb_recipe_rows,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.offset as u64,
        "depdb_recipe_row",
        Exactness::ByteExact,
    )?;
    let feature_choices = feature_choice_records(scan);
    emit_uniform(
        ir,
        annotations,
        "feature_choices",
        &feature_choices,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.offset as u64,
        "feature_choice",
        Exactness::ByteExact,
    )?;
    let feature_choice_fields = feature_choice_field_records(scan);
    emit_uniform(
        ir,
        annotations,
        "feature_choice_fields",
        &feature_choice_fields,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.offset as u64,
        "feature_choice_field",
        Exactness::ByteExact,
    )?;
    let sketches = sketch_records(scan);
    emit_uniform(
        ir,
        annotations,
        "sketches",
        &sketches,
        |sketch| &sketch.id,
        |sketch| &sketch.source_section,
        |sketch| sketch.offset as u64,
        "feature_sketch",
        Exactness::Derived,
    )?;
    // Bespoke annotation: the source offset comes from the parallel scan rows, not
    // the record, so annotation zips the two before the arena is stored.
    let curve_expressions = curve_expression_records(scan);
    for (expression, source) in curve_expressions.iter().zip(&scan.curves.expressions) {
        annotate(
            annotations,
            &expression.id,
            "DEPDB_DATA",
            source.expression_offset as u64,
            "curve_expression_program",
            Exactness::ByteExact,
        );
    }
    store_arena(ir, "curve_expressions", &curve_expressions)?;
    let feature_operation_states = feature_operation_state_records(scan);
    emit_arena(
        ir,
        annotations,
        "feature_operation_states",
        &feature_operation_states,
        |annotations, state| {
            let section = scan
                .framing
                .sections
                .iter()
                .find(|section| {
                    state.state_offset >= section.offset
                        && state.state_offset < section.offset.saturating_add(section.length)
                })
                .map_or("MdlStatus", |section| section.name.as_str());
            annotate(
                annotations,
                &state.id,
                section,
                state.state_offset as u64,
                "feature_operation_state",
                Exactness::ByteExact,
            );
        },
    )?;
    let feature_reference_names = feature_reference_name_records(scan);
    emit_uniform(
        ir,
        annotations,
        "feature_reference_names",
        &feature_reference_names,
        |record| &record.id,
        |_| "MdlRefInfo",
        |record| record.offset as u64,
        "feature_reference_name",
        Exactness::ByteExact,
    )?;
    if let Some(family_table) = family_table_record(scan) {
        annotate(
            annotations,
            family_table.id,
            "FamilyInf",
            family_table.offset as u64,
            "configuration_driver_table_pointer",
            Exactness::ByteExact,
        );
        store_arena(ir, "configuration", &[family_table])?;
    }
    Ok(())
}

pub(super) fn record_coverage<const N: usize>(
    coverage: &mut BTreeMap<String, usize>,
    entries: [(&str, usize); N],
) {
    coverage.extend(
        entries
            .into_iter()
            .map(|(key, count)| (key.to_string(), count)),
    );
}

macro_rules! record_transferred_feature_coverage {
    ($coverage:expr, $($counter:ident),+ $(,)?) => {
        record_coverage(
            $coverage,
            [$((
                concat!("transferred_", stringify!($counter)),
                $counter,
            ),)+],
        );
    };
}

/// Count transferred features by kind and record the counts in `coverage`.
///
/// Walks the transferred feature list once, classifying each feature by its
/// definition and by whether its operands resolved, then writes one
/// `transferred_*` entry per counted kind. Reads the built model; emits no
/// entities and no annotations.
pub(super) fn collect_feature_coverage(
    scan: &ContainerScan,
    ir: &CadIr,
    geometry_generator_feature_count: usize,
    feature_result_topology_count: usize,
    feature_result_edge_count: usize,
    coverage: &mut BTreeMap<String, usize>,
) {
    let native_feature_count = ir
        .model
        .features
        .iter()
        .filter(|feature| matches!(feature.definition, IrFeatureDefinition::Native { .. }))
        .count();
    let mut unresolved_datum_plane_feature_count = 0;
    let mut unresolved_datum_coordinate_system_feature_count = 0;
    let mut unresolved_boundary_surface_feature_count = 0;
    let mut extrude_feature_count = 0;
    let mut incomplete_extrude_feature_count = 0;
    let mut unresolved_extrude_profile_feature_count = 0;
    let mut native_extrude_profile_feature_count = 0;
    let mut incomplete_extrude_start_feature_count = 0;
    let mut incomplete_extrude_termination_feature_count = 0;
    let mut unresolved_extrude_boolean_operation_feature_count = 0;
    let mut revolve_feature_count = 0;
    let mut incomplete_revolve_feature_count = 0;
    let mut unresolved_revolve_profile_feature_count = 0;
    let mut native_revolve_profile_feature_count = 0;
    let mut unresolved_revolve_axis_feature_count = 0;
    let mut incomplete_revolve_extent_feature_count = 0;
    let mut unresolved_revolve_boolean_operation_feature_count = 0;
    let mut hole_feature_count = 0;
    let mut incomplete_hole_feature_count = 0;
    let mut unresolved_hole_location_feature_count = 0;
    let mut unresolved_hole_profile_feature_count = 0;
    let mut native_hole_profile_feature_count = 0;
    let mut unresolved_hole_face_selection_feature_count = 0;
    let mut native_hole_face_selection_feature_count = 0;
    let mut unresolved_hole_direction_feature_count = 0;
    let mut unresolved_hole_kind_feature_count = 0;
    let mut unresolved_hole_diameter_feature_count = 0;
    let mut incomplete_hole_termination_feature_count = 0;
    let mut fillet_feature_count = 0;
    let mut incomplete_fillet_feature_count = 0;
    let mut unresolved_fillet_edge_selection_feature_count = 0;
    let mut native_fillet_edge_selection_feature_count = 0;
    let mut unresolved_fillet_radius_feature_count = 0;
    let mut unresolved_fillet_radius_without_generated_surface_feature_count = 0;
    let mut unresolved_fillet_radius_with_generated_surface_feature_count = 0;
    let mut variable_radius_fillet_feature_count = 0;
    let mut chamfer_feature_count = 0;
    let mut incomplete_chamfer_feature_count = 0;
    let mut unresolved_chamfer_edge_selection_feature_count = 0;
    let mut native_chamfer_edge_selection_feature_count = 0;
    let mut unresolved_chamfer_spec_feature_count = 0;
    let mut draft_feature_count = 0;
    let mut incomplete_draft_feature_count = 0;
    let mut explicitly_unresolved_draft_feature_count = 0;
    let mut unresolved_draft_face_selection_feature_count = 0;
    let mut native_draft_face_selection_feature_count = 0;
    let mut unresolved_draft_neutral_plane_feature_count = 0;
    let mut native_draft_neutral_plane_feature_count = 0;
    let mut unresolved_draft_direction_feature_count = 0;
    let mut unresolved_draft_angle_feature_count = 0;
    let mut unresolved_draft_outward_feature_count = 0;
    let mut filled_surface_feature_count = 0;
    let mut incomplete_filled_surface_feature_count = 0;
    let mut unresolved_filled_surface_boundary_feature_count = 0;
    let mut unresolved_filled_surface_support_feature_count = 0;
    let mut unresolved_filled_surface_continuity_feature_count = 0;
    let mut unresolved_filled_surface_merge_feature_count = 0;
    let mut knit_surface_feature_count = 0;
    let mut incomplete_knit_surface_feature_count = 0;
    let mut unresolved_knit_surface_faces_feature_count = 0;
    let mut native_knit_surface_faces_feature_count = 0;
    let mut unresolved_knit_surface_merge_feature_count = 0;
    let mut unresolved_knit_surface_solid_feature_count = 0;
    let mut thicken_feature_count = 0;
    let mut incomplete_thicken_feature_count = 0;
    let mut unresolved_thicken_faces_feature_count = 0;
    let mut unresolved_thicken_thickness_feature_count = 0;
    let mut unresolved_thicken_side_feature_count = 0;
    let mut section_shape_feature_count = 0;
    let mut incomplete_section_shape_feature_count = 0;
    let mut pattern_feature_count = 0;
    let mut incomplete_pattern_feature_count = 0;
    let mut unresolved_pattern_seed_feature_count = 0;
    let mut unresolved_pattern_transform_feature_count = 0;
    let mut native_axis_helix_feature_count = 0;
    for feature in &ir.model.features {
        match &feature.definition {
            IrFeatureDefinition::DatumPlaneUnresolved => {
                unresolved_datum_plane_feature_count += 1;
            }
            IrFeatureDefinition::DatumCoordinateSystemUnresolved => {
                unresolved_datum_coordinate_system_feature_count += 1;
            }
            IrFeatureDefinition::BoundarySurfaceUnresolved => {
                unresolved_boundary_surface_feature_count += 1;
            }
            IrFeatureDefinition::Extrude {
                profile,
                start,
                extent,
                op,
                ..
            } => {
                extrude_feature_count += 1;
                let unresolved_profile = matches!(profile, ProfileRef::Unresolved(_));
                let native_profile = matches!(profile, ProfileRef::Native(_));
                let incomplete_start = matches!(
                    start,
                    ExtrudeStart::FromFace { face, .. }
                        if face_selection_has_unresolved_operands(face)
                );
                let incomplete_termination = match extent {
                    ExtrudeExtent::OneSided { side } | ExtrudeExtent::Symmetric { side } => {
                        termination_has_unresolved_operands(&side.termination)
                    }
                    ExtrudeExtent::TwoSided { first, second } => {
                        termination_has_unresolved_operands(&first.termination)
                            || termination_has_unresolved_operands(&second.termination)
                    }
                };
                let unresolved_op = *op == BooleanOp::Unresolved;
                unresolved_extrude_profile_feature_count += usize::from(unresolved_profile);
                native_extrude_profile_feature_count += usize::from(native_profile);
                incomplete_extrude_start_feature_count += usize::from(incomplete_start);
                incomplete_extrude_termination_feature_count += usize::from(incomplete_termination);
                unresolved_extrude_boolean_operation_feature_count += usize::from(unresolved_op);
                incomplete_extrude_feature_count += usize::from(
                    unresolved_profile
                        || native_profile
                        || incomplete_start
                        || incomplete_termination
                        || unresolved_op,
                );
            }
            IrFeatureDefinition::Revolve { construction, op } => {
                revolve_feature_count += 1;
                let unresolved_profile = construction
                    .profile
                    .as_ref()
                    .is_none_or(|profile| matches!(profile, ProfileRef::Unresolved(_)));
                let native_profile = matches!(construction.profile, Some(ProfileRef::Native(_)));
                let unresolved_axis = construction.axis.is_none();
                let incomplete_extent =
                    construction
                        .extent
                        .as_ref()
                        .is_none_or(|extent| match extent {
                            RevolveExtent::OneSided { termination }
                            | RevolveExtent::Symmetric { termination } => {
                                termination_has_unresolved_operands(termination)
                            }
                            RevolveExtent::TwoSided { first, second } => {
                                termination_has_unresolved_operands(first)
                                    || termination_has_unresolved_operands(second)
                            }
                        });
                let unresolved_op = *op == BooleanOp::Unresolved;
                unresolved_revolve_profile_feature_count += usize::from(unresolved_profile);
                native_revolve_profile_feature_count += usize::from(native_profile);
                unresolved_revolve_axis_feature_count += usize::from(unresolved_axis);
                incomplete_revolve_extent_feature_count += usize::from(incomplete_extent);
                unresolved_revolve_boolean_operation_feature_count += usize::from(unresolved_op);
                incomplete_revolve_feature_count += usize::from(
                    unresolved_profile
                        || native_profile
                        || unresolved_axis
                        || incomplete_extent
                        || unresolved_op,
                );
            }
            IrFeatureDefinition::Hole {
                profile,
                face,
                position,
                direction,
                placements,
                kind,
                exit_kind,
                diameter,
                extent,
                ..
            } => {
                hole_feature_count += 1;
                let unresolved_location =
                    profile.is_none() && position.is_none() && placements.is_empty();
                let unresolved_profile = matches!(profile, Some(ProfileRef::Unresolved(_)));
                let native_profile = matches!(profile, Some(ProfileRef::Native(_)));
                let unresolved_face = matches!(
                    face,
                    Some(FaceSelection::Unresolved | FaceSelection::HistoricalPartial { .. })
                );
                let native_face = matches!(face, Some(FaceSelection::Native(_)));
                let unresolved_direction = direction.is_none()
                    && !placements.iter().any(|placement| {
                        matches!(
                            placement,
                            cadmpeg_ir::features::HolePlacement::Directed { .. }
                        )
                    });
                let unresolved_kind = matches!(kind, HoleKind::Unresolved { .. })
                    || matches!(exit_kind, Some(HoleKind::Unresolved { .. }));
                let unresolved_diameter = diameter.is_none();
                let incomplete_termination = extent
                    .as_ref()
                    .is_none_or(termination_has_unresolved_operands);
                unresolved_hole_location_feature_count += usize::from(unresolved_location);
                unresolved_hole_profile_feature_count += usize::from(unresolved_profile);
                native_hole_profile_feature_count += usize::from(native_profile);
                unresolved_hole_face_selection_feature_count += usize::from(unresolved_face);
                native_hole_face_selection_feature_count += usize::from(native_face);
                unresolved_hole_direction_feature_count += usize::from(unresolved_direction);
                unresolved_hole_kind_feature_count += usize::from(unresolved_kind);
                unresolved_hole_diameter_feature_count += usize::from(unresolved_diameter);
                incomplete_hole_termination_feature_count += usize::from(incomplete_termination);
                incomplete_hole_feature_count += usize::from(
                    unresolved_location
                        || unresolved_profile
                        || native_profile
                        || unresolved_face
                        || native_face
                        || unresolved_direction
                        || unresolved_kind
                        || unresolved_diameter
                        || incomplete_termination,
                );
            }
            IrFeatureDefinition::Fillet { groups } => {
                fillet_feature_count += 1;
                let unresolved_edges = groups.is_empty()
                    || groups.iter().any(|group| {
                        matches!(
                            &group.edges,
                            EdgeSelection::Unresolved | EdgeSelection::HistoricalPartial { .. }
                        )
                    });
                let native_edges = groups
                    .iter()
                    .any(|group| matches!(&group.edges, EdgeSelection::Native(_)));
                let unresolved_radius = groups.is_empty()
                    || groups
                        .iter()
                        .any(|group| matches!(&group.radius, RadiusSpec::Unresolved { .. }));
                let variable_radius = groups.iter().any(|group| {
                    matches!(
                        &group.radius,
                        RadiusSpec::Unresolved {
                            form: Some(RadiusForm::Variable)
                        }
                    )
                });
                let has_generated_surface = feature
                    .id
                    .as_str()
                    .strip_prefix("creo:model:feature#")
                    .and_then(|value| value.parse::<u32>().ok())
                    .is_some_and(|feature_id| {
                        scan.surfaces
                            .rows
                            .iter()
                            .any(|row| row.feature_id == feature_id)
                    });
                unresolved_fillet_edge_selection_feature_count += usize::from(unresolved_edges);
                native_fillet_edge_selection_feature_count += usize::from(native_edges);
                unresolved_fillet_radius_feature_count += usize::from(unresolved_radius);
                unresolved_fillet_radius_without_generated_surface_feature_count +=
                    usize::from(unresolved_radius && !has_generated_surface);
                unresolved_fillet_radius_with_generated_surface_feature_count +=
                    usize::from(unresolved_radius && has_generated_surface);
                variable_radius_fillet_feature_count += usize::from(variable_radius);
                incomplete_fillet_feature_count +=
                    usize::from(unresolved_edges || native_edges || unresolved_radius);
            }
            IrFeatureDefinition::Chamfer { groups, .. } => {
                chamfer_feature_count += 1;
                let unresolved_edges = groups.is_empty()
                    || groups.iter().any(|group| {
                        matches!(
                            &group.edges,
                            EdgeSelection::Unresolved | EdgeSelection::HistoricalPartial { .. }
                        )
                    });
                let native_edges = groups
                    .iter()
                    .any(|group| matches!(&group.edges, EdgeSelection::Native(_)));
                let unresolved_spec = groups.is_empty()
                    || groups
                        .iter()
                        .any(|group| matches!(&group.spec, ChamferSpec::Unresolved { .. }));
                unresolved_chamfer_edge_selection_feature_count += usize::from(unresolved_edges);
                native_chamfer_edge_selection_feature_count += usize::from(native_edges);
                unresolved_chamfer_spec_feature_count += usize::from(unresolved_spec);
                incomplete_chamfer_feature_count +=
                    usize::from(unresolved_edges || native_edges || unresolved_spec);
            }
            IrFeatureDefinition::Draft {
                faces,
                neutral_plane,
                pull_direction,
                angle,
                outward,
                ..
            } => {
                draft_feature_count += 1;
                let unresolved_faces = matches!(
                    faces,
                    FaceSelection::Unresolved | FaceSelection::HistoricalPartial { .. }
                );
                let native_faces = matches!(faces, FaceSelection::Native(_));
                let unresolved_neutral_plane = matches!(
                    neutral_plane,
                    FaceSelection::Unresolved | FaceSelection::HistoricalPartial { .. }
                );
                let native_neutral_plane = matches!(neutral_plane, FaceSelection::Native(_));
                let unresolved_direction = pull_direction.is_none();
                let unresolved_angle = angle.is_none();
                let unresolved_outward = outward.is_none();
                unresolved_draft_face_selection_feature_count += usize::from(unresolved_faces);
                native_draft_face_selection_feature_count += usize::from(native_faces);
                unresolved_draft_neutral_plane_feature_count +=
                    usize::from(unresolved_neutral_plane);
                native_draft_neutral_plane_feature_count += usize::from(native_neutral_plane);
                unresolved_draft_direction_feature_count += usize::from(unresolved_direction);
                unresolved_draft_angle_feature_count += usize::from(unresolved_angle);
                unresolved_draft_outward_feature_count += usize::from(unresolved_outward);
                incomplete_draft_feature_count += usize::from(
                    unresolved_faces
                        || native_faces
                        || unresolved_neutral_plane
                        || native_neutral_plane
                        || unresolved_direction
                        || unresolved_angle
                        || unresolved_outward,
                );
            }
            IrFeatureDefinition::DraftUnresolved => {
                draft_feature_count += 1;
                incomplete_draft_feature_count += 1;
                explicitly_unresolved_draft_feature_count += 1;
            }
            IrFeatureDefinition::FilledSurface {
                boundary,
                support_faces,
                continuity,
                merge_result,
                ..
            } => {
                filled_surface_feature_count += 1;
                let unresolved_boundary = surface_boundary_has_unresolved_operands(boundary);
                let unresolved_support = face_selection_has_unresolved_operands(support_faces);
                let unresolved_continuity = continuity.is_none();
                let unresolved_merge = merge_result.is_none();
                unresolved_filled_surface_boundary_feature_count +=
                    usize::from(unresolved_boundary);
                unresolved_filled_surface_support_feature_count += usize::from(unresolved_support);
                unresolved_filled_surface_continuity_feature_count +=
                    usize::from(unresolved_continuity);
                unresolved_filled_surface_merge_feature_count += usize::from(unresolved_merge);
                incomplete_filled_surface_feature_count += usize::from(
                    unresolved_boundary
                        || unresolved_support
                        || unresolved_continuity
                        || unresolved_merge,
                );
            }
            IrFeatureDefinition::KnitSurface {
                faces,
                merge_entities,
                create_solid,
                ..
            } => {
                knit_surface_feature_count += 1;
                let unresolved_faces = matches!(
                    faces,
                    FaceSelection::Unresolved | FaceSelection::HistoricalPartial { .. }
                );
                let native_faces = matches!(faces, FaceSelection::Native(_));
                let unresolved_merge = merge_entities.is_none();
                let unresolved_solid = create_solid.is_none();
                unresolved_knit_surface_faces_feature_count += usize::from(unresolved_faces);
                native_knit_surface_faces_feature_count += usize::from(native_faces);
                unresolved_knit_surface_merge_feature_count += usize::from(unresolved_merge);
                unresolved_knit_surface_solid_feature_count += usize::from(unresolved_solid);
                incomplete_knit_surface_feature_count += usize::from(
                    unresolved_faces || native_faces || unresolved_merge || unresolved_solid,
                );
            }
            IrFeatureDefinition::Thicken {
                faces,
                thickness,
                side,
            } => {
                thicken_feature_count += 1;
                let unresolved_faces = face_selection_has_unresolved_operands(faces);
                let unresolved_thickness = thickness.is_none();
                let unresolved_side = side.is_none();
                unresolved_thicken_faces_feature_count += usize::from(unresolved_faces);
                unresolved_thicken_thickness_feature_count += usize::from(unresolved_thickness);
                unresolved_thicken_side_feature_count += usize::from(unresolved_side);
                incomplete_thicken_feature_count +=
                    usize::from(unresolved_faces || unresolved_thickness || unresolved_side);
            }
            IrFeatureDefinition::SectionShape { first, second, .. } => {
                section_shape_feature_count += 1;
                incomplete_section_shape_feature_count += usize::from(
                    body_selection_has_unresolved_operands(first)
                        || body_selection_has_unresolved_operands(second),
                );
            }
            IrFeatureDefinition::Pattern { seeds, pattern } => {
                pattern_feature_count += 1;
                let unresolved_seeds = seeds.is_empty()
                    || seeds.iter().any(|seed| match seed {
                        cadmpeg_ir::features::PatternSeed::Feature(_) => false,
                        cadmpeg_ir::features::PatternSeed::Faces(faces) => {
                            face_selection_has_unresolved_operands(faces)
                        }
                        cadmpeg_ir::features::PatternSeed::Bodies(bodies) => {
                            matches!(
                                bodies,
                                cadmpeg_ir::features::BodySelection::Unresolved
                                    | cadmpeg_ir::features::BodySelection::Native(_)
                            )
                        }
                        cadmpeg_ir::features::PatternSeed::Occurrences(occurrences) => {
                            occurrences.is_empty()
                        }
                    });
                let unresolved_transform = pattern_kind_has_unresolved_operands(pattern);
                unresolved_pattern_seed_feature_count += usize::from(unresolved_seeds);
                unresolved_pattern_transform_feature_count += usize::from(unresolved_transform);
                incomplete_pattern_feature_count +=
                    usize::from(unresolved_seeds || unresolved_transform);
            }
            IrFeatureDefinition::HelixNativeAxis { .. } => {
                native_axis_helix_feature_count += 1;
            }
            _ => {}
        }
    }
    let explicitly_unresolved_feature_count = unresolved_datum_plane_feature_count
        + unresolved_datum_coordinate_system_feature_count
        + unresolved_boundary_surface_feature_count
        + explicitly_unresolved_draft_feature_count;
    let incomplete_recognized_feature_count = incomplete_hole_feature_count
        + incomplete_fillet_feature_count
        + incomplete_chamfer_feature_count
        + incomplete_draft_feature_count;
    let incomplete_sweep_feature_count =
        incomplete_extrude_feature_count + incomplete_revolve_feature_count;
    let incomplete_surface_operation_feature_count = incomplete_filled_surface_feature_count
        + incomplete_knit_surface_feature_count
        + incomplete_thicken_feature_count;
    let incomplete_other_construction_feature_count = incomplete_section_shape_feature_count
        + incomplete_pattern_feature_count
        + native_axis_helix_feature_count;
    record_coverage(
        coverage,
        [
            ("transferred_feature_count", ir.model.features.len()),
            (
                "transferred_feature_result_edge_count",
                feature_result_edge_count,
            ),
            (
                "transferred_feature_result_topology_count",
                feature_result_topology_count,
            ),
            (
                "transferred_typed_feature_count",
                ir.model.features.len() - native_feature_count,
            ),
            ("transferred_native_feature_count", native_feature_count),
            (
                "transferred_geometry_generator_feature_count",
                geometry_generator_feature_count,
            ),
            (
                "transferred_explicitly_unresolved_feature_count",
                explicitly_unresolved_feature_count,
            ),
            (
                "transferred_incomplete_sweep_feature_count",
                incomplete_sweep_feature_count,
            ),
            (
                "transferred_incomplete_recognized_feature_count",
                incomplete_recognized_feature_count,
            ),
            (
                "transferred_incomplete_surface_operation_feature_count",
                incomplete_surface_operation_feature_count,
            ),
            (
                "transferred_incomplete_other_construction_feature_count",
                incomplete_other_construction_feature_count,
            ),
        ],
    );
    record_transferred_feature_coverage!(
        coverage,
        unresolved_datum_plane_feature_count,
        unresolved_datum_coordinate_system_feature_count,
        unresolved_boundary_surface_feature_count,
        extrude_feature_count,
        incomplete_extrude_feature_count,
        unresolved_extrude_profile_feature_count,
        native_extrude_profile_feature_count,
        incomplete_extrude_start_feature_count,
        incomplete_extrude_termination_feature_count,
        unresolved_extrude_boolean_operation_feature_count,
        revolve_feature_count,
        incomplete_revolve_feature_count,
        unresolved_revolve_profile_feature_count,
        native_revolve_profile_feature_count,
        unresolved_revolve_axis_feature_count,
        incomplete_revolve_extent_feature_count,
        unresolved_revolve_boolean_operation_feature_count,
        hole_feature_count,
        incomplete_hole_feature_count,
        unresolved_hole_location_feature_count,
        unresolved_hole_profile_feature_count,
        native_hole_profile_feature_count,
        unresolved_hole_face_selection_feature_count,
        native_hole_face_selection_feature_count,
        unresolved_hole_direction_feature_count,
        unresolved_hole_kind_feature_count,
        unresolved_hole_diameter_feature_count,
        incomplete_hole_termination_feature_count,
        fillet_feature_count,
        incomplete_fillet_feature_count,
        unresolved_fillet_edge_selection_feature_count,
        native_fillet_edge_selection_feature_count,
        unresolved_fillet_radius_feature_count,
        unresolved_fillet_radius_without_generated_surface_feature_count,
        unresolved_fillet_radius_with_generated_surface_feature_count,
        variable_radius_fillet_feature_count,
        chamfer_feature_count,
        incomplete_chamfer_feature_count,
        unresolved_chamfer_edge_selection_feature_count,
        native_chamfer_edge_selection_feature_count,
        unresolved_chamfer_spec_feature_count,
        draft_feature_count,
        incomplete_draft_feature_count,
        explicitly_unresolved_draft_feature_count,
        unresolved_draft_face_selection_feature_count,
        native_draft_face_selection_feature_count,
        unresolved_draft_neutral_plane_feature_count,
        native_draft_neutral_plane_feature_count,
        unresolved_draft_direction_feature_count,
        unresolved_draft_angle_feature_count,
        unresolved_draft_outward_feature_count,
        filled_surface_feature_count,
        incomplete_filled_surface_feature_count,
        unresolved_filled_surface_boundary_feature_count,
        unresolved_filled_surface_support_feature_count,
        unresolved_filled_surface_continuity_feature_count,
        unresolved_filled_surface_merge_feature_count,
        knit_surface_feature_count,
        incomplete_knit_surface_feature_count,
        unresolved_knit_surface_faces_feature_count,
        native_knit_surface_faces_feature_count,
        unresolved_knit_surface_merge_feature_count,
        unresolved_knit_surface_solid_feature_count,
        thicken_feature_count,
        incomplete_thicken_feature_count,
        unresolved_thicken_faces_feature_count,
        unresolved_thicken_thickness_feature_count,
        unresolved_thicken_side_feature_count,
        section_shape_feature_count,
        incomplete_section_shape_feature_count,
        pattern_feature_count,
        incomplete_pattern_feature_count,
        unresolved_pattern_seed_feature_count,
        unresolved_pattern_transform_feature_count,
        native_axis_helix_feature_count,
    );
}

#[derive(Default)]
pub(super) struct TorusParameterCoverage {
    pub(super) radius_overrides: usize,
    pub(super) replayed_minor_radii: usize,
    pub(super) outline_extents: usize,
    pub(super) five_coordinate_envelopes: usize,
    pub(super) split_coordinate_envelopes: usize,
}

pub(super) fn torus_parameter_coverage(scan: &ContainerScan) -> TorusParameterCoverage {
    let rows = scan.surfaces.parameters.iter().filter_map(|record| {
        crate::surface::unique_surface_row(&scan.surfaces.rows, record.surface_id)
            .map(|row| (record, row))
    });
    TorusParameterCoverage {
        radius_overrides: rows
            .clone()
            .filter(|(record, row)| record.torus_radius_overrides(row.type_byte).is_some())
            .count(),
        replayed_minor_radii: rows
            .clone()
            .filter(|(record, row)| replayed_torus_minor_radius(scan, row, record).is_some())
            .count(),
        outline_extents: rows
            .clone()
            .filter(|(record, row)| record.torus_outline_frame(row.type_byte).is_some())
            .count(),
        five_coordinate_envelopes: rows
            .clone()
            .filter(|(record, row)| {
                record
                    .type26_five_coordinate_envelope(row.type_byte)
                    .is_some()
            })
            .count(),
        split_coordinate_envelopes: rows
            .filter(|(record, row)| {
                record
                    .type26_split_coordinate_envelope(row.type_byte)
                    .is_some()
            })
            .count(),
    }
}

pub(super) fn legacy_numeric_coverage<T>(
    records: &[crate::legacy::NumericRecord<T>],
) -> (usize, usize, usize) {
    records.iter().fold(
        (0usize, 0usize, 0usize),
        |(scalars, arrays, elements), record| {
            (
                scalars
                    + usize::from(matches!(
                        record.payload,
                        crate::legacy::NumericPayload::Scalar { .. }
                    )),
                arrays
                    + usize::from(matches!(
                        record.payload,
                        crate::legacy::NumericPayload::Array { .. }
                    )),
                elements.saturating_add(
                    usize::try_from(record.payload.element_count()).unwrap_or(usize::MAX),
                ),
            )
        },
    )
}

pub(super) fn source_meta(scan: &ContainerScan) -> (SourceMeta, BTreeMap<String, usize>) {
    let mut attributes = BTreeMap::new();
    let mut coverage = BTreeMap::new();
    attributes.insert(
        "version_line".to_string(),
        scan.framing.version_line.clone(),
    );
    if let Some(name) = &scan.framing.model_name {
        attributes.insert("model_name".to_string(), name.clone());
    }
    attributes.insert(
        "layout".to_string(),
        scan.framing.layout.token().to_string(),
    );
    if let Some(legacy) = &scan.framing.legacy_ascii {
        attributes.insert("legacy_ascii_schema".to_string(), legacy.schema.clone());
        if let Some(release) = &legacy.product_release {
            attributes.insert("legacy_ascii_product_release".to_string(), release.clone());
        }
        attributes.insert(
            "legacy_ascii_declaration_count".to_string(),
            legacy.persistence.declaration_count().to_string(),
        );
        attributes.insert(
            "legacy_ascii_scope_count".to_string(),
            legacy.persistence.scopes.len().to_string(),
        );
        attributes.insert(
            "legacy_ascii_value_count".to_string(),
            legacy.persistence.value_count().to_string(),
        );
        attributes.insert(
            "legacy_ascii_continuation_count".to_string(),
            legacy.persistence.continuation_count().to_string(),
        );
        attributes.insert(
            "legacy_ascii_unresolved_value_count".to_string(),
            legacy.persistence.unresolved_value_count().to_string(),
        );
        attributes.insert(
            "legacy_ascii_conflicting_declaration_count".to_string(),
            legacy
                .persistence
                .conflicting_declaration_count()
                .to_string(),
        );
    }
    attributes.insert("file_size".to_string(), scan.framing.data.len().to_string());
    attributes.insert(
        "section_count".to_string(),
        scan.framing.sections.len().to_string(),
    );
    for (index, section) in scan.framing.sections.iter().enumerate() {
        let prefix = format!("section.{index}");
        attributes.insert(format!("{prefix}.name"), section.name.clone());
        attributes.insert(format!("{prefix}.raw_name"), section.raw_name.clone());
        attributes.insert(format!("{prefix}.role"), section.role.to_string());
        attributes.insert(format!("{prefix}.offset"), section.offset.to_string());
        attributes.insert(format!("{prefix}.length"), section.length.to_string());
    }
    if let Some(c) = scan.framing.census.srf_array_count {
        attributes.insert("srf_array_count".to_string(), c.to_string());
    }
    if let Some(c) = scan.framing.census.crv_array_count {
        attributes.insert("crv_array_count".to_string(), c.to_string());
    }
    if let Some(unit) = &scan.framing.principal_unit {
        attributes.insert("principal_unit".to_string(), unit.token());
        if scan.framing.layout == crate::container::Layout::LegacyAscii {
            if let Some(scale) = unit.length_scale_mm() {
                attributes.insert("source_length_scale_mm".to_string(), scale.to_string());
            }
        }
    }
    if scan.framing.layout == crate::container::Layout::LegacyAscii {
        coverage.insert(
            "decoded_legacy_principal_unit_count".to_string(),
            usize::from(scan.framing.principal_unit.is_some()),
        );
        if let Some(legacy) = &scan.framing.legacy_ascii {
            let mut object_arrows = 0usize;
            let mut object_inlines = 0usize;
            let mut object_nulls = 0usize;
            let mut object_arrays = 0usize;
            for record in &legacy.persistence.objects {
                match record.payload {
                    crate::legacy::ObjectPayload::Arrow => object_arrows += 1,
                    crate::legacy::ObjectPayload::Inline => object_inlines += 1,
                    crate::legacy::ObjectPayload::Null => object_nulls += 1,
                    crate::legacy::ObjectPayload::Array { .. } => object_arrays += 1,
                    crate::legacy::ObjectPayload::Opaque { .. } => {}
                }
            }
            coverage.insert(
                "decoded_legacy_object_arrow_count".to_string(),
                object_arrows,
            );
            coverage.insert(
                "decoded_legacy_object_inline_count".to_string(),
                object_inlines,
            );
            coverage.insert("decoded_legacy_object_null_count".to_string(), object_nulls);
            coverage.insert(
                "decoded_legacy_object_array_count".to_string(),
                object_arrays,
            );
            coverage.insert(
                "incomplete_legacy_object_array_count".to_string(),
                legacy.persistence.incomplete_object_array_count,
            );
            coverage.insert(
                "unresolved_legacy_object_value_count".to_string(),
                legacy.persistence.unresolved_object_value_count,
            );
            let (integer_scalars, integer_arrays, integer_elements) =
                legacy_numeric_coverage(&legacy.persistence.integer_values);
            coverage.insert(
                "decoded_legacy_integer_scalar_count".to_string(),
                integer_scalars,
            );
            coverage.insert(
                "decoded_legacy_integer_array_count".to_string(),
                integer_arrays,
            );
            coverage.insert(
                "decoded_legacy_integer_element_count".to_string(),
                integer_elements,
            );
            coverage.insert(
                "unresolved_legacy_integer_value_count".to_string(),
                legacy.persistence.unresolved_integer_value_count,
            );
            let (real_scalars, real_arrays, real_elements) =
                legacy_numeric_coverage(&legacy.persistence.real_values);
            coverage.insert("decoded_legacy_real_scalar_count".to_string(), real_scalars);
            coverage.insert("decoded_legacy_real_array_count".to_string(), real_arrays);
            coverage.insert(
                "decoded_legacy_real_element_count".to_string(),
                real_elements,
            );
            coverage.insert(
                "unresolved_legacy_real_value_count".to_string(),
                legacy.persistence.unresolved_real_value_count,
            );
            let (string_scalars, string_arrays, string_elements, undecoded_encodings) =
                legacy.persistence.string_values.iter().fold(
                    (0usize, 0usize, 0usize, 0usize),
                    |(scalars, arrays, elements, undecoded_encodings), record| {
                        (
                            scalars
                                + usize::from(matches!(
                                    record.payload,
                                    crate::legacy::StringPayload::Scalar { .. }
                                )),
                            arrays
                                + usize::from(matches!(
                                    record.payload,
                                    crate::legacy::StringPayload::Array { .. }
                                )),
                            elements.saturating_add(record.payload.element_count()),
                            undecoded_encodings
                                .saturating_add(record.payload.undecoded_encoding_count()),
                        )
                    },
                );
            coverage.insert(
                "decoded_legacy_string_scalar_count".to_string(),
                string_scalars,
            );
            coverage.insert(
                "decoded_legacy_string_array_count".to_string(),
                string_arrays,
            );
            coverage.insert(
                "decoded_legacy_string_element_count".to_string(),
                string_elements,
            );
            coverage.insert(
                "incomplete_legacy_string_array_count".to_string(),
                legacy.persistence.incomplete_string_array_count,
            );
            coverage.insert(
                "unresolved_legacy_string_value_count".to_string(),
                legacy.persistence.unresolved_string_value_count,
            );
            coverage.insert(
                "undecoded_legacy_string_encoding_count".to_string(),
                undecoded_encodings,
            );
            for (type_code, records, unresolved) in [
                (
                    3u8,
                    legacy.persistence.type_3_values.as_slice(),
                    legacy.persistence.unresolved_type_3_value_count,
                ),
                (
                    4u8,
                    legacy.persistence.type_4_values.as_slice(),
                    legacy.persistence.unresolved_type_4_value_count,
                ),
            ] {
                let scalars = records.len();
                let undecoded_encodings = records
                    .iter()
                    .map(|record| record.payload.undecoded_encoding_count())
                    .sum();
                coverage.insert(
                    format!("decoded_legacy_type_{type_code}_scalar_count"),
                    scalars,
                );
                coverage.insert(
                    format!("unresolved_legacy_type_{type_code}_value_count"),
                    unresolved,
                );
                coverage.insert(
                    format!("undecoded_legacy_type_{type_code}_encoding_count"),
                    undecoded_encodings,
                );
            }
            let mut insert_numbered_numeric_coverage =
                |type_code: u8, (scalars, arrays, elements), unresolved| {
                    coverage.insert(
                        format!("decoded_legacy_type_{type_code}_scalar_count"),
                        scalars,
                    );
                    coverage.insert(
                        format!("decoded_legacy_type_{type_code}_array_count"),
                        arrays,
                    );
                    coverage.insert(
                        format!("decoded_legacy_type_{type_code}_element_count"),
                        elements,
                    );
                    coverage.insert(
                        format!("unresolved_legacy_type_{type_code}_value_count"),
                        unresolved,
                    );
                };
            insert_numbered_numeric_coverage(
                5,
                legacy_numeric_coverage(&legacy.persistence.type_5_values),
                legacy.persistence.unresolved_type_5_value_count,
            );
            insert_numbered_numeric_coverage(
                6,
                legacy_numeric_coverage(&legacy.persistence.type_6_values),
                legacy.persistence.unresolved_type_6_value_count,
            );
            insert_numbered_numeric_coverage(
                7,
                legacy_numeric_coverage(&legacy.persistence.type_7_values),
                legacy.persistence.unresolved_type_7_value_count,
            );
            insert_numbered_numeric_coverage(
                9,
                legacy_numeric_coverage(&legacy.persistence.type_9_values),
                legacy.persistence.unresolved_type_9_value_count,
            );
            insert_numbered_numeric_coverage(
                11,
                legacy_numeric_coverage(&legacy.persistence.type_11_values),
                legacy.persistence.unresolved_type_11_value_count,
            );
        }
    }
    coverage.insert(
        "decoded_primitive_triangle_strip_count".to_string(),
        scan.primitives.triangle_strips.len(),
    );
    coverage.insert(
        "conflicting_primitive_triangle_strip_representation_count".to_string(),
        scan.primitives
            .conflicting_triangle_strip_representation_count,
    );
    coverage.insert(
        "decoded_surface_row_count".to_string(),
        scan.surfaces.rows.len(),
    );
    coverage.insert(
        "decoded_cross_section_surface_row_count".to_string(),
        scan.surfaces.cross_section_rows.len(),
    );
    coverage.insert(
        "decoded_surface_parameter_record_count".to_string(),
        scan.surfaces.parameters.len(),
    );
    coverage.insert(
        "decoded_cross_section_surface_parameter_record_count".to_string(),
        scan.surfaces.cross_section_parameters.len(),
    );
    coverage.insert(
        "decoded_positional_extrusion_direction_count".to_string(),
        scan.surfaces
            .parameters
            .iter()
            .filter(|record| {
                crate::surface::unique_surface_row(&scan.surfaces.rows, record.surface_id)
                    .is_some_and(|row| {
                        row.kind == crate::surface::SurfaceKind::Extrusion
                            && record.extrusion_direction(row.type_byte).is_some()
                    })
            })
            .count(),
    );
    let torus_coverage = torus_parameter_coverage(scan);
    coverage.insert(
        "decoded_torus_radius_override_count".to_string(),
        torus_coverage.radius_overrides,
    );
    coverage.insert(
        "decoded_type26_replayed_minor_radius_count".to_string(),
        torus_coverage.replayed_minor_radii,
    );
    coverage.insert(
        "decoded_torus_outline_extent_count".to_string(),
        torus_coverage.outline_extents,
    );
    coverage.insert(
        "decoded_type26_five_coordinate_envelope_count".to_string(),
        torus_coverage.five_coordinate_envelopes,
    );
    coverage.insert(
        "decoded_type26_split_coordinate_envelope_count".to_string(),
        torus_coverage.split_coordinate_envelopes,
    );
    coverage.insert(
        "decoded_plane_local_system_count".to_string(),
        scan.planes.local_systems.len(),
    );
    coverage.insert(
        "decoded_cross_section_plane_local_system_count".to_string(),
        scan.planes.cross_section_local_systems.len(),
    );
    coverage.insert(
        "decoded_plane_envelope_count".to_string(),
        scan.planes.envelopes.len(),
    );
    coverage.insert(
        "decoded_cross_section_plane_envelope_count".to_string(),
        scan.planes.cross_section_envelopes.len(),
    );
    coverage.insert(
        "decoded_outline_plane_count".to_string(),
        scan.planes.outlines.len(),
    );
    coverage.insert(
        "decoded_positional_frame_plane_count".to_string(),
        scan.planes.positional_frames.len(),
    );
    coverage.insert(
        "decoded_cross_section_outline_plane_count".to_string(),
        scan.planes.cross_section_outlines.len(),
    );
    coverage.insert(
        "decoded_surface_prototype_count".to_string(),
        scan.surfaces.prototypes.len(),
    );
    coverage.insert(
        "decoded_named_surface_prototype_count".to_string(),
        scan.surfaces.prototype_records.len(),
    );
    coverage.insert(
        "decoded_reference_line_count".to_string(),
        scan.references.lines.len(),
    );
    coverage.insert(
        "decoded_reference_circle_count".to_string(),
        scan.references.circles.len(),
    );
    coverage.insert(
        "decoded_reference_conic_count".to_string(),
        scan.references.conics.len(),
    );
    coverage.insert(
        "transferred_reference_ellipse_count".to_string(),
        scan.references.ellipses.len(),
    );
    coverage.insert(
        "decoded_tabulated_cylinder_curve_replay_count".to_string(),
        scan.curves.tabulated_cylinder_replays.len(),
    );
    coverage.insert(
        "decoded_tabulated_cylinder_control_point_set_count".to_string(),
        scan.curves
            .tabulated_cylinder_replays
            .iter()
            .filter(|replay| replay.control_points.iter().all(Option::is_some))
            .count(),
    );
    coverage.insert(
        "decoded_curve_prototype_count".to_string(),
        scan.curves.prototypes.len(),
    );
    coverage.insert(
        "decoded_curve_parameter_record_count".to_string(),
        scan.curves.parameters.len(),
    );
    coverage.insert(
        "decoded_curve_expression_record_count".to_string(),
        scan.curves.expressions.len(),
    );
    attributes.insert(
        "expanded_section_count".to_string(),
        scan.framing.expanded_sections.len().to_string(),
    );
    attributes.insert(
        "expanded_section_byte_count".to_string(),
        scan.framing
            .expanded_sections
            .iter()
            .map(|section| section.data.len())
            .sum::<usize>()
            .to_string(),
    );
    if let Some(family_table) = scan.framing.family_table {
        attributes.insert(
            "family_table_pointer".to_string(),
            match family_table.pointer {
                crate::container::FamilyTablePointer::Null => "null".to_string(),
                crate::container::FamilyTablePointer::Entity(id) => format!("entity:{id}"),
            },
        );
        attributes.insert(
            "configuration_state".to_string(),
            match family_table.pointer {
                crate::container::FamilyTablePointer::Null => "none".to_string(),
                crate::container::FamilyTablePointer::Entity(_) => {
                    "driver_table_unresolved".to_string()
                }
            },
        );
    }
    let configuration_driver_table_reference_count =
        usize::from(scan.framing.family_table.is_some_and(|table| {
            matches!(
                table.pointer,
                crate::container::FamilyTablePointer::Entity(_)
            )
        }));
    coverage.insert(
        "decoded_configuration_driver_table_reference_count".to_string(),
        configuration_driver_table_reference_count,
    );
    coverage.insert(
        "transferred_configuration_driver_table_count".to_string(),
        0,
    );
    coverage.insert(
        "decoded_pcurve_count".to_string(),
        scan.curves.pcurves.len(),
    );
    coverage.insert(
        "decoded_fc_curve_coordinate_record_count".to_string(),
        scan.curves.fc_coordinates.len(),
    );
    coverage.insert(
        "decoded_fc05_circle_count".to_string(),
        scan.curves.fc05_circles.len(),
    );
    coverage.insert(
        "decoded_fc05_cylinder_cap_pair_count".to_string(),
        scan.curves.fc05_cylinder_cap_pairs.len(),
    );
    coverage.insert(
        "decoded_prototype_pcurve_count".to_string(),
        scan.curves.prototype_pcurves.len(),
    );
    coverage.insert(
        "decoded_curve_prototype_topology_count".to_string(),
        scan.curves.prototype_topology.len(),
    );
    coverage.insert(
        "decoded_bound_prototype_pcurve_count".to_string(),
        scan.curves.bound_prototype_pcurves.len(),
    );
    coverage.insert(
        "decoded_curve_topology_row_count".to_string(),
        scan.curves.topology_rows.len(),
    );
    coverage.insert(
        "decoded_cross_section_curve_row_count".to_string(),
        scan.curves.cross_section_rows.len(),
    );
    coverage.insert(
        "decoded_cross_section_curve_prototype_count".to_string(),
        scan.curves.cross_section_prototypes.len(),
    );
    coverage.insert(
        "decoded_half_edge_count".to_string(),
        scan.topology.half_edges.len(),
    );
    coverage.insert(
        "decoded_topological_vertex_count".to_string(),
        scan.topology.vertices.len(),
    );
    coverage.insert("decoded_loop_count".to_string(), scan.topology.loops.len());
    coverage.insert(
        "decoded_face_component_count".to_string(),
        scan.topology.face_components.len(),
    );
    coverage.insert(
        "decoded_datum_plane_count".to_string(),
        scan.planes.datums.len(),
    );
    coverage.insert("decoded_feature_count".to_string(), scan.features.ids.len());
    coverage.insert(
        "decoded_feature_row_count".to_string(),
        scan.features.rows.len(),
    );
    coverage.insert(
        "decoded_feature_choice_count".to_string(),
        scan.features.choices.len(),
    );
    coverage.insert(
        "decoded_feature_choice_field_count".to_string(),
        scan.features.choice_fields.len(),
    );
    coverage.insert(
        "decoded_feature_geometry_table_count".to_string(),
        scan.features.geometry_tables.len(),
    );
    coverage.insert(
        "decoded_feature_loop_history_entry_count".to_string(),
        scan.features.loop_history_entries.len(),
    );
    coverage.insert(
        "decoded_feature_affected_id_array_count".to_string(),
        scan.features.affected_ids.len(),
    );
    coverage.insert(
        "decoded_feature_replay_affected_id_count".to_string(),
        scan.features.replay_affected_ids.len(),
    );
    coverage.insert(
        "decoded_surface_merge_replay_affected_id_count".to_string(),
        scan.features.surface_merge_replay_affected_ids.len(),
    );
    coverage.insert(
        "decoded_feature_loop_restore_direction_count".to_string(),
        scan.features.loop_restore_directions.len(),
    );
    coverage.insert(
        "decoded_feature_revolution_extent_count".to_string(),
        scan.features.revolution_extents.len(),
    );
    coverage.insert(
        "decoded_feature_definition_count".to_string(),
        scan.features.definitions.len(),
    );
    coverage.insert(
        "decoded_feature_section_transform_count".to_string(),
        scan.features.section_transforms.len(),
    );
    coverage.insert(
        "decoded_feature_placement_instruction_count".to_string(),
        scan.features
            .definitions
            .iter()
            .map(|definition| crate::feature::placement_instructions(definition).len())
            .sum::<usize>(),
    );
    coverage.insert(
        "decoded_feature_operation_state_count".to_string(),
        scan.features.operation_states.len(),
    );
    coverage.insert(
        "decoded_feature_operation_count".to_string(),
        scan.features.operations.len(),
    );
    coverage.insert(
        "decoded_feature_outline_count".to_string(),
        scan.features
            .definitions
            .iter()
            .map(|definition| definition.outlines.len())
            .sum::<usize>(),
    );
    coverage.insert(
        "decoded_feature_section_point_count".to_string(),
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.variables.as_ref())
            .map(|variables| {
                let (points, ambiguous) = variables.reconciled_points();
                points.len() + ambiguous.len()
            })
            .sum::<usize>(),
    );
    coverage.insert(
        "decoded_feature_solver_variable_count".to_string(),
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.variables.as_ref())
            .map(|variables| variables.rows.len())
            .sum::<usize>(),
    );
    coverage.insert(
        "missing_feature_solver_variable_count".to_string(),
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.variables.as_ref())
            .map(|variables| {
                usize::try_from(variables.declared_count)
                    .expect("u32 variable count fits usize")
                    .saturating_sub(variables.rows.len())
            })
            .sum::<usize>(),
    );
    let (
        decoded_dimension_driven_variable_count,
        decoded_dimension_driven_coordinate_variable_count,
    ) = scan
        .features
        .definitions
        .iter()
        .filter_map(|definition| definition.variables.as_ref())
        .flat_map(|variables| &variables.rows)
        .filter(|row| row.dimension_driven)
        .fold((0usize, 0usize), |(all, coordinates), row| {
            (
                all + 1,
                coordinates + usize::from(matches!(row.variable_type, 1 | 2)),
            )
        });
    let decoded_dimension_driven_guess_count = scan
        .features
        .definitions
        .iter()
        .filter_map(|definition| definition.variables.as_ref())
        .flat_map(|variables| &variables.rows)
        .filter(|row| row.guess_dimension_driven)
        .count();
    let (
        resolved_dimension_driven_variable_count,
        resolved_dimension_driven_coordinate_variable_count,
        resolved_dimension_driven_other_variable_count,
    ) = scan
        .features
        .definitions
        .iter()
        .map(|definition| {
            let resolved_coordinates = resolved_section_coordinates(definition);
            let resolved_radii = resolved_section_radii(definition);
            let resolved_scalars = resolved_section_scalar_values(definition);
            definition
                .variables
                .iter()
                .flat_map(|variables| &variables.rows)
                .filter(|row| row.dimension_driven)
                .fold(
                    (0usize, 0usize, 0usize),
                    |(all, coordinates, other), row| {
                        let resolved = match row.variable_type {
                            1 | 2 => resolved_coordinates
                                .get(&row.key)
                                .and_then(|point| point[usize::from(row.variable_type == 2)]),
                            3 => resolved_radii.get(&row.key).copied(),
                            _ => resolved_scalars.get(&(row.variable_type, row.key)).copied(),
                        };
                        (
                            all + usize::from(resolved.is_some()),
                            coordinates
                                + usize::from(
                                    matches!(row.variable_type, 1 | 2) && resolved.is_some(),
                                ),
                            other
                                + usize::from(
                                    !matches!(row.variable_type, 1 | 2) && resolved.is_some(),
                                ),
                        )
                    },
                )
        })
        .fold((0usize, 0usize, 0usize), |total, counts| {
            (total.0 + counts.0, total.1 + counts.1, total.2 + counts.2)
        });
    coverage.insert(
        "decoded_feature_dimension_driven_variable_count".to_string(),
        decoded_dimension_driven_variable_count,
    );
    coverage.insert(
        "decoded_feature_dimension_driven_coordinate_variable_count".to_string(),
        decoded_dimension_driven_coordinate_variable_count,
    );
    coverage.insert(
        "decoded_feature_dimension_driven_other_variable_count".to_string(),
        decoded_dimension_driven_variable_count
            .saturating_sub(decoded_dimension_driven_coordinate_variable_count),
    );
    coverage.insert(
        "decoded_feature_dimension_driven_guess_count".to_string(),
        decoded_dimension_driven_guess_count,
    );
    coverage.insert(
        "resolved_feature_dimension_driven_variable_count".to_string(),
        resolved_dimension_driven_variable_count,
    );
    coverage.insert(
        "resolved_feature_dimension_driven_coordinate_variable_count".to_string(),
        resolved_dimension_driven_coordinate_variable_count,
    );
    coverage.insert(
        "resolved_feature_dimension_driven_other_variable_count".to_string(),
        resolved_dimension_driven_other_variable_count,
    );
    coverage.insert(
        "unresolved_feature_dimension_driven_variable_count".to_string(),
        decoded_dimension_driven_variable_count
            .saturating_sub(resolved_dimension_driven_variable_count),
    );
    coverage.insert(
        "unresolved_feature_dimension_driven_coordinate_variable_count".to_string(),
        decoded_dimension_driven_coordinate_variable_count
            .saturating_sub(resolved_dimension_driven_coordinate_variable_count),
    );
    coverage.insert(
        "unresolved_feature_dimension_driven_other_variable_count".to_string(),
        decoded_dimension_driven_variable_count
            .saturating_sub(decoded_dimension_driven_coordinate_variable_count)
            .saturating_sub(resolved_dimension_driven_other_variable_count),
    );
    coverage.insert(
        "unresolved_feature_dimension_driven_guess_count".to_string(),
        decoded_dimension_driven_guess_count,
    );
    coverage.insert(
        "decoded_feature_circle_segment_count".to_string(),
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.segments.as_ref())
            .map(|segments| segments.circle_rows.len())
            .sum::<usize>(),
    );
    coverage.insert(
        "decoded_feature_point_segment_count".to_string(),
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.segments.as_ref())
            .map(|segments| segments.point_rows.len())
            .sum::<usize>(),
    );
    coverage.insert(
        "decoded_feature_centered_line_segment_count".to_string(),
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.segments.as_ref())
            .map(|segments| segments.centered_line_rows.len())
            .sum::<usize>(),
    );
    coverage.insert(
        "decoded_feature_reference_line_segment_count".to_string(),
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.segments.as_ref())
            .map(|segments| segments.reference_line_rows.len())
            .sum::<usize>(),
    );
    coverage.insert(
        "decoded_feature_bounded_curve_segment_count".to_string(),
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.segments.as_ref())
            .map(|segments| segments.bounded_curve_rows.len())
            .sum::<usize>(),
    );
    coverage.insert(
        "decoded_feature_conic_segment_count".to_string(),
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.segments.as_ref())
            .map(|segments| segments.conic_rows.len())
            .sum::<usize>(),
    );
    coverage.insert(
        "decoded_feature_opaque_segment_count".to_string(),
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.segments.as_ref())
            .map(|segments| segments.opaque_rows.len())
            .sum::<usize>(),
    );
    coverage.insert(
        "decoded_feature_trim_entity_count".to_string(),
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.trim_entities.as_ref())
            .map(|entities| entities.rows.len())
            .sum::<usize>(),
    );
    coverage.insert(
        "decoded_feature_trim_vertex_count".to_string(),
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.trim_vertices.as_ref())
            .map(|vertices| vertices.rows.len())
            .sum::<usize>(),
    );
    coverage.insert(
        "decoded_feature_order_entry_count".to_string(),
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.order_table.as_ref())
            .map(|order| order.rows.len())
            .sum::<usize>(),
    );
    coverage.insert(
        "decoded_feature_dimension_count".to_string(),
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.dimensions.as_ref())
            .map(|dimensions| dimensions.rows.len())
            .sum::<usize>(),
    );
    coverage.insert(
        "decoded_feature_relation_count".to_string(),
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.relations.as_ref())
            .map(|relations| relations.rows.len())
            .sum::<usize>(),
    );
    coverage.insert(
        "decoded_feature_equation_table_count".to_string(),
        scan.features
            .definitions
            .iter()
            .filter(|definition| {
                crate::feature::equation_table(&definition.body, 0, definition.body.len()).is_some()
            })
            .count(),
    );
    coverage.insert(
        "decoded_feature_equation_count".to_string(),
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| {
                crate::feature::equation_table(&definition.body, 0, definition.body.len())
            })
            .map(|equations| equations.rows.len())
            .sum::<usize>(),
    );
    coverage.insert(
        "decoded_feature_saved_entity_count".to_string(),
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.saved_section.as_ref())
            .map(|saved| saved.entities.len())
            .sum::<usize>(),
    );
    coverage.insert(
        "decoded_feature_saved_conic_count".to_string(),
        scan.features
            .definitions
            .iter()
            .filter_map(|definition| definition.saved_section.as_ref())
            .flat_map(|saved| &saved.entities)
            .filter(|entity| matches!(entity, crate::feature::FeatureSavedEntity::Conic(_)))
            .count(),
    );
    coverage.insert(
        "decoded_feature_entity_count".to_string(),
        scan.features.entities.len(),
    );
    coverage.insert(
        "decoded_feature_entity_reference_count".to_string(),
        scan.features.entity_references.len(),
    );
    coverage.insert(
        "decoded_feature_entity_table_count".to_string(),
        scan.features.entity_tables.len(),
    );
    coverage.insert(
        "decoded_feature_surface_replay_association_count".to_string(),
        feature_surface_replay_associations(scan).len(),
    );
    if let Some(count) = scan.framing.declared_body_count {
        attributes.insert("declared_body_count".to_string(), count.to_string());
    }
    if let Some(value) = scan.framing.first_quilt_ptr {
        attributes.insert("first_quilt_ptr".to_string(), value.to_string());
    }
    (
        SourceMeta {
            format: "creo".to_string(),
            attributes,
        },
        coverage,
    )
}

pub(super) fn has_transferred_geometry(ir: &CadIr) -> bool {
    let model = &ir.model;
    !model.points.is_empty()
        || !model.vertices.is_empty()
        || !model.edges.is_empty()
        || !model.coedges.is_empty()
        || !model.loops.is_empty()
        || !model.faces.is_empty()
        || !model.shells.is_empty()
        || !model.regions.is_empty()
        || !model.bodies.is_empty()
        || model
            .surfaces
            .iter()
            .any(|surface| !matches!(&surface.geometry, SurfaceGeometry::Unknown { .. }))
        || model
            .curves
            .iter()
            .any(|curve| !matches!(&curve.geometry, CurveGeometry::Unknown { .. }))
        || !model.subds.is_empty()
        || !model.pcurves.is_empty()
        || model.procedural_surfaces.iter().any(|surface| {
            !matches!(
                &surface.definition,
                ProceduralSurfaceDefinition::Unknown { .. }
            )
        })
        || model
            .procedural_curves
            .iter()
            .any(|curve| !matches!(&curve.definition, ProceduralCurveDefinition::Unknown { .. }))
        || model
            .sketch_entities
            .iter()
            .any(|entity| !matches!(&entity.geometry, SketchGeometry::Native { .. }))
        || !model.tessellations.is_empty()
}

/// Build diagnostics for data that cannot be represented in the emitted IR.
pub(super) fn build_report(
    scan: &ContainerScan,
    ir: &CadIr,
    coverage: BTreeMap<String, usize>,
    container_only: bool,
) -> DecodeReport {
    let count = |key: &str| coverage.get(key).copied().unwrap_or(0);
    let summary = container::summarize(scan);
    let geom_sections = scan
        .framing
        .sections
        .iter()
        .filter(|s| s.role == role::GEOMETRY)
        .count();
    let mut placed_plane_ids = scan
        .planes
        .local_systems
        .iter()
        .filter(|frame| {
            frame.origin.is_some()
                && frame.u_axis.is_some()
                && frame.normal.is_some_and(|normal| !is_axis_aligned(normal))
        })
        .map(|frame| frame.surface_id)
        .collect::<BTreeSet<_>>();
    placed_plane_ids.extend(scan.planes.outlines.iter().map(|plane| plane.surface_id));
    placed_plane_ids.extend(
        scan.planes
            .positional_frames
            .iter()
            .map(|plane| plane.surface_id),
    );
    let placed_plane_count = placed_plane_ids.len();
    let first_instance_prototype_surface_count =
        count("transferred_first_instance_prototype_surface_count");
    let positional_line_extrusion_plane_count =
        count("transferred_positional_line_extrusion_plane_count");
    let tabulated_cylinder_spline_extrusion_count =
        count("transferred_tabulated_cylinder_spline_extrusion_count");
    let positional_cone_count = count("transferred_positional_cone_count");
    let positional_cylinder_count = count("transferred_positional_cylinder_count");
    let paired_envelope_sphere_count = count("transferred_paired_envelope_sphere_count");
    let positional_torus_count = count("transferred_positional_torus_count");
    let topology_bound_plane_count = count("transferred_topology_bound_plane_surface_count");
    let mut losses = Vec::new();

    if container_only {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::ContainerOnly),
            severity: Severity::Info,
            message: "Container-only decode requested; entity transfer was skipped.".to_string(),
            provenance: None,
        });
    }

    // The namespace census: what is byte-backed and readable.
    let srf = scan
        .framing
        .census
        .srf_array_count
        .map_or_else(|| "n/a".to_string(), |c| c.to_string());
    let crv = scan
        .framing
        .census
        .crv_array_count
        .map_or_else(|| "n/a".to_string(), |c| c.to_string());
    losses.push(LossNote {
        code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::CarrierSummary),
        severity: Severity::Info,
        message: format!(
            "PSB container decoded structurally: {} section(s), {} layout, VisibGeom namespace \
             census srf_array={srf} / crv_array={crv}; {} typed surface rows, {} labeled curve \
             prototypes, {} canonical curve-topology rows, and {} closed native loops were decoded. \
             Outline-backed planes, guarded non-axis support frames, complete ND first-instance \
             plane, cylinder, cone, torus, and interpolation-spline prototypes, unbound straight positional \
             surface-of-extrusion planes, \
             topology-bound planes with analytic boundary carriers, `fc 05` cylinders with a \
             resolved axis-normal cap plane, four-entry two-cap and blind \
             circular-sweep cylinders, \
             four-entry simple-hole cylinders with complete cap outlines, radius-anchored \
             class-911 counterbore and bore patches, and compact simple-hole cylinders with \
             complete positional carriers, complementary split-outline cylinders \
             bound to an axis-normal plane, complete positional cylinder bodies, \
             complete support-apex and planar-envelope positional cones, and complete \
             local-system positional tori transfer as carriers; \
             other parameter bodies remain structural records.",
            scan.framing.sections.len(),
            scan.framing.layout.token(),
            scan.surfaces.rows.len(),
            scan.curves.prototypes.len(),
            scan.curves.topology_rows.len(),
            scan.topology.loops.len(),
        ),
        provenance: None,
    });

    let unresolved_legacy_reals = count("unresolved_legacy_real_value_count");
    if unresolved_legacy_reals != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::RecordNotTyped),
            severity: Severity::Warning,
            message: format!(
                "{unresolved_legacy_reals} legacy type-2 value row(s) did not form a complete \
                 finite scalar or dimension-complete real array."
            ),
            provenance: None,
        });
    }
    let unresolved_legacy_integers = count("unresolved_legacy_integer_value_count");
    if unresolved_legacy_integers != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::RecordNotTyped),
            severity: Severity::Warning,
            message: format!(
                "{unresolved_legacy_integers} legacy type-1 value row(s) did not form a signed \
                 32-bit scalar or dimension-complete integer array."
            ),
            provenance: None,
        });
    }
    for type_code in [3u8, 4] {
        let unresolved = count(&format!("unresolved_legacy_type_{type_code}_value_count"));
        if unresolved != 0 {
            losses.push(LossNote {
                code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::RecordNotTyped),
                severity: Severity::Warning,
                message: format!(
                    "{unresolved} legacy type-{type_code} value row(s) use an undefined \
                     continuation form."
                ),
                provenance: None,
            });
        }
        let undecoded = count(&format!("undecoded_legacy_type_{type_code}_encoding_count"));
        if undecoded != 0 {
            losses.push(LossNote {
                code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::AttributesNotTransferred),
                severity: Severity::Warning,
                message: format!(
                    "{undecoded} legacy type-{type_code} byte-string value(s) retain exact \
                     source bytes because their character encoding is not UTF-8."
                ),
                provenance: None,
            });
        }
    }
    for type_code in [5u8, 7, 9, 11] {
        let unresolved = count(&format!("unresolved_legacy_type_{type_code}_value_count"));
        if unresolved != 0 {
            losses.push(LossNote {
                code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::RecordNotTyped),
                severity: Severity::Warning,
                message: format!(
                    "{unresolved} legacy type-{type_code} value row(s) did not form an unsigned \
                     32-bit scalar or dimension-complete unsigned array."
                ),
                provenance: None,
            });
        }
    }
    let unresolved_legacy_type_6 = count("unresolved_legacy_type_6_value_count");
    if unresolved_legacy_type_6 != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::RecordNotTyped),
            severity: Severity::Warning,
            message: format!(
                "{unresolved_legacy_type_6} legacy type-6 value row(s) did not form a complete \
                 finite compact-real scalar or dimension-complete real array."
            ),
            provenance: None,
        });
    }
    let incomplete_legacy_object_arrays = count("incomplete_legacy_object_array_count");
    if incomplete_legacy_object_arrays != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::RecordNotTyped),
            severity: Severity::Warning,
            message: format!(
                "{incomplete_legacy_object_arrays} legacy type-0 object array(s) have a direct \
                 element count that differs from their declared extents."
            ),
            provenance: None,
        });
    }
    let unresolved_legacy_objects = count("unresolved_legacy_object_value_count");
    if unresolved_legacy_objects != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::RecordNotTyped),
            severity: Severity::Warning,
            message: format!(
                "{unresolved_legacy_objects} legacy type-0 value row(s) use an undefined object \
                 payload form."
            ),
            provenance: None,
        });
    }
    let incomplete_legacy_string_arrays = count("incomplete_legacy_string_array_count");
    if incomplete_legacy_string_arrays != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::RecordNotTyped),
            severity: Severity::Warning,
            message: format!(
                "{incomplete_legacy_string_arrays} legacy type-10 string array(s) have a direct \
                 element count that differs from their first extent."
            ),
            provenance: None,
        });
    }
    let unresolved_legacy_strings = count("unresolved_legacy_string_value_count");
    if unresolved_legacy_strings != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::RecordNotTyped),
            severity: Severity::Warning,
            message: format!(
                "{unresolved_legacy_strings} legacy type-10 value row(s) use an undefined \
                 continuation form."
            ),
            provenance: None,
        });
    }
    let undecoded_legacy_string_encodings = count("undecoded_legacy_string_encoding_count");
    if undecoded_legacy_string_encodings != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::AttributesNotTransferred),
            severity: Severity::Warning,
            message: format!(
                "{undecoded_legacy_string_encodings} legacy type-10 string element(s) retain \
                 exact source bytes because their character encoding is not UTF-8."
            ),
            provenance: None,
        });
    }

    let conflicting_triangle_strip_representations =
        count("conflicting_primitive_triangle_strip_representation_count");
    if conflicting_triangle_strip_representations != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::GeometryNotTransferred),
            severity: Severity::Warning,
            message: format!(
                "{conflicting_triangle_strip_representations} primitive triangle-strip record(s) \
                 contain complete position representations that disagree."
            ),
            provenance: None,
        });
    }

    // The core prototype-vs-instance limitation.
    losses.push(LossNote {
        code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::GeometryNotTransferred),
        severity: Severity::Blocking,
        message: format!(
            "General model B-rep transfer remains incomplete. Native face components transfer \
             when every boundary edge has solved vertex orbits, face orientation is unique, and \
             every loop is complete; a multi-loop planar face additionally requires one strict \
             containment outer boundary. Selected \
             cylinders transfer when an exact `fc 05` record and placed cap outline binds a row, \
             a four-entry class-917 circular-sweep or class-911 simple-hole table with a complete \
             square cap outline establishes the complete axis placement and radius, or a compact \
             class-911 table owns a complete positional cylinder carrier, a class-911 \
             counterbore dimension replay agrees with its generated larger-cylinder carrier, or two same-feature \
             patches have complementary square outline bounds on one axis-normal plane. Later positional \
             instances do not inherit prototype placement or scalar \
             defaults; they require their per-instance parameter bodies \
             ([spec §4.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#32-surface-prototypes)). {geom_sections} PSB geometry section(s) were preserved verbatim as unknown \
             records."
        ),
        provenance: None,
    });

    if !container_only && placed_plane_count != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::CarrierSummary),
            severity: Severity::Info,
            message: format!(
                "Transferred {placed_plane_count} model-space plane carrier(s) from complete \
                 VisibGeom local-system support frames."
            ),
            provenance: None,
        });
    }

    if !container_only && topology_bound_plane_count != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::CarrierSummary),
            severity: Severity::Info,
            message: format!(
                "Transferred {topology_bound_plane_count} model-space plane carrier(s) from \
                 circle, ellipse, or line boundary carriers, coplanar NURBS control nets, or \
                 three or more non-collinear solved boundary vertices of the same native face."
            ),
            provenance: None,
        });
    }

    if !container_only && first_instance_prototype_surface_count != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::CarrierSummary),
            severity: Severity::Info,
            message: format!(
                "Transferred {first_instance_prototype_surface_count} first-instance ND plane, \
                 cylinder, cone, torus, or interpolation-spline carrier(s) from complete named \
                 parameters."
            ),
            provenance: None,
        });
    }

    if !container_only && paired_envelope_sphere_count != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::CarrierSummary),
            severity: Severity::Info,
            message: format!(
                "Transferred {paired_envelope_sphere_count} sphere carrier(s) from complementary \
                 five-coordinate type-26 hemisphere envelopes and their shared zero-major-radius \
                 prototype."
            ),
            provenance: None,
        });
    }

    if !container_only && positional_torus_count != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::CarrierSummary),
            severity: Severity::Info,
            message: format!(
                "Transferred {positional_torus_count} exact positional torus carrier(s) from \
                 complete local-system, radius, and five-coordinate envelope bodies."
            ),
            provenance: None,
        });
    }

    if !container_only && positional_cylinder_count != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::CarrierSummary),
            severity: Severity::Info,
            message: format!(
                "Transferred {positional_cylinder_count} exact positional cylinder carrier(s) \
                 from complete per-instance parameter bodies."
            ),
            provenance: None,
        });
    }

    if !container_only && positional_cone_count != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::CarrierSummary),
            severity: Severity::Info,
            message: format!(
                "Transferred {positional_cone_count} exact positional cone carrier(s) from \
                 complete support-apex or planar-envelope bodies."
            ),
            provenance: None,
        });
    }

    if !container_only && positional_line_extrusion_plane_count != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::CarrierSummary),
            severity: Severity::Info,
            message: format!(
                "Transferred {positional_line_extrusion_plane_count} unbound straight positional \
                 surface-of-extrusion carrier(s) from complete sweep-direction and directrix \
                 frames."
            ),
            provenance: None,
        });
    }

    if !container_only && tabulated_cylinder_spline_extrusion_count != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::CarrierSummary),
            severity: Severity::Info,
            message: format!(
                "Transferred {tabulated_cylinder_spline_extrusion_count} tabulated-cylinder \
                 cubic spline extrusion carrier(s) from uniquely matched directrix and frame spans."
            ),
            provenance: None,
        });
    }

    if !container_only && !scan.planes.datums.is_empty() {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::CarrierSummary),
            severity: Severity::Info,
            message: format!(
                "Transferred {} exact model-space construction datum plane carrier(s) from ActDatums; \
                 these are unbounded reference planes, not model B-rep faces.",
                scan.planes.datums.len()
            ),
            provenance: None,
        });
    }

    if !container_only && !scan.references.lines.is_empty() {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::CarrierSummary),
            severity: Severity::Info,
            message: format!(
                "Transferred {} finite model-space reference line carrier(s) from MdlRefInfo; \
                 their byte-exact endpoints remain attached as native line records.",
                scan.references.lines.len()
            ),
            provenance: None,
        });
    }

    if !container_only && !scan.references.circles.is_empty() {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::CarrierSummary),
            severity: Severity::Info,
            message: format!(
                "Transferred {} circular reference carrier(s) from MdlRefInfo rows whose stored center, radius, and endpoints satisfy the circle equation; byte-exact endpoints remain attached as native circle records.",
                scan.references.circles.len()
            ),
            provenance: None,
        });
    }

    if !container_only && !scan.references.ellipses.is_empty() {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::CarrierSummary),
            severity: Severity::Info,
            message: format!(
                "Transferred {} elliptical reference carrier(s) from MdlRefInfo conic rows whose frame, coefficient radii, and antipodal endpoints satisfy one ellipse equation; the source conic records remain byte-exact native records.",
                scan.references.ellipses.len()
            ),
            provenance: None,
        });
    }

    let topological_point_count = count("transferred_topological_point_count");
    if !container_only && topological_point_count != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::CarrierSummary),
            severity: Severity::Info,
            message: format!(
                "Transferred {topological_point_count} exact model-space point(s) for native topological vertex orbits from unique placed-carrier intersections or pcurve endpoint domains constrained by agreeing face maps and incident analytic edge carriers."
            ),
            provenance: None,
        });
    }

    let native_topological_edge_count = count("transferred_native_topological_edge_count");
    if !container_only && native_topological_edge_count != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::CarrierSummary),
            severity: Severity::Info,
            message: format!(
                "Transferred {native_topological_edge_count} native topological edge(s) whose endpoint vertex orbits have exact model-space points."
            ),
            provenance: None,
        });
    }

    let analytic_pcurve_carrier_count = count("transferred_analytic_pcurve_carrier_count");
    if !container_only && analytic_pcurve_carrier_count != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::CarrierSummary),
            severity: Severity::Info,
            message: format!(
                "Transferred {analytic_pcurve_carrier_count} exact analytic carrier(s) by mapping native linear pcurves through placed planar, cylindrical, conical, spherical, or toroidal face charts."
            ),
            provenance: None,
        });
    }

    let extrusion_plane_boundary_curve_count =
        count("transferred_extrusion_plane_boundary_curve_count");
    if !container_only && extrusion_plane_boundary_curve_count != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::CarrierSummary),
            severity: Severity::Info,
            message: format!(
                "Transferred {extrusion_plane_boundary_curve_count} exact NURBS boundary \
                 carrier(s) where one tabulated-extrusion boundary lies in an adjacent plane \
                 and every other control point lies strictly on one side."
            ),
            provenance: None,
        });
    }

    let extrusion_plane_section_generator_curve_count =
        count("transferred_extrusion_plane_section_generator_curve_count");
    if !container_only && extrusion_plane_section_generator_curve_count != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::CarrierSummary),
            severity: Severity::Info,
            message: format!(
                "Transferred {extrusion_plane_section_generator_curve_count} exact NURBS \
                 generator carrier(s) where an adjacent plane contains the sweep direction and \
                 the cubic directrix has exactly one plane intersection."
            ),
            provenance: None,
        });
    }

    let shared_extrusion_generator_curve_count =
        count("transferred_shared_extrusion_generator_curve_count");
    if !container_only && shared_extrusion_generator_curve_count != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::CarrierSummary),
            severity: Severity::Info,
            message: format!(
                "Transferred {shared_extrusion_generator_curve_count} exact shared NURBS \
                 generator carrier(s) whose two tabulated-extrusion control nets meet on the \
                 same linear boundary and lie strictly on opposite sides of a plane through it."
            ),
            provenance: None,
        });
    }

    let torus_coverage = torus_parameter_coverage(scan);
    if torus_coverage.radius_overrides != 0
        || torus_coverage.replayed_minor_radii != 0
        || torus_coverage.outline_extents != 0
        || torus_coverage.five_coordinate_envelopes != 0
        || torus_coverage.split_coordinate_envelopes != 0
    {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::CarrierSummary),
            severity: Severity::Info,
            message: format!(
                "Retained {} tagged type-26 radius override(s), {} prototype-minor-radius \
                 replay(s), {} terminal outline extent(s), {} five-coordinate envelope(s), and \
                 {} split-coordinate envelope(s). These row-local fields remain byte-exact native \
                 data. Placement-complete paired sphere envelopes additionally transfer as \
                 analytic carriers.",
                torus_coverage.radius_overrides,
                torus_coverage.replayed_minor_radii,
                torus_coverage.outline_extents,
                torus_coverage.five_coordinate_envelopes,
                torus_coverage.split_coordinate_envelopes,
            ),
            provenance: None,
        });
    }

    // The specific undecoded PSB layers that gate per-instance geometry.
    losses.push(LossNote {
        code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::GeometryNotTransferred),
        severity: Severity::Blocking,
        message: "Additional model-space carriers are gated by unresolved lane-specific scalar \
                  prefixes, feature-local transform bindings, placement-incomplete or untagged \
                  `0x26` torus/sphere variants, and the round/fillet feature evaluator. These gaps \
                  prevent transfer of the remaining non-plane per-instance surfaces, curves, and \
                  vertices."
            .to_string(),
        provenance: None,
    });

    // Topology.
    losses.push(LossNote {
        code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::TopologyNotTransferred),
        severity: Severity::Blocking,
        message: "Native curve half-edges and closed loops were decoded. Components with complete \
                  solved boundaries and unique face orientations transfer as \
                  body/region/shell/face/loop/coedge/edge/vertex graphs; multi-loop faces use \
                  strict containment in a placed or boundary-proven plane. Remaining components \
                  require face-instance partitioning, surface parameter bindings, curve geometry, \
                  or vertex coordinates."
            .to_string(),
        provenance: None,
    });

    let configuration_gap = match scan.framing.family_table.map(|record| record.pointer) {
        Some(crate::container::FamilyTablePointer::Null) => "",
        Some(crate::container::FamilyTablePointer::Entity(_)) => {
            ", configuration driver-table rows"
        }
        None => ", configuration presence",
    };
    let unevaluated_curve_expression_record_count = scan
        .curves
        .expressions
        .iter()
        .filter(|record| {
            !record.backup
                && (!record.prohibited_constructs.is_empty()
                    || record.solve_blocks.iter().any(|block| {
                        block.solutions.is_empty() || block.solutions.iter().any(Option::is_none)
                    })
                    || record.unresolved_solve_control)
        })
        .count();
    let curve_expression_transfer = if unevaluated_curve_expression_record_count == 0 {
        "Curve-equation assignments transfer with their source, dependencies, and closed numeric \
         and string operator and deterministic function values."
            .to_string()
    } else {
        format!(
            "Admitted curve-equation assignments transfer with their source, dependencies, and \
             closed numeric and string operator and deterministic function values. \
             {unevaluated_curve_expression_record_count} active curve-equation record(s) \
             containing prohibited datum-curve constructs or unresolved simultaneous-solve \
             control retain \
             source and dependencies without solve-dependent assignment values or derived curves."
        )
    };

    // Features, history, materials.
    losses.push(LossNote {
        code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
        severity: Severity::Warning,
        message: format!(
            "Named feature operations and their decoded dependency/input tables transfer as typed \
             or native design records. {curve_expression_transfer} \
             Full neutral operation semantics\
             {configuration_gap}, graph, case-study, cabling, and cross-model relation functions, \
             materials, and display data \
             remain untransferred."
        ),
        provenance: None,
    });

    // Coverage drops: VisibGeom rows and curve-equation records that decoded
    // but could not be transferred, resolved, or evaluated.
    let untransferred_surface_rows = count("untransferred_visible_surface_row_count");
    if untransferred_surface_rows != 0 {
        let unresolved_families = SURFACE_KINDS
            .into_iter()
            .filter_map(|kind| {
                let family = surface_family(kind);
                let count = count(&format!("untransferred_visible_{family}_surface_row_count"));
                (count != 0).then_some(format!("{family}={count}"))
            })
            .collect::<Vec<_>>()
            .join(", ");
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::GeometryNotTransferred),
            severity: Severity::Warning,
            message: format!(
                "{untransferred_surface_rows} unique VisibGeom surface row(s) were not \
                 transferred as carriers and remain structural namespace records \
                 ({unresolved_families})."
            ),
            provenance: None,
        });
    }
    let untransferred_curve_rows = count("untransferred_visible_curve_row_count");
    if untransferred_curve_rows != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::GeometryNotTransferred),
            severity: Severity::Warning,
            message: format!(
                "{untransferred_curve_rows} unique VisibGeom curve-topology row(s) were not \
                 transferred as carriers and remain structural namespace records."
            ),
            provenance: None,
        });
    }
    let ambiguous_surface_rows = count("ambiguous_visible_surface_row_count");
    if ambiguous_surface_rows != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::GeometryNotTransferred),
            severity: Severity::Info,
            message: format!(
                "{ambiguous_surface_rows} VisibGeom surface row(s) share a non-unique identity \
                 and were not resolved to a single carrier."
            ),
            provenance: None,
        });
    }
    let ambiguous_curve_rows = count("ambiguous_visible_curve_row_count");
    if ambiguous_curve_rows != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::GeometryNotTransferred),
            severity: Severity::Info,
            message: format!(
                "{ambiguous_curve_rows} VisibGeom curve-topology row(s) share a non-unique \
                 identity and were not resolved to a single carrier."
            ),
            provenance: None,
        });
    }
    let missing_segment_rows = count("missing_feature_segment_row_count");
    if missing_segment_rows != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Warning,
            message: format!(
                "{missing_segment_rows} declared section segment row(s) did not decode and remain \
                 unavailable to the defining sketch."
            ),
            provenance: None,
        });
    }
    let missing_relation_rows = count("missing_feature_relation_row_count");
    if missing_relation_rows != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Warning,
            message: format!(
                "{missing_relation_rows} declared section relation row(s) did not decode; the \
                 affected complete-table solver identities remain unavailable."
            ),
            provenance: None,
        });
    }
    let malformed_relation_tables = count("malformed_feature_relation_table_count");
    if malformed_relation_tables != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Warning,
            message: format!(
                "{malformed_relation_tables} section relation table(s) use the invalid zero \
                 allocation count."
            ),
            provenance: None,
        });
    }
    let missing_skamp_rows = count("missing_feature_skamp_row_count");
    if missing_skamp_rows != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Warning,
            message: format!(
                "{missing_skamp_rows} declared section incidence row(s) did not decode; the \
                 affected complete-table solver identities remain unavailable."
            ),
            provenance: None,
        });
    }
    let missing_triple_rows = count("missing_feature_relation_triple_row_count");
    if missing_triple_rows != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Warning,
            message: format!(
                "{missing_triple_rows} declared section relation-incidence join row(s) did not \
                 decode; the affected complete-table solver identities remain unavailable."
            ),
            provenance: None,
        });
    }
    let unresolved_segment_geometry = count("unresolved_feature_segment_geometry_count");
    if unresolved_segment_geometry != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::GeometryNotTransferred),
            severity: Severity::Warning,
            message: format!(
                "{unresolved_segment_geometry} decoded section segment(s) retain source-native \
                 geometry because their exact neutral construction remains unresolved."
            ),
            provenance: None,
        });
    }
    let active_native_skamps = count("active_native_feature_skamp_constraint_count");
    if active_native_skamps != 0 {
        let kinds = constraint_kind_breakdown(&coverage, "active_native_feature_skamp_type_");
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Warning,
            message: format!(
                "{active_native_skamps} active section incidence constraint(s) retain native \
                 operands because their neutral semantics or referenced geometry remain unresolved \
                 ({kinds})."
            ),
            provenance: None,
        });
    }
    let active_native_relations = count("active_native_feature_relation_constraint_count");
    if active_native_relations != 0 {
        let kinds = constraint_kind_breakdown(&coverage, "active_native_feature_relation_type_");
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Warning,
            message: format!(
                "{active_native_relations} active section dimension relation(s) retain native \
                 operands because their neutral semantics, incidence join, or referenced geometry \
                 remain unresolved ({kinds})."
            ),
            provenance: None,
        });
    }
    let incomplete_sweeps = count("transferred_incomplete_sweep_feature_count");
    if incomplete_sweeps != 0 {
        let families = [
            (
                "extrude",
                count("transferred_incomplete_extrude_feature_count"),
            ),
            (
                "revolve",
                count("transferred_incomplete_revolve_feature_count"),
            ),
        ]
        .into_iter()
        .filter_map(|(family, count)| (count != 0).then_some(format!("{family}={count}")))
        .collect::<Vec<_>>()
        .join(", ");
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Warning,
            message: format!(
                "{incomplete_sweeps} profile sweep history feature(s) retain incomplete required \
                 construction operands ({families})."
            ),
            provenance: None,
        });
    }
    let incomplete_surface_operations =
        count("transferred_incomplete_surface_operation_feature_count");
    if incomplete_surface_operations != 0 {
        let families = [
            (
                "fill",
                count("transferred_incomplete_filled_surface_feature_count"),
            ),
            (
                "knit",
                count("transferred_incomplete_knit_surface_feature_count"),
            ),
            (
                "thicken",
                count("transferred_incomplete_thicken_feature_count"),
            ),
        ]
        .into_iter()
        .filter_map(|(family, count)| (count != 0).then_some(format!("{family}={count}")))
        .collect::<Vec<_>>()
        .join(", ");
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Warning,
            message: format!(
                "{incomplete_surface_operations} surface construction history feature(s) retain \
                 incomplete required operands ({families})."
            ),
            provenance: None,
        });
    }
    let incomplete_other_constructions =
        count("transferred_incomplete_other_construction_feature_count");
    if incomplete_other_constructions != 0 {
        let families = [
            (
                "section shape",
                count("transferred_incomplete_section_shape_feature_count"),
            ),
            (
                "pattern",
                count("transferred_incomplete_pattern_feature_count"),
            ),
            (
                "native-axis helix",
                count("transferred_native_axis_helix_feature_count"),
            ),
        ]
        .into_iter()
        .filter_map(|(family, count)| (count != 0).then_some(format!("{family}={count}")))
        .collect::<Vec<_>>()
        .join(", ");
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Warning,
            message: format!(
                "{incomplete_other_constructions} construction history feature(s) retain \
                 unresolved neutral operands ({families})."
            ),
            provenance: None,
        });
    }
    let incomplete_recognized_features = count("transferred_incomplete_recognized_feature_count");
    if incomplete_recognized_features != 0 {
        let families = [
            ("hole", count("transferred_incomplete_hole_feature_count")),
            (
                "fillet",
                count("transferred_incomplete_fillet_feature_count"),
            ),
            (
                "chamfer",
                count("transferred_incomplete_chamfer_feature_count"),
            ),
            ("draft", count("transferred_incomplete_draft_feature_count")),
        ]
        .into_iter()
        .filter_map(|(family, count)| (count != 0).then_some(format!("{family}={count}")))
        .collect::<Vec<_>>()
        .join(", ");
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Warning,
            message: format!(
                "{incomplete_recognized_features} recognized non-sweep history feature(s) retain \
                 incomplete required construction operands ({families})."
            ),
            provenance: None,
        });
    }
    let explicitly_unresolved_features = count("transferred_explicitly_unresolved_feature_count");
    let native_features = count("transferred_native_feature_count");
    if native_features != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Warning,
            message: format!(
                "{native_features} history feature definition(s) retain only source-native \
                 semantics."
            ),
            provenance: None,
        });
    }
    if explicitly_unresolved_features != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Warning,
            message: format!(
                "{explicitly_unresolved_features} typed history feature definition(s) retain an \
                 explicitly unresolved model-space construction."
            ),
            provenance: None,
        });
    }
    let unresolved_dimension_driven_variables =
        count("unresolved_feature_dimension_driven_variable_count");
    if unresolved_dimension_driven_variables != 0 {
        let unresolved_coordinate_variables =
            count("unresolved_feature_dimension_driven_coordinate_variable_count");
        let other_variables = count("unresolved_feature_dimension_driven_other_variable_count");
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Warning,
            message: format!(
                "{unresolved_dimension_driven_variables} dimension-driven section solver \
                 variable(s) retain unresolved exact values: {unresolved_coordinate_variables} \
                 coordinate variable(s) lack a complete dimension equation and {other_variables} \
                 variable(s) have a non-coordinate family whose dimension semantics are \
                 unresolved."
            ),
            provenance: None,
        });
    }
    let unresolved_dimension_driven_guesses =
        count("unresolved_feature_dimension_driven_guess_count");
    if unresolved_dimension_driven_guesses != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Warning,
            message: format!(
                "{unresolved_dimension_driven_guesses} section solver variable pre-solve \
                 estimate(s) use a dimension-driven sentinel whose dimension join is unresolved."
            ),
            provenance: None,
        });
    }
    let missing_solver_variables = count("missing_feature_solver_variable_count");
    if missing_solver_variables != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Warning,
            message: format!(
                "{missing_solver_variables} declared section solver variable row(s) did not \
                 decode; stored and equation-derived coordinates are withheld for the incomplete \
                 table."
            ),
            provenance: None,
        });
    }
    let unresolved_dimension_values = count("unresolved_feature_dimension_value_count");
    if unresolved_dimension_values != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Warning,
            message: format!(
                "{unresolved_dimension_values} section dimension(s) retain source-native value \
                 tokens because their exact scalar encodings remain unresolved."
            ),
            provenance: None,
        });
    }
    let unresolved_configuration_driver_tables =
        count("decoded_configuration_driver_table_reference_count")
            .saturating_sub(count("transferred_configuration_driver_table_count"));
    if unresolved_configuration_driver_tables != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Warning,
            message: format!(
                "{unresolved_configuration_driver_tables} referenced configuration driver \
                 table(s) retain unresolved traversal and row semantics."
            ),
            provenance: None,
        });
    }
    let prohibited_records = count("prohibited_active_curve_expression_record_count");
    if prohibited_records != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Warning,
            message: format!(
                "{prohibited_records} active curve-equation record(s) containing prohibited \
                 datum-curve constructs were not evaluated; source and dependencies were \
                 retained without values or derived curves."
            ),
            provenance: None,
        });
    }
    let unresolved_solve_blocks = count("decoded_active_curve_expression_solve_block_count")
        .saturating_sub(count("evaluated_active_curve_expression_solve_block_count"));
    if unresolved_solve_blocks != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Warning,
            message: format!(
                "{unresolved_solve_blocks} active curve-equation simultaneous-solve block(s) \
                 retain their ordered equations and unknowns without solved values or derived \
                 curves."
            ),
            provenance: None,
        });
    }
    let unresolved_solve_controls = count("unresolved_active_curve_expression_solve_control_count");
    if unresolved_solve_controls != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Warning,
            message: format!(
                "{unresolved_solve_controls} active curve-equation record(s) retain malformed or \
                 incomplete simultaneous-solve control without sequentially interpreting its \
                 bounded source lines."
            ),
            provenance: None,
        });
    }
    let prohibited_kinds = count("prohibited_active_curve_expression_kind_count");
    if prohibited_kinds != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Warning,
            message: format!(
                "{prohibited_kinds} prohibited datum-curve construct(s) across active \
                 curve-equation records were not evaluated."
            ),
            provenance: None,
        });
    }

    DecodeReport {
        format: "creo".to_string(),
        container_only,
        geometry_transferred: has_transferred_geometry(ir),
        coverage,
        transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
        losses,
        notes: summary.notes,
    }
}
