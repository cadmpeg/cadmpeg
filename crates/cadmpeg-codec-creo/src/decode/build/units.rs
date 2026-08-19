// SPDX-License-Identifier: Apache-2.0
//! Conversion of neutral Creo values into the canonical IR length unit.
//!
//! The PSB scanner keeps source values in their stored unit so native records
//! remain faithful to the file.  This module is the single boundary at which
//! the already-built neutral model is converted to millimeters.  Directions,
//! angles, ratios, and source-native arenas are intentionally not scaled.

use std::collections::BTreeMap;

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{FeatureDefinition, Length, ParameterValue};
use cadmpeg_ir::geometry::{CurveGeometry, PcurveGeometry, SurfaceGeometry};
use cadmpeg_ir::ids::PcurveId;
use cadmpeg_ir::math::{Point2, Point3};
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
        if let (Some(range), Some(curve_id)) = (
            coedge.use_curve_parameter_range.as_mut(),
            coedge.use_curve.as_ref(),
        ) {
            if let Some(scale) = curve_parameter_scales.get(curve_id) {
                scale_pair(range, *scale);
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
        scale_transform_translation(&mut occurrence.prototype_transform, length_scale_mm);
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
    if let cadmpeg_ir::features::DatumPlaneReference::Face { origin, .. } = reference {
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
            pitch,
            radial_growth,
            ..
        } => {
            scale_point3(axis_origin, scale);
            scale_length(radius, scale);
            scale_length(pitch, scale);
            scale_optional_length(radial_growth, scale);
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
        FeatureDefinition::Wrap { depth, .. } => scale_optional_length(depth, scale),
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
            if let Some(axis) = construction.axis.as_mut() {
                scale_point3(&mut axis.origin, scale);
            }
            if let Some(extent) = construction.extent.as_mut() {
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
            position,
            placements,
            kind,
            exit_kind,
            diameter,
            extent,
            specification,
            ..
        } => {
            if let Some(position) = position {
                scale_point3(position, scale);
            }
            for placement in placements {
                scale_hole_placement(placement, scale);
            }
            scale_hole_kind(kind, scale);
            if let Some(exit_kind) = exit_kind {
                scale_hole_kind(exit_kind, scale);
            }
            scale_optional_length(diameter, scale);
            if let Some(extent) = extent {
                scale_termination(extent, scale);
            }
            if let Some(specification) = specification {
                scale_hole_specification(specification, scale);
            }
        }
        FeatureDefinition::Pattern { pattern, .. } => scale_pattern_kind(pattern, scale),
        FeatureDefinition::PostProcess { operation, .. } => {
            scale_feature_definition(operation, scale);
        }
        _ => {}
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
    scale_termination(&mut side.termination, scale);
    scale_optional_length(&mut side.offset, scale);
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
            scale_termination(termination, scale);
        }
        RevolveExtent::TwoSided { first, second } => {
            scale_termination(first, scale);
            scale_termination(second, scale);
        }
    }
}

fn scale_termination(termination: &mut cadmpeg_ir::features::Termination, scale: f64) {
    use cadmpeg_ir::features::Termination;

    match termination {
        Termination::Blind { length } => scale_length(length, scale),
        Termination::ToFace { offset, .. } => scale_optional_length(offset, scale),
        Termination::OffsetFromFace { offset, .. } => scale_length(offset, scale),
        Termination::Unresolved
        | Termination::ThroughAll
        | Termination::ThroughNext
        | Termination::ToFirst
        | Termination::ToLast
        | Termination::ToVertex { .. }
        | Termination::ToShape { .. }
        | Termination::Angle { .. } => {}
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
        RadiusSpec::Unresolved { .. } => {}
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
        ChamferSpec::Unresolved { .. } => {}
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
    if let cadmpeg_ir::features::FaceMotion::Offset { distance } = motion {
        scale_length(distance, scale);
    }
}

fn scale_flex_mode(mode: &mut cadmpeg_ir::features::FlexMode, scale: f64) {
    use cadmpeg_ir::features::FlexMode;

    match mode {
        FlexMode::Unresolved { distance, .. } => scale_optional_length(distance, scale),
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
        HoleKind::Unresolved {
            counterbore_diameter,
            counterbore_depth,
            countersink_diameter,
            ..
        } => {
            scale_optional_length(counterbore_diameter, scale);
            scale_optional_length(counterbore_depth, scale);
            scale_optional_length(countersink_diameter, scale);
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
        HoleKind::Threaded {
            major_diameter,
            thread_depth,
            pitch,
            ..
        } => {
            scale_length(major_diameter, scale);
            scale_length(thread_depth, scale);
            scale_optional_length(pitch, scale);
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

fn scale_hole_specification(
    specification: &mut cadmpeg_ir::features::HoleSpecification,
    scale: f64,
) {
    scale_optional_length(&mut specification.pitch, scale);
    scale_optional_length(&mut specification.major_diameter, scale);
    scale_optional_length(&mut specification.clearance, scale);
    if let cadmpeg_ir::features::HoleThreadDepth::Blind { depth } = &mut specification.depth {
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
        PatternKind::Unresolved { .. }
        | PatternKind::MirrorReference { .. }
        | PatternKind::Scale { .. } => {}
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
        for vertex_use in &loop_record.vertex_uses {
            for use_record in &vertex_use.pcurves {
                observe_pcurve_scale(&mut candidates, &use_record.pcurve, scales);
            }
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
        SketchGeometry::Text { height, anchor, .. } => {
            height.0 *= scale;
            if let Some(anchor) = anchor {
                scale_point2(anchor, scale);
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
        BooleanOp, ExtrudeDirection, ExtrudeExtent, ExtrudeSide, ExtrudeStart, Feature,
        FeatureDefinition, ProfileRef, Termination,
    };

    #[test]
    fn scales_model_geometry_and_feature_dimensions() {
        let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
        ir.model.features.push(Feature::new(
            cadmpeg_ir::features::FeatureId::from("feature"),
            0,
            FeatureDefinition::Extrude {
                profile: ProfileRef::Unresolved("profile".into()),
                direction: ExtrudeDirection::ProfileNormal,
                start: ExtrudeStart::OffsetProfilePlane {
                    offset: Length(2.0),
                },
                extent: ExtrudeExtent::OneSided {
                    side: ExtrudeSide {
                        termination: Termination::Blind {
                            length: Length(3.0),
                        },
                        draft: None,
                        offset: Some(Length(4.0)),
                    },
                },
                op: BooleanOp::NewBody,
                direction_source: None,
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
        let ExtrudeExtent::OneSided { side } = extent else {
            panic!("test extent changed family");
        };
        let Termination::Blind { length } = &side.termination else {
            panic!("test termination changed family");
        };
        assert_close(length.0, 76.2);
        assert_close(side.offset.expect("test offset").0, 101.6);
        let Some(ParameterValue::Length(length)) = ir.model.parameters[0].value.as_ref() else {
            panic!("test parameter changed family");
        };
        assert_close(length.0, 127.0);
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
}
