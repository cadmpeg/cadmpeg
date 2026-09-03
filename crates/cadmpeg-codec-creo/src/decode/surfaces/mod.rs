// SPDX-License-Identifier: Apache-2.0
//! Native B-rep, prototype surfaces, positional solids, and carrier intersections.

mod brep;
mod cylinders;
mod intersection_candidates;
mod intersection_resolve;
mod intersections;
mod nurbs_boundaries;
mod positional;
mod prototypes;
mod transfer_curves;

#[allow(clippy::wildcard_imports)]
pub(super) use brep::*;
#[allow(clippy::wildcard_imports)]
pub(super) use cylinders::*;
#[allow(clippy::wildcard_imports, unused_imports)]
pub(super) use intersection_candidates::*;
#[allow(clippy::wildcard_imports)]
pub(super) use intersection_resolve::*;
#[allow(clippy::wildcard_imports, unused_imports)]
pub(super) use intersections::*;
#[allow(clippy::wildcard_imports, unused_imports)]
pub(super) use nurbs_boundaries::*;
#[allow(clippy::wildcard_imports)]
pub(super) use positional::*;
#[allow(clippy::wildcard_imports)]
pub(super) use prototypes::*;
#[allow(clippy::wildcard_imports)]
pub(super) use transfer_curves::*;

use std::collections::BTreeMap;

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{Curve, CurveGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::{CurveId, OccurrenceId, ProductDefinitionId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::products::{
    Occurrence, OccurrenceParent, ProductDefinition, ProductDefinitionKind, PrototypeReference,
};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::{AnnotationBuilder, Exactness, SourceObjectAssociation};

use crate::container::ContainerScan;

use super::native::annotate;

/// Resolve the IR surface identity for a native topology surface identifier.
///
/// Visible geometry, non-visible geometry, and active-datum rows share the
/// compact native identifier space used by topology links. Keep their source
/// namespaces distinct when only one namespace owns the identifier; visible
/// geometry remains the default for the existing and rowless feature-carrier
/// paths.
pub(super) fn native_surface_id(scan: &ContainerScan, surface_id: u32) -> SurfaceId {
    let visible_present = scan.surfaces.rows.iter().any(|row| row.id == surface_id);
    let nonvisible_present = scan
        .surfaces
        .nonvisible_rows
        .iter()
        .any(|row| row.id == surface_id);
    let active_datum_present = scan
        .planes
        .datum_cylinders
        .iter()
        .any(|cylinder| cylinder.id == surface_id);
    if visible_present {
        SurfaceId(format!("creo:visibgeom:surface#{surface_id}"))
    } else if nonvisible_present {
        SurfaceId(format!("creo:novisgeom:surface#{surface_id}"))
    } else if active_datum_present {
        SurfaceId(format!("creo:actdatums:surface#{surface_id}"))
    } else {
        SurfaceId(format!("creo:visibgeom:surface#{surface_id}"))
    }
}

/// Return a native surface row only when its compact identifier is unique
/// across visible and non-visible geometry namespaces.
pub(super) fn unique_native_surface_row<'a>(
    scan: &'a ContainerScan<'_>,
    surface_id: u32,
) -> Option<&'a crate::surface::SurfaceRow> {
    let mut rows = scan
        .surfaces
        .rows
        .iter()
        .chain(&scan.surfaces.nonvisible_rows)
        .filter(|row| row.id == surface_id);
    let row = rows.next()?;
    rows.next().is_none().then_some(row)
}

#[cfg(test)]
mod tests {
    use super::native_surface_id;
    use crate::container::scan_bytes;
    use crate::surface::{SurfaceKind, SurfaceRow};

    #[test]
    fn native_surface_id_preserves_nonvisible_namespace() {
        let mut scan = scan_bytes(Vec::new());
        scan.surfaces.nonvisible_rows.push(SurfaceRow {
            id: 17,
            type_byte: SurfaceKind::Plane.canonical_type_byte(),
            kind: SurfaceKind::Plane,
            feature_id: 1,
            reversed: false,
            boundary_type: 0,
            next_surface: 0,
            offset: 0,
        });

        assert_eq!(native_surface_id(&scan, 17).0, "creo:novisgeom:surface#17");
    }
}

