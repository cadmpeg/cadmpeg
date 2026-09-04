// SPDX-License-Identifier: Apache-2.0
//! Conversion of neutral Creo values into the canonical IR length unit.
//!
//! The PSB scanner keeps source values in their stored unit so native records
//! remain faithful to the file.  This module is the single boundary at which
//! the already-built neutral model is converted to millimeters.  Unit
//! directions, angles, ratios, and source-native arenas are intentionally not
//! scaled.

use std::collections::BTreeMap;

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{FeatureDefinition, Length, ParameterValue, WrapMode};
use cadmpeg_ir::geometry::{CurveGeometry, PcurveGeometry, SurfaceGeometry};
use cadmpeg_ir::ids::PcurveId;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{SketchGeometry, SketchPlacement, SpatialSketchGeometry};
use cadmpeg_ir::transform::Transform;

/// Scale all neutral model lengths from the source unit into millimeters.
pub(super) fn normalize_model_lengths(ir: &mut CadIr, length_scale_mm: f64) {
    if !length_scale_mm.is_finite() || length_scale_mm <= 0.0 || length_scale_mm == 1.0 {
        return;
    }

    let pcurve_scales = pcurve_scales(ir, length_scale_mm);
    for pcurve in &mut ir.model.pcurves {
        if let Some(scales) = pcurve_scales.get(&pcurve.id) {
            let _ = scale_pcurve_geometry(&mut pcurve.geometry, *scales);
        }
    }

    for surface in &mut ir.model.surfaces {
        scale_surface_geometry(&mut surface.geometry, length_scale_mm);
    }
    for curve in &mut ir.model.curves {
        scale_curve_geometry(&mut curve.geometry, length_scale_mm);
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
        for vertex in &mut tessellation.vertices {
            scale_point3(vertex, length_scale_mm);
        }
        scale_optional(&mut tessellation.chordal_deflection, length_scale_mm);
    }
    for feature in &mut ir.model.features {
        scale_feature_definition(&mut feature.definition, length_scale_mm);
    }

    for parameter in &mut ir.model.parameters {
        if let Some(ParameterValue::Length(length)) = parameter.value.as_mut() {
            length.0 *= length_scale_mm;
        }
    }
    for configuration in &mut ir.model.configurations {
        for value in configuration.parameter_values.values_mut() {
            if let ParameterValue::Length(length) = value {
                length.0 *= length_scale_mm;
            }
        }
        for state in configuration.feature_states.values_mut() {
            scale_feature_definition(&mut state.definition, length_scale_mm);
        }
    }
    for sketch in &mut ir.model.sketches {
        if let SketchPlacement::Resolved { origin, .. } = &mut sketch.placement {
            scale_point3(origin, length_scale_mm);
        }
    }
    for entity in &mut ir.model.sketch_entities {
        scale_sketch_geometry(&mut entity.geometry, length_scale_mm);
    }
    for sketch in &mut ir.model.spatial_sketches {
        for profile in &mut sketch.profiles {
            scale_point3(&mut profile.origin, length_scale_mm);
        }
    }
    for entity in &mut ir.model.spatial_sketch_entities {
        scale_spatial_sketch_geometry(&mut entity.geometry, length_scale_mm);
    }
    for constraint in &mut ir.model.sketch_constraints {
        scale_sketch_constraint_definition(&mut constraint.definition, length_scale_mm);
    }
    for constraint in &mut ir.model.spatial_sketch_constraints {
        scale_spatial_sketch_constraint_definition(&mut constraint.definition, length_scale_mm);
    }
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

fn scale_vector3(vector: &mut Vector3, scale: f64) {
    vector.x *= scale;
    vector.y *= scale;
    vector.z *= scale;
}

fn scale_transform_translation(transform: &mut Transform, scale: f64) {
    for row in &mut transform.rows[..3] {
        row[3] *= scale;
    }
}

fn scale_length(length: &mut Length, scale: f64) {
    length.0 *= scale;
}

fn scale_optional_length(length: &mut Option<Length>, scale: f64) {
    if let Some(length) = length.as_mut() {
        scale_length(length, scale);
    }
}

fn scale_datum_plane_reference(
    reference: &mut cadmpeg_ir::features::DatumPlaneReference,
    scale: f64,
) {
    if let cadmpeg_ir::features::DatumPlaneReference::ResolvedPlane { origin, .. } = reference {
        scale_point3(origin, scale);
    }
}

fn scale_datum_point_construction(
    construction: &mut cadmpeg_ir::features::DatumPointConstruction,
    scale: f64,
) {
    match construction {
        cadmpeg_ir::features::DatumPointConstruction::ThreePlaneIntersection { planes } => {
            for plane in planes.iter_mut() {
                scale_datum_plane_reference(plane, scale);
            }
        }
        cadmpeg_ir::features::DatumPointConstruction::EdgePlaneIntersection { plane, .. } => {
            scale_datum_plane_reference(plane, scale);
        }
        cadmpeg_ir::features::DatumPointConstruction::CircleCenter { .. }
        | cadmpeg_ir::features::DatumPointConstruction::TwoEdgeIntersection { .. }
        | cadmpeg_ir::features::DatumPointConstruction::Vertex { .. }
        | cadmpeg_ir::features::DatumPointConstruction::SketchPoint { .. }
        | cadmpeg_ir::features::DatumPointConstruction::DistanceOnEdge { .. } => {}
    }
}

fn scale_feature_definition(definition: &mut FeatureDefinition, scale: f64) {
    match definition {
        FeatureDefinition::CosmeticThread {
            diameter, extent, ..
        } => {
            scale_optional_length(diameter, scale);
            if let Some(cadmpeg_ir::features::CosmeticThreadExtent::Blind { length }) = extent {
                scale_length(length, scale);
            }
        }
        FeatureDefinition::ReferenceImage { origin, bounds, .. } => {
            scale_point3(origin, scale);
            for point in bounds {
                scale_point2(point, scale);
            }
        }
        FeatureDefinition::DatumPlane { origin, .. }
        | FeatureDefinition::DatumAxis { origin, .. }
        | FeatureDefinition::DatumCoordinateSystem { origin, .. }
        | FeatureDefinition::MirrorShape {
            plane_origin: origin,
            ..
        }
        | FeatureDefinition::DatumThreePointPlane { origin, .. } => {
            scale_point3(origin, scale);
        }
        FeatureDefinition::DatumPoint {
            position,
            construction,
        } => {
            scale_point3(position, scale);
            if let Some(construction) = construction {
                scale_datum_point_construction(construction, scale);
            }
        }
        FeatureDefinition::DatumOffsetPlane {
            reference,
            distance,
        } => {
            if let Some(reference) = reference {
                scale_datum_plane_reference(reference, scale);
            }
            scale_length(distance, scale);
        }
        FeatureDefinition::PointGeometry { position } => {
            scale_point3(position, scale);
        }
        FeatureDefinition::LineSegment { start, end } => {
            scale_point3(start, scale);
            scale_point3(end, scale);
        }
        FeatureDefinition::CircularArc { center, radius, .. } => {
            scale_point3(center, scale);
            scale_length(radius, scale);
        }
        FeatureDefinition::EllipticArc {
            center,
            major_radius,
            minor_radius,
            ..
        } => {
            scale_point3(center, scale);
            scale_length(major_radius, scale);
            scale_length(minor_radius, scale);
        }
        FeatureDefinition::Polyline { points, .. } => {
            for point in points {
                scale_point3(point, scale);
            }
        }
        FeatureDefinition::RegularPolygonCurve { circumradius, .. } => {
            scale_length(circumradius, scale);
        }
        FeatureDefinition::PlanarPatch { length, width } => {
            scale_length(length, scale);
            scale_length(width, scale);
        }
        FeatureDefinition::Block {
            dimensions,
            placement,
            ..
        } => {
            if let Some(dimensions) = dimensions {
                for dimension in dimensions {
                    scale_length(dimension, scale);
                }
            }
            if let Some(placement) = placement {
                scale_transform_translation(placement, scale);
            }
        }
        FeatureDefinition::ProjectOnSurface { height, offset, .. } => {
            scale_length(height, scale);
            scale_length(offset, scale);
        }
        FeatureDefinition::Helix {
            axis_origin,
            radius,
            shape,
            ..
        } => {
            scale_point3(axis_origin, scale);
            scale_length(radius, scale);
            match shape {
                cadmpeg_ir::features::HelixShape::Cylindrical { pitch }
                | cadmpeg_ir::features::HelixShape::Conical { pitch, .. } => {
                    if let Some(scaled) =
                        cadmpeg_ir::features::HelixPitch::new(Length(pitch.get().0 * scale))
                    {
                        *pitch = scaled;
                    }
                }
                cadmpeg_ir::features::HelixShape::Spiral { radial_growth } => {
                    scale_length(radial_growth, scale);
                }
            }
        }
        FeatureDefinition::HelixNativeAxis {
            axial_rise, pitch, ..
        } => {
            scale_length(axial_rise, scale);
            scale_length(pitch, scale);
        }
        FeatureDefinition::Sphere { center, radius, .. } => {
            scale_point3(center, scale);
            scale_length(radius, scale);
        }
        FeatureDefinition::Torus {
            center,
            major_radius,
            minor_radius,
            ..
        } => {
            scale_point3(center, scale);
            scale_length(major_radius, scale);
            scale_length(minor_radius, scale);
        }
        FeatureDefinition::Wrap {
            mode: WrapMode::Emboss { depth } | WrapMode::Deboss { depth },
            ..
        } => scale_length(depth, scale),
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
        FeatureDefinition::Primitive { solid, .. } => scale_primitive_solid(solid, scale),
        FeatureDefinition::Sweep {
            section, sections, ..
        } => {
            scale_sweep_section(section, scale);
            for section in sections {
                scale_sweep_section(section, scale);
            }
        }
        FeatureDefinition::HelicalSweep { construction, .. } => {
            scale_point3(&mut construction.axis_origin, scale);
            scale_length(&mut construction.pitch, scale);
            scale_length(&mut construction.height, scale);
            scale_length(&mut construction.radial_growth, scale);
        }
        FeatureDefinition::Coil { construction, .. } => {
            scale_coil_construction(construction, scale);
        }
        FeatureDefinition::Binder {
            construction:
                cadmpeg_ir::features::BinderConstruction::SubShape {
                    offset: Some(offset),
                    ..
                },
            ..
        } => scale_length(&mut offset.distance, scale),
        FeatureDefinition::Binder { .. } => {}
        FeatureDefinition::Loft { sections, .. } => {
            for section in sections {
                if let cadmpeg_ir::features::LoftSection::Point(
                    cadmpeg_ir::features::LoftPointSection::Point(point),
                ) = section
                {
                    scale_point3(point, scale);
                }
            }
        }
        FeatureDefinition::Extrude { start, extent, .. } => {
            scale_extrude_start(start, scale);
            scale_extrude_extent(extent, scale);
        }
        FeatureDefinition::Revolve { construction, .. } => {
            if let Some(axis) = construction.axis_mut() {
                scale_point3(&mut axis.origin, scale);
            }
            if let Some(extent) = construction.extent_mut() {
                scale_revolve_extent(extent, scale);
            }
        }
        FeatureDefinition::Rib { construction, .. } => {
            scale_optional_length(&mut construction.thickness, scale);
        }
        FeatureDefinition::SheetMetalBaseFlange { thickness, .. } => {
            scale_length(thickness, scale);
        }
        FeatureDefinition::SheetMetalEdgeFlange {
            height,
            width,
            bend_radius,
            ..
        } => {
            scale_sheet_metal_flange_height(height, scale);
            scale_sheet_metal_flange_width(width, scale);
            scale_length(bend_radius, scale);
        }
        FeatureDefinition::SheetMetalHem {
            form, bend_radius, ..
        } => {
            scale_sheet_metal_hem_form(form, scale);
            scale_length(bend_radius, scale);
        }
        FeatureDefinition::Fillet { groups } => {
            for group in groups {
                scale_radius_spec(&mut group.radius, scale);
            }
        }
        FeatureDefinition::FaceBlend { radius, .. } => scale_radius_spec(radius, scale),
        FeatureDefinition::Chamfer { groups, .. } => {
            for group in groups {
                scale_chamfer_spec(&mut group.spec, scale);
            }
        }
        FeatureDefinition::Shell { thickness, .. } => scale_optional_length(thickness, scale),
        FeatureDefinition::OffsetShape { distance, .. } => scale_length(distance, scale),
        FeatureDefinition::Thicken { thickness, .. } => scale_optional_length(thickness, scale),
        FeatureDefinition::OffsetSurface { distance, .. } => scale_optional_length(distance, scale),
        FeatureDefinition::KnitSurface { gap_tolerance, .. }
        | FeatureDefinition::SewBodies { gap_tolerance, .. } => {
            scale_optional_length(gap_tolerance, scale);
        }
        FeatureDefinition::ExtendSurface { distance, .. } => scale_optional_length(distance, scale),
        FeatureDefinition::RuledSurface { mode, .. } => scale_ruled_surface_mode(mode, scale),
        FeatureDefinition::Draft { .. } => {}
        FeatureDefinition::MoveFace { motion, .. } => scale_face_motion(motion, scale),
        FeatureDefinition::MoveBody {
            translation,
            rotation,
            ..
        } => {
            translation.x *= scale;
            translation.y *= scale;
            translation.z *= scale;
            if let Some(rotation) = rotation {
                scale_point3(&mut rotation.origin, scale);
            }
        }
        FeatureDefinition::Dome { height, .. } => scale_optional_length(height, scale),
        FeatureDefinition::Flex { mode, .. } => scale_flex_mode(mode, scale),
        FeatureDefinition::Scale {
            center: Some(cadmpeg_ir::features::ScaleCenter::Point(point)),
            ..
        } => scale_point3(point, scale),
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
                scale_hole_placement(placement, scale);
            }
            scale_hole_construction(construction, scale);
            if let Some(exit_kind) = exit_kind {
                scale_hole_kind(exit_kind, scale);
            }
            scale_optional_length(diameter, scale);
            if let Some(extent) = extent {
                scale_linear_termination(extent, scale);
            }
        }
        FeatureDefinition::Pattern { pattern, .. } => scale_pattern_kind(pattern, scale),
        FeatureDefinition::PostProcess {
            operation,
            fuzzy_tolerance,
            ..
        } => {
            scale_feature_definition(operation, scale);
            scale_fuzzy_tolerance(fuzzy_tolerance, scale);
        }
        _ => {}
    }
}

