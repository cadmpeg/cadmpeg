// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_core::json;
use cadmpeg_core::serde_erase::json;
use cadmpeg_ir::document::Topology;
use cadmpeg_ir::math::Point3;

fn model(start: f64, normal: f64) -> CadIr {
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("synthetic cube");
    let history = json::from_value(json!({
        "id":"history", "features":[{
            "id":"native", "parent":"history", "ordinal":0,
            "name":"Extrude", "kind":"BossExtrude", "xml_tag":"Extrusion",
            "parameters":{"Depth":"10mm"},
            "properties":{"EndCondition":"Blind", "Operation":"Join"}
        }]
    }))
    .expect("synthetic history");
    ir.model.features = crate::history::project::project_features(&[history]).expect("feature");
    let sketch: Sketch = json::from_value(json!({
        "id":"synthetic:travel:sketch#0", "placement":{"kind":"resolved","frame":{
            "origin":{"x":0.0,"y":0.0,"z":start},
            "normal":{"x":0.0,"y":0.0,"z":normal},
            "u_axis":{"x":1.0,"y":0.0,"z":0.0}
        }},
        "profiles":[]
    }))
    .expect("synthetic frame");
    let corners = [(2.0, 2.0), (8.0, 2.0), (8.0, 8.0), (2.0, 8.0)];
    for (i, (a, b)) in corners
        .iter()
        .zip(corners.iter().cycle().skip(1))
        .take(4)
        .enumerate()
    {
        ir.model.sketch_entities.push(
            json::from_value(json!({
                "id":format!("synthetic:travel:entity#{i}"),"sketch":sketch.id,
                "construction":false,"endpoint_refs":[],
                "geometry":{"kind":"line","start":{"u":a.0,"v":a.1*normal},
                "end":{"u":b.0,"v":b.1*normal}}
            }))
            .expect("synthetic profile line"),
        );
    }
    ir.model.features[0].evaluation.edit(|definition, _| {
        let FeatureDefinition::Operation(FeatureOperation::Extrude { profile, .. }) = definition
        else {
            panic!("extrude");
        };
        *profile = ProfileRef::Planar(PlanarProfileRef::Sketch(sketch.id.clone()));
    });
    ir.model.sketches.push(sketch);
    ir
}

fn remove_cap(geometry: &mut Topology, z: f64) {
    let ids: HashSet<_> = geometry
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
    geometry.faces.retain(|face| !ids.contains(&face.surface));
}

fn set_direction(ir: &mut CadIr, travel: ExtrudeDirection) {
    ir.model.features[0].evaluation.edit(|definition, _| {
        if let FeatureDefinition::Operation(FeatureOperation::Extrude { direction, .. }) =
            definition
        {
            *direction = travel;
        }
    });
}

#[test]
fn opposite_caps_contradict_implicit_travel_with_mirrored_controls() {
    for (start, normal, contradicted) in [
        (10.0, 1.0, true),
        (10.0, -1.0, false),
        (0.0, 1.0, false),
        (0.0, -1.0, true),
    ] {
        let mut ir = model(start, normal);
        let original = ir.model.features[0].evaluation.definition().clone();
        assert_eq!(
            contradicted_features(&ir).contains(&ir.model.features[0].id),
            contradicted
        );
        assert_eq!(
            retain_contradicted_directions(&mut ir).changed,
            contradicted
        );
        let FeatureOperation::Extrude { direction, .. } =
            ir.model.features[0].evaluation.definition().operation()
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
            assert_eq!(ir.model.features[0].evaluation.definition(), &expected);
            assert!(contradicted_features(&ir).contains(&ir.model.features[0].id));
        } else {
            assert_eq!(ir.model.features[0].evaluation.definition(), &original);
        }
    }
}