pub(super) fn transfer_part_product(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> bool {
    let Some(model_name) = scan.framing.model_name.as_ref() else {
        return false;
    };
    let Some(model_name_offset) = scan.framing.model_name_offset else {
        return false;
    };
    let product_id = ProductDefinitionId("creo:model:product_definition#root".to_string());
    let occurrence_id = OccurrenceId("creo:model:occurrence#root".to_string());
    annotate(
        annotations,
        &product_id,
        "archive_header",
        model_name_offset as u64,
        "part_product",
        Exactness::Derived,
    );
    annotate(
        annotations,
        &occurrence_id,
        "archive_header",
        model_name_offset as u64,
        "part_product_occurrence",
        Exactness::Derived,
    );
    ir.model.product_definitions.push(ProductDefinition {
        id: product_id.clone(),
        kind: ProductDefinitionKind::Part,
        source_name: Some(model_name.clone()),
        label: Some(model_name.clone()),
        description: None,
        part_number: Some(model_name.clone()),
        bom_properties: BTreeMap::default(),
        bodies: ir.model.bodies.iter().map(|body| body.id.clone()).collect(),
        native_ref: None,
    });
    ir.model.occurrences.push(Occurrence {
        id: occurrence_id,
        prototype: PrototypeReference::Local {
            definition: product_id,
        },
        parent: OccurrenceParent::Root,
        ordinal: 0,
        transform: Transform::identity(),
        linked_prototype: None,
        scale: [1.0; 3],
        name: Some(model_name.clone()),
        linked_subelements: Vec::new(),
        visible: None,
        element_component: None,
        claim_child: None,
        copy_on_change: None,
        copy_on_change_source: None,
        copy_on_change_group: None,
        copy_on_change_touched: None,
        native_ref: None,
    });
    true
}

pub(super) fn fc05_model_frame(
    axis_index: usize,
    axis_ordinate: f64,
    center_row_frame: [f64; 2],
    reference_row_frame: [f64; 2],
    axis_sign: f64,
) -> ([f64; 3], [f64; 3], [f64; 3]) {
    let [first, second] = center_row_frame;
    let [reference_x, reference_z] = reference_row_frame;
    match axis_index {
        0 => (
            [axis_ordinate, second, first],
            [axis_sign, 0.0, 0.0],
            [0.0, reference_z, reference_x],
        ),
        1 => (
            [first, axis_ordinate, second],
            [0.0, axis_sign, 0.0],
            [reference_x, 0.0, reference_z],
        ),
        2 => (
            [second, first, axis_ordinate],
            [0.0, 0.0, axis_sign],
            [reference_z, reference_x, 0.0],
        ),
        _ => unreachable!("model-space axis index is bounded by XYZ"),
    }
}

const EPS_FC05_CAP_FRAME: f64 = 1.0e-9;

#[derive(Clone, Copy)]
pub(super) struct Fc05CapPairFrame {
    /// Model-space origin of the native cylinder parameterization (`v = 0`).
    pub(super) origin: [f64; 3],
    pub(super) axis: [f64; 3],
    pub(super) ref_direction: [f64; 3],
    pub(super) axis_index: usize,
    pub(super) axis_sign: f64,
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
    let placed_caps = pair
        .cap_plane_ids
        .iter()
        .zip(&pair.curve_cap_ordinates_row_frame)
        .filter_map(|(id, ordinate)| {
            crate::surface::unique_outline_plane(&scan.planes.outlines, *id)
                .map(|plane| (plane, *ordinate))
        })
        .collect::<Vec<_>>();
    (placed_caps.len() == pair.cap_plane_ids.len() && placed_caps.len() >= 2).then_some(())?;
    let (first_cap, first_ordinate) = placed_caps.first().copied()?;
    let axis_index =
        (0..3).find(|axis| first_cap.normal[*axis].abs() > 1.0 - EPS_FC05_CAP_FRAME)?;
    if placed_caps
        .iter()
        .any(|(plane, _)| plane.normal != first_cap.normal)
    {
        return None;
    }
    let (last_cap, last_ordinate) = placed_caps.last().copied()?;
    let row_span = last_ordinate - first_ordinate;
    let model_span = last_cap.origin[axis_index] - first_cap.origin[axis_index];
    let span_scale = row_span.abs().max(model_span.abs()).max(1.0);
    if !row_span.is_finite()
        || !model_span.is_finite()
        || row_span.abs() <= EPS_FC05_CAP_FRAME
        || (row_span.abs() - model_span.abs()).abs() > EPS_FC05_CAP_FRAME * span_scale
    {
        return None;
    }
    let axis_sign = (model_span / row_span).signum();
    let parameter_origins = placed_caps
        .iter()
        .map(|(plane, ordinate)| plane.origin[axis_index] - axis_sign * ordinate)
        .collect::<Vec<_>>();
    if parameter_origins
        .iter()
        .any(|origin| (origin - parameter_origins[0]).abs() > EPS_FC05_CAP_FRAME)
    {
        // A cap pair whose row-frame and model-space spans do not agree does
        // not establish a unit parameter-axis transform. Retain the circles
        // for their independent carrier evidence, but do not invent a chart.
        return None;
    }
    let axis_origin = parameter_origins[0];
    let (origin, axis, ref_direction) = fc05_model_frame(
        axis_index,
        axis_origin,
        pair.center_row_frame,
        pair.reference_direction_row_frame,
        axis_sign,
    );
    Some(Fc05CapPairFrame {
        origin,
        axis,
        ref_direction,
        axis_index,
        axis_sign,
    })
}

pub(super) fn transfer_fc05_cap_circles(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) {
    for circle in &scan.curves.fc05_circles {
        let topology = scan
            .curves
            .topology_rows
            .iter()
            .filter(|row| row.id == circle.curve_id)
            .collect::<Vec<_>>();
        let [topology] = topology.as_slice() else {
            continue;
        };
        let cap_planes = topology
            .faces
            .iter()
            .filter_map(|face| {
                crate::surface::unique_surface_row(&scan.surfaces.rows, *face)
                    .filter(|row| row.kind == crate::surface::SurfaceKind::Plane)?;
                crate::surface::unique_outline_plane(&scan.planes.outlines, *face)
            })
            .collect::<Vec<_>>();
        let cylinders = topology
            .faces
            .iter()
            .filter(|face| {
                crate::surface::unique_surface_row(&scan.surfaces.rows, **face)
                    .is_some_and(|row| row.kind == crate::surface::SurfaceKind::Cylinder)
            })
            .copied()
            .collect::<Vec<_>>();
        let ([cap], [cylinder_id], Some(_)) = (
            cap_planes.as_slice(),
            cylinders.as_slice(),
            circle.cap_ordinate_row_frame,
        ) else {
            continue;
        };
        let Some(axis_index) =
            (0..3).find(|axis| cap.normal[*axis].abs() > 1.0 - EPS_FC05_CAP_FRAME)
        else {
            continue;
        };
        let [first, second] = circle.center_row_frame;
        let pair_frame = scan
            .curves
            .fc05_cylinder_cap_pairs
            .iter()
            .find(|pair| pair.surface_id == *cylinder_id)
            .and_then(|pair| fc05_cap_pair_model_frame(scan, pair));
        let reference = circle
            .reference_direction_row_frame
            .unwrap_or(circle.sample_direction_row_frame);
        let axis_sign = pair_frame.map_or_else(
            || {
                circle
                    .parameter_sign
                    .map_or_else(|| cap.normal[axis_index].signum(), |sign| -f64::from(sign))
            },
            |frame| frame.axis_sign,
        );
        let legacy_frame = fc05_model_frame(
            axis_index,
            cap.origin[axis_index],
            [first, second],
            reference,
            axis_sign,
        );
        let witness = crate::decode::analytic::fc05_cylinder_model_witness(
            scan,
            *cylinder_id,
            crate::decode::analytic::CylinderEquation {
                origin: legacy_frame.0,
                axis: legacy_frame.1,
                ref_direction: legacy_frame.2,
                radius: circle.radius_mm,
            },
        );
        let mut surface_origin = witness.origin;
        if let Some(frame) = pair_frame {
            surface_origin[axis_index] = frame.origin[axis_index];
        }
        let (center, axis, ref_direction) = (witness.origin, witness.axis, witness.ref_direction);
        let id = CurveId(format!("creo:visibgeom:curve#{}", circle.curve_id));
        if !ir.model.curves.iter().any(|curve| curve.id == id) {
            annotate(
                annotations,
                &id,
                "VisibGeom",
                circle.offset as u64,
                "fc05_cap_circle",
                Exactness::Derived,
            );
            ir.model.curves.push(Curve {
                id,
                geometry: CurveGeometry::Circle {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(axis[0], axis[1], axis[2]),
                    ref_direction: Vector3::new(
                        ref_direction[0],
                        ref_direction[1],
                        ref_direction[2],
                    ),
                    radius: circle.radius_mm,
                },
                source_object: Some(SourceObjectAssociation {
                    format: "creo".to_string(),
                    object_id: format!("VisibGeom:{}", circle.curve_id),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
        }
        let surface_id = SurfaceId(format!("creo:visibgeom:surface#{cylinder_id}"));
        if ir
            .model
            .surfaces
            .iter()
            .any(|surface| surface.id == surface_id)
        {
            continue;
        }
        annotate(
            annotations,
            &surface_id,
            "VisibGeom",
            circle.offset as u64,
            "fc05_axis_cylinder",
            Exactness::Derived,
        );
        ir.model.surfaces.push(Surface {
            id: surface_id,
            geometry: SurfaceGeometry::Cylinder {
                origin: Point3::new(surface_origin[0], surface_origin[1], surface_origin[2]),
                axis: Vector3::new(axis[0], axis[1], axis[2]),
                ref_direction: Vector3::new(ref_direction[0], ref_direction[1], ref_direction[2]),
                radius: circle.radius_mm,
            },
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("VisibGeom:{cylinder_id}"),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
    }
}
