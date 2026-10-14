// SPDX-License-Identifier: Apache-2.0
//! Container IR bootstrap and model-entity assembly.

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
use cadmpeg_ir::features::{FinitePoint3, FiniteVector3};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::tessellation::{
    ShadedVertex, Strip, Strips, Tessellation, TessellationId, TessellationLaneError,
    TessellationMesh,
};
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::AnnotationBuilder;
use cadmpeg_ir::{Exactness, SourceObjectAssociation};

use crate::container::ContainerScan;
use crate::identity::source_object_id_checked;

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
use crate::decode::source_carriers::SourceUnitCarriers;

pub(in super::super) struct BuiltIr {
    pub(in super::super) ir: CadIr,
    pub(in super::super) annotations: cadmpeg_ir::Annotations,
    pub(in super::super) unknowns: Vec<UnknownRecord>,
    pub(in super::super) coverage: cadmpeg_ir::report::decode::Coverage,
    pub(in super::super) brep_diagnostics: BrepTransferDiagnostics,
    pub(in super::super) transfer_losses: Vec<cadmpeg_ir::report::loss::LossNote>,
}

pub(in super::super) fn build_container_ir(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    classification: &crate::dialect::DialectClassification,
) -> Result<BuiltIr, CodecError> {
    let (meta, coverage) = source_meta(ctx, scan, classification)?;
    let mut ir = CadIr::decoded(meta);
    let mut annotations = AnnotationBuilder::new();
    emit_legacy_arenas(ctx, scan, &mut ir, &mut annotations)?;
    let unknowns = preserve_passthrough_sections(ctx, scan, &mut annotations)?;
    attach_expanded_sections(ctx, scan, &mut ir, &mut annotations)?;
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    source_carriers: &mut SourceUnitCarriers,
) -> Result<(), CodecError> {
    let mut line3d_id_counts = BTreeMap::<u32, usize>::new();
    for line in &scan.references.lines {
        if let crate::reference::ReferenceLineKind::Line3d { entity_id, .. } = line.kind() {
            *line3d_id_counts.entry(*entity_id).or_default() += 1;
        }
    }
    for line in &scan.references.lines {
        let start: [f64; 3] = line.start().get().into();
        let end: [f64; 3] = line.end().get().into();
        let direction = std::array::from_fn(|axis| end[axis] - start[axis]);
        let Some((direction, _)) = crate::vecmath::normalize_with_length(direction) else {
            continue;
        };
        let (id, object_id) = match line.kind() {
            crate::reference::ReferenceLineKind::Line => (
                crate::identity::compose_checked::<CurveId>(
                    &crate::identity::MDL_REF_INFO_LINE,
                    line.offset,
                )?,
                source_object_id_checked(format_args!("MdlRefInfo:line:{}", line.offset))?,
            ),
            crate::reference::ReferenceLineKind::Line3d { entity_id, .. } => {
                if line3d_id_counts.get(entity_id) == Some(&1) {
                    (
                        crate::identity::compose_checked::<CurveId>(
                            &crate::identity::MDL_REF_INFO_LINE3D,
                            entity_id,
                        )?,
                        source_object_id_checked(format_args!("MdlRefInfo:line3d:{entity_id}"))?,
                    )
                } else {
                    (
                        crate::identity::compose_checked::<CurveId>(
                            &crate::identity::MDL_REF_INFO_LINE3D,
                            format_args!("{entity_id}@{}", line.offset),
                        )?,
                        source_object_id_checked(format_args!(
                            "MdlRefInfo:line3d:{entity_id}@{}",
                            line.offset
                        ))?,
                    )
                }
            }
        };
        annotate(
            ctx,
            annotations,
            &id,
            "MdlRefInfo",
            cadmpeg_core::decode::u64_from_index(line.offset),
            "reference_line",
            Exactness::Derived,
        )?;
        ctx.charge_entities(1, "admit Creo model curves")?;
        source_carriers.admit_curve(
            ctx,
            ir,
            Curve {
                parameter_range: None,
                id,
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                    cadmpeg_ir::geometry::analytic::LineCurve::new(line.start(), direction),
                )),
                source_object: Some(SourceObjectAssociation {
                    geometry_role: None,
                    format: cadmpeg_ir::CodecFormat::Creo,
                    object_id,
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            },
        )?;
    }
    Ok(())
}

