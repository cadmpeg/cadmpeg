// SPDX-License-Identifier: Apache-2.0
//! Conversion of neutral Creo values into the canonical IR length unit.
//!
//! The PSB scanner keeps source values in their stored unit so native records
//! remain faithful to the file.  This module is the single boundary at which
//! the already-built neutral model is converted to millimeters.  Unit
//! directions, angles, ratios, and source-native arenas are intentionally not
//! scaled.

use std::collections::BTreeMap;

use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    FeatureDefinition, FeatureOperation, FiniteVector3, ParameterValue, WrapMode,
};
use cadmpeg_ir::geometry::scaling::ScaleRefusal;
use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::ids::PcurveId;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::scalar::{Length, PositiveReal};
use cadmpeg_ir::sketches::{SketchGeometry, SpatialSketchGeometry};
use cadmpeg_ir::topology::EdgeCarrier;
use cadmpeg_ir::transform::Transform;

/// Scale all neutral model lengths from the source unit into millimeters.
pub(super) fn normalize_model_lengths(
    ir: &mut CadIr,
    scale: PositiveReal,
) -> Result<(), CodecError> {
    if scale.get() == 1.0 {
        return Ok(());
    }

    let pcurve_scales = pcurve_scales(ir, scale.get());
    for pcurve in &mut ir.model.pcurves {
        if let Some(scales) = pcurve_scales.get(&pcurve.id) {
            if pcurve.geometry.try_scale_coordinates(*scales).is_err() {
                return Err(CodecError::NotImplemented(format!(
                    "Creo pcurve cannot be represented after unit normalization with scales {scales:?}"
                )));
            }
        }
    }

    for surface in &mut ir.model.surfaces {
        if let SurfaceGeometry::Solved(geometry) = &mut surface.geometry {
            scale_surface_geometry(geometry, scale)?;
        }
    }
    for curve in &mut ir.model.curves {
        if let CurveGeometry::Solved(geometry) = &mut curve.geometry {
            scale_curve_geometry(geometry, scale)?;
        }
    }
    for procedural in &mut ir.model.procedural_surfaces {
        procedural
            .edit_definition(|definition| definition.scale_lengths(scale))
            .map_err(cadmpeg_core::CodecError::malformed)?;
        procedural
            .scale_cache_fit_tolerance(scale)
            .map_err(cadmpeg_core::CodecError::malformed)?;
    }
    for procedural in &mut ir.model.procedural_curves {
        procedural
            .edit_definition(|definition| definition.scale_lengths(scale))
            .map_err(cadmpeg_core::CodecError::malformed)?;
        procedural
            .scale_cache_fit_tolerance(scale)
            .map_err(cadmpeg_core::CodecError::malformed)?;
    }
    for point in &mut ir.model.points {
        let position = point
            .position()
            .scaled(scale)
            .ok_or_else(|| CodecError::malformed("Creo scaled model point must be finite"))?;
        point.set_position(position);
    }
    for face in &mut ir.model.faces {
        scale_tolerance(&mut face.tolerance, scale)?;
    }
    for vertex in &mut ir.model.vertices {
        scale_tolerance(&mut vertex.tolerance, scale)?;
    }

    let curve_parameter_scales = ir
        .model
        .curves
        .iter()
        .filter_map(|curve| {
            curve_parameter_scale(curve.geometry.solved()?, scale)
                .map(|scale| (curve.id.clone(), scale))
        })
        .collect::<BTreeMap<_, _>>();
    for edge in &mut ir.model.edges {
        scale_tolerance(&mut edge.tolerance, scale)?;
        if let EdgeCarrier::Bounded(curve, interval) = &mut edge.carrier {
            if let Some(scale) = curve_parameter_scales.get(curve) {
                *interval = interval.scaled(*scale).ok_or_else(|| {
                    CodecError::malformed("edge param_range must be finite and ordered")
                })?;
            }
        }
    }
    for coedge in &mut ir.model.coedges {
        if let Some(use_curve) = &mut coedge.use_curve {
            if let Some(scale) = curve_parameter_scales.get(&use_curve.curve) {
                use_curve.parameter_range =
                    use_curve.parameter_range.scaled(*scale).ok_or_else(|| {
                        CodecError::malformed("parameter_range must be finite and ordered")
                    })?;
            }
        }
    }

    for body in &mut ir.model.bodies {
        if let Some(transform) = body.transform.as_mut() {
            scale_transform_translation(transform, scale)?;
        }
    }
    for occurrence in &mut ir.model.occurrences {
        scale_transform_translation(&mut occurrence.transform, scale)?;
        if let Some(transform) = occurrence.linked_prototype.as_mut() {
            scale_transform_translation(transform, scale)?;
        }
    }
    for tessellation in &mut ir.model.tessellations {
        tessellation
            .edit_vertices(|vertex| {
                scale_point3(vertex, scale);
                Ok(())
            })
            .map_err(|error| {
                CodecError::malformed(format_args!("invalid scaled tessellation: {error}"))
            })?;
        tessellation
            .scale_chordal_deflection(scale)
            .map_err(|error| {
                CodecError::malformed(format_args!("invalid scaled tessellation: {error}"))
            })?;
    }
    for feature in &mut ir.model.features {
        let mut definition = feature.evaluation.definition().clone();
        scale_feature_definition(&mut definition, scale)?;
        feature.evaluation.set_definition(definition);
    }

    for parameter in &mut ir.model.parameters {
        if let Some(ParameterValue::Length(length)) = parameter.value.as_mut() {
            scale_length(length, scale)?;
        }
    }
    for configuration in &mut ir.model.configurations {
        for value in configuration.parameter_values.values_mut() {
            if let ParameterValue::Length(length) = value {
                scale_length(length, scale)?;
            }
        }
        for state in configuration.feature_states.values_mut() {
            scale_feature_definition(&mut state.definition, scale)?;
        }
    }
    for sketch in &mut ir.model.sketches {
        if let Some((origin, _, _)) = sketch.resolved_placement() {
            let origin = origin
                .scaled(scale)
                .ok_or_else(|| CodecError::malformed("sketch origin must be finite"))?;
            sketch.placement = sketch.placement.with_origin(origin);
        }
    }
    for entity in &mut ir.model.sketch_entities {
        scale_sketch_geometry(&mut entity.geometry, scale)?;
    }
    for sketch in &mut ir.model.spatial_sketches {
        for profile in &mut sketch.profiles {
            let mut origin = profile.origin().get();
            scale_point3(&mut origin, scale);
            profile.set_origin(origin).map_err(CodecError::malformed)?;
        }
    }
    for entity in &mut ir.model.spatial_sketch_entities {
        scale_spatial_sketch_geometry(&mut entity.geometry, scale)?;
    }
    for constraint in &mut ir.model.sketch_constraints {
        constraint
            .definition
            .scale_lengths(scale)
            .map_err(|error| match error {
                cadmpeg_ir::sketches::scaling::SketchConstraintScaleError::LengthOverflow => {
                    CodecError::Malformed("Creo scaled length must be finite".into())
                }
                cadmpeg_ir::sketches::scaling::SketchConstraintScaleError::InvalidLocalValue => {
                    CodecError::malformed("invalid sketch constraint local arity or scalar value")
                }
            })?;
    }
    for constraint in &mut ir.model.spatial_sketch_constraints {
        constraint
            .definition
            .scale_lengths(scale)
            .map_err(|error| match error {
                cadmpeg_ir::sketches::scaling::SketchConstraintScaleError::LengthOverflow => {
                    CodecError::Malformed("Creo scaled length must be finite".into())
                }
                cadmpeg_ir::sketches::scaling::SketchConstraintScaleError::InvalidLocalValue => {
                    CodecError::malformed(
                        "invalid spatial sketch constraint local arity or scalar value",
                    )
                }
            })?;
    }
    Ok(())
}

