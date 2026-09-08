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
use cadmpeg_ir::features::{FeatureDefinition, Length, ParameterValue, WrapMode};
use cadmpeg_ir::geometry::{CurveGeometry, PcurveGeometry, SurfaceGeometry};
use cadmpeg_ir::ids::PcurveId;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{SketchGeometry, SketchPlacement, SpatialSketchGeometry};
use cadmpeg_ir::transform::{Transform, Transform2};

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
            if !scale_pcurve_geometry(&mut pcurve.geometry, *scales) {
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
        procedural.edit_definition(|definition| {
            scale_procedural_surface_definition(definition, length_scale_mm);
        });
        procedural.scale_cache_fit_tolerance(length_scale_mm);
    }
    for procedural in &mut ir.model.procedural_curves {
        procedural.edit_definition(|definition| {
            scale_procedural_curve_definition(definition, length_scale_mm);
        });
        procedural.scale_cache_fit_tolerance(length_scale_mm);
    }
    for point in &mut ir.model.points {
        scale_point3(&mut point.position, length_scale_mm);
    }
    for face in &mut ir.model.faces {
        scale_optional(&mut face.tolerance, length_scale_mm);
    }
    for vertex in &mut ir.model.vertices {
        scale_optional(&mut vertex.tolerance, length_scale_mm);
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
        scale_optional(&mut edge.tolerance, length_scale_mm);
        if let (Some(range), Some(scale)) = (
            edge.param_range.as_mut(),
            edge.curve
                .as_ref()
                .and_then(|id| curve_parameter_scales.get(id)),
        ) {
            scale_pair(range, *scale);
        }
    }
    for coedge in &mut ir.model.coedges {
        if let Some(use_curve) = &mut coedge.use_curve {
            if let Some(scale) = curve_parameter_scales.get(&use_curve.curve) {
                scale_pair(&mut use_curve.parameter_range, *scale);
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
        for vertex in tessellation.vertices_mut() {
            scale_point3(vertex, length_scale_mm);
        }
        scale_optional(&mut tessellation.chordal_deflection, length_scale_mm);
    }
    for feature in &mut ir.model.features {
        scale_feature_definition(&mut feature.definition, length_scale_mm)?;
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
        if let SketchPlacement::Resolved { origin, .. } = &mut sketch.placement {
            scale_point3(origin, length_scale_mm);
        }
    }
    for entity in &mut ir.model.sketch_entities {
        scale_sketch_geometry(&mut entity.geometry, length_scale_mm)?;
    }
    for sketch in &mut ir.model.spatial_sketches {
        for profile in &mut sketch.profiles {
            scale_point3(&mut profile.origin, length_scale_mm);
        }
    }
    for entity in &mut ir.model.spatial_sketch_entities {
        scale_spatial_sketch_geometry(&mut entity.geometry, length_scale_mm)?;
    }
    for constraint in &mut ir.model.sketch_constraints {
        scale_sketch_constraint_definition(&mut constraint.definition, length_scale_mm)?;
    }
    for constraint in &mut ir.model.spatial_sketch_constraints {
        scale_spatial_sketch_constraint_definition(&mut constraint.definition, length_scale_mm)?;
    }
    Ok(())
}

fn scale_optional(value: &mut Option<f64>, scale: f64) {
    if let Some(value) = value.as_mut() {
        *value *= scale;
    }
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
    length: &mut cadmpeg_ir::features::PositiveLength,
    scale: f64,
) -> Result<(), CodecError> {
    *length = cadmpeg_ir::features::PositiveLength::new(length.get() * scale).ok_or_else(|| {
        CodecError::Malformed("Creo scaled length must be positive and finite".into())
    })?;
    Ok(())
}

fn scale_nonzero_length(
    length: &mut cadmpeg_ir::features::NonZeroLength,
    scale: f64,
) -> Result<(), CodecError> {
    *length = cadmpeg_ir::features::NonZeroLength::new(length.get() * scale)
        .ok_or_else(|| CodecError::malformed("Creo scaled length must be finite and nonzero"))?;
    Ok(())
}

fn scale_nonnegative_length(
    length: &mut cadmpeg_ir::features::NonNegativeLength,
    scale: f64,
) -> Result<(), CodecError> {
    *length =
        cadmpeg_ir::features::NonNegativeLength::new(length.get() * scale).ok_or_else(|| {
            CodecError::malformed("Creo scaled length must be nonnegative and finite")
        })?;
    Ok(())
}

fn scale_optional_positive_length(
    length: &mut Option<cadmpeg_ir::features::PositiveLength>,
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
    };
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
        FeatureDefinition::Sweep {
            section, sections, ..
        } => {
            scale_sweep_section(section, scale)?;
            for section in sections {
                scale_sweep_section(section, scale)?;
            }
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
            scale_optional_positive_length(thickness, scale)?
        }
        FeatureDefinition::OffsetShape { distance, .. } => scale_nonzero_length(distance, scale)?,
        FeatureDefinition::Thicken { thickness, .. } => {
            scale_optional_positive_length(thickness, scale)?
        }
        FeatureDefinition::OffsetSurface { distance, .. } => {
            scale_optional_length(distance, scale)?
        }
        FeatureDefinition::KnitSurface { gap_tolerance, .. } => {
            if let Some(gap) = gap_tolerance {
                scale_nonnegative_length(gap, scale)?;
            }
        }
        FeatureDefinition::SewBodies { gap_tolerance, .. } => {
            scale_optional_positive_length(gap_tolerance, scale)?
        }
        FeatureDefinition::ExtendSurface { distance, .. } => {
            scale_optional_positive_length(distance, scale)?
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
            construction,
            exit_kind,
            diameter,
            extent,
            ..
        } => {
            for placement in placements.iter_mut().flatten() {
                scale_hole_placement(placement, scale)?;
            }
            scale_hole_construction(construction, scale)?;
            if let Some(exit_kind) = exit_kind {
                scale_hole_kind(exit_kind, scale)?;
            }
            scale_optional_positive_length(diameter, scale)?;
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
            scale_feature_definition(operation, scale)?;
            scale_fuzzy_tolerance(fuzzy_tolerance, scale)?;
        }
        _ => {}
    };
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
    };
    *solid =
        PrimitiveSolid::new(candidate).map_err(|message| CodecError::Malformed(message.into()))?;
    Ok(())
}

