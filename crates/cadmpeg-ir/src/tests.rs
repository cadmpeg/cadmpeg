// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
//! Unit tests for the IR: the worked cube validates clean, JSON round-trips,
//! and each validation check actually fires when its invariant is broken.

use crate::annotations::{ExactnessNote, Provenance};
use crate::diff::diff;
use crate::document::Model;
use crate::examples::unit_cube;
use crate::features::ExtrudeDirection;
use crate::geometry::{
    Curve, CurveGeometry, ProceduralCurve, ProceduralCurveDefinition, ProceduralSurface,
    ProceduralSurfaceDefinition, SplineSurfaceParameters, SurfaceGeometry,
};
use crate::ids::{
    CoedgeId, CurveId, EdgeId, ProceduralCurveId, ProceduralSurfaceId, SubdId, UnknownId,
};
use crate::math::{Point3, Vector3};
use crate::native::NativeRecord;
use crate::provenance::{Exactness, SourceObjectAssociation};
use crate::report::{LossCategory, LossCode, LossNote, Severity};
use crate::subd::{
    SubdEdge, SubdEdgeTag, SubdEdgeUse, SubdFace, SubdScheme, SubdSurface, SubdVertex,
    SubdVertexTag,
};
use crate::tessellation::TessellationChannel;
use crate::topology::Color;
use crate::unknown::{NativeUnknownRecord, UnknownRecord};
use crate::validate::validate;
use crate::validate::Check;
use crate::CadIr;
use serde::{de::DeserializeOwned, Serialize};
use std::fmt::Debug;

fn assert_base64_round_trip_and_rejection<T>(value: &T, field: &str)
where
    T: Serialize + DeserializeOwned + PartialEq + Debug,
{
    let mut json = serde_json::to_value(value).unwrap();
    assert_eq!(json[field], "AQID");
    assert_eq!(serde_json::from_value::<T>(json.clone()).unwrap(), *value);
    json[field] = serde_json::Value::String("%%%".into());
    assert!(serde_json::from_value::<T>(json).is_err());
}

#[test]
fn product_occurrence_tree_validates_references_and_cycles() {
    use crate::ids::{OccurrenceId, ProductId};
    use crate::product::{OccurrenceParent, Product, ProductOccurrence};
    use crate::transform::Transform;
    use crate::units::Units;

    let mut ir = CadIr::empty(Units::default());
    ir.model.products.push(Product {
        id: ProductId("test:product:product#assembly".into()),
        product_id: "assembly".into(),
        name: Some("Assembly".into()),
        bodies: Vec::new(),
    });
    ir.model.product_occurrences.push(ProductOccurrence {
        id: OccurrenceId("test:product:occurrence#root".into()),
        product: ProductId("test:product:product#assembly".into()),
        parent: OccurrenceParent::Root,
        transform: Transform::identity(),
        name: None,
    });
    ir.model.product_occurrences.push(ProductOccurrence {
        id: OccurrenceId("test:product:occurrence#child".into()),
        product: ProductId("test:product:product#assembly".into()),
        parent: OccurrenceParent::Occurrence {
            occurrence: OccurrenceId("test:product:occurrence#root".into()),
        },
        transform: Transform::identity(),
        name: None,
    });
    ir.finalize();
    assert!(crate::validate(&ir, Vec::new()).is_ok());

    ir.model.product_occurrences[1].transform.rows[0][0] = f64::INFINITY;
    assert!(crate::validate(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| {
            finding.check == crate::validate::Check::ProductStructure
                && finding.message.contains("non-finite")
        }));
    ir.model.product_occurrences[1].transform = Transform::identity();

    ir.model.product_occurrences[1].parent = OccurrenceParent::Occurrence {
        occurrence: OccurrenceId("test:product:occurrence#child".into()),
    };
    let report = crate::validate(&ir, Vec::new());
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.check == crate::validate::Check::ProductStructure));
}

#[test]
fn non_finite_body_transform_is_invalid() {
    let mut ir = unit_cube();
    let mut transform = crate::transform::Transform::identity();
    transform.rows[2][3] = f64::NAN;
    ir.model.bodies[0].transform = Some(transform);
    assert!(validate(&ir, Vec::new()).findings.iter().any(|finding| {
        finding.check == Check::Bounds && finding.message.contains("non-finite")
    }));
}

/// Replace the surface of the cube's first face with an unknown surface,
/// optionally linking a preserved record, and return the face id and its
/// surface id. Leaves every loop/coedge/edge of the face intact.
fn make_first_face_surface_unknown(ir: &mut crate::CadIr, record: Option<UnknownId>) -> String {
    let face = &ir.model.faces[0];
    let surface_id = face.surface.0.clone();
    for s in &mut ir.model.surfaces {
        if s.id.0 == surface_id {
            s.geometry = SurfaceGeometry::Unknown { record };
            break;
        }
    }
    surface_id
}

#[test]
fn face_on_unknown_surface_validates_clean() {
    let mut ir = unit_cube();
    // Preserve a raw record and point the unknown surface at it.
    let rec = UnknownId("synthetic:cube:unknown#0".into());
    ir.push_native_unknown(
        "synthetic",
        NativeUnknownRecord {
            id: rec.clone(),
            links: Vec::new(),
        },
    )
    .unwrap();
    make_first_face_surface_unknown(&mut ir, Some(rec));

    let report = validate(&ir, Vec::new());
    assert!(
        report.is_ok(),
        "a face on an unknown surface is legal, got: {:?}",
        report.findings
    );
    // The face and its topology stay in the graph.
    assert_eq!(ir.model.faces.len(), 6);
    // The situation is surfaced as a count.
    assert_eq!(
        report.entity_counts.get("surfaces_unknown_geometry"),
        Some(&1)
    );
}

#[test]
fn unknown_surface_without_record_is_legal() {
    let mut ir = unit_cube();
    make_first_face_surface_unknown(&mut ir, None);
    let report = validate(&ir, Vec::new());
    assert!(
        report.is_ok(),
        "an unknown surface need not preserve bytes, got: {:?}",
        report.findings
    );
    assert_eq!(
        report.entity_counts.get("surfaces_unknown_geometry"),
        Some(&1)
    );
}

#[test]
fn procedural_surface_carrier_requires_its_exact_owner() {
    let mut ir = unit_cube();
    let surface = ir.model.surfaces[0].id.clone();
    let construction = ProceduralSurfaceId("synthetic:cube:procedural-surface#0".into());
    ir.model.surfaces[0].geometry = SurfaceGeometry::Procedural {
        construction: construction.clone(),
    };
    ir.model.procedural_surfaces.push(ProceduralSurface {
        id: construction.clone(),
        surface: surface.clone(),
        definition: ProceduralSurfaceDefinition::Exact {
            parameters: SplineSurfaceParameters::OrderedRanges {
                ranges: [[0.0, 1.0], [0.0, 1.0]],
            },
            extension: 0,
            revision_form: None,
        },
        cache_fit_tolerance: None,
        record_bounds: None,
    });
    let report = validate(&ir, Vec::new());
    assert!(report.is_ok(), "{:?}", report.findings);

    ir.model.procedural_surfaces[0].cache_fit_tolerance = Some(0.01);
    assert!(validate(&ir, Vec::new()).findings.iter().any(|finding| {
        finding
            .message
            .contains("construction-backed surface cannot carry a cache-fit tolerance")
    }));
    ir.model.procedural_surfaces[0].cache_fit_tolerance = None;

    ir.model.surfaces[0].geometry = SurfaceGeometry::Procedural {
        construction: ProceduralSurfaceId("synthetic:missing".into()),
    };
    assert!(validate(&ir, Vec::new()).findings.iter().any(|finding| {
        finding
            .message
            .contains("references missing procedural surface construction")
    }));

    ir.model.surfaces[0].geometry = SurfaceGeometry::Procedural { construction };
    ir.model.procedural_surfaces[0].surface = ir.model.surfaces[1].id.clone();
    assert!(validate(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message.contains("does not produce surface")));
}

#[test]
fn procedural_curve_carrier_requires_its_exact_owner() {
    let mut ir = unit_cube();
    let curve = ir.model.curves[0].id.clone();
    let construction = ProceduralCurveId("synthetic:cube:procedural-curve#0".into());
    ir.model.curves[0].geometry = CurveGeometry::Procedural {
        construction: construction.clone(),
    };
    ir.model.procedural_curves.push(ProceduralCurve {
        id: construction.clone(),
        curve: curve.clone(),
        definition: ProceduralCurveDefinition::Helix {
            angle_range: [0.0, std::f64::consts::TAU],
            center: Point3::new(0.0, 0.0, 0.0),
            major: Vector3::new(1.0, 0.0, 0.0),
            minor: Vector3::new(0.0, 1.0, 0.0),
            pitch: Vector3::new(0.0, 0.0, 1.0),
            apex_factor: 0.0,
            axis: Vector3::new(0.0, 0.0, 1.0),
        },
        cache_fit_tolerance: None,
    });
    let report = validate(&ir, Vec::new());
    assert!(report.is_ok(), "{:?}", report.findings);

    ir.model.procedural_curves[0].cache_fit_tolerance = Some(0.01);
    assert!(validate(&ir, Vec::new()).findings.iter().any(|finding| {
        finding
            .message
            .contains("construction-backed curve cannot carry a cache-fit tolerance")
    }));
    ir.model.procedural_curves[0].cache_fit_tolerance = None;

    ir.model.curves[0].geometry = CurveGeometry::Procedural {
        construction: ProceduralCurveId("synthetic:missing".into()),
    };
    assert!(validate(&ir, Vec::new()).findings.iter().any(|finding| {
        finding
            .message
            .contains("references missing procedural curve construction")
    }));

    ir.model.curves[0].geometry = CurveGeometry::Procedural { construction };
    ir.model.procedural_curves[0].curve = ir.model.curves[1].id.clone();
    assert!(validate(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message.contains("does not produce curve")));
}

#[test]
fn unknown_surface_dangling_record_is_flagged() {
    let mut ir = unit_cube();
    // Link a record id that is not in the unknowns arena.
    make_first_face_surface_unknown(&mut ir, Some(UnknownId("missing".into())));
    let report = validate(&ir, Vec::new());
    assert!(report.findings.iter().any(|finding| {
        finding.check == Check::ReferentialIntegrity
            && finding.message.contains("missing unknown record `missing`")
    }));
}

#[test]
fn unknown_surface_json_round_trips() {
    let mut ir = unit_cube();
    let rec = UnknownId("synthetic:cube:unknown#0".into());
    ir.push_native_unknown(
        "synthetic",
        NativeUnknownRecord {
            id: rec.clone(),
            links: Vec::new(),
        },
    )
    .unwrap();
    make_first_face_surface_unknown(&mut ir, Some(rec));

    let json = ir.to_canonical_json().unwrap();
    let parsed = crate::CadIr::from_json(&json).unwrap();
    assert_eq!(parsed, ir, "round-trip must preserve the unknown surface");
}

#[test]
fn unit_cube_has_expected_census() {
    let ir = unit_cube();
    assert_eq!(ir.model.bodies.len(), 1);
    assert_eq!(ir.model.regions.len(), 1);
    assert_eq!(ir.model.shells.len(), 1);
    assert_eq!(ir.model.faces.len(), 6);
    assert_eq!(ir.model.loops.len(), 6);
    assert_eq!(ir.model.coedges.len(), 24);
    assert_eq!(ir.model.edges.len(), 12);
    assert_eq!(ir.model.vertices.len(), 8);
    assert_eq!(ir.model.points.len(), 8);
    assert_eq!(ir.model.surfaces.len(), 6);
    assert_eq!(ir.model.curves.len(), 12);
}

#[test]
fn unit_cube_validates_clean() {
    let ir = unit_cube();
    let report = validate(&ir, Vec::new());
    assert!(
        report.is_ok(),
        "cube should have no error findings, got: {:?}",
        report.findings
    );
    assert_eq!(report.error_count(), 0);
    assert_eq!(report.warning_count(), 0);
    assert_eq!(report.entity_counts.get("coedges"), Some(&24));
}

#[test]
fn arena_registry_drives_counts_and_diff_dispatch() {
    let ir = unit_cube();
    let report = validate(&ir, Vec::new());
    let diff_kinds = diff(&ir, &ir)
        .per_arena
        .into_iter()
        .map(|arena| arena.kind)
        .collect::<Vec<_>>();

    assert_eq!(
        &diff_kinds[..Model::arena_names().len()],
        Model::arena_names()
    );
    for name in Model::arena_names() {
        assert!(
            report.entity_counts.contains_key(*name),
            "entity counts omitted registered arena {name}"
        );
    }
}

#[test]
fn current_json_without_configurations_defaults_to_empty() {
    let ir = unit_cube();
    let mut value = serde_json::to_value(&ir).unwrap();
    value
        .get_mut("model")
        .and_then(serde_json::Value::as_object_mut)
        .unwrap()
        .remove("configurations");

    let decoded: CadIr = serde_json::from_value(value).unwrap();
    assert!(decoded.model.configurations.is_empty());
}

#[test]
fn current_json_without_parameters_defaults_to_empty() {
    let ir = unit_cube();
    let mut value = serde_json::to_value(&ir).unwrap();
    value
        .get_mut("model")
        .and_then(serde_json::Value::as_object_mut)
        .unwrap()
        .remove("parameters");

    let decoded: CadIr = serde_json::from_value(value).unwrap();
    assert!(decoded.model.parameters.is_empty());
}

#[test]
fn current_json_without_sketch_arenas_defaults_to_empty() {
    let ir = unit_cube();
    let mut value = serde_json::to_value(&ir).unwrap();
    let model = value
        .get_mut("model")
        .and_then(serde_json::Value::as_object_mut)
        .unwrap();
    model.remove("sketches");
    model.remove("sketch_entities");
    model.remove("sketch_constraints");

    let decoded: CadIr = serde_json::from_value(value).unwrap();
    assert!(decoded.model.sketches.is_empty());
    assert!(decoded.model.sketch_entities.is_empty());
    assert!(decoded.model.sketch_constraints.is_empty());
}

#[test]
fn malformed_sketch_geometry_and_constraints_are_rejected() {
    use crate::features::Length;
    use crate::math::{Point2, Point3, Vector3};
    use crate::sketches::{
        Sketch, SketchConstraint, SketchConstraintDefinition, SketchConstraintId, SketchEntity,
        SketchEntityId, SketchEntityUse, SketchGeometry, SketchId,
    };

    let mut ir = unit_cube();
    let sketch_id = SketchId("synthetic:test:sketch#0".into());
    let circle_id = SketchEntityId("synthetic:test:sketch-entity#0".into());
    let nurbs_id = SketchEntityId("synthetic:test:sketch-entity#1".into());
    ir.model.sketches.push(Sketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        placement: crate::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 1.0),
        },
        profiles: vec![vec![SketchEntityUse {
            entity: circle_id.clone(),
            reversed: false,
        }]],
        native_ref: None,
    });
    ir.model.sketch_entities.extend([
        SketchEntity {
            id: circle_id.clone(),
            sketch: sketch_id.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Circle {
                center: Point2::new(0.0, 0.0),
                radius: Length(-1.0),
            },
        },
        SketchEntity {
            id: nurbs_id,
            sketch: sketch_id.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Nurbs {
                degree: 3,
                knots: vec![0.0, 1.0],
                control_points: vec![Point2::new(0.0, 0.0)],
                weights: Some(vec![0.0]),
                periodic: false,
            },
        },
    ]);
    ir.model.sketch_constraints.push(SketchConstraint {
        id: SketchConstraintId("synthetic:test:sketch-constraint#0".into()),
        sketch: sketch_id,
        definition: SketchConstraintDefinition::Coincident {
            entities: vec![circle_id],
        },
        name: None,
        driving: None,
        active: None,
        virtual_space: None,
        visible: None,
        orientation: None,
        label_distance: None,
        label_position: None,
        metadata: None,
        native_ref: None,
    });
    ir.finalize();

    let report = validate(&ir, Vec::new());
    assert!(report.findings.iter().any(|finding| {
        finding.check == Check::GeometricConsistency
            && finding.entity.as_deref() == Some("synthetic:test:sketch#0")
    }));
    assert!(report.findings.iter().any(|finding| {
        finding.check == Check::Bounds
            && finding.entity.as_deref() == Some("synthetic:test:sketch-entity#0")
    }));
    assert!(report.findings.iter().any(|finding| {
        finding.check == Check::ParameterDomain
            && finding.entity.as_deref() == Some("synthetic:test:sketch-entity#1")
    }));
    assert!(report.findings.iter().any(|finding| {
        finding.check == Check::Counts
            && finding.entity.as_deref() == Some("synthetic:test:sketch-constraint#0")
    }));
}

