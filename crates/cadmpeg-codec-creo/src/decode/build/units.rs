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
use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};
use cadmpeg_ir::ids::PcurveId;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{
    SketchGeometry, SketchGeometryDefinition, SketchPlacement, SpatialSketchGeometry,
    SpatialSketchGeometryDefinition,
};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::{
    features::{FeatureDefinition, ParameterValue, WrapMode},
    scalar::Length,
};

/// Scale all neutral model lengths from the source unit into millimeters.
pub(super) fn normalize_model_lengths(
    ir: &mut CadIr,
    length_scale_mm: f64,
) -> Result<(), CodecError> {
    if !length_scale_mm.is_finite() || length_scale_mm <= 0.0 || length_scale_mm == 1.0 {
        return Ok(());
    }

    let pcurve_scales = pcurve_scales(ir, length_scale_mm);
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
        scale_surface_geometry(&mut surface.geometry, length_scale_mm)?;
    }
    for curve in &mut ir.model.curves {
        scale_curve_geometry(&mut curve.geometry, length_scale_mm)?;
    }
    for procedural in &mut ir.model.procedural_surfaces {
        procedural
            .edit_definition(|definition| {
                scale_procedural_surface_definition(definition, length_scale_mm);
            })
            .map_err(cadmpeg_core::CodecError::malformed)?;
        procedural
            .scale_cache_fit_tolerance(length_scale_mm)
            .map_err(cadmpeg_core::CodecError::malformed)?;
    }
    for procedural in &mut ir.model.procedural_curves {
        procedural
            .edit_definition(|definition| {
                scale_procedural_curve_definition(definition, length_scale_mm)
            })
            .map_err(cadmpeg_core::CodecError::malformed)?
            .map_err(cadmpeg_core::CodecError::malformed)?;
        procedural
            .scale_cache_fit_tolerance(length_scale_mm)
            .map_err(cadmpeg_core::CodecError::malformed)?;
    }
    for point in &mut ir.model.points {
        scale_point3(&mut point.position, length_scale_mm);
    }
    for face in &mut ir.model.faces {
        scale_tolerance(&mut face.tolerance, length_scale_mm)?;
    }
    for vertex in &mut ir.model.vertices {
        scale_tolerance(&mut vertex.tolerance, length_scale_mm)?;
    }

    let curve_parameter_scales = ir
        .model
        .curves
        .iter()
        .filter_map(|curve| {
            curve_parameter_scale(&curve.geometry, length_scale_mm)
                .map(|scale| (curve.id.clone(), scale))
        })
        .collect::<BTreeMap<_, _>>();
    for edge in &mut ir.model.edges {
        scale_tolerance(&mut edge.tolerance, length_scale_mm)?;
        if let (Some(mut range), Some(scale)) = (
            edge.param_range(),
            edge.curve()
                .as_ref()
                .and_then(|id| curve_parameter_scales.get(id)),
        ) {
            scale_pair(&mut range, *scale);
            edge.set_param_range(Some(range))
                .map_err(cadmpeg_core::CodecError::malformed)?;
        }
    }
    for coedge in &mut ir.model.coedges {
        if let Some(use_curve) = &mut coedge.use_curve {
            if let Some(scale) = curve_parameter_scales.get(&use_curve.curve) {
                let mut range = use_curve.parameter_range.endpoints();
                scale_pair(&mut range, *scale);
                use_curve.parameter_range = cadmpeg_ir::topology::ParameterInterval::new(range)
                    .map_err(cadmpeg_core::CodecError::malformed)?;
            }
        }
    }

    for body in &mut ir.model.bodies {
        if let Some(transform) = body.transform.as_mut() {
            scale_transform_translation(transform, length_scale_mm);
        }
    }
    for occurrence in &mut ir.model.occurrences {
        scale_transform_translation(&mut occurrence.transform, length_scale_mm);
        if let Some(transform) = occurrence.linked_prototype.as_mut() {
            scale_transform_translation(transform, length_scale_mm);
        }
    }
    for tessellation in &mut ir.model.tessellations {
        tessellation
            .edit_vertices(|vertices| {
                for vertex in vertices {
                    scale_point3(vertex, length_scale_mm);
                }
            })
            .map_err(|error| {
                CodecError::malformed(format_args!("invalid scaled tessellation: {error}"))
            })?;
        tessellation
            .set_chordal_deflection(
                tessellation
                    .chordal_deflection()
                    .map(|value| value * length_scale_mm),
            )
            .map_err(|error| {
                CodecError::malformed(format_args!("invalid scaled tessellation: {error}"))
            })?;
    }
    for feature in &mut ir.model.features {
        let mut definition = feature.evaluation.definition().clone();
        scale_feature_definition(&mut definition, length_scale_mm)?;
        feature
            .evaluation
            .set_definition(definition)
            .map_err(cadmpeg_core::CodecError::malformed)?;
    }

    for parameter in &mut ir.model.parameters {
        if let Some(ParameterValue::Length(length)) = parameter.value.as_mut() {
            scale_length(length, length_scale_mm)?;
        }
    }
    for configuration in &mut ir.model.configurations {
        for value in configuration.parameter_values.values_mut() {
            if let ParameterValue::Length(length) = value {
                scale_length(length, length_scale_mm)?;
            }
        }
        for state in configuration.feature_states.values_mut() {
            scale_feature_definition(&mut state.definition, length_scale_mm)?;
        }
    }
    for sketch in &mut ir.model.sketches {
        if let Some((mut origin, normal, u_axis)) = sketch.resolved_placement() {
            scale_point3(&mut origin, length_scale_mm);
            sketch.placement = SketchPlacement::try_resolved(origin, normal, u_axis)
                .map_err(CodecError::malformed)?;
        }
    }
    for entity in &mut ir.model.sketch_entities {
        scale_sketch_geometry(&mut entity.geometry, length_scale_mm)?;
    }
    for sketch in &mut ir.model.spatial_sketches {
        for profile in &mut sketch.profiles {
            let mut origin = profile.origin();
            scale_point3(&mut origin, length_scale_mm);
            profile.set_origin(origin).map_err(CodecError::malformed)?;
        }
    }
    for entity in &mut ir.model.spatial_sketch_entities {
        scale_spatial_sketch_geometry(&mut entity.geometry, length_scale_mm)?;
    }
    for constraint in &mut ir.model.sketch_constraints {
        constraint
            .definition
            .edit(|kind| scale_sketch_constraint_definition(kind, length_scale_mm))
            .map_err(cadmpeg_core::CodecError::malformed)??;
    }
    for constraint in &mut ir.model.spatial_sketch_constraints {
        constraint
            .definition
            .edit(|kind| scale_spatial_sketch_constraint_definition(kind, length_scale_mm))
            .map_err(cadmpeg_core::CodecError::malformed)??;
    }
    Ok(())
}

fn scale_tolerance(
    value: &mut Option<cadmpeg_ir::units::PositiveScalar>,
    scale: f64,
) -> Result<(), CodecError> {
    if let Some(current) = value {
        *current =
            cadmpeg_ir::units::PositiveScalar::new(current.get() * scale).ok_or_else(|| {
                CodecError::malformed("scaled topology tolerance must be positive and finite")
            })?;
    }
    Ok(())
}

fn scale_pair(values: &mut [f64; 2], scale: f64) {
    values[0] *= scale;
    values[1] *= scale;
}

fn scale_point2(point: &mut Point2, scale: f64) {
    point.u *= scale;
    point.v *= scale;
}

fn scale_point3(point: &mut Point3, scale: f64) {
    point.x *= scale;
    point.y *= scale;
    point.z *= scale;
}

fn scale_finite_point3(
    point: &mut cadmpeg_ir::features::FinitePoint3,
    scale: f64,
) -> Result<(), CodecError> {
    let mut scaled = point.get();
    scale_point3(&mut scaled, scale);
    *point = cadmpeg_ir::features::FinitePoint3::new(scaled)
        .ok_or_else(|| CodecError::Malformed("Creo scaled feature point must be finite".into()))?;
    Ok(())
}

