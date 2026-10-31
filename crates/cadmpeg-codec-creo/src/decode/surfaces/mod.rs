// SPDX-License-Identifier: Apache-2.0
//! Native B-rep, prototype surfaces, positional solids, and carrier intersections.

pub(super) mod brep;
pub(super) mod cylinders;
pub(super) mod intersection_candidates;
pub(super) mod intersection_resolve;
pub(super) mod intersections;
pub(super) mod nurbs_boundaries;
pub(super) mod positional;
pub(super) mod prototypes;
pub(super) mod transfer_curves;

use crate::axis::{Axis, Sign};

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, OccurrenceId, ProductDefinitionId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::products::{
    Occurrence, OccurrenceParent, ProductDefinition, ProductDefinitionKind, PrototypeReference,
};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::{AnnotationBuilder, Exactness, SourceObjectAssociation};

use crate::container::ContainerScan;

use super::native::annotate;
use super::uniqueness::UniqueById;

/// Builds the apex cone the positional and legacy carrier routes all state.
///
/// The apex is the frame origin, the radius there is zero and the ratio is one, so the half
/// angle alone states the taper.
fn apex_cone(
    frame: &crate::surface::PositionalFrame,
    half_angle: crate::surface::ApexConeHalfAngle,
) -> cadmpeg_ir::geometry::analytic::ConeSurface {
    cadmpeg_ir::geometry::analytic::ConeSurface::new(
        frame.finite_origin(),
        frame.orthonormal_frame(),
        cadmpeg_ir::scalar::NonNegativeLength::ZERO,
        cadmpeg_ir::scalar::PositiveReal::ONE,
        half_angle.get().into(),
    )
}

/// Select the IR surface namespace and its canonical prefix for a native ID.
///
/// Visible geometry, non-visible geometry, and active-datum rows share the
/// compact native identifier space used by topology links. Keep their source
/// namespaces distinct when only one namespace owns the identifier; visible
/// geometry remains the default for the existing and rowless feature-carrier
/// paths.
fn select_native_surface_namespace(
    visible_present: bool,
    nonvisible_present: bool,
    active_datum_present: bool,
) -> (cadmpeg_ir::ids::IdentityNamespace, &'static str) {
    if visible_present {
        (
            crate::identity::VISIBGEOM_SURFACE,
            "creo:visibgeom:surface#",
        )
    } else if nonvisible_present {
        (
            crate::identity::NOVISGEOM_SURFACE,
            "creo:novisgeom:surface#",
        )
    } else if active_datum_present {
        (crate::identity::ACTDATUM_SURFACE, "creo:actdatums:surface#")
    } else {
        (
            crate::identity::VISIBGEOM_SURFACE,
            "creo:visibgeom:surface#",
        )
    }
}

/// The native surface rows of a scan, indexed once by compact identifier.
pub(super) struct NativeSurfaceNamespaces<'a> {
    visible: UniqueById<'a, crate::surface::SurfaceRow>,
    nonvisible: BTreeSet<u32>,
    /// The first active-datum cylinder that carries each identifier.
    active_datum: BTreeMap<u32, &'a crate::datum::DatumCylinder>,
    active_planes: BTreeMap<u32, &'a crate::datum::boundary_planes::BoundaryPlane>,
}

impl<'a> NativeSurfaceNamespaces<'a> {
    pub(super) fn new(scan: &'a ContainerScan) -> Self {
        let mut active_datum = BTreeMap::new();
        for cylinder in &scan.planes.datum_cylinders {
            active_datum.entry(cylinder.id).or_insert(cylinder);
        }
        let active_planes = scan
            .planes
            .datum_boundary_planes
            .iter()
            .filter(|plane| {
                plane.admitted(
                    &scan.planes.datum_boundary_planes,
                    &scan.surfaces.rows,
                    &scan.surfaces.nonvisible_rows,
                    &scan.planes.datum_cylinders,
                )
            })
            .map(|plane| (plane.row.id, plane))
            .collect();
        Self {
            active_planes,
            visible: UniqueById::new(&scan.surfaces.rows, |row| row.id),
            nonvisible: scan
                .surfaces
                .nonvisible_rows
                .iter()
                .map(|row| row.id)
                .collect(),
            active_datum,
        }
    }

    fn selected(&self, surface_id: u32) -> (cadmpeg_ir::ids::IdentityNamespace, &'static str) {
        select_native_surface_namespace(
            self.visible.contains(surface_id),
            self.nonvisible.contains(&surface_id),
            self.active_datum.contains_key(&surface_id)
                || self.active_planes.contains_key(&surface_id),
        )
    }