fn scale_fuzzy_tolerance(tolerance: &mut cadmpeg_ir::features::FuzzyTolerance, scale: f64) {
    if let cadmpeg_ir::features::FuzzyTolerance::Explicit(value) = tolerance {
        *value *= scale;
    }
}

fn scale_primitive_solid(solid: &mut cadmpeg_ir::features::PrimitiveSolid, scale: f64) {
    use cadmpeg_ir::features::PrimitiveSolid;

    match solid {
        PrimitiveSolid::Box {
            length,
            width,
            height,
        } => {
            scale_length(length, scale);
            scale_length(width, scale);
            scale_length(height, scale);
        }
        PrimitiveSolid::Cylinder { radius, height, .. } => {
            scale_length(radius, scale);
            scale_length(height, scale);
        }
        PrimitiveSolid::Cone {
            radius1,
            radius2,
            height,
            ..
        } => {
            scale_length(radius1, scale);
            scale_length(radius2, scale);
            scale_length(height, scale);
        }
        PrimitiveSolid::Sphere { radius, .. } => scale_length(radius, scale),
        PrimitiveSolid::Ellipsoid {
            x_radius,
            y_radius,
            z_radius,
            ..
        } => {
            scale_length(x_radius, scale);
            scale_length(y_radius, scale);
            scale_length(z_radius, scale);
        }
        PrimitiveSolid::Torus {
            major_radius,
            minor_radius,
            ..
        } => {
            scale_length(major_radius, scale);
            scale_length(minor_radius, scale);
        }
        PrimitiveSolid::Prism {
            circumradius,
            height,
            ..
        } => {
            scale_length(circumradius, scale);
            scale_length(height, scale);
        }
        PrimitiveSolid::Wedge {
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
                scale_length(length, scale);
            }
        }
    }
}