fn scale_vector3(vector: &mut Vector3, scale: f64) {
    vector.x *= scale;
    vector.y *= scale;
    vector.z *= scale;
}

fn scale_transform_translation(transform: &mut Transform, scale: f64) {
    let mut rows = transform.rows();
    for row in &mut rows[..3] {
        row[3] *= scale;
    }
    *transform = Transform::from_rows(rows).expect("affine transform");
}

fn scale_length(length: &mut Length, scale: f64) -> Result<(), CodecError> {
    *length = Length::new(length.get() * scale)
        .ok_or_else(|| CodecError::Malformed("Creo scaled length must be finite".into()))?;
    Ok(())
}

fn scale_positive_length(
    length: &mut cadmpeg_ir::scalar::PositiveLength,
    scale: f64,
) -> Result<(), CodecError> {
    *length = cadmpeg_ir::scalar::PositiveLength::new(length.get() * scale).ok_or_else(|| {
        CodecError::Malformed("Creo scaled length must be positive and finite".into())
    })?;
    Ok(())
}

fn scale_nonzero_length(
    length: &mut cadmpeg_ir::scalar::NonZeroLength,
    scale: f64,
) -> Result<(), CodecError> {
    *length = cadmpeg_ir::scalar::NonZeroLength::new(length.get() * scale)
        .ok_or_else(|| CodecError::malformed("Creo scaled length must be finite and nonzero"))?;
    Ok(())
}

fn scale_nonnegative_length(
    length: &mut cadmpeg_ir::scalar::NonNegativeLength,
    scale: f64,
) -> Result<(), CodecError> {
    *length =
        cadmpeg_ir::scalar::NonNegativeLength::new(length.get() * scale).ok_or_else(|| {
            CodecError::malformed("Creo scaled length must be nonnegative and finite")
        })?;
    Ok(())
}

fn scale_optional_positive_length(
    length: &mut Option<cadmpeg_ir::scalar::PositiveLength>,
    scale: f64,
) -> Result<(), CodecError> {
    if let Some(length) = length {
        scale_positive_length(length, scale)?;
    }
    Ok(())
}

fn scale_optional_length(
    length: &mut Option<Length>,
    scale: f64,
) -> Result<(), cadmpeg_core::CodecError> {
    if let Some(length) = length.as_mut() {
        scale_length(length, scale)?;
    }
    Ok(())
}

fn scale_datum_plane_reference(
    reference: &mut cadmpeg_ir::features::DatumPlaneReference,
    scale: f64,
) -> Result<(), CodecError> {
    if let cadmpeg_ir::features::DatumPlaneReference::ResolvedPlane { frame } = reference {
        let mut origin = frame.origin();
        scale_point3(&mut origin, scale);
        *frame = cadmpeg_ir::features::FeatureSupportPlaneFrame::new(
            origin,
            frame.normal(),
            frame.u_axis(),
        )
        .ok_or_else(|| {
            CodecError::Malformed("Creo scaled plane support must have a finite origin".into())
        })?;
    }
    Ok(())
}