    /// Return the canonical identity prefix selected for a native identifier.
    pub(super) fn prefix(&self, surface_id: u32) -> &'static str {
        self.selected(surface_id).1
    }

    /// Construct the selected surface identity for a native topology identifier.
    pub(super) fn surface_id(
        &self,
        surface_id: u32,
    ) -> Result<SurfaceId, cadmpeg_core::CodecError> {
        crate::identity::compose_checked(&self.selected(surface_id).0, surface_id)
    }

    /// Return whether any visible geometry row carries the identifier.
    pub(super) fn is_visible(&self, surface_id: u32) -> bool {
        self.visible.contains(surface_id)
    }

    /// Return the only visible geometry row that carries the identifier.
    pub(super) fn unique_visible_row(
        &self,
        surface_id: u32,
    ) -> Option<&'a crate::surface::SurfaceRow> {
        self.visible.get(surface_id)
    }

    pub(super) fn active_plane(
        &self,
        surface_id: u32,
    ) -> Option<&'a crate::datum::boundary_planes::BoundaryPlane> {
        self.active_planes.get(&surface_id).copied()
    }

    /// Return the first active-datum cylinder that carries the identifier.
    pub(super) fn active_datum(&self, surface_id: u32) -> Option<&'a crate::datum::DatumCylinder> {
        self.active_datum.get(&surface_id).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::{select_native_surface_namespace, NativeSurfaceNamespaces};
    use crate::container::ContainerScan;
    use crate::surface::{SurfaceKind, SurfaceRow};

    fn row(id: u32, offset: usize) -> SurfaceRow {
        SurfaceRow {
            id,
            kind: SurfaceKind::Plane,
            feature_id: 1,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset,
        }
    }

    fn datum(id: u32, offset_in_payload: usize) -> crate::datum::DatumCylinder {
        crate::datum::DatumCylinder {
            id,
            feature_id: 1,
            reversed: false,
            frame: crate::surface::PositionalCylinderFrame::new(
                [0.0, 0.0, 0.0],
                [0.0, 0.0, 1.0],
                [1.0, 0.0, 0.0],
                1.0,
                None,
            )
            .expect("valid positional cylinder frame"),
            offset_in_payload,
        }
    }

    /// The per-identifier row scan the index replaces.
    fn scanned_namespace(
        scan: &ContainerScan,
        surface_id: u32,
    ) -> (cadmpeg_ir::ids::IdentityNamespace, &'static str) {
        select_native_surface_namespace(
            scan.surfaces.rows.iter().any(|row| row.id == surface_id),
            scan.surfaces
                .nonvisible_rows
                .iter()
                .any(|row| row.id == surface_id),
            scan.planes
                .datum_cylinders
                .iter()
                .any(|cylinder| cylinder.id == surface_id),
        )
    }

    #[test]
    fn native_surface_id_preserves_nonvisible_namespace() {
        let mut scan = crate::test_support::empty_container_scan();
        scan.surfaces.nonvisible_rows.push(row(17, 0));

        let native = NativeSurfaceNamespaces::new(&scan)
            .surface_id(17)
            .expect("build the native surface identity");
        assert_eq!(native.as_str(), "creo:novisgeom:surface#17");
    }

    #[test]
    fn native_surface_namespaces_match_the_row_scans() {
        let mut scan = crate::test_support::empty_container_scan();
        scan.surfaces.rows.extend(
            [(1, 10), (2, 20), (2, 21), (5, 50), (7, 70)].map(|(id, offset)| row(id, offset)),
        );
        scan.surfaces
            .nonvisible_rows
            .extend([(2, 22), (3, 30), (4, 40), (3, 31)].map(|(id, offset)| row(id, offset)));
        scan.planes
            .datum_cylinders
            .extend([(4, 41), (6, 60), (6, 61), (7, 71)].map(|(id, offset)| datum(id, offset)));
        let namespaces = NativeSurfaceNamespaces::new(&scan);
        for id in 0..=9 {
            let (namespace, prefix) = scanned_namespace(&scan, id);
            assert_eq!(namespaces.prefix(id), prefix, "{id}");
            assert_eq!(
                namespaces
                    .surface_id(id)
                    .expect("build the native surface identity"),
                crate::identity::compose_checked::<cadmpeg_ir::ids::SurfaceId>(&namespace, id)
                    .expect("build the scanned surface identity"),
                "{id}"
            );
            assert_eq!(
                namespaces.is_visible(id),
                scan.surfaces.rows.iter().any(|row| row.id == id),
                "{id}"
            );
            assert_eq!(
                namespaces.unique_visible_row(id).map(|row| row.offset),
                crate::surface::unique_surface_row(&scan.surfaces.rows, id).map(|row| row.offset),
                "{id}"
            );
            assert_eq!(
                namespaces
                    .active_datum(id)
                    .map(|datum| datum.offset_in_payload),
                scan.planes
                    .datum_cylinders
                    .iter()
                    .find(|datum| datum.id == id)
                    .map(|datum| datum.offset_in_payload),
                "{id}"
            );
        }
    }
}