fn transfer_reference_circles(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    source_carriers: &mut SourceUnitCarriers,
) -> Result<(), CodecError> {
    let mut circle_id_counts = BTreeMap::<u32, usize>::new();
    for circle in &scan.references.circles {
        *circle_id_counts.entry(circle.entity_id).or_default() += 1;
    }
    for circle in &scan.references.circles {
        let start: [f64; 3] = circle.start().get().into();
        let center: [f64; 3] = circle.center().get().into();
        let radial = std::array::from_fn(|axis| start[axis] - center[axis]);
        let Some((reference, _)) = crate::vecmath::normalize_with_length(radial) else {
            continue;
        };
        let (id, object_id) = if circle_id_counts.get(&circle.entity_id) == Some(&1) {
            (
                crate::identity::compose_checked::<CurveId>(
                    &crate::identity::MDL_REF_INFO_ARC_Z,
                    circle.entity_id,
                )?,
                source_object_id_checked(format_args!("MdlRefInfo:arc_z:{}", circle.entity_id))?,
            )
        } else {
            (
                crate::identity::compose_checked::<CurveId>(
                    &crate::identity::MDL_REF_INFO_ARC_Z,
                    format_args!("{}@{}", circle.entity_id, circle.offset),
                )?,
                source_object_id_checked(format_args!(
                    "MdlRefInfo:arc_z:{}@{}",
                    circle.entity_id, circle.offset
                ))?,
            )
        };
        annotate(
            ctx,
            annotations,
            &id,
            "MdlRefInfo",
            cadmpeg_core::decode::u64_from_index(circle.offset),
            "reference_circle",
            Exactness::Derived,
        )?;
        ctx.charge_entities(1, "admit Creo model curves")?;
        let frame = cadmpeg_ir::units::OrthonormalFrame3::from_units(circle.axis(), reference)
            .ok_or_else(|| {
                CodecError::malformed(
                    "CircleCurve.axis/ref_direction must form an orthonormal frame",
                )
            })?;
        source_carriers.admit_curve(
            ctx,
            ir,
            Curve {
                parameter_range: None,
                id,
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                    cadmpeg_ir::geometry::analytic::CircleCurve::new(
                        circle.center(),
                        frame,
                        circle.radius(),
                    ),
                )),
                source_object: Some(SourceObjectAssociation {
                    geometry_role: None,
                    format: cadmpeg_ir::CodecFormat::Creo,
                    object_id,
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            },
        )?;
    }
    Ok(())
}