fn scale_sweep_section(
    section: &mut cadmpeg_ir::features::SweepSection,
    scale: f64,
) -> Result<(), cadmpeg_core::CodecError> {
    if let cadmpeg_ir::features::SweepSection::Generated(
        cadmpeg_ir::features::GeneratedSweepSection::CircularRegion {
            outer_radius,
            wall_thickness,
        },
    ) = section
    {
        scale_positive_length(outer_radius, scale)?;
        scale_optional_positive_length(wall_thickness, scale)?;
    };
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
    };
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
    };
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
    };
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
    };
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
    };
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
    };
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
                scale_length(&mut width.first, scale)?;
                scale_length(&mut width.second, scale)?;
            }
        }
        SheetMetalFlangeWidth::FullEdge => {}
    };
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
    };
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
    };
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
    };
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
    };
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
    };
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
    };
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
            diameter,
            entry_diameter,
            depth,
            ..
        } => {
            scale_positive_length(diameter, scale)?;
            scale_optional_positive_length(entry_diameter, scale)?;
            scale_positive_length(depth, scale)?;
        }
        HoleKind::Simple | HoleKind::SimpleDrilled { .. } => {}
    };
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
    };
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
    };
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
    };
    *pattern = cadmpeg_ir::features::PatternKind::new(transform)
        .map_err(|message| CodecError::Malformed(message.into()))?;
    Ok(())
}