pub(super) fn transfer_part_product(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
) -> Result<bool, cadmpeg_core::CodecError> {
    let Some(model_name) = scan.framing.model_name.as_ref() else {
        return Ok(false);
    };
    let model_name_offset = model_name.offset;
    let model_name = &model_name.name;
    let product_id = crate::identity::compose_checked::<ProductDefinitionId>(
        &crate::identity::MODEL_PRODUCT_DEFINITION,
        "root",
    )?;
    let occurrence_id = crate::identity::compose_checked::<OccurrenceId>(
        &crate::identity::MODEL_OCCURRENCE,
        "root",
    )?;
    annotate(
        annotations,
        &product_id,
        "archive_header",
        cadmpeg_core::decode::u64_from_index(model_name_offset),
        "part_product",
        Exactness::Derived,
    )?;
    annotate(
        annotations,
        &occurrence_id,
        "archive_header",
        cadmpeg_core::decode::u64_from_index(model_name_offset),
        "part_product_occurrence",
        Exactness::Derived,
    )?;
    ctx.charge_entities(1, "admit Creo model product_definitions")?;
    let mut bodies = Vec::new();
    for body in &ir.model.bodies {
        let body_id = body.id.as_str().to_owned();
        bodies.push(
            cadmpeg_ir::ids::BodyId::mint(body_id).map_err(cadmpeg_core::CodecError::malformed)?,
        );
    }
    let product_ref = ProductDefinitionId::mint(product_id.as_str().to_owned())
        .map_err(cadmpeg_core::CodecError::malformed)?;
    let source_name = model_name.to_owned();
    let label = model_name.to_owned();
    let part_number = model_name.to_owned();
    ir.model.product_definitions.push(ProductDefinition {
        sketches: Vec::default(),
        id: product_ref,
        kind: ProductDefinitionKind::Part,
        source_name: Some(source_name),
        label: Some(label),
        description: None,
        part_number: Some(part_number),
        bom_properties: BTreeMap::default(),
        bodies,
        native_ref: None,
    });
    ctx.charge_entities(1, "admit Creo model occurrences")?;
    let occurrence_name = model_name.to_owned();
    source_carriers.admit_occurrence(
        ir,
        Occurrence {
            id: occurrence_id,
            prototype: PrototypeReference::Local {
                definition: product_id,
            },
            parent: OccurrenceParent::Root {},
            ordinal: 0,
            transform: Transform::identity(),
            linked_prototype: None,
            scale: [cadmpeg_ir::scalar::FiniteReal::ONE; 3],
            name: Some(occurrence_name),
            visible: None,
            link: None,
            native_ref: None,
        },
    )?;
    Ok(true)
}

pub(super) fn fc05_model_frame(
    axis_index: Axis,
    axis_ordinate: f64,
    center_row_frame: [f64; 2],
    reference_row_frame: [f64; 2],
    axis_sign: Sign,
) -> ([f64; 3], [f64; 3], [f64; 3]) {
    let [first, second] = center_row_frame;
    let [reference_x, reference_z] = reference_row_frame;
    let axis_sign = axis_sign.scale();
    match axis_index {
        Axis::X => (
            [axis_ordinate, second, first],
            [axis_sign, 0.0, 0.0],
            [0.0, reference_z, reference_x],
        ),
        Axis::Y => (
            [first, axis_ordinate, second],
            [0.0, axis_sign, 0.0],
            [reference_x, 0.0, reference_z],
        ),
        Axis::Z => (
            [second, first, axis_ordinate],
            [0.0, 0.0, axis_sign],
            [reference_z, reference_x, 0.0],
        ),
    }
}

const EPS_FC05_CAP_FRAME: f64 = 1.0e-9;

#[derive(Clone, Copy)]
pub(super) struct Fc05CapPairFrame {
    /// Model-space origin of the native cylinder parameterization (`v = 0`).
    pub(super) origin: [f64; 3],
    pub(super) ref_direction: [f64; 3],
    axis_index: Axis,
    axis_sign: Sign,
}