fn transfer_reference_ellipses(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    source_carriers: &mut SourceUnitCarriers,
) -> Result<(), CodecError> {
    let mut ellipse_id_counts = BTreeMap::<u32, usize>::new();
    for ellipse in &scan.references.ellipses {
        *ellipse_id_counts
            .entry(ellipse.source_entity_id)
            .or_default() += 1;
    }
    for ellipse in &scan.references.ellipses {
        let (id, object_id) = if ellipse_id_counts.get(&ellipse.source_entity_id) == Some(&1) {
            (
                crate::identity::compose_checked::<CurveId>(
                    &crate::identity::MDL_REF_INFO_CONIC,
                    ellipse.source_entity_id,
                )?,
                source_object_id_checked(format_args!(
                    "MdlRefInfo:conic:{}",
                    ellipse.source_entity_id
                ))?,
            )
        } else {
            (
                crate::identity::compose_checked::<CurveId>(
                    &crate::identity::MDL_REF_INFO_CONIC,
                    format_args!("{}@{}", ellipse.source_entity_id, ellipse.offset),
                )?,
                source_object_id_checked(format_args!(
                    "MdlRefInfo:conic:{}@{}",
                    ellipse.source_entity_id, ellipse.offset
                ))?,
            )
        };
        annotate(
            ctx,
            annotations,
            &id,
            "MdlRefInfo",
            cadmpeg_core::decode::u64_from_index(ellipse.offset),
            "reference_ellipse",
            Exactness::Derived,
        )?;
        ctx.charge_entities(1, "admit Creo model curves")?;
        source_carriers.admit_curve(
            ctx,
            ir,
            Curve {
                parameter_range: None,
                id,
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(
                    cadmpeg_ir::geometry::analytic::EllipseCurve::try_from_parts(
                        ellipse.center(),
                        cadmpeg_ir::units::OrthonormalFrame3::from_units(
                            ellipse.axis(),
                            ellipse.major_direction(),
                        )
                        .ok_or_else(|| {
                            CodecError::malformed(
                                "EllipseCurve.axis/ref_direction must form an orthonormal frame",
                            )
                        })?,
                        ellipse.major_radius(),
                        ellipse.minor_radius(),
                    )
                    .map_err(CodecError::malformed)?,
                )),
                source_object: Some(SourceObjectAssociation {
                    geometry_role: None,
                    format: cadmpeg_ir::CodecFormat::Creo,
                    object_id,
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            },
        )?;
    }
    Ok(())
}

fn display_strip_error(offset: usize, detail: impl std::fmt::Display) -> CodecError {
    CodecError::Malformed(format!(
        "SolidPrimdata display triangle strip at byte {offset}: {detail}"
    ))
}

fn admitted_display_strips<V>(vertices: Vec<V>, spans: &[u32]) -> Option<Strips<V>> {
    let mut remaining = vertices.into_iter();
    let mut strips = Vec::new();
    for span in spans {
        let Ok(count) = usize::try_from(*span) else {
            return None;
        };
        let mut run = Vec::new();
        for _ in 0..count {
            let vertex = remaining.next()?;
            run.push(vertex);
        }
        let strip = Strip::new(run)?;
        strips.push(strip);
    }
    if remaining.next().is_some() {
        return None;
    }
    Strips::new(strips)
}

fn transfer_display_tessellations(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> Result<(), CodecError> {
    let length_scale = scan
        .framing
        .principal_unit
        .and_then(crate::legacy::PrincipalUnitSystem::length_scale_mm);
    for strip in &scan.primitives.triangle_strips {
        let id: TessellationId = crate::identity::compose_checked(
            &cadmpeg_ir::identity_namespace!("creo", "solid_primdata", "tessellation"),
            strip.offset,
        )?;
        annotate(
            ctx,
            annotations,
            id.as_str(),
            "SolidPrimdata",
            cadmpeg_core::decode::u64_from_index(strip.offset),
            "display_triangle_strip",
            Exactness::Derived,
        )?;
        ctx.charge_entities(1, "admit Creo model tessellations")?;
        let mut positions = Vec::new();
        for position in strip.positions() {
            let mut point = Point3::from(position.get());
            if let Some(scale) = length_scale {
                point = Point3::new(
                    point.x * scale.get(),
                    point.y * scale.get(),
                    point.z * scale.get(),
                );
                if !point.is_finite() {
                    return Err(CodecError::NotImplemented(format!("SolidPrimdata display triangle strip at byte {} has a vertex that cannot be represented in millimeters", strip.offset)));
                }
            }
            let Some(point) = FinitePoint3::new(point) else {
                return Err(display_strip_error(
                    strip.offset,
                    "vertices contain a non-finite coordinate",
                ));
            };
            positions.push(point);
        }
        let mesh = if let Some(normals) = strip.normals() {
            let mut rows = Vec::new();
            for (position, normal) in positions.into_iter().zip(normals) {
                let Some(normal) = FiniteVector3::new(Vector3::from(normal.get())) else {
                    return Err(display_strip_error(
                        strip.offset,
                        "normals contain a non-finite coordinate",
                    ));
                };
                rows.push(ShadedVertex { position, normal });
            }
            let Some(strips) = admitted_display_strips(rows, strip.strip_lengths()) else {
                return Err(display_strip_error(
                    strip.offset,
                    TessellationLaneError::Strips {
                        spans: strip.strip_lengths().len(),
                    },
                ));
            };
            TessellationMesh::ShadedStrips { strips }
        } else {
            // An absent normal lane is an unshaded strip set.
            let Some(strips) = admitted_display_strips(positions, strip.strip_lengths()) else {
                return Err(display_strip_error(
                    strip.offset,
                    TessellationLaneError::Strips {
                        spans: strip.strip_lengths().len(),
                    },
                ));
            };
            TessellationMesh::Strips { strips }
        };
        let tessellation = match Tessellation::from_parts(id, mesh, Vec::new()) {
            Ok(tessellation) => tessellation,
            Err(error) => return Err(display_strip_error(strip.offset, error)),
        };
        ir.model.tessellations.push(tessellation);
    }
    Ok(())
}

fn transfer_datum_plane_surfaces(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    source_carriers: &mut SourceUnitCarriers,
) -> Result<(), CodecError> {
    for plane in &scan.planes.datums {
        let normal = plane.plane().normal();
        let id = crate::identity::compose_checked::<SurfaceId>(
            &crate::identity::ACTDATUM_SURFACE,
            plane.id,
        )?;
        annotate(
            ctx,
            annotations,
            &id,
            "ActDatums",
            cadmpeg_core::decode::u64_from_index(plane.offset_in_payload),
            "datum_plane_outline",
            Exactness::Derived,
        )?;
        ctx.charge_entities(1, "admit Creo model surfaces")?;
        source_carriers.admit_surface(
            ctx,
            ir,
            Surface {
                id,
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                    cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                        Point3::new(
                            normal[0] * plane.plane().offset(),
                            normal[1] * plane.plane().offset(),
                            normal[2] * plane.plane().offset(),
                        ),
                        Vector3::from(normal),
                        cadmpeg_ir::geometry::derive_reference_direction(Vector3::from(normal)),
                    )
                    .map_err(CodecError::malformed)?,
                )),
                source_object: Some(SourceObjectAssociation {
                    geometry_role: None,
                    format: cadmpeg_ir::CodecFormat::Creo,
                    object_id: source_object_id_checked(format_args!("ActDatums:{}", plane.id))?,
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            },
        )?;
    }
    Ok(())
}

fn transfer_placed_plane_surfaces_into_ir(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    source_carriers: &mut SourceUnitCarriers,
) -> Result<(), CodecError> {
    for frame in &scan.planes.local_systems {
        if frame.frame().cross_overflow {
            return Err(CodecError::NotImplemented(format!("Creo plane local system at byte {} has a cross product outside the representable range", frame.offset)));
        }
    }
    for (
        surface_id,
        crate::decode::analytic::planes::PlacedPlaneSurface {
            plane,
            u_axis,
            offset,
        },
    ) in placed_plane_surfaces(scan)?
    {
        let id = crate::identity::compose_checked::<SurfaceId>(
            &crate::identity::VISIBGEOM_SURFACE,
            surface_id,
        )?;
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
            ctx,
            annotations,
            &id,
            "VisibGeom",
            cadmpeg_core::decode::u64_from_index(offset),
            tag,
            Exactness::Derived,
        )?;
        ctx.charge_entities(1, "admit Creo model surfaces")?;
        source_carriers.admit_surface(
            ctx,
            ir,
            Surface {
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
                    geometry_role: None,
                    format: cadmpeg_ir::CodecFormat::Creo,
                    object_id: source_object_id_checked(format_args!("VisibGeom:{surface_id}"))?,
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            },
        )?;
    }
    Ok(())
}

