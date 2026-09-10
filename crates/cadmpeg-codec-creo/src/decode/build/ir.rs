// SPDX-License-Identifier: Apache-2.0
//! Container IR bootstrap and model-entity assembly.

use crate::vecmath::normalize;
use std::collections::BTreeMap;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::VertexSelection;
use cadmpeg_ir::features::{
    AngularTermination, BodySelection, EdgeSelection, FaceSelection, LinearTermination, PathRef,
    PatternKind, PatternTransform, SurfaceBoundary,
};
use cadmpeg_ir::geometry::{Curve, CurveGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::{CurveId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::tessellation::Tessellation;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::AnnotationBuilder;
use cadmpeg_ir::{Exactness, SourceObjectAssociation};

use crate::container::ContainerScan;

use super::super::expanded::attach_expanded_sections;
use super::super::native::annotate;
use super::super::surfaces::BrepTransferDiagnostics;
use super::arenas::{emit_geometry_arenas, emit_reference_arenas};
use super::coverage::collect_feature_coverage;
use super::ir_features::{emit_model_features, finish_feature_transfers};
use super::ir_geometry::transfer_and_record_scanned_geometry;
use super::meta::source_meta;
use super::passthrough::{emit_legacy_arenas, preserve_passthrough_sections};
use crate::decode::analytic::planes::placed_plane_surfaces;

pub(in super::super) struct BuiltIr {
    pub(in super::super) ir: CadIr,
    pub(in super::super) annotations: cadmpeg_ir::Annotations,
    pub(in super::super) unknowns: Vec<UnknownRecord>,
    pub(in super::super) coverage: cadmpeg_ir::Coverage,
    pub(in super::super) brep_diagnostics: BrepTransferDiagnostics,
    pub(in super::super) transfer_losses: Vec<cadmpeg_ir::report::LossNote>,
}

pub(in super::super) fn build_container_ir(
    scan: &ContainerScan,
    classification: &crate::dialect::DialectClassification,
) -> Result<BuiltIr, CodecError> {
    let (meta, coverage) = source_meta(scan, classification);
    let mut ir = CadIr::decoded(meta);
    let mut annotations = AnnotationBuilder::new();
    emit_legacy_arenas(scan, &mut ir, &mut annotations)?;
    let unknowns = preserve_passthrough_sections(scan, &mut annotations);
    attach_expanded_sections(scan, &mut ir, &mut annotations)?;
    Ok(BuiltIr {
        ir,
        annotations: annotations.build(),
        unknowns,
        coverage,
        brep_diagnostics: BrepTransferDiagnostics::default(),
        transfer_losses: Vec::new(),
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
    match pattern.definition() {
        PatternTransform::Unresolved
        | PatternTransform::UnresolvedLinear
        | PatternTransform::UnresolvedCircular
        | PatternTransform::UnresolvedCurveDriven
        | PatternTransform::UnresolvedMirror
        | PatternTransform::UnresolvedScale
        | PatternTransform::UnresolvedComposite => true,
        PatternTransform::Linear { direction, .. }
        | PatternTransform::LinearOffsets { direction, .. } => direction.is_none(),
        PatternTransform::CurveDriven { path, .. } => {
            path.as_ref().is_none_or(path_has_unresolved_operands)
        }
        PatternTransform::Scale { center, .. } => {
            matches!(center, cadmpeg_ir::features::PatternScaleCenter::Native(_))
        }
        PatternTransform::Composite { stages } => stages
            .iter()
            .any(|stage| pattern_kind_has_unresolved_operands(&stage.pattern)),
        PatternTransform::Circular { .. }
        | PatternTransform::CircularAngles { .. }
        | PatternTransform::Mirror { .. } => false,
        PatternTransform::MirrorReference { .. } => true,
    }
}

pub(in super::super) fn linear_termination_has_unresolved_operands(
    termination: &LinearTermination,
) -> bool {
    match termination {
        LinearTermination::Unresolved {} => true,
        LinearTermination::ToFace { face, .. }
        | LinearTermination::OffsetFromFace { face, .. }
        | LinearTermination::ToShape { target: face } => {
            face_selection_has_unresolved_operands(face)
        }
        LinearTermination::ToVertex { vertex } => {
            matches!(
                vertex,
                VertexSelection::Unresolved | VertexSelection::Native(_)
            )
        }
        LinearTermination::Blind { .. }
        | LinearTermination::ThroughAll {}
        | LinearTermination::ThroughNext {}
        | LinearTermination::ToFirst {}
        | LinearTermination::ToLast {} => false,
    }
}

pub(in super::super) fn angular_termination_has_unresolved_operands(
    termination: &AngularTermination,
) -> bool {
    match termination {
        AngularTermination::Unresolved {} => true,
        AngularTermination::ToFace { face, .. }
        | AngularTermination::OffsetFromFace { face, .. }
        | AngularTermination::ToShape { target: face } => {
            face_selection_has_unresolved_operands(face)
        }
        AngularTermination::ToVertex { vertex } => {
            matches!(
                vertex,
                VertexSelection::Unresolved | VertexSelection::Native(_)
            )
        }
        AngularTermination::ThroughAll {}
        | AngularTermination::ThroughNext {}
        | AngularTermination::ToFirst {}
        | AngularTermination::ToLast {}
        | AngularTermination::Angle { .. } => false,
    }
}

fn transfer_reference_lines(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> Result<(), CodecError> {
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
        let Some(direction) = normalize(direction) else {
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
        let id = CurveId::mint(prefix).expect("identity grammar");
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
            geometry: CurveGeometry::Line(
                cadmpeg_ir::geometry::LineCurve::try_new(
                    Point3::new(line.start[0], line.start[1], line.start[2]),
                    Vector3::new(direction[0], direction[1], direction[2]),
                )
                .map_err(CodecError::malformed)?,
            ),
            source_object: Some(SourceObjectAssociation {
                format: cadmpeg_ir::CodecFormat::Creo,
                object_id: cadmpeg_ir::products::NonEmptyString::new(format!(
                    "MdlRefInfo:{family}:{native_identity}"
                ))
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("source object_id must not be empty")
                })?,
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
    }
    Ok(())
}

fn transfer_reference_circles(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> Result<(), CodecError> {
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
        let Some(reference) = normalize(radial) else {
            continue;
        };
        let native_identity = if circle_id_counts.get(&circle.entity_id) == Some(&1) {
            circle.entity_id.to_string()
        } else {
            format!("{}@{}", circle.entity_id, circle.offset)
        };
        let id = CurveId::mint(format!("creo:mdl_ref_info:arc_z#{native_identity}"))
            .expect("identity grammar");
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
            geometry: CurveGeometry::Circle(
                cadmpeg_ir::geometry::CircleCurve::try_new(
                    Point3::new(circle.center[0], circle.center[1], circle.center[2]),
                    Vector3::new(circle.axis[0], circle.axis[1], circle.axis[2]),
                    Vector3::new(reference[0], reference[1], reference[2]),
                    circle.radius,
                )
                .map_err(CodecError::malformed)?,
            ),
            source_object: Some(SourceObjectAssociation {
                format: cadmpeg_ir::CodecFormat::Creo,
                object_id: cadmpeg_ir::products::NonEmptyString::new(format!(
                    "MdlRefInfo:arc_z:{native_identity}"
                ))
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("source object_id must not be empty")
                })?,
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
    }
    Ok(())
}

fn transfer_reference_ellipses(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> Result<(), CodecError> {
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
        let id = CurveId::mint(format!("creo:mdl_ref_info:conic#{native_identity}"))
            .expect("identity grammar");
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
            geometry: CurveGeometry::Ellipse(
                cadmpeg_ir::geometry::EllipseCurve::try_new(
                    Point3::new(ellipse.center[0], ellipse.center[1], ellipse.center[2]),
                    Vector3::new(ellipse.axis[0], ellipse.axis[1], ellipse.axis[2]),
                    Vector3::new(
                        ellipse.major_direction[0],
                        ellipse.major_direction[1],
                        ellipse.major_direction[2],
                    ),
                    ellipse.major_radius,
                    ellipse.minor_radius,
                )
                .map_err(CodecError::malformed)?,
            ),
            source_object: Some(SourceObjectAssociation {
                format: cadmpeg_ir::CodecFormat::Creo,
                object_id: cadmpeg_ir::products::NonEmptyString::new(format!(
                    "MdlRefInfo:conic:{native_identity}"
                ))
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("source object_id must not be empty")
                })?,
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
    }
    Ok(())
}

fn transfer_display_tessellations(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> Result<(), CodecError> {
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
        ir.model.tessellations.push(
            Tessellation::from_decoded(
                id,
                strip
                    .positions
                    .iter()
                    .map(|point| Point3::new(point[0], point[1], point[2]))
                    .collect(),
                triangles,
                strip.strip_lengths.clone(),
                cadmpeg_ir::tessellation::TessellationNormals::per_vertex(
                    strip
                        .normals
                        .iter()
                        .map(|normal| Vector3::new(normal[0], normal[1], normal[2]))
                        .collect(),
                ),
                Vec::new(),
            )
            .map_err(|error| {
                CodecError::malformed(format_args!("invalid display tessellation: {error}"))
            })?,
        );
    }
    Ok(())
}

fn transfer_datum_plane_surfaces(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> Result<(), CodecError> {
    for plane in &scan.planes.datums {
        let normal = plane.plane.normal();
        let id = SurfaceId::mint(format!("creo:actdatums:surface#{}", plane.id))
            .expect("identity grammar");
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
            geometry: SurfaceGeometry::Plane(
                cadmpeg_ir::geometry::PlaneSurface::try_new(
                    Point3::new(
                        normal[0] * plane.plane.offset,
                        normal[1] * plane.plane.offset,
                        normal[2] * plane.plane.offset,
                    ),
                    Vector3::new(normal[0], normal[1], normal[2]),
                    cadmpeg_ir::geometry::derive_reference_direction(Vector3::new(
                        normal[0], normal[1], normal[2],
                    )),
                )
                .map_err(CodecError::malformed)?,
            ),
            source_object: Some(SourceObjectAssociation {
                format: cadmpeg_ir::CodecFormat::Creo,
                object_id: cadmpeg_ir::products::NonEmptyString::new(format!(
                    "ActDatums:{}",
                    plane.id
                ))
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("source object_id must not be empty")
                })?,
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
    }
    Ok(())
}

fn transfer_placed_plane_surfaces_into_ir(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> Result<(), CodecError> {
    for (surface_id, (plane, u_axis, offset)) in placed_plane_surfaces(scan) {
        let id = SurfaceId::mint(format!("creo:visibgeom:surface#{surface_id}"))
            .expect("identity grammar");
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
            geometry: SurfaceGeometry::Plane(
                cadmpeg_ir::geometry::PlaneSurface::try_new(
                    Point3::new(plane.origin[0], plane.origin[1], plane.origin[2]),
                    Vector3::new(plane.normal[0], plane.normal[1], plane.normal[2]),
                    Vector3::new(u_axis[0], u_axis[1], u_axis[2]),
                )
                .map_err(CodecError::malformed)?,
            ),
            source_object: Some(SourceObjectAssociation {
                format: cadmpeg_ir::CodecFormat::Creo,
                object_id: cadmpeg_ir::products::NonEmptyString::new(format!(
                    "VisibGeom:{surface_id}"
                ))
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("source object_id must not be empty")
                })?,
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
    }
    Ok(())
}

/// Build source metadata, preserved geometry records, and transferred entities.
pub(in super::super) fn build_ir(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    classification: &crate::dialect::DialectClassification,
) -> Result<BuiltIr, CodecError> {
    let (meta, mut coverage) = source_meta(scan, classification);
    let mut ir = CadIr::decoded(meta);
    let mut annotations = AnnotationBuilder::new();
    let mut brep_diagnostics = BrepTransferDiagnostics::default();
    let mut transfer_losses = Vec::new();
    emit_legacy_arenas(scan, &mut ir, &mut annotations)?;
    let unknowns = preserve_passthrough_sections(scan, &mut annotations);
    emit_reference_arenas(scan, &mut ir, &mut annotations)?;
    transfer_reference_lines(scan, &mut ir, &mut annotations)?;
    transfer_reference_circles(scan, &mut ir, &mut annotations)?;
    transfer_reference_ellipses(scan, &mut ir, &mut annotations)?;
    transfer_display_tessellations(scan, &mut ir, &mut annotations)?;
    transfer_datum_plane_surfaces(scan, &mut ir, &mut annotations)?;
    transfer_placed_plane_surfaces_into_ir(scan, &mut ir, &mut annotations)?;
    transfer_and_record_scanned_geometry(
        ctx,
        scan,
        &mut ir,
        &mut annotations,
        &mut coverage,
        &mut brep_diagnostics,
        &mut transfer_losses,
    )?;
    let geometry_generator_feature_count = emit_model_features(scan, &mut ir, &mut annotations)?;
    let (feature_result_topology_count, feature_result_edge_count) =
        finish_feature_transfers(scan, &mut ir, &mut annotations, &mut coverage)?;
    attach_expanded_sections(scan, &mut ir, &mut annotations)?;
    emit_geometry_arenas(scan, &mut ir, &mut annotations, &brep_diagnostics)?;
    if let Some(length_scale_mm) = scan
        .framing
        .principal_unit
        .and_then(crate::legacy::PrincipalUnitSystem::length_scale_mm)
    {
        super::units::normalize_model_lengths(&mut ir, length_scale_mm)?;
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
        brep_diagnostics,
        transfer_losses,
    })
}