impl Fc05CapPairFrame {
    /// The signed unit vector of the cylinder axis.
    pub(super) fn unit_vector(self) -> [f64; 3] {
        let mut axis = [0.0; 3];
        axis[self.axis_index.index()] = self.axis_sign.scale();
        axis
    }
}

/// Resolve one cap-pair cylinder in model space from its two placed cap planes.
///
/// The cap ordinates and the cap-plane origins must describe one translation
/// along the cap normal. This is the same bounded witness used by B-rep
/// transfer and is also available to analytic plane-branch selection.
pub(super) fn fc05_cap_pair_model_frame(
    scan: &ContainerScan,
    pair: &crate::curve::Fc05CylinderCapPair,
) -> Option<Fc05CapPairFrame> {
    let mut placed_caps = pair.cap_edges.iter().map(|edge| {
        crate::surface::unique_outline_plane(&scan.planes.outlines, edge.cap_plane_id)
            .map(|plane| (plane, edge.cap_ordinate_row_frame))
    });
    let (first_cap, first_ordinate) = placed_caps.next()??;
    let (mut last_cap, mut last_ordinate) = placed_caps.next()??;
    let axis_index = Axis::ALL
        .into_iter()
        .find(|axis| first_cap.normal()[axis.index()].abs() > 1.0 - EPS_FC05_CAP_FRAME)?;
    if last_cap.normal != first_cap.normal {
        return None;
    }
    for placed_cap in placed_caps {
        let (plane, ordinate) = placed_cap?;
        if plane.normal != first_cap.normal {
            return None;
        }
        last_cap = plane;
        last_ordinate = ordinate;
    }
    let row_span = last_ordinate - first_ordinate;
    let model_span = last_cap.origin[axis_index.index()] - first_cap.origin[axis_index.index()];
    let span_scale = row_span.abs().max(model_span.abs()).max(1.0);
    if !row_span.is_finite()
        || !model_span.is_finite()
        || row_span.abs() <= EPS_FC05_CAP_FRAME
        || (row_span.abs() - model_span.abs()).abs() > EPS_FC05_CAP_FRAME * span_scale
    {
        return None;
    }
    let axis_sign = if (model_span / row_span).is_sign_negative() {
        Sign::Negative
    } else {
        Sign::Positive
    };
    let axis_origin = first_cap.origin[axis_index.index()] - axis_sign.scale() * first_ordinate;
    if pair.cap_edges.iter().any(|edge| {
        let Some(plane) =
            crate::surface::unique_outline_plane(&scan.planes.outlines, edge.cap_plane_id)
        else {
            return true;
        };
        (plane.origin[axis_index.index()]
            - axis_sign.scale() * edge.cap_ordinate_row_frame
            - axis_origin)
            .abs()
            > EPS_FC05_CAP_FRAME
    }) {
        // A cap pair whose row-frame and model-space spans do not agree does
        // not establish a unit parameter-axis transform. Retain the circles
        // for their independent carrier evidence, but do not invent a chart.
        return None;
    }
    let (origin, _, ref_direction) = fc05_model_frame(
        axis_index,
        axis_origin,
        pair.center_row_frame,
        pair.reference_direction_row_frame,
        axis_sign,
    );
    Some(Fc05CapPairFrame {
        origin,
        ref_direction,
        axis_index,
        axis_sign,
    })
}

