// SPDX-License-Identifier: Apache-2.0
//! Conversion of neutral Creo values into the canonical IR length unit.
//!
//! The PSB scanner keeps source values in their stored unit so native records
//! remain faithful to the file. Admission routes use these operations to
//! convert model lengths before insertion into the IR.
//! Unit directions, angles, ratios, and source-native arenas are not scaled.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, FiniteVector3, WrapMode};
use cadmpeg_ir::geometry::scaling::ScaleRefusal;
use cadmpeg_ir::geometry::{SolvedCurveGeometry, SolvedSurfaceGeometry};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::scalar::{Length, PositiveReal};
use cadmpeg_ir::sketches::SketchGeometry;
use cadmpeg_ir::transform::Transform;

pub(in crate::decode) fn malformed_refusal(
    ctx: &DecodeContext<'_>,
    message: impl std::fmt::Display,
) -> CodecError {
    match ctx.format_retained(message, "creo unit normalization refusal text") {
        Ok(message) => CodecError::Malformed(message),
        Err(error) => error,
    }
}

pub(in crate::decode) fn not_implemented_refusal(
    ctx: &DecodeContext<'_>,
    message: impl std::fmt::Display,
) -> CodecError {
    match ctx.format_retained(message, "creo unit normalization refusal text") {
        Ok(message) => CodecError::NotImplemented(message),
        Err(error) => error,
    }
}

fn scale_point2(point: &mut Point2, scale: PositiveReal) {
    point.u *= scale.get();
    point.v *= scale.get();
}

fn scale_point3(point: &mut Point3, scale: PositiveReal) {
    point.x *= scale.get();
    point.y *= scale.get();
    point.z *= scale.get();
}

fn scale_finite_point3(
    ctx: &DecodeContext<'_>,
    point: &mut cadmpeg_ir::features::FinitePoint3,
    scale: PositiveReal,
) -> Result<(), CodecError> {
    *point = point
        .scaled(scale)
        .ok_or_else(|| malformed_refusal(ctx, "Creo scaled feature point must be finite"))?;
    Ok(())
}

fn scale_vector3(vector: &mut Vector3, scale: PositiveReal) {
    vector.x *= scale.get();
    vector.y *= scale.get();
    vector.z *= scale.get();
}

fn scale_transform_translation(
    ctx: &DecodeContext<'_>,
    transform: &mut Transform,
    scale: PositiveReal,
) -> Result<(), CodecError> {
    *transform = transform.scaled_translation(scale).ok_or_else(|| {
        malformed_refusal(
            ctx,
            format_args!(
                "Creo length scale {} drives a transform translation the carrier refuses",
                scale.get()
            ),
        )
    })?;
    Ok(())
}

pub(in crate::decode) fn scale_length(
    ctx: &DecodeContext<'_>,
    length: &mut Length,
    scale: PositiveReal,
) -> Result<(), CodecError> {
    *length = Length::new(length.get() * scale.get())
        .ok_or_else(|| malformed_refusal(ctx, "Creo scaled length must be finite"))?;
    Ok(())
}

fn scale_positive_length(
    ctx: &DecodeContext<'_>,
    length: &mut cadmpeg_ir::scalar::PositiveLength,
    scale: PositiveReal,
) -> Result<(), CodecError> {
    *length = cadmpeg_ir::scalar::PositiveLength::new(length.get() * scale.get())
        .ok_or_else(|| malformed_refusal(ctx, "Creo scaled length must be positive and finite"))?;
    Ok(())
}

fn scale_nonzero_length(
    ctx: &DecodeContext<'_>,
    length: &mut cadmpeg_ir::scalar::NonZeroLength,
    scale: PositiveReal,
) -> Result<(), CodecError> {
    *length = cadmpeg_ir::scalar::NonZeroLength::new(length.get() * scale.get())
        .ok_or_else(|| malformed_refusal(ctx, "Creo scaled length must be finite and nonzero"))?;
    Ok(())
}

fn scale_nonnegative_length(
    ctx: &DecodeContext<'_>,
    length: &mut cadmpeg_ir::scalar::NonNegativeLength,
    scale: PositiveReal,
) -> Result<(), CodecError> {
    *length = length.scaled(scale).ok_or_else(|| {
        malformed_refusal(ctx, "Creo scaled length must be nonnegative and finite")
    })?;
    Ok(())
}

fn scale_optional_positive_length(
    ctx: &DecodeContext<'_>,
    length: &mut Option<cadmpeg_ir::scalar::PositiveLength>,
    scale: PositiveReal,
) -> Result<(), CodecError> {
    if let Some(length) = length {
        scale_positive_length(ctx, length, scale)?;
    }
    Ok(())
}

fn scale_optional_length(
    ctx: &DecodeContext<'_>,
    length: &mut Option<Length>,
    scale: PositiveReal,
) -> Result<(), cadmpeg_core::CodecError> {
    if let Some(length) = length.as_mut() {
        scale_length(ctx, length, scale)?;
    }
    Ok(())
}

fn scale_datum_plane_reference(
    ctx: &DecodeContext<'_>,
    reference: &mut cadmpeg_ir::features::DatumPlaneReference,
    scale: PositiveReal,
) -> Result<(), CodecError> {
    if let cadmpeg_ir::features::DatumPlaneReference::ResolvedPlane { frame } = reference {
        let mut origin = frame.origin().get();
        scale_point3(&mut origin, scale);
        let origin = cadmpeg_ir::features::FinitePoint3::new(origin).ok_or_else(|| {
            malformed_refusal(ctx, "Creo scaled plane support must have a finite origin")
        })?;
        *frame = frame.with_origin(origin);
    }
    Ok(())
}

fn scale_datum_point_construction(
    ctx: &DecodeContext<'_>,
    construction: &mut cadmpeg_ir::features::DatumPointConstruction,
    scale: PositiveReal,
) -> Result<(), CodecError> {
    match construction {
        cadmpeg_ir::features::DatumPointConstruction::ThreePlaneIntersection { planes } => {
            for plane in planes.iter_mut() {
                scale_datum_plane_reference(ctx, plane, scale)?;
            }
        }
        cadmpeg_ir::features::DatumPointConstruction::EdgePlaneIntersection { plane, .. } => {
            scale_datum_plane_reference(ctx, plane, scale)?;
        }
        cadmpeg_ir::features::DatumPointConstruction::CircleCenter { .. }
        | cadmpeg_ir::features::DatumPointConstruction::TwoEdgeIntersection { .. }
        | cadmpeg_ir::features::DatumPointConstruction::Vertex { .. }
        | cadmpeg_ir::features::DatumPointConstruction::SketchPoint { .. }
        | cadmpeg_ir::features::DatumPointConstruction::DistanceOnEdge { .. } => {}
    }
    Ok(())
}

pub(in crate::decode) fn scale_feature_definition(
    ctx: &DecodeContext<'_>,
    definition: &mut FeatureDefinition,
    scale: PositiveReal,
) -> Result<(), cadmpeg_core::CodecError> {
    match definition {
        FeatureDefinition::PostProcess {
            operation,
            fuzzy_tolerance,
            ..
        } => {
            scale_feature_operation(ctx, operation, scale)?;
            scale_fuzzy_tolerance(ctx, fuzzy_tolerance, scale)
        }
        FeatureDefinition::Operation(operation) => scale_feature_operation(ctx, operation, scale),
    }
}

