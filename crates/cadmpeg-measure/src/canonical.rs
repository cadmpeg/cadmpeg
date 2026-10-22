// SPDX-License-Identifier: Apache-2.0
//! Canonical carriers: the face's surface with its placement chain and the
//! body placement composed, the canonical kind, and the reported parameters.

pub(crate) mod curve;
pub(crate) mod placement;
pub(crate) mod surface;

use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::source_fidelity::SourceFidelity;

use crate::failure::{Measure, MeasureFailure};
use crate::reason::UnavailableReason;
use crate::report::{
    CarrierMeasures, CarrierParameters, NormalDirection, PlacementClass, PlacementReport, TorusForm,
};
use crate::select::SurfaceKind;
use crate::value::{
    measured, Errors, Method, Point, Quantity, Scalar, SourceExactness, SourceField, Unit,
};
use placement::Placement;
use surface::{Chart, ConeChart, CylinderChart, PlaneChart, SphereChart, TorusChart};

/// How the source established one field of one entity.
pub(crate) fn exactness(
    fidelity: Option<&SourceFidelity>,
    entity: &str,
    field: &str,
) -> SourceExactness {
    let Some(fidelity) = fidelity else {
        return SourceExactness::NotRecorded;
    };
    let Some(note) = fidelity.annotations.exactness().get(entity) else {
        return SourceExactness::ByteExact;
    };
    let exactness = note.fields().get(field).copied().unwrap_or(note.entity());
    match exactness {
        cadmpeg_ir::Exactness::ByteExact => SourceExactness::ByteExact,
        cadmpeg_ir::Exactness::Derived => SourceExactness::Derived,
        cadmpeg_ir::Exactness::Inferred => SourceExactness::Inferred,
        cadmpeg_ir::Exactness::Unknown => SourceExactness::Unknown,
    }
}

/// Where a carrier's stored fields live: the surface, its placement chain
/// depth, and the body that places it.
#[derive(Clone, Copy)]
pub(crate) struct Sources<'a> {
    pub(crate) fidelity: Option<&'a SourceFidelity>,
    pub(crate) surface: &'a str,
    pub(crate) depth: usize,
    pub(crate) body: Option<&'a str>,
}

impl Sources<'_> {
    fn prefix(depth: usize) -> String {
        let mut path = String::from("geometry");
        for _ in 0..depth {
            path.push_str(".basis");
        }
        path
    }

    /// The leaf fields `names` and, for a placed value, every transform of
    /// the chain and the body transform.
    fn fields(&self, names: &[&str], placed: bool) -> Vec<SourceField> {
        let prefix = Self::prefix(self.depth);
        let mut fields: Vec<SourceField> = names
            .iter()
            .map(|name| {
                let field = format!("{prefix}.{name}");
                let exactness = exactness(self.fidelity, self.surface, &field);
                SourceField::new(self.surface, field, exactness)
            })
            .collect();
        if placed {
            for depth in 0..self.depth {
                let field = format!("{}.transform", Self::prefix(depth));
                let exactness = exactness(self.fidelity, self.surface, &field);
                fields.push(SourceField::new(self.surface, field, exactness));
            }
            if let Some(body) = self.body {
                let exactness = exactness(self.fidelity, body, "transform");
                fields.push(SourceField::new(body, "transform", exactness));
            }
        }
        fields
    }
}

/// The analytic chart of a carrier, or why it has none.
#[derive(Debug, Clone)]
pub(crate) enum Shape {
    Analytic(Chart),
    Unsupported {
        reason: UnavailableReason,
        message: String,
    },
}

/// A face's carrier in model space.
#[derive(Debug, Clone)]
pub(crate) struct Carrier<'ir> {
    pub(crate) surface: &'ir Surface,
    pub(crate) kind: SurfaceKind,
    pub(crate) chain: Vec<String>,
    pub(crate) placement: Placement,
    pub(crate) depth: usize,
    pub(crate) leaf: Option<&'ir SolvedSurfaceGeometry>,
    pub(crate) shape: Shape,
}

