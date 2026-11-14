// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_ir::math::Point3;
use serde_json::json;

fn model(start: f64, normal: f64) -> Model {
    let mut model = cadmpeg_ir::examples::unit_cube()
        .expect("synthetic cube")
        .model;
    let history = serde_json::from_value(json!({
        "id":"history", "features":[{
            "id":"native", "parent":"history", "ordinal":0,
            "name":"Extrude", "kind":"BossExtrude", "xml_tag":"Extrusion",
            "parameters":{"Depth":"10mm"},
            "properties":{"EndCondition":"Blind", "Operation":"Join"}
        }]
    }))
    .expect("synthetic history");
    model.features = crate::history::project::project_features(&[history]).expect("feature");
    let sketch: Sketch = serde_json::from_value(json!({
        "id":"synthetic:travel:sketch#0", "placement":{"kind":"resolved","frame":{
            "origin":{"x":0.0,"y":0.0,"z":start},
            "normal":{"x":0.0,"y":0.0,"z":normal},
            "u_axis":{"x":1.0,"y":0.0,"z":0.0}
        }}
    }))
    .expect("synthetic frame");
    let corners = [(2.0, 2.0), (8.0, 2.0), (8.0, 8.0), (2.0, 8.0)];
    for (i, (a, b)) in corners
        .iter()
        .zip(corners.iter().cycle().skip(1))
        .take(4)
        .enumerate()
    {
        model.sketch_entities.push(
            serde_json::from_value(json!({
                "id":format!("synthetic:travel:entity#{i}"),"sketch":sketch.id,
                "geometry":{"kind":"line","start":{"u":a.0,"v":a.1*normal},
                "end":{"u":b.0,"v":b.1*normal}}
            }))
            .expect("synthetic profile line"),
        );
    }
    model.features[0].evaluation.edit(|definition, _| {
        let FeatureDefinition::Operation(FeatureOperation::Extrude { profile, .. }) = definition
        else {
            panic!("extrude");
        };
        *profile = ProfileRef::Planar(PlanarProfileRef::Sketch(sketch.id.clone()));
    });
    model.sketches.push(sketch);
    model
}

fn remove_cap(model: &mut Model, z: f64) {
    let ids: HashSet<_> = model
        .surfaces
        .iter()
        .filter_map(|surface| {
            let SolvedSurfaceGeometry::Plane(plane) = surface.geometry.solved()? else {
                return None;
            };
            (plane.frame().axis().as_raw().z.abs() == 1.0 && plane.origin().get().z == z)
                .then_some(surface.id.clone())
        })
        .collect();
    model.faces.retain(|face| !ids.contains(&face.surface));
}

#[test]
fn opposite_caps_contradict_implicit_travel_with_mirrored_controls() {
    for (start, normal, contradicted) in [
        (10.0, 1.0, true),
        (10.0, -1.0, false),
        (0.0, 1.0, false),
        (0.0, -1.0, true),
    ] {
        let mut model = model(start, normal);
        let original = model.features[0].evaluation.definition().clone();
        assert_eq!(
            contradicted_features(&model).contains(&model.features[0].id),
            contradicted
        );
        assert_eq!(retain_contradicted_directions(&mut model), contradicted);
        let FeatureOperation::Extrude { direction, .. } =
            model.features[0].evaluation.definition().operation()
        else {
            panic!("extrude");
        };
        assert_eq!(
            matches!(direction, ExtrudeDirection::Unresolved {}),
            contradicted
        );
        if contradicted {
            let mut expected = original;
            retain(&mut expected);
            assert_eq!(model.features[0].evaluation.definition(), &expected);
            assert!(contradicted_features(&model).contains(&model.features[0].id));
        } else {
            assert_eq!(model.features[0].evaluation.definition(), &original);
        }
    }
}

#[test]
fn absent_caps_and_carrier_failures_preserve_travel() {
    for missing in [0.0, 10.0] {
        let mut model = model(10.0, 1.0);
        remove_cap(&mut model, missing);
        assert!(contradicted_features(&model).is_empty());
        assert!(!retain_contradicted_directions(&mut model));
    }
    let mut model = model(10.0, 1.0);
    model.sketches[0].placement = cadmpeg_ir::sketches::SketchPlacement::try_resolved(
        Point3::new(20.0, 0.0, 10.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
    )
    .expect("translated profile");
    assert!(contradicted_features(&model).is_empty());
}

#[test]
fn through_all_and_explicit_travel_preserve_their_definitions() {
    for through_all in [false, true] {
        let mut model = model(10.0, 1.0);
        model.features[0].evaluation.edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::Extrude {
                direction, extent, ..
            }) = definition
            else {
                panic!("extrude");
            };
            if through_all {
                let ExtrudeExtent::OneSided { side } = extent else {
                    panic!("one side");
                };
                side.termination = LinearTermination::ThroughAll {};
            } else {
                *direction = ExtrudeDirection::Explicit {
                    vector: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(
                        0.0, 0.0, 1.0,
                    ))
                    .expect("unit vector"),
                    source: None,
                };
            }
        });
        assert!(contradicted_features(&model).is_empty());
    }
}

