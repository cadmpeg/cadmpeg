// SPDX-License-Identifier: Apache-2.0
//! Geometry decode unknown-record admission.

use super::retain_live_annotations;

#[test]
fn live_annotation_lookup_scales_with_tree_depth() {
    let mut ir = cadmpeg_ir::CadIr::empty();
    let mut annotations = {
        let mut builder = cadmpeg_ir::annotations::AnnotationBuilder::new();
        for index in 0..4000 {
            let id = cadmpeg_ir::ids::PointId::mint(format!("nx:model:point#{index}"))
                .expect("identity");
            builder.exactness(&id, cadmpeg_ir::Exactness::Derived);
            ir.model.points.push(cadmpeg_ir::topology::Point::new(
                id,
                cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(0., 0., 0.))
                    .expect("position"),
                None,
            ));
        }
        builder.exactness("nx:model:point#absent", cadmpeg_ir::Exactness::Derived);
        builder.build()
    };
    ({
        retain_live_annotations(&ir, &[], &mut annotations);
    });
    assert_eq!(annotations.exactness().len(), 4000);
    assert!(!annotations
        .exactness()
        .contains_key("nx:model:point#absent"));
}

#[test]
fn annotation_identity_retention_walks_ordered_keys_once() {
    let names: Vec<_> = (0..10_000).map(|i| format!("id{i:05}")).collect();
    let ids: std::collections::BTreeSet<_> = names.iter().map(String::as_str).collect();
    let mut cursor = ids.iter().peekable();
    for id in &names {
        assert!(super::ordered_live_identity_contains(&mut cursor, id));
    }
    assert!(!super::ordered_live_identity_contains(
        &mut cursor,
        "id99999"
    ));
}

#[test]
fn annotation_identity_cursor_preserves_sorted_keep_decisions() {
    let ids: std::collections::BTreeSet<_> = ["b", "d", "f"].into_iter().collect();
    let mut cursor = ids.iter().peekable();
    for (id, expected) in [
        ("a", false),
        ("b", true),
        ("b", true),
        ("c", false),
        ("d", true),
        ("e", false),
        ("f", true),
        ("g", false),
    ] {
        assert_eq!(
            super::ordered_live_identity_contains(&mut cursor, id),
            expected,
            "{id}"
        );
    }
}

fn cone_record(xmt: u16, sine: f64, cosine: f64) -> Vec<u8> {
    use crate::test_support::test_bytes::{put_f64, put_ref, put_vec3, record};
    let mut bytes = record(0x34, 115);
    put_ref(&mut bytes, 2, xmt);
    bytes[18] = b'+';
    put_vec3(&mut bytes, 43, [0.0, 0.0, 1.0]);
    put_f64(&mut bytes, 67, 0.001);
    put_f64(&mut bytes, 75, sine);
    put_f64(&mut bytes, 83, cosine);
    put_vec3(&mut bytes, 91, [1.0, 0.0, 0.0]);
    bytes
}

fn torus_record(xmt: u16, major: f64) -> Vec<u8> {
    use crate::test_support::test_bytes::{put_f64, put_ref, put_vec3, record};
    let mut bytes = record(0x36, 107);
    put_ref(&mut bytes, 2, xmt);
    bytes[18] = b'+';
    put_vec3(&mut bytes, 43, [0.0, 0.0, 1.0]);
    put_f64(&mut bytes, 67, major);
    put_f64(&mut bytes, 75, 0.003);
    put_vec3(&mut bytes, 83, [1.0, 0.0, 0.0]);
    bytes
}

fn surface_refusal_codes(stream: &[u8]) -> Vec<cadmpeg_ir::report::loss::LossKind> {
    let graph = crate::topology::Graph::parse(stream);
    super::surface_refusals(0, &graph)
        .into_iter()
        .map(|loss| loss.code)
        .collect()
}

#[test]
fn a_negative_cone_half_angle_field_and_a_negative_torus_major_radius_each_report_a_loss() {
    let root_half = std::f64::consts::FRAC_1_SQRT_2;
    let mut stream = cone_record(2, root_half, root_half);
    stream.extend(cone_record(3, -root_half, root_half));
    stream.extend(torus_record(4, 0.002));
    stream.extend(torus_record(5, -0.002));
    assert_eq!(
        surface_refusal_codes(&stream),
        [
            crate::loss::NxLossCode::CarrierConeHalfAngleSignUnresolved.kind(),
            crate::loss::NxLossCode::CarrierTorusNegativeMajorUnresolved.kind(),
        ]
    );
}

