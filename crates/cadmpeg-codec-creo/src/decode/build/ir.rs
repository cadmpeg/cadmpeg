// SPDX-License-Identifier: Apache-2.0
//! Container IR bootstrap and model-entity assembly.

use std::collections::BTreeMap;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::VertexSelection;
use cadmpeg_ir::features::{
    BodySelection, EdgeSelection, FaceSelection, PathRef, PatternKind, SurfaceBoundary, Termination,
};
use cadmpeg_ir::geometry::{Curve, CurveGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::{CurveId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::tessellation::Tessellation;
use cadmpeg_ir::units::Units;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::AnnotationBuilder;
use cadmpeg_ir::{Exactness, SourceObjectAssociation};

use crate::container::ContainerScan;

use super::super::analytic::placed_plane_surfaces;
use super::super::expanded::attach_expanded_sections;
use super::super::native::annotate;
use super::super::sketch::normalized;
use super::arenas::{emit_geometry_arenas, emit_reference_arenas};
use super::coverage::collect_feature_coverage;
use super::ir_features::{emit_model_features, finish_feature_transfers};
use super::ir_geometry::transfer_and_record_scanned_geometry;
use super::meta::source_meta;
use super::passthrough::{emit_legacy_arenas, preserve_passthrough_sections};

pub(in super::super) struct BuiltIr {
    pub(in super::super) ir: CadIr,
    pub(in super::super) annotations: cadmpeg_ir::Annotations,
    pub(in super::super) unknowns: Vec<UnknownRecord>,
    pub(in super::super) coverage: BTreeMap<String, usize>,
}

pub(in super::super) fn build_container_ir(scan: &ContainerScan) -> Result<BuiltIr, CodecError> {
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

pub(in super::super) fn face_selection_has_unresolved_operands(selection: &FaceSelection) -> bool {
    matches!(
        selection,
        FaceSelection::Unresolved
            | FaceSelection::HistoricalPartial { .. }
            | FaceSelection::Native(_)
    )
}

pub(in super::super) fn body_selection_has_unresolved_operands(selection: &BodySelection) -> bool {
    matches!(
        selection,
        BodySelection::Unresolved | BodySelection::Native(_) | BodySelection::NativeSet(_)
    )
}

pub(in super::super) fn edge_selection_has_unresolved_operands(selection: &EdgeSelection) -> bool {
    matches!(
        selection,
        EdgeSelection::Unresolved
            | EdgeSelection::HistoricalPartial { .. }
            | EdgeSelection::Native(_)
    )
}

pub(in super::super) fn path_has_unresolved_operands(path: &PathRef) -> bool {
    matches!(
        path,
        PathRef::Unresolved(_) | PathRef::Native(_) | PathRef::SpatialSketchSelection { .. }
    )
}

pub(in super::super) fn surface_boundary_has_unresolved_operands(
    boundary: &SurfaceBoundary,
) -> bool {
    match boundary {
        SurfaceBoundary::Edges(edges) => edge_selection_has_unresolved_operands(edges),
        SurfaceBoundary::Path(path) => path_has_unresolved_operands(path),
    }
}

pub(in super::super) fn pattern_kind_has_unresolved_operands(pattern: &PatternKind) -> bool {
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

pub(in super::super) fn termination_has_unresolved_operands(termination: &Termination) -> bool {
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

fn transfer_reference_lines(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) {
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
            annotations,
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
}

fn transfer_reference_circles(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) {
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
            annotations,
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
}

fn transfer_reference_ellipses(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) {
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
            annotations,
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
}

fn transfer_display_tessellations(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) {
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
            annotations,
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
}

fn transfer_datum_plane_surfaces(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) {
    for plane in &scan.planes.datums {
        let id = SurfaceId(format!("creo:actdatums:surface#{}", plane.id));
        annotate(
            annotations,
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
}

fn transfer_placed_plane_surfaces_into_ir(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) {
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
            annotations,
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
}

/// Build source metadata, preserved geometry records, and transferred entities.
pub(in super::super) fn build_ir(
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
    transfer_reference_lines(scan, &mut ir, &mut annotations);
    transfer_reference_circles(scan, &mut ir, &mut annotations);
    transfer_reference_ellipses(scan, &mut ir, &mut annotations);
    transfer_display_tessellations(scan, &mut ir, &mut annotations);
    transfer_datum_plane_surfaces(scan, &mut ir, &mut annotations);
    transfer_placed_plane_surfaces_into_ir(scan, &mut ir, &mut annotations);
    transfer_and_record_scanned_geometry(ctx, scan, &mut ir, &mut annotations, &mut coverage)?;
    let geometry_generator_feature_count = emit_model_features(scan, &mut ir, &mut annotations);
    let (feature_result_topology_count, feature_result_edge_count) =
        finish_feature_transfers(scan, &mut ir, &mut annotations, &mut coverage);
    attach_expanded_sections(scan, &mut ir, &mut annotations)?;
    emit_geometry_arenas(scan, &mut ir, &mut annotations)?;
    if let Some(length_scale_mm) = scan
        .framing
        .principal_unit
        .and_then(crate::legacy::PrincipalUnitSystem::length_scale_mm)
    {
        super::units::normalize_model_lengths(&mut ir, length_scale_mm);
    }
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