/// A reversal that the caps contradict travels along the profile normal.
#[test]
fn opposite_caps_contradict_reversed_travel() {
    for (start, normal, contradicted) in [
        (10.0, 1.0, false),
        (10.0, -1.0, true),
        (0.0, 1.0, true),
        (0.0, -1.0, false),
    ] {
        let mut ir = model(start, normal);
        set_direction(&mut ir, ExtrudeDirection::ReversedProfileNormal {});
        assert_eq!(
            contradicted_features(&ir).contains(&ir.model.features[0].id),
            contradicted
        );
        let label = format!("\"Extrude\" ({})", ir.model.features[0].id.as_str());
        let retained = retain_contradicted_directions(&mut ir);
        assert_eq!(
            retained,
            Retained {
                changed: contradicted,
                reversals: if contradicted {
                    vec![label]
                } else {
                    Vec::new()
                },
                assumed: Vec::new(),
                ..Retained::default()
            }
        );
        assert_eq!(
            reversal_unread(&ir.model.features[0].source_properties),
            contradicted
        );
        let FeatureOperation::Extrude { direction, .. } =
            ir.model.features[0].evaluation.definition().operation()
        else {
            panic!("extrude");
        };
        let expected = if contradicted {
            ExtrudeDirection::ProfileNormal {}
        } else {
            ExtrudeDirection::ReversedProfileNormal {}
        };
        assert_eq!(direction, &expected);
        assert!(contradicted_features(&ir).is_empty());
    }
}

#[test]
fn absent_caps_and_carrier_failures_preserve_travel() {
    for missing in [0.0, 10.0] {
        let mut ir = model(10.0, 1.0);
        remove_cap(&mut ir.geometry, missing);
        assert!(contradicted_features(&ir).is_empty());
        assert!(!retain_contradicted_directions(&mut ir).changed);
    }
    let mut ir = model(10.0, 1.0);
    ir.model.sketches[0].placement = cadmpeg_ir::sketches::SketchPlacement::try_resolved(
        Point3::new(20.0, 0.0, 10.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
    )
    .expect("translated profile");
    assert!(contradicted_features(&ir).is_empty());
}

#[test]
fn through_all_and_explicit_travel_preserve_their_definitions() {
    for through_all in [false, true] {
        let mut ir = model(10.0, 1.0);
        ir.model.features[0].evaluation.edit(|definition, _| {
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
                    vector: cadmpeg_ir::features::geometry::FeatureDirection3::new(Vector3::new(
                        0.0, 0.0, 1.0,
                    ))
                    .expect("unit vector"),
                    source: None,
                };
            }
        });
        assert!(contradicted_features(&ir).is_empty());
    }
}

#[test]
fn decoded_side_cap_prevents_a_contradiction() {
    let ir = model(10.0, 1.0);
    let work = WorkBudget::new(MAX_WITNESS_WORK);
    let mut witness = evidence(&ir, &work, true).expect("finite caps");
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
    witness.caps.push(decoded);
    assert_ne!(
        verdict(&ir.model, &ir.model.features[0], &witness, &work),
        Some(false)
    );
}

#[test]
fn cut_witness_uses_the_cavity_face_sense() {
    let mut ir = model(10.0, 1.0);
    for face in &mut ir.geometry.faces {
        face.sense = Sense::Reversed;
    }
    ir.model.features[0].evaluation.edit(|definition, _| {
        let FeatureDefinition::Operation(FeatureOperation::Extrude { op, .. }) = definition else {
            panic!("extrude");
        };
        *op = BooleanOp::Cut;
    });
    assert!(contradicted_features(&ir).contains(&ir.model.features[0].id));
}

#[test]
fn placed_body_caps_are_compared_in_model_space() {
    let mut ir = model(30.0, 1.0);
    ir.geometry.bodies[0].transform = Transform::affine([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 20.0],
    ]);
    assert!(contradicted_features(&ir).contains(&ir.model.features[0].id));
}

#[test]
fn suppressed_and_unresolved_suppression_preserve_travel() {
    for suppressed in [None, Some(true)] {
        let mut ir = model(10.0, 1.0);
        ir.model.features[0].suppressed = suppressed;
        assert!(contradicted_features(&ir).is_empty());
    }
}