fn scale_feature_operation(
    ctx: &DecodeContext<'_>,
    definition: &mut FeatureOperation,
    scale: PositiveReal,
) -> Result<(), cadmpeg_core::CodecError> {
    match definition {
        FeatureOperation::CosmeticThread {
            diameter, extent, ..
        } => {
            scale_optional_positive_length(ctx, diameter, scale)?;
            if let Some(cadmpeg_ir::features::CosmeticThreadExtent::Blind { length }) = extent {
                scale_positive_length(ctx, length, scale)?;
            }
        }
        FeatureOperation::ReferenceImage { frame, bounds, .. } => {
            scale_unit_plane_frame(ctx, frame, scale)?;
            let mut corners = bounds.corners().map(cadmpeg_ir::units::FinitePoint2::get);
            for point in &mut corners {
                scale_point2(point, scale);
            }
            *bounds = cadmpeg_ir::features::FeatureImageBounds::new(corners).ok_or_else(|| {
                malformed_refusal(
                    ctx,
                    "Creo scaled image bounds must have finite corners and nonzero extents",
                )
            })?;
        }
        FeatureOperation::DatumCoordinateSystem { frame } => {
            let mut origin = frame.origin().get();
            scale_point3(&mut origin, scale);
            let origin = cadmpeg_ir::features::FinitePoint3::new(origin).ok_or_else(|| {
                malformed_refusal(
                    ctx,
                    "Creo scaled coordinate frame must have a finite origin",
                )
            })?;
            *frame = frame.with_origin(origin);
        }
        FeatureOperation::DatumPlane { frame }
        | FeatureOperation::DatumThreePointPlane { frame, .. } => {
            let mut origin = frame.origin().get();
            scale_point3(&mut origin, scale);
            let origin = cadmpeg_ir::features::FinitePoint3::new(origin).ok_or_else(|| {
                malformed_refusal(ctx, "Creo scaled datum plane must have a finite origin")
            })?;
            *frame = frame.with_origin(origin);
        }
        FeatureOperation::DatumAxis { origin, .. }
        | FeatureOperation::MirrorShape {
            plane_origin: origin,
            ..
        } => {
            scale_finite_point3(ctx, origin, scale)?;
        }
        FeatureOperation::DatumPoint {
            position,
            construction,
        } => {
            scale_finite_point3(ctx, position, scale)?;
            if let Some(construction) = construction {
                scale_datum_point_construction(ctx, construction, scale)?;
            }
        }
        FeatureOperation::DatumOffsetPlane {
            reference,
            distance,
        } => {
            if let Some(reference) = reference {
                scale_datum_plane_reference(ctx, reference, scale)?;
            }
            scale_length(ctx, distance, scale)?;
        }
        FeatureOperation::PointGeometry { position } => {
            scale_finite_point3(ctx, position, scale)?;
        }
        FeatureOperation::LineSegment { segment } => {
            let mut start = segment.start().get();
            let mut end = segment.end().get();
            scale_point3(&mut start, scale);
            scale_point3(&mut end, scale);
            *segment =
                cadmpeg_ir::features::FeatureLineSegment::new(start, end).ok_or_else(|| {
                    malformed_refusal(ctx, "Creo scaled line must have finite distinct endpoints")
                })?;
        }
        FeatureOperation::CircularArc { arc } => {
            let mut center = arc.center().get();
            let mut radius = arc.radius();
            scale_point3(&mut center, scale);
            scale_positive_length(ctx, &mut radius, scale)?;
            let center = cadmpeg_ir::features::FinitePoint3::new(center).ok_or_else(|| {
                malformed_refusal(ctx, "Creo scaled circular arc must have finite geometry")
            })?;
            *arc = cadmpeg_ir::features::FeatureCircularArc::from_parts(
                center,
                arc.normal(),
                radius,
                arc.angles(),
            );
        }
        FeatureOperation::EllipticArc { arc } => {
            let center = arc.center().scaled(scale);
            let scaled = arc.with_scaled_radii(scale).ok_or_else(|| {
                malformed_refusal(ctx, "Creo scaled length must be positive and finite")
            })?;
            let center = center.ok_or_else(|| {
                malformed_refusal(
                    ctx,
                    "Creo scaled elliptic arc must have finite ordered geometry",
                )
            })?;
            *arc = scaled.with_center(center);
        }
        FeatureOperation::Polyline { chain } => {
            let mut points = Vec::new();
            ctx.try_reserve_items(
                &mut points,
                chain.points().len(),
                "creo scaled feature polyline points",
            )?;
            for point in chain.points() {
                ctx.charge_work(2, "creo unit scaling polyline work")?;
                let scaled = point.scaled(scale).ok_or_else(|| {
                    malformed_refusal(
                        ctx,
                        "Creo scaled polyline must have finite distinct adjacent vertices",
                    )
                })?;
                points.push(scaled);
            }
            *chain = cadmpeg_ir::features::FeaturePolyline::from_parts(points, chain.closed())
                .ok_or_else(|| {
                    malformed_refusal(
                        ctx,
                        "Creo scaled polyline must have finite distinct adjacent vertices",
                    )
                })?;
        }
        FeatureOperation::RegularPolygonCurve { circumradius, .. } => {
            scale_positive_length(ctx, circumradius, scale)?;
        }
        FeatureOperation::PlanarPatch { length, width } => {
            scale_positive_length(ctx, length, scale)?;
            scale_positive_length(ctx, width, scale)?;
        }
        FeatureOperation::Block {
            dimensions,
            placement,
            ..
        } => {
            if let Some(dimensions) = dimensions {
                for dimension in dimensions {
                    scale_positive_length(ctx, dimension, scale)?;
                }
            }
            if let Some(placement) = placement {
                *placement = placement.scaled_translation(scale).ok_or_else(|| {
                    malformed_refusal(
                        ctx,
                        "Creo scaled block placement must remain finite and rigid",
                    )
                })?;
            }
        }
        FeatureOperation::ProjectOnSurface { height, offset, .. } => {
            scale_nonnegative_length(ctx, height, scale)?;
            scale_length(ctx, offset, scale)?;
        }
        FeatureOperation::Helix {
            axis_origin,
            radius,
            shape,
            ..
        } => {
            scale_finite_point3(ctx, axis_origin, scale)?;
            scale_positive_length(ctx, radius, scale)?;
            match shape {
                cadmpeg_ir::features::HelixShape::Cylindrical { pitch }
                | cadmpeg_ir::features::HelixShape::Conical { pitch, .. } => {
                    scale_nonzero_length(ctx, pitch, scale)?;
                }
                cadmpeg_ir::features::HelixShape::Spiral { radial_growth } => {
                    scale_length(ctx, radial_growth, scale)?;
                }
            }
        }
        FeatureOperation::HelixNativeAxis {
            axial_rise, pitch, ..
        } => {
            scale_length(ctx, axial_rise, scale)?;
            scale_length(ctx, pitch, scale)?;
        }
        FeatureOperation::Sphere { center, radius, .. } => {
            scale_finite_point3(ctx, center, scale)?;
            scale_positive_length(ctx, radius, scale)?;
        }
        FeatureOperation::Torus {
            center,
            major_radius,
            minor_radius,
            ..
        } => {
            scale_finite_point3(ctx, center, scale)?;
            scale_positive_length(ctx, major_radius, scale)?;
            scale_positive_length(ctx, minor_radius, scale)?;
        }
        FeatureOperation::Wrap {
            mode: WrapMode::Emboss { depth } | WrapMode::Deboss { depth },
            ..
        } => scale_positive_length(ctx, depth, scale)?,
        FeatureOperation::Wrap {
            mode: WrapMode::Scribe,
            ..
        } => {}
        FeatureOperation::SketchBlockInstance {
            placement: Some(placement),
            ..
        } => scale_transform_translation(ctx, placement, scale)?,
        FeatureOperation::SketchBlockInstance {
            placement: None, ..
        } => {}
        FeatureOperation::Primitive { solid, .. } => scale_primitive_solid(ctx, solid, scale)?,
        FeatureOperation::Sweep { shape, .. } => {
            let count = u64::try_from(shape.additional_section_count())
                .ok()
                .and_then(|count| count.checked_add(1))
                .ok_or_else(|| {
                    ctx.refuse_codec_limit("creo unit scaling member work", u64::MAX, u64::MAX)
                })?;
            ctx.charge_work(count, "creo unit scaling member work")?;
            for generated in shape.generated_sections_mut() {
                scale_sweep_section(ctx, generated, scale)?;
            }
        }
        FeatureOperation::HelicalSweep { construction, .. } => {
            scale_finite_point3(ctx, &mut construction.axis_origin, scale)?;
            scale_nonnegative_length(ctx, &mut construction.pitch, scale)?;
            let mut height = construction.travel.height();
            let mut radial_growth = construction.travel.radial_growth();
            scale_length(ctx, &mut height, scale)?;
            scale_length(ctx, &mut radial_growth, scale)?;
            construction.travel = cadmpeg_ir::features::HelicalSweepTravel::new(
                height,
                radial_growth,
            )
            .ok_or_else(|| {
                malformed_refusal(ctx, "Creo scaled helical sweep must retain nonzero travel")
            })?;
        }
        FeatureOperation::Coil { construction, .. } => {
            scale_coil_construction(ctx, construction, scale)?;
        }
        FeatureOperation::Binder {
            construction:
                cadmpeg_ir::features::BinderConstruction::SubShape {
                    offset: Some(offset),
                    ..
                },
            ..
        } => scale_nonzero_length(ctx, &mut offset.distance, scale)?,
        FeatureOperation::Binder { .. } => {}
        FeatureOperation::Loft { sections, .. } => {
            for section in sections {
                ctx.charge_work(1, "creo unit scaling member work")?;
                if let cadmpeg_ir::features::LoftSection::Point(
                    cadmpeg_ir::features::LoftPointSection::Point(point),
                ) = section
                {
                    scale_finite_point3(ctx, point, scale)?;
                }
            }
        }
        FeatureOperation::Extrude { start, extent, .. } => {
            scale_extrude_start(ctx, start, scale)?;
            scale_extrude_extent(ctx, extent, scale)?;
        }
        FeatureOperation::Revolve { construction, .. } => {
            if let Some(axis) = construction.axis_mut() {
                scale_finite_point3(ctx, &mut axis.origin, scale)?;
            }
            if let Some(extent) = construction.extent_mut() {
                scale_revolve_extent(ctx, extent, scale)?;
            }
        }
        FeatureOperation::Rib { construction, .. } => {
            scale_optional_positive_length(ctx, &mut construction.thickness, scale)?;
        }
        FeatureOperation::SheetMetalBaseFlange { thickness, .. } => {
            scale_positive_length(ctx, thickness, scale)?;
        }
        FeatureOperation::SheetMetalEdgeFlange {
            height,
            width,
            bend_radius,
            ..
        } => {
            scale_sheet_metal_flange_height(ctx, height, scale)?;
            scale_sheet_metal_flange_width(ctx, width, scale)?;
            scale_positive_length(ctx, bend_radius, scale)?;
        }
        FeatureOperation::SheetMetalHem {
            form, bend_radius, ..
        } => {
            scale_sheet_metal_hem_form(ctx, form, scale)?;
            scale_positive_length(ctx, bend_radius, scale)?;
        }
        FeatureOperation::Fillet { groups } => {
            for group in groups {
                ctx.charge_work(1, "creo unit scaling member work")?;
                scale_radius_spec(ctx, &mut group.radius, scale)?;
            }
        }
        FeatureOperation::FaceBlend { radius, .. } => scale_radius_spec(ctx, radius, scale)?,
        FeatureOperation::Chamfer { groups, .. } => {
            for group in groups {
                ctx.charge_work(1, "creo unit scaling member work")?;
                scale_chamfer_spec(ctx, &mut group.spec, scale)?;
            }
        }
        FeatureOperation::Shell { thickness, .. } => {
            scale_optional_positive_length(ctx, thickness, scale)?;
        }
        FeatureOperation::OffsetShape { distance, .. } => {
            scale_nonzero_length(ctx, distance, scale)?;
        }
        FeatureOperation::Thicken { thickness, .. } => {
            scale_optional_positive_length(ctx, thickness, scale)?;
        }
        FeatureOperation::OffsetSurface { distance, .. } => {
            scale_optional_length(ctx, distance, scale)?;
        }
        FeatureOperation::KnitSurface {
            gap_tolerance: Some(gap),
            ..
        } => {
            scale_nonnegative_length(ctx, gap, scale)?;
        }
        FeatureOperation::SewBodies { gap_tolerance, .. } => {
            scale_optional_positive_length(ctx, gap_tolerance, scale)?;
        }
        FeatureOperation::ExtendSurface { distance, .. } => {
            scale_optional_positive_length(ctx, distance, scale)?;
        }
        FeatureOperation::RuledSurface { mode, .. } => scale_ruled_surface_mode(ctx, mode, scale)?,
        FeatureOperation::Draft { .. } => {}
        FeatureOperation::MoveFace { motion, .. } => scale_face_motion(ctx, motion, scale)?,
        FeatureOperation::MoveBody {
            translation,
            rotation,
            ..
        } => {
            *translation = cadmpeg_ir::features::FiniteVector3::new(translation.scale(scale.get()))
                .ok_or_else(|| {
                    malformed_refusal(ctx, "Creo scaled body translation must be finite")
                })?;
            if let Some(rotation) = rotation {
                scale_finite_point3(ctx, &mut rotation.origin, scale)?;
            }
        }
        FeatureOperation::Dome { height, .. } => {
            scale_optional_positive_length(ctx, height, scale)?;
        }
        FeatureOperation::Flex { mode, .. } => scale_flex_mode(ctx, mode, scale)?,
        FeatureOperation::Scale {
            center: Some(cadmpeg_ir::features::ScaleCenter::Point(point)),
            ..
        } => scale_finite_point3(ctx, point, scale)?,
        FeatureOperation::Scale { .. } => {}
        FeatureOperation::Hole {
            placements,
            shape,
            extent,
            ..
        } => {
            for placement in placements.iter_mut().flatten() {
                ctx.charge_work(1, "creo unit scaling member work")?;
                scale_hole_placement(ctx, placement, scale)?;
            }
            scale_hole_shape(ctx, shape, scale)?;
            if let Some(extent) = extent {
                scale_linear_termination(ctx, extent, scale)?;
            }
        }
        FeatureOperation::Pattern { pattern, .. } => scale_pattern_kind(ctx, pattern, scale)?,
        _ => {}
    }
    Ok(())
}

fn scale_fuzzy_tolerance(
    ctx: &DecodeContext<'_>,
    tolerance: &mut cadmpeg_ir::features::FuzzyTolerance,
    scale: PositiveReal,
) -> Result<(), CodecError> {
    if let cadmpeg_ir::features::FuzzyTolerance::Explicit(value) = tolerance {
        scale_positive_length(ctx, value, scale)?;
    }
    Ok(())
}

fn scale_primitive_solid(
    ctx: &DecodeContext<'_>,
    solid: &mut cadmpeg_ir::features::PrimitiveSolid,
    scale: PositiveReal,
) -> Result<(), cadmpeg_core::CodecError> {
    *solid = solid.scaled(scale).map_err(|error| match error {
        cadmpeg_ir::features::PrimitiveSolidScaleError::NonFinite => {
            malformed_refusal(ctx, "Creo scaled length must be finite")
        }
        cadmpeg_ir::features::PrimitiveSolidScaleError::Admission(message) => {
            malformed_refusal(ctx, message)
        }
    })?;
    Ok(())
}

fn scale_sweep_section(
    ctx: &DecodeContext<'_>,
    section: &mut cadmpeg_ir::features::GeneratedSweepSection,
    scale: PositiveReal,
) -> Result<(), cadmpeg_core::CodecError> {
    let cadmpeg_ir::features::GeneratedSweepSection::CircularRegion { region } = section;
    let mut outer_radius = region.outer_radius();
    let mut wall_thickness = region.wall_thickness();
    scale_positive_length(ctx, &mut outer_radius, scale)?;
    scale_optional_positive_length(ctx, &mut wall_thickness, scale)?;
    *region = cadmpeg_ir::features::SweepCircularRegion::new(outer_radius, wall_thickness)
        .map_err(|message| malformed_refusal(ctx, message))?;
    Ok(())
}

fn scale_unit_plane_frame(
    ctx: &DecodeContext<'_>,
    frame: &mut cadmpeg_ir::features::FeatureUnitPlaneFrame,
    scale: PositiveReal,
) -> Result<(), CodecError> {
    let mut origin = frame.origin().get();
    scale_point3(&mut origin, scale);
    let origin = cadmpeg_ir::features::FinitePoint3::new(origin).ok_or_else(|| {
        malformed_refusal(ctx, "Creo scaled plane frame must have a finite origin")
    })?;
    *frame = frame.with_origin(origin);
    Ok(())
}

fn scale_coil_construction(
    ctx: &DecodeContext<'_>,
    construction: &mut cadmpeg_ir::features::CoilConstruction,
    scale: PositiveReal,
) -> Result<(), cadmpeg_core::CodecError> {
    if let cadmpeg_ir::features::CoilPlacement::Explicit { frame } = &mut construction.placement {
        scale_unit_plane_frame(ctx, frame, scale)?;
    }
    scale_positive_length(ctx, &mut construction.diameter, scale)?;
    match &mut construction.extent {
        cadmpeg_ir::features::CoilExtent::RevolutionsHeight { height, .. } => {
            scale_length(ctx, height, scale)?;
        }
        cadmpeg_ir::features::CoilExtent::RevolutionsPitch { pitch, .. } => {
            scale_nonzero_length(ctx, pitch, scale)?;
        }
        cadmpeg_ir::features::CoilExtent::HeightPitch { height, pitch } => {
            scale_nonzero_length(ctx, height, scale)?;
            scale_nonzero_length(ctx, pitch, scale)?;
        }
        cadmpeg_ir::features::CoilExtent::Spiral { radial_pitch, .. } => {
            scale_nonzero_length(ctx, radial_pitch, scale)?;
        }
    }
    match &mut construction.section {
        cadmpeg_ir::features::CoilSection::Circular { diameter }
        | cadmpeg_ir::features::CoilSection::Square { size: diameter }
        | cadmpeg_ir::features::CoilSection::ExternalTriangle { size: diameter }
        | cadmpeg_ir::features::CoilSection::InternalTriangle { size: diameter } => {
            scale_positive_length(ctx, diameter, scale)?;
        }
    }
    Ok(())
}

