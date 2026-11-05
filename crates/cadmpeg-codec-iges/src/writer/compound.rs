// SPDX-License-Identifier: Apache-2.0
//! Exact Type 102 construction from a neutral compound curve.

use super::{CadIr, CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::geometry::{
    CompositeCurveSegment, CompositeCurveSegments, CompositeCurveTransition,
    ProceduralCurveDefinition,
};
use cadmpeg_ir::ids::CurveId;

pub(super) fn geometry(ir: &CadIr, id: &CurveId) -> Option<CurveGeometry> {
    let curve = ir.model.curves.iter().find(|curve| &curve.id == id)?;
    let construction = curve.geometry.procedural_construction()?;
    let procedural = ir
        .model
        .procedural_curves
        .iter()
        .find(|record| &record.id == construction)?;
    let ProceduralCurveDefinition::Compound(compound) = procedural.definition() else {
        return None;
    };
    let components = compound.components();
    let parameters = compound.parameters();
    if compound.legacy_cache().is_some()
        || parameters.len() != components.len() + 1
        || parameters.first()?.get() != 0.0
    {
        return None;
    }
    let mut segments = Vec::with_capacity(components.len());
    let mut previous_end = None;
    for (index, component) in components.iter().enumerate() {
        let child = ir
            .model
            .curves
            .iter()
            .find(|curve| curve.id == component.component)?;
        let range = child.parameter_range?.endpoints();
        let width = range[1] - range[0];
        if !width.is_finite()
            || width <= 0.0
            || parameters[index + 1].get() != parameters[index].get() + width
        {
            return None;
        }
        if range[0] != 0.0 || component.parameter.get() != 0.0 {
            return None;
        }
        let Some(SolvedCurveGeometry::Line(line)) = child.geometry.solved() else {
            return None;
        };
        let origin = line.origin().get();
        if previous_end.is_some_and(|end| end != origin) {
            return None;
        }
        let direction = *line.direction().as_raw();
        let end = cadmpeg_ir::math::Point3::new(
            origin.x + direction.x * range[1],
            origin.y + direction.y * range[1],
            origin.z + direction.z * range[1],
        );
        if !end.x.is_finite() || !end.y.is_finite() || !end.z.is_finite() {
            return None;
        }
        previous_end = Some(end);
        segments.push(CompositeCurveSegment {
            curve: child.id.clone(),
            same_sense: true,
            transition: CompositeCurveTransition::Discontinuous,
        });
    }
    Some(CurveGeometry::Solved(SolvedCurveGeometry::Composite {
        segments: CompositeCurveSegments::try_from(segments).ok()?,
        self_intersect: None,
    }))
}