#[test]
fn polygon_constraints_round_trip_and_require_distinct_members() {
    use crate::math::{Point2, Point3, Vector3};
    use crate::sketches::{
        Sketch, SketchConstraint, SketchConstraintDefinition, SketchConstraintId, SketchEntity,
        SketchEntityId, SketchGeometry, SketchId,
    };

    let mut ir = unit_cube();
    let sketch = SketchId("synthetic:test:sketch#polygon".into());
    ir.model.sketches.push(Sketch {
        id: sketch.clone(),
        name: None,
        configuration: None,
        placement: crate::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: Vec::new(),
        native_ref: None,
    });
    let members = (0..3)
        .map(|ordinal| SketchEntityId(format!("synthetic:test:polygon-point#{ordinal}")))
        .collect::<Vec<_>>();
    ir.model.sketch_entities.extend(
        members
            .iter()
            .enumerate()
            .map(|(ordinal, id)| SketchEntity {
                id: id.clone(),
                sketch: sketch.clone(),
                construction: false,
                native_ref: None,
                geometry_ref: None,
                endpoint_refs: Vec::new(),
                geometry: SketchGeometry::Point {
                    position: Point2::new(ordinal as f64, 0.0),
                },
            }),
    );
    let constraint = SketchConstraintId("synthetic:test:polygon-constraint#0".into());
    ir.model.sketch_constraints.push(SketchConstraint {
        id: constraint.clone(),
        sketch,
        definition: SketchConstraintDefinition::Polygon {
            entities: members.clone(),
        },
        name: None,
        driving: None,
        active: None,
        virtual_space: None,
        visible: None,
        orientation: None,
        label_distance: None,
        label_position: None,
        metadata: None,
        native_ref: None,
    });
    ir.finalize();
    assert!(validate(&ir, Vec::new()).is_ok());
    let round_trip = CadIr::from_json(&serde_json::to_string(&ir).unwrap()).unwrap();
    assert_eq!(
        round_trip.model.sketch_constraints,
        ir.model.sketch_constraints
    );

    ir.model.sketch_constraints[0].definition = SketchConstraintDefinition::Polygon {
        entities: vec![members[0].clone(), members[1].clone(), members[0].clone()],
    };
    let report = validate(&ir, Vec::new());
    assert!(report.findings.iter().any(|finding| {
        finding.entity.as_deref() == Some(constraint.0.as_str())
            && finding.message.contains("three distinct members")
    }));
}

#[test]
fn locus_aware_sketch_constraints_round_trip_and_validate_geometry() {
    use crate::features::{Length, ParameterId};
    use crate::math::{Point2, Point3, Vector3};
    use crate::sketches::{
        Sketch, SketchConstraint, SketchConstraintDefinition, SketchConstraintId,
        SketchDistanceMeasurement, SketchEntity, SketchEntityId, SketchGeometry, SketchId,
        SketchLocus, SketchOffsetPair,
    };

    let entity = SketchEntityId("synthetic:test:entity#0".into());
    let parameter = ParameterId("synthetic:test:parameter#0".into());
    let definitions = vec![
        SketchConstraintDefinition::Disabled,
        SketchConstraintDefinition::CoincidentLoci {
            loci: vec![
                SketchLocus::Start(entity.clone()),
                SketchLocus::Center(entity.clone()),
            ],
        },
        SketchConstraintDefinition::PointOnObject {
            point: SketchLocus::Start(entity.clone()),
            entity: entity.clone(),
        },
        SketchConstraintDefinition::Midpoint {
            point: SketchLocus::End(entity.clone()),
            entity: entity.clone(),
        },
        SketchConstraintDefinition::Offset {
            pairs: vec![SketchOffsetPair {
                source: entity.clone(),
                result: entity.clone(),
                source_reversed: false,
            }],
            distance: Length(2.0),
            parameter: Some(parameter.clone()),
            parameter_factor: Some(-1.0),
        },
        SketchConstraintDefinition::Concentric {
            first: entity.clone(),
            second: entity.clone(),
        },
        SketchConstraintDefinition::Curvature {
            first: entity.clone(),
            second: entity.clone(),
        },
        SketchConstraintDefinition::Collinear {
            first: entity.clone(),
            second: entity.clone(),
        },
        SketchConstraintDefinition::Symmetric {
            first: SketchLocus::Start(entity.clone()),
            second: SketchLocus::End(entity.clone()),
            axis: entity.clone(),
        },
        SketchConstraintDefinition::Radius {
            entity: entity.clone(),
            parameter: parameter.clone(),
        },
        SketchConstraintDefinition::Diameter {
            entity: entity.clone(),
            parameter: parameter.clone(),
        },
        SketchConstraintDefinition::DistanceLoci {
            first: SketchLocus::Start(entity.clone()),
            second: SketchLocus::End(entity.clone()),
            parameter: parameter.clone(),
        },
        SketchConstraintDefinition::HorizontalDistance {
            first: SketchLocus::Start(entity.clone()),
            second: SketchLocus::End(entity.clone()),
            parameter: parameter.clone(),
        },
        SketchConstraintDefinition::HorizontalLoci {
            first: SketchLocus::Start(entity.clone()),
            second: SketchLocus::End(entity.clone()),
        },
        SketchConstraintDefinition::VerticalDistance {
            first: SketchLocus::Start(entity.clone()),
            second: SketchLocus::End(entity.clone()),
            parameter: parameter.clone(),
        },
        SketchConstraintDefinition::VerticalLoci {
            first: SketchLocus::Start(entity.clone()),
            second: SketchLocus::End(entity.clone()),
        },
        SketchConstraintDefinition::RepeatedDistance {
            measurements: vec![SketchDistanceMeasurement::Horizontal {
                first: SketchLocus::Start(entity.clone()),
                second: SketchLocus::End(entity.clone()),
            }],
            parameter,
        },
        SketchConstraintDefinition::SnellsLaw {
            incident: SketchLocus::Start(entity.clone()),
            refracted: SketchLocus::End(entity.clone()),
            interface: entity.clone(),
            parameter: ParameterId("synthetic:test:parameter#0".into()),
        },
        SketchConstraintDefinition::Weight {
            entity: entity.clone(),
            parameter: ParameterId("synthetic:test:parameter#0".into()),
        },
        SketchConstraintDefinition::InternalAlignment {
            helper: entity.clone(),
            parent: entity.clone(),
            alignment: crate::sketches::SketchInternalAlignment::BsplineControlPoint,
            index: Some(2),
        },
        SketchConstraintDefinition::Group {
            elements: vec![SketchLocus::Entity(entity.clone())],
        },
        SketchConstraintDefinition::Text {
            elements: vec![SketchLocus::Entity(entity.clone())],
            text: "R42".into(),
            font: Some("Mono".into()),
            is_text_height: false,
        },
    ];
    let json = serde_json::to_string(&definitions).unwrap();
    assert_eq!(
        serde_json::from_str::<Vec<SketchConstraintDefinition>>(&json).unwrap(),
        definitions
    );

    let mut ir = unit_cube();
    let sketch = SketchId("synthetic:test:sketch#locus".into());
    ir.model.sketches.push(Sketch {
        id: sketch.clone(),
        name: None,
        configuration: None,
        placement: crate::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: Vec::new(),
        native_ref: None,
    });
    ir.model.sketch_entities.push(SketchEntity {
        id: entity.clone(),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(1.0, 0.0),
        },
    });
    let constraint_id = SketchConstraintId("synthetic:test:constraint#locus".into());
    ir.model.sketch_constraints.push(SketchConstraint {
        id: constraint_id.clone(),
        sketch,
        definition: SketchConstraintDefinition::CoincidentLoci {
            loci: vec![
                SketchLocus::Center(entity.clone()),
                SketchLocus::Start(entity),
            ],
        },
        name: None,
        driving: None,
        active: None,
        virtual_space: None,
        visible: None,
        orientation: None,
        label_distance: None,
        label_position: None,
        metadata: None,
        native_ref: None,
    });
    ir.finalize();
    let report = validate(&ir, Vec::new());
    assert!(report.findings.iter().any(|finding| {
        finding.entity.as_deref() == Some(constraint_id.0.as_str())
            && finding.check == Check::GeometricConsistency
    }));
}

#[test]
fn sketch_profiles_and_constraints_enforce_local_connectivity() {
    use crate::math::{Point2, Point3, Vector3};
    use crate::sketches::{
        Sketch, SketchConstraint, SketchConstraintDefinition, SketchConstraintId, SketchEntity,
        SketchEntityId, SketchEntityUse, SketchGeometry, SketchId,
    };

    let mut ir = unit_cube();
    let first_sketch = SketchId("synthetic:test:sketch#first".into());
    let second_sketch = SketchId("synthetic:test:sketch#second".into());
    let first = SketchEntityId("synthetic:test:entity#first".into());
    let disconnected = SketchEntityId("synthetic:test:entity#disconnected".into());
    let foreign = SketchEntityId("synthetic:test:entity#foreign".into());
    let plane = |id: SketchId, profiles| Sketch {
        id,
        name: None,
        configuration: None,
        placement: crate::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles,
        native_ref: None,
    };
    ir.model.sketches.extend([
        plane(
            first_sketch.clone(),
            vec![vec![
                SketchEntityUse {
                    entity: first.clone(),
                    reversed: false,
                },
                SketchEntityUse {
                    entity: disconnected.clone(),
                    reversed: false,
                },
            ]],
        ),
        plane(second_sketch.clone(), Vec::new()),
    ]);
    let line = |id, sketch, start, end| SketchEntity {
        id,
        sketch,
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Line { start, end },
    };
    ir.model.sketch_entities.extend([
        line(
            first.clone(),
            first_sketch.clone(),
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
        ),
        line(
            disconnected,
            first_sketch.clone(),
            Point2::new(2.0, 0.0),
            Point2::new(3.0, 0.0),
        ),
        line(
            foreign.clone(),
            second_sketch,
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
        ),
    ]);
    let constraint = SketchConstraintId("synthetic:test:constraint#foreign".into());
    ir.model.sketch_constraints.push(SketchConstraint {
        id: constraint.clone(),
        sketch: first_sketch.clone(),
        definition: SketchConstraintDefinition::Parallel {
            first,
            second: foreign,
        },
        name: None,
        driving: None,
        active: None,
        virtual_space: None,
        visible: None,
        orientation: None,
        label_distance: None,
        label_position: None,
        metadata: None,
        native_ref: None,
    });
    ir.finalize();
    let report = validate(&ir, Vec::new());
    assert!(report.findings.iter().any(|finding| {
        finding.entity.as_deref() == Some(first_sketch.0.as_str())
            && finding.message.contains("disconnected consecutive")
    }));
    assert!(report.findings.iter().any(|finding| {
        finding.entity.as_deref() == Some(constraint.0.as_str())
            && finding.message.contains("different sketch")
    }));
}

#[test]
fn neutral_features_resolve_sketch_profile_and_path_operands() {
    use crate::features::{
        BooleanOp, ExtrudeExtent, ExtrudeSide, Feature, FeatureDefinition, FeatureId, Length,
        PathRef, ProfileRef, Termination,
    };
    use crate::sketches::SketchId;

    let sketch = SketchId("synthetic:test:sketch#missing".into());
    let definitions = [
        FeatureDefinition::Extrude {
            profile: ProfileRef::Sketch(sketch.clone()),
            direction: ExtrudeDirection::ProfileNormal,
            start: crate::features::ExtrudeStart::ProfilePlane,
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(10.0),
                    },
                    draft: None,
                    offset: None,
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
        FeatureDefinition::Sweep {
            profile: Some(ProfileRef::Sketch(sketch.clone())),
            sections: Vec::new(),
            path: Some(PathRef::Sketch(sketch.clone())),
            mode: crate::features::SweepMode::Solid {
                op: BooleanOp::NewBody,
            },
            orientation: None,
            transition: None,
            transformation: None,
            path_tangent: false,
            linearize: false,
            twist: None,
            scale: None,
            allow_multi_profile_faces: None,
        },
    ];
    let json = serde_json::to_string(&definitions).unwrap();
    assert_eq!(
        serde_json::from_str::<[FeatureDefinition; 2]>(&json).unwrap(),
        definitions
    );

    let mut ir = unit_cube();
    ir.model.features.push(Feature {
        id: FeatureId("synthetic:test:feature#sketch-ref".into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: definitions[1].clone(),
        native_ref: None,
    });
    ir.finalize();
    let report = validate(&ir, Vec::new());
    assert_eq!(
        report
            .findings
            .iter()
            .filter(|finding| finding.message.contains("missing sketch"))
            .count(),
        2
    );
}

#[test]
fn feature_history_rejects_dangling_and_forward_dependencies() {
    use crate::features::{
        BooleanOp, ExtrudeExtent, ExtrudeSide, FaceSelection, Feature, FeatureDefinition,
        FeatureId, FeatureSourceContent, ParameterId, ProfileRef, Termination,
    };
    use crate::ids::{BodyId, FaceId};
    use std::collections::BTreeMap;

    let mut ir = unit_cube();
    let feature_id = FeatureId("synthetic:test:feature#invalid".into());
    ir.model.features.push(Feature {
        id: feature_id.clone(),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: Some(feature_id.clone()),
        dependencies: vec![feature_id.clone(), feature_id.clone()],
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: vec![
            FeatureSourceContent::Parameter(ParameterId("synthetic:test:parameter#missing".into())),
            FeatureSourceContent::Feature(feature_id.clone()),
        ],
        outputs: vec![BodyId("synthetic:test:body#missing".into())],
        definition: FeatureDefinition::Extrude {
            profile: ProfileRef::Faces(vec![FaceId("synthetic:test:face#profile-missing".into())]),
            direction: ExtrudeDirection::ProfileNormal,
            start: crate::features::ExtrudeStart::ProfilePlane,
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::ToFace {
                        face: FaceSelection::Faces(vec![FaceId(
                            "synthetic:test:face#termination-missing".into(),
                        )]),
                        offset: None,
                    },
                    draft: None,
                    offset: None,
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
        native_ref: None,
    });
    ir.model.features.push(Feature {
        id: FeatureId("synthetic:test:feature#duplicate-order".into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Native {
            kind: "Marker".into(),
            parameters: BTreeMap::new(),
            properties: BTreeMap::new(),
        },
        native_ref: None,
    });
    ir.finalize();
    let report = validate(&ir, Vec::new());
    for fragment in [
        "does not precede",
        "missing output body",
        "missing profile face",
        "missing termination face",
        "repeats feature ordinal",
        "repeats dependency",
        "missing content parameter",
        "content child",
    ] {
        assert!(
            report.findings.iter().any(|finding| {
                finding.entity.as_deref() == Some(feature_id.0.as_str())
                    && finding.message.contains(fragment)
            }),
            "missing finding containing {fragment:?}"
        );
    }
}

#[test]
fn feature_parameters_require_unique_names_and_ordinals() {
    use crate::features::{DesignParameter, Feature, FeatureDefinition, FeatureId, ParameterId};
    use std::collections::BTreeMap;

    let mut ir = unit_cube();
    let owner = FeatureId("synthetic:test:feature#parameters".into());
    ir.model.features.push(Feature {
        id: owner.clone(),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Native {
            kind: "Test".into(),
            parameters: BTreeMap::new(),
            properties: BTreeMap::new(),
        },
        native_ref: None,
    });
    for (index, name) in ["Width", "Width"].into_iter().enumerate() {
        ir.model.parameters.push(DesignParameter {
            id: ParameterId(format!("synthetic:test:parameter#{index}")),
            owner: Some(owner.clone()),
            ordinal: 0,
            name: name.into(),
            expression: "1mm".into(),
            display: None,
            value: None,
            dependencies: Vec::new(),
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: None,
        });
    }
    ir.finalize();
    let report = validate(&ir, Vec::new());
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.message.contains("repeats parameter name")));
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.message.contains("repeats parameter ordinal")));
}