fn scale_sweep_section(section: &mut cadmpeg_ir::features::SweepSection, scale: f64) {
    if let cadmpeg_ir::features::SweepSection::Generated(
        cadmpeg_ir::features::GeneratedSweepSection::CircularRegion {
            outer_radius,
            wall_thickness,
        },
    ) = section
    {
        scale_length(outer_radius, scale);
        scale_optional_length(wall_thickness, scale);
    }
}

fn scale_coil_construction(construction: &mut cadmpeg_ir::features::CoilConstruction, scale: f64) {
    if let cadmpeg_ir::features::CoilPlacement::Explicit { origin, .. } =
        &mut construction.placement
    {
        scale_point3(origin, scale);
    }
    scale_length(&mut construction.diameter, scale);
    match &mut construction.extent {
        cadmpeg_ir::features::CoilExtent::RevolutionsHeight { height, .. } => {
            scale_length(height, scale);
        }
        cadmpeg_ir::features::CoilExtent::RevolutionsPitch { pitch, .. } => {
            scale_length(pitch, scale);
        }
        cadmpeg_ir::features::CoilExtent::HeightPitch { height, pitch } => {
            scale_length(height, scale);
            scale_length(pitch, scale);
        }
        cadmpeg_ir::features::CoilExtent::Spiral { radial_pitch, .. } => {
            scale_length(radial_pitch, scale);
        }
    }
    match &mut construction.section {
        cadmpeg_ir::features::CoilSection::Circular { diameter }
        | cadmpeg_ir::features::CoilSection::Square { size: diameter }
        | cadmpeg_ir::features::CoilSection::ExternalTriangle { size: diameter }
        | cadmpeg_ir::features::CoilSection::InternalTriangle { size: diameter } => {
            scale_length(diameter, scale);
        }
    }
}