#[test]
fn a_cone_pair_off_the_unit_identity_by_1e7_reports_a_loss() {
    let sine = 0.6_f64;
    let cosine = (1.0 + 1.0e-7 - sine * sine).sqrt();
    assert_eq!(
        surface_refusal_codes(&cone_record(2, sine, cosine)),
        [crate::loss::NxLossCode::CarrierConeHalfAngleIdentityRefused.kind()]
    );
}

#[test]
fn inactive_body_pruning_keeps_the_vertex_of_a_kept_vertex_loop() {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use cadmpeg_ir::ids::{BodyId, PointId, RegionId, ShellId, VertexId};
    use cadmpeg_ir::topology::{Body, BodyKind, Point, Region, Shell, Vertex};

    let stream = crate::test_support::test_streams::vertex_loop_topology_partition_stream(40);
    let file = crate::test_support::test_prt::prt_with_partition(&stream);
    let decoded = crate::NxCodec
        .decode(&mut std::io::Cursor::new(file), &DecodeOptions::default())
        .expect("decode");
    let mut ir = decoded.ir().clone();
    let kept = ir.model.bodies[0].id.clone();
    let (vertex, _) = ir
        .model
        .loops
        .iter()
        .find_map(|loop_| loop_.singular_vertex())
        .expect("vertex loop");
    let vertex = vertex.clone();
    assert!(ir
        .model
        .edges
        .iter()
        .all(|edge| edge.start != vertex && edge.end != vertex));

    let point = PointId::mint("nx:test:point#other").expect("identity");
    let other_vertex = VertexId::mint("nx:test:vertex#other").expect("identity");
    let shell = ShellId::mint("nx:test:shell#other").expect("identity");
    let region = RegionId::mint("nx:test:region#other").expect("identity");
    let body = BodyId::mint("nx:test:body#other").expect("identity");
    ir.model.points.push(Point::new(
        point.clone(),
        cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(5.0, 5.0, 5.0))
            .expect("position"),
        None,
    ));
    ir.model.vertices.push(Vertex {
        id: other_vertex.clone(),
        point,
        tolerance: None,
    });
    ir.model.shells.push(Shell::with_free_vertex(
        shell.clone(),
        region.clone(),
        other_vertex,
    ));
    ir.model.regions.push(Region {
        id: region.clone(),
        body: body.clone(),
        shells: vec![shell],
    });
    ir.model.bodies.push(Body {
        id: body,
        kind: BodyKind::General,
        regions: vec![region],
        transform: None,
        name: None,
        color: None,
        visible: None,
    });

    super::prune_inactive_topology(&mut ir, &std::collections::BTreeSet::from([kept]));
    let kept_vertex = ir
        .model
        .vertices
        .iter()
        .find(|candidate| candidate.id == vertex)
        .expect("vertex-loop vertex is kept");
    assert!(ir
        .model
        .points
        .iter()
        .any(|point| point.id == kept_vertex.point));
    let validation = cadmpeg_ir::validate::validate_neutral(&ir, Vec::new())
        .expect("validate the neutral model");
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn body_node_ids_require_the_vertex_loop_point_that_emission_requires() {
    let resolved = crate::test_support::test_streams::vertex_loop_topology_partition_stream(40);
    let graph = crate::topology::Graph::parse(&resolved);
    assert_eq!(
        super::topology_body_node_ids(0, &graph)
            .expect("body node ids")
            .len(),
        1
    );

    let mut unresolved = resolved;
    let apex = crate::test_support::test_bytes::record_offset(&unresolved, 18, 40);
    crate::test_support::test_bytes::put_ref(&mut unresolved, apex + 16, 99);
    let graph = crate::topology::Graph::parse(&unresolved);
    assert!(graph
        .get(crate::framing::node_kind::NodeKind::Vertex, 40)
        .is_some());
    assert!(super::topology_body_node_ids(0, &graph)
        .expect("body node ids")
        .is_empty());
}
