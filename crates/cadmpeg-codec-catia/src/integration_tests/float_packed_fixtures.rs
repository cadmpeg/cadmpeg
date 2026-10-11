use super::{assert_valid, decode};
use cadmpeg_ir::report::Severity;
use cadmpeg_test_support::wire;

const EPS_FIXTURE_POINT_MM: f64 = 1.0e-12;

fn assert_native_fixture(bytes: &[u8], expected_points: &[[f64; 3]]) {
    let result = decode(bytes.to_vec());
    let model = &result.ir().model;
    let count = expected_points.len();
    assert!(result.report().geometry_transferred());
    assert_eq!(model.bodies.len(), 1);
    assert_eq!(model.regions.len(), 1);
    assert_eq!(model.shells.len(), 1);
    assert_eq!(model.faces.len(), 1);
    assert_eq!(model.loops.len(), 1);
    assert_eq!(model.surfaces.len(), 1);
    assert_eq!(model.coedges.len(), count);
    assert_eq!(model.edges.len(), count);
    assert_eq!(model.vertices.len(), count);
    assert_eq!(model.pcurves.len(), count);
    assert_eq!(model.points.len(), count);
    for expected in expected_points {
        assert!(model.points.iter().any(|point| {
            let position = point.position().get();
            (position.x - expected[0])
                .hypot(position.y - expected[1])
                .hypot(position.z - expected[2])
                <= EPS_FIXTURE_POINT_MM
        }));
    }
    for (key, expected) in [
        (crate::coverage::DECODED_OBJECT_STREAM_RUN_COUNT, 1),
        (crate::coverage::SELECTED_OBJECT_STREAM_RUN_COUNT, 1),
        (crate::coverage::EXHAUSTED_OBJECT_STREAM_SELECTION_COUNT, 0),
        (crate::coverage::UNSELECTED_OBJECT_STREAM_RUN_COUNT, 0),
        (
            crate::coverage::TYPED_OBJECT_STREAM_EDGE_TERMINAL_CONTROL_2A_COUNT,
            count,
        ),
        (
            crate::coverage::TYPED_OBJECT_STREAM_PARAMETER_INCIDENCE_COUNT,
            count,
        ),
        (
            crate::coverage::TYPED_OBJECT_STREAM_PARAMETER_INCIDENCE_MEMBER_COUNT,
            2 * count,
        ),
        (
            crate::coverage::TYPED_OBJECT_STREAM_VERTEX_INCIDENCE_ROSTER_COUNT,
            count,
        ),
        (
            crate::coverage::TYPED_OBJECT_STREAM_VERTEX_INCIDENCE_ROSTER_MEMBER_COUNT,
            count,
        ),
        (
            crate::coverage::TYPED_OBJECT_STREAM_VERTEX_INCIDENCE_TERMINAL_CONTROL_04_COUNT,
            count,
        ),
        (
            crate::coverage::TYPED_UNRESOLVED_OBJECT_STREAM_FACE_COUNT,
            0,
        ),
        (
            crate::coverage::TYPED_UNRESOLVED_OBJECT_STREAM_LOOP_COUNT,
            0,
        ),
    ] {
        assert_eq!(
            wire::coverage_count(result.report(), key.as_str()),
            expected,
            "{}",
            key.as_str()
        );
    }
    let [loss] = result.report().losses.as_slice() else {
        panic!("the fixture has one topology gauge warning");
    };
    assert_eq!(
        loss.code,
        crate::loss::CatiaLossCode::TopologyB5GaugeSubstituted
            .note("fixture")
            .code
    );
    assert_eq!(loss.severity, Severity::Warning);
    assert_valid(&result);
}

#[test]
fn frozen_native_plane_fixture_transfers_its_complete_identity_chain() {
    assert_native_fixture(
        include_bytes!("../../tests/golden/fixtures/float_packed_native_plane.catpart"),
        &[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
    );
}

#[test]
fn frozen_native_cone_fixture_preserves_radius_and_radial_plane_angle() {
    assert_native_fixture(
        include_bytes!("../../tests/golden/fixtures/float_packed_native_cone.catpart"),
        &[
            [4.364_654_691_212_069, 0.0, 0.723_230_863_929_924],
            [6.229_309_382_424_138, 0.0, 1.446_461_727_859_848],
            [0.0, 6.229_309_382_424_138, 1.446_461_727_859_848],
            [0.0, 4.364_654_691_212_069, 0.723_230_863_929_924],
        ],
    );
}