fn leaf_kind(geometry: &SolvedSurfaceGeometry) -> (SurfaceKind, &'static str) {
    match geometry {
        SolvedSurfaceGeometry::Plane(_) => (SurfaceKind::Plane, "plane"),
        SolvedSurfaceGeometry::Cylinder(_) => (SurfaceKind::Cylinder, "cylinder"),
        SolvedSurfaceGeometry::Cone(_) => (SurfaceKind::Cone, "cone"),
        SolvedSurfaceGeometry::Sphere(_) => (SurfaceKind::Sphere, "sphere"),
        SolvedSurfaceGeometry::Torus(_) => (SurfaceKind::Torus, "torus"),
        SolvedSurfaceGeometry::Nurbs(_) => (SurfaceKind::Nurbs, "nurbs"),
        SolvedSurfaceGeometry::Polygonal(_) => (SurfaceKind::Polygonal, "polygonal"),
        SolvedSurfaceGeometry::Transformed(_) => (SurfaceKind::Unknown, "transformed"),
        SolvedSurfaceGeometry::Unknown { .. } => (SurfaceKind::Unknown, "unknown"),
    }
}

impl<'ir> Carrier<'ir> {
    /// The carrier of `surface` placed by `body`.
    pub(crate) fn resolve(surface: &'ir Surface, body: Placement) -> Measure<Self> {
        match &surface.geometry {
            SurfaceGeometry::Procedural { .. } => Ok(Self {
                surface,
                kind: SurfaceKind::Procedural,
                chain: vec!["procedural".to_owned()],
                placement: body,
                depth: 0,
                leaf: None,
                shape: Shape::Unsupported {
                    reason: UnavailableReason::CarrierProceduralUnevaluated,
                    message: "procedural carriers are not measured in this build".to_owned(),
                },
            }),
            SurfaceGeometry::Solved(geometry) => {
                let mut chain = Vec::new();
                let mut placement = Placement::identity();
                let mut depth = 0;
                let mut current = geometry;
                while let SolvedSurfaceGeometry::Transformed(placed) = current {
                    chain.push("transformed".to_owned());
                    placement = Placement::of(*placed.transform())?.then(placement)?;
                    depth += 1;
                    current = placed.basis();
                }
                let (kind, name) = leaf_kind(current);
                chain.push(name.to_owned());
                let placement = placement.then(body)?;
                let shape = analytic_chart(current, placement);
                Ok(Self {
                    surface,
                    kind,
                    chain,
                    placement,
                    depth,
                    leaf: Some(current),
                    shape,
                })
            }
        }
    }

    /// The surface ID.
    pub(crate) fn id(&self) -> &'ir str {
        self.surface.id.as_str()
    }

    /// The analytic chart, or why the carrier has none.
    pub(crate) fn chart(&self) -> Measure<&Chart> {
        match &self.shape {
            Shape::Analytic(chart) => Ok(chart),
            Shape::Unsupported { reason, message } => Err(MeasureFailure::unavailable(
                *reason,
                Some(self.id()),
                message.clone(),
            )),
        }
    }

    /// The carrier row: kind, chain, placement and parameters.
    pub(crate) fn measures(&self, sources: Sources<'_>) -> CarrierMeasures {
        let placement = PlacementReport {
            class: self.placement.class,
            scale: (self.placement.class == PlacementClass::Similarity)
                .then_some(self.placement.scale),
            reflection: (self.placement.class != PlacementClass::Identity)
                .then_some(self.placement.reflection),
        };
        CarrierMeasures {
            surface: self.id().to_owned(),
            kind: self.kind,
            chain: self.chain.clone(),
            analytic: self.kind.analytic(),
            placement,
            parameters: self.parameters(sources),
        }
    }

    fn parameters(&self, sources: Sources<'_>) -> CarrierParameters {
        let Some(leaf) = self.leaf else {
            return CarrierParameters::None {};
        };
        Parameters {
            sources,
            placement: self.placement,
            entity: self.id(),
        }
        .of(leaf, &self.shape)
    }
}