#[test]
fn parameter_dependencies_must_exist_and_precede_consumers() {
    use crate::features::{DesignParameter, Feature, FeatureDefinition, FeatureId, ParameterId};
    use std::collections::BTreeMap;

    let mut ir = unit_cube();
    let owner = FeatureId("synthetic:test:feature#dependency-owner".into());
    ir.model.features.push(Feature {
        id: owner.clone(),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Native {
            kind: "Test".into(),
            parameters: BTreeMap::new(),
            properties: BTreeMap::new(),
        },
        native_ref: None,
    });
    let first = ParameterId("synthetic:test:parameter#first".into());
    let second = ParameterId("synthetic:test:parameter#second".into());
    for (id, ordinal, dependencies) in [
        (first.clone(), 0, vec![second.clone()]),
        (
            second,
            1,
            vec![ParameterId("synthetic:test:parameter#missing".into())],
        ),
    ] {
        ir.model.parameters.push(DesignParameter {
            id,
            owner: Some(owner.clone()),
            ordinal,
            name: format!("P{ordinal}"),
            expression: String::new(),
            display: None,
            value: None,
            dependencies,
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: None,
        });
    }
    let findings = validate(&ir, Vec::new()).findings;
    assert!(findings
        .iter()
        .any(|finding| finding.message.contains("does not precede its consumer")));
    assert!(findings.iter().any(|finding| {
        finding
            .message
            .contains("parameter dependency `synthetic:test:parameter#missing`")
    }));
}

#[test]
fn document_parameters_can_feed_feature_parameters() {
    use crate::features::{DesignParameter, Feature, FeatureDefinition, FeatureId, ParameterId};
    use std::collections::BTreeMap;

    let mut ir = unit_cube();
    let owner = FeatureId("synthetic:test:feature#consumer".into());
    ir.model.features.push(Feature {
        id: owner.clone(),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Native {
            kind: "Test".into(),
            parameters: BTreeMap::new(),
            properties: BTreeMap::new(),
        },
        native_ref: None,
    });
    let document = ParameterId("synthetic:test:parameter#document".into());
    ir.model.parameters.push(DesignParameter {
        id: document.clone(),
        owner: None,
        ordinal: 0,
        name: "Width".into(),
        expression: "60 mm".into(),
        display: None,
        value: None,
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
    ir.model.parameters.push(DesignParameter {
        id: ParameterId("synthetic:test:parameter#owned".into()),
        owner: Some(owner),
        ordinal: 0,
        name: "Distance".into(),
        expression: "Width / 2".into(),
        display: None,
        value: None,
        dependencies: vec![document],
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
    ir.finalize();
    assert!(validate(&ir, Vec::new()).findings.is_empty());
}

#[test]
fn tessellation_counts_must_be_consistent() {
    use crate::ids::FaceId;
    use crate::math::{Point3, Vector3};
    use crate::tessellation::Tessellation;

    let mut ir = unit_cube();
    ir.model.tessellations.push(Tessellation {
        id: "synthetic:test:tessellation#invalid-counts".into(),
        body: None,
        faces: vec![FaceId("synthetic:test:face#missing".into())],
        chordal_deflection: Some(-1.0),
        source_object: None,
        vertices: vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
        ],
        triangles: vec![[0, 1, 2]],
        strip_lengths: vec![4],
        normals: vec![Vector3::new(0.0, 0.0, 1.0); 2],
        channels: Vec::new(),
    });
    ir.model.tessellations.push(Tessellation {
        id: "synthetic:test:tessellation#invalid-strips".into(),
        body: None,
        faces: Vec::new(),
        chordal_deflection: None,
        source_object: None,
        vertices: vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
        ],
        triangles: vec![[0, 2, 1]],
        strip_lengths: vec![3],
        normals: vec![Vector3::new(0.0, 0.0, 1.0); 3],
        channels: Vec::new(),
    });
    ir.finalize();
    let report = validate(&ir, Vec::new());
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.message.contains("normals do not match")));
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.message.contains("strips do not match")));
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.message.contains("triangles do not match strips")));
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.message.contains("missing tessellation face")));
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.message.contains("invalid tessellation deflection")));
}

#[test]
fn configuration_body_membership_round_trips_and_validates() {
    use crate::features::{
        ConfigurationId, DesignConfiguration, DesignParameter, FeatureId, ParameterId,
    };
    use crate::ids::BodyId;
    use std::collections::BTreeMap;

    let mut ir = unit_cube();
    let configuration_id = ConfigurationId("synthetic:test:configuration#0".into());
    let parameter_id = ParameterId("synthetic:test:parameter#width".into());
    let body = ir.model.bodies[0].id.clone();
    ir.model.parameters.push(DesignParameter {
        id: parameter_id.clone(),
        owner: None,
        ordinal: 0,
        name: "width".into(),
        expression: "10 mm".into(),
        display: None,
        value: None,
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
    ir.model.configurations.push(DesignConfiguration {
        id: configuration_id.clone(),
        ordinal: 0,
        active: false,
        source_index: Some(7),
        name: "Default".into(),
        material: None,
        properties: BTreeMap::new(),
        parameter_overrides: BTreeMap::from([(parameter_id.clone(), "25 mm".into())]),
        suppressed_features: Vec::new(),
        bodies: crate::features::ConfigurationBodies::Resolved(vec![body.clone()]),
        parameter_values: BTreeMap::new(),
        feature_states: BTreeMap::new(),
        native_ref: None,
    });
    ir.finalize();
    assert!(validate(&ir, Vec::new()).is_ok());
    let round_trip = CadIr::from_json(&serde_json::to_string(&ir).unwrap()).unwrap();
    assert_eq!(round_trip.model.configurations[0].bodies, vec![body]);
    assert_eq!(
        round_trip.model.configurations[0].parameter_overrides[&parameter_id],
        "25 mm"
    );

    ir.model.configurations[0].parameter_overrides = BTreeMap::from([(
        ParameterId("synthetic:test:parameter#missing".into()),
        "30 mm".into(),
    )]);
    let report = validate(&ir, Vec::new());
    assert!(report.findings.iter().any(|finding| {
        finding.entity.as_deref() == Some(configuration_id.0.as_str())
            && finding.message.contains("configuration parameter override")
    }));
    ir.model.configurations[0].parameter_overrides.clear();

    ir.model.configurations[0].suppressed_features =
        vec![FeatureId("synthetic:test:feature#missing".into())];
    let report = validate(&ir, Vec::new());
    assert!(report.findings.iter().any(|finding| {
        finding.entity.as_deref() == Some(configuration_id.0.as_str())
            && finding.message.contains("configuration suppressed feature")
    }));
    ir.model.configurations[0].suppressed_features.clear();

    ir.model.configurations[0].bodies = crate::features::ConfigurationBodies::Resolved(vec![
        BodyId("synthetic:test:body#missing".into()),
        BodyId("synthetic:test:body#missing".into()),
    ]);
    let report = validate(&ir, Vec::new());
    assert!(report.findings.iter().any(|finding| {
        finding.entity.as_deref() == Some(configuration_id.0.as_str())
            && finding.message.contains("missing configuration body")
    }));
    assert!(report.findings.iter().any(|finding| {
        finding.entity.as_deref() == Some(configuration_id.0.as_str())
            && finding.message.contains("repeats body")
    }));

    ir.model.configurations.push(DesignConfiguration {
        id: ConfigurationId("synthetic:test:configuration#1".into()),
        ordinal: 0,
        active: false,
        source_index: Some(7),
        name: "Alternate".into(),
        material: None,
        properties: BTreeMap::new(),
        parameter_overrides: BTreeMap::new(),
        suppressed_features: Vec::new(),
        bodies: crate::features::ConfigurationBodies::Resolved(Vec::new()),
        parameter_values: BTreeMap::new(),
        feature_states: BTreeMap::new(),
        native_ref: None,
    });
    ir.model.configurations[0].active = true;
    ir.model.configurations[1].active = true;
    ir.finalize();
    let report = validate(&ir, Vec::new());
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.message.contains("repeats configuration ordinal")));
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.message.contains("multiple active configurations")));
    assert!(report.findings.iter().any(|finding| finding
        .message
        .contains("repeats configuration source index")));
}

#[test]
fn native_records_use_own_ids_for_counts_diff_and_validation() {
    let left = unit_cube();
    let mut right = left.clone();
    right.native.namespace_mut("f3d").arenas.insert(
        "act_guids".into(),
        vec![NativeRecord {
            id: "f3d:test:act-guid#0".into(),
            fields: serde_json::Map::new(),
        }],
    );
    right.native.namespace_mut("sldprt").arenas.insert(
        "configurations".into(),
        vec![NativeRecord {
            id: "sldprt:test:configuration#0".into(),
            fields: serde_json::Map::new(),
        }],
    );
    right.native.finalize();

    let result = diff(&left, &right);
    assert_eq!(
        result
            .per_arena
            .iter()
            .find(|arena| arena.kind == "native.f3d.act_guids")
            .unwrap()
            .added,
        ["f3d:test:act-guid#0"]
    );
    assert_eq!(
        result
            .per_arena
            .iter()
            .find(|arena| arena.kind == "native.sldprt.configurations")
            .unwrap()
            .added,
        ["sldprt:test:configuration#0"]
    );
    let report = validate(&right, Vec::new());
    assert_eq!(report.entity_counts["native.f3d.act_guids"], 1);
    assert_eq!(report.entity_counts["native.sldprt.configurations"], 1);
    assert!(report.is_ok(), "{:?}", report.findings);

    right
        .native
        .namespace_mut("sldprt")
        .arenas
        .get_mut("configurations")
        .unwrap()[0]
        .id = "f3d:test:act-guid#0".into();
    right.native.finalize();
    assert!(validate(&right, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message == "entity id is not globally unique"));
}

#[test]
fn every_cube_edge_has_two_opposite_sense_coedges() {
    let ir = unit_cube();
    for edge in &ir.model.edges {
        let coedges: Vec<_> = ir
            .model
            .coedges
            .iter()
            .filter(|c| c.edge == edge.id)
            .collect();
        assert_eq!(coedges.len(), 2, "edge {} should have 2 coedges", edge.id);
        assert_ne!(
            coedges[0].sense, coedges[1].sense,
            "edge {} coedges should have opposite sense",
            edge.id
        );
        // Partners point at each other.
        assert_eq!(coedges[0].radial_next, coedges[1].id);
        assert_eq!(coedges[1].radial_next, coedges[0].id);
    }
}

#[test]
fn json_round_trips_and_is_deterministic() {
    let ir = unit_cube();
    let json1 = ir.to_canonical_json().unwrap();
    let json2 = ir.to_canonical_json().unwrap();
    assert_eq!(json1, json2, "serialization must be deterministic");

    let parsed = crate::CadIr::from_json(&json1).unwrap();
    assert_eq!(parsed, ir, "round-trip must preserve the document");
    assert_eq!(parsed.to_canonical_json().unwrap(), json1);
}

#[test]
fn json_round_trip_preserves_ulp_edge_scalars_exactly() {
    // Byte-backed writers compare parsed documents against fresh decodes with
    // exact f64 equality, so JSON parsing must be correctly rounded. The
    // values one to a few ULPs below 1.0 are the ones a fast non-roundtrip
    // float parser misparses by one ULP.
    let mut ir = unit_cube();
    let edge_values: Vec<f64> = (1..40)
        .map(|n| 1.0f64 - f64::from(n) * f64::EPSILON / 2.0)
        .collect();
    for (point, value) in ir.model.points.iter_mut().zip(edge_values.iter().cycle()) {
        point.position.x = *value;
    }
    let json = ir.to_canonical_json().unwrap();
    let parsed = crate::CadIr::from_json(&json).unwrap();
    for (before, after) in ir.model.points.iter().zip(&parsed.model.points) {
        assert_eq!(
            before.position.x.to_bits(),
            after.position.x.to_bits(),
            "JSON round-trip changed {} by at least one ULP",
            before.position.x
        );
    }
}

#[test]
fn appearance_asset_and_binding_round_trip() {
    use crate::appearance::{
        Appearance, AppearanceBinding, AppearanceTarget, BumpMap, TextureMap2d, TextureRef,
    };
    use crate::ids::AppearanceId;

    let mut ir = unit_cube();
    let body = ir.model.bodies[0].id.clone();
    ir.model.appearances.push(Appearance {
        id: AppearanceId("synthetic:test:appearance#prism-001".into()),
        name: Some("Prism-001".into()),
        asset_guid: Some("visual-guid".into()),
        visual_guid: Some("visual-guid".into()),
        physical_token: Some("physical-token".into()),
        schema: Some("GenericSchema".into()),
        category: None,
        base_color: Some(crate::topology::Color {
            r: 0.1,
            g: 0.2,
            b: 0.3,
            a: 1.0,
        }),
        properties: std::collections::BTreeMap::new(),
        textures: vec![TextureRef {
            asset_guid: "texture-guid".into(),
            slot: "generic_bump_map".into(),
            schema: "BumpMapSchema".into(),
            paths: vec!["cloud/resource/texture.png".into()],
            urn: Some("adsk.raas:asset.name:texture".into()),
            mapping: TextureMap2d {
                map_channel: 1,
                uvw_source: 0,
                u_offset: 0.25,
                v_offset: -0.5,
                u_scale: 2.0,
                v_scale: 3.0,
                rotation: std::f64::consts::FRAC_PI_2,
                repeat_u: true,
                repeat_v: false,
                real_world_offset_x: 12.7,
                real_world_offset_y: 25.4,
                real_world_scale_x: 304.8,
                real_world_scale_y: 609.6,
            },
            bump: Some(BumpMap {
                normal_map: true,
                depth: 2.54,
                normal_scale: 0.75,
            }),
        }],
    });
    ir.model.appearance_bindings.push(AppearanceBinding {
        id: "synthetic:test:appearance-binding#0".into(),
        target: AppearanceTarget::Body(body),
        appearance: AppearanceId("synthetic:test:appearance#prism-001".into()),
        source_entity_id: Some("0_1".into()),
        object_type: Some("Body".into()),
        channels: std::collections::BTreeMap::new(),
    });
    ir.model.appearance_bindings.push(AppearanceBinding {
        id: "synthetic:test:appearance-binding#edge".into(),
        target: AppearanceTarget::Edge(ir.model.edges[0].id.clone()),
        appearance: AppearanceId("synthetic:test:appearance#prism-001".into()),
        source_entity_id: Some("0_1".into()),
        object_type: Some("Edge".into()),
        channels: std::collections::BTreeMap::new(),
    });
    ir.model.appearance_bindings.push(AppearanceBinding {
        id: "synthetic:test:appearance-binding#vertex".into(),
        target: AppearanceTarget::Vertex(ir.model.vertices[0].id.clone()),
        appearance: AppearanceId("synthetic:test:appearance#prism-001".into()),
        source_entity_id: Some("0_1".into()),
        object_type: Some("Vertex".into()),
        channels: std::collections::BTreeMap::new(),
    });

    let json = ir.to_canonical_json().unwrap();
    let decoded = CadIr::from_json(&json).unwrap();
    assert_eq!(decoded.model.appearances, ir.model.appearances);
    assert_eq!(
        decoded.model.appearance_bindings,
        ir.model.appearance_bindings
    );
}

#[test]
fn dangling_reference_is_flagged() {
    let mut ir = unit_cube();
    // Point a coedge's edge at something that does not exist.
    ir.model.coedges[0].edge = EdgeId("does-not-exist".into());
    let report = validate(&ir, Vec::new());
    assert!(report
        .findings
        .iter()
        .any(|f| f.check == Check::ReferentialIntegrity));
    assert!(!report.is_ok());
}

#[test]
fn coedge_use_curve_requires_a_resolved_carrier_and_interval() {
    let mut ir = unit_cube();
    ir.model.coedges[0].use_curve = Some(CurveId("missing:use-curve#0".into()));
    let report = validate(&ir, Vec::new());
    assert!(report.findings.iter().any(|finding| {
        finding.check == Check::ReferentialIntegrity && finding.message.contains("coedge use curve")
    }));
    assert!(report.findings.iter().any(|finding| {
        finding.check == Check::ParameterDomain
            && finding
                .message
                .contains("use curve and parameter range must occur together")
    }));
}