fn scale_extrude_start(
    ctx: &DecodeContext<'_>,
    start: &mut cadmpeg_ir::features::ExtrudeStart,
    scale: PositiveReal,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::ExtrudeStart;

    match start {
        ExtrudeStart::OffsetProfilePlane { offset } => scale_length(ctx, offset, scale)?,
        ExtrudeStart::FromFace { offset, .. } => scale_optional_length(ctx, offset, scale)?,
        ExtrudeStart::Unresolved {} | ExtrudeStart::ProfilePlane {} => {}
    }
    Ok(())
}

fn scale_extrude_side(
    ctx: &DecodeContext<'_>,
    side: &mut cadmpeg_ir::features::ExtrudeSide,
    scale: PositiveReal,
) -> Result<(), cadmpeg_core::CodecError> {
    scale_linear_termination(ctx, &mut side.termination, scale)?;
    Ok(())
}

fn scale_extrude_extent(
    ctx: &DecodeContext<'_>,
    extent: &mut cadmpeg_ir::features::ExtrudeExtent,
    scale: PositiveReal,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::ExtrudeExtent;

    match extent {
        ExtrudeExtent::OneSided { side } | ExtrudeExtent::Symmetric { side } => {
            scale_extrude_side(ctx, side, scale)?;
        }
        ExtrudeExtent::TwoSided { first, second } => {
            scale_extrude_side(ctx, first, scale)?;
            scale_extrude_side(ctx, second, scale)?;
        }
    }
    Ok(())
}

fn scale_revolve_extent(
    ctx: &DecodeContext<'_>,
    extent: &mut cadmpeg_ir::features::RevolveExtent,
    scale: PositiveReal,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::RevolveExtent;

    match extent {
        RevolveExtent::OneSided { termination } | RevolveExtent::Symmetric { termination } => {
            scale_angular_termination(ctx, termination, scale)?;
        }
        RevolveExtent::TwoSided { first, second } => {
            scale_angular_termination(ctx, first, scale)?;
            scale_angular_termination(ctx, second, scale)?;
        }
    }
    Ok(())
}

fn scale_linear_termination(
    ctx: &DecodeContext<'_>,
    termination: &mut cadmpeg_ir::features::LinearTermination,
    scale: PositiveReal,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::LinearTermination;

    match termination {
        LinearTermination::Blind { length } => scale_nonzero_length(ctx, length, scale)?,
        LinearTermination::ToFace { offset, .. } => scale_optional_length(ctx, offset, scale)?,
        LinearTermination::OffsetFromFace { offset, .. } => {
            scale_positive_length(ctx, offset, scale)?;
        }
        LinearTermination::Unresolved {}
        | LinearTermination::ThroughAll {}
        | LinearTermination::ThroughNext {}
        | LinearTermination::ToFirst {}
        | LinearTermination::ToLast {}
        | LinearTermination::ToVertex { .. }
        | LinearTermination::ToShape { .. } => {}
    }
    Ok(())
}

fn scale_angular_termination(
    ctx: &DecodeContext<'_>,
    termination: &mut cadmpeg_ir::features::AngularTermination,
    scale: PositiveReal,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::AngularTermination;

    match termination {
        AngularTermination::ToFace { offset, .. } => scale_optional_length(ctx, offset, scale)?,
        AngularTermination::OffsetFromFace { offset, .. } => {
            scale_positive_length(ctx, offset, scale)?;
        }
        AngularTermination::Unresolved {}
        | AngularTermination::ThroughAll {}
        | AngularTermination::ThroughNext {}
        | AngularTermination::ToFirst {}
        | AngularTermination::ToLast {}
        | AngularTermination::ToVertex { .. }
        | AngularTermination::ToShape { .. }
        | AngularTermination::Angle { .. } => {}
    }
    Ok(())
}

fn scale_sheet_metal_flange_height(
    ctx: &DecodeContext<'_>,
    height: &mut cadmpeg_ir::features::SheetMetalFlangeHeight,
    scale: PositiveReal,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::SheetMetalFlangeHeight;

    match height {
        SheetMetalFlangeHeight::Distance(distance) => scale_positive_length(ctx, distance, scale)?,
        SheetMetalFlangeHeight::ToObject { offset, .. } => scale_length(ctx, offset, scale)?,
    }
    Ok(())
}

fn scale_sheet_metal_flange_width(
    ctx: &DecodeContext<'_>,
    width: &mut cadmpeg_ir::features::SheetMetalFlangeWidth,
    scale: PositiveReal,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::SheetMetalFlangeWidth;

    match width {
        SheetMetalFlangeWidth::Symmetric { width } => scale_positive_length(ctx, width, scale)?,
        SheetMetalFlangeWidth::TwoSides { first, second } => {
            scale_positive_length(ctx, first, scale)?;
            scale_positive_length(ctx, second, scale)?;
        }
        SheetMetalFlangeWidth::TwoSidesPerEdge { widths } => {
            for width in widths.as_mut_slice() {
                scale_positive_length(ctx, &mut width.first, scale)?;
                scale_positive_length(ctx, &mut width.second, scale)?;
            }
        }
        SheetMetalFlangeWidth::FullEdge => {}
    }
    Ok(())
}

fn scale_sheet_metal_hem_form(
    ctx: &DecodeContext<'_>,
    form: &mut cadmpeg_ir::features::SheetMetalHemForm,
    scale: PositiveReal,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::SheetMetalHemForm;

    match form {
        SheetMetalHemForm::Flat { length } | SheetMetalHemForm::Rolled { radius: length, .. } => {
            scale_positive_length(ctx, length, scale)?;
        }
        SheetMetalHemForm::Open { gap, length } | SheetMetalHemForm::GapLength { gap, length } => {
            scale_nonnegative_length(ctx, gap, scale)?;
            scale_positive_length(ctx, length, scale)?;
        }
        SheetMetalHemForm::Teardrop {
            gap,
            length,
            radius,
        } => {
            scale_nonnegative_length(ctx, gap, scale)?;
            scale_positive_length(ctx, length, scale)?;
            scale_positive_length(ctx, radius, scale)?;
        }
    }
    Ok(())
}

fn scale_radius_spec(
    ctx: &DecodeContext<'_>,
    radius: &mut cadmpeg_ir::features::edge_treatments::RadiusSpec,
    scale: PositiveReal,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::edge_treatments::RadiusSpec;

    let owned = std::mem::replace(radius, RadiusSpec::Unresolved { form: None });
    *radius = match owned {
        RadiusSpec::Constant { mut radius } => {
            scale_positive_length(ctx, &mut radius, scale)?;
            RadiusSpec::Constant { radius }
        }
        RadiusSpec::Chordal { mut chord_length } => {
            scale_positive_length(ctx, &mut chord_length, scale)?;
            RadiusSpec::Chordal { chord_length }
        }
        RadiusSpec::Asymmetric {
            mut offset_one,
            mut offset_two,
        } => {
            scale_positive_length(ctx, &mut offset_one, scale)?;
            scale_positive_length(ctx, &mut offset_two, scale)?;
            RadiusSpec::Asymmetric {
                offset_one,
                offset_two,
            }
        }
        RadiusSpec::Variable { points } => {
            use cadmpeg_ir::features::edge_treatments::VariableRadiiMapError;
            RadiusSpec::Variable {
                points: points
                    .try_map_radii_owned_admitted(ctx, |radius| {
                        radius.scaled(scale).ok_or_else(|| {
                            malformed_refusal(ctx, "Creo scaled length must be finite")
                        })
                    })?
                    .map_err(|error| match error {
                        VariableRadiiMapError::Radius(error) => error,
                        VariableRadiiMapError::Admission(message) => {
                            malformed_refusal(ctx, message)
                        }
                    })?,
            }
        }
        unresolved @ RadiusSpec::Unresolved { .. } => unresolved,
    };
    Ok(())
}

fn scale_chamfer_spec(
    ctx: &DecodeContext<'_>,
    spec: &mut cadmpeg_ir::features::edge_treatments::ChamferSpec,
    scale: PositiveReal,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::edge_treatments::ChamferSpec;

    match spec {
        ChamferSpec::Distance { distance } | ChamferSpec::DistanceAngle { distance, .. } => {
            scale_positive_length(ctx, distance, scale)?;
        }
        ChamferSpec::TwoDistances { first, second } => {
            scale_positive_length(ctx, first, scale)?;
            scale_positive_length(ctx, second, scale)?;
        }
        cadmpeg_ir::features::edge_treatments::ChamferSpec::Unresolved { .. } => {}
    }
    Ok(())
}

fn scale_ruled_surface_mode(
    ctx: &DecodeContext<'_>,
    mode: &mut cadmpeg_ir::features::RuledSurfaceMode,
    scale: PositiveReal,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::RuledSurfaceMode;

    match mode {
        RuledSurfaceMode::Normal { distance }
        | RuledSurfaceMode::Tangent { distance }
        | RuledSurfaceMode::Direction { distance, .. } => {
            scale_positive_length(ctx, distance, scale)?;
        }
    }
    Ok(())
}

fn scale_face_motion(
    ctx: &DecodeContext<'_>,
    motion: &mut cadmpeg_ir::features::FaceMotion,
    scale: PositiveReal,
) -> Result<(), cadmpeg_core::CodecError> {
    match motion {
        cadmpeg_ir::features::FaceMotion::Offset { distance }
        | cadmpeg_ir::features::FaceMotion::Translate { distance, .. } => {
            scale_length(ctx, distance, scale)?;
        }
        cadmpeg_ir::features::FaceMotion::Rotate { axis_origin, .. } => {
            scale_finite_point3(ctx, axis_origin, scale)?;
        }
    }
    Ok(())
}

fn scale_flex_mode(
    ctx: &DecodeContext<'_>,
    mode: &mut cadmpeg_ir::features::FlexMode,
    scale: PositiveReal,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::FlexMode;

    match mode {
        FlexMode::Unresolved { .. } => {}
        FlexMode::Stretching { distance } => scale_length(ctx, distance, scale)?,
        FlexMode::Bending { .. } | FlexMode::Twisting { .. } | FlexMode::Tapering { .. } => {}
    }
    Ok(())
}

fn scale_hole_placement(
    ctx: &DecodeContext<'_>,
    placement: &mut cadmpeg_ir::features::holes::HolePlacement,
    scale: PositiveReal,
) -> Result<(), CodecError> {
    match placement {
        cadmpeg_ir::features::holes::HolePlacement::Directed { position, .. }
        | cadmpeg_ir::features::holes::HolePlacement::Axis {
            origin: position, ..
        } => scale_finite_point3(ctx, position, scale),
    }
}

/// Scale the bore and treatment dimensions of a hole. The treatment
/// diameters exceed the bore diameter strictly, and two scaled diameters can
/// round to one value, so the relation is admitted again.
fn scale_hole_shape(
    ctx: &DecodeContext<'_>,
    shape: &mut cadmpeg_ir::features::holes::HoleShape,
    scale: PositiveReal,
) -> Result<(), CodecError> {
    use cadmpeg_ir::features::holes::HoleLengthEditError;

    let placeholder = cadmpeg_ir::features::holes::HoleShape::new(
        cadmpeg_ir::features::holes::HoleConstruction::Form {
            kind: cadmpeg_ir::features::holes::HoleKind::Simple,
            specification: None,
        },
        None,
        None,
    )
    .map_err(|message| malformed_refusal(ctx, message))?;
    let owned = std::mem::replace(shape, placeholder);
    *shape = owned
        .try_map_lengths_owned(
            &mut |value| {
                let mut value = value;
                scale_positive_length(ctx, &mut value, scale)?;
                Ok(value)
            },
            &mut |value| {
                let mut value = value;
                scale_length(ctx, &mut value, scale)?;
                Ok(value)
            },
        )
        .map_err(|error| match error {
            HoleLengthEditError::Field(error) => error,
            HoleLengthEditError::Counterdrill(message)
            | HoleLengthEditError::Treatment(message) => malformed_refusal(ctx, message),
        })?;
    Ok(())
}