fn scale_surface_geometry(geometry: &mut SurfaceGeometry, scale: f64) -> Result<(), CodecError> {
    match geometry {
        SurfaceGeometry::Plane { origin, .. } => scale_point3(origin, scale),
        SurfaceGeometry::Cylinder { origin, radius, .. } => {
            scale_point3(origin, scale);
            *radius *= scale;
        }
        SurfaceGeometry::Cone { origin, radius, .. } => {
            scale_point3(origin, scale);
            *radius *= scale;
        }
        SurfaceGeometry::Sphere { center, radius, .. } => {
            scale_point3(center, scale);
            *radius *= scale;
        }
        SurfaceGeometry::Torus {
            center,
            major_radius,
            minor_radius,
            ..
        } => {
            scale_point3(center, scale);
            *major_radius *= scale;
            *minor_radius *= scale;
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
            for point in surface.vertices_mut() {
                scale_point3(point, scale);
            }
            surface.set_chordal_deflection(surface.chordal_deflection() * scale);
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
        CurveGeometry::Line { origin, .. } => scale_point3(origin, scale),
        CurveGeometry::Circle { center, radius, .. } => {
            scale_point3(center, scale);
            *radius *= scale;
        }
        CurveGeometry::Ellipse {
            center,
            major_radius,
            minor_radius,
            ..
        } => {
            scale_point3(center, scale);
            *major_radius *= scale;
            *minor_radius *= scale;
        }
        CurveGeometry::Parabola {
            vertex,
            focal_distance,
            ..
        } => {
            scale_point3(vertex, scale);
            *focal_distance *= scale;
        }
        CurveGeometry::Hyperbola {
            center,
            major_radius,
            minor_radius,
            ..
        } => {
            scale_point3(center, scale);
            *major_radius *= scale;
            *minor_radius *= scale;
        }
        CurveGeometry::Degenerate { point } => scale_point3(point, scale),
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
            for point in polyline.points_mut() {
                scale_point3(point, scale);
            }
            polyline.set_chordal_deflection(polyline.chordal_deflection() * scale);
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
) {
    use cadmpeg_ir::geometry::ProceduralCurveDefinition;

    if let ProceduralCurveDefinition::Helix {
        center,
        major,
        minor,
        pitch,
        ..
    } = definition
    {
        scale_point3(center, scale);
        scale_vector3(major, scale);
        scale_vector3(minor, scale);
        scale_vector3(pitch, scale);
    }
}

fn curve_parameter_scale(geometry: &CurveGeometry, length_scale_mm: f64) -> Option<f64> {
    match geometry {
        CurveGeometry::Line { .. } => Some(length_scale_mm),
        CurveGeometry::Circle { .. }
        | CurveGeometry::Ellipse { .. }
        | CurveGeometry::Parabola { .. }
        | CurveGeometry::Hyperbola { .. } => Some(1.0),
        CurveGeometry::Transformed { basis, .. } => curve_parameter_scale(basis, length_scale_mm),
        CurveGeometry::Nurbs { .. }
        | CurveGeometry::Degenerate { .. }
        | CurveGeometry::Composite { .. }
        | CurveGeometry::Procedural { .. }
        | CurveGeometry::Polyline(_)
        | CurveGeometry::Unknown { .. } => None,
    }
}

fn surface_parameter_scales(geometry: &SurfaceGeometry, length_scale_mm: f64) -> [f64; 2] {
    match geometry {
        SurfaceGeometry::Plane { .. } => [length_scale_mm, length_scale_mm],
        SurfaceGeometry::Cylinder { .. } | SurfaceGeometry::Cone { .. } => [1.0, length_scale_mm],
        SurfaceGeometry::Sphere { .. } | SurfaceGeometry::Torus { .. } => [1.0, 1.0],
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

/// Scale pcurve coordinates into the units of their owning surface.
///
/// The pcurve's own parameter interval remains unchanged.  When the two
/// surface-coordinate axes have different scales, circular, elliptic, and
/// hyperbolic carriers become their harmonic equivalents so their geometry
/// remains exact after anisotropic coordinate scaling.
fn scale_pcurve_geometry(geometry: &mut PcurveGeometry, scales: [f64; 2]) -> bool {
    let [u_scale, v_scale] = scales;
    let scale_point = |point: Point2| Point2::new(point.u * u_scale, point.v * v_scale);
    let isotropic = u_scale == v_scale;

    match geometry {
        PcurveGeometry::Line { origin, direction } => {
            *origin = scale_point(*origin);
            *direction = scale_point(*direction);
        }
        PcurveGeometry::Circle {
            center,
            x_axis,
            y_axis,
            radius,
        } => {
            let scaled_center = scale_point(*center);
            if isotropic {
                *center = scaled_center;
                *radius *= u_scale;
            } else {
                *geometry = PcurveGeometry::Harmonic {
                    center: scaled_center,
                    cosine: scale_point(Point2::new(*radius * x_axis.u, *radius * x_axis.v)),
                    sine: scale_point(Point2::new(*radius * y_axis.u, *radius * y_axis.v)),
                };
            }
        }
        PcurveGeometry::Ellipse {
            center,
            x_axis,
            y_axis,
            major_radius,
            minor_radius,
        } => {
            let scaled_center = scale_point(*center);
            if isotropic {
                *center = scaled_center;
                *major_radius *= u_scale;
                *minor_radius *= u_scale;
            } else {
                *geometry = PcurveGeometry::Harmonic {
                    center: scaled_center,
                    cosine: scale_point(Point2::new(
                        *major_radius * x_axis.u,
                        *major_radius * x_axis.v,
                    )),
                    sine: scale_point(Point2::new(
                        *minor_radius * y_axis.u,
                        *minor_radius * y_axis.v,
                    )),
                };
            }
        }
        PcurveGeometry::Parabola {
            vertex,
            focal_distance,
            ..
        } => {
            if !isotropic {
                return false;
            }
            *vertex = scale_point(*vertex);
            *focal_distance *= u_scale;
        }
        PcurveGeometry::Hyperbola {
            center,
            x_axis,
            y_axis,
            major_radius,
            minor_radius,
        } => {
            let scaled_center = scale_point(*center);
            if isotropic {
                *center = scaled_center;
                *major_radius *= u_scale;
                *minor_radius *= u_scale;
            } else {
                *geometry = PcurveGeometry::Hyperbolic {
                    center: scaled_center,
                    cosine: scale_point(Point2::new(
                        *major_radius * x_axis.u,
                        *major_radius * x_axis.v,
                    )),
                    sine: scale_point(Point2::new(
                        *minor_radius * y_axis.u,
                        *minor_radius * y_axis.v,
                    )),
                };
            }
        }
        PcurveGeometry::Harmonic {
            center,
            cosine,
            sine,
        }
        | PcurveGeometry::Hyperbolic {
            center,
            cosine,
            sine,
        } => {
            *center = scale_point(*center);
            *cosine = scale_point(*cosine);
            *sine = scale_point(*sine);
        }
        PcurveGeometry::Nurbs { nurbs } => {
            if nurbs
                .edit_control_points(|points| {
                    for point in points {
                        *point = scale_point(*point);
                    }
                })
                .is_err()
            {
                return false;
            }
        }
        PcurveGeometry::Trimmed { basis, .. } => {
            if !scale_pcurve_geometry(basis, scales) {
                return false;
            }
        }
        PcurveGeometry::Offset { distance, basis } => {
            if !isotropic || !scale_pcurve_geometry(basis, scales) {
                return false;
            }
            *distance *= u_scale;
        }
        PcurveGeometry::Transformed { basis, transform } => {
            if !u_scale.is_finite() || !v_scale.is_finite() || u_scale == 0.0 || v_scale == 0.0 {
                return false;
            }
            let mut rows = transform.rows();
            rows[0][1] *= u_scale / v_scale;
            rows[0][2] *= u_scale;
            rows[1][0] *= v_scale / u_scale;
            rows[1][2] *= v_scale;
            let Some(scaled) = Transform2::from_rows(rows) else {
                return false;
            };
            *transform = scaled;
            if !scale_pcurve_geometry(basis, scales) {
                return false;
            }
        }
        PcurveGeometry::PolarHarmonic { .. }
        | PcurveGeometry::PolarNurbs { .. }
        | PcurveGeometry::SphericalGreatCircle { .. } => return isotropic && u_scale == 1.0,
    }
    true
}

fn scale_sketch_geometry(geometry: &mut SketchGeometry, scale: f64) -> Result<(), CodecError> {
    match geometry {
        SketchGeometry::Point { position } => scale_point2(position, scale),
        SketchGeometry::Line { start, end } => {
            scale_point2(start, scale);
            scale_point2(end, scale);
        }
        SketchGeometry::ReferenceLine { origin, .. } => scale_point2(origin, scale),
        SketchGeometry::Circle { center, radius } => {
            scale_point2(center, scale);
            scale_length(radius, scale)?;
        }
        SketchGeometry::Arc { center, radius, .. } => {
            scale_point2(center, scale);
            scale_length(radius, scale)?;
        }
        SketchGeometry::Ellipse {
            center,
            major_radius,
            minor_radius,
            ..
        }
        | SketchGeometry::Hyperbola {
            center,
            major_radius,
            minor_radius,
            ..
        } => {
            scale_point2(center, scale);
            scale_length(major_radius, scale)?;
            scale_length(minor_radius, scale)?;
        }
        SketchGeometry::Parabola {
            vertex,
            focal_length,
            ..
        } => {
            scale_point2(vertex, scale);
            scale_length(focal_length, scale)?;
        }
        SketchGeometry::Nurbs { curve } => {
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
        SketchGeometry::Text {
            height, placement, ..
        } => {
            scale_length(height, scale)?;
            if let Some(placement) = placement {
                scale_point2(&mut placement.anchor, scale);
            }
        }
        SketchGeometry::ExternalReference { .. } | SketchGeometry::Native { .. } => {}
    }
    Ok(())
}

fn scale_spatial_sketch_geometry(
    geometry: &mut SpatialSketchGeometry,
    scale: f64,
) -> Result<(), CodecError> {
    match geometry {
        SpatialSketchGeometry::Point { position } => scale_point3(position, scale),
        SpatialSketchGeometry::Line { start, end } => {
            scale_point3(start, scale);
            scale_point3(end, scale);
        }
        SpatialSketchGeometry::Circle { center, radius, .. }
        | SpatialSketchGeometry::Arc { center, radius, .. } => {
            scale_point3(center, scale);
            scale_length(radius, scale)?;
        }
        SpatialSketchGeometry::Nurbs { curve } => {
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
        SpatialSketchGeometry::NurbsSurface { surface } => {
            for point in surface.control_points_mut() {
                scale_point3(point, scale);
            }
        }
        SpatialSketchGeometry::Native { .. } => {}
    }
    Ok(())
}

fn scale_sketch_constraint_definition(
    definition: &mut cadmpeg_ir::sketches::SketchConstraintDefinition,
    scale: f64,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::sketches::SketchConstraintDefinition;

    match definition {
        SketchConstraintDefinition::PointCoordinateValues { values, .. } => {
            for value in values {
                scale_length(value, scale)?;
            }
        }
        SketchConstraintDefinition::MidpointCoordinate { value, .. }
        | SketchConstraintDefinition::DistanceLociValue {
            distance: value, ..
        }
        | SketchConstraintDefinition::PolarDistance {
            distance: value, ..
        }
        | SketchConstraintDefinition::Offset {
            distance: value, ..
        } => scale_length(value, scale)?,
        _ => {}
    };
    Ok(())
}

fn scale_spatial_sketch_constraint_definition(
    definition: &mut cadmpeg_ir::sketches::SpatialSketchConstraintDefinition,
    scale: f64,
) -> Result<(), cadmpeg_core::CodecError> {
    if let cadmpeg_ir::sketches::SpatialSketchConstraintDefinition::Offset { distance, .. } =
        definition
    {
        scale_length(distance, scale)?;
    };
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
            cadmpeg_ir::features::FeatureId::mint("feature").expect("identity grammar"),
            0,
            FeatureDefinition::Extrude {
                profile: ProfileRef::Unresolved("profile".into()),
                direction: ExtrudeDirection::ProfileNormal,
                start: ExtrudeStart::OffsetProfilePlane {
                    offset: Length::new(2.0).unwrap(),
                },
                extent: ExtrudeExtent::TwoSided {
                    first: ExtrudeSide {
                        termination: LinearTermination::Blind {
                            length: cadmpeg_ir::features::NonZeroLength::new(3.0).unwrap(),
                        },
                        draft: None,
                    },
                    second: ExtrudeSide {
                        termination: LinearTermination::ToFace {
                            face: cadmpeg_ir::features::FaceSelection::Native("face".into()),
                            offset: Some(Length::new(4.0).unwrap()),
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
                id: cadmpeg_ir::features::ParameterId::mint("length").expect("identity grammar"),
                owner: None,
                ordinal: 0,
                name: "length".into(),
                expression: "2".into(),
                display: None,
                value: Some(ParameterValue::Length(Length::new(5.0).unwrap())),
                dependencies: Vec::new(),
                properties: BTreeMap::new(),
                pmi: None,
                native_ref: None,
            });
        normalize_model_lengths(&mut ir, 25.4).expect("valid unit scaling");

        let FeatureDefinition::Extrude { start, extent, .. } = &ir.model.features[0].definition
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
            .unwrap(),
            distance: Length::new(2.0).unwrap(),
        };
        scale_face_motion(&mut translate, 25.4).unwrap();
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
                .unwrap(),
            axis_dir: cadmpeg_ir::features::FeatureDirection3::new(cadmpeg_ir::math::Vector3::new(
                0.0, 0.0, 1.0,
            ))
            .unwrap(),
            angle: cadmpeg_ir::features::Angle::new(0.5).unwrap(),
        };
        scale_face_motion(&mut rotate, 25.4).unwrap();
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
        .unwrap();
        scale_pattern_kind(&mut pattern, 25.4).unwrap();
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
            operation: Box::new(FeatureDefinition::Native {
                kind: "Boolean".into(),
                parameters: BTreeMap::new(),
            }),
            refine: false,
            fuzzy_tolerance: FuzzyTolerance::Explicit(
                cadmpeg_ir::features::PositiveLength::new(2.0).unwrap(),
            ),
        };

        scale_feature_definition(&mut definition, 25.4).unwrap();

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
            cadmpeg_ir::geometry::ProceduralCurveDefinition::Helix {
                angle_range: [0.0, 1.0],
                center: Point3::new(1.0, 2.0, 3.0),
                major: Vector3::new(4.0, 5.0, 6.0),
                minor: Vector3::new(7.0, 8.0, 9.0),
                pitch: Vector3::new(10.0, 11.0, 12.0),
                apex_factor: 0.25,
                axis: Vector3::new(0.0, 0.0, 1.0),
            },
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
        let cadmpeg_ir::geometry::ProceduralCurveDefinition::Helix {
            center,
            major,
            minor,
            pitch,
            axis,
            apex_factor,
            ..
        } = curve.definition()
        else {
            panic!("test curve construction changed family");
        };
        assert_point3(*center, [25.4, 50.8, 76.2]);
        assert_vector3(*major, [101.6, 127.0, 152.4]);
        assert_vector3(*minor, [177.8, 203.2, 228.6]);
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
        let mut geometry = PcurveGeometry::Line {
            origin: Point2::new(1.0, 2.0),
            direction: Point2::new(3.0, 4.0),
        };

        assert!(scale_pcurve_geometry(&mut geometry, [25.4, 1.0]));
        let PcurveGeometry::Line { origin, direction } = geometry else {
            panic!("test pcurve changed family");
        };
        assert_point2(origin, [25.4, 2.0]);
        assert_point2(direction, [76.2, 4.0]);
    }

    #[test]
    fn scales_analytic_surface_and_curve_without_scaling_directions() {
        let mut surface = SurfaceGeometry::Cylinder {
            origin: Point3::new(1.0, 2.0, 3.0),
            axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            ref_direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            radius: 4.0,
        };
        let mut curve = CurveGeometry::Circle {
            center: Point3::new(2.0, 3.0, 4.0),
            axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            ref_direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            radius: 5.0,
        };

        scale_surface_geometry(&mut surface, 25.4).expect("finite surface scaling");
        scale_curve_geometry(&mut curve, 25.4).expect("finite curve scaling");

        let SurfaceGeometry::Cylinder {
            origin,
            axis,
            radius,
            ..
        } = surface
        else {
            panic!("test surface changed family");
        };
        assert_point3(origin, [25.4, 50.8, 76.2]);
        assert_eq!(axis, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0));
        assert_close(radius, 101.6);
        let CurveGeometry::Circle {
            center,
            axis,
            radius,
            ..
        } = curve
        else {
            panic!("test curve changed family");
        };
        assert_point3(center, [50.8, 76.2, 101.6]);
        assert_eq!(axis, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0));
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
}