#[test]
fn broken_loop_ring_is_flagged() {
    let mut ir = unit_cube();
    // Redirect a coedge's `next` to a valid coedge in a different loop, so the
    // referenced id resolves but the ring no longer closes.
    let foreign = ir.model.coedges[20].id.clone();
    ir.model.coedges[0].next = foreign;
    let report = validate(&ir, Vec::new());
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.check == Check::LoopClosure),
        "expected a loop-closure finding, got: {:?}",
        report.findings
    );
}

#[test]
fn mismatched_partner_edge_is_flagged() {
    let mut ir = unit_cube();
    // Force a coedge's partner to reference a coedge on a different edge by
    // repointing the partner's edge. Find coedge[0]'s partner and change it.
    let partner_id: CoedgeId = ir.model.coedges[0].radial_next.clone();
    let other_edge = ir
        .model
        .coedges
        .iter()
        .find(|c| c.edge != ir.model.coedges[0].edge)
        .unwrap()
        .edge
        .clone();
    for c in &mut ir.model.coedges {
        if c.id == partner_id {
            c.edge = other_edge.clone();
        }
    }
    let report = validate(&ir, Vec::new());
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.check == Check::CoedgePairing),
        "expected a coedge-pairing finding, got: {:?}",
        report.findings
    );
}

#[test]
fn signed_sphere_radius_is_valid() {
    let mut ir = unit_cube();
    ir.model.surfaces[0].geometry = SurfaceGeometry::Sphere {
        center: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: -1.0,
    };
    let report = validate(&ir, Vec::new());
    assert!(report.is_ok(), "findings: {:?}", report.findings);
}

#[test]
fn degenerate_plane_normal_is_flagged() {
    let mut ir = unit_cube();
    if let SurfaceGeometry::Plane { normal, .. } = &mut ir.model.surfaces[0].geometry {
        *normal = Vector3::new(0.0, 0.0, 0.0);
    }
    let report = validate(&ir, Vec::new());
    assert!(report.findings.iter().any(|f| f.check == Check::Bounds));
}

#[test]
fn new_topology_references_are_validated() {
    let mut ir = unit_cube();
    ir.model.shells[0]
        .wire_edges
        .push(EdgeId("missing-wire".into()));
    ir.model.shells[0]
        .free_vertices
        .push(crate::ids::VertexId("missing-free".into()));
    ir.model.coedges[0].radial_next = CoedgeId("missing-radial".into());

    let report = validate(&ir, Vec::new());
    let messages = report
        .findings
        .iter()
        .map(|finding| finding.message.as_str())
        .collect::<Vec<_>>();
    assert!(messages.iter().any(|message| message.contains("wire edge")));
    assert!(messages
        .iter()
        .any(|message| message.contains("free vertex")));
    assert!(messages
        .iter()
        .any(|message| message.contains("coedge(radial_next)")));
}

#[test]
fn topology_tolerance_and_new_conics_are_bounds_checked() {
    let mut ir = unit_cube();
    let edge_id = ir.model.edges[0].id.0.clone();
    ir.model.edges[0].tolerance = Some(-1.0);
    ir.model.curves.push(Curve {
        id: CurveId("synthetic:test:curve#bad-parabola".into()),
        geometry: CurveGeometry::Parabola {
            vertex: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            major_direction: Vector3::new(1.0, 0.0, 0.0),
            focal_distance: 0.0,
        },
        source_object: None,
    });
    ir.model.curves.push(Curve {
        id: CurveId("synthetic:test:curve#bad-hyperbola".into()),
        geometry: CurveGeometry::Hyperbola {
            center: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            major_direction: Vector3::new(1.0, 0.0, 0.0),
            major_radius: -1.0,
            minor_radius: 1.0,
        },
        source_object: None,
    });

    let report = validate(&ir, Vec::new());
    for entity in [
        edge_id.as_str(),
        "synthetic:test:curve#bad-parabola",
        "synthetic:test:curve#bad-hyperbola",
    ] {
        assert!(report
            .findings
            .iter()
            .any(
                |finding| (finding.check == Check::Bounds || finding.check == Check::Tolerances)
                    && finding.entity.as_deref() == Some(entity)
            ));
    }
}

#[test]
fn wrong_document_version_is_flagged() {
    let mut ir = unit_cube();
    ir.ir_version = "1".into();
    assert!(validate(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::Version));
}

#[test]
fn parser_rejects_unsupported_missing_and_non_string_versions() {
    let canonical = serde_json::to_value(unit_cube()).unwrap();
    for version in [
        Some(serde_json::Value::String("0".into())),
        None,
        Some(serde_json::Value::Number(1.into())),
    ] {
        let mut value = canonical.clone();
        let object = value.as_object_mut().unwrap();
        match version {
            Some(version) => {
                object.insert("ir_version".into(), version);
            }
            None => {
                object.remove("ir_version");
            }
        }
        let json = serde_json::to_string(&value).unwrap();
        let error = CadIr::from_json(&json).unwrap_err();
        assert!(!error.is_syntax());
        assert!(error.to_string().contains("unsupported ir_version"));
        assert!(serde_json::from_str::<CadIr>(&json).is_err());
    }
}

#[test]
fn direct_deserialization_accepts_current_version_and_canonical_round_trip() {
    let ir = unit_cube();
    let json = ir.to_canonical_json().unwrap();
    let parsed = serde_json::from_str::<CadIr>(&json).unwrap();
    assert_eq!(parsed, ir);
    assert_eq!(parsed.to_canonical_json().unwrap(), json);
}

#[test]
fn schema_constrains_version_and_requires_subd_arena() {
    let schema = serde_json::to_value(crate::cadir_json_schema()).unwrap();
    assert_eq!(
        schema.pointer("/properties/ir_version/const"),
        Some(&serde_json::json!(crate::IR_VERSION))
    );
    assert!(schema
        .pointer("/properties/model/$ref")
        .and_then(serde_json::Value::as_str)
        .is_some());

    let model_schema = schema.pointer("/$defs/Model").unwrap();
    assert!(model_schema
        .pointer("/required")
        .and_then(serde_json::Value::as_array)
        .unwrap()
        .contains(&serde_json::json!("subds")));
    assert!(schema.pointer("/properties/byte_ledger").is_none());

    let mut value = serde_json::to_value(unit_cube()).unwrap();
    value
        .pointer_mut("/model")
        .unwrap()
        .as_object_mut()
        .unwrap()
        .remove("subds");
    assert!(serde_json::from_value::<CadIr>(value).is_err());
}

#[test]
fn subd_round_trip_and_directed_ring_validation() {
    let mut ir = CadIr::empty(crate::units::Units::default());
    ir.model.subds.push(SubdSurface {
        id: SubdId("synthetic:subd:surface#0".into()),
        scheme: SubdScheme::CatmullClark,
        vertices: vec![
            SubdVertex {
                point: Point3::new(0.0, 0.0, 0.0),
                tag: SubdVertexTag::Smooth,
            },
            SubdVertex {
                point: Point3::new(1.0, 0.0, 0.0),
                tag: SubdVertexTag::Smooth,
            },
            SubdVertex {
                point: Point3::new(0.0, 1.0, 0.0),
                tag: SubdVertexTag::Smooth,
            },
        ],
        edges: vec![
            SubdEdge {
                vertices: [0, 1],
                sharpness: [0.0, 0.25],
                tag: SubdEdgeTag::Smooth,
                sector_coefficients: [1.0, 1.0],
            },
            SubdEdge {
                vertices: [1, 2],
                sharpness: [0.25, 0.0],
                tag: SubdEdgeTag::SmoothX,
                sector_coefficients: [1.0, 1.0],
            },
            SubdEdge {
                vertices: [2, 0],
                sharpness: [0.0, 0.0],
                tag: SubdEdgeTag::Smooth,
                sector_coefficients: [1.0, 1.0],
            },
        ],
        faces: vec![SubdFace {
            edges: vec![
                SubdEdgeUse {
                    edge: 0,
                    reversed: false,
                },
                SubdEdgeUse {
                    edge: 1,
                    reversed: false,
                },
                SubdEdgeUse {
                    edge: 2,
                    reversed: false,
                },
            ],
        }],
        source_object: None,
    });
    assert!(validate(&ir, Vec::new()).is_ok());
    let parsed = CadIr::from_json(&ir.to_canonical_json().unwrap()).unwrap();
    assert_eq!(parsed, ir);
    assert_eq!(
        serde_json::to_value(SubdEdgeTag::SmoothX).unwrap(),
        serde_json::json!("smooth_x")
    );
    ir.model.subds[0].faces[0].edges[1].reversed = true;
    assert!(!validate(&ir, Vec::new()).is_ok());
}

#[test]
fn subd_rejects_short_rings_and_negative_sharpness() {
    let mut ir = CadIr::empty(crate::units::Units::default());
    ir.model.subds.push(SubdSurface {
        id: SubdId("synthetic:subd:surface#short".into()),
        scheme: SubdScheme::CatmullClark,
        vertices: vec![
            SubdVertex {
                point: Point3::new(0.0, 0.0, 0.0),
                tag: SubdVertexTag::Smooth,
            },
            SubdVertex {
                point: Point3::new(1.0, 0.0, 0.0),
                tag: SubdVertexTag::Smooth,
            },
        ],
        edges: vec![SubdEdge {
            vertices: [0, 1],
            sharpness: [-0.1, 0.0],
            tag: SubdEdgeTag::Smooth,
            sector_coefficients: [0.0, 0.0],
        }],
        faces: vec![SubdFace {
            edges: vec![
                SubdEdgeUse {
                    edge: 0,
                    reversed: false,
                },
                SubdEdgeUse {
                    edge: 0,
                    reversed: true,
                },
            ],
        }],
        source_object: None,
    });
    let findings = validate(&ir, Vec::new()).findings;
    assert!(findings
        .iter()
        .any(|finding| finding.message.contains("fewer than three")));
    assert!(findings
        .iter()
        .any(|finding| finding.message.contains("edge 0 is invalid")));
}

#[test]
fn revolution_rejects_equal_intervals() {
    let mut ir = unit_cube();
    ir.model.procedural_surfaces.push(ProceduralSurface {
        id: ProceduralSurfaceId("synthetic:test:procedural-surface#equal".into()),
        surface: ir.model.surfaces[0].id.clone(),
        definition: ProceduralSurfaceDefinition::Revolution {
            directrix: ir.model.curves[0].id.clone(),
            axis_origin: Point3::new(0.0, 0.0, 0.0),
            axis_direction: Vector3::new(0.0, 0.0, 1.0),
            angular_interval: [1.0, 1.0],
            parameter_interval: Some([0.0, 1.0]),
            transposed: false,
            revision_form: None,
        },
        cache_fit_tolerance: None,
        record_bounds: None,
    });
    assert!(validate(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message.contains("revolution interval")));
}

#[test]
fn source_association_is_a_free_carrier_root() {
    let mut ir = CadIr::empty(crate::units::Units::default());
    ir.model.curves.push(Curve {
        id: CurveId("synthetic:source:curve#0".into()),
        geometry: CurveGeometry::Unknown { record: None },
        source_object: Some(SourceObjectAssociation {
            format: "rhino".into(),
            object_id: "00000000-0000-0000-0000-000000000000".into(),
            name: Some("curve".into()),
            color: None,
            visible: Some(true),
            layer: Some("layer-0".into()),
            instance_path: Vec::new(),
        }),
    });
    let report = validate(&ir, Vec::new());
    assert!(report.is_ok(), "{:?}", report.findings);
    let parsed = CadIr::from_json(&ir.to_canonical_json().unwrap()).unwrap();
    assert_eq!(parsed, ir);
}

#[test]
fn source_association_rejects_out_of_range_color() {
    let mut ir = CadIr::empty(crate::units::Units::default());
    ir.model.curves.push(Curve {
        id: CurveId("synthetic:source:curve#color".into()),
        geometry: CurveGeometry::Unknown { record: None },
        source_object: Some(SourceObjectAssociation {
            format: "rhino".into(),
            object_id: "object".into(),
            name: None,
            color: Some(Color {
                r: 1.1,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            }),
            visible: None,
            layer: None,
            instance_path: Vec::new(),
        }),
    });
    assert!(validate(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message.contains("outside [0, 1]")));
}

#[test]
fn parser_distinguishes_malformed_json_from_version_rejection() {
    let error = CadIr::from_json("{\"ir_version\":\"1\"").unwrap_err();
    assert!(error.is_syntax() || error.is_eof());
    assert!(!error.to_string().contains("unsupported ir_version"));
}

#[test]
fn entity_ids_follow_canonical_grammar() {
    let mut ir = unit_cube();
    ir.model.points[0].id.0 = "synthetic:scope:point".into();
    let findings = validate(&ir, Vec::new()).findings;
    assert!(findings.iter().any(|finding| {
        finding.check == Check::Identity
            && finding.entity.as_deref() == Some("synthetic:scope:point")
            && finding.message.contains("<format>:<scope>:<kind>#<key>")
    }));
}

#[test]
fn ids_are_globally_unique_across_arenas() {
    let mut ir = unit_cube();
    ir.model.points[0].id.0 = ir.model.vertices[0].id.0.clone();
    assert!(validate(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::Identity));
}

#[test]
fn arena_ids_must_be_sorted() {
    let mut ir = unit_cube();
    ir.model.points.swap(0, 1);
    assert!(validate(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::ArenaOrder));
}

#[test]
fn two_member_radial_ring_with_equal_senses_warns() {
    let mut ir = unit_cube();
    let other_id = ir.model.coedges[0].radial_next.clone();
    let sense = ir.model.coedges[0].sense;
    ir.model
        .coedges
        .iter_mut()
        .find(|coedge| coedge.id == other_id)
        .unwrap()
        .sense = sense;
    assert!(validate(&ir, Vec::new()).findings.iter().any(|finding| {
        finding.check == Check::CoedgePairing
            && finding.severity == crate::report::Severity::Warning
    }));
}

#[test]
fn coedge_backed_edge_cannot_be_a_wire_edge() {
    let mut ir = unit_cube();
    ir.model.shells[0]
        .wire_edges
        .push(ir.model.coedges[0].edge.clone());
    assert!(validate(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::WireTopology));
}

#[test]
fn wire_and_free_topology_negative_cases_are_reported() {
    let mut ir = unit_cube();

    let mut unowned_edge = ir.model.edges[0].clone();
    unowned_edge.id.0 = "synthetic:test:edge#unowned".into();
    ir.model.edges.push(unowned_edge);

    let mut duplicate_edge = ir.model.edges[1].clone();
    duplicate_edge.id.0 = "synthetic:test:edge#duplicate".into();
    ir.model.shells[0]
        .wire_edges
        .extend([duplicate_edge.id.clone(), duplicate_edge.id.clone()]);
    ir.model.edges.push(duplicate_edge);

    let mut unowned_vertex = ir.model.vertices[0].clone();
    unowned_vertex.id.0 = "synthetic:test:vertex#unowned".into();
    ir.model.vertices.push(unowned_vertex);

    ir.model.shells[0]
        .free_vertices
        .push(ir.model.edges[0].start.clone());
    ir.model.bodies[0].kind = crate::topology::BodyKind::Wire;
    ir.finalize();

    let findings = validate(&ir, Vec::new()).findings;
    for message in [
        "wire edge must belong to exactly one shell",
        "free vertex must belong to exactly one shell",
        "free vertex is also referenced by an edge",
        "wire body contains faces",
    ] {
        assert!(
            findings.iter().any(|finding| {
                finding.check == Check::WireTopology && finding.message == message
            }),
            "missing `{message}` in {findings:?}"
        );
    }
}

