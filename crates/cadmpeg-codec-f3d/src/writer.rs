// SPDX-License-Identifier: Apache-2.0
//! Native F3D regeneration for supported semantic edits.

use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};

use cadmpeg_ir::codec::{Codec, CodecError, DecodeOptions};
use cadmpeg_ir::design::SketchCurveGeometry;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{Curve, CurveGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::math::{Point3, Vector3};
use zip::write::SimpleFileOptions;

use crate::{asm_header, decode, sab, F3dCodec};

/// Regenerate an F3D archive for supported analytic B-rep carrier edits.
pub fn write_semantic(
    target: &CadIr,
    source_image: &[u8],
    writer: &mut dyn Write,
) -> Result<(), CodecError> {
    let baseline = F3dCodec.decode(&mut Cursor::new(source_image), &DecodeOptions::default())?;
    let baseline_point_ids = baseline
        .ir
        .model
        .points
        .iter()
        .map(|point| point.id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let target_point_ids = target
        .model
        .points
        .iter()
        .map(|point| point.id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    if baseline_point_ids != target_point_ids
        || target.model.points.iter().any(|point| {
            !point.position.x.is_finite()
                || !point.position.y.is_finite()
                || !point.position.z.is_finite()
        })
    {
        return Err(CodecError::NotImplemented(
            "F3D point regeneration requires the unchanged point-id set and finite coordinates"
                .into(),
        ));
    }
    let edited_curves = validate_curve_edits(&baseline.ir.model.curves, &target.model.curves)?;
    let edited_surfaces =
        validate_surface_edits(&baseline.ir.model.surfaces, &target.model.surfaces)?;
    let sketch_point_edits = validate_sketch_point_edits(&baseline.ir, target)?;
    let sketch_curve_edits = validate_sketch_curve_edits(&baseline.ir, target)?;
    let sketch_relation_edits = validate_sketch_relation_edits(&baseline.ir, target)?;
    let mut supported_target = baseline.ir.clone();
    supported_target
        .model
        .points
        .clone_from(&target.model.points);
    supported_target
        .model
        .curves
        .clone_from(&target.model.curves);
    supported_target
        .model
        .surfaces
        .clone_from(&target.model.surfaces);
    if let (Some(supported), Some(target_native)) = (
        supported_target.native.f3d.as_mut(),
        target.native.f3d.as_ref(),
    ) {
        supported
            .sketch_points
            .clone_from(&target_native.sketch_points);
        supported
            .sketch_curve_identities
            .clone_from(&target_native.sketch_curve_identities);
        supported
            .sketch_relations
            .clone_from(&target_native.sketch_relations);
    }
    if decode::semantic_hash(&supported_target) != decode::semantic_hash(target) {
        return Err(CodecError::NotImplemented(
            "modified F3D IR contains edits beyond supported point, line, and plane carriers"
                .into(),
        ));
    }

    let active_brep = baseline
        .ir
        .source
        .as_ref()
        .and_then(|source| source.attributes.get("active_brep"))
        .ok_or_else(|| CodecError::Malformed("F3D baseline has no active BREP".into()))?;
    let positions = target
        .model
        .points
        .iter()
        .map(|point| (point.id.0.clone(), point.position))
        .collect::<BTreeMap<_, _>>();
    let lines = target
        .model
        .curves
        .iter()
        .filter_map(|curve| match curve.geometry {
            CurveGeometry::Line { origin, direction } => edited_curves
                .contains(curve.id.as_str())
                .then(|| (curve.id.0.clone(), (origin, direction))),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let conics = target
        .model
        .curves
        .iter()
        .filter_map(|curve| match curve.geometry {
            CurveGeometry::Circle {
                center,
                axis,
                ref_direction,
                radius,
            } => edited_curves.contains(curve.id.as_str()).then(|| {
                (
                    curve.id.0.clone(),
                    (center, axis, ref_direction, radius, radius),
                )
            }),
            CurveGeometry::Ellipse {
                center,
                axis,
                major_direction,
                major_radius,
                minor_radius,
            } => edited_curves.contains(curve.id.as_str()).then(|| {
                (
                    curve.id.0.clone(),
                    (center, axis, major_direction, major_radius, minor_radius),
                )
            }),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let planes = target
        .model
        .surfaces
        .iter()
        .filter_map(|surface| match surface.geometry {
            SurfaceGeometry::Plane {
                origin,
                normal,
                u_axis,
            } => edited_surfaces
                .contains(surface.id.as_str())
                .then(|| (surface.id.0.clone(), (origin, normal, u_axis))),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let spheres = target
        .model
        .surfaces
        .iter()
        .filter_map(|surface| match surface.geometry {
            SurfaceGeometry::Sphere {
                center,
                axis,
                ref_direction,
                radius,
            } => edited_surfaces
                .contains(surface.id.as_str())
                .then(|| (surface.id.0.clone(), (center, axis, ref_direction, radius))),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let tori = target
        .model
        .surfaces
        .iter()
        .filter_map(|surface| match surface.geometry {
            SurfaceGeometry::Torus {
                center,
                axis,
                ref_direction,
                major_radius,
                minor_radius,
            } => edited_surfaces.contains(surface.id.as_str()).then(|| {
                (
                    surface.id.0.clone(),
                    (center, axis, ref_direction, major_radius, minor_radius),
                )
            }),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let cones = target
        .model
        .surfaces
        .iter()
        .filter_map(|surface| match surface.geometry {
            SurfaceGeometry::Cylinder {
                origin,
                axis,
                ref_direction,
                radius,
            }
            | SurfaceGeometry::Cone {
                origin,
                axis,
                ref_direction,
                radius,
                ..
            } => edited_surfaces
                .contains(surface.id.as_str())
                .then(|| (surface.id.0.clone(), (origin, axis, ref_direction, radius))),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();

    let mut archive = zip::ZipArchive::new(Cursor::new(source_image))
        .map_err(|error| CodecError::Malformed(format!("retained F3D ZIP is invalid: {error}")))?;
    let output = Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(output);
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| CodecError::Malformed(format!("invalid F3D ZIP entry: {error}")))?;
        let name = entry.name().to_owned();
        let options = SimpleFileOptions::default().compression_method(entry.compression());
        if entry.is_dir() {
            zip.add_directory(name, options).map_err(|error| {
                CodecError::Malformed(format!("cannot write F3D directory: {error}"))
            })?;
            continue;
        }
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut bytes)?;
        if name == *active_brep {
            patch_geometry(
                &mut bytes, &positions, &lines, &conics, &planes, &spheres, &tori, &cones,
            )?;
        } else {
            if let Some(edits) = sketch_point_edits.get(&name) {
                patch_sketch_points(&mut bytes, edits)?;
            }
            if let Some(edits) = sketch_curve_edits.get(&name) {
                patch_sketch_curves(&mut bytes, edits)?;
            }
            if let Some(edits) = sketch_relation_edits.get(&name) {
                patch_sketch_relations(&mut bytes, edits)?;
            }
        }
        zip.start_file(name, options)
            .map_err(|error| CodecError::Malformed(format!("cannot write F3D entry: {error}")))?;
        zip.write_all(&bytes)?;
    }
    let output = zip
        .finish()
        .map_err(|error| CodecError::Malformed(format!("cannot finish F3D ZIP: {error}")))?
        .into_inner();
    writer.write_all(&output)?;
    Ok(())
}

type SketchPointEdit = (u64, u32, cadmpeg_ir::math::Point2);

fn validate_sketch_point_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, Vec<SketchPointEdit>>, CodecError> {
    let baseline = baseline
        .native
        .f3d
        .as_ref()
        .map(|native| &native.sketch_points[..])
        .unwrap_or_default();
    let target = target
        .native
        .f3d
        .as_ref()
        .map(|native| &native.sketch_points[..])
        .unwrap_or_default();
    let by_id = baseline
        .iter()
        .map(|point| (point.id.as_str(), point))
        .collect::<BTreeMap<_, _>>();
    if by_id
        .keys()
        .copied()
        .ne(target.iter().map(|point| point.id.as_str()))
    {
        return Err(CodecError::NotImplemented(
            "F3D sketch-point regeneration requires the unchanged point-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, Vec<SketchPointEdit>> = BTreeMap::new();
    for point in target {
        let before = by_id[point.id.as_str()];
        let mut normalized = point.clone();
        normalized.coordinates = before.coordinates;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D sketch-point edit changes fields other than coordinates: {}",
                point.id
            )));
        }
        if point.coordinates == before.coordinates {
            continue;
        }
        if !point.coordinates.u.is_finite() || !point.coordinates.v.is_finite() {
            return Err(CodecError::Malformed(format!(
                "F3D sketch point {} has non-finite coordinates",
                point.id
            )));
        }
        let stream = point
            .id
            .strip_prefix("f3d:")
            .and_then(|id| id.rsplit_once(":sketch-point#"))
            .map(|(stream, _)| stream.to_owned())
            .ok_or_else(|| {
                CodecError::Malformed(format!("invalid sketch-point id {}", point.id))
            })?;
        edits.entry(stream).or_default().push((
            point.byte_offset,
            point.coordinate_offset,
            point.coordinates,
        ));
    }
    Ok(edits)
}

fn patch_sketch_points(bytes: &mut [u8], edits: &[SketchPointEdit]) -> Result<(), CodecError> {
    for (record_offset, coordinate_offset, coordinates) in edits {
        let start = usize::try_from(*record_offset)
            .ok()
            .and_then(|record| record.checked_add(*coordinate_offset as usize))
            .ok_or_else(|| {
                CodecError::Malformed("sketch-point offset exceeds address space".into())
            })?;
        let payload = bytes.get_mut(start..start + 16).ok_or_else(|| {
            CodecError::Malformed("sketch-point coordinate payload is outside BulkStream".into())
        })?;
        payload[..8].copy_from_slice(&(coordinates.u / 10.0).to_le_bytes());
        payload[8..].copy_from_slice(&(coordinates.v / 10.0).to_le_bytes());
    }
    Ok(())
}

type SketchCurveEdit = (u64, u32, SketchCurveGeometry);

fn validate_sketch_curve_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, Vec<SketchCurveEdit>>, CodecError> {
    let baseline = baseline
        .native
        .f3d
        .as_ref()
        .map(|native| &native.sketch_curve_identities[..])
        .unwrap_or_default();
    let target = target
        .native
        .f3d
        .as_ref()
        .map(|native| &native.sketch_curve_identities[..])
        .unwrap_or_default();
    let by_id = baseline
        .iter()
        .map(|curve| (curve.id.as_str(), curve))
        .collect::<BTreeMap<_, _>>();
    if by_id
        .keys()
        .copied()
        .ne(target.iter().map(|curve| curve.id.as_str()))
    {
        return Err(CodecError::NotImplemented(
            "F3D sketch-curve regeneration requires the unchanged curve-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, Vec<SketchCurveEdit>> = BTreeMap::new();
    for curve in target {
        let before = by_id[curve.id.as_str()];
        let mut normalized = curve.clone();
        normalized.geometry.clone_from(&before.geometry);
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D sketch-curve edit changes fields other than geometry: {}",
                curve.id
            )));
        }
        if curve.geometry == before.geometry {
            continue;
        }
        let geometry = curve.geometry.clone().ok_or_else(|| {
            CodecError::NotImplemented(format!("cannot remove sketch-curve geometry: {}", curve.id))
        })?;
        if !valid_sketch_geometry(&geometry)
            || !same_sketch_layout(before.geometry.as_ref(), &geometry)
        {
            return Err(CodecError::NotImplemented(format!(
                "F3D sketch-curve edit requires valid geometry with the original native layout: {}",
                curve.id
            )));
        }
        let stream = curve
            .id
            .strip_prefix("f3d:")
            .and_then(|id| id.rsplit_once(":sketch-curve-identity#"))
            .map(|(stream, _)| stream.to_owned())
            .ok_or_else(|| {
                CodecError::Malformed(format!("invalid sketch-curve id {}", curve.id))
            })?;
        edits
            .entry(stream)
            .or_default()
            .push((curve.byte_offset, curve.geometry_offset, geometry));
    }
    Ok(edits)
}

fn same_sketch_layout(before: Option<&SketchCurveGeometry>, after: &SketchCurveGeometry) -> bool {
    match (before, after) {
        (Some(SketchCurveGeometry::Line { .. }), SketchCurveGeometry::Line { .. })
        | (Some(SketchCurveGeometry::Arc { .. }), SketchCurveGeometry::Arc { .. }) => true,
        (
            Some(SketchCurveGeometry::Nurbs {
                carrier_reference: old_carrier,
                subtype_class_tag: old_tag,
                subtype_record_index: old_index,
                degree: old_degree,
                scalar_width: old_width,
                knots: old_knots,
                weights: old_weights,
                control_points: old_points,
                ..
            }),
            SketchCurveGeometry::Nurbs {
                carrier_reference,
                subtype_class_tag,
                subtype_record_index,
                degree,
                scalar_width,
                knots,
                weights,
                control_points,
                ..
            },
        ) => {
            old_carrier == carrier_reference
                && old_tag == subtype_class_tag
                && old_index == subtype_record_index
                && old_degree == degree
                && old_width == scalar_width
                && old_knots.len() == knots.len()
                && old_weights.len() == weights.len()
                && old_points.len() == control_points.len()
        }
        _ => false,
    }
}

fn valid_sketch_geometry(geometry: &SketchCurveGeometry) -> bool {
    match geometry {
        SketchCurveGeometry::Line {
            start,
            end,
            direction,
            normal,
        } => finite_point(*start) && finite_point(*end) && orthonormal_pair(*direction, *normal),
        SketchCurveGeometry::Arc {
            center,
            normal,
            reference_direction,
            radius,
            start_angle,
            end_angle,
        } => {
            finite_point(*center)
                && orthonormal_pair(*normal, *reference_direction)
                && radius.is_finite()
                && *radius > 0.0
                && start_angle.is_finite()
                && end_angle.is_finite()
        }
        SketchCurveGeometry::Nurbs {
            degree,
            fit_tolerance,
            scalar_width,
            knots,
            weights,
            control_points,
            ..
        } => {
            *scalar_width == 8
                && fit_tolerance.is_finite()
                && *fit_tolerance >= 0.0
                && knots.len() == control_points.len() + *degree as usize + 1
                && knots.iter().all(|knot| knot.is_finite())
                && knots.windows(2).all(|pair| pair[0] <= pair[1])
                && (weights.is_empty() || weights.len() == control_points.len())
                && weights
                    .iter()
                    .all(|weight| weight.is_finite() && *weight > 0.0)
                && control_points.iter().all(|point| finite_point(*point))
        }
    }
}

fn patch_sketch_curves(bytes: &mut [u8], edits: &[SketchCurveEdit]) -> Result<(), CodecError> {
    for (record_offset, geometry_offset, geometry) in edits {
        let start = usize::try_from(*record_offset)
            .ok()
            .and_then(|record| record.checked_add(*geometry_offset as usize))
            .ok_or_else(|| {
                CodecError::Malformed("sketch-curve offset exceeds address space".into())
            })?;
        if let SketchCurveGeometry::Nurbs {
            fit_tolerance,
            knots,
            weights,
            control_points,
            ..
        } = geometry
        {
            patch_sketch_nurbs(bytes, start, *fit_tolerance, knots, weights, control_points)?;
            continue;
        }
        let payload = bytes.get_mut(start..start + 96).ok_or_else(|| {
            CodecError::Malformed("sketch-curve analytic payload is outside BulkStream".into())
        })?;
        let values = match geometry {
            SketchCurveGeometry::Line {
                start,
                end,
                direction,
                normal,
            } => [
                start.x / 10.0,
                start.y / 10.0,
                start.z / 10.0,
                (end.x - start.x) / 10.0,
                (end.y - start.y) / 10.0,
                (end.z - start.z) / 10.0,
                direction.x,
                direction.y,
                direction.z,
                normal.x,
                normal.y,
                normal.z,
            ],
            SketchCurveGeometry::Arc {
                center,
                normal,
                reference_direction,
                radius,
                start_angle,
                end_angle,
            } => [
                center.x / 10.0,
                center.y / 10.0,
                center.z / 10.0,
                normal.x,
                normal.y,
                normal.z,
                reference_direction.x,
                reference_direction.y,
                reference_direction.z,
                radius / 10.0,
                *start_angle,
                *end_angle,
            ],
            SketchCurveGeometry::Nurbs { .. } => unreachable!("NURBS handled before fixed payload"),
        };
        for (ordinal, value) in values.into_iter().enumerate() {
            payload[ordinal * 8..ordinal * 8 + 8].copy_from_slice(&value.to_le_bytes());
        }
    }
    Ok(())
}

fn patch_sketch_nurbs(
    bytes: &mut [u8],
    start: usize,
    fit_tolerance: f64,
    knots: &[f64],
    weights: &[f64],
    control_points: &[Point3],
) -> Result<(), CodecError> {
    let fit_at = start + 94;
    let knots_at = start + 114;
    let weights_header = knots_at + knots.len() * 8;
    let weights_at = weights_header + 12;
    let points_header = weights_at + weights.len() * 8;
    let points_at = points_header + 12;
    let end = points_at + control_points.len() * 24;
    if end > bytes.len() {
        return Err(CodecError::Malformed(
            "sketch NURBS arrays extend beyond BulkStream".into(),
        ));
    }
    bytes[fit_at..fit_at + 8].copy_from_slice(&(fit_tolerance / 10.0).to_le_bytes());
    for (ordinal, value) in knots.iter().enumerate() {
        let at = knots_at + ordinal * 8;
        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    for (ordinal, value) in weights.iter().enumerate() {
        let at = weights_at + ordinal * 8;
        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    for (ordinal, point) in control_points.iter().enumerate() {
        let at = points_at + ordinal * 24;
        for (component, value) in [point.x, point.y, point.z].into_iter().enumerate() {
            let component_at = at + component * 8;
            bytes[component_at..component_at + 8].copy_from_slice(&(value / 10.0).to_le_bytes());
        }
    }
    Ok(())
}

type SketchRelationEdit = (u64, u32, u32);

fn validate_sketch_relation_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, Vec<SketchRelationEdit>>, CodecError> {
    let baseline = baseline
        .native
        .f3d
        .as_ref()
        .map(|native| &native.sketch_relations[..])
        .unwrap_or_default();
    let target = target
        .native
        .f3d
        .as_ref()
        .map(|native| &native.sketch_relations[..])
        .unwrap_or_default();
    let by_id = baseline
        .iter()
        .map(|relation| (relation.id.as_str(), relation))
        .collect::<BTreeMap<_, _>>();
    if by_id
        .keys()
        .copied()
        .ne(target.iter().map(|relation| relation.id.as_str()))
    {
        return Err(CodecError::NotImplemented(
            "F3D sketch-relation regeneration requires the unchanged relation-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, Vec<SketchRelationEdit>> = BTreeMap::new();
    for relation in target {
        let before = by_id[relation.id.as_str()];
        let mut normalized = relation.clone();
        normalized.state = before.state;
        normalized
            .constraint_kinds
            .clone_from(&before.constraint_kinds);
        normalized.unknown_constraint_bits = before.unknown_constraint_bits;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D sketch-relation edit changes fields other than its constraint mask: {}",
                relation.id
            )));
        }
        if relation.state == before.state {
            continue;
        }
        let (kinds, unknown) = crate::design::decode_constraint_kinds(relation.state);
        if kinds != relation.constraint_kinds || unknown != relation.unknown_constraint_bits {
            return Err(CodecError::Malformed(format!(
                "F3D sketch relation {} has a mask inconsistent with its typed constraint kinds",
                relation.id
            )));
        }
        let stream = relation
            .id
            .strip_prefix("f3d:")
            .and_then(|id| id.rsplit_once(":sketch-relation#"))
            .map(|(stream, _)| stream.to_owned())
            .ok_or_else(|| {
                CodecError::Malformed(format!("invalid sketch-relation id {}", relation.id))
            })?;
        edits.entry(stream).or_default().push((
            relation.byte_offset,
            relation.state_offset,
            relation.state,
        ));
    }
    Ok(edits)
}

fn patch_sketch_relations(
    bytes: &mut [u8],
    edits: &[SketchRelationEdit],
) -> Result<(), CodecError> {
    for (record_offset, state_offset, state) in edits {
        let start = usize::try_from(*record_offset)
            .ok()
            .and_then(|record| record.checked_add(*state_offset as usize))
            .ok_or_else(|| {
                CodecError::Malformed("sketch-relation offset exceeds address space".into())
            })?;
        let payload = bytes.get_mut(start..start + 4).ok_or_else(|| {
            CodecError::Malformed("sketch-relation mask is outside BulkStream".into())
        })?;
        payload.copy_from_slice(&state.to_le_bytes());
    }
    Ok(())
}

fn validate_curve_edits(
    baseline: &[Curve],
    target: &[Curve],
) -> Result<std::collections::BTreeSet<String>, CodecError> {
    let baseline = baseline
        .iter()
        .map(|curve| (curve.id.as_str(), &curve.geometry))
        .collect::<BTreeMap<_, _>>();
    let target = target
        .iter()
        .map(|curve| (curve.id.as_str(), &curve.geometry))
        .collect::<BTreeMap<_, _>>();
    if baseline.keys().ne(target.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D curve regeneration requires the unchanged curve-id set".into(),
        ));
    }
    let mut edited = std::collections::BTreeSet::new();
    for (id, before) in baseline {
        let after = target[id];
        if before == after {
            continue;
        }
        edited.insert(id.to_owned());
        let valid = match after {
            CurveGeometry::Line { origin, direction } => {
                finite_point(*origin)
                    && finite_vector(*direction)
                    && (direction.norm() - 1.0).abs() <= 1e-9
            }
            CurveGeometry::Circle {
                center,
                axis,
                ref_direction,
                radius,
            } if matches!(before, CurveGeometry::Circle { .. }) => {
                finite_point(*center)
                    && orthonormal_pair(*axis, *ref_direction)
                    && radius.is_finite()
                    && *radius > 0.0
            }
            CurveGeometry::Ellipse {
                center,
                axis,
                major_direction,
                major_radius,
                minor_radius,
            } if matches!(before, CurveGeometry::Ellipse { .. }) => {
                finite_point(*center)
                    && orthonormal_pair(*axis, *major_direction)
                    && major_radius.is_finite()
                    && minor_radius.is_finite()
                    && *major_radius > 0.0
                    && *minor_radius > 0.0
                    && *minor_radius <= *major_radius
            }
            _ => {
                return Err(CodecError::NotImplemented(format!(
                    "F3D regeneration does not support edits to curve {id}"
                )));
            }
        };
        if !valid {
            return Err(CodecError::Malformed(format!(
                "edited F3D curve {id} has an invalid frame or radius"
            )));
        }
    }
    Ok(edited)
}

fn validate_surface_edits(
    baseline: &[Surface],
    target: &[Surface],
) -> Result<std::collections::BTreeSet<String>, CodecError> {
    let baseline = baseline
        .iter()
        .map(|surface| (surface.id.as_str(), &surface.geometry))
        .collect::<BTreeMap<_, _>>();
    let target = target
        .iter()
        .map(|surface| (surface.id.as_str(), &surface.geometry))
        .collect::<BTreeMap<_, _>>();
    if baseline.keys().ne(target.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D surface regeneration requires the unchanged surface-id set".into(),
        ));
    }
    let mut edited = std::collections::BTreeSet::new();
    for (id, before) in baseline {
        let after = target[id];
        if before == after {
            continue;
        }
        let valid = match after {
            SurfaceGeometry::Plane {
                origin,
                normal,
                u_axis,
            } => finite_point(*origin) && orthonormal_pair(*normal, *u_axis),
            SurfaceGeometry::Sphere {
                center,
                axis,
                ref_direction,
                radius,
            } => {
                finite_point(*center)
                    && orthonormal_pair(*axis, *ref_direction)
                    && radius.is_finite()
                    && *radius != 0.0
            }
            SurfaceGeometry::Torus {
                center,
                axis,
                ref_direction,
                major_radius,
                minor_radius,
            } => {
                finite_point(*center)
                    && orthonormal_pair(*axis, *ref_direction)
                    && major_radius.is_finite()
                    && minor_radius.is_finite()
                    && *major_radius != 0.0
                    && *minor_radius != 0.0
            }
            SurfaceGeometry::Cylinder {
                origin,
                axis,
                ref_direction,
                radius,
            } if matches!(before, SurfaceGeometry::Cylinder { .. }) => {
                finite_point(*origin)
                    && orthonormal_pair(*axis, *ref_direction)
                    && radius.is_finite()
                    && *radius != 0.0
            }
            SurfaceGeometry::Cone {
                origin,
                axis,
                ref_direction,
                radius,
                half_angle,
            } => {
                let unchanged_angle = matches!(
                    before,
                    SurfaceGeometry::Cone {
                        half_angle: old,
                        ..
                    } if old == half_angle
                );
                unchanged_angle
                    && finite_point(*origin)
                    && orthonormal_pair(*axis, *ref_direction)
                    && radius.is_finite()
                    && *radius != 0.0
            }
            _ => {
                return Err(CodecError::NotImplemented(format!(
                    "F3D regeneration does not support edits to surface {id}"
                )));
            }
        };
        if !valid {
            return Err(CodecError::Malformed(format!(
                "edited F3D surface {id} requires a finite orthonormal frame and valid radius"
            )));
        }
        edited.insert(id.to_owned());
    }
    Ok(edited)
}

fn finite_point(point: Point3) -> bool {
    point.x.is_finite() && point.y.is_finite() && point.z.is_finite()
}

fn finite_vector(vector: Vector3) -> bool {
    vector.x.is_finite() && vector.y.is_finite() && vector.z.is_finite()
}

fn orthonormal_pair(first: Vector3, second: Vector3) -> bool {
    finite_vector(first)
        && finite_vector(second)
        && (first.norm() - 1.0).abs() <= 1e-9
        && (second.norm() - 1.0).abs() <= 1e-9
        && (first.x * second.x + first.y * second.y + first.z * second.z).abs() <= 1e-9
}

#[allow(clippy::too_many_arguments)]
fn patch_geometry(
    bytes: &mut [u8],
    positions: &BTreeMap<String, Point3>,
    lines: &BTreeMap<String, (Point3, Vector3)>,
    conics: &BTreeMap<String, (Point3, Vector3, Vector3, f64, f64)>,
    planes: &BTreeMap<String, (Point3, Vector3, Vector3)>,
    spheres: &BTreeMap<String, (Point3, Vector3, Vector3, f64)>,
    tori: &BTreeMap<String, (Point3, Vector3, Vector3, f64, f64)>,
    cones: &BTreeMap<String, (Point3, Vector3, Vector3, f64)>,
) -> Result<(), CodecError> {
    let start = asm_header::record_stream_start(bytes)
        .ok_or_else(|| CodecError::Malformed("active BREP has no SAB record stream".into()))?;
    let limit = asm_header::first_delta_state_offset(bytes).unwrap_or(bytes.len());
    let records = sab::frame(bytes, start, limit, 8)
        .map_err(|error| CodecError::Malformed(format!("cannot frame active BREP: {error}")))?;
    patch_framed_geometry(
        bytes, &records, positions, lines, conics, planes, spheres, tori, cones,
    )
}

#[allow(clippy::too_many_arguments)]
fn patch_framed_geometry(
    bytes: &mut [u8],
    records: &[sab::Record],
    positions: &BTreeMap<String, Point3>,
    lines: &BTreeMap<String, (Point3, Vector3)>,
    conics: &BTreeMap<String, (Point3, Vector3, Vector3, f64, f64)>,
    planes: &BTreeMap<String, (Point3, Vector3, Vector3)>,
    spheres: &BTreeMap<String, (Point3, Vector3, Vector3, f64)>,
    tori: &BTreeMap<String, (Point3, Vector3, Vector3, f64, f64)>,
    cones: &BTreeMap<String, (Point3, Vector3, Vector3, f64)>,
) -> Result<(), CodecError> {
    for record in records {
        let id = format!("f3d:brep:entity#{}", record.index);
        if record.head == "point" {
            if let Some(position) = positions.get(&id) {
                patch_vec3_token(
                    bytes,
                    record,
                    0x13,
                    0,
                    [position.x / 10.0, position.y / 10.0, position.z / 10.0],
                )?;
            }
        } else if record.head == "straight" {
            if let Some((origin, direction)) = lines.get(&id) {
                patch_vec3_token(
                    bytes,
                    record,
                    0x13,
                    0,
                    [origin.x / 10.0, origin.y / 10.0, origin.z / 10.0],
                )?;
                patch_vec3_token(
                    bytes,
                    record,
                    0x14,
                    0,
                    [direction.x, direction.y, direction.z],
                )?;
            }
        } else if record.head == "ellipse" {
            if let Some((center, axis, direction, major_radius, minor_radius)) = conics.get(&id) {
                patch_vec3_token(
                    bytes,
                    record,
                    0x13,
                    0,
                    [center.x / 10.0, center.y / 10.0, center.z / 10.0],
                )?;
                patch_vec3_token(bytes, record, 0x14, 0, [axis.x, axis.y, axis.z])?;
                let major = major_radius / 10.0;
                patch_vec3_token(
                    bytes,
                    record,
                    0x14,
                    1,
                    [
                        direction.x * major,
                        direction.y * major,
                        direction.z * major,
                    ],
                )?;
                patch_signed_ratio_token(bytes, record, minor_radius / major_radius)?;
            }
        } else if record.head == "plane" {
            if let Some((origin, normal, u_axis)) = planes.get(&id) {
                patch_vec3_token(
                    bytes,
                    record,
                    0x13,
                    0,
                    [origin.x / 10.0, origin.y / 10.0, origin.z / 10.0],
                )?;
                patch_vec3_token(bytes, record, 0x14, 0, [normal.x, normal.y, normal.z])?;
                patch_vec3_token(bytes, record, 0x14, 1, [u_axis.x, u_axis.y, u_axis.z])?;
            }
        } else if record.head == "sphere" {
            if let Some((center, axis, ref_direction, radius)) = spheres.get(&id) {
                patch_vec3_token(
                    bytes,
                    record,
                    0x13,
                    0,
                    [center.x / 10.0, center.y / 10.0, center.z / 10.0],
                )?;
                patch_double_token(bytes, record, 0, radius / 10.0)?;
                patch_vec3_token(
                    bytes,
                    record,
                    0x14,
                    0,
                    [ref_direction.x, ref_direction.y, ref_direction.z],
                )?;
                patch_vec3_token(bytes, record, 0x14, 1, [axis.x, axis.y, axis.z])?;
            }
        } else if record.head == "torus" {
            if let Some((center, axis, ref_direction, major_radius, minor_radius)) = tori.get(&id) {
                patch_vec3_token(
                    bytes,
                    record,
                    0x13,
                    0,
                    [center.x / 10.0, center.y / 10.0, center.z / 10.0],
                )?;
                patch_vec3_token(bytes, record, 0x14, 0, [axis.x, axis.y, axis.z])?;
                patch_double_token(bytes, record, 0, major_radius / 10.0)?;
                patch_double_token(bytes, record, 1, minor_radius / 10.0)?;
                patch_vec3_token(
                    bytes,
                    record,
                    0x14,
                    1,
                    [ref_direction.x, ref_direction.y, ref_direction.z],
                )?;
            }
        } else if record.head == "cone" {
            if let Some((origin, axis, ref_direction, radius)) = cones.get(&id) {
                patch_vec3_token(
                    bytes,
                    record,
                    0x13,
                    0,
                    [origin.x / 10.0, origin.y / 10.0, origin.z / 10.0],
                )?;
                patch_vec3_token(bytes, record, 0x14, 0, [axis.x, axis.y, axis.z])?;
                let scaled_radius = radius / 10.0;
                patch_vec3_token(
                    bytes,
                    record,
                    0x14,
                    1,
                    [
                        ref_direction.x * scaled_radius,
                        ref_direction.y * scaled_radius,
                        ref_direction.z * scaled_radius,
                    ],
                )?;
                patch_double_token(bytes, record, 3, scaled_radius)?;
            }
        }
    }
    Ok(())
}

fn patch_double_token(
    bytes: &mut [u8],
    record: &sab::Record,
    ordinal: usize,
    value: f64,
) -> Result<(), CodecError> {
    let offset = *sab::payload_token_offsets(bytes, record, 8, 0x06)
        .map_err(|error| CodecError::Malformed(error.to_string()))?
        .get(ordinal)
        .ok_or_else(|| {
            CodecError::Malformed(format!(
                "{} record {} lacks double token [{ordinal}]",
                record.head, record.index
            ))
        })?;
    bytes[offset + 1..offset + 9].copy_from_slice(&value.to_le_bytes());
    Ok(())
}

fn patch_signed_ratio_token(
    bytes: &mut [u8],
    record: &sab::Record,
    magnitude: f64,
) -> Result<(), CodecError> {
    let offset = *sab::payload_token_offsets(bytes, record, 8, 0x06)
        .map_err(|error| CodecError::Malformed(error.to_string()))?
        .first()
        .ok_or_else(|| {
            CodecError::Malformed(format!(
                "{} record {} lacks ratio token",
                record.head, record.index
            ))
        })?;
    let raw = f64::from_le_bytes(
        bytes[offset + 1..offset + 9]
            .try_into()
            .expect("framed double token contains eight payload bytes"),
    );
    let signed = if raw.is_sign_negative() {
        -magnitude
    } else {
        magnitude
    };
    bytes[offset + 1..offset + 9].copy_from_slice(&signed.to_le_bytes());
    Ok(())
}

fn patch_vec3_token(
    bytes: &mut [u8],
    record: &sab::Record,
    tag: u8,
    ordinal: usize,
    values: [f64; 3],
) -> Result<(), CodecError> {
    let offset = *sab::payload_token_offsets(bytes, record, 8, tag)
        .map_err(|error| CodecError::Malformed(error.to_string()))?
        .get(ordinal)
        .ok_or_else(|| {
            CodecError::Malformed(format!(
                "{} record {} lacks payload token {tag:#04x}[{ordinal}]",
                record.head, record.index,
            ))
        })?;
    for (component, value) in values.into_iter().enumerate() {
        let at = offset + 1 + component * 8;
        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_straight_record_patches_by_token_boundaries() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"\x0d\x08straight");
        bytes.push(0x13);
        for value in [1.0f64, 2.0, 3.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.push(0x14);
        for value in [1.0f64, 0.0, 0.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.push(0x11);
        let records = sab::frame(&bytes, 0, bytes.len(), 8).expect("generated straight record");
        let lines = BTreeMap::from([(
            "f3d:brep:entity#0".to_string(),
            (Point3::new(40.0, 50.0, 60.0), Vector3::new(0.0, 1.0, 0.0)),
        )]);
        patch_framed_geometry(
            &mut bytes,
            &records,
            &BTreeMap::new(),
            &lines,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
        )
        .expect("generated line edit");
        let decoded =
            sab::frame(&bytes, 0, bytes.len(), 8).expect("patched generated straight record");
        assert!(matches!(
            crate::brep::decode_curve(&decoded[0]),
            Some(CurveGeometry::Line { origin, direction })
                if origin == Point3::new(40.0, 50.0, 60.0)
                    && direction == Vector3::new(0.0, 1.0, 0.0)
        ));
    }

    #[test]
    fn generated_signed_sphere_patches_exact_frame_and_radius() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"\x0d\x06sphere");
        bytes.push(0x13);
        for value in [0.0f64, 0.0, 0.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.push(0x06);
        bytes.extend_from_slice(&1.0f64.to_le_bytes());
        for vector in [[1.0f64, 0.0, 0.0], [0.0, 0.0, 1.0]] {
            bytes.push(0x14);
            for value in vector {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        bytes.push(0x11);
        let records = sab::frame(&bytes, 0, bytes.len(), 8).expect("generated sphere record");
        let spheres = BTreeMap::from([(
            "f3d:brep:entity#0".to_string(),
            (
                Point3::new(10.0, 20.0, 30.0),
                Vector3::new(0.0, 1.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
                -25.0,
            ),
        )]);
        patch_framed_geometry(
            &mut bytes,
            &records,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &spheres,
            &BTreeMap::new(),
            &BTreeMap::new(),
        )
        .expect("generated sphere edit");
        let decoded = sab::frame(&bytes, 0, bytes.len(), 8).expect("patched sphere record");
        assert!(matches!(
            crate::brep::decode_surface(&decoded[0]),
            Some((SurfaceGeometry::Sphere { center, axis, ref_direction, radius }, false))
                if center == Point3::new(10.0, 20.0, 30.0)
                    && axis == Vector3::new(0.0, 1.0, 0.0)
                    && ref_direction == Vector3::new(1.0, 0.0, 0.0)
                    && radius == -25.0
        ));
    }

    #[test]
    fn generated_torus_preserves_signed_self_intersecting_radii() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"\x0d\x05torus");
        bytes.push(0x13);
        for value in [0.0f64, 0.0, 0.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.push(0x14);
        for value in [0.0f64, 0.0, 1.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for value in [1.0f64, 0.25] {
            bytes.push(0x06);
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.push(0x14);
        for value in [1.0f64, 0.0, 0.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.push(0x11);
        let records = sab::frame(&bytes, 0, bytes.len(), 8).expect("generated torus record");
        let tori = BTreeMap::from([(
            "f3d:brep:entity#0".to_string(),
            (
                Point3::new(10.0, 20.0, 30.0),
                Vector3::new(0.0, 1.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
                20.0,
                -35.0,
            ),
        )]);
        patch_framed_geometry(
            &mut bytes,
            &records,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &tori,
            &BTreeMap::new(),
        )
        .expect("generated torus edit");
        let decoded = sab::frame(&bytes, 0, bytes.len(), 8).expect("patched torus record");
        assert!(matches!(
            crate::brep::decode_surface(&decoded[0]),
            Some((SurfaceGeometry::Torus {
                center,
                axis,
                ref_direction,
                major_radius,
                minor_radius,
            }, false))
                if center == Point3::new(10.0, 20.0, 30.0)
                    && axis == Vector3::new(0.0, 1.0, 0.0)
                    && ref_direction == Vector3::new(1.0, 0.0, 0.0)
                    && major_radius == 20.0
                    && minor_radius == -35.0
        ));
    }

    #[test]
    fn generated_cylinder_preserves_native_angle_branch() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"\x0d\x04cone");
        bytes.push(0x13);
        for value in [0.0f64, 0.0, 0.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for vector in [[0.0f64, 0.0, 1.0], [1.0, 0.0, 0.0]] {
            bytes.push(0x14);
            for value in vector {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        for value in [1.0f64, 0.0, -1.0, 1.0] {
            bytes.push(0x06);
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.push(0x11);
        let records = sab::frame(&bytes, 0, bytes.len(), 8).expect("generated cylinder record");
        let cones = BTreeMap::from([(
            "f3d:brep:entity#0".to_string(),
            (
                Point3::new(10.0, 20.0, 30.0),
                Vector3::new(0.0, 1.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
                40.0,
            ),
        )]);
        patch_framed_geometry(
            &mut bytes,
            &records,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &cones,
        )
        .expect("generated cylinder edit");
        let decoded = sab::frame(&bytes, 0, bytes.len(), 8).expect("patched cylinder record");
        assert!(matches!(
            crate::brep::decode_surface(&decoded[0]),
            Some((SurfaceGeometry::Cylinder {
                origin,
                axis,
                ref_direction,
                radius,
            }, false))
                if origin == Point3::new(10.0, 20.0, 30.0)
                    && axis == Vector3::new(0.0, 1.0, 0.0)
                    && ref_direction == Vector3::new(1.0, 0.0, 0.0)
                    && radius == 40.0
        ));
    }

    #[test]
    fn generated_ellipse_preserves_negative_ratio_phase() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"\x0d\x07ellipse");
        bytes.push(0x13);
        for value in [0.0f64, 0.0, 0.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for vector in [[0.0f64, 0.0, 1.0], [1.0, 0.0, 0.0]] {
            bytes.push(0x14);
            for value in vector {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        bytes.push(0x06);
        bytes.extend_from_slice(&(-0.5f64).to_le_bytes());
        bytes.push(0x11);
        let records = sab::frame(&bytes, 0, bytes.len(), 8).expect("generated ellipse record");
        let conics = BTreeMap::from([(
            "f3d:brep:entity#0".to_string(),
            (
                Point3::new(10.0, 20.0, 30.0),
                Vector3::new(0.0, 1.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
                40.0,
                10.0,
            ),
        )]);
        patch_framed_geometry(
            &mut bytes,
            &records,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &conics,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
        )
        .expect("generated ellipse edit");
        let ratio_offset =
            sab::payload_token_offsets(&bytes, &records[0], 8, 0x06).expect("ellipse tokens")[0];
        assert_eq!(
            f64::from_le_bytes(
                bytes[ratio_offset + 1..ratio_offset + 9]
                    .try_into()
                    .expect("ratio payload"),
            ),
            -0.25
        );
        let decoded = sab::frame(&bytes, 0, bytes.len(), 8).expect("patched ellipse record");
        assert!(matches!(
            crate::brep::decode_curve(&decoded[0]),
            Some(CurveGeometry::Ellipse {
                center,
                axis,
                major_direction,
                major_radius,
                minor_radius,
            })
                if center == Point3::new(10.0, 20.0, 30.0)
                    && axis == Vector3::new(0.0, 1.0, 0.0)
                    && major_direction == Vector3::new(1.0, 0.0, 0.0)
                    && major_radius == 40.0
                    && minor_radius == 10.0
        ));
    }
}
