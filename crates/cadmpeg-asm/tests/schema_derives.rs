// SPDX-License-Identifier: Apache-2.0
//! JSON Schema checks for `cadmpeg-asm` public types.

use std::collections::BTreeSet;

use cadmpeg_asm::brep::records::WireMembers;
use cadmpeg_asm::brep::AsmBrep;
use cadmpeg_core::serde_erase::json;
use cadmpeg_ir::ids::{EdgeId, VertexId};

#[test]
fn every_asm_brep_schema_position_has_one_spelling() {
    let empty = json::to_value(AsmBrep::default()).expect("empty B-rep serializes");
    let mut populated = AsmBrep::default();
    for map in [
        &mut populated.stats.missing_face_surface_kinds,
        &mut populated.stats.unknown_surface_kinds,
        &mut populated.stats.procedural_curve_kinds,
        &mut populated.stats.undecoded_pcurve_kinds,
        &mut populated.stats.other_record_kinds,
    ] {
        map.insert("synthetic".to_owned(), 1);
    }
    let populated = json::to_value(populated).expect("B-rep serializes");
    let mut native = empty.clone();
    let id = "f3d:brep:entity#1";
    for (field, kind, body) in [
        (
            "edge_continuities",
            "edge-continuity",
            cadmpeg_core::json!({"edge": id, "sense": "forward", "continuity": "tangent"}),
        ),
        (
            "edge_ownerships",
            "edge-ownership",
            cadmpeg_core::json!({"edge": id, "owner_coedge": id}),
        ),
        (
            "vertex_ownerships",
            "vertex-ownership",
            cadmpeg_core::json!({"vertex": id, "owning_edge": id, "endpoint_index": 0}),
        ),
        (
            "face_sidedness",
            "face-sidedness",
            cadmpeg_core::json!({"face": id, "native_sense": "forward", "normalized_sense": "forward"}),
        ),
        (
            "face_native_keys",
            "face-native-key",
            cadmpeg_core::json!({"face": id, "asm_face_key": 1}),
        ),
        (
            "tolerant_vertex_tails",
            "tolerant-vertex-tail",
            cadmpeg_core::json!({"vertex": id, "leading_tolerances": [-1.0, 0.03], "evaluated_slot": {"slot": "unset", "trailing": 0}}),
        ),
        (
            "tolerant_edge_tails",
            "tolerant-edge-tail",
            cadmpeg_core::json!({"edge": id, "entity_revision": 1, "trailing_field": 1}),
        ),
        (
            "tolerant_coedge_parameters",
            "tolerant-coedge-parameters",
            cadmpeg_core::json!({"coedge": id, "parameter_range": [0.0, 1.0], "extension": {"layout": "none"}}),
        ),
        (
            "mesh_surface_sentinels",
            "mesh-surface-sentinel",
            cadmpeg_core::json!({"surface": id}),
        ),
        (
            "wire_topologies",
            "wire-topology",
            cadmpeg_core::json!({"shell": id, "edges": [id], "side": "in"}),
        ),
        (
            "body_native_keys",
            "body-native-key",
            cadmpeg_core::json!({"body": id, "body_ordinal": 0, "source_brep": "source", "asm_body_key": 1}),
        ),
        (
            "transform_hints",
            "transform-hints",
            cadmpeg_core::json!({"body": id, "rotation": false, "reflection": false, "shear": false}),
        ),
    ] {
        let mut body = body;
        body["id"] = format!("f3d:asm:{kind}#1").into();
        body["record_index"] = 1.into();
        native[field] = cadmpeg_core::json!([body]);
    }
    let mut geometry = empty.clone();
    let metadata = cadmpeg_core::json!({"form": {"fit_tolerance": 0.01}, "kind": "general"});
    let mut pcurves = Vec::new();
    for poles in [
        cadmpeg_core::json!({"kind": "polynomial", "points": [{"u": 1.0, "v": 0.0}, {"u": 1.0, "v": 1.0}]}),
        cadmpeg_core::json!({"kind": "rational", "points": [{"point": {"u": 1.0, "v": 0.0}, "weight": 1.0}, {"point": {"u": 1.0, "v": 1.0}, "weight": 1.0}]}),
    ] {
        pcurves.push(cadmpeg_core::json!({"id": id, "metadata": metadata, "geometry": {"kind": "nurbs", "degree": 1, "knots": [0.0, 0.0, 1.0, 1.0], "periodic": false, "poles": poles}}));
    }
    for poles in [
        cadmpeg_core::json!({"kind": "polynomial", "poles": [{"axial": 0.0, "radial": {"u": 1.0, "v": 0.0}}, {"axial": 1.0, "radial": {"u": 1.0, "v": 1.0}}]}),
        cadmpeg_core::json!({"kind": "rational", "poles": [{"axial": 0.0, "radial": {"u": 1.0, "v": 0.0}, "weight": 1.0}, {"axial": 1.0, "radial": {"u": 1.0, "v": 1.0}, "weight": 1.0}]}),
    ] {
        pcurves.push(cadmpeg_core::json!({"id": id, "metadata": metadata, "geometry": {"kind": "polar_nurbs", "degree": 1, "knots": [0.0, 0.0, 1.0, 1.0], "periodic": false, "poles": poles}}));
    }
    geometry["pcurves"] = pcurves.into();
    use cadmpeg_ir::geometry::blends::{
        RollingBallJetDerivative, RollingBallJetSite, RollingBallJetStation, RollingBallJetStations,
    };
    use cadmpeg_ir::geometry::procedural_surfaces::ProceduralSurfaceDefinition;
    use cadmpeg_ir::geometry::ProceduralSurface;
    use cadmpeg_ir::ids::ProceduralSurfaceId;
    use cadmpeg_ir::math::{Point3, Vector3};
    let station = |knot| {
        let derivative = RollingBallJetDerivative {
            first_limit: Vector3::new(0.0, 0.0, 0.0),
            second_limit: Vector3::new(0.0, 0.0, 0.0),
            center: Vector3::new(0.0, 0.0, 0.0),
            angle: 0.0,
        };
        RollingBallJetStation {
            knot,
            multiplicity: 6,
            site: RollingBallJetSite {
                first_limit: Point3::new(1.0, 0.0, knot),
                second_limit: Point3::new(0.0, 1.0, knot),
                center: Point3::new(0.0, 0.0, knot),
                angle: std::f64::consts::FRAC_PI_2,
                first_derivative: derivative,
                second_derivative: derivative,
            },
        }
    };
    let jet =
        RollingBallJetStations::try_new(5, vec![station(2.0), station(8.0)]).expect("valid jet");
    let surface = ProceduralSurface::new(
        ProceduralSurfaceId::mint("f3d:surface:construction#1").expect("surface id"),
        ProceduralSurfaceDefinition::RollingBallJet(jet),
        None,
    );
    geometry["procedural_surfaces"] = cadmpeg_core::json!([[id, surface]]);
    let schema = json::to_value(schemars::schema_for!(AsmBrep)).expect("B-rep schema");
    let read = |text: &str| json::to_value(serde_json::from_str::<AsmBrep>(text)?);
    let report = cadmpeg_test_support::schema_sweep::sweep(
        &schema,
        &[empty.clone(), populated, native, geometry],
        &empty,
        &read,
    );
    assert!(report.unreached.is_empty(), "{:?}", report.unreached);
    assert!(report.unprobed.is_empty(), "{:?}", report.unprobed);
    assert!(report.accepting.is_empty(), "{:?}", report.accepting);
}

/// `WireMembers` writes a map keyed by `edges` or `free_vertex`, and the
/// `JsonSchema` derive reads `#[serde(try_from = "WireMembersWire")]`, so
/// the published schema states those two properties rather than the
/// neutral enum's `Edges` and `Vertex` arms.
#[test]
fn the_wire_member_schema_states_the_keys_the_writer_states() {
    let schema = json::to_value(schemars::schema_for!(WireMembers)).expect("members schema");
    let properties: BTreeSet<String> = schema["properties"]
        .as_object()
        .expect("the members schema states properties")
        .keys()
        .cloned()
        .collect();
    let edge = EdgeId::mint("asm:test:edge#1").expect("edge id");
    let vertex = VertexId::mint("asm:test:vertex#2").expect("vertex id");
    let written: BTreeSet<String> = [WireMembers::Edges(vec![edge]), WireMembers::Vertex(vertex)]
        .iter()
        .flat_map(|members| {
            json::to_value(members)
                .expect("serialize members")
                .as_object()
                .expect("members write a map")
                .keys()
                .cloned()
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(properties, written);
}