#[test]
fn singular_loop_vertex_cannot_have_multiple_free_shell_owners() {
    let mut ir = unit_cube();
    let vertex = ir.model.vertices[0].id.clone();
    ir.model.loops[0].coedges.clear();
    ir.model.loops[0].vertex_uses = vec![crate::topology::VertexUse {
        vertex: vertex.clone(),
        after: None,
        pcurves: Vec::new(),
    }];
    ir.model.shells[0].free_vertices.push(vertex.clone());
    let mut second_shell = ir.model.shells[0].clone();
    second_shell.id.0 = "synthetic:test:shell#second".into();
    second_shell.faces.clear();
    second_shell.wire_edges.clear();
    second_shell.free_vertices = vec![vertex];
    ir.model.regions[0].shells.push(second_shell.id.clone());
    ir.model.shells.push(second_shell);

    assert!(validate(&ir, Vec::new()).findings.iter().any(|finding| {
        finding.check == Check::WireTopology
            && finding.message == "free vertex must belong to exactly one shell"
    }));
}

#[test]
fn self_referential_composite_curve_is_invalid() {
    use crate::geometry::{CompositeCurveSegment, CompositeCurveTransition};

    let mut ir = unit_cube();
    let id = CurveId("synthetic:test:curve#recursive".into());
    ir.model.curves.push(Curve {
        id: id.clone(),
        geometry: CurveGeometry::Composite {
            segments: vec![CompositeCurveSegment {
                curve: id,
                same_sense: true,
                transition: CompositeCurveTransition::Continuous,
            }],
            self_intersect: Some(false),
        },
        source_object: None,
    });

    assert!(validate(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::ReferentialIntegrity));
}

#[test]
fn empty_shell_is_reported() {
    let mut ir = unit_cube();
    ir.model.shells[0].faces.clear();
    let findings = validate(&ir, Vec::new()).findings;
    assert!(findings.iter().any(|finding| {
        finding.check == Check::WireTopology && finding.message == "shell owns no topology"
    }));
}

#[test]
fn orphan_carrier_is_flagged() {
    let mut ir = unit_cube();
    let mut orphan = ir.model.curves[0].clone();
    orphan.id = CurveId("zz:orphan".into());
    ir.model.curves.push(orphan);
    assert!(validate(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::CarrierReachability));
}

#[test]
fn annotation_keys_streams_and_field_paths_are_checked() {
    let ir = unit_cube();
    let mut source_fidelity = crate::source_fidelity::SourceFidelity::default();
    source_fidelity.annotations.provenance.insert(
        "missing".into(),
        Provenance {
            stream: u32::MAX,
            offset: 0,
            tag: None,
        },
    );
    source_fidelity.annotations.exactness.insert(
        ir.model.edges[0].id.0.clone(),
        ExactnessNote {
            entity: Exactness::Derived,
            fields: std::collections::BTreeMap::from([(
                "not_a_serialized_field".into(),
                Exactness::Derived,
            )]),
        },
    );
    let findings = crate::validate_with_source_fidelity(&ir, &source_fidelity, Vec::new()).findings;
    assert!(findings.iter().any(|finding| {
        finding.check == Check::Annotations && finding.severity == crate::report::Severity::Error
    }));
    assert!(findings.iter().any(|finding| {
        finding.check == Check::Annotations && finding.severity == crate::report::Severity::Warning
    }));
}

#[test]
fn native_topology_link_must_resolve() {
    let mut ir = unit_cube();
    ir.native.namespace_mut("f3d").arenas.insert(
        "sketch_curve_links".into(),
        vec![NativeRecord {
            id: "native:link#0".into(),
            fields: serde_json::from_value(serde_json::json!({"links": ["missing"]})).unwrap(),
        }],
    );
    ir.native.finalize();
    assert!(validate(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::NativeLinks));
}

#[test]
fn parameter_native_ref_must_resolve() {
    let mut ir = unit_cube();
    let id = crate::features::ParameterId("synthetic:test:parameter#native-ref".into());
    ir.model.parameters.push(crate::features::DesignParameter {
        id: id.clone(),
        owner: Some(crate::features::FeatureId(
            "synthetic:test:feature#missing".into(),
        )),
        ordinal: 0,
        name: "D1".into(),
        expression: "1mm".into(),
        display: None,
        value: None,
        dependencies: Vec::new(),
        properties: std::collections::BTreeMap::new(),
        pmi: Some(crate::features::ParameterPmi {
            subtype: crate::features::PmiDimensionSubtype::Linear,
            precision: 2,
            display_text: None,
            basic: false,
            inspection: false,
            reference_only: false,
            native_ref: "native:pmi-missing#0".into(),
        }),
        native_ref: Some("native:missing#0".into()),
    });
    assert!(validate(&ir, Vec::new()).findings.iter().any(|finding| {
        finding.check == Check::NativeLinks && finding.entity.as_deref() == Some(id.0.as_str())
    }));
    assert!(validate(&ir, Vec::new()).findings.iter().any(|finding| {
        finding.check == Check::NativeLinks
            && finding.message.contains("PMI native_ref")
            && finding.entity.as_deref() == Some(id.0.as_str())
    }));
}

#[test]
fn sketch_constraint_native_ref_must_resolve() {
    let mut ir = unit_cube();
    let id =
        crate::sketches::SketchConstraintId("synthetic:test:sketch-constraint#native-ref".into());
    ir.model
        .sketch_constraints
        .push(crate::sketches::SketchConstraint {
            id: id.clone(),
            sketch: crate::sketches::SketchId("synthetic:test:sketch#missing".into()),
            definition: crate::sketches::SketchConstraintDefinition::Native {
                native_kind: "test".into(),
                native_state: None,
                entities: Vec::new(),
                parameter: None,
                operands: vec![crate::sketches::SketchNativeOperand {
                    native_kind: "test".into(),
                    native_field: None,
                    native_role: None,
                    object_index: 0,
                    native_ref: Some("native:missing-operand#0".into()),
                }],
            },
            name: None,
            driving: None,
            active: None,
            virtual_space: None,
            visible: None,
            orientation: None,
            label_distance: None,
            label_position: None,
            metadata: None,
            native_ref: Some("native:missing-relation#0".into()),
        });

    assert!(validate(&ir, Vec::new()).findings.iter().any(|finding| {
        finding.check == Check::NativeLinks
            && finding.entity.as_deref() == Some(id.0.as_str())
            && finding.message.contains("native:missing-relation#0")
    }));
    assert!(validate(&ir, Vec::new()).findings.iter().any(|finding| {
        finding.check == Check::NativeLinks
            && finding.entity.as_deref() == Some(id.0.as_str())
            && finding.message.contains("native:missing-operand#0")
    }));
    let crate::sketches::SketchConstraintDefinition::Native { operands, .. } =
        &mut ir.model.sketch_constraints[0].definition
    else {
        unreachable!("test constraint is native")
    };
    operands[0].native_role = Some(7);
    assert!(validate(&ir, Vec::new()).findings.iter().any(|finding| {
        finding.check == Check::Counts && finding.entity.as_deref() == Some(id.0.as_str())
    }));
}

#[test]
fn periodic_curve_parameter_domain_is_checked() {
    let mut ir = unit_cube();
    let curve_id = ir.model.edges[0].curve.clone().unwrap();
    ir.model
        .curves
        .iter_mut()
        .find(|curve| curve.id == curve_id)
        .unwrap()
        .geometry = CurveGeometry::Circle {
        center: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 1.0,
    };
    ir.model.edges[0].param_range = Some([0.0, 7.0]);
    assert!(validate(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::ParameterDomain));

    ir.model.edges[0].param_range = Some([-std::f64::consts::PI, std::f64::consts::PI]);
    assert!(!validate(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::ParameterDomain));
}

#[test]
fn periodic_nurbs_parameters_preserve_phase_and_wrap_for_evaluation() {
    let nurbs = crate::geometry::NurbsCurve {
        degree: 1,
        knots: vec![0.0, 0.0, 1.0, 2.0, 2.0],
        control_points: vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 0.0, 0.0),
        ],
        weights: None,
        periodic: true,
    };
    let geometry = CurveGeometry::Nurbs(nurbs.clone());
    assert_eq!(
        crate::eval::curve_point(&geometry, 0.5),
        crate::eval::curve_point(&geometry, 2.5)
    );

    let mut ir = unit_cube();
    let curve_id = ir.model.edges[0].curve.clone().unwrap();
    ir.model
        .curves
        .iter_mut()
        .find(|curve| curve.id == curve_id)
        .unwrap()
        .geometry = geometry;
    ir.model.edges[0].param_range = Some([0.5, 2.5]);
    assert!(!validate(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::ParameterDomain));

    ir.model.edges[0].param_range = Some([0.5, 2.500_001]);
    assert!(validate(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::ParameterDomain));

    let CurveGeometry::Nurbs(nurbs) = &mut ir
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == curve_id)
        .unwrap()
        .geometry
    else {
        unreachable!()
    };
    nurbs.periodic = false;
    ir.model.edges[0].param_range = Some([0.5, 2.5]);
    assert!(validate(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::ParameterDomain));
}

#[test]
fn document_and_entity_tolerances_are_checked() {
    let mut ir = unit_cube();
    ir.tolerances.angular = f64::NAN;
    ir.model.faces[0].tolerance = Some(0.0);
    assert!(validate(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::Tolerances));
}

#[test]
fn byte_payloads_use_nonempty_base64_and_reject_invalid_text() {
    assert_base64_round_trip_and_rejection(
        &UnknownRecord {
            id: UnknownId("synthetic:test:unknown#0".into()),
            offset: 0,
            byte_len: 3,
            sha256: "00".repeat(32),
            data: Some(vec![1, 2, 3]),
            links: Vec::new(),
        },
        "data",
    );
    assert_base64_round_trip_and_rejection(
        &TessellationChannel {
            item_size: 3,
            kind: 0,
            flags: 0,
            count: 1,
            data: vec![1, 2, 3],
        },
        "data",
    );
}

#[test]
fn schema_generation_produces_definitions() {
    let schema = crate::cadir_json_schema();
    // The schema must reference the entity types, not just be an empty object.
    let json = serde_json::to_string(&schema).unwrap();
    assert!(json.contains("Body"));
    assert!(json.contains("Coedge"));
    assert!(json.contains("SurfaceGeometry"));
    let defs = schema
        .get("$defs")
        .and_then(serde_json::Value::as_object)
        .expect("schema has a $defs object");
    assert!(!defs.is_empty());
}

#[test]
fn loss_provenance_root_alias_constructs_and_serializes() {
    let note = LossNote {
        code: LossCode::GeometryNotTransferred,
        category: LossCategory::Geometry,
        severity: Severity::Warning,
        message: "geometry was retained as metadata".into(),
        provenance: Some(crate::provenance::Provenance {
            format: "rhino".into(),
            stream: String::new(),
            offset: 42,
            tag: Some(
                "OBJECT_RECORD/class=00000000-0000-0000-0000-000000000000/type=0x00000020".into(),
            ),
        }),
    };
    let json = serde_json::to_value(&note).unwrap();
    assert_eq!(json["provenance"]["format"], "rhino");
    assert_eq!(json["provenance"]["stream"], "");
    assert_eq!(json["provenance"]["offset"], 42);
    assert_eq!(
        json["provenance"]["tag"],
        "OBJECT_RECORD/class=00000000-0000-0000-0000-000000000000/type=0x00000020"
    );
}

#[test]
fn rational_quadratic_arc_evaluates_on_the_circle() {
    // Quarter circle of radius 5 as a rational quadratic Bezier.
    let weight = 0.5_f64.sqrt();
    let point = crate::eval::nurbs_curve_point(
        2,
        &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        &[
            Point3::new(5.0, 0.0, 0.0),
            Point3::new(5.0, 5.0, 0.0),
            Point3::new(0.0, 5.0, 0.0),
        ],
        Some(&[1.0, weight, 1.0]),
        0.5,
    )
    .unwrap();
    let radius = (point.x * point.x + point.y * point.y).sqrt();
    assert!((radius - 5.0).abs() < 1e-12, "mid-span radius {radius}");
}

#[test]
fn rational_pcurve_membership_finds_interior_points_without_sampling() {
    use crate::math::Point2;

    let weight = 0.5_f64.sqrt();
    let knots = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
    let controls = [
        Point2::new(5.0, 0.0),
        Point2::new(5.0, 5.0),
        Point2::new(0.0, 5.0),
    ];
    let weights = [1.0, weight, 1.0];
    let interior =
        crate::eval::nurbs_pcurve_uv(2, &knots, &controls, Some(&weights), 0.375).unwrap();
    assert_eq!(
        crate::eval::nurbs_pcurve_contains_point(
            2,
            &knots,
            &controls,
            Some(&weights),
            interior,
            1.0e-9,
        ),
        Some(true)
    );
    assert_eq!(
        crate::eval::nurbs_pcurve_contains_point(
            2,
            &knots,
            &controls,
            Some(&weights),
            Point2::new(4.0, 4.0),
            1.0e-6,
        ),
        Some(false)
    );
}

#[test]
fn analytic_parabola_and_hyperbola_use_step_parameterization() {
    let axis = Vector3::new(0.0, 0.0, 1.0);
    let major = Vector3::new(1.0, 0.0, 0.0);
    let parabola = CurveGeometry::Parabola {
        vertex: Point3::new(0.0, 0.0, 0.0),
        axis,
        major_direction: major,
        focal_distance: 2.0,
    };
    assert_eq!(
        crate::eval::curve_point(&parabola, 1.5),
        Some(Point3::new(4.5, 6.0, 0.0))
    );

    let hyperbola = CurveGeometry::Hyperbola {
        center: Point3::new(1.0, 2.0, 3.0),
        axis,
        major_direction: major,
        major_radius: 2.0,
        minor_radius: 3.0,
    };
    let point = crate::eval::curve_point(&hyperbola, 0.5).unwrap();
    assert_eq!(point.x, 1.0 + 2.0 * 0.5_f64.cosh());
    assert_eq!(point.y, 2.0 + 3.0 * 0.5_f64.sinh());
    assert_eq!(point.z, 3.0);
}

#[test]
fn edge_endpoint_mismatch_is_flagged() {
    let mut ir = unit_cube();
    let report = validate(&ir, Vec::new());
    assert!(
        !report
            .findings
            .iter()
            .any(|f| f.check == Check::GeometricConsistency),
        "worked cube must be geometrically consistent, got: {:?}",
        report.findings
    );

    // Displace one corner: the point no longer lies on its edges' curves at
    // the stored parameter values.
    ir.model.points[0].position.z += 1.0;
    let report = validate(&ir, Vec::new());
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.check == Check::GeometricConsistency
                && f.severity == Severity::Error
                && f.entity.as_deref().is_some_and(|e| e.contains("edge"))),
        "displaced vertex must fail edge endpoint consistency, got: {:?}",
        report.findings
    );
}