fn scale_datum_point_construction(
    construction: &mut cadmpeg_ir::features::DatumPointConstruction,
    scale: f64,
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
    scale: f64,
) -> Result<(), cadmpeg_core::CodecError> {
    match definition {
        FeatureDefinition::CosmeticThread {
            diameter, extent, ..
        } => {
            scale_optional_positive_length(diameter, scale)?;
            if let Some(cadmpeg_ir::features::CosmeticThreadExtent::Blind { length }) = extent {
                scale_positive_length(length, scale)?;
            }
        }
        FeatureDefinition::ReferenceImage { frame, bounds, .. } => {
            scale_unit_plane_frame(frame, scale)?;
            let mut corners = bounds.corners();
            for point in &mut corners {
                scale_point2(point, scale);
            }
            *bounds = cadmpeg_ir::features::FeatureImageBounds::new(corners).ok_or_else(|| {
                CodecError::Malformed(
                    "Creo scaled image bounds must have finite corners and nonzero extents".into(),
                )
            })?;
        }
        FeatureDefinition::DatumCoordinateSystem { frame } => {
            let mut origin = frame.origin();
            scale_point3(&mut origin, scale);
            *frame = cadmpeg_ir::features::FeatureCoordinateFrame::new(
                origin,
                frame.x_axis(),
                frame.y_axis(),
                frame.z_axis(),
            )
            .ok_or_else(|| {
                CodecError::Malformed(
                    "Creo scaled coordinate frame must have a finite origin".into(),
                )
            })?;
        }
        FeatureDefinition::DatumPlane { frame }
        | FeatureDefinition::DatumThreePointPlane { frame, .. } => {
            let mut origin = frame.origin();
            scale_point3(&mut origin, scale);
            *frame = cadmpeg_ir::features::FeatureDatumPlaneFrame::new(
                origin,
                frame.normal(),
                frame.u_axis(),
            )
            .ok_or_else(|| {
                CodecError::Malformed("Creo scaled datum plane must have a finite origin".into())
            })?;
        }
        FeatureDefinition::DatumAxis { origin, .. }
        | FeatureDefinition::MirrorShape {
            plane_origin: origin,
            ..
        } => {
            scale_finite_point3(origin, scale)?;
        }
        FeatureDefinition::DatumPoint {
            position,
            construction,
        } => {
            scale_finite_point3(position, scale)?;
            if let Some(construction) = construction {
                scale_datum_point_construction(construction, scale)?;
            }
        }
        FeatureDefinition::DatumOffsetPlane {
            reference,
            distance,
        } => {
            if let Some(reference) = reference {
                scale_datum_plane_reference(reference, scale)?;
            }
            scale_length(distance, scale)?;
        }
        FeatureDefinition::PointGeometry { position } => {
            scale_finite_point3(position, scale)?;
        }
        FeatureDefinition::LineSegment { segment } => {
            let mut start = segment.start();
            let mut end = segment.end();
            scale_point3(&mut start, scale);
            scale_point3(&mut end, scale);
            *segment =
                cadmpeg_ir::features::FeatureLineSegment::new(start, end).ok_or_else(|| {
                    CodecError::Malformed(
                        "Creo scaled line must have finite distinct endpoints".into(),
                    )
                })?;
        }
        FeatureDefinition::CircularArc { arc } => {
            let mut center = arc.center();
            let mut radius = arc.radius();
            scale_point3(&mut center, scale);
            scale_positive_length(&mut radius, scale)?;
            *arc = cadmpeg_ir::features::FeatureCircularArc::new(
                center,
                arc.normal(),
                radius,
                arc.angles(),
            )
            .ok_or_else(|| {
                CodecError::Malformed("Creo scaled circular arc must have finite geometry".into())
            })?;
        }
        FeatureDefinition::EllipticArc { arc } => {
            let mut center = arc.center();
            let mut radii = arc.radii();
            scale_point3(&mut center, scale);
            for radius in &mut radii {
                scale_positive_length(radius, scale)?;
            }
            *arc = cadmpeg_ir::features::FeatureEllipticArc::new(
                center,
                arc.normal(),
                arc.major_axis(),
                radii,
                arc.angles(),
            )
            .ok_or_else(|| {
                CodecError::Malformed(
                    "Creo scaled elliptic arc must have finite ordered geometry".into(),
                )
            })?;
        }
        FeatureDefinition::Polyline { chain } => {
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
        FeatureDefinition::RegularPolygonCurve { circumradius, .. } => {
            scale_positive_length(circumradius, scale)?;
        }
        FeatureDefinition::PlanarPatch { length, width } => {
            scale_positive_length(length, scale)?;
            scale_positive_length(width, scale)?;
        }
        FeatureDefinition::Block {
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
                let mut rows = placement.rows();
                for row in &mut rows[..3] {
                    row[3] *= scale;
                }
                *placement = Transform::from_rows(rows)
                    .and_then(cadmpeg_ir::features::FeatureRigidPlacement::new)
                    .ok_or_else(|| {
                        CodecError::Malformed(
                            "Creo scaled block placement must remain finite and rigid".into(),
                        )
                    })?;
            }
        }
        FeatureDefinition::ProjectOnSurface { height, offset, .. } => {
            scale_nonnegative_length(height, scale)?;
            scale_length(offset, scale)?;
        }
        FeatureDefinition::Helix {
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
        FeatureDefinition::HelixNativeAxis {
            axial_rise, pitch, ..
        } => {
            scale_length(axial_rise, scale)?;
            scale_length(pitch, scale)?;
        }
        FeatureDefinition::Sphere { center, radius, .. } => {
            scale_finite_point3(center, scale)?;
            scale_positive_length(radius, scale)?;
        }
        FeatureDefinition::Torus {
            center,
            major_radius,
            minor_radius,
            ..
        } => {
            scale_finite_point3(center, scale)?;
            scale_positive_length(major_radius, scale)?;
            scale_positive_length(minor_radius, scale)?;
        }
        FeatureDefinition::Wrap {
            mode: WrapMode::Emboss { depth } | WrapMode::Deboss { depth },
            ..
        } => scale_length(depth, scale)?,
        FeatureDefinition::Wrap {
            mode: WrapMode::Scribe,
            ..
        } => {}
        FeatureDefinition::SketchBlockInstance {
            placement: Some(placement),
            ..
        } => scale_transform_translation(placement, scale),
        FeatureDefinition::SketchBlockInstance {
            placement: None, ..
        } => {}
        FeatureDefinition::Primitive { solid, .. } => scale_primitive_solid(solid, scale)?,
        FeatureDefinition::Sweep { shape, .. } => {
            let mut section = shape.section().clone();
            let mut sections = shape.sections().to_vec();
            scale_sweep_section(&mut section, scale)?;
            for section in &mut sections {
                scale_sweep_section(section, scale)?;
            }
            *shape = cadmpeg_ir::features::SweepShape::new(section, sections, shape.mode())
                .map_err(CodecError::malformed)?;
        }
        FeatureDefinition::HelicalSweep { construction, .. } => {
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
        FeatureDefinition::Coil { construction, .. } => {
            scale_coil_construction(construction, scale)?;
        }
        FeatureDefinition::Binder {
            construction:
                cadmpeg_ir::features::BinderConstruction::SubShape {
                    offset: Some(offset),
                    ..
                },
            ..
        } => scale_nonzero_length(&mut offset.distance, scale)?,
        FeatureDefinition::Binder { .. } => {}
        FeatureDefinition::Loft { sections, .. } => {
            for section in sections {
                if let cadmpeg_ir::features::LoftSection::Point(
                    cadmpeg_ir::features::LoftPointSection::Point(point),
                ) = section
                {
                    scale_finite_point3(point, scale)?;
                }
            }
        }
        FeatureDefinition::Extrude { start, extent, .. } => {
            scale_extrude_start(start, scale)?;
            scale_extrude_extent(extent, scale)?;
        }
        FeatureDefinition::Revolve { construction, .. } => {
            if let Some(axis) = construction.axis_mut() {
                scale_finite_point3(&mut axis.origin, scale)?;
            }
            if let Some(extent) = construction.extent_mut() {
                scale_revolve_extent(extent, scale)?;
            }
        }
        FeatureDefinition::Rib { construction, .. } => {
            scale_optional_positive_length(&mut construction.thickness, scale)?;
        }
        FeatureDefinition::SheetMetalBaseFlange { thickness, .. } => {
            scale_positive_length(thickness, scale)?;
        }
        FeatureDefinition::SheetMetalEdgeFlange {
            height,
            width,
            bend_radius,
            ..
        } => {
            scale_sheet_metal_flange_height(height, scale)?;
            scale_sheet_metal_flange_width(width, scale)?;
            scale_positive_length(bend_radius, scale)?;
        }
        FeatureDefinition::SheetMetalHem {
            form, bend_radius, ..
        } => {
            scale_sheet_metal_hem_form(form, scale)?;
            scale_positive_length(bend_radius, scale)?;
        }
        FeatureDefinition::Fillet { groups } => {
            for group in groups {
                scale_radius_spec(&mut group.radius, scale)?;
            }
        }
        FeatureDefinition::FaceBlend { radius, .. } => scale_radius_spec(radius, scale)?,
        FeatureDefinition::Chamfer { groups, .. } => {
            for group in groups {
                scale_chamfer_spec(&mut group.spec, scale)?;
            }
        }
        FeatureDefinition::Shell { thickness, .. } => {
            scale_optional_positive_length(thickness, scale)?;
        }
        FeatureDefinition::OffsetShape { distance, .. } => scale_nonzero_length(distance, scale)?,
        FeatureDefinition::Thicken { thickness, .. } => {
            scale_optional_positive_length(thickness, scale)?;
        }
        FeatureDefinition::OffsetSurface { distance, .. } => {
            scale_optional_length(distance, scale)?;
        }
        FeatureDefinition::KnitSurface {
            gap_tolerance: Some(gap),
            ..
        } => {
            scale_nonnegative_length(gap, scale)?;
        }
        FeatureDefinition::SewBodies { gap_tolerance, .. } => {
            scale_optional_positive_length(gap_tolerance, scale)?;
        }
        FeatureDefinition::ExtendSurface { distance, .. } => {
            scale_optional_positive_length(distance, scale)?;
        }
        FeatureDefinition::RuledSurface { mode, .. } => scale_ruled_surface_mode(mode, scale)?,
        FeatureDefinition::Draft { .. } => {}
        FeatureDefinition::MoveFace { motion, .. } => scale_face_motion(motion, scale)?,
        FeatureDefinition::MoveBody {
            translation,
            rotation,
            ..
        } => {
            *translation = cadmpeg_ir::features::FiniteVector3::new(translation.scale(scale))
                .ok_or_else(|| {
                    CodecError::Malformed("Creo scaled body translation must be finite".into())
                })?;
            if let Some(rotation) = rotation {
                scale_finite_point3(&mut rotation.origin, scale)?;
            }
        }
        FeatureDefinition::Dome { height, .. } => scale_optional_positive_length(height, scale)?,
        FeatureDefinition::Flex { mode, .. } => scale_flex_mode(mode, scale)?,
        FeatureDefinition::Scale {
            center: Some(cadmpeg_ir::features::ScaleCenter::Point(point)),
            ..
        } => scale_finite_point3(point, scale)?,
        FeatureDefinition::Scale { .. } => {}
        FeatureDefinition::Hole {
            placements,
            shape,
            extent,
            ..
        } => {
            for placement in placements.iter_mut().flatten() {
                scale_hole_placement(placement, scale)?;
            }
            let mut construction = shape.construction().clone();
            let mut exit_kind = *shape.exit_kind();
            let mut diameter = shape.diameter();
            scale_hole_construction(&mut construction, scale)?;
            if let Some(exit_kind) = &mut exit_kind {
                scale_hole_kind(exit_kind, scale)?;
            }
            scale_optional_positive_length(&mut diameter, scale)?;
            *shape = cadmpeg_ir::features::HoleShape::new(construction, exit_kind, diameter)
                .map_err(CodecError::malformed)?;
            if let Some(extent) = extent {
                scale_linear_termination(extent, scale)?;
            }
        }
        FeatureDefinition::Pattern { pattern, .. } => scale_pattern_kind(pattern, scale)?,
        FeatureDefinition::PostProcess {
            operation,
            fuzzy_tolerance,
            ..
        } => {
            let mut definition = operation.as_ref().clone();
            scale_feature_definition(&mut definition, scale)?;
            *operation = definition.try_into().map_err(CodecError::malformed)?;
            scale_fuzzy_tolerance(fuzzy_tolerance, scale)?;
        }
        _ => {}
    }
    Ok(())
}

fn scale_fuzzy_tolerance(
    tolerance: &mut cadmpeg_ir::features::FuzzyTolerance,
    scale: f64,
) -> Result<(), CodecError> {
    if let cadmpeg_ir::features::FuzzyTolerance::Explicit(value) = tolerance {
        scale_positive_length(value, scale)?;
    }
    Ok(())
}

fn scale_primitive_solid(
    solid: &mut cadmpeg_ir::features::PrimitiveSolid,
    scale: f64,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::{PrimitiveSolid, PrimitiveSolidKind};

    let mut candidate = solid.kind().clone();

    match &mut candidate {
        PrimitiveSolidKind::Box {
            length,
            width,
            height,
        } => {
            scale_length(length, scale)?;
            scale_length(width, scale)?;
            scale_length(height, scale)?;
        }
        PrimitiveSolidKind::Cylinder { radius, height, .. } => {
            scale_length(radius, scale)?;
            scale_length(height, scale)?;
        }
        PrimitiveSolidKind::Cone {
            radius1,
            radius2,
            height,
            ..
        } => {
            scale_length(radius1, scale)?;
            scale_length(radius2, scale)?;
            scale_length(height, scale)?;
        }
        PrimitiveSolidKind::Sphere { radius, .. } => scale_length(radius, scale)?,
        PrimitiveSolidKind::Ellipsoid {
            x_radius,
            y_radius,
            z_radius,
            ..
        } => {
            scale_length(x_radius, scale)?;
            scale_length(y_radius, scale)?;
            scale_length(z_radius, scale)?;
        }
        PrimitiveSolidKind::Torus {
            major_radius,
            minor_radius,
            ..
        } => {
            scale_length(major_radius, scale)?;
            scale_length(minor_radius, scale)?;
        }
        PrimitiveSolidKind::Prism {
            circumradius,
            height,
            ..
        } => {
            scale_length(circumradius, scale)?;
            scale_length(height, scale)?;
        }
        PrimitiveSolidKind::Wedge {
            xmin,
            ymin,
            zmin,
            x2min,
            z2min,
            xmax,
            ymax,
            zmax,
            x2max,
            z2max,
        } => {
            for length in [
                xmin, ymin, zmin, x2min, z2min, xmax, ymax, zmax, x2max, z2max,
            ] {
                scale_length(length, scale)?;
            }
        }
    }
    *solid =
        PrimitiveSolid::new(candidate).map_err(|message| CodecError::Malformed(message.into()))?;
    Ok(())
}

fn scale_sweep_section(
    section: &mut cadmpeg_ir::features::SweepSection,
    scale: f64,
) -> Result<(), cadmpeg_core::CodecError> {
    if let cadmpeg_ir::features::SweepSection::Generated(
        cadmpeg_ir::features::GeneratedSweepSection::CircularRegion { region },
    ) = section
    {
        let mut outer_radius = region.outer_radius();
        let mut wall_thickness = region.wall_thickness();
        scale_positive_length(&mut outer_radius, scale)?;
        scale_optional_positive_length(&mut wall_thickness, scale)?;
        *region = cadmpeg_ir::features::SweepCircularRegion::new(outer_radius, wall_thickness)
            .map_err(CodecError::malformed)?;
    }
    Ok(())
}

fn scale_unit_plane_frame(
    frame: &mut cadmpeg_ir::features::FeatureUnitPlaneFrame,
    scale: f64,
) -> Result<(), CodecError> {
    let mut origin = frame.origin();
    scale_point3(&mut origin, scale);
    *frame =
        cadmpeg_ir::features::FeatureUnitPlaneFrame::new(origin, frame.u_axis(), frame.v_axis())
            .ok_or_else(|| {
                CodecError::Malformed("Creo scaled plane frame must have a finite origin".into())
            })?;
    Ok(())
}

fn scale_coil_construction(
    construction: &mut cadmpeg_ir::features::CoilConstruction,
    scale: f64,
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
    scale: f64,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::ExtrudeStart;

    match start {
        ExtrudeStart::OffsetProfilePlane { offset } => scale_length(offset, scale)?,
        ExtrudeStart::FromFace { offset, .. } => scale_optional_length(offset, scale)?,
        ExtrudeStart::Unresolved | ExtrudeStart::ProfilePlane => {}
    }
    Ok(())
}

fn scale_extrude_side(
    side: &mut cadmpeg_ir::features::ExtrudeSide,
    scale: f64,
) -> Result<(), cadmpeg_core::CodecError> {
    scale_linear_termination(&mut side.termination, scale)?;
    Ok(())
}

fn scale_extrude_extent(
    extent: &mut cadmpeg_ir::features::ExtrudeExtent,
    scale: f64,
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
    scale: f64,
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
    scale: f64,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::LinearTermination;

    match termination {
        LinearTermination::Blind { length } => scale_nonzero_length(length, scale)?,
        LinearTermination::ToFace { offset, .. } => scale_optional_length(offset, scale)?,
        LinearTermination::OffsetFromFace { offset, .. } => scale_positive_length(offset, scale)?,
        LinearTermination::Unresolved
        | LinearTermination::ThroughAll
        | LinearTermination::ThroughNext
        | LinearTermination::ToFirst
        | LinearTermination::ToLast
        | LinearTermination::ToVertex { .. }
        | LinearTermination::ToShape { .. } => {}
    }
    Ok(())
}

fn scale_angular_termination(
    termination: &mut cadmpeg_ir::features::AngularTermination,
    scale: f64,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::AngularTermination;

    match termination {
        AngularTermination::ToFace { offset, .. } => scale_optional_length(offset, scale)?,
        AngularTermination::OffsetFromFace { offset, .. } => scale_positive_length(offset, scale)?,
        AngularTermination::Unresolved
        | AngularTermination::ThroughAll
        | AngularTermination::ThroughNext
        | AngularTermination::ToFirst
        | AngularTermination::ToLast
        | AngularTermination::ToVertex { .. }
        | AngularTermination::ToShape { .. }
        | AngularTermination::Angle { .. } => {}
    }
    Ok(())
}

fn scale_sheet_metal_flange_height(
    height: &mut cadmpeg_ir::features::SheetMetalFlangeHeight,
    scale: f64,
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
    scale: f64,
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
    scale: f64,
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
    radius: &mut cadmpeg_ir::features::RadiusSpec,
    scale: f64,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::RadiusSpec;

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
            let mut scaled = points.as_slice().to_vec();
            for point in &mut scaled {
                scale_length(&mut point.radius, scale)?;
            }
            *points = cadmpeg_ir::features::VariableRadii::new(scaled)
                .map_err(|message| CodecError::Malformed(message.into()))?;
        }
        RadiusSpec::Unresolved
        | RadiusSpec::UnresolvedConstant
        | RadiusSpec::UnresolvedChordal
        | RadiusSpec::UnresolvedAsymmetric
        | RadiusSpec::UnresolvedVariable => {}
    }
    Ok(())
}

