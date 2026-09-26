// SPDX-License-Identifier: Apache-2.0
//! Container IR bootstrap and model-entity assembly.

use std::collections::{BTreeMap, BTreeSet};

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
use cadmpeg_ir::scalar::PositiveLength;
use cadmpeg_ir::scalar::PositiveReal;
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
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    classification: &crate::dialect::DialectClassification,
) -> Result<BuiltIr, CodecError> {
    let (meta, coverage) = source_meta(scan, classification)?;
    let mut ir = CadIr::decoded(meta);
    let mut annotations = AnnotationBuilder::new();
    emit_legacy_arenas(scan, &mut ir, &mut annotations)?;
    let unknowns = preserve_passthrough_sections(ctx, scan, &mut annotations)?;
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> Result<BTreeSet<usize>, CodecError> {
    let length_scale = scan
        .framing
        .principal_unit
        .and_then(crate::legacy::PrincipalUnitSystem::length_scale_mm);
    let mut transferred_indices = BTreeSet::new();
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
        let start: [f64; 3] = line.start.get().into();
        let end: [f64; 3] = line.end.get().into();
        let direction = std::array::from_fn(|axis| end[axis] - start[axis]);
        let Some((direction, _)) = crate::vecmath::normalize_with_length(direction) else {
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
        ctx.charge_entities(1, "admit Creo model curves")?;
        let origin = if let Some(scale) = length_scale {
            line.start.scaled(scale).ok_or_else(|| {
                CodecError::NotImplemented(
                    "Creo reference line origin cannot be represented in millimeters".into(),
                )
            })?
        } else {
            line.start
        };
        let index = ir.model.curves.len();
        ir.model.curves.push(Curve {
            id,
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                cadmpeg_ir::geometry::analytic::LineCurve::new(origin, direction),
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
        transferred_indices.insert(index);
    }
    Ok(transferred_indices)
}

fn transfer_reference_circles(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> Result<BTreeSet<usize>, CodecError> {
    let length_scale = scan
        .framing
        .principal_unit
        .and_then(crate::legacy::PrincipalUnitSystem::length_scale_mm);
    let scale_mm = length_scale.map_or(1.0, cadmpeg_ir::scalar::PositiveReal::get);
    let mut transferred_indices = BTreeSet::new();
    let circle_id_counts =
        scan.references
            .circles
            .iter()
            .fold(BTreeMap::<u32, usize>::new(), |mut counts, circle| {
                *counts.entry(circle.entity_id).or_default() += 1;
                counts
            });
    for circle in &scan.references.circles {
        let start: [f64; 3] = circle.start.get().into();
        let radial = std::array::from_fn(|axis| start[axis] - circle.center[axis]);
        let Some((reference, _)) = crate::vecmath::normalize_with_length(radial) else {
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
        ctx.charge_entities(1, "admit Creo model curves")?;
        let frame = cadmpeg_ir::units::OrthonormalFrame3::from_units(circle.axis, reference)
            .ok_or_else(|| {
                CodecError::malformed(
                    "CircleCurve.axis/ref_direction must form an orthonormal frame",
                )
            })?;
        let center = cadmpeg_ir::features::FinitePoint3::new(Point3::from(circle.center))
            .ok_or_else(|| CodecError::malformed("CircleCurve.center must be finite"))?;
        let center = if let Some(scale) = length_scale {
            center.scaled(scale).ok_or_else(|| {
                CodecError::NotImplemented(
                    "Creo reference circle center cannot be represented in millimeters".into(),
                )
            })?
        } else {
            center
        };
        let radius = PositiveLength::new(circle.radius.get() * scale_mm).ok_or_else(|| {
            CodecError::NotImplemented(
                "Creo reference circle radius cannot be represented in millimeters".into(),
            )
        })?;
        let index = ir.model.curves.len();
        ir.model.curves.push(Curve {
            id,
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                cadmpeg_ir::geometry::analytic::CircleCurve::new(center, frame, radius),
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
        transferred_indices.insert(index);
    }
    Ok(transferred_indices)
}

fn transfer_reference_ellipses(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> Result<BTreeSet<usize>, CodecError> {
    let length_scale = scan
        .framing
        .principal_unit
        .and_then(crate::legacy::PrincipalUnitSystem::length_scale_mm);
    let scale_mm = length_scale.map_or(1.0, cadmpeg_ir::scalar::PositiveReal::get);
    let mut transferred_indices = BTreeSet::new();
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
        ctx.charge_entities(1, "admit Creo model curves")?;
        let frame =
            cadmpeg_ir::units::OrthonormalFrame3::from_units(ellipse.axis, ellipse.major_direction)
                .ok_or_else(|| {
                    CodecError::malformed(
                        "EllipseCurve.axis/ref_direction must form an orthonormal frame",
                    )
                })?;
        let center = if let Some(scale) = length_scale {
            ellipse.center.scaled(scale).ok_or_else(|| {
                CodecError::NotImplemented(
                    "Creo reference ellipse center cannot be represented in millimeters".into(),
                )
            })?
        } else {
            ellipse.center
        };
        let major_radius =
            PositiveLength::new(ellipse.major_radius.get() * scale_mm).ok_or_else(|| {
                CodecError::NotImplemented(
                    "Creo reference ellipse major radius cannot be represented in millimeters"
                        .into(),
                )
            })?;
        let minor_radius =
            PositiveLength::new(ellipse.minor_radius.get() * scale_mm).ok_or_else(|| {
                CodecError::NotImplemented(
                    "Creo reference ellipse minor radius cannot be represented in millimeters"
                        .into(),
                )
            })?;
        let index = ir.model.curves.len();
        ir.model.curves.push(Curve {
            id,
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(
                cadmpeg_ir::geometry::analytic::EllipseCurve::try_from_parts(
                    center,
                    frame,
                    major_radius,
                    minor_radius,
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
        transferred_indices.insert(index);
    }
    Ok(transferred_indices)
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
        let id = format!("creo:solid_primdata:tessellation#{}", strip.offset);
        annotate(
            annotations,
            &id,
            "SolidPrimdata",
            strip.offset as u64,
            "display_triangle_strip",
            Exactness::Derived,
        );
        ctx.charge_entities(1, "admit Creo model tessellations")?;
        let positions = strip
            .positions
            .iter()
            .copied()
            .map(|position| {
                let position = Point3::from(position);
                let Some(scale) = length_scale else {
                    return Ok(position);
                };
                if !position.is_finite() {
                    return Err(CodecError::malformed(format_args!(
                        "SolidPrimdata display triangle strip at byte {}: vertices contain a non-finite coordinate",
                        strip.offset
                    )));
                }
                let position = Point3::new(
                    position.x * scale.get(),
                    position.y * scale.get(),
                    position.z * scale.get(),
                );
                if !position.is_finite() {
                    return Err(CodecError::NotImplemented(format!(
                        "SolidPrimdata display triangle strip at byte {} has a vertex that cannot be represented in millimeters",
                        strip.offset
                    )));
                }
                Ok(position)
            })
            .collect::<Result<Vec<_>, CodecError>>()?;
        ir.model.tessellations.push(
            Tessellation::new(
                id,
                cadmpeg_ir::tessellation::TessellationMesh::from_strip_lanes(
                    positions,
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> Result<BTreeSet<usize>, CodecError> {
    let length_scale = scan
        .framing
        .principal_unit
        .and_then(crate::legacy::PrincipalUnitSystem::length_scale_mm);
    let mut transferred_indices = BTreeSet::new();
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
        ctx.charge_entities(1, "admit Creo model surfaces")?;
        let origin = plane_origin_mm(
            Point3::new(
                normal[0] * plane.plane.offset,
                normal[1] * plane.plane.offset,
                normal[2] * plane.plane.offset,
            ),
            length_scale,
        )?;
        let index = ir.model.surfaces.len();
        ir.model.surfaces.push(Surface {
            id,
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    origin,
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
        transferred_indices.insert(index);
    }
    Ok(transferred_indices)
}

fn plane_origin_mm(origin: Point3, scale: Option<PositiveReal>) -> Result<Point3, CodecError> {
    if !origin.is_finite() {
        return Err(CodecError::malformed("PlaneSurface.origin must be finite"));
    }
    let Some(scale) = scale else {
        return Ok(origin);
    };
    let origin = Point3::new(
        origin.x * scale.get(),
        origin.y * scale.get(),
        origin.z * scale.get(),
    );
    if !origin.is_finite() {
        return Err(CodecError::NotImplemented(
            "Creo plane origin cannot be represented in millimeters".into(),
        ));
    }
    Ok(origin)
}

fn transfer_placed_plane_surfaces_into_ir(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> Result<BTreeSet<usize>, CodecError> {
    let length_scale = scan
        .framing
        .principal_unit
        .and_then(crate::legacy::PrincipalUnitSystem::length_scale_mm);
    let mut transferred_indices = BTreeSet::new();
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
        ctx.charge_entities(1, "admit Creo model surfaces")?;
        let origin = plane_origin_mm(Point3::from(plane.origin), length_scale)?;
        let index = ir.model.surfaces.len();
        ir.model.surfaces.push(Surface {
            id,
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    origin,
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
        transferred_indices.insert(index);
    }
    Ok(transferred_indices)
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
    let unknowns = preserve_passthrough_sections(ctx, scan, &mut annotations)?;
    emit_reference_arenas(scan, &mut ir, &mut annotations)?;
    let mut converted_reference_curves =
        transfer_reference_lines(ctx, scan, &mut ir, &mut annotations)?;
    converted_reference_curves.extend(transfer_reference_circles(
        ctx,
        scan,
        &mut ir,
        &mut annotations,
    )?);
    converted_reference_curves.extend(transfer_reference_ellipses(
        ctx,
        scan,
        &mut ir,
        &mut annotations,
    )?);
    transfer_display_tessellations(ctx, scan, &mut ir, &mut annotations)?;
    let mut converted_surfaces =
        transfer_datum_plane_surfaces(ctx, scan, &mut ir, &mut annotations)?;
    converted_surfaces.extend(transfer_placed_plane_surfaces_into_ir(
        ctx,
        scan,
        &mut ir,
        &mut annotations,
    )?);
    transfer_and_record_scanned_geometry(
        ctx,
        scan,
        &mut ir,
        &mut annotations,
        &mut coverage,
        &mut brep_diagnostics,
        &mut transfer_losses,
    )?;
    let geometry_generator_feature_count =
        emit_model_features(ctx, scan, &mut ir, &mut annotations)?;
    let (feature_result_topology_count, feature_result_edge_count) =
        finish_feature_transfers(ctx, scan, &mut ir, &mut annotations, &mut coverage)?;
    attach_expanded_sections(scan, &mut ir, &mut annotations)?;
    emit_geometry_arenas(scan, &mut ir, &mut annotations, &brep_diagnostics)?;
    if let Some(length_scale_mm) = scan
        .framing
        .principal_unit
        .and_then(crate::legacy::PrincipalUnitSystem::length_scale_mm)
    {
        super::units::normalize_model_lengths(
            &mut ir,
            length_scale_mm,
            &super::units::ConvertedGeometry {
                curves: converted_reference_curves,
                surfaces: converted_surfaces,
            },
        )?;
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

#[cfg(test)]
mod tests;