fn scale_tolerance(
    value: &mut Option<cadmpeg_ir::scalar::PositiveReal>,
    scale: PositiveReal,
) -> Result<(), CodecError> {
    if let Some(current) = value {
        *current = cadmpeg_ir::scalar::PositiveReal::new(current.get() * scale.get()).ok_or_else(
            || CodecError::malformed("scaled topology tolerance must be positive and finite"),
        )?;
    }
    Ok(())
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
    point: &mut cadmpeg_ir::features::FinitePoint3,
    scale: PositiveReal,
) -> Result<(), CodecError> {
    *point = point
        .scaled(scale)
        .ok_or_else(|| CodecError::Malformed("Creo scaled feature point must be finite".into()))?;
    Ok(())
}

fn scale_vector3(vector: &mut Vector3, scale: PositiveReal) {
    vector.x *= scale.get();
    vector.y *= scale.get();
    vector.z *= scale.get();
}

fn scale_transform_translation(
    transform: &mut Transform,
    scale: PositiveReal,
) -> Result<(), CodecError> {
    // `scale` is the length scale the file states and `transform` comes from
    // the document, so a scale that drives a translation non-finite is a
    // source the transform carrier refuses, not an impossible state.
    *transform = transform.scaled_translation(scale).ok_or_else(|| {
        CodecError::malformed(format_args!(
            "Creo length scale {} drives a transform translation the carrier refuses",
            scale.get()
        ))
    })?;
    Ok(())
}

fn scale_length(length: &mut Length, scale: PositiveReal) -> Result<(), CodecError> {
    *length = Length::new(length.get() * scale.get())
        .ok_or_else(|| CodecError::Malformed("Creo scaled length must be finite".into()))?;
    Ok(())
}

fn scale_positive_length(
    length: &mut cadmpeg_ir::scalar::PositiveLength,
    scale: PositiveReal,
) -> Result<(), CodecError> {
    *length =
        cadmpeg_ir::scalar::PositiveLength::new(length.get() * scale.get()).ok_or_else(|| {
            CodecError::Malformed("Creo scaled length must be positive and finite".into())
        })?;
    Ok(())
}

fn scale_nonzero_length(
    length: &mut cadmpeg_ir::scalar::NonZeroLength,
    scale: PositiveReal,
) -> Result<(), CodecError> {
    *length = cadmpeg_ir::scalar::NonZeroLength::new(length.get() * scale.get())
        .ok_or_else(|| CodecError::malformed("Creo scaled length must be finite and nonzero"))?;
    Ok(())
}

fn scale_nonnegative_length(
    length: &mut cadmpeg_ir::scalar::NonNegativeLength,
    scale: PositiveReal,
) -> Result<(), CodecError> {
    *length = length.scaled(scale).ok_or_else(|| {
        CodecError::malformed("Creo scaled length must be nonnegative and finite")
    })?;
    Ok(())
}

fn scale_optional_positive_length(
    length: &mut Option<cadmpeg_ir::scalar::PositiveLength>,
    scale: PositiveReal,
) -> Result<(), CodecError> {
    if let Some(length) = length {
        scale_positive_length(length, scale)?;
    }
    Ok(())
}

fn scale_optional_length(
    length: &mut Option<Length>,
    scale: PositiveReal,
) -> Result<(), cadmpeg_core::CodecError> {
    if let Some(length) = length.as_mut() {
        scale_length(length, scale)?;
    }
    Ok(())
}

fn scale_datum_plane_reference(
    reference: &mut cadmpeg_ir::features::DatumPlaneReference,
    scale: PositiveReal,
) -> Result<(), CodecError> {
    if let cadmpeg_ir::features::DatumPlaneReference::ResolvedPlane { frame } = reference {
        let mut origin = frame.origin().get();
        scale_point3(&mut origin, scale);
        let origin = cadmpeg_ir::features::FinitePoint3::new(origin).ok_or_else(|| {
            CodecError::Malformed("Creo scaled plane support must have a finite origin".into())
        })?;
        *frame = frame.with_origin(origin);
    }
    Ok(())
}

fn scale_datum_point_construction(
    construction: &mut cadmpeg_ir::features::DatumPointConstruction,
    scale: PositiveReal,
) -> Result<(), CodecError> {
    match construction {
        cadmpeg_ir::features::DatumPointConstruction::ThreePlaneIntersection { planes } => {
            for plane in planes.iter_mut() {
                scale_datum_plane_reference(plane, scale)?;
            }
        }
        cadmpeg_ir::features::DatumPointConstruction::EdgePlaneIntersection { plane, .. } => {
            scale_datum_plane_reference(plane, scale)?;
        }
        cadmpeg_ir::features::DatumPointConstruction::CircleCenter { .. }
        | cadmpeg_ir::features::DatumPointConstruction::TwoEdgeIntersection { .. }
        | cadmpeg_ir::features::DatumPointConstruction::Vertex { .. }
        | cadmpeg_ir::features::DatumPointConstruction::SketchPoint { .. }
        | cadmpeg_ir::features::DatumPointConstruction::DistanceOnEdge { .. } => {}
    }
    Ok(())
}

fn scale_feature_definition(
    definition: &mut FeatureDefinition,
    scale: PositiveReal,
) -> Result<(), cadmpeg_core::CodecError> {
    match definition {
        FeatureDefinition::PostProcess {
            operation,
            fuzzy_tolerance,
            ..
        } => {
            scale_feature_operation(operation, scale)?;
            scale_fuzzy_tolerance(fuzzy_tolerance, scale)
        }
        FeatureDefinition::Operation(operation) => scale_feature_operation(operation, scale),
    }
}