#[test]
fn pcurve_surface_mismatch_is_flagged() {
    // The bottom face's plane is `origin (0,0,0), normal (0,0,-1)`, whose
    // derived u/v frame maps `(u, v) -> (u, -v, 0)`. Edge #0 runs from
    // `(0,0,0)` to `(10,0,0)`, so its parameter image is the line
    // `(0,0) -> (10,0)`.
    let good = |u_end: f64, v_end: f64| {
        let mut ir = unit_cube();
        ir.model.pcurves.push(crate::geometry::Pcurve {
            id: crate::ids::PcurveId("synthetic:cube:pcurve#0".into()),
            geometry: crate::geometry::PcurveGeometry::Nurbs {
                degree: 1,
                knots: vec![0.0, 0.0, 1.0, 1.0],
                control_points: vec![
                    crate::math::Point2::new(0.0, 0.0),
                    crate::math::Point2::new(u_end, v_end),
                ],
                weights: None,
                periodic: false,
            },
            wrapper_reversed: None,
            native_tail_flags: None,
            parameter_range: None,
            fit_tolerance: None,
        });
        let coedge = ir
            .model
            .coedges
            .iter_mut()
            .find(|coedge| {
                coedge.id.0.contains("bottom") && coedge.edge.0 == "synthetic:cube:edge#0"
            })
            .expect("bottom face uses edge #0");
        coedge.pcurves = vec![crate::topology::PcurveUse {
            pcurve: crate::ids::PcurveId("synthetic:cube:pcurve#0".into()),
            isoparametric: None,
            parameter_range: None,
        }];
        validate(&ir, Vec::new())
    };

    let consistent = good(10.0, 0.0);
    assert!(
        !consistent
            .findings
            .iter()
            .any(|f| f.check == Check::GeometricConsistency),
        "matching pcurve must validate, got: {:?}",
        consistent.findings
    );

    let inconsistent = good(10.0, 5.0);
    assert!(
        inconsistent
            .findings
            .iter()
            .any(|f| f.check == Check::GeometricConsistency
                && f.entity.as_deref().is_some_and(|e| e.contains("coedge"))),
        "off-surface-image pcurve must be flagged, got: {:?}",
        inconsistent.findings
    );

    let mut procedural = unit_cube();
    procedural.model.pcurves.push(crate::geometry::Pcurve {
        id: crate::ids::PcurveId("synthetic:cube:pcurve#procedural".into()),
        geometry: crate::geometry::PcurveGeometry::Nurbs {
            degree: 1,
            knots: vec![0.0, 0.0, 1.0, 1.0],
            control_points: vec![
                crate::math::Point2::new(0.0, 0.0),
                crate::math::Point2::new(10.0, 5.0),
            ],
            weights: None,
            periodic: false,
        },
        wrapper_reversed: None,
        native_tail_flags: None,
        parameter_range: None,
        fit_tolerance: None,
    });
    let coedge = procedural
        .model
        .coedges
        .iter_mut()
        .find(|coedge| coedge.id.0.contains("bottom") && coedge.edge.0 == "synthetic:cube:edge#0")
        .expect("bottom face uses edge #0");
    coedge.pcurves = vec![crate::topology::PcurveUse {
        pcurve: crate::ids::PcurveId("synthetic:cube:pcurve#procedural".into()),
        isoparametric: None,
        parameter_range: None,
    }];
    let owner_loop = coedge.owner_loop.clone();
    let face = procedural
        .model
        .loops
        .iter()
        .find(|lp| lp.id == owner_loop)
        .and_then(|lp| {
            procedural
                .model
                .faces
                .iter()
                .find(|face| face.id == lp.face)
        })
        .expect("coedge owner face");
    procedural
        .model
        .procedural_surfaces
        .push(ProceduralSurface {
            id: ProceduralSurfaceId("synthetic:cube:procedural-surface#0".into()),
            surface: face.surface.clone(),
            definition: ProceduralSurfaceDefinition::Revolution {
                directrix: procedural.model.curves[0].id.clone(),
                axis_origin: Point3::new(0.0, 0.0, 0.0),
                axis_direction: Vector3::new(0.0, 0.0, 1.0),
                angular_interval: [0.0, std::f64::consts::TAU],
                parameter_interval: Some([0.0, 1.0]),
                transposed: false,
                revision_form: None,
            },
            cache_fit_tolerance: Some(0.01),
            record_bounds: None,
        });
    let procedural_report = validate(&procedural, Vec::new());
    assert!(
        !procedural_report
            .findings
            .iter()
            .any(|finding| finding.check == Check::GeometricConsistency),
        "procedural UVs must not be evaluated on the solved cache, got: {:?}",
        procedural_report.findings
    );

    let mut negative_parameterization = unit_cube();
    negative_parameterization
        .model
        .pcurves
        .push(crate::geometry::Pcurve {
            id: crate::ids::PcurveId("synthetic:cube:pcurve#negative".into()),
            geometry: crate::geometry::PcurveGeometry::Nurbs {
                degree: 1,
                knots: vec![-10.0, -10.0, 0.0, 0.0],
                control_points: vec![
                    crate::math::Point2::new(10.0, 0.0),
                    crate::math::Point2::new(0.0, 0.0),
                ],
                weights: None,
                periodic: false,
            },
            wrapper_reversed: None,
            native_tail_flags: None,
            parameter_range: None,
            fit_tolerance: None,
        });
    let coedge = negative_parameterization
        .model
        .coedges
        .iter_mut()
        .find(|coedge| coedge.id.0.contains("bottom") && coedge.edge.0 == "synthetic:cube:edge#0")
        .expect("bottom face uses edge #0");
    coedge.pcurves = vec![crate::topology::PcurveUse {
        pcurve: crate::ids::PcurveId("synthetic:cube:pcurve#negative".into()),
        isoparametric: None,
        parameter_range: Some([-10.0, 0.0]),
    }];
    let ranged_coedge_id = coedge.id.clone();
    let negative = validate(&negative_parameterization, Vec::new());
    assert!(
        !negative
            .findings
            .iter()
            .any(|finding| finding.check == Check::GeometricConsistency),
        "opposite-sign pcurve parameterization must validate, got: {:?}",
        negative.findings
    );

    negative_parameterization
        .model
        .coedges
        .iter_mut()
        .find(|coedge| coedge.id == ranged_coedge_id)
        .expect("ranged coedge")
        .pcurves[0]
        .parameter_range = Some([-11.0, 0.0]);
    let invalid_range = validate(&negative_parameterization, Vec::new());
    assert!(invalid_range.findings.iter().any(|finding| {
        finding.check == Check::ParameterDomain && finding.message.contains("coedge pcurve range")
    }));
}

#[test]
fn vertex_loop_is_valid_and_exclusive_with_coedges() {
    let mut ir = unit_cube();
    let face_id = ir.model.faces[0].id.clone();
    let vertex_id = ir.model.vertices[0].id.clone();
    let loop_id = crate::ids::LoopId("synthetic:cube:vertex-loop#0".into());
    ir.model.loops.push(crate::topology::Loop {
        id: loop_id.clone(),
        face: face_id,
        boundary_role: crate::topology::LoopBoundaryRole::Inner,
        coedges: Vec::new(),
        vertex_uses: vec![crate::topology::VertexUse {
            vertex: vertex_id,
            after: None,
            pcurves: Vec::new(),
        }],
    });
    ir.model.faces[0].loops.push(loop_id.clone());
    ir.model.finalize();
    let report = validate(&ir, Vec::new());
    assert!(report.is_ok(), "{:#?}", report.findings);

    ir.model
        .loops
        .iter_mut()
        .find(|loop_| loop_.id == loop_id)
        .unwrap()
        .boundary_role = crate::topology::LoopBoundaryRole::Outer;
    let report = validate(&ir, Vec::new());
    assert!(report.findings.iter().any(|finding| {
        finding.check == Check::LoopClosure
            && finding.message == "face has more than one explicit outer loop"
    }));
    ir.model
        .loops
        .iter_mut()
        .find(|loop_| loop_.id == loop_id)
        .unwrap()
        .boundary_role = crate::topology::LoopBoundaryRole::Inner;

    let coedge = ir.model.loops[0].coedges[0].clone();
    ir.model
        .loops
        .iter_mut()
        .find(|loop_| loop_.id == loop_id)
        .unwrap()
        .coedges
        .push(coedge);
    let report = validate(&ir, Vec::new());
    assert!(report.findings.iter().any(|finding| {
        finding.check == Check::LoopClosure && finding.entity.as_deref() == Some(loop_id.0.as_str())
    }));
}

#[test]
fn ordered_pcurve_uses_round_trip_with_isoparametric_state() {
    let uses = vec![
        crate::topology::PcurveUse {
            pcurve: crate::ids::PcurveId("test:pcurve#first".into()),
            isoparametric: Some(true),
            parameter_range: None,
        },
        crate::topology::PcurveUse {
            pcurve: crate::ids::PcurveId("test:pcurve#second".into()),
            isoparametric: Some(false),
            parameter_range: Some([0.0, 1.0]),
        },
    ];
    let json = serde_json::to_string(&uses).unwrap();
    assert_eq!(
        serde_json::from_str::<Vec<crate::topology::PcurveUse>>(&json).unwrap(),
        uses
    );
}

#[test]
fn feature_extents_round_trip_through_json() {
    use crate::features::{
        Angle, ExtrudeExtent, ExtrudeSide, FaceSelection, Length, RevolveExtent, Termination,
    };
    use crate::ids::FaceId;

    let extents = vec![
        ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: Termination::Blind {
                    length: Length(12.5),
                },
                draft: Some(Angle(0.1)),
                offset: None,
            },
        },
        ExtrudeExtent::Symmetric {
            side: ExtrudeSide {
                termination: Termination::Blind {
                    length: Length(25.0),
                },
                draft: None,
                offset: None,
            },
        },
        ExtrudeExtent::TwoSided {
            first: ExtrudeSide {
                termination: Termination::Blind {
                    length: Length(10.0),
                },
                draft: Some(Angle(0.2)),
                offset: Some(Length(1.0)),
            },
            second: ExtrudeSide {
                termination: Termination::ToFace {
                    face: FaceSelection::Faces(vec![FaceId("synthetic:test:face#0".into())]),
                    offset: None,
                },
                draft: None,
                offset: Some(Length(-2.0)),
            },
        },
        ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: Termination::ThroughAll,
                draft: None,
                offset: None,
            },
        },
    ];
    let json = serde_json::to_string(&extents).unwrap();
    assert_eq!(
        serde_json::from_str::<Vec<ExtrudeExtent>>(&json).unwrap(),
        extents
    );

    let revolve_extents = vec![
        RevolveExtent::OneSided {
            termination: Termination::Angle {
                angle: Angle(std::f64::consts::PI),
            },
        },
        RevolveExtent::Symmetric {
            termination: Termination::Angle {
                angle: Angle(std::f64::consts::FRAC_PI_2),
            },
        },
        RevolveExtent::TwoSided {
            first: Termination::Angle { angle: Angle(0.25) },
            second: Termination::Angle { angle: Angle(0.75) },
        },
    ];
    let json = serde_json::to_string(&revolve_extents).unwrap();
    assert_eq!(
        serde_json::from_str::<Vec<RevolveExtent>>(&json).unwrap(),
        revolve_extents
    );
}

#[test]
fn feature_extent_magnitudes_are_validated() {
    use crate::features::{
        Angle, BooleanOp, ExtrudeExtent, ExtrudeSide, Feature, FeatureDefinition, FeatureId,
        Length, ProfileRef, Termination,
    };

    let side = |termination: Termination| ExtrudeSide {
        termination,
        draft: None,
        offset: None,
    };
    for extent in [
        ExtrudeExtent::OneSided {
            side: side(Termination::Blind {
                length: Length(0.0),
            }),
        },
        ExtrudeExtent::TwoSided {
            first: side(Termination::Blind {
                length: Length(1.0),
            }),
            second: side(Termination::Blind {
                length: Length(f64::NAN),
            }),
        },
        ExtrudeExtent::OneSided {
            side: side(Termination::Angle { angle: Angle(-1.0) }),
        },
    ] {
        let mut ir = unit_cube();
        ir.model.features.push(Feature {
            id: FeatureId("synthetic:test:feature#invalid-extent".into()),
            ordinal: 0,
            name: None,
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: std::collections::BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Extrude {
                profile: ProfileRef::Native("profile".into()),
                direction: ExtrudeDirection::ProfileNormal,
                start: crate::features::ExtrudeStart::ProfilePlane,
                extent,
                op: BooleanOp::NewBody,
                direction_source: None,
                solid: None,
                face_maker: None,
                inner_wire_taper: None,
                length_along_profile_normal: None,
                allow_multi_profile_faces: None,
            },
            native_ref: None,
        });
        assert!(validate(&ir, Vec::new())
            .findings
            .iter()
            .any(|finding| finding.message == "feature extent magnitude is invalid"));
    }
}

#[test]
fn explicit_extrusion_direction_must_be_nonzero() {
    use crate::features::{
        BooleanOp, ExtrudeExtent, ExtrudeSide, Feature, FeatureDefinition, FeatureId, Length,
        ProfileRef, Termination,
    };

    let mut ir = unit_cube();
    ir.model.features.push(Feature {
        id: FeatureId("synthetic:test:feature#invalid-extrude-direction".into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Extrude {
            profile: ProfileRef::Native("profile".into()),
            direction: ExtrudeDirection::Explicit(Vector3::new(0.0, 0.0, 0.0)),
            start: crate::features::ExtrudeStart::ProfilePlane,
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(1.0),
                    },
                    draft: None,
                    offset: None,
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
        native_ref: None,
    });
    assert!(validate(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message == "extrusion direction is invalid"));
}

#[test]
fn loft_sections_accept_legacy_profiles_and_preserve_profile_shape() {
    use crate::features::{BooleanOp, FeatureDefinition, LoftSection, ProfileRef};

    let legacy = serde_json::json!({
        "definition": "loft",
        "profiles": [{"kind": "native", "value": "native:section"}],
        "op": "new_body",
        "closed": false
    });
    let definition: FeatureDefinition = serde_json::from_value(legacy).unwrap();
    assert!(matches!(
        &definition,
        FeatureDefinition::Loft {
            sections,
            guides,
            centerline: None,
            op: BooleanOp::NewBody,
            closed: false,
            ..
        } if sections == &vec![LoftSection::Profile(ProfileRef::Native("native:section".into()))]
            && guides.is_empty()
    ));
    let encoded = serde_json::to_value(definition).unwrap();
    assert_eq!(
        encoded["sections"][0],
        serde_json::json!({"kind": "native", "value": "native:section"})
    );
}

#[test]
fn extrusion_side_drafts_are_validated() {
    use crate::features::{
        Angle, BooleanOp, ExtrudeExtent, ExtrudeSide, Feature, FeatureDefinition, FeatureId,
        Length, ProfileRef, Termination,
    };

    let side = |length: f64, draft: Option<Angle>| ExtrudeSide {
        termination: Termination::Blind {
            length: Length(length),
        },
        draft,
        offset: None,
    };
    for (extent, expected_invalid) in [
        (
            ExtrudeExtent::TwoSided {
                first: side(1.0, None),
                second: side(2.0, Some(Angle(0.25))),
            },
            false,
        ),
        (
            ExtrudeExtent::TwoSided {
                first: side(1.0, None),
                second: side(2.0, Some(Angle(f64::NAN))),
            },
            true,
        ),
        (
            ExtrudeExtent::Symmetric {
                side: side(1.0, Some(Angle(std::f64::consts::FRAC_PI_2))),
            },
            true,
        ),
    ] {
        let mut ir = unit_cube();
        ir.model.features.push(Feature {
            id: FeatureId("synthetic:test:feature#side-draft".into()),
            ordinal: 0,
            name: None,
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: std::collections::BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Extrude {
                profile: ProfileRef::Native("profile".into()),
                direction: ExtrudeDirection::ProfileNormal,
                start: crate::features::ExtrudeStart::ProfilePlane,
                extent,
                op: BooleanOp::NewBody,
                direction_source: None,
                solid: None,
                face_maker: None,
                inner_wire_taper: None,
                length_along_profile_normal: None,
                allow_multi_profile_faces: None,
            },
            native_ref: None,
        });
        let has_draft_finding = validate(&ir, Vec::new())
            .findings
            .iter()
            .any(|finding| finding.message == "extrusion draft is invalid");
        assert_eq!(has_draft_finding, expected_invalid);
    }
}

#[test]
fn sketch_feature_ownership_and_order_are_validated() {
    use crate::features::{
        BooleanOp, ExtrudeExtent, ExtrudeSide, Feature, FeatureDefinition, FeatureId, Length,
        ProfileRef, Termination,
    };
    use crate::sketches::{Sketch, SketchId};

    let mut ir = unit_cube();
    let sketch_id = SketchId("synthetic:test:sketch#ordered".into());
    ir.model.sketches.push(Sketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        placement: crate::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: Vec::new(),
        native_ref: None,
    });
    ir.model.features.push(Feature {
        id: FeatureId("synthetic:test:feature#consumer".into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Extrude {
            profile: ProfileRef::Sketch(sketch_id.clone()),
            direction: ExtrudeDirection::ProfileNormal,
            start: crate::features::ExtrudeStart::ProfilePlane,
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(1.0),
                    },
                    draft: None,
                    offset: None,
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
        native_ref: None,
    });
    for (ordinal, suffix) in [(1, "owner"), (2, "duplicate-owner")] {
        ir.model.features.push(Feature {
            id: FeatureId(format!("synthetic:test:feature#{suffix}")),
            ordinal,
            name: None,
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: std::collections::BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Sketch {
                space: crate::features::SketchSpace::Planar,
                sketch: Some(sketch_id.clone()),
            },
            native_ref: None,
        });
    }
    let findings = validate(&ir, Vec::new()).findings;
    assert!(findings.iter().any(|finding| finding
        .message
        .contains("does not precede its profile consumer")));
    assert!(findings
        .iter()
        .any(|finding| finding.message.contains("has multiple owning features")));
}

#[test]
fn sketch_profile_subselections_are_bounds_checked() {
    use crate::features::{
        BooleanOp, ExtrudeExtent, ExtrudeSide, Feature, FeatureDefinition, FeatureId, Length,
        ProfileRef, SketchProfileRegion, Termination,
    };
    use crate::sketches::{Sketch, SketchId};

    let mut ir = unit_cube();
    let sketch_id = SketchId("synthetic:test:sketch#selection".into());
    ir.model.sketches.push(Sketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        placement: crate::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: Vec::new(),
        native_ref: None,
    });
    let feature = |suffix: &str, ordinal, profile| Feature {
        id: FeatureId(format!("synthetic:test:feature#{suffix}")),
        ordinal,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Extrude {
            profile,
            direction: ExtrudeDirection::ProfileNormal,
            start: crate::features::ExtrudeStart::ProfilePlane,
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(1.0),
                    },
                    draft: None,
                    offset: None,
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
        native_ref: None,
    };
    ir.model.features.push(feature(
        "invalid-profile-index",
        1,
        ProfileRef::SketchProfiles {
            sketch: sketch_id.clone(),
            profiles: vec![0, 0],
        },
    ));
    ir.model.features.push(feature(
        "invalid-region",
        2,
        ProfileRef::SketchRegions {
            sketch: sketch_id.clone(),
            regions: vec![SketchProfileRegion::Loops {
                outer: 0,
                holes: vec![0, 0],
            }],
        },
    ));
    ir.model.features.push(feature(
        "empty-native-selection",
        3,
        ProfileRef::SketchSelection {
            sketch: sketch_id,
            selections: Vec::new(),
        },
    ));

    let findings = validate(&ir, Vec::new()).findings;
    assert!(findings.iter().any(|finding| {
        finding.message == "sketch profile indices are empty, repeated, or out of range"
    }));
    assert!(
        findings
            .iter()
            .any(|finding| finding.message
                == "native sketch profile selections are empty or repeated")
    );
    assert!(findings.iter().any(|finding| {
        finding.message
            == "sketch regions have empty, repeated, invalid, or out-of-range boundaries"
    }));
}