fn scale_extrude_start(start: &mut cadmpeg_ir::features::ExtrudeStart, scale: f64) {
    use cadmpeg_ir::features::ExtrudeStart;

    match start {
        ExtrudeStart::OffsetProfilePlane { offset } => scale_length(offset, scale),
        ExtrudeStart::FromFace { offset, .. } => scale_optional_length(offset, scale),
        ExtrudeStart::Unresolved | ExtrudeStart::ProfilePlane => {}
    }
}

fn scale_extrude_side(side: &mut cadmpeg_ir::features::ExtrudeSide, scale: f64) {
    scale_linear_termination(&mut side.termination, scale);
}

fn scale_extrude_extent(extent: &mut cadmpeg_ir::features::ExtrudeExtent, scale: f64) {
    use cadmpeg_ir::features::ExtrudeExtent;

    match extent {
        ExtrudeExtent::OneSided { side } | ExtrudeExtent::Symmetric { side } => {
            scale_extrude_side(side, scale);
        }
        ExtrudeExtent::TwoSided { first, second } => {
            scale_extrude_side(first, scale);
            scale_extrude_side(second, scale);
        }
    }
}

fn scale_revolve_extent(extent: &mut cadmpeg_ir::features::RevolveExtent, scale: f64) {
    use cadmpeg_ir::features::RevolveExtent;

    match extent {
        RevolveExtent::OneSided { termination } | RevolveExtent::Symmetric { termination } => {
            scale_angular_termination(termination, scale);
        }
        RevolveExtent::TwoSided { first, second } => {
            scale_angular_termination(first, scale);
            scale_angular_termination(second, scale);
        }
    }
}

fn scale_linear_termination(termination: &mut cadmpeg_ir::features::LinearTermination, scale: f64) {
    use cadmpeg_ir::features::LinearTermination;

    match termination {
        LinearTermination::Blind { length } => scale_length(length, scale),
        LinearTermination::ToFace { offset, .. } => scale_optional_length(offset, scale),
        LinearTermination::OffsetFromFace { offset, .. } => scale_length(offset, scale),
        LinearTermination::Unresolved
        | LinearTermination::ThroughAll
        | LinearTermination::ThroughNext
        | LinearTermination::ToFirst
        | LinearTermination::ToLast
        | LinearTermination::ToVertex { .. }
        | LinearTermination::ToShape { .. } => {}
    }
}

