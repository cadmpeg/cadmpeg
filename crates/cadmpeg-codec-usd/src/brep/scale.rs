// SPDX-License-Identifier: Apache-2.0
//! Uniform length division of B-rep records: positions, length
//! parameters, the parameters of lines, and the length axes of each
//! surface's `(u, v)`.

use std::collections::BTreeMap;

use super::{Brep, Catalog, CurveKind, Reference, SurfaceKind};

/// Whether a curve parameter is a length: the parameter of a line.
fn curve_is_length(brep: &Brep, curve: Reference) -> bool {
    matches!(curve, (Catalog::Analytic, index)
        if brep.analytic_curves.get(index).map(|curve| curve.kind) == Some(CurveKind::Line))
}

/// Which of a surface's `(u, v)` are lengths.
fn chart_lengths(brep: &Brep, surface: Reference) -> [bool; 2] {
    let (Catalog::Analytic, index) = surface else {
        return [false, false];
    };
    let Some(record) = brep.analytic_surfaces.get(index) else {
        return [false, false];
    };
    let profile = record
        .profile
        .is_some_and(|profile| curve_is_length(brep, profile));
    match record.kind {
        SurfaceKind::Plane => [true, true],
        SurfaceKind::Cylinder | SurfaceKind::Cone => [false, true],
        SurfaceKind::Sphere | SurfaceKind::Torus => [false, false],
        SurfaceKind::Extrusion => [profile, true],
        SurfaceKind::Revolution => [false, profile],
    }
}

/// The indices of `params` that are lengths, for a surface type.
fn surface_length_params(kind: SurfaceKind) -> &'static [usize] {
    match kind {
        SurfaceKind::Plane | SurfaceKind::Extrusion | SurfaceKind::Revolution => &[],
        SurfaceKind::Cylinder | SurfaceKind::Sphere | SurfaceKind::Cone => &[0],
        SurfaceKind::Torus => &[0, 1],
    }
}

fn divide_point<const N: usize>(point: &mut [f64; N], divisor: f64) {
    for value in point.iter_mut() {
        *value /= divisor;
    }
}

fn divide_axes(point: &mut [f64; 2], lengths: [bool; 2], divisor: f64) {
    for (value, length) in point.iter_mut().zip(lengths) {
        if length {
            *value /= divisor;
        }
    }
}

impl Brep {
    /// The records with every length divided by `divisor`.
    pub(crate) fn divided(&self, divisor: f64) -> Self {
        let mut out = self.clone();
        let mut surfaces: BTreeMap<Reference, Reference> = BTreeMap::new();
        for face in &self.faces {
            for oriented in face
                .loops
                .iter()
                .filter_map(|index| self.loops.get(*index))
                .flat_map(|record| record.oriented_edges.iter())
                .filter_map(|index| self.oriented_edges.get(*index))
            {
                if let Some(pcurve) = oriented.pcurve {
                    surfaces.entry(pcurve).or_insert(face.surface);
                }
            }
        }
        for vertex in &mut out.vertices {
            divide_point(vertex, divisor);
        }
        for edge in &mut out.edges {
            if edge.curve.is_some_and(|curve| curve_is_length(self, curve)) {
                divide_point(&mut edge.range, divisor);
            }
            if let Some(tolerance) = &mut edge.tolerance {
                *tolerance /= divisor;
            }
        }
        for surface in &mut out.analytic_surfaces {
            divide_point(&mut surface.frame.position, divisor);
            for index in surface_length_params(surface.kind) {
                if let Some(param) = surface.params.get_mut(*index) {
                    *param /= divisor;
                }
            }
        }
        for surface in &mut out.nurbs_surfaces {
            for point in &mut surface.points {
                divide_point(point, divisor);
            }
        }
        for curve in &mut out.analytic_curves {
            divide_point(&mut curve.frame.position, divisor);
            for param in &mut curve.params {
                *param /= divisor;
            }
        }
        for curve in &mut out.nurbs_curves {
            for point in &mut curve.points {
                divide_point(point, divisor);
            }
        }
        for (index, pcurve) in out.analytic_pcurves.iter_mut().enumerate() {
            let lengths = surfaces
                .get(&(Catalog::Analytic, index))
                .map_or([false, false], |surface| chart_lengths(self, *surface));
            divide_axes(&mut pcurve.position, lengths, divisor);
            for param in &mut pcurve.params {
                *param /= divisor;
            }
        }
        for (index, pcurve) in out.nurbs_pcurves.iter_mut().enumerate() {
            let lengths = surfaces
                .get(&(Catalog::Nurbs, index))
                .map_or([false, false], |surface| chart_lengths(self, *surface));
            for point in &mut pcurve.points {
                divide_axes(point, lengths, divisor);
            }
        }
        out.tolerance /= divisor;
        for tolerance in [
            &mut out.tolerances.faces,
            &mut out.tolerances.edges,
            &mut out.tolerances.vertices,
        ]
        .into_iter()
        .flatten()
        .flatten()
        {
            *tolerance /= divisor;
        }
        out
    }
}

#[cfg(test)]
mod tests;