fn scale_chamfer_spec(
    spec: &mut cadmpeg_ir::features::ChamferSpec,
    scale: f64,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::ChamferSpec;

    match spec {
        ChamferSpec::Distance { distance } | ChamferSpec::DistanceAngle { distance, .. } => {
            scale_positive_length(distance, scale)?;
        }
        ChamferSpec::TwoDistances { first, second } => {
            scale_positive_length(first, scale)?;
            scale_positive_length(second, scale)?;
        }
        ChamferSpec::Unresolved
        | ChamferSpec::UnresolvedDistance
        | ChamferSpec::UnresolvedTwoDistances
        | ChamferSpec::UnresolvedDistanceAngle => {}
    }
    Ok(())
}

fn scale_ruled_surface_mode(
    mode: &mut cadmpeg_ir::features::RuledSurfaceMode,
    scale: f64,
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
    scale: f64,
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
    scale: f64,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::FlexMode;

    match mode {
        FlexMode::Unresolved(_) => {}
        FlexMode::Stretching { distance } => scale_length(distance, scale)?,
        FlexMode::Bending { .. } | FlexMode::Twisting { .. } | FlexMode::Tapering { .. } => {}
    }
    Ok(())
}

fn scale_hole_placement(
    placement: &mut cadmpeg_ir::features::HolePlacement,
    scale: f64,
) -> Result<(), CodecError> {
    match placement {
        cadmpeg_ir::features::HolePlacement::Directed { position, .. }
        | cadmpeg_ir::features::HolePlacement::Axis {
            origin: position, ..
        } => scale_finite_point3(position, scale),
    }
}