#[test]
fn decoded_side_cap_prevents_a_contradiction() {
    let model = model(10.0, 1.0);
    let work = WorkBudget::new(MAX_WITNESS_WORK);
    let mut caps = caps(&model, &work).expect("finite caps");
    let mut decoded = Cap {
        origin: Point3::new(0.0, 0.0, 20.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
        sense: 1.0,
        vertices: Vec::new(),
        triangles: Vec::new(),
        transform: Transform::identity(),
        polygons: Some(vec![vec![
            Point3::new(0.0, 0.0, 20.0),
            Point3::new(10.0, 0.0, 20.0),
            Point3::new(10.0, 10.0, 20.0),
            Point3::new(0.0, 10.0, 20.0),
        ]]),
    };
    decoded.vertices.extend(
        decoded.polygons.as_ref().expect("polygon")[0]
            .iter()
            .copied(),
    );
    caps.push(decoded);
    assert!(!contradicts(&model, &model.features[0], &caps, &work));
}

#[test]
fn cut_witness_uses_the_cavity_face_sense() {
    let mut model = model(10.0, 1.0);
    for face in &mut model.faces {
        face.sense = Sense::Reversed;
    }
    model.features[0].evaluation.edit(|definition, _| {
        let FeatureDefinition::Operation(FeatureOperation::Extrude { op, .. }) = definition else {
            panic!("extrude");
        };
        *op = BooleanOp::Cut;
    });
    assert!(contradicted_features(&model).contains(&model.features[0].id));
}

#[test]
fn placed_body_caps_are_compared_in_model_space() {
    let mut model = model(30.0, 1.0);
    model.bodies[0].transform = Transform::affine([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 20.0],
    ]);
    assert!(contradicted_features(&model).contains(&model.features[0].id));
}

#[test]
fn suppressed_and_unresolved_suppression_preserve_travel() {
    for suppressed in [None, Some(true)] {
        let mut model = model(10.0, 1.0);
        model.features[0].suppressed = suppressed;
        assert!(contradicted_features(&model).is_empty());
    }
}

#[test]
fn active_state_retention_preserves_inactive_configuration_travel() {
    let mut model = model(10.0, 1.0);
    let feature = &model.features[0];
    for active in [true, false] {
        let mut configuration: cadmpeg_ir::features::DesignConfiguration =
            serde_json::from_value(json!({
                "id":format!("synthetic:travel:configuration#{active}"),"active":active
            }))
            .expect("synthetic configuration");
        configuration
            .feature_states
            .insert(feature.id.clone(), feature.configuration_state());
        model.configurations.push(configuration);
    }
    assert!(retain_contradicted_directions(&mut model));
    for configuration in &model.configurations {
        let state = &configuration.feature_states[&model.features[0].id];
        let FeatureOperation::Extrude { direction, .. } = state.definition.operation() else {
            panic!("extrude");
        };
        assert_eq!(
            matches!(direction, ExtrudeDirection::Unresolved {}),
            configuration.active
        );
    }
}

#[test]
fn bounded_planar_meshes_supply_finite_cap_witnesses() {
    use cadmpeg_ir::tessellation::{Tessellation, TessellationId, TessellationMesh};
    let mut model = model(10.0, 1.0);
    model.loops.clear();
    model.coedges.clear();
    model.edges.clear();
    model.vertices.clear();
    model.points.clear();
    model.curves.clear();
    for (i, face) in model.faces.iter().enumerate() {
        let surface = model
            .surfaces
            .iter()
            .find(|s| s.id == face.surface)
            .expect("surface");
        let Some(SolvedSurfaceGeometry::Plane(plane)) = surface.geometry.solved() else {
            continue;
        };
        if plane.frame().axis().as_raw().z.abs() != 1.0 {
            continue;
        }
        let z = plane.origin().get().z + EPS_CAP_MESH_DISTANCE / 2.0;
        let mut mesh = Tessellation::new(
            TessellationId::mint(format!("synthetic:travel:mesh#{i}")).expect("identity"),
            TessellationMesh::List {
                vertices: vec![
                    Point3::new(0.0, 0.0, z),
                    Point3::new(10.0, 0.0, z),
                    Point3::new(10.0, 10.0, z),
                    Point3::new(0.0, 10.0, z),
                ],
                triangles: vec![[0, 1, 2], [0, 2, 3]],
            },
            Vec::new(),
        )
        .expect("planar mesh");
        mesh.faces.push(face.id.clone());
        model.tessellations.push(mesh);
    }
    assert!(contradicted_features(&model).contains(&model.features[0].id));
    assert!(!contradicts(
        &model,
        &model.features[0],
        &caps(&model, &WorkBudget::new(MAX_WITNESS_WORK)).expect("finite caps"),
        &WorkBudget::new(0)
    ));
}

#[test]
fn mesh_work_counts_each_face_and_triangle_vertex() {
    use cadmpeg_ir::tessellation::{Tessellation, TessellationId, TessellationMesh};
    let mut model = model(10.0, 1.0);
    let mut mesh = Tessellation::new(
        TessellationId::mint("synthetic:travel:mesh#shared").expect("identity"),
        TessellationMesh::List {
            vertices: vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(10.0, 0.0, 0.0),
                Point3::new(0.0, 10.0, 0.0),
            ],
            triangles: (0..300).map(|_| [0, 1, 2]).collect(),
        },
        Vec::new(),
    )
    .expect("shared mesh");
    mesh.faces = model.faces.iter().map(|face| face.id.clone()).collect();
    model.tessellations.push(mesh);
    assert!(caps(&model, &WorkBudget::new(4000)).is_none());
}