#[test]
fn sketch_regions_round_trip_with_explicit_boundary_roles() {
    use crate::features::{ProfileRef, SketchProfileBoundaryUse, SketchProfileRegion};
    use crate::sketches::{SketchEntityId, SketchId};

    let profile = ProfileRef::SketchRegions {
        sketch: SketchId("synthetic:test:sketch#region".into()),
        regions: vec![
            SketchProfileRegion::Loops {
                outer: 2,
                holes: vec![3, 5],
            },
            SketchProfileRegion::Loops {
                outer: 8,
                holes: Vec::new(),
            },
            SketchProfileRegion::Trimmed {
                outer_boundary: vec![SketchProfileBoundaryUse {
                    entity: SketchEntityId("synthetic:test:sketch-entity#curve".into()),
                    parameter_range: [0.25, 0.75],
                    reversed: true,
                }],
                hole_boundaries: Vec::new(),
            },
        ],
    };
    let json = serde_json::to_value(&profile).expect("serialize sketch regions");
    assert_eq!(json["kind"], "sketch_regions");
    assert_eq!(json["value"]["regions"][0]["outer"], 2);
    assert_eq!(
        json["value"]["regions"][0]["holes"],
        serde_json::json!([3, 5])
    );
    assert!(json["value"]["regions"][1].get("holes").is_none());
    assert_eq!(
        json["value"]["regions"][2]["outer_boundary"][0]["parameter_range"],
        serde_json::json!([0.25, 0.75])
    );
    assert_eq!(
        json["value"]["regions"][2]["outer_boundary"][0]["reversed"],
        true
    );
    assert_eq!(
        serde_json::from_value::<ProfileRef>(json).expect("deserialize sketch regions"),
        profile
    );
}

#[test]
fn spatial_sketch_feature_owns_spatial_geometry() {
    use crate::features::{Feature, FeatureDefinition, FeatureId};
    use crate::sketches::{SpatialSketch, SpatialSketchId};

    let mut ir = unit_cube();
    let sketch_id = SpatialSketchId("synthetic:test:spatial-sketch#owned".into());
    ir.model.spatial_sketches.push(SpatialSketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        profiles: Vec::new(),
        native_ref: None,
    });
    ir.model.features.push(Feature {
        id: FeatureId("synthetic:test:feature#spatial-sketch".into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::SpatialSketch {
            sketch: Some(sketch_id),
        },
        native_ref: None,
    });

    assert!(validate(&ir, Vec::new()).findings.is_empty());
    let mut duplicate = ir.model.features.last().expect("spatial owner").clone();
    duplicate.id = FeatureId("synthetic:test:feature#duplicate-spatial-sketch".into());
    duplicate.ordinal = 1;
    ir.model.features.push(duplicate);
    assert!(validate(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message.contains("has multiple owning features")));
}

#[test]
fn spatial_sketch_geometry_round_trips_and_validates() {
    use crate::features::{DesignParameter, Length, ParameterId, ParameterValue};
    use crate::sketches::{
        SketchConstraintId, SpatialSketch, SpatialSketchConstraint,
        SpatialSketchConstraintDefinition, SpatialSketchEntity, SpatialSketchEntityId,
        SpatialSketchEntityUse, SpatialSketchGeometry, SpatialSketchId, SpatialSketchProfile,
    };

    let mut ir = unit_cube();
    let sketch = SpatialSketchId("synthetic:test:spatial-sketch#one".into());
    let circle = SpatialSketchEntityId("synthetic:test:spatial-sketch-entity#circle".into());
    ir.model.spatial_sketches.push(SpatialSketch {
        id: sketch.clone(),
        name: Some("3D path".into()),
        configuration: None,
        profiles: vec![SpatialSketchProfile {
            origin: Point3::new(1.0, 2.0, 3.0),
            normal: Vector3::new(0.0, 1.0, 0.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
            boundary: vec![SpatialSketchEntityUse {
                entity: circle.clone(),
                reversed: false,
            }],
        }],
        native_ref: None,
    });
    ir.model.spatial_sketch_entities.push(SpatialSketchEntity {
        id: circle.clone(),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SpatialSketchGeometry::Circle {
            center: Point3::new(1.0, 2.0, 3.0),
            normal: Vector3::new(0.0, 1.0, 0.0),
            reference_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: Length(4.0),
        },
    });
    let parallel_line =
        SpatialSketchEntityId("synthetic:test:spatial-sketch-entity#parallel-line".into());
    ir.model.spatial_sketch_entities.push(SpatialSketchEntity {
        id: parallel_line.clone(),
        sketch: sketch.clone(),
        construction: true,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SpatialSketchGeometry::Line {
            start: Point3::new(0.0, 2.0f64.sqrt(), -2.0f64.sqrt()),
            end: Point3::new(1.0, 1.0 + 2.0f64.sqrt(), 1.0 - 2.0f64.sqrt()),
        },
    });
    let distance = ParameterId("synthetic:test:parameter#spatial-distance".into());
    ir.model.parameters.push(DesignParameter {
        id: distance.clone(),
        owner: None,
        ordinal: 0,
        name: "spatial_distance".into(),
        expression: "2 mm".into(),
        display: None,
        value: Some(ParameterValue::Length(Length(2.0))),
        dependencies: Vec::new(),
        properties: Default::default(),
        pmi: None,
        native_ref: None,
    });
    let surface = SpatialSketchEntityId("synthetic:test:spatial-sketch-entity#surface".into());
    ir.model.spatial_sketch_entities.push(SpatialSketchEntity {
        id: surface.clone(),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SpatialSketchGeometry::NurbsSurface {
            u_degree: 1,
            v_degree: 1,
            u_knots: vec![0.0, 0.0, 1.0, 1.0],
            v_knots: vec![0.0, 0.0, 1.0, 1.0],
            control_points: vec![
                vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
                vec![Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)],
            ],
        },
    });
    let surface_point =
        SpatialSketchEntityId("synthetic:test:spatial-sketch-entity#surface-point".into());
    ir.model.spatial_sketch_entities.push(SpatialSketchEntity {
        id: surface_point.clone(),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SpatialSketchGeometry::Point {
            position: Point3::new(0.5, 0.5, 0.0),
        },
    });
    let line = SpatialSketchEntityId("synthetic:test:spatial-sketch-entity#line".into());
    ir.model.spatial_sketch_entities.push(SpatialSketchEntity {
        id: line.clone(),
        sketch: sketch.clone(),
        construction: true,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SpatialSketchGeometry::Line {
            start: Point3::new(0.0, 0.0, 0.0),
            end: Point3::new(1.0, 1.0, 1.0),
        },
    });
    let point = SpatialSketchEntityId("synthetic:test:spatial-sketch-entity#point".into());
    ir.model.spatial_sketch_entities.push(SpatialSketchEntity {
        id: point.clone(),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SpatialSketchGeometry::Point {
            position: Point3::new(0.5, 0.5, 0.5),
        },
    });
    let coincident_point =
        SpatialSketchEntityId("synthetic:test:spatial-sketch-entity#coincident-point".into());
    ir.model.spatial_sketch_entities.push(SpatialSketchEntity {
        id: coincident_point.clone(),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SpatialSketchGeometry::Point {
            position: Point3::new(0.5, 0.5, 0.5),
        },
    });
    ir.model
        .spatial_sketch_constraints
        .push(SpatialSketchConstraint {
            id: SketchConstraintId("synthetic:test:spatial-sketch-constraint#group".into()),
            sketch: sketch.clone(),
            definition: SpatialSketchConstraintDefinition::SplineGroup {
                entities: vec![line.clone(), circle.clone()],
            },
            native_ref: None,
        });
    ir.model
        .spatial_sketch_constraints
        .push(SpatialSketchConstraint {
            id: SketchConstraintId("synthetic:test:spatial-sketch-constraint#point-surface".into()),
            sketch: sketch.clone(),
            definition: SpatialSketchConstraintDefinition::PointOnSurface {
                point: surface_point,
                surface,
            },
            native_ref: None,
        });
    ir.model
        .spatial_sketch_constraints
        .push(SpatialSketchConstraint {
            id: SketchConstraintId("synthetic:test:spatial-sketch-constraint#coincident".into()),
            sketch: sketch.clone(),
            definition: SpatialSketchConstraintDefinition::Coincident {
                first: point.clone(),
                second: coincident_point,
            },
            native_ref: None,
        });
    ir.model
        .spatial_sketch_constraints
        .push(SpatialSketchConstraint {
            id: SketchConstraintId("synthetic:test:spatial-sketch-constraint#midpoint".into()),
            sketch: sketch.clone(),
            definition: SpatialSketchConstraintDefinition::Midpoint {
                point,
                entity: line.clone(),
            },
            native_ref: None,
        });
    ir.model
        .spatial_sketch_constraints
        .push(SpatialSketchConstraint {
            id: SketchConstraintId("synthetic:test:spatial-sketch-constraint#direction".into()),
            sketch: sketch.clone(),
            definition: SpatialSketchConstraintDefinition::ParallelToDirection {
                entity: line.clone(),
                direction: Vector3::new(
                    1.0 / 3.0f64.sqrt(),
                    1.0 / 3.0f64.sqrt(),
                    1.0 / 3.0f64.sqrt(),
                ),
            },
            native_ref: None,
        });
    ir.model
        .spatial_sketch_constraints
        .push(SpatialSketchConstraint {
            id: SketchConstraintId("synthetic:test:spatial-sketch-constraint#distance".into()),
            sketch: sketch.clone(),
            definition: SpatialSketchConstraintDefinition::ParallelLineDistance {
                first: line.clone(),
                second: parallel_line,
                parameter: distance.clone(),
            },
            native_ref: None,
        });
    ir.model
        .spatial_sketch_constraints
        .push(SpatialSketchConstraint {
            id: SketchConstraintId("synthetic:test:spatial-sketch-constraint#tangent".into()),
            sketch,
            definition: SpatialSketchConstraintDefinition::Tangent {
                first: line,
                second: circle,
            },
            native_ref: None,
        });
    ir.finalize();
    assert!(validate(&ir, Vec::new()).findings.is_empty());
    let mut invalid_distance = ir.clone();
    invalid_distance
        .model
        .parameters
        .iter_mut()
        .find(|parameter| parameter.id == distance)
        .expect("spatial distance parameter")
        .value = Some(ParameterValue::Length(Length(3.0)));
    assert!(validate(&invalid_distance, Vec::new())
        .findings
        .iter()
        .any(|finding| finding
            .message
            .contains("spatial distance requires parallel lines")));
    let json = ir.to_canonical_json().expect("serialize spatial sketch");
    let decoded = CadIr::from_json(&json).expect("deserialize spatial sketch");
    assert_eq!(decoded.model.spatial_sketches, ir.model.spatial_sketches);
    assert_eq!(
        decoded.model.spatial_sketch_entities,
        ir.model.spatial_sketch_entities
    );
    assert_eq!(
        decoded.model.spatial_sketch_constraints,
        ir.model.spatial_sketch_constraints
    );
}