fn scale_feature_operation(
    definition: &mut FeatureOperation,
    scale: PositiveReal,
) -> Result<(), cadmpeg_core::CodecError> {
    match definition {
        FeatureOperation::CosmeticThread {
            diameter, extent, ..
        } => {
            scale_optional_positive_length(diameter, scale)?;
            if let Some(cadmpeg_ir::features::CosmeticThreadExtent::Blind { length }) = extent {
                scale_positive_length(length, scale)?;
            }
        }
        FeatureOperation::ReferenceImage { frame, bounds, .. } => {
            scale_unit_plane_frame(frame, scale)?;
            let mut corners = bounds.corners().map(cadmpeg_ir::units::FinitePoint2::get);
            for point in &mut corners {
                scale_point2(point, scale);
            }
            *bounds = cadmpeg_ir::features::FeatureImageBounds::new(corners).ok_or_else(|| {
                CodecError::Malformed(
                    "Creo scaled image bounds must have finite corners and nonzero extents".into(),
                )
            })?;
        }
        FeatureOperation::DatumCoordinateSystem { frame } => {
            let mut origin = frame.origin().get();
            scale_point3(&mut origin, scale);
            let origin = cadmpeg_ir::features::FinitePoint3::new(origin).ok_or_else(|| {
                CodecError::Malformed(
                    "Creo scaled coordinate frame must have a finite origin".into(),
                )
            })?;
            *frame = frame.with_origin(origin);
        }
        FeatureOperation::DatumPlane { frame }
        | FeatureOperation::DatumThreePointPlane { frame, .. } => {
            let mut origin = frame.origin().get();
            scale_point3(&mut origin, scale);
            let origin = cadmpeg_ir::features::FinitePoint3::new(origin).ok_or_else(|| {
                CodecError::Malformed("Creo scaled datum plane must have a finite origin".into())
            })?;
            *frame = frame.with_origin(origin);
        }
        FeatureOperation::DatumAxis { origin, .. }
        | FeatureOperation::MirrorShape {
            plane_origin: origin,
            ..
        } => {
            scale_finite_point3(origin, scale)?;
        }
        FeatureOperation::DatumPoint {
            position,
            construction,
        } => {
            scale_finite_point3(position, scale)?;
            if let Some(construction) = construction {
                scale_datum_point_construction(construction, scale)?;
            }
        }
        FeatureOperation::DatumOffsetPlane {
            reference,
            distance,
        } => {
            if let Some(reference) = reference {
                scale_datum_plane_reference(reference, scale)?;
            }
            scale_length(distance, scale)?;
        }
        FeatureOperation::PointGeometry { position } => {
            scale_finite_point3(position, scale)?;
        }
        FeatureOperation::LineSegment { segment } => {
            let mut start = segment.start().get();
            let mut end = segment.end().get();
            scale_point3(&mut start, scale);
            scale_point3(&mut end, scale);
            *segment =
                cadmpeg_ir::features::FeatureLineSegment::new(start, end).ok_or_else(|| {
                    CodecError::Malformed(
                        "Creo scaled line must have finite distinct endpoints".into(),
                    )
                })?;
        }
        FeatureOperation::CircularArc { arc } => {
            let mut center = arc.center().get();
            let mut radius = arc.radius();
            scale_point3(&mut center, scale);
            scale_positive_length(&mut radius, scale)?;
            let center = cadmpeg_ir::features::FinitePoint3::new(center).ok_or_else(|| {
                CodecError::Malformed("Creo scaled circular arc must have finite geometry".into())
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
                CodecError::Malformed("Creo scaled length must be positive and finite".into())
            })?;
            let center = center.ok_or_else(|| {
                CodecError::Malformed(
                    "Creo scaled elliptic arc must have finite ordered geometry".into(),
                )
            })?;
            *arc = scaled.with_center(center);
        }
        FeatureOperation::Polyline { chain } => {
            let points = chain
                .points()
                .iter()
                .map(|point| {
                    let mut point = point.get();
                    scale_point3(&mut point, scale);
                    point
                })
                .collect();
            *chain = cadmpeg_ir::features::FeaturePolyline::new(points, chain.closed())
                .ok_or_else(|| {
                    CodecError::Malformed(
                        "Creo scaled polyline must have finite distinct adjacent vertices".into(),
                    )
                })?;
        }
        FeatureOperation::RegularPolygonCurve { circumradius, .. } => {
            scale_positive_length(circumradius, scale)?;
        }
        FeatureOperation::PlanarPatch { length, width } => {
            scale_positive_length(length, scale)?;
            scale_positive_length(width, scale)?;
        }
        FeatureOperation::Block {
            dimensions,
            placement,
            ..
        } => {
            if let Some(dimensions) = dimensions {
                for dimension in dimensions {
                    scale_positive_length(dimension, scale)?;
                }
            }
            if let Some(placement) = placement {
                *placement = placement.scaled_translation(scale).ok_or_else(|| {
                    CodecError::Malformed(
                        "Creo scaled block placement must remain finite and rigid".into(),
                    )
                })?;
            }
        }
        FeatureOperation::ProjectOnSurface { height, offset, .. } => {
            scale_nonnegative_length(height, scale)?;
            scale_length(offset, scale)?;
        }
        FeatureOperation::Helix {
            axis_origin,
            radius,
            shape,
            ..
        } => {
            scale_finite_point3(axis_origin, scale)?;
            scale_positive_length(radius, scale)?;
            match shape {
                cadmpeg_ir::features::HelixShape::Cylindrical { pitch }
                | cadmpeg_ir::features::HelixShape::Conical { pitch, .. } => {
                    scale_nonzero_length(pitch, scale)?;
                }
                cadmpeg_ir::features::HelixShape::Spiral { radial_growth } => {
                    scale_length(radial_growth, scale)?;
                }
            }
        }
        FeatureOperation::HelixNativeAxis {
            axial_rise, pitch, ..
        } => {
            scale_length(axial_rise, scale)?;
            scale_length(pitch, scale)?;
        }
        FeatureOperation::Sphere { center, radius, .. } => {
            scale_finite_point3(center, scale)?;
            scale_positive_length(radius, scale)?;
        }
        FeatureOperation::Torus {
            center,
            major_radius,
            minor_radius,
            ..
        } => {
            scale_finite_point3(center, scale)?;
            scale_positive_length(major_radius, scale)?;
            scale_positive_length(minor_radius, scale)?;
        }
        FeatureOperation::Wrap {
            mode: WrapMode::Emboss { depth } | WrapMode::Deboss { depth },
            ..
        } => scale_positive_length(depth, scale)?,
        FeatureOperation::Wrap {
            mode: WrapMode::Scribe,
            ..
        } => {}
        FeatureOperation::SketchBlockInstance {
            placement: Some(placement),
            ..
        } => scale_transform_translation(placement, scale)?,
        FeatureOperation::SketchBlockInstance {
            placement: None, ..
        } => {}
        FeatureOperation::Primitive { solid, .. } => scale_primitive_solid(solid, scale)?,
        FeatureOperation::Sweep { shape, .. } => {
            for generated in shape.generated_sections_mut() {
                scale_sweep_section(generated, scale)?;
            }
        }
        FeatureOperation::HelicalSweep { construction, .. } => {
            scale_finite_point3(&mut construction.axis_origin, scale)?;
            scale_nonnegative_length(&mut construction.pitch, scale)?;
            let mut height = construction.travel.height();
            let mut radial_growth = construction.travel.radial_growth();
            scale_length(&mut height, scale)?;
            scale_length(&mut radial_growth, scale)?;
            construction.travel = cadmpeg_ir::features::HelicalSweepTravel::new(
                height,
                radial_growth,
            )
            .ok_or_else(|| {
                CodecError::Malformed("Creo scaled helical sweep must retain nonzero travel".into())
            })?;
        }
        FeatureOperation::Coil { construction, .. } => {
            scale_coil_construction(construction, scale)?;
        }
        FeatureOperation::Binder {
            construction:
                cadmpeg_ir::features::BinderConstruction::SubShape {
                    offset: Some(offset),
                    ..
                },
            ..
        } => scale_nonzero_length(&mut offset.distance, scale)?,
        FeatureOperation::Binder { .. } => {}
        FeatureOperation::Loft { sections, .. } => {
            for section in sections {
                if let cadmpeg_ir::features::LoftSection::Point(
                    cadmpeg_ir::features::LoftPointSection::Point(point),
                ) = section
                {
                    scale_finite_point3(point, scale)?;
                }
            }
        }
        FeatureOperation::Extrude { start, extent, .. } => {
            scale_extrude_start(start, scale)?;
            scale_extrude_extent(extent, scale)?;
        }
        FeatureOperation::Revolve { construction, .. } => {
            if let Some(axis) = construction.axis_mut() {
                scale_finite_point3(&mut axis.origin, scale)?;
            }
            if let Some(extent) = construction.extent_mut() {
                scale_revolve_extent(extent, scale)?;
            }
        }
        FeatureOperation::Rib { construction, .. } => {
            scale_optional_positive_length(&mut construction.thickness, scale)?;
        }
        FeatureOperation::SheetMetalBaseFlange { thickness, .. } => {
            scale_positive_length(thickness, scale)?;
        }
        FeatureOperation::SheetMetalEdgeFlange {
            height,
            width,
            bend_radius,
            ..
        } => {
            scale_sheet_metal_flange_height(height, scale)?;
            scale_sheet_metal_flange_width(width, scale)?;
            scale_positive_length(bend_radius, scale)?;
        }
        FeatureOperation::SheetMetalHem {
            form, bend_radius, ..
        } => {
            scale_sheet_metal_hem_form(form, scale)?;
            scale_positive_length(bend_radius, scale)?;
        }
        FeatureOperation::Fillet { groups } => {
            for group in groups {
                scale_radius_spec(&mut group.radius, scale)?;
            }
        }
        FeatureOperation::FaceBlend { radius, .. } => scale_radius_spec(radius, scale)?,
        FeatureOperation::Chamfer { groups, .. } => {
            for group in groups {
                scale_chamfer_spec(&mut group.spec, scale)?;
            }
        }
        FeatureOperation::Shell { thickness, .. } => {
            scale_optional_positive_length(thickness, scale)?;
        }
        FeatureOperation::OffsetShape { distance, .. } => scale_nonzero_length(distance, scale)?,
        FeatureOperation::Thicken { thickness, .. } => {
            scale_optional_positive_length(thickness, scale)?;
        }
        FeatureOperation::OffsetSurface { distance, .. } => {
            scale_optional_length(distance, scale)?;
        }
        FeatureOperation::KnitSurface {
            gap_tolerance: Some(gap),
            ..
        } => {
            scale_nonnegative_length(gap, scale)?;
        }
        FeatureOperation::SewBodies { gap_tolerance, .. } => {
            scale_optional_positive_length(gap_tolerance, scale)?;
        }
        FeatureOperation::ExtendSurface { distance, .. } => {
            scale_optional_positive_length(distance, scale)?;
        }
        FeatureOperation::RuledSurface { mode, .. } => scale_ruled_surface_mode(mode, scale)?,
        FeatureOperation::Draft { .. } => {}
        FeatureOperation::MoveFace { motion, .. } => scale_face_motion(motion, scale)?,
        FeatureOperation::MoveBody {
            translation,
            rotation,
            ..
        } => {
            *translation = cadmpeg_ir::features::FiniteVector3::new(translation.scale(scale.get()))
                .ok_or_else(|| {
                    CodecError::Malformed("Creo scaled body translation must be finite".into())
                })?;
            if let Some(rotation) = rotation {
                scale_finite_point3(&mut rotation.origin, scale)?;
            }
        }
        FeatureOperation::Dome { height, .. } => scale_optional_positive_length(height, scale)?,
        FeatureOperation::Flex { mode, .. } => scale_flex_mode(mode, scale)?,
        FeatureOperation::Scale {
            center: Some(cadmpeg_ir::features::ScaleCenter::Point(point)),
            ..
        } => scale_finite_point3(point, scale)?,
        FeatureOperation::Scale { .. } => {}
        FeatureOperation::Hole {
            placements,
            shape,
            extent,
            ..
        } => {
            for placement in placements.iter_mut().flatten() {
                scale_hole_placement(placement, scale)?;
            }
            scale_hole_shape(shape, scale)?;
            if let Some(extent) = extent {
                scale_linear_termination(extent, scale)?;
            }
        }
        FeatureOperation::Pattern { pattern, .. } => scale_pattern_kind(pattern, scale)?,
        _ => {}
    }
    Ok(())
}