fn scale_hole_kind(
    kind: &mut cadmpeg_ir::features::HoleKind,
    scale: f64,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::HoleKind;

    match kind {
        HoleKind::Unresolved(_) => {}
        HoleKind::PartialCounterbore { diameter, depth } => {
            scale_optional_positive_length(diameter, scale)?;
            scale_optional_positive_length(depth, scale)?;
        }
        HoleKind::PartialCountersink { diameter, .. } => {
            scale_optional_positive_length(diameter, scale)?;
        }
        HoleKind::Chamfer { diameter, .. } | HoleKind::Countersink { diameter, .. } => {
            scale_positive_length(diameter, scale)?;
        }
        HoleKind::Counterbore { diameter, depth }
        | HoleKind::CounterboreDrilled {
            diameter, depth, ..
        } => {
            scale_positive_length(diameter, scale)?;
            scale_positive_length(depth, scale)?;
        }
        HoleKind::Counterdrill {
            diameters, depth, ..
        } => {
            let mut diameter = diameters.diameter();
            let mut entry_diameter = diameters.entry_diameter();
            scale_positive_length(&mut diameter, scale)?;
            scale_optional_positive_length(&mut entry_diameter, scale)?;
            *diameters = cadmpeg_ir::features::CounterdrillDiameters::new(diameter, entry_diameter)
                .map_err(CodecError::malformed)?;
            scale_positive_length(depth, scale)?;
        }
        HoleKind::Simple | HoleKind::SimpleDrilled { .. } => {}
    }
    Ok(())
}

fn scale_hole_construction(
    construction: &mut cadmpeg_ir::features::HoleConstruction,
    scale: f64,
) -> Result<(), cadmpeg_core::CodecError> {
    match construction {
        cadmpeg_ir::features::HoleConstruction::Form {
            kind,
            specification,
        } => {
            scale_hole_kind(kind, scale)?;
            if let Some(specification) = specification {
                scale_hole_specification(specification, scale)?;
            }
        }
        cadmpeg_ir::features::HoleConstruction::NativeThread {
            major_diameter,
            thread_depth,
            pitch,
            ..
        } => {
            scale_positive_length(major_diameter, scale)?;
            scale_positive_length(thread_depth, scale)?;
            scale_optional_positive_length(pitch, scale)?;
        }
    }
    Ok(())
}

fn scale_hole_specification(
    specification: &mut cadmpeg_ir::features::HoleSpecification,
    scale: f64,
) -> Result<(), cadmpeg_core::CodecError> {
    let (pitch, major_diameter, clearance, depth) = match specification {
        cadmpeg_ir::features::HoleSpecification::Clearance {
            clearance, depth, ..
        } => (None, None, clearance, depth),
        cadmpeg_ir::features::HoleSpecification::Threaded {
            pitch,
            major_diameter,
            clearance,
            depth,
            ..
        } => (Some(pitch), Some(major_diameter), clearance, depth),
    };
    if let Some(pitch) = pitch {
        scale_optional_positive_length(pitch, scale)?;
    }
    if let Some(major_diameter) = major_diameter {
        scale_optional_positive_length(major_diameter, scale)?;
    }
    scale_optional_length(clearance, scale)?;
    if let cadmpeg_ir::features::HoleThreadDepth::Blind { depth } = depth {
        scale_positive_length(depth, scale)?;
    }
    Ok(())
}

fn scale_pattern_kind(
    pattern: &mut cadmpeg_ir::features::PatternKind,
    scale: f64,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::PatternTransform;

    let mut transform = pattern.definition().clone();
    match &mut transform {
        PatternTransform::Linear {
            spacing, second, ..
        } => {
            scale_length(spacing, scale)?;
            if let Some(second) = second {
                scale_length(&mut second.spacing, scale)?;
            }
        }
        PatternTransform::LinearOffsets { offsets, .. } => {
            for offset in offsets {
                scale_length(offset, scale)?;
            }
        }
        PatternTransform::CurveDriven { spacing, .. } => scale_length(spacing, scale)?,
        PatternTransform::Circular { axis_origin, .. } => scale_point3(axis_origin, scale),
        PatternTransform::CircularAngles { axis_origin, .. } => scale_point3(axis_origin, scale),
        PatternTransform::Mirror { plane_origin, .. } => scale_point3(plane_origin, scale),
        PatternTransform::Composite { stages } => {
            for stage in stages {
                scale_pattern_kind(&mut stage.pattern, scale)?;
            }
        }
        PatternTransform::Scale { center, .. } => {
            if let cadmpeg_ir::features::PatternScaleCenter::Point(point) = center {
                scale_point3(point, scale);
            }
        }
        PatternTransform::Unresolved
        | PatternTransform::UnresolvedLinear
        | PatternTransform::UnresolvedCircular
        | PatternTransform::UnresolvedCurveDriven
        | PatternTransform::UnresolvedMirror
        | PatternTransform::UnresolvedScale
        | PatternTransform::UnresolvedComposite
        | PatternTransform::MirrorReference { .. } => {}
    }
    *pattern = cadmpeg_ir::features::PatternKind::new(transform)
        .map_err(|message| CodecError::Malformed(message.into()))?;
    Ok(())
}

fn scale_surface_geometry(geometry: &mut SurfaceGeometry, scale: f64) -> Result<(), CodecError> {
    match geometry {
        SurfaceGeometry::Plane(plane_surface) => {
            let origin = plane_surface.origin();
            let normal = plane_surface.normal();
            let u_axis = plane_surface.u_axis();
            *plane_surface = cadmpeg_ir::geometry::PlaneSurface::try_new(
                Point3::new(origin.x * scale, origin.y * scale, origin.z * scale),
                *normal,
                *u_axis,
            )
            .map_err(CodecError::malformed)?;
        }
        SurfaceGeometry::Cylinder(cylinder_surface) => {
            let origin = cylinder_surface.origin();
            let axis = cylinder_surface.axis();
            let ref_direction = cylinder_surface.ref_direction();
            let radius = &cylinder_surface.radius();
            *cylinder_surface = cadmpeg_ir::geometry::CylinderSurface::try_new(
                Point3::new(origin.x * scale, origin.y * scale, origin.z * scale),
                *axis,
                *ref_direction,
                *radius * scale,
            )
            .map_err(CodecError::malformed)?;
        }
        SurfaceGeometry::Cone(cone_surface) => {
            let origin = cone_surface.origin();
            let axis = cone_surface.axis();
            let ref_direction = cone_surface.ref_direction();
            let radius = &cone_surface.radius();
            let ratio = &cone_surface.ratio();
            let half_angle = &cone_surface.half_angle();
            *cone_surface = cadmpeg_ir::geometry::ConeSurface::try_new(
                Point3::new(origin.x * scale, origin.y * scale, origin.z * scale),
                *axis,
                *ref_direction,
                *radius * scale,
                *ratio,
                *half_angle,
            )
            .map_err(CodecError::malformed)?;
        }
        SurfaceGeometry::Sphere(sphere_surface) => {
            let center = sphere_surface.center();
            let axis = sphere_surface.axis();
            let ref_direction = sphere_surface.ref_direction();
            let radius = &sphere_surface.radius();
            *sphere_surface = cadmpeg_ir::geometry::SphereSurface::try_new(
                Point3::new(center.x * scale, center.y * scale, center.z * scale),
                *axis,
                *ref_direction,
                *radius * scale,
            )
            .map_err(CodecError::malformed)?;
        }
        SurfaceGeometry::Torus(torus_surface) => {
            let center = torus_surface.center();
            let axis = torus_surface.axis();
            let ref_direction = torus_surface.ref_direction();
            let major_radius = &torus_surface.major_radius();
            let minor_radius = &torus_surface.minor_radius();
            *torus_surface = cadmpeg_ir::geometry::TorusSurface::try_new(
                Point3::new(center.x * scale, center.y * scale, center.z * scale),
                *axis,
                *ref_direction,
                *major_radius * scale,
                *minor_radius * scale,
            )
            .map_err(CodecError::malformed)?;
        }
        SurfaceGeometry::Nurbs(surface) => {
            surface
                .edit_control_points(|points| {
                    for point in points {
                        scale_point3(point, scale);
                    }
                })
                .map_err(|error| {
                    CodecError::malformed(format_args!(
                        "Creo surface unit normalization produced invalid NURBS control points: {error}"
                    ))
                })?;
        }
        SurfaceGeometry::Polygonal(surface) => {
            let mut scaled = surface.clone();
            scaled
                .edit_vertices(|points| {
                    for point in points {
                        scale_point3(point, scale);
                    }
                })
                .map_err(|error| CodecError::malformed(error.to_string()))?;
            scaled
                .set_chordal_deflection(scaled.chordal_deflection() * scale)
                .map_err(|error| CodecError::malformed(error.to_string()))?;
            *surface = scaled;
        }
        SurfaceGeometry::Transformed {
            basis, transform, ..
        } => {
            scale_surface_geometry(basis, scale)?;
            scale_transform_translation(transform, scale);
        }
        SurfaceGeometry::Procedural { .. } | SurfaceGeometry::Unknown { .. } => {}
    }
    Ok(())
}

