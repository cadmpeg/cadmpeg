// SPDX-License-Identifier: Apache-2.0
//! A placed Type 120 written in its source layout.
//!
//! A source Type 120 can state its axis and generatrix in a local frame and
//! place the surface through a Type 124 (IGES 5.3 §4.18). When that layout,
//! placed, still states the current construction, the surface writes in it:
//! the local axis and generatrix are its constituents and the placement is
//! its transformation (#209 §2.3 W2).

use super::{
    append_curve_entity, curve_reference_span, finite, flatten_curve, number, reference_marker,
    same_point, CadIr, CodecError, CurveEntityRequest, Entity, EntityStatus, Placement,
    RevolutionSweep, Sense,
};
use cadmpeg_ir::geometry::{Curve, ProceduralSurface, ProceduralSurfaceDefinition};
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::transform::Transform;

/// Relative tolerance of the placed axis against the construction axis.
const PLACED_AXIS_PARALLEL_TOLERANCE: f64 = 1.0e-9;

/// The source layout of a placed revolution.
pub(super) struct PlacedRevolution<'ir> {
    pub(super) axis: &'ir Curve,
    pub(super) generatrix: &'ir Curve,
    placement: Placement,
}

/// The source layout of `procedural`, when its source Type 120 names a local
/// axis and generatrix and a single Type 124 that place them onto the
/// current construction.
pub(super) fn source<'ir>(
    ir: &'ir CadIr,
    procedural: &ProceduralSurface,
) -> Option<PlacedRevolution<'ir>> {
    let ProceduralSurfaceDefinition::Revolution(revolution) = procedural.definition() else {
        return None;
    };
    let owner = ir.model.procedural_surface_owner(&procedural.id)?;
    let surface = ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == *owner)?;
    let source = surface
        .source_object
        .as_ref()
        .filter(|source| source.format == cadmpeg_ir::CodecFormat::Iges)?;
    let sequence = source.object_id.as_str().strip_prefix('D')?;
    let namespace = ir.native.namespace("iges")?;
    let directory = format!("iges:entity:directory#{sequence}");
    let record = namespace
        .arenas()
        .get("entities")?
        .iter()
        .find(|record| record.id() == directory)?;
    if record.field("entity_type")?.as_i64()? != 120 {
        return None;
    }
    let transform = record
        .field("transform")?
        .as_i64()
        .filter(|value| *value > 0)?;
    let parameters = record.field("parameters")?;
    let pointer = |index: usize| parameters.get(index)?.get("value")?.get("value")?.as_i64();
    let curve = |sequence: i64| {
        let id = format!("iges:model:curve#D{sequence}");
        ir.model.curves.iter().find(|curve| curve.id.as_str() == id)
    };
    let axis = curve(pointer(1)?)?;
    let generatrix = curve(pointer(2)?)?;
    let placement_source = format!("iges:entity:directory#{transform}");
    let placement = namespace
        .arenas()
        .get("transformations")?
        .iter()
        .find(|record| {
            record
                .field("source_entity")
                .and_then(|value| value.as_str().map(str::to_owned))
                == Some(placement_source.clone())
        })?;
    if !placement.field("parent")?.is_null() {
        return None;
    }
    let coefficients = placement.field("coefficients")?;
    let coefficients = coefficients.as_array()?;
    if coefficients.len() != 12 {
        return None;
    }
    let mut rows = [[0.0; 4]; 3];
    for (index, value) in coefficients.iter().enumerate() {
        rows[index / 4][index % 4] = value.as_f64()?;
    }
    let transform = Transform::affine(rows)?;
    if !transform.is_proper_rigid() {
        return None;
    }
    let line = |curve: &Curve| match flatten_curve(curve.geometry.solved()?).ok()? {
        cadmpeg_ir::geometry::CurveGeometry::Solved(
            cadmpeg_ir::geometry::SolvedCurveGeometry::Line(line),
        ) => Some(line),
        _ => None,
    };
    let local_axis = line(axis)?;
    let origin = transform.apply_point(local_axis.origin().get())?.get();
    let direction = transform
        .apply_vector(*local_axis.direction().as_raw())?
        .get();
    let world_direction = *revolution.axis_direction().as_raw();
    let world_origin = revolution.axis_origin().get();
    let offset = Vector3::new(
        origin.x - world_origin.x,
        origin.y - world_origin.y,
        origin.z - world_origin.z,
    );
    let parallel = |vector: Vector3| {
        vector.cross(world_direction).norm()
            <= PLACED_AXIS_PARALLEL_TOLERANCE * vector.norm().max(1.0)
    };
    if !parallel(offset) || !parallel(direction) || direction.dot(world_direction) <= 0.0 {
        return None;
    }
    let directrix = ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id == *revolution.directrix())?;
    let current = curve_reference_span(
        ir,
        &directrix.id,
        &flatten_curve(directrix.geometry.solved()?).ok()?,
    )
    .ok()?;
    let local = curve_reference_span(
        ir,
        &generatrix.id,
        &flatten_curve(generatrix.geometry.solved()?).ok()?,
    )
    .ok()?;
    let start = transform.apply_point(local.start)?.get();
    let end = transform.apply_point(local.end)?.get();
    if !same_point(start, current.start) || !same_point(end, current.end) {
        return None;
    }
    let mut placed = [[FiniteReal::ZERO; 4]; 3];
    for (target, source) in placed.iter_mut().zip(rows) {
        for (target, source) in target.iter_mut().zip(source) {
            *target = FiniteReal::new(source)?;
        }
    }
    Some(PlacedRevolution {
        axis,
        generatrix,
        placement: Placement { rows: placed },
    })
}