fn scale_fuzzy_tolerance(
    tolerance: &mut cadmpeg_ir::features::FuzzyTolerance,
    scale: PositiveReal,
) -> Result<(), CodecError> {
    if let cadmpeg_ir::features::FuzzyTolerance::Explicit(value) = tolerance {
        scale_positive_length(value, scale)?;
    }
    Ok(())
}

fn scale_primitive_solid(
    solid: &mut cadmpeg_ir::features::PrimitiveSolid,
    scale: PositiveReal,
) -> Result<(), cadmpeg_core::CodecError> {
    *solid = solid.scaled(scale).map_err(|error| match error {
        cadmpeg_ir::features::PrimitiveSolidScaleError::NonFinite => {
            CodecError::Malformed("Creo scaled length must be finite".into())
        }
        cadmpeg_ir::features::PrimitiveSolidScaleError::Admission(message) => {
            CodecError::Malformed(message.into())
        }
    })?;
    Ok(())
}

fn scale_sweep_section(
    section: &mut cadmpeg_ir::features::GeneratedSweepSection,
    scale: PositiveReal,
) -> Result<(), cadmpeg_core::CodecError> {
    let cadmpeg_ir::features::GeneratedSweepSection::CircularRegion { region } = section;
    let mut outer_radius = region.outer_radius();
    let mut wall_thickness = region.wall_thickness();
    scale_positive_length(&mut outer_radius, scale)?;
    scale_optional_positive_length(&mut wall_thickness, scale)?;
    *region = cadmpeg_ir::features::SweepCircularRegion::new(outer_radius, wall_thickness)
        .map_err(CodecError::malformed)?;
    Ok(())
}