/// The analytic chart of a leaf under a composed placement.
fn analytic_chart(geometry: &SolvedSurfaceGeometry, placement: Placement) -> Shape {
    let unsupported = |reason, message: &str| Shape::Unsupported {
        reason,
        message: message.to_owned(),
    };
    match geometry {
        SolvedSurfaceGeometry::Nurbs(_) => {
            return unsupported(
                UnavailableReason::RegionDomainUnsupported,
                "NURBS faces are not measured in this build",
            )
        }
        SolvedSurfaceGeometry::Polygonal(_) => {
            return unsupported(
                UnavailableReason::CarrierPolygonal,
                "bounded measures are not computed on polygonal carriers",
            )
        }
        SolvedSurfaceGeometry::Unknown { .. } | SolvedSurfaceGeometry::Transformed(_) => {
            return unsupported(
                UnavailableReason::CarrierUnknown,
                "the carrier is stored as an unknown shape",
            )
        }
        _ => {}
    }
    if !placement.is_similarity() {
        return unsupported(
            UnavailableReason::PlacementNonSimilarity,
            "the composed placement is not a similarity",
        );
    }
    let s = placement.scale;
    let frame = |axis: Vector3, reference: Vector3| {
        (
            placement.rotate(reference),
            placement.rotate(axis.cross(reference)),
            placement.rotate(axis),
        )
    };
    Shape::Analytic(match geometry {
        SolvedSurfaceGeometry::Plane(plane) => {
            let (eu, ev, _) = frame(
                *plane.frame().axis().as_raw(),
                *plane.frame().reference().as_raw(),
            );
            Chart::Plane(PlaneChart {
                origin: placement.point(plane.origin().get()),
                eu,
                ev,
                h: s,
            })
        }
        SolvedSurfaceGeometry::Cylinder(cylinder) => {
            let (e1, e2, axis) = frame(
                *cylinder.frame().axis().as_raw(),
                *cylinder.frame().reference().as_raw(),
            );
            Chart::Cylinder(CylinderChart {
                origin: placement.point(cylinder.origin().get()),
                e1,
                e2,
                axis,
                radius: cylinder.radius().get() * s,
                h: s,
            })
        }
        SolvedSurfaceGeometry::Cone(cone) => {
            let (e1, e2, axis) = frame(
                *cone.frame().axis().as_raw(),
                *cone.frame().reference().as_raw(),
            );
            Chart::Cone(ConeChart {
                origin: placement.point(cone.origin().get()),
                e1,
                e2,
                axis,
                rho0: cone.radius().get() * s,
                tau: cone.half_angle().get().tan() * s,
                ratio: cone.ratio().get(),
                h: s,
            })
        }
        SolvedSurfaceGeometry::Sphere(sphere) => {
            let (e1, e2, axis) = frame(
                *sphere.frame().axis().as_raw(),
                *sphere.frame().reference().as_raw(),
            );
            Chart::Sphere(SphereChart {
                center: placement.point(sphere.center().get()),
                e1,
                e2,
                axis,
                radius: sphere.radius().get() * s,
            })
        }
        SolvedSurfaceGeometry::Torus(torus) => {
            let (e1, e2, axis) = frame(
                *torus.frame().axis().as_raw(),
                *torus.frame().reference().as_raw(),
            );
            Chart::Torus(TorusChart {
                center: placement.point(torus.center().get()),
                e1,
                e2,
                axis,
                major: torus.major_radius().get() * s,
                minor: torus.minor_radius().get() * s,
            })
        }
        SolvedSurfaceGeometry::Nurbs(_)
        | SolvedSurfaceGeometry::Polygonal(_)
        | SolvedSurfaceGeometry::Transformed(_)
        | SolvedSurfaceGeometry::Unknown { .. } => {
            return unsupported(
                UnavailableReason::CarrierUnknown,
                "the carrier has no analytic chart",
            )
        }
    })
}

/// Builds parameter values with their method and provenance.
struct Parameters<'a> {
    sources: Sources<'a>,
    placement: Placement,
    entity: &'a str,
}

