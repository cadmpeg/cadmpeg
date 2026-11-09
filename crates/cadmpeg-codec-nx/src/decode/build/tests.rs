// SPDX-License-Identifier: Apache-2.0
//! Geometry decode unknown-record admission.

use std::collections::BTreeSet;

use cadmpeg_ir::ids::BodyId;

use super::{
    body_preselection, has_plain_beside_partition_streams, plain_history_images,
    retain_live_annotations,
};
use crate::parasolid::StreamKind;

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

fn bodies(ids: &[&str]) -> Vec<BodyId> {
    ids.iter()
        .map(|id| BodyId::mint((*id).to_owned()).expect("body identity"))
        .collect()
}

fn stream_kinds(kinds: &[StreamKind]) -> impl Fn(usize) -> Option<StreamKind> + '_ {
    |index| kinds.get(index).copied()
}

#[test]
fn plain_images_are_withheld_only_beside_partition_bodies() {
    let kinds = [
        StreamKind::Partition,
        StreamKind::Deltas,
        StreamKind::Plain,
        StreamKind::Partition,
        StreamKind::Plain,
    ];
    let mixed = bodies(&[
        "nx:s0:body#3",
        "nx:s2:body#2",
        "nx:s3:body#2",
        "nx:s4:body#2",
    ]);
    let expected: BTreeSet<BodyId> = bodies(&["nx:s2:body#2", "nx:s4:body#2"])
        .into_iter()
        .collect();
    assert_eq!(
        plain_history_images(stream_kinds(&kinds), &mixed),
        Some(expected)
    );

    let plain_only = bodies(&["nx:s2:body#2", "nx:s4:body#2"]);
    assert_eq!(
        plain_history_images(stream_kinds(&kinds), &plain_only),
        None
    );

    let partitions_only = bodies(&["nx:s0:body#3", "nx:s3:body#2"]);
    assert_eq!(
        plain_history_images(stream_kinds(&kinds), &partitions_only),
        None
    );
}

#[test]
fn an_unattributed_body_withholds_no_plain_image() {
    let kinds = [StreamKind::Partition, StreamKind::Plain];
    let malformed = bodies(&["nx:s0:body#3", "nx:s1:body#2", "nx:model:body#9"]);
    assert_eq!(plain_history_images(stream_kinds(&kinds), &malformed), None);
    let unknown_stream = bodies(&["nx:s0:body#3", "nx:s1:body#2", "nx:s7:body#2"]);
    assert_eq!(
        plain_history_images(stream_kinds(&kinds), &unknown_stream),
        None
    );
}

#[test]
fn plain_images_beside_partitions_suppress_terminal_lineage_preselection() {
    assert!(has_plain_beside_partition_streams([
        StreamKind::Partition,
        StreamKind::Deltas,
        StreamKind::Plain,
    ]));
    assert!(!has_plain_beside_partition_streams([
        StreamKind::Plain,
        StreamKind::Plain
    ]));
    assert!(!has_plain_beside_partition_streams([
        StreamKind::Partition,
        StreamKind::Deltas
    ]));

    assert_eq!(body_preselection(None, Some("terminal"), true), None);
    assert_eq!(
        body_preselection(Some("membership"), Some("terminal"), true),
        Some("membership")
    );
    assert_eq!(
        body_preselection(None, Some("terminal"), false),
        Some("terminal")
    );
    assert_eq!(
        body_preselection(Some("membership"), Some("terminal"), false),
        Some("membership")
    );
}