pub(super) fn transfer_fc05_cap_circles(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    source_carriers: &mut crate::decode::source_carriers::SourceUnitCarriers,
) -> Result<(), cadmpeg_core::CodecError> {
    for circle in &scan.curves.fc05_circles {
        let Some(topology) = crate::decode::uniqueness::exactly_one(
            scan.curves
                .topology_rows
                .iter()
                .filter(|row| row.id == circle.curve_id),
        ) else {
            continue;
        };
        let cap_plane = crate::decode::uniqueness::exactly_one(
            topology.bounded_face_ids().filter_map(|face| {
                crate::surface::unique_surface_row(&scan.surfaces.rows, face)
                    .filter(|row| row.kind == crate::surface::SurfaceKind::Plane)?;
                crate::surface::unique_outline_plane(&scan.planes.outlines, face)
            }),
        );
        let cylinder =
            crate::decode::uniqueness::exactly_one(topology.bounded_face_ids().filter(|face| {
                crate::surface::unique_surface_row(&scan.surfaces.rows, *face)
                    .is_some_and(|row| row.kind == crate::surface::SurfaceKind::Cylinder)
            }));
        let (Some(cap), Some(cylinder_id), Some(_)) =
            (cap_plane, cylinder, circle.cap_ordinate_row_frame)
        else {
            continue;
        };
        let Some(axis_index) = Axis::ALL
            .into_iter()
            .find(|axis| cap.normal()[axis.index()].abs() > 1.0 - EPS_FC05_CAP_FRAME)
        else {
            continue;
        };
        let [first, second] = circle.center_row_frame;
        let pair_frame = scan
            .curves
            .fc05_cylinder_cap_pairs
            .iter()
            .find(|pair| pair.surface_id == cylinder_id)
            .and_then(|pair| fc05_cap_pair_model_frame(scan, pair));
        let (reference, circle_axis_sign) = match circle.angle_parameter {
            crate::curve::Fc05AngleParameterRelation::Inconsistent => (
                circle.sample_direction_row_frame.get(),
                Sign::of_component(cap.normal()[axis_index.index()]),
            ),
            crate::curve::Fc05AngleParameterRelation::Consistent {
                sense,
                reference_direction_row_frame,
            } => (reference_direction_row_frame, Sign::from(sense).reversed()),
        };
        let axis_sign = pair_frame.map_or(circle_axis_sign, |frame| frame.axis_sign);
        let legacy_frame = fc05_model_frame(
            axis_index,
            cap.origin[axis_index.index()],
            [first, second],
            reference,
            axis_sign,
        );
        let witness = crate::decode::analytic::planes::fc05_cylinder_model_witness(
            scan,
            cylinder_id,
            crate::decode::analytic::equations::CylinderEquation {
                origin: legacy_frame.0,
                axis: legacy_frame.1,
                ref_direction: legacy_frame.2,
                radius: circle.radius_mm,
            },
        );
        let mut surface_origin = witness.origin;
        if let Some(frame) = pair_frame {
            surface_origin[axis_index.index()] = frame.origin[axis_index.index()];
        }
        let (center, axis, ref_direction) = (witness.origin, witness.axis, witness.ref_direction);
        let id = crate::identity::compose_checked::<CurveId>(
            &crate::identity::VISIBGEOM_CURVE,
            circle.curve_id,
        )?;
        if !ir.model.curves.iter().any(|curve| curve.id == id) {
            let Ok(circle_curve) = cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                Point3::from(center),
                Vector3::from(axis),
                Vector3::from(ref_direction),
                circle.radius_mm,
            ) else {
                continue;
            };
            annotate(
                annotations,
                &id,
                "VisibGeom",
                cadmpeg_core::decode::u64_from_index(circle.offset),
                "fc05_cap_circle",
                Exactness::Derived,
            )?;
            ctx.charge_entities(1, "admit Creo model curves")?;
            source_carriers.admit_curve(
                ctx,
                ir,
                Curve {
                    parameter_range: None,
                    id,
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)),
                    source_object: Some(SourceObjectAssociation {
                        geometry_role: None,
                        format: cadmpeg_ir::CodecFormat::Creo,
                        object_id: cadmpeg_core::text::NonBlankString::new(format!(
                            "VisibGeom:{}",
                            circle.curve_id
                        ))
                        .ok_or_else(|| {
                            cadmpeg_core::CodecError::malformed(
                                "source object_id must not be empty",
                            )
                        })?,
                        name: None,
                        color: None,
                        visible: None,
                        layer: None,
                        instance_path: Vec::new(),
                    }),
                },
            )?;
        }
        let surface_id = crate::identity::compose_checked::<SurfaceId>(
            &crate::identity::VISIBGEOM_SURFACE,
            cylinder_id,
        )?;
        if ir
            .model
            .surfaces
            .iter()
            .any(|surface| surface.id == surface_id)
        {
            continue;
        }
        let Ok(cylinder_surface) = cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
            Point3::from(surface_origin),
            Vector3::from(axis),
            Vector3::from(ref_direction),
            circle.radius_mm,
        ) else {
            continue;
        };
        annotate(
            annotations,
            &surface_id,
            "VisibGeom",
            cadmpeg_core::decode::u64_from_index(circle.offset),
            "fc05_axis_cylinder",
            Exactness::Derived,
        )?;
        ctx.charge_entities(1, "admit Creo model surfaces")?;
        source_carriers.admit_surface(
            ctx,
            ir,
            Surface {
                id: surface_id,
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
                    cylinder_surface,
                )),
                source_object: Some(SourceObjectAssociation {
                    geometry_role: None,
                    format: cadmpeg_ir::CodecFormat::Creo,
                    object_id: cadmpeg_core::text::NonBlankString::new(format!(
                        "VisibGeom:{cylinder_id}"
                    ))
                    .ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed("source object_id must not be empty")
                    })?,
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            },
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod fc05_tests;