impl Parameters<'_> {
    fn identity(&self) -> bool {
        self.placement.class == PlacementClass::Identity
    }

    fn rigid(&self) -> bool {
        matches!(
            self.placement.class,
            PlacementClass::Identity | PlacementClass::Rigid
        )
    }

    /// A length read from `field` and scaled by the placement. `stored` is
    /// the magnitude of the stored value; `exact` states whether it equals
    /// the stored value bit for bit.
    fn length(&self, field: &str, stored: f64, exact: bool) -> Quantity<Scalar> {
        if !self.placement.is_similarity() {
            return non_similarity(self.entity);
        }
        let rigid = self.rigid();
        let value = if rigid {
            stored
        } else {
            stored * self.placement.scale
        };
        let method = if rigid && exact {
            Method::Source
        } else {
            Method::ClosedForm
        };
        let model = self.placement.model_term(1, value);
        with_fields(
            measured(
                Scalar::new(value),
                Unit::Millimeter,
                method,
                Errors::new(0.0, model),
                self.entity,
            ),
            self.sources.fields(&[field], !rigid),
        )
    }

    /// A dimensionless or angular scalar invariant under a similarity.
    fn invariant(&self, fields: &[&str], value: f64, exact: bool, unit: Unit) -> Quantity<Scalar> {
        if !self.placement.is_similarity() {
            return non_similarity(self.entity);
        }
        let method = if self.rigid() && exact {
            Method::Source
        } else {
            Method::ClosedForm
        };
        with_fields(
            measured(Scalar::new(value), unit, method, Errors::ZERO, self.entity),
            self.sources.fields(fields, !self.rigid()),
        )
    }

    /// A model-space point: the stored value under the identity, the placed
    /// value otherwise.
    fn point(&self, fields: &[&str], stored: Point3, placed: Point3) -> Quantity<Point> {
        let (value, method) = if self.identity() {
            (stored, Method::Source)
        } else {
            (placed, Method::ClosedForm)
        };
        with_fields(
            measured(
                Point::new(value),
                Unit::Millimeter,
                method,
                Errors::ZERO,
                self.entity,
            ),
            self.sources.fields(fields, !self.identity()),
        )
    }

    /// A unit direction: the stored value under the identity when `stored`
    /// is given, the normalized placed value otherwise.
    fn direction(
        &self,
        fields: &[&str],
        stored: Option<Vector3>,
        placed: Vector3,
    ) -> Quantity<Point> {
        let (value, method) = match stored {
            Some(stored) if self.identity() => (Some(stored), Method::Source),
            _ => (placed.unit(), Method::ClosedForm),
        };
        let point = value.and_then(|unit| Point::new(Point3::new(unit.x, unit.y, unit.z)));
        with_fields(
            measured(point, Unit::One, method, Errors::ZERO, self.entity),
            self.sources.fields(fields, !self.identity()),
        )
    }

    fn diameter(&self, field: &str, radius: &Quantity<Scalar>) -> Quantity<Scalar> {
        match radius {
            Quantity::Measured(radius) => {
                let value = 2.0 * radius.value().value.get();
                with_fields(
                    measured(
                        Scalar::new(value),
                        Unit::Millimeter,
                        Method::ClosedForm,
                        Errors::new(0.0, 2.0 * radius.model_uncertainty().get()),
                        self.entity,
                    ),
                    self.sources.fields(&[field], !self.rigid()),
                )
            }
            Quantity::Unavailable(unavailable) => Quantity::Unavailable(unavailable.recast()),
        }
    }

    fn of(&self, leaf: &SolvedSurfaceGeometry, shape: &Shape) -> CarrierParameters {
        let chart = match shape {
            Shape::Analytic(chart) => Some(chart),
            Shape::Unsupported { .. } => None,
        };
        match (leaf, chart) {
            (SolvedSurfaceGeometry::Plane(plane), Some(Chart::Plane(chart))) => {
                CarrierParameters::Plane {
                    origin: self.point(&["origin"], plane.origin().get(), chart.origin),
                    normal: self.direction(
                        &["normal", "u_axis"],
                        Some(*plane.frame().axis().as_raw()),
                        chart.eu.cross(chart.ev),
                    ),
                    u_axis: self.direction(
                        &["u_axis"],
                        Some(*plane.frame().reference().as_raw()),
                        chart.eu,
                    ),
                }
            }
            (SolvedSurfaceGeometry::Cylinder(cylinder), Some(Chart::Cylinder(chart))) => {
                let radius = self.length("radius", cylinder.radius().get(), true);
                let diameter = self.diameter("radius", &radius);
                CarrierParameters::Cylinder {
                    radius,
                    diameter,
                    axis_point: self.point(&["origin"], cylinder.origin().get(), chart.origin),
                    axis_direction: self.direction(
                        &["axis"],
                        Some(*cylinder.frame().axis().as_raw()),
                        chart.axis,
                    ),
                    ref_direction: self.direction(
                        &["ref_direction"],
                        Some(*cylinder.frame().reference().as_raw()),
                        chart.e1,
                    ),
                }
            }
            (SolvedSurfaceGeometry::Cone(cone), Some(Chart::Cone(chart))) => {
                let stored = cone.half_angle().get();
                let slope = stored.tan();
                let half = slope.atan().abs();
                let exact = half.to_bits() == stored.to_bits();
                let half_angle = self.invariant(&["half_angle"], half, exact, Unit::Radian);
                let included_angle = match &half_angle {
                    Quantity::Measured(_) => with_fields(
                        measured(
                            Scalar::new(2.0 * half),
                            Unit::Radian,
                            Method::ClosedForm,
                            Errors::ZERO,
                            self.entity,
                        ),
                        self.sources.fields(&["half_angle"], !self.rigid()),
                    ),
                    Quantity::Unavailable(unavailable) => {
                        Quantity::Unavailable(unavailable.recast())
                    }
                };
                let apex = (slope != 0.0).then(|| {
                    let v_apex = -chart.rho0 / chart.tau;
                    with_fields(
                        measured(
                            Point::new(crate::geom::offset(
                                chart.origin,
                                chart.axis.scale(chart.h * v_apex),
                            )),
                            Unit::Millimeter,
                            Method::ClosedForm,
                            Errors::ZERO,
                            self.entity,
                        ),
                        self.sources
                            .fields(&["origin", "axis", "radius", "half_angle"], true),
                    )
                });
                let opening = if slope < 0.0 {
                    chart.axis.scale(-1.0)
                } else {
                    chart.axis
                };
                let stored_axis = (slope >= 0.0).then(|| *cone.frame().axis().as_raw());
                let ratio = (cone.ratio().get() != 1.0)
                    .then(|| self.invariant(&["ratio"], cone.ratio().get(), true, Unit::One));
                CarrierParameters::Cone {
                    half_angle,
                    included_angle,
                    apex,
                    opening_direction: self.direction(
                        &["axis", "half_angle"],
                        stored_axis,
                        opening,
                    ),
                    ratio,
                    reference_radius: self.length("radius", cone.radius().get(), true),
                    reference_point: self.point(&["origin"], cone.origin().get(), chart.origin),
                }
            }
            (SolvedSurfaceGeometry::Sphere(sphere), Some(Chart::Sphere(chart))) => {
                let stored = sphere.radius().get();
                let radius = self.length("radius", stored.abs(), stored > 0.0);
                let diameter = self.diameter("radius", &radius);
                let partials = Chart::Sphere(*chart).partials(0.0, 0.0);
                let outward = partials
                    .du
                    .cross(partials.dv)
                    .dot(partials.point.vector_from(chart.center))
                    > 0.0;
                CarrierParameters::Sphere {
                    radius,
                    diameter,
                    center: self.point(&["center"], sphere.center().get(), chart.center),
                    normal_direction: if outward {
                        NormalDirection::Outward
                    } else {
                        NormalDirection::Inward
                    },
                }
            }
            (SolvedSurfaceGeometry::Torus(torus), Some(Chart::Torus(chart))) => {
                let major = torus.major_radius().get();
                let minor = torus.minor_radius().get();
                let form = if minor.abs() < major {
                    TorusForm::Ring
                } else if minor.abs() == major {
                    TorusForm::Horn
                } else {
                    TorusForm::Spindle
                };
                CarrierParameters::Torus {
                    major_radius: self.length("major_radius", major, true),
                    minor_radius: self.length("minor_radius", minor.abs(), minor > 0.0),
                    center: self.point(&["center"], torus.center().get(), chart.center),
                    axis_direction: self.direction(
                        &["axis"],
                        Some(*torus.frame().axis().as_raw()),
                        chart.axis,
                    ),
                    form,
                }
            }
            _ => CarrierParameters::None {},
        }
    }
}

fn non_similarity<T>(entity: &str) -> Quantity<T> {
    Quantity::missing(
        UnavailableReason::PlacementNonSimilarity,
        Some(entity),
        "the composed placement is not a similarity; radii and angles are not invariant",
    )
}

fn with_fields<T>(quantity: Quantity<T>, fields: Vec<SourceField>) -> Quantity<T> {
    match quantity {
        Quantity::Measured(measured) => Quantity::Measured(measured.with_provenance(fields)),
        unavailable @ Quantity::Unavailable(_) => unavailable,
    }
}

#[cfg(test)]
mod tests;