fn scale_unit_plane_frame(
    frame: &mut cadmpeg_ir::features::FeatureUnitPlaneFrame,
    scale: PositiveReal,
) -> Result<(), CodecError> {
    let mut origin = frame.origin().get();
    scale_point3(&mut origin, scale);
    let origin = cadmpeg_ir::features::FinitePoint3::new(origin).ok_or_else(|| {
        CodecError::Malformed("Creo scaled plane frame must have a finite origin".into())
    })?;
    *frame = frame.with_origin(origin);
    Ok(())
}

fn scale_coil_construction(
    construction: &mut cadmpeg_ir::features::CoilConstruction,
    scale: PositiveReal,
) -> Result<(), cadmpeg_core::CodecError> {
    if let cadmpeg_ir::features::CoilPlacement::Explicit { frame } = &mut construction.placement {
        scale_unit_plane_frame(frame, scale)?;
    }
    scale_positive_length(&mut construction.diameter, scale)?;
    match &mut construction.extent {
        cadmpeg_ir::features::CoilExtent::RevolutionsHeight { height, .. } => {
            scale_length(height, scale)?;
        }
        cadmpeg_ir::features::CoilExtent::RevolutionsPitch { pitch, .. } => {
            scale_nonzero_length(pitch, scale)?;
        }
        cadmpeg_ir::features::CoilExtent::HeightPitch { height, pitch } => {
            scale_nonzero_length(height, scale)?;
            scale_nonzero_length(pitch, scale)?;
        }
        cadmpeg_ir::features::CoilExtent::Spiral { radial_pitch, .. } => {
            scale_nonzero_length(radial_pitch, scale)?;
        }
    }
    match &mut construction.section {
        cadmpeg_ir::features::CoilSection::Circular { diameter }
        | cadmpeg_ir::features::CoilSection::Square { size: diameter }
        | cadmpeg_ir::features::CoilSection::ExternalTriangle { size: diameter }
        | cadmpeg_ir::features::CoilSection::InternalTriangle { size: diameter } => {
            scale_positive_length(diameter, scale)?;
        }
    }
    Ok(())
}

fn scale_extrude_start(
    start: &mut cadmpeg_ir::features::ExtrudeStart,
    scale: PositiveReal,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::ExtrudeStart;

    match start {
        ExtrudeStart::OffsetProfilePlane { offset } => scale_length(offset, scale)?,
        ExtrudeStart::FromFace { offset, .. } => scale_optional_length(offset, scale)?,
        ExtrudeStart::Unresolved {} | ExtrudeStart::ProfilePlane {} => {}
    }
    Ok(())
}

fn scale_extrude_side(
    side: &mut cadmpeg_ir::features::ExtrudeSide,
    scale: PositiveReal,
) -> Result<(), cadmpeg_core::CodecError> {
    scale_linear_termination(&mut side.termination, scale)?;
    Ok(())
}

fn scale_extrude_extent(
    extent: &mut cadmpeg_ir::features::ExtrudeExtent,
    scale: PositiveReal,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::ExtrudeExtent;

    match extent {
        ExtrudeExtent::OneSided { side } | ExtrudeExtent::Symmetric { side } => {
            scale_extrude_side(side, scale)?;
        }
        ExtrudeExtent::TwoSided { first, second } => {
            scale_extrude_side(first, scale)?;
            scale_extrude_side(second, scale)?;
        }
    }
    Ok(())
}

fn scale_revolve_extent(
    extent: &mut cadmpeg_ir::features::RevolveExtent,
    scale: PositiveReal,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::RevolveExtent;

    match extent {
        RevolveExtent::OneSided { termination } | RevolveExtent::Symmetric { termination } => {
            scale_angular_termination(termination, scale)?;
        }
        RevolveExtent::TwoSided { first, second } => {
            scale_angular_termination(first, scale)?;
            scale_angular_termination(second, scale)?;
        }
    }
    Ok(())
}