#[test]
fn active_state_retention_preserves_inactive_configuration_travel() {
    let mut ir = model(10.0, 1.0);
    let feature = &ir.model.features[0];
    for active in [true, false] {
        let mut configuration: cadmpeg_ir::features::parameters::DesignConfiguration =
            json::from_value(json!({
                "id":format!("synthetic:travel:configuration#{active}"),"active":active,
                "ordinal":0,"feature_states":{},"parameter_overrides":{},"parameter_values":{},
                "properties":{}
            }))
            .expect("synthetic configuration");
        configuration
            .feature_states
            .insert(feature.id.clone(), feature.configuration_state());
        ir.model.configurations.push(configuration);
    }
    assert!(retain_contradicted_directions(&mut ir).changed);
    for configuration in &ir.model.configurations {
        let state = &configuration.feature_states[&ir.model.features[0].id];
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
    let mut ir = model(10.0, 1.0);
    ir.geometry.loops.clear();
    ir.geometry.coedges.clear();
    ir.geometry.edges.clear();
    ir.geometry.vertices.clear();
    ir.geometry.points.clear();
    ir.geometry.curves.clear();
    for (i, face) in ir.geometry.faces.iter().enumerate() {
        let surface = ir
            .geometry
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
        mesh.of = cadmpeg_ir::tessellation::Meshed::Body {
            body: ir.geometry.bodies[0].id.clone(),
            faces: vec![face.id.clone()],
        };
        ir.model.tessellations.push(mesh);
    }
    assert!(contradicted_features(&ir).contains(&ir.model.features[0].id));
    assert_ne!(
        verdict(
            &ir.model,
            &ir.model.features[0],
            &evidence(&ir, &WorkBudget::new(MAX_WITNESS_WORK), true).expect("finite caps"),
            &WorkBudget::new(0)
        ),
        Some(false)
    );
}

#[test]
fn mesh_work_counts_each_face_and_triangle_vertex() {
    use cadmpeg_ir::tessellation::{Tessellation, TessellationId, TessellationMesh};
    let mut ir = model(10.0, 1.0);
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
    mesh.of = cadmpeg_ir::tessellation::Meshed::Body {
        body: ir.geometry.bodies[0].id.clone(),
        faces: ir
            .geometry
            .faces
            .iter()
            .map(|face| face.id.clone())
            .collect(),
    };
    ir.model.tessellations.push(mesh);
    assert!(evidence(&ir, &WorkBudget::new(4000), true).is_none());
}

/// A reversal whose whole solid lies on the profile normal's side of the
/// profile plane travels along the normal, though no start cap holds the
/// profile (a later feature removed a corner of it).
#[test]
fn a_solid_along_the_normal_contradicts_a_reversal_without_its_start_cap() {
    let mut ir = model(0.0, 1.0);
    set_direction(&mut ir, ExtrudeDirection::ReversedProfileNormal {});
    // The profile's corners at x = 13 lie off the start cap.
    ir.model.sketches[0].placement = cadmpeg_ir::sketches::SketchPlacement::try_resolved(
        Point3::new(5.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
    )
    .expect("translated profile");
    let work = WorkBudget::new(MAX_WITNESS_WORK);
    let caps_only = Witness {
        caps: evidence(&ir, &work, true).expect("finite caps").caps,
        solid: Solid::default(),
    };
    assert_ne!(
        verdict(&ir.model, &ir.model.features[0], &caps_only, &work),
        Some(false)
    );
    assert!(contradicted_features(&ir).contains(&ir.model.features[0].id));
    let retained = retain_contradicted_directions(&mut ir);
    assert_eq!(retained.reversals.len(), 1);
    let FeatureOperation::Extrude { direction, .. } =
        ir.model.features[0].evaluation.definition().operation()
    else {
        panic!("extrude");
    };
    assert_eq!(direction, &ExtrudeDirection::ProfileNormal {});
    assert!(reversal_unread(&ir.model.features[0].source_properties));
    // The whole solid contradicts a reversal of any termination.
    let mut ir = model(0.0, 1.0);
    set_direction(&mut ir, ExtrudeDirection::ReversedProfileNormal {});
    ir.model.features[0].evaluation.edit(|definition, _| {
        if let FeatureDefinition::Operation(FeatureOperation::Extrude {
            extent: ExtrudeExtent::OneSided { side },
            ..
        }) = definition
        {
            side.termination = LinearTermination::ThroughAll {};
        }
    });
    assert!(contradicted_features(&ir).contains(&ir.model.features[0].id));
    // A solid against the normal keeps the reversal.
    let mut ir = model(10.0, 1.0);
    set_direction(&mut ir, ExtrudeDirection::ReversedProfileNormal {});
    ir.model.sketches[0].placement = cadmpeg_ir::sketches::SketchPlacement::try_resolved(
        Point3::new(5.0, 0.0, 10.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
    )
    .expect("translated profile");
    assert!(contradicted_features(&ir).is_empty());
}

/// A circle reaches its radius across the direction; an edge of unknown
/// extent leaves the span unknown.
#[test]
fn the_solid_span_bounds_circles_and_refuses_unknown_edges() {
    let mut solid = Solid {
        points: vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 0.0, 10.0)],
        circles: vec![(Point3::new(0.0, 0.0, 1.0), Vector3::new(1.0, 0.0, 0.0), 2.0)],
        unknown: false,
    };
    let z = Vector3::new(0.0, 0.0, 1.0);
    assert_eq!(
        solid.span(Point3::new(0.0, 0.0, 0.0), z),
        Some((-1.0, 10.0))
    );
    solid.unknown = true;
    assert_eq!(solid.span(Point3::new(0.0, 0.0, 0.0), z), None);
}

/// A two-direction join over a reversal flag the projection left unread
/// travels against the normal only where its end caps confirm it.
#[test]
fn end_caps_confirm_a_two_direction_reversal() {
    for (start, first, second, confirmed) in [
        (4.0, 4.0, 6.0, true),
        (4.0, 6.0, 4.0, false),
        (5.0, 5.0, 5.0, false),
    ] {
        let mut ir = model(start, 1.0);
        let feature = &mut ir.model.features[0];
        feature.evaluation.edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::Extrude { extent, .. }) = definition
            else {
                panic!("extrude");
            };
            let side = |length: f64| cadmpeg_ir::features::extents::ExtrudeSide {
                termination: LinearTermination::Blind {
                    length: cadmpeg_ir::scalar::NonZeroLength::new(length).expect("a depth"),
                },
                draft: None,
            };
            *extent = ExtrudeExtent::TwoSided {
                first: side(first),
                second: side(second),
            };
        });
        feature.source_properties.insert(
            cadmpeg_core::nonblank_literal!("Reverse"),
            "true".to_owned(),
        );
        mark_unread_reversal(feature);
        assert!(reversal_unread(&feature.source_properties));
        let retained = retain_contradicted_directions(&mut ir);
        assert_eq!(
            retained,
            Retained {
                changed: confirmed,
                reversals: Vec::new(),
                assumed: Vec::new(),
                ..Retained::default()
            },
            "{first} {second}"
        );
        let feature = &ir.model.features[0];
        let FeatureOperation::Extrude { direction, .. } =
            feature.evaluation.definition().operation()
        else {
            panic!("extrude");
        };
        let expected = if confirmed {
            ExtrudeDirection::ReversedProfileNormal {}
        } else {
            ExtrudeDirection::ProfileNormal {}
        };
        assert_eq!(direction, &expected);
        assert_eq!(reversal_unread(&feature.source_properties), !confirmed);
    }
}