fn scale_angular_termination(
    termination: &mut cadmpeg_ir::features::AngularTermination,
    scale: f64,
) {
    use cadmpeg_ir::features::AngularTermination;

    match termination {
        AngularTermination::ToFace { offset, .. } => scale_optional_length(offset, scale),
        AngularTermination::OffsetFromFace { offset, .. } => scale_length(offset, scale),
        AngularTermination::Unresolved
        | AngularTermination::ThroughAll
        | AngularTermination::ThroughNext
        | AngularTermination::ToFirst
        | AngularTermination::ToLast
        | AngularTermination::ToVertex { .. }
        | AngularTermination::ToShape { .. }
        | AngularTermination::Angle { .. } => {}
    }
}

fn scale_sheet_metal_flange_height(
    height: &mut cadmpeg_ir::features::SheetMetalFlangeHeight,
    scale: f64,
) {
    use cadmpeg_ir::features::SheetMetalFlangeHeight;

    match height {
        SheetMetalFlangeHeight::Distance(distance)
        | SheetMetalFlangeHeight::ToObject {
            offset: distance, ..
        } => {
            scale_length(distance, scale);
        }
    }
}

fn scale_sheet_metal_flange_width(
    width: &mut cadmpeg_ir::features::SheetMetalFlangeWidth,
    scale: f64,
) {
    use cadmpeg_ir::features::SheetMetalFlangeWidth;

    match width {
        SheetMetalFlangeWidth::Symmetric { width } => scale_length(width, scale),
        SheetMetalFlangeWidth::TwoSides { first, second } => {
            scale_length(first, scale);
            scale_length(second, scale);
        }
        SheetMetalFlangeWidth::TwoSidesPerEdge { widths } => {
            for width in widths {
                scale_length(&mut width.first, scale);
                scale_length(&mut width.second, scale);
            }
        }
        SheetMetalFlangeWidth::FullEdge => {}
    }
}

fn scale_sheet_metal_hem_form(form: &mut cadmpeg_ir::features::SheetMetalHemForm, scale: f64) {
    use cadmpeg_ir::features::SheetMetalHemForm;

    match form {
        SheetMetalHemForm::Flat { length } | SheetMetalHemForm::Rolled { radius: length, .. } => {
            scale_length(length, scale);
        }
        SheetMetalHemForm::Open { gap, length } | SheetMetalHemForm::GapLength { gap, length } => {
            scale_length(gap, scale);
            scale_length(length, scale);
        }
        SheetMetalHemForm::Teardrop {
            gap,
            length,
            radius,
        } => {
            scale_length(gap, scale);
            scale_length(length, scale);
            scale_length(radius, scale);
        }
    }
}

fn scale_radius_spec(radius: &mut cadmpeg_ir::features::RadiusSpec, scale: f64) {
    use cadmpeg_ir::features::RadiusSpec;

    match radius {
        RadiusSpec::Constant { radius }
        | RadiusSpec::Chordal {
            chord_length: radius,
        } => scale_length(radius, scale),
        RadiusSpec::Asymmetric {
            offset_one,
            offset_two,
        } => {
            scale_length(offset_one, scale);
            scale_length(offset_two, scale);
        }
        RadiusSpec::Variable { points } => {
            for point in points {
                scale_length(&mut point.radius, scale);
            }
        }
        RadiusSpec::Unresolved
        | RadiusSpec::UnresolvedConstant
        | RadiusSpec::UnresolvedChordal
        | RadiusSpec::UnresolvedAsymmetric
        | RadiusSpec::UnresolvedVariable => {}
    }
}

fn scale_chamfer_spec(spec: &mut cadmpeg_ir::features::ChamferSpec, scale: f64) {
    use cadmpeg_ir::features::ChamferSpec;

    match spec {
        ChamferSpec::Distance { distance } | ChamferSpec::DistanceAngle { distance, .. } => {
            scale_length(distance, scale);
        }
        ChamferSpec::TwoDistances { first, second } => {
            scale_length(first, scale);
            scale_length(second, scale);
        }
        ChamferSpec::Unresolved
        | ChamferSpec::UnresolvedDistance
        | ChamferSpec::UnresolvedTwoDistances
        | ChamferSpec::UnresolvedDistanceAngle => {}
    }
}

fn scale_ruled_surface_mode(mode: &mut cadmpeg_ir::features::RuledSurfaceMode, scale: f64) {
    use cadmpeg_ir::features::RuledSurfaceMode;

    match mode {
        RuledSurfaceMode::Normal { distance }
        | RuledSurfaceMode::Tangent { distance }
        | RuledSurfaceMode::Direction { distance, .. } => scale_length(distance, scale),
    }
}

fn scale_face_motion(motion: &mut cadmpeg_ir::features::FaceMotion, scale: f64) {
    match motion {
        cadmpeg_ir::features::FaceMotion::Offset { distance }
        | cadmpeg_ir::features::FaceMotion::Translate { distance, .. } => {
            scale_length(distance, scale);
        }
        cadmpeg_ir::features::FaceMotion::Rotate { axis_origin, .. } => {
            scale_point3(axis_origin, scale);
        }
    }
}