#[test]
fn feature_operation_geometry_is_validated() {
    use crate::features::{
        BooleanOp, EdgeSelection, FaceSelection, Feature, FeatureDefinition, FeatureId,
        FilletGroup, HoleKind, Length, PatternKind, ProfileRef, RadiusSpec, RibConstruction,
        RibDraft, RibSide, ScaleCenter, ScaleFactors, Termination, ThickenSide, VariableRadius,
    };

    let definitions = vec![
        FeatureDefinition::Form { cages: Vec::new() },
        FeatureDefinition::Form {
            cages: vec![crate::ids::SubdId("synthetic:test:subd#missing".into())],
        },
        FeatureDefinition::Fillet {
            groups: vec![FilletGroup {
                edges: EdgeSelection::Unresolved,
                radius: RadiusSpec::Variable {
                    points: vec![
                        VariableRadius {
                            parameter: 0.5,
                            radius: Length(2.0),
                        },
                        VariableRadius {
                            parameter: 0.25,
                            radius: Length(-1.0),
                        },
                    ],
                },
                tangency_weight: None,
            }],
        },
        FeatureDefinition::Rib {
            construction: RibConstruction {
                profile: Some(ProfileRef::Native("profile".into())),
                direction: Some(Vector3::new(0.0, 0.0, 0.0)),
                thickness: Some(Length(0.0)),
                side: Some(RibSide::OneSided),
                draft: RibDraft::None,
            },
            op: BooleanOp::Join,
        },
        FeatureDefinition::Hole {
            profile: None,
            profile_filter: None,
            face: Some(FaceSelection::Unresolved),
            position: None,
            direction: None,
            kind: HoleKind::Simple,
            exit_kind: None,
            diameter: Some(Length(0.0)),
            extent: Some(Termination::ThroughAll),
            bottom: None,
            taper_angle: None,
            specification: None,
            placements: Vec::new(),
            allow_multi_profile_faces: None,
        },
        FeatureDefinition::Thicken {
            faces: FaceSelection::Unresolved,
            thickness: Some(Length(0.0)),
            side: Some(ThickenSide::Forward),
        },
        FeatureDefinition::OffsetSurface {
            faces: FaceSelection::Unresolved,
            distance: Some(Length(f64::NAN)),
        },
        FeatureDefinition::KnitSurface {
            faces: FaceSelection::Unresolved,
            merge_entities: Some(true),
            create_solid: Some(false),
            gap_tolerance: Some(Length(-1.0)),
        },
        FeatureDefinition::ExtendSurface {
            faces: FaceSelection::Unresolved,
            distance: Some(Length(0.0)),
            method: crate::features::SurfaceExtension::Natural,
        },
        FeatureDefinition::RuledSurface {
            edges: EdgeSelection::Unresolved,
            support_faces: FaceSelection::Unresolved,
            mode: crate::features::RuledSurfaceMode::Direction {
                direction: Vector3::new(0.0, 0.0, 0.0),
                distance: Length(0.0),
            },
        },
        FeatureDefinition::Scale {
            bodies: crate::features::BodySelection::Unresolved,
            center: Some(ScaleCenter::Point(Point3::new(0.0, f64::NAN, 0.0))),
            factors: ScaleFactors {
                uniform: None,
                x: Some(1.0),
                y: Some(0.0),
                z: Some(1.0),
            },
        },
        FeatureDefinition::DatumCoordinateSystem {
            origin: Point3::new(0.0, 0.0, 0.0),
            x_axis: Vector3::new(1.0, 0.0, 0.0),
            y_axis: Vector3::new(1.0, 0.0, 0.0),
            z_axis: Vector3::new(0.0, 0.0, 1.0),
        },
        FeatureDefinition::EquationCurve {
            parameter: String::new(),
            x_expression: "t".into(),
            y_expression: "0".into(),
            z_expression: "0".into(),
            start: 1.0,
            end: 0.0,
        },
        FeatureDefinition::ProjectedCurve {
            source: crate::features::PathRef::Native("source".into()),
            target_faces: FaceSelection::Unresolved,
            direction: crate::features::CurveProjectionDirection::Vector(Vector3::new(
                0.0, 0.0, 0.0,
            )),
            bidirectional: Some(false),
        },
        FeatureDefinition::CompositeCurve {
            segments: Vec::new(),
            closed: false,
        },
        FeatureDefinition::Helix {
            axis_origin: Point3::new(0.0, 0.0, 0.0),
            axis_direction: Vector3::new(0.0, 0.0, 0.0),
            radius: Length(-1.0),
            pitch: Length(f64::NAN),
            revolutions: 0.0,
            start_angle: crate::features::Angle(0.0),
            clockwise: false,
            radial_growth: None,
            cone_angle: None,
            segment_turns: None,
            construction_style: None,
        },
        FeatureDefinition::HelixNativeAxis {
            axis_native_ref: String::new(),
            axial_rise: Length(f64::NAN),
            pitch: Length(f64::NAN),
            revolutions: 0.0,
            start_angle: crate::features::Angle(f64::NAN),
            clockwise: false,
        },
        FeatureDefinition::Sphere {
            center: Point3::new(0.0, f64::NAN, 0.0),
            radius: Length(0.0),
            op: BooleanOp::NewBody,
        },
        FeatureDefinition::Torus {
            center: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 0.0),
            major_radius: Length(10.0),
            minor_radius: Length(-1.0),
            op: BooleanOp::NewBody,
        },
        FeatureDefinition::HelicalSweep {
            construction: crate::features::HelicalSweepConstruction {
                profile: ProfileRef::Native("profile".into()),
                axis_origin: Point3::new(0.0, 0.0, 0.0),
                axis_direction: Vector3::new(0.0, 0.0, 0.0),
                law: crate::features::HelicalSweepLaw::HeightTurnsGrowth,
                pitch: Length(0.0),
                height: Length(0.0),
                turns: 0.0,
                radial_growth: Length(0.0),
                cone_angle: crate::features::Angle(0.0),
                left_handed: false,
                reversed: false,
                tolerance: 0.0,
                allow_multi_profile_faces: None,
            },
            op: crate::features::BooleanOp::Join,
        },
        FeatureDefinition::Binder {
            sources: vec![crate::features::BinderSource {
                target: crate::features::BinderTarget::Native {
                    reference: String::new(),
                },
                subelements: vec![String::new()],
            }],
            construction: crate::features::BinderConstruction::SubShape {
                lifecycle: crate::features::BinderLifecycle::Synchronized,
                placement: crate::features::BinderPlacement::Relative,
                copy_on_change: crate::features::BinderCopyOnChange::Disabled,
                claim_children: false,
                fuse: false,
                make_face: true,
                partial_load: false,
                refine: true,
                offset: Some(crate::features::BinderOffset {
                    distance: Length(0.0),
                    join: crate::features::BinderOffsetJoin::Arcs,
                    fill: false,
                    open_result: false,
                    intersection: false,
                }),
                context: None,
            },
        },
        FeatureDefinition::Wrap {
            profile: ProfileRef::Native("profile".into()),
            face: FaceSelection::Unresolved,
            mode: crate::features::WrapMode::Emboss,
            depth: None,
        },
        FeatureDefinition::MoveBody {
            bodies: crate::features::BodySelection::Unresolved,
            translation: Vector3::new(f64::NAN, 0.0, 0.0),
            rotation: Some(crate::features::AxisAngle {
                origin: Point3::new(0.0, 0.0, 0.0),
                direction: Vector3::new(0.0, 0.0, 0.0),
                angle: crate::features::Angle(0.5),
            }),
            copies: 0,
        },
        FeatureDefinition::Pattern {
            seeds: Vec::new(),
            pattern: PatternKind::Linear {
                direction: Some(Vector3::new(0.0, 0.0, 0.0)),
                spacing: Length(-1.0),
                count: 0,
                second: None,
            },
        },
        FeatureDefinition::Pattern {
            seeds: Vec::new(),
            pattern: PatternKind::CurveDriven {
                path: None,
                spacing: Length(0.0),
                count: 0,
            },
        },
        FeatureDefinition::Pattern {
            seeds: Vec::new(),
            pattern: PatternKind::Composite {
                stages: vec![
                    crate::features::PatternStage {
                        pattern: Box::new(PatternKind::Linear {
                            direction: Some(Vector3::new(1.0, 0.0, 0.0)),
                            spacing: Length(1.0),
                            count: 3,
                            second: None,
                        }),
                        combination: crate::features::PatternStageCombination::Initialize,
                    },
                    crate::features::PatternStage {
                        pattern: Box::new(PatternKind::Scale {
                            center: crate::features::PatternScaleCenter::FirstSeedCentroid,
                            final_factor: 2.0,
                            count: 2,
                        }),
                        combination: crate::features::PatternStageCombination::AlignedSlices,
                    },
                ],
            },
        },
        FeatureDefinition::Sweep {
            profile: None,
            sections: Vec::new(),
            path: None,
            mode: crate::features::SweepMode::Unresolved,
            orientation: None,
            transition: None,
            transformation: None,
            path_tangent: false,
            linearize: false,
            twist: None,
            scale: Some(-1.0),
            allow_multi_profile_faces: None,
        },
        FeatureDefinition::DatumOffsetPlane {
            reference: None,
            distance: Length(f64::NAN),
        },
    ];
    let expected = [
        "Form operation has no control cage",
        "references missing Form control cage `synthetic:test:subd#missing`",
        "fillet radius is invalid",
        "rib geometry is invalid",
        "hole geometry is invalid",
        "thicken thickness is invalid",
        "surface offset is invalid",
        "knit tolerance is invalid",
        "surface extension is invalid",
        "ruled surface is invalid",
        "scale transform is invalid",
        "coordinate-system frame is invalid",
        "equation curve is invalid",
        "projection direction is invalid",
        "composite curve is empty",
        "helix geometry is invalid",
        "sphere primitive is invalid",
        "torus primitive is invalid",
        "helical sweep is invalid",
        "binder construction is invalid",
        "wrap depth is invalid",
        "body motion is invalid",
        "pattern geometry is invalid",
        "pattern geometry is invalid",
        "pattern geometry is invalid",
        "sweep magnitude is invalid",
        "datum-plane offset is invalid",
    ];
    let mut ir = unit_cube();
    for (ordinal, definition) in definitions.into_iter().enumerate() {
        ir.model.features.push(Feature {
            id: FeatureId(format!("synthetic:test:feature#invalid-{ordinal}")),
            ordinal: ordinal as u64,
            name: None,
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: std::collections::BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition,
            native_ref: None,
        });
    }
    let findings = validate(&ir, Vec::new()).findings;
    for message in expected {
        assert!(findings.iter().any(|finding| finding.message == message));
    }
}

#[test]
fn flex_modes_round_trip_and_validate() {
    use crate::features::{Angle, Feature, FeatureDefinition, FeatureId, FlexMode, Length};

    let modes = vec![
        FlexMode::Bending { angle: Angle(0.5) },
        FlexMode::Twisting { angle: Angle(1.0) },
        FlexMode::Tapering { factor: 1.5 },
        FlexMode::Stretching {
            distance: Length(12.0),
        },
    ];
    let json = serde_json::to_string(&modes).unwrap();
    assert_eq!(serde_json::from_str::<Vec<FlexMode>>(&json).unwrap(), modes);

    let mut ir = unit_cube();
    ir.model.features.push(Feature {
        id: FeatureId("synthetic:test:feature#flex".into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Flex {
            axis: Some(Vector3::new(0.0, 0.0, 0.0)),
            mode: FlexMode::Tapering { factor: 0.0 },
        },
        native_ref: None,
    });
    let findings = validate(&ir, Vec::new()).findings;
    assert!(findings
        .iter()
        .any(|finding| finding.message == "flex axis is degenerate"));
    assert!(findings
        .iter()
        .any(|finding| finding.message == "flex magnitude is invalid"));
}

#[test]
fn edge_selections_round_trip_through_json() {
    use crate::features::EdgeSelection;
    use crate::ids::{EdgeId, FeatureInputTopologyId, HistoricalEdgeId};

    let selections = vec![
        EdgeSelection::Unresolved,
        EdgeSelection::Edges(vec![EdgeId("synthetic:test:edge#0".into())]),
        EdgeSelection::Resolved {
            edges: vec![EdgeId("synthetic:test:edge#0".into())],
            native: "edge:10".into(),
        },
        EdgeSelection::Historical {
            state: FeatureInputTopologyId("synthetic:history-input:state#0".into()),
            edges: vec![HistoricalEdgeId("synthetic:history-input:edge#0".into())],
            native: "edge:9".into(),
        },
        EdgeSelection::HistoricalPartial {
            state: FeatureInputTopologyId("synthetic:history-input:state#0".into()),
            edges: vec![HistoricalEdgeId("synthetic:history-input:edge#0".into())],
            unresolved: vec!["native:edge-operand#1".into()],
            native: "edge:9".into(),
        },
        EdgeSelection::Native("sldprt:history:feature#10:0".into()),
    ];
    let json = serde_json::to_string(&selections).unwrap();
    assert_eq!(
        serde_json::from_str::<Vec<EdgeSelection>>(&json).unwrap(),
        selections
    );
}

#[test]
fn historical_edge_paths_round_trip_through_json() {
    use crate::features::PathRef;
    use crate::ids::{FeatureInputTopologyId, HistoricalEdgeId};

    let path = PathRef::HistoricalEdges {
        state: FeatureInputTopologyId("synthetic:history-input:state#0".into()),
        edges: vec![
            HistoricalEdgeId("synthetic:history-input:edge#0".into()),
            HistoricalEdgeId("synthetic:history-input:edge#1".into()),
        ],
        native: "native:path#0".into(),
    };
    let json = serde_json::to_string(&path).unwrap();
    assert_eq!(serde_json::from_str::<PathRef>(&json).unwrap(), path);
}

#[test]
fn spatial_sketch_paths_round_trip_through_json() {
    use crate::features::PathRef;
    use crate::sketches::SpatialSketchId;

    let path = PathRef::SpatialSketchSelection {
        sketch: SpatialSketchId("synthetic:test:spatial-sketch#0".into()),
        selections: vec!["native:path-selection#0".into()],
    };
    let json = serde_json::to_string(&path).unwrap();
    assert_eq!(serde_json::from_str::<PathRef>(&json).unwrap(), path);
}

#[test]
fn face_selections_round_trip_through_json() {
    use crate::features::FaceSelection;
    use crate::ids::{FaceId, FeatureInputTopologyId, HistoricalFaceId};

    let selections = vec![
        FaceSelection::Unresolved,
        FaceSelection::Faces(vec![FaceId("synthetic:test:face#0".into())]),
        FaceSelection::Resolved {
            faces: vec![FaceId("synthetic:test:face#0".into())],
            native: "face:14".into(),
        },
        FaceSelection::Historical {
            state: FeatureInputTopologyId("synthetic:history-input:state#0".into()),
            faces: vec![HistoricalFaceId("synthetic:history-input:face#0".into())],
            native: "face:13".into(),
        },
        FaceSelection::HistoricalPartial {
            state: FeatureInputTopologyId("synthetic:history-input:state#0".into()),
            faces: vec![HistoricalFaceId("synthetic:history-input:face#0".into())],
            unresolved: vec!["native:face-operand#1".into()],
            native: "face:12".into(),
        },
        FaceSelection::Native("sldprt:history:feature#14:0".into()),
    ];
    let json = serde_json::to_string(&selections).unwrap();
    assert_eq!(
        serde_json::from_str::<Vec<FaceSelection>>(&json).unwrap(),
        selections
    );
}

#[test]
fn historical_face_profiles_round_trip_through_json() {
    use crate::features::ProfileRef;
    use crate::ids::{FeatureInputTopologyId, HistoricalFaceId};

    let profile = ProfileRef::HistoricalFaces {
        state: FeatureInputTopologyId("synthetic:history-input:state#0".into()),
        faces: vec![HistoricalFaceId("synthetic:history-input:face#0".into())],
        native: vec!["native:profile-group#0".into()],
    };
    let json = serde_json::to_string(&profile).unwrap();
    assert_eq!(serde_json::from_str::<ProfileRef>(&json).unwrap(), profile);
}

#[test]
fn body_selections_round_trip_through_json() {
    use crate::features::BodySelection;
    use crate::ids::{BodyId, FeatureInputTopologyId, HistoricalBodyId};

    let selections = vec![
        BodySelection::Unresolved,
        BodySelection::Bodies(vec![BodyId("synthetic:test:body#0".into())]),
        BodySelection::Resolved {
            bodies: vec![BodyId("synthetic:test:body#0".into())],
            native: "body:17".into(),
        },
        BodySelection::Historical {
            state: FeatureInputTopologyId("synthetic:history-input:state#0".into()),
            bodies: vec![HistoricalBodyId("synthetic:history-input:body#0".into())],
            native: "body:16".into(),
        },
        BodySelection::Native("body:17,body:18".into()),
    ];
    let json = serde_json::to_string(&selections).unwrap();
    assert_eq!(
        serde_json::from_str::<Vec<BodySelection>>(&json).unwrap(),
        selections
    );
}

#[test]
fn transformed_carriers_preserve_basis_parameters() {
    let transform = crate::transform::Transform {
        rows: [
            [-2.0, 0.0, 0.0, 4.0],
            [0.0, 2.0, 0.0, 5.0],
            [0.0, 0.0, 2.0, 6.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    };
    let curve = CurveGeometry::Transformed {
        basis: Box::new(CurveGeometry::Line {
            origin: Point3::new(1.0, 0.0, 0.0),
            direction: Vector3::new(1.0, 0.0, 0.0),
        }),
        transform,
    };
    assert_eq!(
        crate::eval::curve_point(&curve, 3.0),
        Some(Point3::new(-4.0, 5.0, 6.0))
    );

    let surface = SurfaceGeometry::Transformed {
        basis: Box::new(SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        }),
        transform,
    };
    assert_eq!(
        crate::eval::surface_point(&surface, 2.0, 3.0),
        Some(Point3::new(0.0, 11.0, 6.0))
    );
}

#[test]
fn polyline_carriers_evaluate_in_both_parameter_directions() {
    let increasing = CurveGeometry::Polyline {
        points: vec![Point3::new(0.0, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0)],
        parameters: Some(vec![1.0, 3.0]),
        chordal_deflection: 0.01,
    };
    assert_eq!(
        crate::eval::curve_point(&increasing, 2.0),
        Some(Point3::new(1.0, 0.0, 0.0))
    );

    let decreasing = CurveGeometry::Polyline {
        points: vec![Point3::new(0.0, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0)],
        parameters: Some(vec![3.0, 1.0]),
        chordal_deflection: 0.01,
    };
    assert_eq!(
        crate::eval::curve_point(&decreasing, 2.5),
        Some(Point3::new(0.5, 0.0, 0.0))
    );
}

#[test]
fn current_document_excludes_source_byte_accounting() {
    let ir = CadIr::empty(crate::units::Units::default());
    let json = serde_json::to_value(&ir).unwrap();

    assert_eq!(json["ir_version"], crate::IR_VERSION);
    assert!(json.get("byte_ledger").is_none());
}