fn scale_curve_geometry(geometry: &mut CurveGeometry, scale: f64) -> Result<(), CodecError> {
    match geometry {
        CurveGeometry::Line(line_curve) => {
            let origin = line_curve.origin();
            let direction = line_curve.direction();
            *line_curve = cadmpeg_ir::geometry::LineCurve::try_new(
                Point3::new(origin.x * scale, origin.y * scale, origin.z * scale),
                *direction,
            )
            .map_err(CodecError::malformed)?;
        }
        CurveGeometry::Circle(circle_curve) => {
            let center = circle_curve.center();
            let axis = circle_curve.axis();
            let ref_direction = circle_curve.ref_direction();
            let radius = &circle_curve.radius();
            *circle_curve = cadmpeg_ir::geometry::CircleCurve::try_new(
                Point3::new(center.x * scale, center.y * scale, center.z * scale),
                *axis,
                *ref_direction,
                *radius * scale,
            )
            .map_err(CodecError::malformed)?;
        }
        CurveGeometry::Ellipse(ellipse_curve) => {
            let center = ellipse_curve.center();
            let axis = ellipse_curve.axis();
            let major_direction = ellipse_curve.major_direction();
            let major_radius = &ellipse_curve.major_radius();
            let minor_radius = &ellipse_curve.minor_radius();
            *ellipse_curve = cadmpeg_ir::geometry::EllipseCurve::try_new(
                Point3::new(center.x * scale, center.y * scale, center.z * scale),
                *axis,
                *major_direction,
                *major_radius * scale,
                *minor_radius * scale,
            )
            .map_err(CodecError::malformed)?;
        }
        CurveGeometry::Parabola(parabola_curve) => {
            let vertex = parabola_curve.vertex();
            let axis = parabola_curve.axis();
            let major_direction = parabola_curve.major_direction();
            let focal_distance = &parabola_curve.focal_distance();
            *parabola_curve = cadmpeg_ir::geometry::ParabolaCurve::try_new(
                Point3::new(vertex.x * scale, vertex.y * scale, vertex.z * scale),
                *axis,
                *major_direction,
                *focal_distance * scale,
            )
            .map_err(CodecError::malformed)?;
        }
        CurveGeometry::Hyperbola(hyperbola_curve) => {
            let center = hyperbola_curve.center();
            let axis = hyperbola_curve.axis();
            let major_direction = hyperbola_curve.major_direction();
            let major_radius = &hyperbola_curve.major_radius();
            let minor_radius = &hyperbola_curve.minor_radius();
            *hyperbola_curve = cadmpeg_ir::geometry::HyperbolaCurve::try_new(
                Point3::new(center.x * scale, center.y * scale, center.z * scale),
                *axis,
                *major_direction,
                *major_radius * scale,
                *minor_radius * scale,
            )
            .map_err(CodecError::malformed)?;
        }
        CurveGeometry::Degenerate(degenerate_curve) => {
            let point = degenerate_curve.point();
            *degenerate_curve = cadmpeg_ir::geometry::DegenerateCurve::try_new(Point3::new(
                point.x * scale,
                point.y * scale,
                point.z * scale,
            ))
            .map_err(CodecError::malformed)?;
        }
        CurveGeometry::Nurbs(curve) => {
            curve
                .edit_control_points(|points| {
                    for point in points {
                        scale_point3(point, scale);
                    }
                })
                .map_err(|error| {
                    CodecError::malformed(format_args!(
                        "Creo curve unit normalization produced invalid NURBS control points: {error}"
                    ))
                })?;
        }
        CurveGeometry::Polyline(polyline) => {
            let mut scaled = polyline.clone();
            scaled
                .edit_points(|points| {
                    for point in points {
                        scale_point3(point, scale);
                    }
                })
                .map_err(|error| CodecError::malformed(error.to_string()))?;
            scaled
                .set_chordal_deflection(scaled.chordal_deflection() * scale)
                .map_err(|error| CodecError::malformed(error.to_string()))?;
            *polyline = scaled;
        }
        CurveGeometry::Transformed {
            basis, transform, ..
        } => {
            scale_curve_geometry(basis, scale)?;
            scale_transform_translation(transform, scale);
        }
        CurveGeometry::Composite { .. }
        | CurveGeometry::Procedural { .. }
        | CurveGeometry::Unknown { .. } => {}
    }
    Ok(())
}

fn scale_procedural_surface_definition(
    definition: &mut cadmpeg_ir::geometry::ProceduralSurfaceDefinition,
    scale: f64,
) {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    match definition {
        ProceduralSurfaceDefinition::Extrusion {
            direction,
            native_position,
            ..
        } => {
            scale_vector3(direction, scale);
            if let Some(position) = native_position {
                scale_point3(position, scale);
            }
        }
        ProceduralSurfaceDefinition::LinearSweep { direction, .. } => {
            scale_vector3(direction, scale);
        }
        ProceduralSurfaceDefinition::Revolution { axis_origin, .. }
        | ProceduralSurfaceDefinition::AxisRevolution { axis_origin, .. } => {
            scale_point3(axis_origin, scale);
        }
        ProceduralSurfaceDefinition::Sum { basepoint, .. } => {
            scale_vector3(basepoint, scale);
        }
        _ => {}
    }
}

fn scale_procedural_curve_definition(
    definition: &mut cadmpeg_ir::geometry::ProceduralCurveDefinition,
    scale: f64,
) -> Result<(), &'static str> {
    if let cadmpeg_ir::geometry::ProceduralCurveDefinition::Helix(helix) = definition {
        helix.try_scale_lengths(scale)?;
    }
    Ok(())
}

