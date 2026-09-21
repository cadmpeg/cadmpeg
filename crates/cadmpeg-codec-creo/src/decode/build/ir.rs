// SPDX-License-Identifier: Apache-2.0
//! Container IR bootstrap and model-entity assembly.

use crate::vecmath::normalize;
use std::collections::BTreeMap;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::VertexSelection;
use cadmpeg_ir::features::{
    patterns::{PatternKind, PatternTransform},
    AngularTermination, BodySelection, EdgeSelection, FaceSelection, LinearTermination, PathRef,
    SurfaceBoundary,
};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::tessellation::Tessellation;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::AnnotationBuilder;
use cadmpeg_ir::{Exactness, SourceObjectAssociation};

use crate::container::ContainerScan;

use super::super::expanded::attach_expanded_sections;
use super::super::native::annotate;
use super::super::surfaces::brep::BrepTransferDiagnostics;
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
    pub(in super::super) coverage: cadmpeg_ir::report::decode::Coverage,
    pub(in super::super) brep_diagnostics: BrepTransferDiagnostics,
    pub(in super::super) transfer_losses: Vec<cadmpeg_ir::report::loss::LossNote>,
}

pub(in super::super) fn build_container_ir(
    scan: &ContainerScan,
    classification: &crate::dialect::DialectClassification,
) -> Result<BuiltIr, CodecError> {
    let (meta, coverage) = source_meta(scan, classification)?;
    let mut ir = CadIr::decoded(meta);
    let mut annotations = AnnotationBuilder::new();
    emit_legacy_arenas(scan, &mut ir, &mut annotations)?;
    let unknowns = preserve_passthrough_sections(scan, &mut annotations)?;
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

fn edge_selection_has_unresolved_operands(selection: &EdgeSelection) -> bool {
    matches!(
        selection,
        EdgeSelection::Unresolved
            | EdgeSelection::HistoricalPartial { .. }
            | EdgeSelection::Native(_)
    )
}

fn path_has_unresolved_operands(path: &PathRef) -> bool {
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

pub(super) fn pattern_kind_has_unresolved_operands<
    C: cadmpeg_ir::features::patterns::CompositeStages,
>(
    pattern: &PatternKind<C>,
) -> bool {
    match pattern.definition() {
        PatternTransform::Unresolved { .. } => true,
        PatternTransform::Linear { direction, .. }
        | PatternTransform::LinearOffsets { direction, .. } => direction.is_none(),
        PatternTransform::CurveDriven { path, .. } => {
            path.as_ref().is_none_or(path_has_unresolved_operands)
        }
        PatternTransform::Scale { center, .. } => {
            matches!(
                center,
                cadmpeg_ir::features::patterns::PatternScaleCenter::Native(_)
            )
        }
        PatternTransform::Composite { stages } => stages
            .stages()
            .iter()
            .any(|stage| pattern_kind_has_unresolved_operands(&stage.pattern)),
        PatternTransform::Circular { .. }
        | PatternTransform::CircularAngles { .. }
        | PatternTransform::Mirror { .. } => false,
        PatternTransform::MirrorReference { .. } => true,
    }
}

pub(super) fn linear_termination_has_unresolved_operands(termination: &LinearTermination) -> bool {
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

pub(super) fn angular_termination_has_unresolved_operands(
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
        let (family, native_identity, id) = match &line.kind {
            crate::reference::ReferenceLineKind::Line => (
                "line",
                line.offset.to_string(),
                CurveId::compose(&crate::identity::MDL_REF_INFO_LINE, line.offset),
            ),
            crate::reference::ReferenceLineKind::Line3d { entity_id, .. } => {
                let (identity, key) = if line3d_id_counts.get(entity_id) == Some(&1) {
                    (
                        entity_id.to_string(),
                        cadmpeg_ir::ids::IdentityKey::from(*entity_id),
                    )
                } else {
                    let identity = format!("{entity_id}@{}", line.offset);
                    let key = cadmpeg_ir::ids::IdentityKey::try_new(identity.clone()).map_err(
                        |error| {
                            CodecError::malformed(format!(
                                "MdlRefInfo line3d identity for source line: {error}"
                            ))
                        },
                    )?;
                    (identity, key)
                };
                (
                    "line3d",
                    identity,
                    CurveId::compose(&crate::identity::MDL_REF_INFO_LINE3D, key),
                )
            }
        };
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
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                    Point3::from(line.start),
                    Vector3::from(direction),
                )
                .map_err(CodecError::malformed)?,
            )),
            source_object: Some(SourceObjectAssociation {
                format: cadmpeg_ir::CodecFormat::Creo,
                object_id: cadmpeg_core::text::NonBlankString::new(format!(
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
        let native_key = if circle_id_counts.get(&circle.entity_id) == Some(&1) {
            cadmpeg_ir::ids::IdentityKey::from(circle.entity_id)
        } else {
            cadmpeg_ir::ids::IdentityKey::try_new(format!("{}@{}", circle.entity_id, circle.offset))
                .map_err(|error| {
                    CodecError::malformed(format!(
                        "MdlRefInfo arc_z identity for source circle: {error}"
                    ))
                })?
        };
        let id = CurveId::compose(&crate::identity::MDL_REF_INFO_ARC_Z, native_key);
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
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                    Point3::from(circle.center),
                    Vector3::from(circle.axis),
                    Vector3::from(reference),
                    circle.radius,
                )
                .map_err(CodecError::malformed)?,
            )),
            source_object: Some(SourceObjectAssociation {
                format: cadmpeg_ir::CodecFormat::Creo,
                object_id: cadmpeg_core::text::NonBlankString::new(format!(
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
        let native_key = if ellipse_id_counts.get(&ellipse.source_entity_id) == Some(&1) {
            cadmpeg_ir::ids::IdentityKey::from(ellipse.source_entity_id)
        } else {
            cadmpeg_ir::ids::IdentityKey::try_new(format!(
                "{}@{}",
                ellipse.source_entity_id, ellipse.offset
            ))
            .map_err(|error| {
                CodecError::malformed(format!(
                    "MdlRefInfo conic identity for source ellipse: {error}"
                ))
            })?
        };
        let id = CurveId::compose(&crate::identity::MDL_REF_INFO_CONIC, native_key);
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
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(
                cadmpeg_ir::geometry::analytic::EllipseCurve::try_new(
                    Point3::from(ellipse.center),
                    Vector3::from(ellipse.axis),
                    Vector3::from(ellipse.major_direction),
                    ellipse.major_radius,
                    ellipse.minor_radius,
                )
                .map_err(CodecError::malformed)?,
            )),
            source_object: Some(SourceObjectAssociation {
                format: cadmpeg_ir::CodecFormat::Creo,
                object_id: cadmpeg_core::text::NonBlankString::new(format!(
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
        annotate(
            annotations,
            &id,
            "SolidPrimdata",
            strip.offset as u64,
            "display_triangle_strip",
            Exactness::Derived,
        );
        ir.model.tessellations.push(
            Tessellation::new(
                id,
                cadmpeg_ir::tessellation::TessellationMesh::from_strip_lanes(
                    strip.positions.iter().copied().map(Point3::from).collect(),
                    // A primitive that carries only `mv_p_xyz` states an
                    // unshaded strip set: the normal lane is absent, never
                    // empty.
                    strip
                        .normals
                        .as_ref()
                        .map(|normals| normals.iter().copied().map(Vector3::from).collect()),
                    &strip.strip_lengths,
                )
                .map_err(|error| {
                    CodecError::malformed(format_args!(
                        "SolidPrimdata display triangle strip at byte {}: {error}",
                        strip.offset
                    ))
                })?,
                Vec::new(),
            )
            .map_err(|error| {
                CodecError::malformed(format_args!(
                    "SolidPrimdata display triangle strip at byte {}: {error}",
                    strip.offset
                ))
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
        let id = SurfaceId::compose(&crate::identity::ACTDATUM_SURFACE, plane.id);
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
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    Point3::new(
                        normal[0] * plane.plane.offset,
                        normal[1] * plane.plane.offset,
                        normal[2] * plane.plane.offset,
                    ),
                    Vector3::from(normal),
                    cadmpeg_ir::geometry::derive_reference_direction(Vector3::from(normal)),
                )
                .map_err(CodecError::malformed)?,
            )),
            source_object: Some(SourceObjectAssociation {
                format: cadmpeg_ir::CodecFormat::Creo,
                object_id: cadmpeg_core::text::NonBlankString::new(format!(
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
        let id = SurfaceId::compose(&crate::identity::VISIBGEOM_SURFACE, surface_id);
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
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    Point3::from(plane.origin),
                    Vector3::from(plane.normal),
                    Vector3::from(u_axis),
                )
                .map_err(CodecError::malformed)?,
            )),
            source_object: Some(SourceObjectAssociation {
                format: cadmpeg_ir::CodecFormat::Creo,
                object_id: cadmpeg_core::text::NonBlankString::new(format!(
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
    let (meta, mut coverage) = source_meta(scan, classification)?;
    let mut ir = CadIr::decoded(meta);
    let mut annotations = AnnotationBuilder::new();
    let mut brep_diagnostics = BrepTransferDiagnostics::default();
    let mut transfer_losses = Vec::new();
    emit_legacy_arenas(scan, &mut ir, &mut annotations)?;
    let unknowns = preserve_passthrough_sections(scan, &mut annotations)?;
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