/// The constituents and the placed Type 120 entity of `placed`, sweeping
/// `angular_interval`.
pub(super) fn entities(
    ir: &CadIr,
    placed: &PlacedRevolution<'_>,
    angular_interval: cadmpeg_ir::topology::IncreasingParameterInterval,
    base_index: usize,
    version: crate::IgesVersion,
) -> Result<Vec<Entity>, CodecError> {
    let mut entities = Vec::new();
    let mut indices = Vec::with_capacity(2);
    for curve in [placed.axis, placed.generatrix] {
        let geometry = flatten_curve(curve.geometry.solved().ok_or_else(|| {
            CodecError::InvalidInput("IGES curve carrier has no solved geometry".into())
        })?)?;
        let span = curve
            .parameter_range
            .map(|_| curve_reference_span(ir, &curve.id, &geometry))
            .transpose()?;
        let local = append_curve_entity(
            &mut entities,
            ir,
            CurveEntityRequest {
                version,
                curve_id: &curve.id,
                geometry: &geometry,
                span: span.as_ref(),
                sense: Sense::Forward,
                status: EntityStatus::PhysicallyDependent,
                reference_offset: base_index,
            },
        )?;
        indices.push(
            base_index
                .checked_add(local)
                .ok_or_else(|| CodecError::Malformed("IGES entity index overflows".into()))?,
        );
    }
    let start_angle = angular_interval.lower();
    let terminate_angle = RevolutionSweep::classify(angular_interval)?.terminate_angle(start_angle);
    entities.push(Entity {
        structure: None,
        property_links: Vec::new(),
        association_links: Vec::new(),
        line_font: None,
        source_targets: Vec::new(),
        type_code: 120,
        form: 0,
        label: String::new(),
        status: EntityStatus::Independent,
        subscript: 0,
        parameter_body: format!(
            "{},{},{},{};",
            reference_marker(indices[0]),
            reference_marker(indices[1]),
            number(finite(start_angle, "Type 120 start angle")?),
            number(finite(terminate_angle, "Type 120 terminate angle")?)
        )
        .into_bytes(),
        transform: Some(placed.placement),
        transform_link: None,
    });
    Ok(entities)
}