fn scale_pattern_kind<C: cadmpeg_ir::features::patterns::CompositeStages>(
    ctx: &DecodeContext<'_>,
    pattern: &mut cadmpeg_ir::features::patterns::PatternKind<C>,
    scale: PositiveReal,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::patterns::{PatternLengthEditError, PatternLengthField};

    charge_pattern_scaling_work(ctx, pattern)?;
    let owned = std::mem::replace(
        pattern,
        cadmpeg_ir::features::patterns::PatternKind::UNRESOLVED,
    );
    *pattern = owned
        .try_map_lengths_owned(&mut |field| match field {
            PatternLengthField::Length(length) => scale_length(ctx, length, scale),
            PatternLengthField::PositiveLength(length) => scale_positive_length(ctx, length, scale),
            PatternLengthField::Point(point) => scale_finite_point3(ctx, point, scale),
        })
        .map_err(|error| match error {
            PatternLengthEditError::Field(error) => error,
            PatternLengthEditError::Offsets(message) => malformed_refusal(ctx, message),
        })?;
    Ok(())
}

fn charge_pattern_scaling_work<C: cadmpeg_ir::features::patterns::CompositeStages>(
    ctx: &DecodeContext<'_>,
    pattern: &cadmpeg_ir::features::patterns::PatternKind<C>,
) -> Result<(), CodecError> {
    use cadmpeg_ir::features::patterns::PatternTransform;
    ctx.charge_work(1, "creo pattern scaling work")?;
    match pattern.definition() {
        PatternTransform::LinearOffsets { offsets, .. } => {
            for _offset in offsets {
                ctx.charge_work(2, "creo pattern offset scaling work")?;
            }
        }
        PatternTransform::Composite { stages } => {
            let _depth = ctx.enter_nested("creo pattern scaling nesting")?;
            for stage in stages.stages() {
                charge_pattern_scaling_work(ctx, &stage.pattern)?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub(in crate::decode) fn scale_surface_geometry(
    ctx: &DecodeContext<'_>,
    geometry: &mut SolvedSurfaceGeometry,
    scale: PositiveReal,
) -> Result<(), CodecError> {
    let owned = std::mem::replace(geometry, SolvedSurfaceGeometry::Unknown { record: None });
    *geometry = owned
        .scaled_owned_admitted(ctx, scale)?
        .map_err(|refusal| scale_refusal(ctx, refusal, "surface", scale))?;
    Ok(())
}

pub(in crate::decode) fn scale_curve_geometry(
    ctx: &DecodeContext<'_>,
    geometry: &mut SolvedCurveGeometry,
    scale: PositiveReal,
) -> Result<(), CodecError> {
    let owned = std::mem::replace(geometry, SolvedCurveGeometry::Unknown { record: None });
    *geometry = owned
        .scaled_owned_admitted(ctx, scale)?
        .map_err(|refusal| scale_refusal(ctx, refusal, "curve", scale))?;
    Ok(())
}

/// The codec error for a solved carrier's refusal of the unit scaling.
/// `carrier` names the carrier family in a control-point refusal.
fn scale_refusal(
    ctx: &DecodeContext<'_>,
    refusal: ScaleRefusal,
    carrier: &str,
    scale: PositiveReal,
) -> CodecError {
    match refusal {
        ScaleRefusal::Field(message) => malformed_refusal(ctx, message),
        ScaleRefusal::ControlPoints(error) => malformed_refusal(
            ctx,
            format_args!(
                "Creo {carrier} unit normalization produced invalid NURBS control points: {error}"
            ),
        ),
        ScaleRefusal::Samples(error) => malformed_refusal(ctx, error),
        ScaleRefusal::Translation => malformed_refusal(
            ctx,
            format_args!(
                "Creo length scale {} drives a transform translation the carrier refuses",
                scale.get()
            ),
        ),
    }
}

/// Refusal of a scaled revolution-axis origin that is not finite. The text is
/// the one both revolution constructions give for an axis they do not admit.
const SCALED_REVOLUTION_AXIS_REFUSAL: cadmpeg_ir::geometry::ProceduralGeometryError =
    cadmpeg_ir::geometry::ProceduralGeometryError::Payload(
        "revolution axis_origin and axis_direction must be finite, with unit axis_direction",
    );

/// Refusal of a scaled extrusion direction that is not finite. The text is
/// the one the extrusion construction gives for a direction it does not
/// admit.
const SCALED_EXTRUSION_DIRECTION_REFUSAL: cadmpeg_ir::geometry::ProceduralGeometryError =
    cadmpeg_ir::geometry::ProceduralGeometryError::Payload("Extrusion.direction is not finite");

/// Refusal of a scaled extrusion native position that is not finite. The
/// text is the one the extrusion construction gives for a position it does
/// not admit.
const SCALED_EXTRUSION_POSITION_REFUSAL: cadmpeg_ir::geometry::ProceduralGeometryError =
    cadmpeg_ir::geometry::ProceduralGeometryError::Payload(
        "Extrusion.native_position is not finite",
    );

/// Refusal of a scaled sum basepoint that is not finite. The text is the one
/// the sum construction gives for a basepoint it does not admit.
const SCALED_SUM_BASEPOINT_REFUSAL: cadmpeg_ir::geometry::ProceduralGeometryError =
    cadmpeg_ir::geometry::ProceduralGeometryError::Payload("sum basepoint must be finite");

trait ScaleProceduralLengths {
    fn scale_lengths(
        &mut self,
        scale: PositiveReal,
    ) -> Result<(), cadmpeg_ir::geometry::ProceduralGeometryError>;
}

impl ScaleProceduralLengths for cadmpeg_ir::geometry::ProceduralSurfaceDefinition {
    fn scale_lengths(
        &mut self,
        scale: PositiveReal,
    ) -> Result<(), cadmpeg_ir::geometry::ProceduralGeometryError> {
        use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

        match self {
            ProceduralSurfaceDefinition::Extrusion(payload) => {
                let direction = FiniteVector3::new(payload.direction().scale(scale.get()))
                    .ok_or(SCALED_EXTRUSION_DIRECTION_REFUSAL)?;
                let native_position = payload
                    .native_position()
                    .map(|position| {
                        position
                            .scaled(scale)
                            .ok_or(SCALED_EXTRUSION_POSITION_REFUSAL)
                    })
                    .transpose()?;
                payload.set_direction(direction);
                payload.set_native_position(native_position);
            }
            ProceduralSurfaceDefinition::LinearSweep(payload) => {
                let mut direction = payload.direction().get();
                scale_vector3(&mut direction, scale);
                let direction = cadmpeg_ir::units::DirectionAboveEpsilon::new(direction).ok_or(
                    cadmpeg_ir::geometry::ProceduralGeometryError::Payload(
                        "invalid linear-sweep direction",
                    ),
                )?;
                payload.set_direction(direction);
            }
            ProceduralSurfaceDefinition::Revolution(payload) => {
                let mut axis_origin = payload.axis_origin().get();
                scale_point3(&mut axis_origin, scale);
                payload.set_axis_origin(
                    cadmpeg_ir::features::FinitePoint3::new(axis_origin)
                        .ok_or(SCALED_REVOLUTION_AXIS_REFUSAL)?,
                );
            }
            ProceduralSurfaceDefinition::AxisRevolution(payload) => {
                let mut axis_origin = payload.axis_origin().get();
                scale_point3(&mut axis_origin, scale);
                payload.set_axis_origin(
                    cadmpeg_ir::features::FinitePoint3::new(axis_origin)
                        .ok_or(SCALED_REVOLUTION_AXIS_REFUSAL)?,
                );
            }
            ProceduralSurfaceDefinition::Sum(payload) => {
                payload.set_basepoint(
                    FiniteVector3::new(payload.basepoint().scale(scale.get()))
                        .ok_or(SCALED_SUM_BASEPOINT_REFUSAL)?,
                );
            }
            _ => {}
        }
        Ok(())
    }
}

impl ScaleProceduralLengths for cadmpeg_ir::geometry::ProceduralCurveDefinition {
    fn scale_lengths(
        &mut self,
        scale: PositiveReal,
    ) -> Result<(), cadmpeg_ir::geometry::ProceduralGeometryError> {
        if let Self::Helix(helix) = self {
            helix
                .try_scale_lengths(scale.get())
                .map_err(cadmpeg_ir::geometry::ProceduralGeometryError::Payload)?;
        }
        Ok(())
    }
}

pub(in crate::decode) fn scale_procedural_surface(
    ctx: &DecodeContext<'_>,
    procedural: &mut cadmpeg_ir::geometry::ProceduralSurface,
    scale: PositiveReal,
) -> Result<(), CodecError> {
    procedural
        .edit_definition(|definition| definition.scale_lengths(scale))
        .map_err(|error| not_implemented_refusal(ctx, error))?;
    procedural
        .scale_cache_fit_tolerance(scale)
        .map_err(|error| not_implemented_refusal(ctx, error))?;
    Ok(())
}

pub(in crate::decode) fn scale_procedural_curve(
    ctx: &DecodeContext<'_>,
    procedural: &mut cadmpeg_ir::geometry::ProceduralCurve,
    scale: PositiveReal,
) -> Result<(), CodecError> {
    procedural
        .edit_definition(|definition| definition.scale_lengths(scale))
        .map_err(|error| not_implemented_refusal(ctx, error))?;
    procedural
        .scale_cache_fit_tolerance(scale)
        .map_err(|error| not_implemented_refusal(ctx, error))?;
    Ok(())
}

/// The scale of a curve's parameter under the unit scaling. A line is
/// parameterized by length. A conic's parameter is dimensionless, so the
/// scaling keeps it, and the other carriers state no parameter scale.
pub(in crate::decode) fn curve_parameter_scale(
    ctx: &DecodeContext<'_>,
    geometry: &SolvedCurveGeometry,
    length_scale_mm: PositiveReal,
) -> Result<Option<PositiveReal>, CodecError> {
    ctx.charge_work(1, "creo curve parameter scale work")?;
    Ok(match geometry {
        SolvedCurveGeometry::Line(_) => Some(length_scale_mm),
        SolvedCurveGeometry::Transformed(placed) => {
            let _depth = ctx.enter_nested("creo curve parameter scale nesting")?;
            curve_parameter_scale(ctx, placed.basis(), length_scale_mm)?
        }
        SolvedCurveGeometry::Circle(_)
        | SolvedCurveGeometry::Ellipse(_)
        | SolvedCurveGeometry::Parabola(_)
        | SolvedCurveGeometry::Hyperbola(_)
        | SolvedCurveGeometry::Nurbs { .. }
        | SolvedCurveGeometry::Degenerate(_)
        | SolvedCurveGeometry::Composite { .. }
        | SolvedCurveGeometry::Polyline(_)
        | SolvedCurveGeometry::Unknown { .. } => None,
    })
}

pub(in crate::decode) fn surface_parameter_scales(
    ctx: &DecodeContext<'_>,
    geometry: &SolvedSurfaceGeometry,
    length_scale_mm: f64,
) -> Result<[f64; 2], CodecError> {
    ctx.charge_work(1, "creo surface parameter scale work")?;
    Ok(match geometry {
        SolvedSurfaceGeometry::Plane(_) => [length_scale_mm, length_scale_mm],
        SolvedSurfaceGeometry::Cylinder(_) => [1.0, length_scale_mm],
        SolvedSurfaceGeometry::Cone(_) => [1.0, length_scale_mm],
        SolvedSurfaceGeometry::Sphere(_) => [1.0, 1.0],
        SolvedSurfaceGeometry::Torus(_) => [1.0, 1.0],
        SolvedSurfaceGeometry::Transformed(placed) => {
            let _depth = ctx.enter_nested("creo surface parameter scale nesting")?;
            surface_parameter_scales(ctx, placed.basis(), length_scale_mm)?
        }
        SolvedSurfaceGeometry::Nurbs { .. }
        | SolvedSurfaceGeometry::Polygonal(_)
        | SolvedSurfaceGeometry::Unknown { .. } => [1.0, 1.0],
    })
}

pub(in crate::decode) fn scale_sketch_geometry(
    ctx: &DecodeContext<'_>,
    geometry: SketchGeometry,
    scale: PositiveReal,
) -> Result<SketchGeometry, CodecError> {
    use cadmpeg_ir::sketches::scaling::SketchLengthScaleError;

    geometry
        .scaled_lengths_owned_admitted(ctx, scale)?
        .map_err(|error| match error {
            SketchLengthScaleError::LengthOverflow => {
                malformed_refusal(ctx, "Creo scaled length must be finite")
            }
            SketchLengthScaleError::Field(message) => malformed_refusal(ctx, message),
            SketchLengthScaleError::CurveControlPoints(error) => malformed_refusal(
                ctx,
                format_args!(
                    "Creo sketch unit normalization produced invalid NURBS control points: {error}"
                ),
            ),
            SketchLengthScaleError::SurfaceControlPoints(error) => malformed_refusal(
                ctx,
                format_args!(
            "Creo sketch unit normalization produced invalid B-spline control points: {error}"
        ),
            ),
        })
}

#[cfg(test)]
mod tests {

    #[test]
    fn numerical_followup_parabola_parameter_bounds_scale_as_lengths() {
        use cadmpeg_ir::sketches::{SketchGeometry, SketchGeometryDefinition};
        let geometry = SketchGeometry::try_from(SketchGeometryDefinition::Parabola {
            vertex: Point2::new(0., 0.),
            axis_angle: cadmpeg_ir::scalar::Angle::new(0.)
                .expect("valid finite regression fixture"),
            focal_length: Length::new(2.).expect("valid finite regression fixture"),
            bounds: Some([1., 2.]),
        })
        .expect("valid finite regression fixture");
        let geometry = crate::decode::with_test_decode_ctx(|ctx| {
            super::scale_sketch_geometry(ctx, geometry, positive(10.))
        })
        .expect("valid finite regression fixture");
        assert!(
            matches!(geometry.definition(),SketchGeometryDefinition::Parabola{bounds:Some(bounds),focal_length,..} if cadmpeg_ir::scalar::FiniteReal::raw_array(*bounds) == [10., 20.] && focal_length.get()==20.)
        );
    }

    use super::{
        scale_curve_geometry, scale_face_motion, scale_feature_definition, scale_pattern_kind,
        scale_surface_geometry,
    };
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::features::ParameterValue;
    use cadmpeg_ir::geometry::pcurve::PcurveGeometry;
    use cadmpeg_ir::geometry::{
        CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
    };
    use cadmpeg_ir::math::{Point2, Point3, Vector3};
    use cadmpeg_ir::scalar::Length;
    use std::collections::BTreeMap;

    const EPS_UNIT_SCALE: f64 = f64::EPSILON * 4096.0;

    fn positive(scale: f64) -> cadmpeg_ir::scalar::PositiveReal {
        cadmpeg_ir::scalar::PositiveReal::new(scale).expect("a positive scale fixture")
    }

    use cadmpeg_ir::features::{
        patterns::{PatternKind, PatternScaleCenter, PatternTransform},
        BooleanOp, ExtrudeDirection, ExtrudeExtent, ExtrudeSide, ExtrudeStart, FaceMotion, Feature,
        FeatureDefinition, FeatureOperation, FuzzyTolerance, LinearTermination, PlanarProfileRef,
        ProfileRef,
    };

    #[test]
    fn scaled_feature_polyline_refuses_point_collection_limit() {
        let definition = || {
            FeatureDefinition::Operation(FeatureOperation::Polyline {
                chain: cadmpeg_ir::features::FeaturePolyline::new(
                    vec![Point3::new(1.0, 2.0, 3.0), Point3::new(4.0, 5.0, 6.0)],
                    false,
                )
                .expect("two distinct source points"),
            })
        };
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = 1;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root admitted");
        let mut limited = definition();
        let error = scale_feature_definition(&ctx, &mut limited, positive(25.4))
            .expect_err("two scaled points exceed one collection item");
        assert!(
            matches!(error, CodecError::ResourceLimit(resource)
            if resource.operation == "creo scaled feature polyline points"),
            "{error:?}"
        );

        let mut scaled = definition();
        crate::decode::with_test_decode_ctx(|ctx| {
            scale_feature_definition(ctx, &mut scaled, positive(25.4))
        })
        .expect("service-profile scaling");
        let FeatureDefinition::Operation(FeatureOperation::Polyline { chain }) = scaled else {
            panic!("scaled feature changed family");
        };
        assert_point3(chain.points()[0].get(), [25.4, 50.8, 76.2]);
        assert_point3(chain.points()[1].get(), [101.6, 127.0, 152.4]);
    }

    #[test]
    fn feature_and_parameter_lengths_are_in_millimeters_at_admission() {
        let mut ir = CadIr::empty();
        let carriers =
            crate::decode::source_carriers::SourceUnitCarriers::new(Some(positive(25.4)));
        crate::decode::with_test_decode_ctx(|ctx| {
            carriers.admit_feature(
                ctx,
                &mut ir,
                Feature {
                    id: cadmpeg_ir::features::FeatureId::mint("synthetic:test:id#feature")
                        .expect("identity grammar"),
                    ordinal: 0,
                    name: None,
                    suppressed: None,
                    dependencies: cadmpeg_ir::features::DistinctMembers::default(),
                    source_properties: std::collections::BTreeMap::default(),
                    source_tag: None,
                    source_text: None,
                    source_content: cadmpeg_ir::features::FeatureContent::default(),
                    evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                        FeatureDefinition::Operation(FeatureOperation::Extrude {
                            profile: ProfileRef::Planar(PlanarProfileRef::Unresolved(
                                "profile".into(),
                            )),
                            direction: ExtrudeDirection::ProfileNormal {},
                            start: ExtrudeStart::OffsetProfilePlane {
                                offset: Length::new(2.0).expect("finite length fixture"),
                            },
                            extent: ExtrudeExtent::TwoSided {
                                first: ExtrudeSide {
                                    termination: LinearTermination::Blind {
                                        length: cadmpeg_ir::scalar::NonZeroLength::new(3.0)
                                            .expect("nonzero length fixture"),
                                    },
                                    draft: None,
                                },
                                second: ExtrudeSide {
                                    termination: LinearTermination::ToFace {
                                        face: cadmpeg_ir::features::FaceSelection::Native(
                                            "face".into(),
                                        ),
                                        offset: Some(
                                            Length::new(4.0).expect("finite length fixture"),
                                        ),
                                    },
                                    draft: None,
                                },
                            },
                            op: BooleanOp::NewBody,
                            solid: None,
                            face_maker: None,
                            inner_wire_taper: None,
                            length_along_profile_normal: None,
                            allow_multi_profile_faces: None,
                        }),
                    ),
                    native_ref: None,
                },
            )
        })
        .expect("feature admission");
        crate::decode::with_test_decode_ctx(|ctx| {
            carriers.admit_parameter(
                ctx,
                &mut ir,
                cadmpeg_ir::features::DesignParameter {
                    id: cadmpeg_ir::features::ParameterId::mint("synthetic:test:id#length")
                        .expect("identity grammar"),
                    owner: None,
                    ordinal: 0,
                    name: "length".into(),
                    expression: "2".into(),
                    display: None,
                    value: Some(ParameterValue::Length(
                        Length::new(5.0).expect("finite length fixture"),
                    )),
                    dependencies: cadmpeg_ir::features::DistinctMembers::default(),
                    properties: BTreeMap::new(),
                    pmi: None,
                    native_ref: None,
                },
            )
        })
        .expect("parameter admission");
        let FeatureDefinition::Operation(FeatureOperation::Extrude {
            start: ExtrudeStart::OffsetProfilePlane { offset },
            ..
        }) = ir.model.features[0].evaluation.definition()
        else {
            panic!("admitted feature changed family");
        };
        assert_close(offset.get(), 50.8);
        let Some(ParameterValue::Length(length)) = ir.model.parameters[0].value.as_ref() else {
            panic!("admitted parameter changed family");
        };
        assert_close(length.get(), 127.0);

        let FeatureDefinition::Operation(FeatureOperation::Extrude { start, extent, .. }) =
            ir.model.features[0].evaluation.definition()
        else {
            panic!("test feature changed family");
        };
        let ExtrudeStart::OffsetProfilePlane { offset } = start else {
            panic!("test start changed family");
        };
        assert_close(offset.get(), 50.8);
        let ExtrudeExtent::TwoSided { first, second } = extent else {
            panic!("test extent changed family");
        };
        let LinearTermination::Blind { length } = &first.termination else {
            panic!("test termination changed family");
        };
        assert_close(length.get(), 76.2);
        let LinearTermination::ToFace {
            offset: Some(offset),
            ..
        } = &second.termination
        else {
            panic!("test offset termination changed family");
        };
        assert_close(offset.get(), 101.6);
        let Some(ParameterValue::Length(length)) = ir.model.parameters[0].value.as_ref() else {
            panic!("test parameter changed family");
        };
        assert_close(length.get(), 127.0);
    }

    fn model_point_ir(position: Point3) -> Result<CadIr, CodecError> {
        let mut ir = CadIr::empty();
        crate::decode::with_test_decode_ctx(|ctx| {
            crate::decode::source_carriers::SourceUnitCarriers::new(Some(positive(25.4)))
                .admit_point(
                    ctx,
                    &mut ir,
                    cadmpeg_ir::topology::Point::new(
                        cadmpeg_ir::ids::PointId::mint("test:model:entity#point")
                            .expect("identity grammar"),
                        cadmpeg_ir::features::FinitePoint3::new(position)
                            .expect("finite point fixture"),
                        None,
                    ),
                )
        })?;
        Ok(ir)
    }

    #[test]
    fn model_points_of_an_inch_model_are_converted_to_millimetres() {
        let ir =
            model_point_ir(Point3::new(1.0, -2.0, 0.5)).expect("valid millimeter point admission");
        assert_point3(ir.model.points[0].position().get(), [25.4, -50.8, 12.7]);
    }

    #[test]
    fn a_model_point_that_overflows_in_millimetres_is_refused() {
        let error = model_point_ir(Point3::new(0.0, f64::MAX, 0.0))
            .expect_err("an overflowing point has no position")
            .to_string();
        assert!(
            error.contains("scaled model point must be finite"),
            "{error}"
        );
    }

    #[test]
    fn rejects_nurbs_unit_overflow_without_committing_nonfinite_poles() {
        let curve_id = cadmpeg_ir::ids::CurveId::mint("test:model:entity#overflow-curve")
            .expect("identity grammar");
        let curve = cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point3::new(f64::MAX, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
            None,
            false,
        )
        .expect("finite NURBS fixture");
        let mut ir = CadIr::empty();
        let error = crate::decode::with_test_decode_ctx(|ctx| {
            crate::decode::source_carriers::SourceUnitCarriers::new(Some(positive(25.4)))
                .admit_curve(
                    ctx,
                    &mut ir,
                    cadmpeg_ir::geometry::Curve {
                        id: curve_id,
                        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)),
                        source_object: None,
                    },
                )
        })
        .expect_err("overflow must refuse");
        assert!(matches!(error, CodecError::NotImplemented(_)));
        assert!(ir.model.curves.is_empty());
    }

    #[test]
    fn scales_face_motion_lengths_and_origins() {
        let mut translate = FaceMotion::Translate {
            direction: cadmpeg_ir::features::FeatureDirection3::new(
                cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("valid direction fixture"),
            distance: Length::new(2.0).expect("finite length fixture"),
        };
        crate::decode::with_test_decode_ctx(|ctx| {
            scale_face_motion(ctx, &mut translate, positive(25.4))
        })
        .expect("valid test fixture");
        let FaceMotion::Translate {
            direction,
            distance,
        } = translate
        else {
            panic!("test motion changed family");
        };
        assert_eq!(direction, cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0));
        assert_close(distance.get(), 50.8);

        let mut rotate = FaceMotion::Rotate {
            axis_origin: cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 2.0, 3.0))
                .expect("finite point fixture"),
            axis_dir: cadmpeg_ir::features::FeatureDirection3::new(cadmpeg_ir::math::Vector3::new(
                0.0, 0.0, 1.0,
            ))
            .expect("valid direction fixture"),
            angle: cadmpeg_ir::scalar::Angle::new(0.5).expect("finite angle fixture"),
        };
        crate::decode::with_test_decode_ctx(|ctx| {
            scale_face_motion(ctx, &mut rotate, positive(25.4))
        })
        .expect("valid test fixture");
        let FaceMotion::Rotate {
            axis_origin,
            axis_dir,
            angle,
        } = rotate
        else {
            panic!("test motion changed family");
        };
        assert_point3(axis_origin.get(), [25.4, 50.8, 76.2]);
        assert_eq!(axis_dir, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0));
        assert_close(angle.get(), 0.5);
    }

    #[test]
    fn scales_explicit_pattern_scale_center() {
        let mut pattern = PatternKind::<cadmpeg_ir::features::patterns::CompositePattern>::new(
            PatternTransform::Scale {
                center: PatternScaleCenter::Point(
                    cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 2.0, 3.0))
                        .expect("finite point fixture"),
                ),
                final_factor: cadmpeg_ir::scalar::PositiveReal::new(2.0)
                    .expect("positive factor fixture"),
                count: 3,
            },
        )
        .expect("valid test fixture");
        crate::decode::with_test_decode_ctx(|ctx| {
            scale_pattern_kind(ctx, &mut pattern, positive(25.4))
        })
        .expect("valid test fixture");
        let PatternTransform::Scale {
            center,
            final_factor,
            count,
        } = pattern.definition().clone()
        else {
            panic!("test pattern changed family");
        };
        let PatternScaleCenter::Point(point) = center else {
            panic!("test pattern center changed family");
        };
        assert_point3(point.get(), [25.4, 50.8, 76.2]);
        assert_close(final_factor.get(), 2.0);
        assert_eq!(count, 3);
    }

    #[test]
    fn scales_explicit_fuzzy_tolerance() {
        let mut definition = FeatureDefinition::PostProcess {
            operation: FeatureOperation::Native {
                kind: "Boolean".into(),
                parameters: BTreeMap::new(),
            },
            refine: false,
            fuzzy_tolerance: FuzzyTolerance::Explicit(
                cadmpeg_ir::scalar::PositiveLength::new(2.0).expect("positive length fixture"),
            ),
        };

        crate::decode::with_test_decode_ctx(|ctx| {
            scale_feature_definition(ctx, &mut definition, positive(25.4))
        })
        .expect("valid test fixture");

        let FeatureDefinition::PostProcess {
            fuzzy_tolerance, ..
        } = definition
        else {
            panic!("test definition changed family");
        };
        let FuzzyTolerance::Explicit(value) = fuzzy_tolerance else {
            panic!("test tolerance changed family");
        };
        assert_close(value.get(), 50.8);
    }

    #[test]
    // These checked constructors must accept the explicit test fixtures.
    #[allow(clippy::unwrap_used)]
    fn scales_procedural_model_lengths_and_cache_tolerances() {
        let mut ir = CadIr::empty();
        let mut source_carriers =
            crate::decode::source_carriers::SourceUnitCarriers::new(Some(positive(25.4)));
        let surface_id = cadmpeg_ir::ids::SurfaceId::mint("test:model:entity#surface")
            .expect("identity grammar");
        crate::decode::with_test_decode_ctx(|ctx| {
            source_carriers.admit_surface(
                ctx,
                &mut ir,
                cadmpeg_ir::geometry::Surface {
                    id: surface_id.clone(),
                    geometry: cadmpeg_ir::geometry::SurfaceGeometry::Solved(
                        SolvedSurfaceGeometry::Unknown { record: None },
                    ),
                    source_object: None,
                },
            )
        })
        .expect("surface admission");
        let mut surface_definition = cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Extrusion(
            cadmpeg_ir::geometry::surface_payloads::ExtrusionSurfaceConstruction::try_new(
                cadmpeg_ir::ids::CurveId::mint("test:model:entity#directrix")
                    .expect("identity grammar"),
                Some([1.0, 2.0]),
                Vector3::new(1.0, 2.0, 3.0),
                Some(Point3::new(4.0, 5.0, 6.0)),
                cadmpeg_ir::geometry::CacheContract::from_form(None),
            )
            .unwrap(),
        );
        surface_definition
            .set_legacy_cache(Some(
                cadmpeg_ir::geometry::LegacyCache::try_new(7.0).unwrap(),
            ))
            .unwrap();
        let surface = cadmpeg_ir::geometry::ProceduralSurface::new(
            cadmpeg_ir::ids::ProceduralSurfaceId::mint("test:model:entity#surface-construction")
                .expect("identity grammar"),
            surface_definition,
            Some(
                cadmpeg_ir::geometry::RecordBounds::try_new([Some(8.0), None, Some(9.0), None])
                    .unwrap(),
            ),
        );
        crate::decode::with_test_decode_ctx(|ctx| {
            source_carriers.admit_procedural_surface(ctx, &mut ir, &surface_id, surface)
        })
        .expect("surface construction admission");
        let curve_id =
            cadmpeg_ir::ids::CurveId::mint("test:model:entity#curve").expect("identity grammar");
        crate::decode::with_test_decode_ctx(|ctx| {
            source_carriers.admit_curve(
                ctx,
                &mut ir,
                cadmpeg_ir::geometry::Curve {
                    id: curve_id.clone(),
                    geometry: cadmpeg_ir::geometry::CurveGeometry::Solved(
                        SolvedCurveGeometry::Unknown { record: None },
                    ),
                    source_object: None,
                },
            )
        })
        .expect("curve admission");
        let mut curve_definition = cadmpeg_ir::geometry::ProceduralCurveDefinition::Helix(
            cadmpeg_ir::geometry::HelixCurveConstruction::try_new(
                [0.0, 1.0],
                cadmpeg_ir::geometry::HelixFrame {
                    center: Point3::new(1.0, 2.0, 3.0),
                    major: Vector3::new(4.0, 5.0, 6.0),
                    minor: Vector3::new(-5.0, 4.0, 6.0),
                    pitch: Vector3::new(10.0, 11.0, 12.0),
                    axis: Vector3::new(0.0, 0.0, 1.0),
                },
                0.25,
                None,
            )
            .unwrap(),
        );
        curve_definition
            .set_legacy_cache(cadmpeg_ir::geometry::LegacyCache::try_new(13.0).unwrap())
            .unwrap();
        let curve = cadmpeg_ir::geometry::ProceduralCurve::new(
            cadmpeg_ir::ids::ProceduralCurveId::mint("test:model:entity#curve-construction")
                .expect("identity grammar"),
            curve_definition,
        );
        crate::decode::with_test_decode_ctx(|ctx| {
            source_carriers.admit_procedural_curve(ctx, &mut ir, &curve_id, curve)
        })
        .expect("curve construction admission");

        let surface = &ir.model.procedural_surfaces[0];
        let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Extrusion(definition_payload) =
            surface.definition()
        else {
            panic!("test surface construction changed family");
        };
        let direction = definition_payload.direction();
        let native_position = definition_payload.native_position();
        let parameter_interval = definition_payload.parameter_interval();
        assert_vector3(direction.get(), [25.4, 50.8, 76.2]);
        assert_point3(
            native_position
                .as_ref()
                .expect("test native position")
                .get(),
            [101.6, 127.0, 152.4],
        );
        assert_eq!(
            parameter_interval.map(cadmpeg_ir::units::FiniteVector::get),
            Some([1.0, 2.0])
        );
        assert_close(
            surface
                .cache_fit_tolerance()
                .expect("test surface tolerance")
                .get(),
            177.8,
        );
        assert_eq!(
            surface
                .record_bounds()
                .map(cadmpeg_ir::geometry::RecordBounds::get),
            Some([Some(8.0), None, Some(9.0), None])
        );

        let curve = &ir.model.procedural_curves[0];
        let cadmpeg_ir::geometry::ProceduralCurveDefinition::Helix(helix_payload) =
            curve.definition()
        else {
            panic!("test curve construction changed family");
        };
        let center = helix_payload.center().as_raw();
        let major = helix_payload.major();
        let minor = helix_payload.minor();
        let pitch = helix_payload.pitch();
        let apex_factor = helix_payload.apex_factor();
        let axis = helix_payload.axis();

        assert_point3(*center, [25.4, 50.8, 76.2]);
        assert_vector3(major.get(), [101.6, 127.0, 152.4]);
        assert_vector3(minor.get(), [-127.0, 101.6, 152.4]);
        assert_vector3(pitch.get(), [254.0, 279.4, 304.8]);
        assert_eq!(*axis, Vector3::new(0.0, 0.0, 1.0));
        assert_close(apex_factor.get(), 0.25);
        assert_close(
            curve
                .cache_fit_tolerance()
                .expect("test curve tolerance")
                .get(),
            330.2,
        );
    }

    /// The length scale is any finite positive value the file states, so a
    /// finite extrusion direction or sum basepoint can overflow; the rebuilt
    /// payload refuses it with the construction's own text.
    #[test]
    // These checked constructors must accept the explicit test fixtures.
    #[allow(clippy::unwrap_used)]
    fn scaled_procedural_vectors_that_overflow_are_refused() {
        let directrix = cadmpeg_ir::ids::CurveId::mint("test:model:entity#directrix")
            .expect("identity grammar");
        let payloads = [
            (
                cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Extrusion(
                    cadmpeg_ir::geometry::surface_payloads::ExtrusionSurfaceConstruction::try_new(
                        directrix.clone(),
                        None,
                        Vector3::new(f64::MAX, 0.0, 0.0),
                        None,
                        cadmpeg_ir::geometry::CacheContract::from_form(None),
                    )
                    .unwrap(),
                ),
                "Extrusion.direction is not finite",
            ),
            (
                cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Extrusion(
                    cadmpeg_ir::geometry::surface_payloads::ExtrusionSurfaceConstruction::try_new(
                        directrix.clone(),
                        None,
                        Vector3::new(1.0, 0.0, 0.0),
                        Some(Point3::new(0.0, f64::MAX, 0.0)),
                        cadmpeg_ir::geometry::CacheContract::from_form(None),
                    )
                    .unwrap(),
                ),
                "Extrusion.native_position is not finite",
            ),
            (
                cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Sum(
                    cadmpeg_ir::geometry::surface_payloads::SumSurfaceConstruction::try_new(
                        directrix.clone(),
                        directrix,
                        Vector3::new(0.0, 0.0, -f64::MAX),
                        cadmpeg_ir::geometry::CacheContract::from_form(None),
                    )
                    .unwrap(),
                ),
                "sum basepoint must be finite",
            ),
        ];
        for (definition, refusal) in payloads {
            let mut ir = CadIr::empty();
            let mut source_carriers =
                crate::decode::source_carriers::SourceUnitCarriers::new(Some(positive(25.4)));
            let surface_id = cadmpeg_ir::ids::SurfaceId::mint("test:model:entity#surface")
                .expect("identity grammar");
            ir.model.surfaces.push(cadmpeg_ir::geometry::Surface {
                id: surface_id.clone(),
                geometry: cadmpeg_ir::geometry::SurfaceGeometry::Solved(
                    SolvedSurfaceGeometry::Unknown { record: None },
                ),
                source_object: None,
            });
            let surface = cadmpeg_ir::geometry::ProceduralSurface::new(
                cadmpeg_ir::ids::ProceduralSurfaceId::mint(
                    "test:model:entity#surface-construction",
                )
                .expect("identity grammar"),
                definition,
                None,
            );
            let error = crate::decode::with_test_decode_ctx(|ctx| {
                source_carriers.admit_procedural_surface(ctx, &mut ir, &surface_id, surface)
            })
            .expect_err("an overflowing scaled vector has no payload")
            .to_string();
            assert!(error.contains(refusal), "{error}");
        }
    }

    #[test]
    fn scales_pcurve_coordinates_per_surface_axis() {
        let mut geometry = PcurveGeometry::Line(
            cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                Point2::new(1.0, 2.0),
                Point2::new(3.0, 4.0),
            )
            .expect("valid LinePcurve fixture"),
        );

        assert!(geometry.try_scale_coordinates([25.4, 1.0]).is_ok());
        let PcurveGeometry::Line(line_pcurve) = geometry else {
            panic!("test pcurve changed family");
        };
        let origin = line_pcurve.origin().as_raw();
        let direction = line_pcurve.direction().as_raw();
        assert_point2(*origin, [25.4, 2.0]);
        assert_point2(*direction, [76.2, 4.0]);
    }

    #[test]
    fn scales_analytic_surface_and_curve_without_scaling_directions() {
        let mut surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
            cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                Point3::new(1.0, 2.0, 3.0),
                cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
                4.0,
            )
            .expect("valid CylinderSurface fixture"),
        ));
        let mut curve = CurveGeometry::Solved(SolvedCurveGeometry::Circle(
            cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                Point3::new(2.0, 3.0, 4.0),
                cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
                5.0,
            )
            .expect("valid CircleCurve fixture"),
        ));

        let SurfaceGeometry::Solved(surface_solved) = &mut surface else {
            panic!("test surface changed family");
        };
        crate::decode::with_test_decode_ctx(|ctx| {
            scale_surface_geometry(ctx, surface_solved, positive(25.4))
        })
        .expect("finite surface scaling");
        let CurveGeometry::Solved(curve_solved) = &mut curve else {
            panic!("test curve changed family");
        };
        crate::decode::with_test_decode_ctx(|ctx| {
            scale_curve_geometry(ctx, curve_solved, positive(25.4))
        })
        .expect("finite curve scaling");

        let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) = surface
        else {
            panic!("test surface changed family");
        };
        let origin = cylinder_surface.origin().get();
        let axis = cylinder_surface.frame().axis().as_raw();
        let radius = cylinder_surface.radius().get();
        assert_point3(origin, [25.4, 50.8, 76.2]);
        assert_eq!(*axis, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0));
        assert_close(radius, 101.6);
        let CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) = curve else {
            panic!("test curve changed family");
        };
        let center = circle_curve.center().get();
        let axis = circle_curve.frame().axis().as_raw();
        let radius = circle_curve.radius().get();
        assert_point3(center, [50.8, 76.2, 101.6]);
        assert_eq!(*axis, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0));
        assert_close(radius, 127.0);
    }

    fn assert_close(actual: f64, expected: f64) {
        assert!((actual - expected).abs() <= EPS_UNIT_SCALE);
    }

    fn assert_point2(actual: Point2, expected: [f64; 2]) {
        assert_close(actual.u, expected[0]);
        assert_close(actual.v, expected[1]);
    }

    fn assert_point3(actual: Point3, expected: [f64; 3]) {
        assert_close(actual.x, expected[0]);
        assert_close(actual.y, expected[1]);
        assert_close(actual.z, expected[2]);
    }

    fn assert_vector3(actual: Vector3, expected: [f64; 3]) {
        assert_close(actual.x, expected[0]);
        assert_close(actual.y, expected[1]);
        assert_close(actual.z, expected[2]);
    }

    fn positive_length(value: f64) -> cadmpeg_ir::scalar::PositiveLength {
        cadmpeg_ir::scalar::PositiveLength::new(value).expect("a positive length fixture")
    }

    /// 1.9 and its successor, which both scale to 48.26 at the inch scale.
    fn collapsing_pair() -> [f64; 2] {
        let low = 1.9;
        [low, f64::from_bits(low.to_bits() + 1)]
    }

    fn elliptic_arc(center: Point3, radii: [f64; 2]) -> FeatureDefinition {
        FeatureDefinition::Operation(FeatureOperation::EllipticArc {
            arc: cadmpeg_ir::features::FeatureEllipticArc::new(
                center,
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                radii.map(positive_length),
                cadmpeg_ir::geometry::DirectedParameterRange::new([0.0, 1.0])
                    .expect("a nonzero interval"),
            )
            .expect("an ordered arc fixture"),
        })
    }

    /// The radius order of an elliptic arc is not strict, so radii that
    /// round to one value stay admitted; a radius is refused before the
    /// center, each with its own text.
    #[test]
    fn an_elliptic_arc_keeps_radii_that_round_to_one_value() {
        let [low, high] = collapsing_pair();
        let mut definition = elliptic_arc(Point3::new(1.0, 2.0, 3.0), [high, low]);
        crate::decode::with_test_decode_ctx(|ctx| {
            scale_feature_definition(ctx, &mut definition, positive(25.4))
        })
        .expect("ordered radii");
        let FeatureDefinition::Operation(FeatureOperation::EllipticArc { arc }) = definition else {
            panic!("test feature changed family");
        };
        assert_eq!(
            arc.radii().map(cadmpeg_ir::scalar::PositiveLength::get),
            [48.26, 48.26]
        );
        assert_point3(arc.center().get(), [25.4, 50.8, 76.2]);

        let far = Point3::new(f64::MAX, 0.0, 0.0);
        for (definition, text) in [
            (
                elliptic_arc(far, [f64::MAX, 1.0]),
                "Creo scaled length must be positive and finite",
            ),
            (
                elliptic_arc(far, [2.0, 1.0]),
                "Creo scaled elliptic arc must have finite ordered geometry",
            ),
        ] {
            let mut definition = definition;
            let error = crate::decode::with_test_decode_ctx(|ctx| {
                scale_feature_definition(ctx, &mut definition, positive(25.4))
            })
            .expect_err("an overflowing arc")
            .to_string();
            assert!(error.contains(text), "{error}");
        }
    }

    /// A counterdrill entry diameter exceeds the bore strictly; the two can
    /// round to one value, so the scaled pair is admitted again.
    #[test]
    fn a_counterdrill_whose_diameters_round_to_one_value_is_refused() {
        let [low, high] = collapsing_pair();
        let kind = cadmpeg_ir::features::holes::HoleKind::Counterdrill {
            diameters: cadmpeg_ir::features::holes::CounterdrillDiameters::new(
                positive_length(low),
                Some(positive_length(high)),
            )
            .expect("an entry above the bore"),
            depth: positive_length(1.0),
            angle: cadmpeg_ir::scalar::InteriorAngle::new(1.0).expect("an interior angle"),
        };
        let error = kind
            .try_map_lengths(&mut |value| -> Result<_, cadmpeg_core::CodecError> {
                let mut value = value;
                crate::decode::with_test_decode_ctx(|ctx| {
                    super::scale_positive_length(ctx, &mut value, positive(25.4))
                })?;
                Ok(value)
            })
            .expect_err("the diameters collapse");
        let error = format!("{error:?}");
        assert!(
            error.contains("entry_diameter must exceed diameter"),
            "{error}"
        );
    }

    /// A counterbore diameter exceeds the bore strictly; the two can round to
    /// one value, so the scaled hole shape is admitted again.
    #[test]
    fn a_hole_whose_treatment_rounds_onto_its_bore_is_refused() {
        let [low, high] = collapsing_pair();
        let mut shape = cadmpeg_ir::features::holes::HoleShape::new(
            cadmpeg_ir::features::holes::HoleConstruction::Form {
                kind: cadmpeg_ir::features::holes::HoleKind::Counterbore {
                    diameter: positive_length(high),
                    depth: positive_length(1.0),
                },
                specification: None,
            },
            None,
            Some(positive_length(low)),
        )
        .expect("a counterbore above the bore");
        let error = crate::decode::with_test_decode_ctx(|ctx| {
            super::scale_hole_shape(ctx, &mut shape, positive(25.4))
        })
        .expect_err("the diameters collapse")
        .to_string();
        assert!(
            error.contains("treatment diameters must exceed the bore diameter"),
            "{error}"
        );
    }

    /// A sweep wall is strictly thinner than the outer radius; the two can
    /// round to one value, so the scaled region is admitted again.
    #[test]
    fn a_sweep_wall_that_rounds_onto_its_radius_is_refused() {
        let [low, high] = collapsing_pair();
        let mut section = cadmpeg_ir::features::GeneratedSweepSection::CircularRegion {
            region: cadmpeg_ir::features::SweepCircularRegion::new(
                positive_length(high),
                Some(positive_length(low)),
            )
            .expect("a wall below the radius"),
        };
        let error = crate::decode::with_test_decode_ctx(|ctx| {
            super::scale_sweep_section(ctx, &mut section, positive(25.4))
        })
        .expect_err("the wall collapses")
        .to_string();
        assert!(
            error.contains("wall_thickness must be less than outer_radius"),
            "{error}"
        );
    }

    /// Pattern offsets increase strictly; two can round to one value, so the
    /// scaled offsets are admitted again.
    #[test]
    fn pattern_offsets_that_round_to_one_value_are_refused() {
        let [low, high] = collapsing_pair();
        let mut pattern = PatternKind::<cadmpeg_ir::features::patterns::CompositePattern>::new(
            PatternTransform::LinearOffsets {
                direction: None,
                offsets: [0.0, low, high]
                    .into_iter()
                    .map(|offset| Length::new(offset).expect("a finite offset"))
                    .collect(),
            },
        )
        .expect("strictly increasing offsets");
        let error = crate::decode::with_test_decode_ctx(|ctx| {
            scale_pattern_kind(ctx, &mut pattern, positive(25.4))
        })
        .expect_err("the offsets collapse")
        .to_string();
        assert!(
            error.contains("pattern offsets must start at zero and strictly increase"),
            "{error}"
        );
    }

    /// A wedge extent is strictly ordered; its bounds can round to one value,
    /// so the scaled wedge is admitted again.
    #[test]
    fn a_wedge_whose_extent_rounds_to_one_value_is_refused() {
        let [low, high] = collapsing_pair();
        let length = |value: f64| Length::new(value).expect("a finite length");
        let mut solid = cadmpeg_ir::features::PrimitiveSolid::new(
            cadmpeg_ir::features::PrimitiveSolidKind::Wedge {
                xmin: length(low),
                ymin: length(0.0),
                zmin: length(0.0),
                x2min: length(0.0),
                z2min: length(0.0),
                xmax: length(high),
                ymax: length(1.0),
                zmax: length(1.0),
                x2max: length(0.0),
                z2max: length(0.0),
            },
        )
        .expect("a wedge fixture");
        let error = crate::decode::with_test_decode_ctx(|ctx| {
            super::scale_primitive_solid(ctx, &mut solid, positive(25.4))
        })
        .expect_err("the extent collapses")
        .to_string();
        assert!(
            error.contains("primitive dimensions are invalid"),
            "{error}"
        );
    }
    fn scaling_refusal_below_need(
        run: impl Fn(&cadmpeg_core::decode::DecodeContext<'_>) -> Result<(), CodecError>,
        text: &str,
    ) {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = u64::try_from(text.len() - 1).expect("text need");
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        assert!(matches!(run(&ctx), Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "creo unit normalization refusal text"));
        assert!(
            matches!(crate::decode::with_test_decode_ctx(|ctx| run(ctx)),
            Err(CodecError::Malformed(message)) if message == text)
        );
    }

    #[test]
    fn scalar_scaling_refusals_admit_each_retained_message() {
        scaling_refusal_below_need(
            |ctx| {
                super::scale_length(
                    ctx,
                    &mut Length::new(f64::MAX).expect("length"),
                    positive(2.0),
                )
            },
            "Creo scaled length must be finite",
        );
        scaling_refusal_below_need(
            |ctx| super::scale_positive_length(ctx, &mut positive_length(f64::MAX), positive(2.0)),
            "Creo scaled length must be positive and finite",
        );
        scaling_refusal_below_need(
            |ctx| {
                super::scale_nonzero_length(
                    ctx,
                    &mut cadmpeg_ir::scalar::NonZeroLength::new(f64::MAX).expect("length"),
                    positive(2.0),
                )
            },
            "Creo scaled length must be finite and nonzero",
        );
        scaling_refusal_below_need(
            |ctx| {
                super::scale_nonnegative_length(
                    ctx,
                    &mut cadmpeg_ir::scalar::NonNegativeLength::new(f64::MAX).expect("length"),
                    positive(2.0),
                )
            },
            "Creo scaled length must be nonnegative and finite",
        );
    }

    #[test]
    fn transform_scaling_refusal_admits_formatted_text() {
        let transform = cadmpeg_ir::transform::Transform::affine([
            [1.0, 0.0, 0.0, f64::MAX],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ])
        .expect("transform");
        scaling_refusal_below_need(
            |ctx| {
                let mut candidate = transform;
                super::scale_transform_translation(ctx, &mut candidate, positive(2.0))
            },
            "Creo length scale 2 drives a transform translation the carrier refuses",
        );
    }

    #[test]
    fn sketch_scaling_refusal_admits_retained_text() {
        scaling_refusal_below_need(
            |ctx| {
                super::scale_sketch_geometry(
                    ctx,
                    cadmpeg_ir::sketches::SketchGeometry::try_from(
                        cadmpeg_ir::sketches::SketchGeometryDefinition::Point {
                            position: Point2::new(f64::MAX, 0.0),
                        },
                    )
                    .expect("point"),
                    positive(2.0),
                )
                .map(|_| ())
            },
            "sketch point position must be finite",
        );
    }

    #[test]
    fn owned_pattern_scaling_refuses_offset_work_and_stage_nesting() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let offsets = || {
            PatternKind::<cadmpeg_ir::features::patterns::CompositePattern>::new(
                PatternTransform::LinearOffsets {
                    direction: None,
                    offsets: [0.0, 1.0, 2.0]
                        .map(|x| Length::new(x).expect("length"))
                        .to_vec(),
                },
            )
            .expect("pattern")
        };
        for cap in [2, 4, 6] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            assert!(
                matches!(scale_pattern_kind(&ctx, &mut offsets(), positive(2.0)),
                Err(CodecError::ResourceLimit(resource)) if resource.operation == "creo pattern offset scaling work")
            );
        }
        let composite = || {
            PatternKind::new(PatternTransform::Composite {
                stages: cadmpeg_ir::features::patterns::CompositePattern::new(vec![
                    cadmpeg_ir::features::patterns::PatternStage {
                        pattern: Box::new(
                            cadmpeg_ir::features::patterns::StagePatternKind::UNRESOLVED,
                        ),
                    },
                    cadmpeg_ir::features::patterns::PatternStage {
                        pattern: Box::new(
                            cadmpeg_ir::features::patterns::StagePatternKind::UNRESOLVED,
                        ),
                    },
                ])
                .expect("stages"),
            })
            .expect("composite")
        };
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        assert!(
            matches!(scale_pattern_kind(&ctx, &mut composite(), positive(2.0)),
            Err(CodecError::ResourceLimit(resource)) if resource.operation == "creo pattern scaling nesting")
        );
        for cap in [1, 2] {
            policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            assert!(
                matches!(scale_pattern_kind(&ctx, &mut composite(), positive(2.0)),
                Err(CodecError::ResourceLimit(resource)) if resource.operation == "creo pattern scaling work")
            );
        }
        policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        policy.limits.max_retained_bytes = 0;
        for mut value in [offsets(), composite()] {
            let expected = value
                .try_map_lengths(&mut |field| -> Result<(), CodecError> {
                    if let cadmpeg_ir::features::patterns::PatternLengthField::Length(length) =
                        field
                    {
                        *length = Length::new(length.get() * 2.0).expect("length");
                    }
                    Ok(())
                })
                .expect("reference");
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            scale_pattern_kind(&ctx, &mut value, positive(2.0)).expect("no copies");
            assert_eq!(value, expected);
        }
    }

    #[test]
    fn owned_hole_scaling_preserves_specification_storage() {
        use cadmpeg_ir::features::holes::{
            HoleConstruction, HoleKind, HoleShape, HoleSpecification, HoleThreadDepth, ThreadHand,
        };
        let mut shape = HoleShape::new(
            HoleConstruction::Form {
                kind: HoleKind::Simple,
                specification: Some(Box::new(HoleSpecification::Clearance {
                    standard: cadmpeg_core::text::NonBlankString::new("test-standard".to_owned())
                        .expect("standard"),
                    designation: Some("test-size".to_owned()),
                    fit: Some("test-fit".to_owned()),
                    modeled: false,
                    cosmetic: false,
                    hand: ThreadHand::Right,
                    depth: HoleThreadDepth::Blind {
                        depth: positive_length(3.0),
                    },
                    clearance: Some(Length::new(1.0).expect("length")),
                })),
            },
            None,
            Some(positive_length(2.0)),
        )
        .expect("hole");
        let specification_ptr = match shape.construction() {
            HoleConstruction::Form {
                specification: Some(specification),
                ..
            } => std::ptr::from_ref(specification.as_ref()),
            _ => panic!("fixture"),
        };
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("root");
        super::scale_hole_shape(&ctx, &mut shape, positive(2.0)).expect("no copies");
        let HoleConstruction::Form {
            specification: Some(specification),
            ..
        } = shape.construction()
        else {
            panic!("fixture");
        };
        assert_eq!(
            std::ptr::from_ref(specification.as_ref()),
            specification_ptr
        );
        assert_eq!(shape.diameter().expect("bore").get(), 4.0);
        assert!(
            matches!(specification.as_ref(), HoleSpecification::Clearance {
            standard, designation: Some(designation), fit: Some(fit), depth: HoleThreadDepth::Blind { depth }, clearance: Some(clearance), ..
        } if standard.as_str() == "test-standard" && designation == "test-size" && fit == "test-fit" && depth.get() == 6.0 && clearance.get() == 2.0)
        );
    }

    #[test]
    fn parameter_scale_traversals_refuse_each_recursive_frame() {
        let curve = SolvedCurveGeometry::Transformed(
            cadmpeg_ir::geometry::PlacedCurve::try_new(
                Box::new(SolvedCurveGeometry::Line(
                    cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                        Point3::new(0.0, 0.0, 0.0),
                        Vector3::new(1.0, 0.0, 0.0),
                    )
                    .expect("line"),
                )),
                cadmpeg_ir::transform::Transform::identity(),
            )
            .expect("placed"),
        );
        let surface = SolvedSurfaceGeometry::Transformed(
            cadmpeg_ir::geometry::PlacedSurface::try_new(
                Box::new(SolvedSurfaceGeometry::Plane(
                    cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                        Point3::new(0.0, 0.0, 0.0),
                        Vector3::new(0.0, 0.0, 1.0),
                        Vector3::new(1.0, 0.0, 0.0),
                    )
                    .expect("plane"),
                )),
                cadmpeg_ir::transform::Transform::identity(),
            )
            .expect("placed"),
        );
        for (depth, work, operation) in [(0, u64::MAX, "nesting"), (1, 1, "work")] {
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_recursion_depth = depth;
            policy.limits.max_work_units = work;
            let (ctx, _) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
                    .expect("root");
            let error =
                super::curve_parameter_scale(&ctx, &curve, positive(2.0)).expect_err("limit");
            assert!(
                matches!(error, CodecError::ResourceLimit(resource) if resource.operation == format!("creo curve parameter scale {operation}"))
            );
            let (ctx, _) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
                    .expect("root");
            let error = super::surface_parameter_scales(&ctx, &surface, 2.0).expect_err("limit");
            assert!(
                matches!(error, CodecError::ResourceLimit(resource) if resource.operation == format!("creo surface parameter scale {operation}"))
            );
        }
        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| super::curve_parameter_scale(
                ctx,
                &curve,
                positive(2.0)
            ))
            .expect("service"),
            Some(positive(2.0))
        );
        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| super::surface_parameter_scales(
                ctx, &surface, 2.0
            ))
            .expect("service"),
            [2.0, 2.0]
        );
    }

    fn check_member_work(mut definition: FeatureDefinition, cap: u64) -> FeatureDefinition {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("root");
        assert!(
            matches!(scale_feature_definition(&ctx, &mut definition.clone(), positive(2.0)),
            Err(CodecError::ResourceLimit(resource)) if resource.operation == "creo unit scaling member work")
        );
        crate::decode::with_test_decode_ctx(|ctx| {
            scale_feature_definition(ctx, &mut definition, positive(2.0))
        })
        .expect("service");
        definition
    }

    #[test]
    fn generated_sweep_scaling_refuses_member_work_without_reference_rows() {
        let shape = cadmpeg_ir::features::SweepShape::Solid {
            op: cadmpeg_ir::features::SolidSweepOperation::NewBody,
            section: cadmpeg_ir::features::SweepSection::Unresolved(None),
            sections: vec![cadmpeg_ir::features::SweepSection::Generated(
                cadmpeg_ir::features::GeneratedSweepSection::CircularRegion {
                    region: cadmpeg_ir::features::SweepCircularRegion::new(
                        positive_length(1.0),
                        None,
                    )
                    .expect("region"),
                },
            )],
        };
        let mut definition = check_member_work(
            FeatureDefinition::Operation(FeatureOperation::Sweep {
                shape,
                path: None,
                orientation: None,
                transition: None,
                transformation: None,
                path_tangent: false,
                linearize: false,
                twist: None,
                path_extent: None,
                guide_rail: None,
                taper: None,
                scale: None,
                allow_multi_profile_faces: None,
            }),
            1,
        );
        let FeatureDefinition::Operation(FeatureOperation::Sweep { shape, .. }) = &mut definition
        else {
            panic!("fixture");
        };
        assert!(
            matches!(shape.generated_sections_mut().next(), Some(cadmpeg_ir::features::GeneratedSweepSection::CircularRegion { region }) if region.outer_radius().get() == 2.0)
        );
    }

    #[test]
    fn loft_scaling_refuses_each_member_work() {
        let point =
            cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 0.0, 0.0)).expect("point");
        let result = check_member_work(
            FeatureDefinition::Operation(FeatureOperation::Loft {
                sections: vec![cadmpeg_ir::features::LoftSection::Point(
                    cadmpeg_ir::features::LoftPointSection::Point(point),
                )],
                guidance: cadmpeg_ir::features::LoftGuidance::default(),
                op: cadmpeg_ir::features::BooleanOp::Join,
                closed: false,
                solid: true,
                ruled: false,
                linearize: false,
                max_degree: None,
                allow_multi_profile_faces: None,
            }),
            0,
        );
        assert!(
            matches!(result, FeatureDefinition::Operation(FeatureOperation::Loft { sections, .. })
            if matches!(&sections[0], cadmpeg_ir::features::LoftSection::Point(cadmpeg_ir::features::LoftPointSection::Point(point)) if point.get().x == 2.0))
        );
    }

    #[test]
    fn fillet_and_chamfer_scaling_refuse_each_group_work() {
        use cadmpeg_ir::features::edge_treatments::{
            ChamferGroup, ChamferSpec, FilletGroup, RadiusSpec,
        };
        let fillet = check_member_work(
            FeatureDefinition::Operation(FeatureOperation::Fillet {
                groups: vec![FilletGroup {
                    edges: cadmpeg_ir::features::EdgeSelection::All,
                    radius: RadiusSpec::Constant {
                        radius: positive_length(1.0),
                    },
                    tangency_weight: None,
                }]
                .try_into()
                .expect("group"),
            }),
            0,
        );
        assert!(
            matches!(fillet, FeatureDefinition::Operation(FeatureOperation::Fillet { groups })
            if matches!(groups[0].radius, RadiusSpec::Constant { radius } if radius.get() == 2.0))
        );
        let chamfer = check_member_work(
            FeatureDefinition::Operation(FeatureOperation::Chamfer {
                groups: vec![ChamferGroup {
                    edges: cadmpeg_ir::features::EdgeSelection::All,
                    spec: ChamferSpec::Distance {
                        distance: positive_length(1.0),
                    },
                }]
                .try_into()
                .expect("group"),
                flip_direction: false,
            }),
            0,
        );
        assert!(
            matches!(chamfer, FeatureDefinition::Operation(FeatureOperation::Chamfer { groups, .. })
            if matches!(groups[0].spec, ChamferSpec::Distance { distance } if distance.get() == 2.0))
        );
    }

    #[test]
    fn hole_scaling_refuses_each_placement_work() {
        use cadmpeg_ir::features::holes::{HoleConstruction, HoleKind, HolePlacement, HoleShape};
        let point =
            cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 0.0, 0.0)).expect("point");
        let result = check_member_work(
            FeatureDefinition::Operation(FeatureOperation::Hole {
                profile: None,
                profile_filter: None,
                face: None,
                direction: None,
                placements: Some(vec![HolePlacement::Axis {
                    origin: point,
                    axis: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(0.0, 0.0, 1.0))
                        .expect("axis"),
                }]),
                shape: HoleShape::new(HoleConstruction::form(HoleKind::Simple), None, None)
                    .expect("shape"),
                extent: None,
                bottom: None,
                taper_angle: None,
                allow_multi_profile_faces: None,
            }),
            0,
        );
        assert!(
            matches!(result, FeatureDefinition::Operation(FeatureOperation::Hole { placements: Some(placements), .. })
            if matches!(&placements[0], HolePlacement::Axis { origin, .. } if origin.get().x == 2.0))
        );
    }

    #[test]
    fn feature_polyline_scaling_refuses_each_point_work() {
        let definition = || {
            FeatureDefinition::Operation(FeatureOperation::Polyline {
                chain: cadmpeg_ir::features::FeaturePolyline::new(
                    vec![Point3::new(1.0, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0)],
                    false,
                )
                .expect("chain"),
            })
        };
        for cap in [1, 3] {
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
                    .expect("root");
            assert!(
                matches!(scale_feature_definition(&ctx, &mut definition(), positive(2.0)),
                Err(CodecError::ResourceLimit(resource)) if resource.operation == "creo unit scaling polyline work")
            );
        }
        let mut result = definition();
        crate::decode::with_test_decode_ctx(|ctx| {
            scale_feature_definition(ctx, &mut result, positive(2.0))
        })
        .expect("service");
        assert!(
            matches!(result, FeatureDefinition::Operation(FeatureOperation::Polyline { chain }) if chain.points()[0].get().x == 2.0 && chain.points()[1].get().x == 4.0)
        );
    }
}