fn curve_parameter_scale(geometry: &CurveGeometry, length_scale_mm: f64) -> Option<f64> {
    match geometry {
        CurveGeometry::Line(_) => Some(length_scale_mm),
        CurveGeometry::Circle(_) => Some(1.0),
        CurveGeometry::Ellipse(_) => Some(1.0),
        CurveGeometry::Parabola(_) => Some(1.0),
        CurveGeometry::Hyperbola(_) => Some(1.0),
        CurveGeometry::Transformed { basis, .. } => curve_parameter_scale(basis, length_scale_mm),
        CurveGeometry::Nurbs { .. } => None,
        CurveGeometry::Degenerate(_) => None,
        CurveGeometry::Composite { .. } => None,
        CurveGeometry::Procedural { .. } => None,
        CurveGeometry::Polyline(_) => None,
        CurveGeometry::Unknown { .. } => None,
    }
}

fn surface_parameter_scales(geometry: &SurfaceGeometry, length_scale_mm: f64) -> [f64; 2] {
    match geometry {
        SurfaceGeometry::Plane(_) => [length_scale_mm, length_scale_mm],
        SurfaceGeometry::Cylinder(_) => [1.0, length_scale_mm],
        SurfaceGeometry::Cone(_) => [1.0, length_scale_mm],
        SurfaceGeometry::Sphere(_) => [1.0, 1.0],
        SurfaceGeometry::Torus(_) => [1.0, 1.0],
        SurfaceGeometry::Transformed { basis, .. } => {
            surface_parameter_scales(basis, length_scale_mm)
        }
        SurfaceGeometry::Nurbs { .. }
        | SurfaceGeometry::Procedural { .. }
        | SurfaceGeometry::Polygonal(_)
        | SurfaceGeometry::Unknown { .. } => [1.0, 1.0],
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
        let scales = surface_parameter_scales(&surface.geometry, length_scale_mm);
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
        let scales = surface_parameter_scales(&surface.geometry, length_scale_mm);
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

fn scale_sketch_geometry(geometry: &mut SketchGeometry, scale: f64) -> Result<(), CodecError> {
    let mut definition = geometry.definition().clone();
    match &mut definition {
        SketchGeometryDefinition::Point { position } => scale_point2(position, scale),
        SketchGeometryDefinition::Line { start, end } => {
            scale_point2(start, scale);
            scale_point2(end, scale);
        }
        SketchGeometryDefinition::ReferenceLine { origin, .. } => scale_point2(origin, scale),
        SketchGeometryDefinition::Circle { center, radius } => {
            scale_point2(center, scale);
            scale_length(radius, scale)?;
        }
        SketchGeometryDefinition::Arc { center, radius, .. } => {
            scale_point2(center, scale);
            scale_length(radius, scale)?;
        }
        SketchGeometryDefinition::Ellipse {
            center,
            major_radius,
            minor_radius,
            ..
        }
        | SketchGeometryDefinition::Hyperbola {
            center,
            major_radius,
            minor_radius,
            ..
        } => {
            scale_point2(center, scale);
            scale_length(major_radius, scale)?;
            scale_length(minor_radius, scale)?;
        }
        SketchGeometryDefinition::Parabola {
            vertex,
            focal_length,
            ..
        } => {
            scale_point2(vertex, scale);
            scale_length(focal_length, scale)?;
        }
        SketchGeometryDefinition::Nurbs { curve } => {
            curve
                .edit_control_points(|points| {
                    for point in points {
                        scale_point2(point, scale);
                    }
                })
                .map_err(|error| {
                    CodecError::malformed(format_args!(
                        "Creo sketch unit normalization produced invalid NURBS control points: {error}"
                    ))
                })?;
        }
        SketchGeometryDefinition::Text {
            height, placement, ..
        } => {
            scale_length(height, scale)?;
            if let Some(placement) = placement {
                scale_point2(&mut placement.anchor, scale);
            }
        }
        SketchGeometryDefinition::ExternalReference { .. }
        | SketchGeometryDefinition::Native { .. } => {}
    }
    *geometry = definition.try_into().map_err(CodecError::malformed)?;
    Ok(())
}

fn scale_spatial_sketch_geometry(
    geometry: &mut SpatialSketchGeometry,
    scale: f64,
) -> Result<(), CodecError> {
    let mut definition = geometry.definition().clone();
    match &mut definition {
        SpatialSketchGeometryDefinition::Point { position } => scale_point3(position, scale),
        SpatialSketchGeometryDefinition::Line { start, end } => {
            scale_point3(start, scale);
            scale_point3(end, scale);
        }
        SpatialSketchGeometryDefinition::Circle { center, radius, .. }
        | SpatialSketchGeometryDefinition::Arc { center, radius, .. } => {
            scale_point3(center, scale);
            scale_length(radius, scale)?;
        }
        SpatialSketchGeometryDefinition::Nurbs { curve } => {
            curve
                .edit_control_points(|points| {
                    for point in points {
                        scale_point3(point, scale);
                    }
                })
                .map_err(|error| {
                    CodecError::malformed(format_args!(
                        "Creo spatial sketch unit normalization produced invalid NURBS control points: {error}"
                    ))
                })?;
        }
        SpatialSketchGeometryDefinition::NurbsSurface { surface } => {
            surface
                .edit_control_points(|point| scale_point3(point, scale))
                .map_err(|error| {
                    CodecError::malformed(format_args!(
                        "Creo spatial sketch unit normalization produced invalid B-spline control points: {error}"
                    ))
                })?;
        }
        SpatialSketchGeometryDefinition::Native { .. } => {}
    }
    *geometry = definition.try_into().map_err(CodecError::malformed)?;
    Ok(())
}

fn scale_sketch_constraint_definition(
    definition: &mut cadmpeg_ir::sketches::SketchConstraintDefinitionInput,
    scale: f64,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::sketches::SketchConstraintDefinitionInput;

    match definition {
        SketchConstraintDefinitionInput::PointCoordinateValues { values, .. } => {
            for value in values {
                scale_length(value, scale)?;
            }
        }
        SketchConstraintDefinitionInput::MidpointCoordinate { value, .. }
        | SketchConstraintDefinitionInput::DistanceLociValue {
            distance: value, ..
        }
        | SketchConstraintDefinitionInput::PolarDistance {
            distance: value, ..
        }
        | SketchConstraintDefinitionInput::Offset {
            distance: value, ..
        } => scale_length(value, scale)?,
        _ => {}
    }
    Ok(())
}

fn scale_spatial_sketch_constraint_definition(
    definition: &mut cadmpeg_ir::sketches::SpatialSketchConstraintDefinitionInput,
    scale: f64,
) -> Result<(), cadmpeg_core::CodecError> {
    if let cadmpeg_ir::sketches::SpatialSketchConstraintDefinitionInput::Offset {
        distance, ..
    } = definition
    {
        scale_length(distance, scale)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_ir::geometry::PcurveGeometry;

    const EPS_UNIT_SCALE: f64 = f64::EPSILON * 4096.0;

    use cadmpeg_ir::features::{
        BooleanOp, ExtrudeDirection, ExtrudeExtent, ExtrudeSide, ExtrudeStart, FaceMotion, Feature,
        FeatureDefinition, FuzzyTolerance, LinearTermination, PatternKind, PatternScaleCenter,
        PatternTransform, ProfileRef,
    };

    #[test]
    fn scales_model_geometry_and_feature_dimensions() {
        let mut ir = CadIr::empty();
        ir.model.features.push(Feature::new(
            cadmpeg_ir::features::FeatureId::mint("synthetic:test:id#feature")
                .expect("identity grammar"),
            0,
            FeatureDefinition::Extrude {
                profile: ProfileRef::Unresolved("profile".into()),
                direction: ExtrudeDirection::ProfileNormal,
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
            },
        ));
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
        normalize_model_lengths(&mut ir, 25.4).expect("valid unit scaling");

        let FeatureDefinition::Extrude { start, extent, .. } =
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

    #[test]
    fn rejects_nurbs_unit_overflow_without_committing_nonfinite_poles() {
        let curve_id = cadmpeg_ir::ids::CurveId::mint("test:model:entity#overflow-curve")
            .expect("identity grammar");
        let curve = cadmpeg_ir::geometry::NurbsCurve::new(
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
            geometry: CurveGeometry::Nurbs(curve),
            source_object: None,
        });

        let error = normalize_model_lengths(&mut ir, 25.4).expect_err("overflow must refuse");
        assert!(matches!(error, CodecError::Malformed(_)));
        let CurveGeometry::Nurbs(curve) = &ir.model.curves[0].geometry else {
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
        scale_face_motion(&mut translate, 25.4).expect("valid test fixture");
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
        scale_face_motion(&mut rotate, 25.4).expect("valid test fixture");
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
        let mut pattern = PatternKind::new(PatternTransform::Scale {
            center: PatternScaleCenter::Point(Point3::new(1.0, 2.0, 3.0)),
            final_factor: 2.0,
            count: 3,
        })
        .expect("valid test fixture");
        scale_pattern_kind(&mut pattern, 25.4).expect("valid test fixture");
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
        assert_point3(point, [25.4, 50.8, 76.2]);
        assert_close(final_factor, 2.0);
        assert_eq!(count, 3);
    }

    #[test]
    fn scales_explicit_fuzzy_tolerance() {
        let mut definition = FeatureDefinition::PostProcess {
            operation: FeatureDefinition::Native {
                kind: "Boolean".into(),
                parameters: BTreeMap::new(),
            }
            .try_into()
            .expect("valid test fixture"),
            refine: false,
            fuzzy_tolerance: FuzzyTolerance::Explicit(
                cadmpeg_ir::scalar::PositiveLength::new(2.0).expect("positive length fixture"),
            ),
        };

        scale_feature_definition(&mut definition, 25.4).expect("valid test fixture");

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
            geometry: cadmpeg_ir::geometry::SurfaceGeometry::Unknown { record: None },
            source_object: None,
        });
        let surface = cadmpeg_ir::geometry::ProceduralSurface::try_new(
            cadmpeg_ir::ids::ProceduralSurfaceId::mint("test:model:entity#surface-construction")
                .expect("identity grammar"),
            cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Extrusion {
                directrix: cadmpeg_ir::ids::CurveId::mint("test:model:entity#directrix")
                    .expect("identity grammar"),
                parameter_interval: Some([1.0, 2.0]),
                direction: Vector3::new(1.0, 2.0, 3.0),
                native_position: Some(Point3::new(4.0, 5.0, 6.0)),
                revision_form: None,
            },
            Some(7.0),
            Some([Some(8.0), None, Some(9.0), None]),
        )
        .unwrap();
        ir.model
            .add_procedural_surface(surface_id, surface)
            .unwrap();
        let curve_id =
            cadmpeg_ir::ids::CurveId::mint("test:model:entity#curve").expect("identity grammar");
        ir.model.curves.push(cadmpeg_ir::geometry::Curve {
            id: curve_id.clone(),
            geometry: cadmpeg_ir::geometry::CurveGeometry::Unknown { record: None },
            source_object: None,
        });
        let curve = cadmpeg_ir::geometry::ProceduralCurve::try_new(
            cadmpeg_ir::ids::ProceduralCurveId::mint("test:model:entity#curve-construction")
                .expect("identity grammar"),
            cadmpeg_ir::geometry::ProceduralCurveDefinition::Helix(
                cadmpeg_ir::geometry::HelixCurveConstruction::try_new(
                    [0.0, 1.0],
                    Point3::new(1.0, 2.0, 3.0),
                    Vector3::new(4.0, 5.0, 6.0),
                    Vector3::new(-5.0, 4.0, 6.0),
                    Vector3::new(10.0, 11.0, 12.0),
                    0.25,
                    Vector3::new(0.0, 0.0, 1.0),
                )
                .unwrap(),
            ),
            Some(13.0),
        )
        .unwrap();
        ir.model.add_procedural_curve(curve_id, curve).unwrap();

        normalize_model_lengths(&mut ir, 25.4).expect("valid unit scaling");

        let surface = &ir.model.procedural_surfaces[0];
        let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Extrusion {
            direction,
            native_position,
            parameter_interval,
            ..
        } = surface.definition()
        else {
            panic!("test surface construction changed family");
        };
        assert_vector3(*direction, [25.4, 50.8, 76.2]);
        assert_point3(
            *native_position.as_ref().expect("test native position"),
            [101.6, 127.0, 152.4],
        );
        assert_eq!(*parameter_interval, Some([1.0, 2.0]));
        assert_close(
            surface
                .cache_fit_tolerance()
                .expect("test surface tolerance"),
            177.8,
        );
        assert_eq!(
            surface.record_bounds,
            Some([Some(8.0), None, Some(9.0), None])
        );

        let curve = &ir.model.procedural_curves[0];
        let cadmpeg_ir::geometry::ProceduralCurveDefinition::Helix(helix_payload) =
            curve.definition()
        else {
            panic!("test curve construction changed family");
        };
        let center = helix_payload.center();
        let major = helix_payload.major();
        let minor = helix_payload.minor();
        let pitch = helix_payload.pitch();
        let apex_factor = &helix_payload.apex_factor();
        let axis = helix_payload.axis();

        assert_point3(*center, [25.4, 50.8, 76.2]);
        assert_vector3(*major, [101.6, 127.0, 152.4]);
        assert_vector3(*minor, [-127.0, 101.6, 152.4]);
        assert_vector3(*pitch, [254.0, 279.4, 304.8]);
        assert_eq!(*axis, Vector3::new(0.0, 0.0, 1.0));
        assert_close(*apex_factor, 0.25);
        assert_close(
            curve.cache_fit_tolerance().expect("test curve tolerance"),
            330.2,
        );
    }

    #[test]
    fn scales_pcurve_coordinates_per_surface_axis() {
        let mut geometry = PcurveGeometry::Line(
            cadmpeg_ir::geometry::LinePcurve::try_new(Point2::new(1.0, 2.0), Point2::new(3.0, 4.0))
                .expect("valid LinePcurve fixture"),
        );

        assert!(geometry.try_scale_coordinates([25.4, 1.0]).is_ok());
        let PcurveGeometry::Line(line_pcurve) = geometry else {
            panic!("test pcurve changed family");
        };
        let origin = line_pcurve.origin();
        let direction = line_pcurve.direction();
        assert_point2(*origin, [25.4, 2.0]);
        assert_point2(*direction, [76.2, 4.0]);
    }

    #[test]
    fn scales_analytic_surface_and_curve_without_scaling_directions() {
        let mut surface = SurfaceGeometry::Cylinder(
            cadmpeg_ir::geometry::CylinderSurface::try_new(
                Point3::new(1.0, 2.0, 3.0),
                cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
                4.0,
            )
            .expect("valid CylinderSurface fixture"),
        );
        let mut curve = CurveGeometry::Circle(
            cadmpeg_ir::geometry::CircleCurve::try_new(
                Point3::new(2.0, 3.0, 4.0),
                cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
                5.0,
            )
            .expect("valid CircleCurve fixture"),
        );

        scale_surface_geometry(&mut surface, 25.4).expect("finite surface scaling");
        scale_curve_geometry(&mut curve, 25.4).expect("finite curve scaling");

        let SurfaceGeometry::Cylinder(cylinder_surface) = surface else {
            panic!("test surface changed family");
        };
        let origin = cylinder_surface.origin();
        let axis = cylinder_surface.axis();
        let radius = &cylinder_surface.radius();
        assert_point3(*origin, [25.4, 50.8, 76.2]);
        assert_eq!(*axis, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0));
        assert_close(*radius, 101.6);
        let CurveGeometry::Circle(circle_curve) = curve else {
            panic!("test curve changed family");
        };
        let center = circle_curve.center();
        let axis = circle_curve.axis();
        let radius = &circle_curve.radius();
        assert_point3(*center, [50.8, 76.2, 101.6]);
        assert_eq!(*axis, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0));
        assert_close(*radius, 127.0);
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
}