/// A reversal projected before its Boolean operation binds keeps its travel
/// once the operation adds material, and travels along the normal over an
/// unread flag otherwise; a settled extrusion is never reversed again.
#[test]
fn a_reversal_settles_with_the_bound_operation() {
    for (op, reversed) in [
        (BooleanOp::Join, true),
        (BooleanOp::NewBody, true),
        (BooleanOp::Cut, false),
        (BooleanOp::Unresolved, false),
    ] {
        let mut ir = model(0.0, -1.0);
        set_direction(&mut ir, ExtrudeDirection::ReversedProfileNormal {});
        let feature = &mut ir.model.features[0];
        feature.source_properties.insert(
            cadmpeg_core::nonblank_literal!("Reverse"),
            "true".to_owned(),
        );
        feature.evaluation.edit(|definition, _| {
            if let FeatureDefinition::Operation(FeatureOperation::Extrude { op: bound, .. }) =
                definition
            {
                *bound = op;
            }
        });
        for _ in 0..2 {
            settle_reversal(feature);
            let FeatureOperation::Extrude { direction, .. } =
                feature.evaluation.definition().operation()
            else {
                panic!("extrude");
            };
            let expected = if reversed {
                ExtrudeDirection::ReversedProfileNormal {}
            } else {
                ExtrudeDirection::ProfileNormal {}
            };
            assert_eq!(direction, &expected, "{op:?}");
            assert_eq!(reversal_unread(&feature.source_properties), !reversed);
        }
    }
}