/// Build source metadata, preserved geometry records, and transferred entities.
pub(in super::super) fn build_ir(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    classification: &crate::dialect::DialectClassification,
) -> Result<BuiltIr, CodecError> {
    let (meta, mut coverage) = source_meta(ctx, scan, classification)?;
    let mut ir = CadIr::decoded(meta);
    let mut annotations = AnnotationBuilder::new();
    let mut transfer_losses = Vec::new();
    let length_scale_mm = scan
        .framing
        .principal_unit
        .and_then(crate::legacy::PrincipalUnitSystem::length_scale_mm);
    let mut source_carriers = SourceUnitCarriers::new(length_scale_mm);
    emit_legacy_arenas(ctx, scan, &mut ir, &mut annotations)?;
    let unknowns = preserve_passthrough_sections(ctx, scan, &mut annotations)?;
    emit_reference_arenas(ctx, scan, &mut ir, &mut annotations)?;
    transfer_reference_lines(ctx, scan, &mut ir, &mut annotations, &mut source_carriers)?;
    transfer_reference_circles(ctx, scan, &mut ir, &mut annotations, &mut source_carriers)?;
    transfer_reference_ellipses(ctx, scan, &mut ir, &mut annotations, &mut source_carriers)?;
    transfer_display_tessellations(ctx, scan, &mut ir, &mut annotations)?;
    transfer_datum_plane_surfaces(ctx, scan, &mut ir, &mut annotations, &mut source_carriers)?;
    transfer_placed_plane_surfaces_into_ir(
        ctx,
        scan,
        &mut ir,
        &mut annotations,
        &mut source_carriers,
    )?;
    let brep_diagnostics = transfer_and_record_scanned_geometry(
        ctx,
        scan,
        &mut ir,
        &mut annotations,
        &mut coverage,
        &mut transfer_losses,
        &mut source_carriers,
    )?;
    let geometry_generator_feature_count =
        emit_model_features(ctx, scan, &mut ir, &mut annotations, &source_carriers)?;
    let (feature_result_topology_count, feature_result_edge_count) = finish_feature_transfers(
        ctx,
        scan,
        &mut ir,
        &mut annotations,
        &mut coverage,
        &mut source_carriers,
    )?;
    attach_expanded_sections(ctx, scan, &mut ir, &mut annotations)?;
    emit_geometry_arenas(ctx, scan, &mut ir, &mut annotations, &brep_diagnostics)?;
    collect_feature_coverage(
        ctx,
        scan,
        &ir,
        geometry_generator_feature_count,
        feature_result_topology_count,
        feature_result_edge_count,
        &mut coverage,
    )?;
    Ok(BuiltIr {
        ir,
        annotations: annotations.build(),
        unknowns,
        coverage,
        brep_diagnostics,
        transfer_losses,
    })
}

#[cfg(test)]
mod tests;
