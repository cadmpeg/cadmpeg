// SPDX-License-Identifier: Apache-2.0
//! Conservative parameter domains enclosing written planar trim carriers.

use cadmpeg_ir::geometry::pcurve::PcurveGeometry;
use cadmpeg_ir::math::Point3;

use super::model::{WritableEdgeCurve, WritableFaceSurface, WritableModel};
use super::WriteError;

struct Bounds {
    lower: [f64; 2],
    upper: [f64; 2],
}

impl Bounds {
    fn add(&mut self, point: [f64; 2], owner: &str) -> Result<(), WriteError> {
        if point.iter().any(|value| !value.is_finite()) {
            return Err(WriteError::Omit(format!(
                "{owner} has a non-finite plane parameter bound"
            )));
        }
        for (index, coordinate) in point.into_iter().enumerate() {
            self.lower[index] = self.lower[index].min(coordinate);
            self.upper[index] = self.upper[index].max(coordinate);
        }
        Ok(())
    }

    fn circle(
        &mut self,
        center: [f64; 2],
        extent: [f64; 2],
        owner: &str,
    ) -> Result<(), WriteError> {
        self.add([center[0] - extent[0], center[1] - extent[1]], owner)?;
        self.add([center[0] + extent[0], center[1] + extent[1]], owner)
    }

    fn finish(mut self, owner: &str) -> Result<[[f64; 2]; 2], WriteError> {
        for i in 0..2 {
            if self.lower[i] == self.upper[i] && self.lower[i].is_finite() {
                self.lower[i] = self.lower[i].min(super::PLANE_PARAMETER_DOMAIN[0]);
                self.upper[i] = self.upper[i].max(super::PLANE_PARAMETER_DOMAIN[1]);
            }
        }
        if (0..2).any(|i| {
            !self.lower[i].is_finite()
                || !self.upper[i].is_finite()
                || self.lower[i] >= self.upper[i]
                || !(self.upper[i] - self.lower[i]).is_finite()
        }) {
            return Err(WriteError::Omit(format!(
                "{owner} has no finite increasing plane trim envelope"
            )));
        }
        Ok([
            [self.lower[0], self.upper[0]],
            [self.lower[1], self.upper[1]],
        ])
    }
}

fn compatible_weights(weights: Option<Vec<cadmpeg_ir::scalar::NonZeroReal>>) -> bool {
    weights.is_none_or(|weights| {
        weights.first().is_none_or(|first| {
            weights
                .iter()
                .all(|weight| weight.get().is_sign_negative() == first.get().is_sign_negative())
        })
    })
}

pub(super) fn domain(
    model: &WritableModel<'_>,
    surface_index: usize,
) -> Result<[[f64; 2]; 2], WriteError> {
    let source = model
        .faces
        .iter()
        .find(|face| face.surface == surface_index)
        .ok_or_else(|| {
            WriteError::Omit(format!(
                "plane surface slot {surface_index} has no owning face"
            ))
        })?;
    let surface_id = source.source.surface.as_str();
    let WritableFaceSurface::Plane {
        origin,
        normal,
        u_axis,
        ..
    } = &model.surfaces[surface_index]
    else {
        return Err(WriteError::Omit(format!(
            "surface {surface_id} is not a plane"
        )));
    };
    let mut bounds = Bounds {
        lower: [f64::INFINITY; 2],
        upper: [f64::NEG_INFINITY; 2],
    };
    for coedge in &model.coedges {
        let face = &model.faces[model.loops[coedge.owner_loop].face];
        if face.surface != surface_index {
            continue;
        }
        let edge = &model.edges[coedge.edge];
        let owner = coedge.source.id.as_str();
        if let Some(pcurve) = coedge.explicit_pcurve {
            match &pcurve.geometry {
                PcurveGeometry::Line(line) => {
                    let origin = line.origin().get();
                    let direction = line.direction().as_raw();
                    for t in cadmpeg_ir::scalar::FiniteReal::raw_array(coedge.trim_domain(edge)) {
                        bounds.add(
                            [origin.u + t * direction.u, origin.v + t * direction.v],
                            owner,
                        )?;
                    }
                }
                PcurveGeometry::Circle(circle) => {
                    let center = circle.center().get();
                    let x = circle.x_axis().get();
                    let y = circle.y_axis().get();
                    let radius = circle.radius().get();
                    bounds.circle(
                        [center.u, center.v],
                        [
                            radius * (x.u.abs() + y.u.abs()),
                            radius * (x.v.abs() + y.v.abs()),
                        ],
                        owner,
                    )?;
                }
                PcurveGeometry::Nurbs { nurbs } => {
                    if !compatible_weights(nurbs.weights()) {
                        return Err(WriteError::Omit(format!(
                            "pcurve {} has no proven finite trim envelope",
                            pcurve.id
                        )));
                    }
                    for point in nurbs.control_points() {
                        let point = point.get();
                        bounds.add([point.u, point.v], owner)?;
                    }
                }
                _ => {
                    return Err(WriteError::Omit(format!(
                        "pcurve {} has no exact plane trim envelope",
                        pcurve.id
                    )))
                }
            }
            continue;
        }
        let project = |point: Point3| {
            let uv = super::plane_uv(point, *origin, *u_axis, normal.cross(*u_axis));
            [uv[0], uv[1]]
        };
        match &edge.curve {
            WritableEdgeCurve::Singular | WritableEdgeCurve::Line(_) => {
                bounds.add(project(model.vertices[edge.start].point), owner)?;
                bounds.add(project(model.vertices[edge.end].point), owner)?;
            }
            WritableEdgeCurve::Circle(circle) => {
                let center = project(circle.center().get());
                let radius = circle.radius().get();
                bounds.circle(center, [radius, radius], owner)?;
            }
            WritableEdgeCurve::Nurbs(nurbs) => {
                add_curve(&mut bounds, nurbs, project, edge.source.id.as_str())?;
            }
            WritableEdgeCurve::NurbsOwned(nurbs) => {
                add_curve(&mut bounds, nurbs, project, edge.source.id.as_str())?;
            }
        }
    }
    bounds.finish(&format!("surface {surface_id}"))
}

fn add_curve(
    bounds: &mut Bounds,
    nurbs: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
    project: impl Fn(Point3) -> [f64; 2],
    owner: &str,
) -> Result<(), WriteError> {
    if !compatible_weights(nurbs.weights()) {
        return Err(WriteError::Omit(format!(
            "edge {owner} has no proven finite trim envelope"
        )));
    }
    for point in nurbs.control_points() {
        bounds.add(project(point.get()), owner)?;
    }
    Ok(())
}