fn scale_linear_termination(
    termination: &mut cadmpeg_ir::features::LinearTermination,
    scale: PositiveReal,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::LinearTermination;

    match termination {
        LinearTermination::Blind { length } => scale_nonzero_length(length, scale)?,
        LinearTermination::ToFace { offset, .. } => scale_optional_length(offset, scale)?,
        LinearTermination::OffsetFromFace { offset, .. } => scale_positive_length(offset, scale)?,
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
    termination: &mut cadmpeg_ir::features::AngularTermination,
    scale: PositiveReal,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::AngularTermination;

    match termination {
        AngularTermination::ToFace { offset, .. } => scale_optional_length(offset, scale)?,
        AngularTermination::OffsetFromFace { offset, .. } => scale_positive_length(offset, scale)?,
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
    height: &mut cadmpeg_ir::features::SheetMetalFlangeHeight,
    scale: PositiveReal,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::SheetMetalFlangeHeight;

    match height {
        SheetMetalFlangeHeight::Distance(distance) => scale_positive_length(distance, scale)?,
        SheetMetalFlangeHeight::ToObject { offset, .. } => scale_length(offset, scale)?,
    }
    Ok(())
}

fn scale_sheet_metal_flange_width(
    width: &mut cadmpeg_ir::features::SheetMetalFlangeWidth,
    scale: PositiveReal,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::SheetMetalFlangeWidth;

    match width {
        SheetMetalFlangeWidth::Symmetric { width } => scale_positive_length(width, scale)?,
        SheetMetalFlangeWidth::TwoSides { first, second } => {
            scale_positive_length(first, scale)?;
            scale_positive_length(second, scale)?;
        }
        SheetMetalFlangeWidth::TwoSidesPerEdge { widths } => {
            for width in widths.as_mut_slice() {
                scale_positive_length(&mut width.first, scale)?;
                scale_positive_length(&mut width.second, scale)?;
            }
        }
        SheetMetalFlangeWidth::FullEdge => {}
    }
    Ok(())
}

fn scale_sheet_metal_hem_form(
    form: &mut cadmpeg_ir::features::SheetMetalHemForm,
    scale: PositiveReal,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::SheetMetalHemForm;

    match form {
        SheetMetalHemForm::Flat { length } | SheetMetalHemForm::Rolled { radius: length, .. } => {
            scale_positive_length(length, scale)?;
        }
        SheetMetalHemForm::Open { gap, length } | SheetMetalHemForm::GapLength { gap, length } => {
            scale_nonnegative_length(gap, scale)?;
            scale_positive_length(length, scale)?;
        }
        SheetMetalHemForm::Teardrop {
            gap,
            length,
            radius,
        } => {
            scale_nonnegative_length(gap, scale)?;
            scale_positive_length(length, scale)?;
            scale_positive_length(radius, scale)?;
        }
    }
    Ok(())
}

fn scale_radius_spec(
    radius: &mut cadmpeg_ir::features::edge_treatments::RadiusSpec,
    scale: PositiveReal,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::edge_treatments::RadiusSpec;

    match radius {
        RadiusSpec::Constant { radius }
        | RadiusSpec::Chordal {
            chord_length: radius,
        } => scale_positive_length(radius, scale)?,
        RadiusSpec::Asymmetric {
            offset_one,
            offset_two,
        } => {
            scale_positive_length(offset_one, scale)?;
            scale_positive_length(offset_two, scale)?;
        }
        RadiusSpec::Variable { points } => {
            use cadmpeg_ir::features::edge_treatments::VariableRadiiMapError;
            *points = points
                .try_map_radii(|radius| {
                    radius.scaled(scale).ok_or_else(|| {
                        CodecError::Malformed("Creo scaled length must be finite".into())
                    })
                })
                .map_err(|error| match error {
                    VariableRadiiMapError::Radius(error) => error,
                    VariableRadiiMapError::Admission(message) => {
                        CodecError::Malformed(message.into())
                    }
                })?;
        }
        RadiusSpec::Unresolved { .. } => {}
    }
    Ok(())
}

fn scale_chamfer_spec(
    spec: &mut cadmpeg_ir::features::edge_treatments::ChamferSpec,
    scale: PositiveReal,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::edge_treatments::ChamferSpec;

    match spec {
        ChamferSpec::Distance { distance } | ChamferSpec::DistanceAngle { distance, .. } => {
            scale_positive_length(distance, scale)?;
        }
        ChamferSpec::TwoDistances { first, second } => {
            scale_positive_length(first, scale)?;
            scale_positive_length(second, scale)?;
        }
        cadmpeg_ir::features::edge_treatments::ChamferSpec::Unresolved { .. } => {}
    }
    Ok(())
}

fn scale_ruled_surface_mode(
    mode: &mut cadmpeg_ir::features::RuledSurfaceMode,
    scale: PositiveReal,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::RuledSurfaceMode;

    match mode {
        RuledSurfaceMode::Normal { distance }
        | RuledSurfaceMode::Tangent { distance }
        | RuledSurfaceMode::Direction { distance, .. } => scale_positive_length(distance, scale)?,
    }
    Ok(())
}

fn scale_face_motion(
    motion: &mut cadmpeg_ir::features::FaceMotion,
    scale: PositiveReal,
) -> Result<(), cadmpeg_core::CodecError> {
    match motion {
        cadmpeg_ir::features::FaceMotion::Offset { distance }
        | cadmpeg_ir::features::FaceMotion::Translate { distance, .. } => {
            scale_length(distance, scale)?;
        }
        cadmpeg_ir::features::FaceMotion::Rotate { axis_origin, .. } => {
            scale_finite_point3(axis_origin, scale)?;
        }
    }
    Ok(())
}

fn scale_flex_mode(
    mode: &mut cadmpeg_ir::features::FlexMode,
    scale: PositiveReal,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::FlexMode;

    match mode {
        FlexMode::Unresolved { .. } => {}
        FlexMode::Stretching { distance } => scale_length(distance, scale)?,
        FlexMode::Bending { .. } | FlexMode::Twisting { .. } | FlexMode::Tapering { .. } => {}
    }
    Ok(())
}

fn scale_hole_placement(
    placement: &mut cadmpeg_ir::features::holes::HolePlacement,
    scale: PositiveReal,
) -> Result<(), CodecError> {
    match placement {
        cadmpeg_ir::features::holes::HolePlacement::Directed { position, .. }
        | cadmpeg_ir::features::holes::HolePlacement::Axis {
            origin: position, ..
        } => scale_finite_point3(position, scale),
    }
}

/// Scale the bore and treatment dimensions of a hole. The treatment
/// diameters exceed the bore diameter strictly, and two scaled diameters can
/// round to one value, so the relation is admitted again.
fn scale_hole_shape(
    shape: &mut cadmpeg_ir::features::holes::HoleShape,
    scale: PositiveReal,
) -> Result<(), CodecError> {
    use cadmpeg_ir::features::holes::HoleLengthEditError;

    *shape = shape
        .try_map_lengths(
            &mut |value| {
                let mut value = value;
                scale_positive_length(&mut value, scale)?;
                Ok(value)
            },
            &mut |value| {
                let mut value = value;
                scale_length(&mut value, scale)?;
                Ok(value)
            },
        )
        .map_err(|error| match error {
            HoleLengthEditError::Field(error) => error,
            HoleLengthEditError::Counterdrill(message)
            | HoleLengthEditError::Treatment(message) => CodecError::malformed(message),
        })?;
    Ok(())
}

fn scale_pattern_kind<C: cadmpeg_ir::features::patterns::CompositeStages + Clone>(
    pattern: &mut cadmpeg_ir::features::patterns::PatternKind<C>,
    scale: PositiveReal,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::patterns::{PatternLengthEditError, PatternLengthField};

    *pattern = pattern
        .try_map_lengths(&mut |field| match field {
            PatternLengthField::Length(length) => scale_length(length, scale),
            PatternLengthField::PositiveLength(length) => scale_positive_length(length, scale),
            PatternLengthField::Point(point) => scale_finite_point3(point, scale),
        })
        .map_err(|error| match error {
            PatternLengthEditError::Field(error) => error,
            PatternLengthEditError::Offsets(message) => CodecError::Malformed(message.into()),
        })?;
    Ok(())
}

fn scale_surface_geometry(
    geometry: &mut SolvedSurfaceGeometry,
    scale: PositiveReal,
) -> Result<(), CodecError> {
    *geometry = geometry
        .scaled(scale)
        .map_err(|refusal| scale_refusal(refusal, "surface", scale))?;
    Ok(())
}

fn scale_curve_geometry(
    geometry: &mut SolvedCurveGeometry,
    scale: PositiveReal,
) -> Result<(), CodecError> {
    *geometry = geometry
        .scaled(scale)
        .map_err(|refusal| scale_refusal(refusal, "curve", scale))?;
    Ok(())
}

/// The codec error for a solved carrier's refusal of the unit scaling.
/// `carrier` names the carrier family in a control-point refusal.
fn scale_refusal(refusal: ScaleRefusal, carrier: &str, scale: PositiveReal) -> CodecError {
    match refusal {
        ScaleRefusal::Field(message) => CodecError::malformed(message),
        ScaleRefusal::ControlPoints(error) => CodecError::malformed(format_args!(
            "Creo {carrier} unit normalization produced invalid NURBS control points: {error}"
        )),
        ScaleRefusal::Samples(error) => CodecError::malformed(error),
        ScaleRefusal::Translation => CodecError::malformed(format_args!(
            "Creo length scale {} drives a transform translation the carrier refuses",
            scale.get()
        )),
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
                *payload =
                    cadmpeg_ir::geometry::surface_payloads::LinearSweepSurfaceConstruction::try_new(
                        payload.directrix().clone(),
                        direction,
                    )?;
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

/// The scale of a curve's parameter under the unit scaling. A line is
/// parameterized by length. A conic's parameter is dimensionless, so the
/// scaling keeps it, and the other carriers state no parameter scale.
fn curve_parameter_scale(
    geometry: &SolvedCurveGeometry,
    length_scale_mm: PositiveReal,
) -> Option<PositiveReal> {
    match geometry {
        SolvedCurveGeometry::Line(_) => Some(length_scale_mm),
        SolvedCurveGeometry::Transformed(placed) => {
            curve_parameter_scale(placed.basis(), length_scale_mm)
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
    }
}

fn surface_parameter_scales(geometry: &SolvedSurfaceGeometry, length_scale_mm: f64) -> [f64; 2] {
    match geometry {
        SolvedSurfaceGeometry::Plane(_) => [length_scale_mm, length_scale_mm],
        SolvedSurfaceGeometry::Cylinder(_) => [1.0, length_scale_mm],
        SolvedSurfaceGeometry::Cone(_) => [1.0, length_scale_mm],
        SolvedSurfaceGeometry::Sphere(_) => [1.0, 1.0],
        SolvedSurfaceGeometry::Torus(_) => [1.0, 1.0],
        SolvedSurfaceGeometry::Transformed(placed) => {
            surface_parameter_scales(placed.basis(), length_scale_mm)
        }
        SolvedSurfaceGeometry::Nurbs { .. }
        | SolvedSurfaceGeometry::Polygonal(_)
        | SolvedSurfaceGeometry::Unknown { .. } => [1.0, 1.0],
    }
}

fn pcurve_scales(ir: &CadIr, length_scale_mm: f64) -> BTreeMap<PcurveId, [f64; 2]> {
    let mut candidates = BTreeMap::<PcurveId, Vec<[f64; 2]>>::new();
    for coedge in &ir.model.coedges {
        let Some(loop_record) = ir
            .model
            .loops
            .iter()
            .find(|item| item.id == coedge.owner_loop)
        else {
            continue;
        };
        let Some(face) = ir
            .model
            .faces
            .iter()
            .find(|item| item.id == loop_record.face)
        else {
            continue;
        };
        let Some(surface) = ir
            .model
            .surfaces
            .iter()
            .find(|item| item.id == face.surface)
        else {
            continue;
        };
        let Some(geometry) = surface.geometry.solved() else {
            continue;
        };
        let scales = surface_parameter_scales(geometry, length_scale_mm);
        for use_record in &coedge.pcurves {
            observe_pcurve_scale(&mut candidates, &use_record.pcurve, scales);
        }
    }
    for loop_record in &ir.model.loops {
        let Some(face) = ir
            .model
            .faces
            .iter()
            .find(|item| item.id == loop_record.face)
        else {
            continue;
        };
        let Some(surface) = ir
            .model
            .surfaces
            .iter()
            .find(|item| item.id == face.surface)
        else {
            continue;
        };
        let Some(geometry) = surface.geometry.solved() else {
            continue;
        };
        let scales = surface_parameter_scales(geometry, length_scale_mm);
        for use_record in loop_record.vertex_pcurves() {
            observe_pcurve_scale(&mut candidates, &use_record.pcurve, scales);
        }
    }
    candidates
        .into_iter()
        .filter_map(|(id, values)| {
            let first = *values.first()?;
            values
                .iter()
                .all(|value| *value == first)
                .then_some((id, first))
        })
        .collect()
}

fn observe_pcurve_scale(
    candidates: &mut BTreeMap<PcurveId, Vec<[f64; 2]>>,
    id: &PcurveId,
    scales: [f64; 2],
) {
    let values = candidates.entry(id.clone()).or_default();
    if !values.contains(&scales) {
        values.push(scales);
    }
}

fn scale_sketch_geometry(
    geometry: &mut SketchGeometry,
    scale: PositiveReal,
) -> Result<(), CodecError> {
    use cadmpeg_ir::sketches::scaling::SketchLengthScaleError;

    *geometry = geometry
        .scaled_lengths(scale)
        .map_err(|error| match error {
            SketchLengthScaleError::LengthOverflow => {
                CodecError::Malformed("Creo scaled length must be finite".into())
            }
            SketchLengthScaleError::Field(message) => CodecError::Malformed(message.into()),
            SketchLengthScaleError::CurveControlPoints(error) => {
                CodecError::malformed(format_args!(
                    "Creo sketch unit normalization produced invalid NURBS control points: {error}"
                ))
            }
            SketchLengthScaleError::SurfaceControlPoints(error) => {
                CodecError::malformed(format_args!(
            "Creo sketch unit normalization produced invalid B-spline control points: {error}"
        ))
            }
        })?;
    Ok(())
}

fn scale_spatial_sketch_geometry(
    geometry: &mut SpatialSketchGeometry,
    scale: PositiveReal,
) -> Result<(), CodecError> {
    use cadmpeg_ir::sketches::scaling::SketchLengthScaleError;

    *geometry = geometry.scaled_lengths(scale).map_err(|error| match error {
        SketchLengthScaleError::LengthOverflow => {
            CodecError::Malformed("Creo scaled length must be finite".into())
        }
        SketchLengthScaleError::Field(message) => CodecError::Malformed(message.into()),
        SketchLengthScaleError::CurveControlPoints(error) => CodecError::malformed(format_args!(
            "Creo spatial sketch unit normalization produced invalid NURBS control points: {error}"
        )),
        SketchLengthScaleError::SurfaceControlPoints(error) => CodecError::malformed(format_args!(
            "Creo spatial sketch unit normalization produced invalid B-spline control points: {error}"
        )),
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {

    #[test]
    fn numerical_followup_parabola_parameter_bounds_scale_as_lengths() {
        use cadmpeg_ir::sketches::{SketchGeometry, SketchGeometryDefinition};
        let mut geometry = SketchGeometry::try_from(SketchGeometryDefinition::Parabola {
            vertex: Point2::new(0., 0.),
            axis_angle: cadmpeg_ir::scalar::Angle::new(0.)
                .expect("valid finite regression fixture"),
            focal_length: Length::new(2.).expect("valid finite regression fixture"),
            bounds: Some([1., 2.]),
        })
        .expect("valid finite regression fixture");
        super::scale_sketch_geometry(&mut geometry, positive(10.))
            .expect("valid finite regression fixture");
        assert!(
            matches!(geometry.definition(),SketchGeometryDefinition::Parabola{bounds:Some(bounds),focal_length,..} if cadmpeg_ir::scalar::FiniteReal::raw_array(*bounds) == [10., 20.] && focal_length.get()==20.)
        );
    }

    use super::{
        normalize_model_lengths, scale_curve_geometry, scale_face_motion, scale_feature_definition,
        scale_pattern_kind, scale_surface_geometry,
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
    use cadmpeg_ir::transform::Transform;
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

    /// The length scale and the transform both come from the file, so a scale
    /// that drives a translation non-finite is a `CodecError`, not a panic.
    #[test]
    fn a_length_scale_that_overflows_a_translation_is_refused() {
        let transform = Transform::affine([
            [1.0, 0.0, 0.0, f64::MAX],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ])
        .expect("a finite affine fixture");
        let mut ir = CadIr::empty();
        ir.model.occurrences.push(cadmpeg_ir::products::Occurrence {
            id: cadmpeg_ir::ids::OccurrenceId::mint("creo:test:occurrence#0")
                .expect("identity grammar"),
            prototype: cadmpeg_ir::products::PrototypeReference::Local {
                definition: cadmpeg_ir::ids::ProductDefinitionId::mint("creo:test:product#0")
                    .expect("identity grammar"),
            },
            parent: cadmpeg_ir::products::OccurrenceParent::Root {},
            ordinal: 0,
            transform,
            linked_prototype: None,
            scale: [cadmpeg_ir::scalar::FiniteReal::ONE; 3],
            name: None,
            visible: None,
            link: None,
            native_ref: None,
        });
        let error = normalize_model_lengths(&mut ir, positive(1000.0))
            .expect_err("a non-finite translation has no transform")
            .to_string();
        assert!(error.contains("transform translation"), "{error}");
    }

    #[test]
    fn scales_model_geometry_and_feature_dimensions() {
        let mut ir = CadIr::empty();
        ir.model.features.push(Feature {
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
                    profile: ProfileRef::Planar(PlanarProfileRef::Unresolved("profile".into())),
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
                                face: cadmpeg_ir::features::FaceSelection::Native("face".into()),
                                offset: Some(Length::new(4.0).expect("finite length fixture")),
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
        });
        ir.model
            .parameters
            .push(cadmpeg_ir::features::DesignParameter {
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
            });
        normalize_model_lengths(&mut ir, positive(25.4)).expect("valid unit scaling");

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

    fn model_point_ir(position: Point3) -> CadIr {
        let mut ir = CadIr::empty();
        ir.model.points.push(cadmpeg_ir::topology::Point::new(
            cadmpeg_ir::ids::PointId::mint("test:model:entity#point").expect("identity grammar"),
            cadmpeg_ir::features::FinitePoint3::new(position).expect("finite point fixture"),
            None,
        ));
        ir
    }

    #[test]
    fn model_points_of_an_inch_model_are_converted_to_millimetres() {
        let mut ir = model_point_ir(Point3::new(1.0, -2.0, 0.5));
        normalize_model_lengths(&mut ir, positive(25.4)).expect("valid unit scaling");
        assert_point3(ir.model.points[0].position().get(), [25.4, -50.8, 12.7]);
    }

    #[test]
    fn a_model_point_that_overflows_in_millimetres_is_refused() {
        let mut ir = model_point_ir(Point3::new(0.0, f64::MAX, 0.0));
        let error = normalize_model_lengths(&mut ir, positive(25.4))
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
        ir.model.curves.push(cadmpeg_ir::geometry::Curve {
            id: curve_id,
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)),
            source_object: None,
        });

        let error =
            normalize_model_lengths(&mut ir, positive(25.4)).expect_err("overflow must refuse");
        assert!(matches!(error, CodecError::Malformed(_)));
        let Some(SolvedCurveGeometry::Nurbs(curve)) = ir.model.curves[0].geometry.solved() else {
            panic!("test curve changed family");
        };
        assert_eq!(curve.control_points()[0], Point3::new(f64::MAX, 0.0, 0.0));
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
        scale_face_motion(&mut translate, positive(25.4)).expect("valid test fixture");
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
        scale_face_motion(&mut rotate, positive(25.4)).expect("valid test fixture");
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
        scale_pattern_kind(&mut pattern, positive(25.4)).expect("valid test fixture");
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

        scale_feature_definition(&mut definition, positive(25.4)).expect("valid test fixture");

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
        let surface_id = cadmpeg_ir::ids::SurfaceId::mint("test:model:entity#surface")
            .expect("identity grammar");
        ir.model.surfaces.push(cadmpeg_ir::geometry::Surface {
            id: surface_id.clone(),
            geometry: cadmpeg_ir::geometry::SurfaceGeometry::Solved(
                SolvedSurfaceGeometry::Unknown { record: None },
            ),
            source_object: None,
        });
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
        ir.model
            .add_procedural_surface(surface_id, surface)
            .unwrap();
        let curve_id =
            cadmpeg_ir::ids::CurveId::mint("test:model:entity#curve").expect("identity grammar");
        ir.model.curves.push(cadmpeg_ir::geometry::Curve {
            id: curve_id.clone(),
            geometry: cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Unknown {
                record: None,
            }),
            source_object: None,
        });
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
        ir.model.add_procedural_curve(curve_id, curve).unwrap();

        normalize_model_lengths(&mut ir, positive(25.4)).expect("valid unit scaling");

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
            ir.model
                .add_procedural_surface(surface_id, surface)
                .unwrap();
            let error = normalize_model_lengths(&mut ir, positive(25.4))
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
        scale_surface_geometry(surface_solved, positive(25.4)).expect("finite surface scaling");
        let CurveGeometry::Solved(curve_solved) = &mut curve else {
            panic!("test curve changed family");
        };
        scale_curve_geometry(curve_solved, positive(25.4)).expect("finite curve scaling");

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
        scale_feature_definition(&mut definition, positive(25.4)).expect("ordered radii");
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
            let error = scale_feature_definition(&mut definition, positive(25.4))
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
                super::scale_positive_length(&mut value, positive(25.4))?;
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
        let error = super::scale_hole_shape(&mut shape, positive(25.4))
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
        let error = super::scale_sweep_section(&mut section, positive(25.4))
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
        let error = scale_pattern_kind(&mut pattern, positive(25.4))
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
        let error = super::scale_primitive_solid(&mut solid, positive(25.4))
            .expect_err("the extent collapses")
            .to_string();
        assert!(
            error.contains("primitive dimensions are invalid"),
            "{error}"
        );
    }
}