fn scale_flex_mode(mode: &mut cadmpeg_ir::features::FlexMode, scale: f64) {
    use cadmpeg_ir::features::FlexMode;

    match mode {
        FlexMode::Unresolved(_) => {}
        FlexMode::Stretching { distance } => scale_length(distance, scale),
        FlexMode::Bending { .. } | FlexMode::Twisting { .. } | FlexMode::Tapering { .. } => {}
    }
}

fn scale_hole_placement(placement: &mut cadmpeg_ir::features::HolePlacement, scale: f64) {
    match placement {
        cadmpeg_ir::features::HolePlacement::Directed { position, .. }
        | cadmpeg_ir::features::HolePlacement::Axis {
            origin: position, ..
        } => scale_point3(position, scale),
    }
}

fn scale_hole_kind(kind: &mut cadmpeg_ir::features::HoleKind, scale: f64) {
    use cadmpeg_ir::features::HoleKind;

    match kind {
        HoleKind::Unresolved(_) => {}
        HoleKind::PartialCounterbore { diameter, depth } => {
            scale_optional_length(diameter, scale);
            scale_optional_length(depth, scale);
        }
        HoleKind::PartialCountersink { diameter, .. } => {
            scale_optional_length(diameter, scale);
        }
        HoleKind::Chamfer { diameter, .. } | HoleKind::Countersink { diameter, .. } => {
            scale_length(diameter, scale);
        }
        HoleKind::Counterbore { diameter, depth }
        | HoleKind::CounterboreDrilled {
            diameter, depth, ..
        } => {
            scale_length(diameter, scale);
            scale_length(depth, scale);
        }
        HoleKind::Counterdrill {
            diameter,
            entry_diameter,
            depth,
            ..
        } => {
            scale_length(diameter, scale);
            scale_optional_length(entry_diameter, scale);
            scale_length(depth, scale);
        }
        HoleKind::Simple | HoleKind::SimpleDrilled { .. } => {}
    }
}

fn scale_hole_construction(construction: &mut cadmpeg_ir::features::HoleConstruction, scale: f64) {
    match construction {
        cadmpeg_ir::features::HoleConstruction::Form {
            kind,
            specification,
        } => {
            scale_hole_kind(kind, scale);
            if let Some(specification) = specification {
                scale_hole_specification(specification, scale);
            }
        }
        cadmpeg_ir::features::HoleConstruction::NativeThread {
            major_diameter,
            thread_depth,
            pitch,
            ..
        } => {
            scale_length(major_diameter, scale);
            scale_length(thread_depth, scale);
            scale_optional_length(pitch, scale);
        }
    }
}

fn scale_hole_specification(
    specification: &mut cadmpeg_ir::features::HoleSpecification,
    scale: f64,
) {
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
        scale_optional_length(pitch, scale);
    }
    if let Some(major_diameter) = major_diameter {
        scale_optional_length(major_diameter, scale);
    }
    scale_optional_length(clearance, scale);
    if let cadmpeg_ir::features::HoleThreadDepth::Blind { depth } = depth {
        scale_length(depth, scale);
    }
}

fn scale_pattern_kind(pattern: &mut cadmpeg_ir::features::PatternKind, scale: f64) {
    use cadmpeg_ir::features::PatternKind;

    match pattern {
        PatternKind::Linear {
            spacing, second, ..
        } => {
            scale_length(spacing, scale);
            if let Some(second) = second {
                scale_length(&mut second.spacing, scale);
            }
        }
        PatternKind::LinearOffsets { offsets, .. } => {
            for offset in offsets {
                scale_length(offset, scale);
            }
        }
        PatternKind::CurveDriven { spacing, .. } => scale_length(spacing, scale),
        PatternKind::Circular { axis_origin, .. } => scale_point3(axis_origin, scale),
        PatternKind::CircularAngles { axis_origin, .. } => scale_point3(axis_origin, scale),
        PatternKind::Mirror { plane_origin, .. } => scale_point3(plane_origin, scale),
        PatternKind::Composite { stages } => {
            for stage in stages {
                scale_pattern_kind(&mut stage.pattern, scale);
            }
        }
        PatternKind::Scale { center, .. } => {
            if let cadmpeg_ir::features::PatternScaleCenter::Point(point) = center {
                scale_point3(point, scale);
            }
        }
        PatternKind::Unresolved
        | PatternKind::UnresolvedLinear
        | PatternKind::UnresolvedCircular
        | PatternKind::UnresolvedCurveDriven
        | PatternKind::UnresolvedMirror
        | PatternKind::UnresolvedScale
        | PatternKind::UnresolvedComposite
        | PatternKind::MirrorReference { .. } => {}
    }
}

fn scale_surface_geometry(geometry: &mut SurfaceGeometry, scale: f64) {
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
            for point in &mut surface.control_points {
                scale_point3(point, scale);
            }
        }
        SurfaceGeometry::Polygonal {
            vertices,
            chordal_deflection,
            ..
        } => {
            for point in vertices {
                scale_point3(point, scale);
            }
            *chordal_deflection *= scale;
        }
        SurfaceGeometry::Transformed {
            basis, transform, ..
        } => {
            scale_surface_geometry(basis, scale);
            scale_transform_translation(transform, scale);
        }
        SurfaceGeometry::Procedural { .. } | SurfaceGeometry::Unknown { .. } => {}
    }
}

fn scale_curve_geometry(geometry: &mut CurveGeometry, scale: f64) {
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
            for point in &mut curve.control_points {
                scale_point3(point, scale);
            }
        }
        CurveGeometry::Polyline {
            points,
            chordal_deflection,
            ..
        } => {
            for point in points {
                scale_point3(point, scale);
            }
            *chordal_deflection *= scale;
        }
        CurveGeometry::Transformed {
            basis, transform, ..
        } => {
            scale_curve_geometry(basis, scale);
            scale_transform_translation(transform, scale);
        }
        CurveGeometry::Composite { .. }
        | CurveGeometry::Procedural { .. }
        | CurveGeometry::Unknown { .. } => {}
    }
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
        | CurveGeometry::Polyline { .. }
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
        | SurfaceGeometry::Polygonal { .. }
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
        PcurveGeometry::Nurbs { control_points, .. } => {
            for point in control_points {
                *point = scale_point(*point);
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
            if !u_scale.is_finite()
                || !v_scale.is_finite()
                || u_scale == 0.0
                || v_scale == 0.0
                || !transform.is_affine()
            {
                return false;
            }
            transform.rows[0][1] *= u_scale / v_scale;
            transform.rows[0][2] *= u_scale;
            transform.rows[1][0] *= v_scale / u_scale;
            transform.rows[1][2] *= v_scale;
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

fn scale_sketch_geometry(geometry: &mut SketchGeometry, scale: f64) {
    match geometry {
        SketchGeometry::Point { position } => scale_point2(position, scale),
        SketchGeometry::Line { start, end } => {
            scale_point2(start, scale);
            scale_point2(end, scale);
        }
        SketchGeometry::ReferenceLine { origin, .. } => scale_point2(origin, scale),
        SketchGeometry::Circle { center, radius } => {
            scale_point2(center, scale);
            radius.0 *= scale;
        }
        SketchGeometry::Arc { center, radius, .. } => {
            scale_point2(center, scale);
            radius.0 *= scale;
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
            major_radius.0 *= scale;
            minor_radius.0 *= scale;
        }
        SketchGeometry::Parabola {
            vertex,
            focal_length,
            ..
        } => {
            scale_point2(vertex, scale);
            focal_length.0 *= scale;
        }
        SketchGeometry::Nurbs { control_points, .. } => {
            for point in control_points {
                scale_point2(point, scale);
            }
        }
        SketchGeometry::Text {
            height, placement, ..
        } => {
            height.0 *= scale;
            if let Some(placement) = placement {
                scale_point2(&mut placement.anchor, scale);
            }
        }
        SketchGeometry::ExternalReference { .. } | SketchGeometry::Native { .. } => {}
    }
}

fn scale_spatial_sketch_geometry(geometry: &mut SpatialSketchGeometry, scale: f64) {
    match geometry {
        SpatialSketchGeometry::Point { position } => scale_point3(position, scale),
        SpatialSketchGeometry::Line { start, end } => {
            scale_point3(start, scale);
            scale_point3(end, scale);
        }
        SpatialSketchGeometry::Circle { center, radius, .. }
        | SpatialSketchGeometry::Arc { center, radius, .. } => {
            scale_point3(center, scale);
            radius.0 *= scale;
        }
        SpatialSketchGeometry::Nurbs { control_points, .. } => {
            for point in control_points {
                scale_point3(point, scale);
            }
        }
        SpatialSketchGeometry::NurbsSurface { control_points, .. } => {
            for row in control_points {
                for point in row {
                    scale_point3(point, scale);
                }
            }
        }
        SpatialSketchGeometry::Native { .. } => {}
    }
}

fn scale_sketch_constraint_definition(
    definition: &mut cadmpeg_ir::sketches::SketchConstraintDefinition,
    scale: f64,
) {
    use cadmpeg_ir::sketches::SketchConstraintDefinition;

    match definition {
        SketchConstraintDefinition::PointCoordinateValues { values, .. } => {
            for value in values {
                scale_length(value, scale);
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
        } => scale_length(value, scale),
        _ => {}
    }
}

fn scale_spatial_sketch_constraint_definition(
    definition: &mut cadmpeg_ir::sketches::SpatialSketchConstraintDefinition,
    scale: f64,
) {
    if let cadmpeg_ir::sketches::SpatialSketchConstraintDefinition::Offset { distance, .. } =
        definition
    {
        scale_length(distance, scale);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS_UNIT_SCALE: f64 = f64::EPSILON * 4096.0;

    use cadmpeg_ir::features::{
        BooleanOp, ExtrudeDirection, ExtrudeExtent, ExtrudeSide, ExtrudeStart, FaceMotion, Feature,
        FeatureDefinition, FuzzyTolerance, LinearTermination, PatternKind, PatternScaleCenter,
        ProfileRef,
    };

    #[test]
    fn scales_model_geometry_and_feature_dimensions() {
        let mut ir = CadIr::empty();
        ir.model.features.push(Feature::new(
            cadmpeg_ir::features::FeatureId::from("feature"),
            0,
            FeatureDefinition::Extrude {
                profile: ProfileRef::Unresolved("profile".into()),
                direction: ExtrudeDirection::ProfileNormal,
                start: ExtrudeStart::OffsetProfilePlane {
                    offset: Length(2.0),
                },
                extent: ExtrudeExtent::TwoSided {
                    first: ExtrudeSide {
                        termination: LinearTermination::Blind {
                            length: Length(3.0),
                        },
                        draft: None,
                    },
                    second: ExtrudeSide {
                        termination: LinearTermination::ToFace {
                            face: cadmpeg_ir::features::FaceSelection::Native("face".into()),
                            offset: Some(Length(4.0)),
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
                id: cadmpeg_ir::features::ParameterId("length".into()),
                owner: None,
                ordinal: 0,
                name: "length".into(),
                expression: "2".into(),
                display: None,
                value: Some(ParameterValue::Length(Length(5.0))),
                dependencies: Vec::new(),
                properties: BTreeMap::new(),
                pmi: None,
                native_ref: None,
            });
        normalize_model_lengths(&mut ir, 25.4);

        let FeatureDefinition::Extrude { start, extent, .. } = &ir.model.features[0].definition
        else {
            panic!("test feature changed family");
        };
        let ExtrudeStart::OffsetProfilePlane { offset } = start else {
            panic!("test start changed family");
        };
        assert_close(offset.0, 50.8);
        let ExtrudeExtent::TwoSided { first, second } = extent else {
            panic!("test extent changed family");
        };
        let LinearTermination::Blind { length } = &first.termination else {
            panic!("test termination changed family");
        };
        assert_close(length.0, 76.2);
        let LinearTermination::ToFace {
            offset: Some(offset),
            ..
        } = &second.termination
        else {
            panic!("test offset termination changed family");
        };
        assert_close(offset.0, 101.6);
        let Some(ParameterValue::Length(length)) = ir.model.parameters[0].value.as_ref() else {
            panic!("test parameter changed family");
        };
        assert_close(length.0, 127.0);
    }

    #[test]
    fn scales_face_motion_lengths_and_origins() {
        let mut translate = FaceMotion::Translate {
            direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            distance: Length(2.0),
        };
        scale_face_motion(&mut translate, 25.4);
        let FaceMotion::Translate {
            direction,
            distance,
        } = translate
        else {
            panic!("test motion changed family");
        };
        assert_eq!(direction, cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0));
        assert_close(distance.0, 50.8);

        let mut rotate = FaceMotion::Rotate {
            axis_origin: Point3::new(1.0, 2.0, 3.0),
            axis_dir: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            angle: cadmpeg_ir::features::Angle(0.5),
        };
        scale_face_motion(&mut rotate, 25.4);
        let FaceMotion::Rotate {
            axis_origin,
            axis_dir,
            angle,
        } = rotate
        else {
            panic!("test motion changed family");
        };
        assert_point3(axis_origin, [25.4, 50.8, 76.2]);
        assert_eq!(axis_dir, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0));
        assert_close(angle.0, 0.5);
    }

    #[test]
    fn scales_explicit_pattern_scale_center() {
        let mut pattern = PatternKind::Scale {
            center: PatternScaleCenter::Point(Point3::new(1.0, 2.0, 3.0)),
            final_factor: 2.0,
            count: 3,
        };
        scale_pattern_kind(&mut pattern, 25.4);
        let PatternKind::Scale {
            center,
            final_factor,
            count,
        } = pattern
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
            fuzzy_tolerance: FuzzyTolerance::Explicit(2.0),
        };

        scale_feature_definition(&mut definition, 25.4);

        let FeatureDefinition::PostProcess {
            fuzzy_tolerance, ..
        } = definition
        else {
            panic!("test definition changed family");
        };
        let FuzzyTolerance::Explicit(value) = fuzzy_tolerance else {
            panic!("test tolerance changed family");
        };
        assert_close(value, 50.8);
    }

    #[test]
    fn scales_procedural_model_lengths_and_cache_tolerances() {
        let mut ir = CadIr::empty();
        let surface_id = cadmpeg_ir::ids::SurfaceId("surface".into());
        ir.model.surfaces.push(cadmpeg_ir::geometry::Surface {
            id: surface_id.clone(),
            geometry: cadmpeg_ir::geometry::SurfaceGeometry::Unknown { record: None },
            source_object: None,
        });
        let surface = cadmpeg_ir::geometry::ProceduralSurface::try_new(
            cadmpeg_ir::ids::ProceduralSurfaceId("surface-construction".into()),
            cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Extrusion {
                directrix: cadmpeg_ir::ids::CurveId("directrix".into()),
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
        let curve_id = cadmpeg_ir::ids::CurveId("curve".into());
        ir.model.curves.push(cadmpeg_ir::geometry::Curve {
            id: curve_id.clone(),
            geometry: cadmpeg_ir::geometry::CurveGeometry::Unknown { record: None },
            source_object: None,
        });
        let curve = cadmpeg_ir::geometry::ProceduralCurve::try_new(
            cadmpeg_ir::ids::ProceduralCurveId("curve-construction".into()),
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

        normalize_model_lengths(&mut ir, 25.4);

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

        scale_surface_geometry(&mut surface, 25.4);
        scale_curve_geometry(&mut curve, 25.4);

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
