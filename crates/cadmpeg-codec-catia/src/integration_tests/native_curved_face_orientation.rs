// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::topology::Sense;

use super::{assert_valid, decode};

#[test]
fn native_cylinder_faces_follow_outer_control_in_the_preserved_chart() {
    let positive = decode(
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/float_packed_native_cylinder_positive.catpart"
        ))
        .to_vec(),
    );
    let negative = decode(
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/float_packed_native_cylinder_negative.catpart"
        ))
        .to_vec(),
    );
    for (result, expected) in [(&positive, Sense::Forward), (&negative, Sense::Reversed)] {
        assert_valid(result);
        assert_eq!(result.ir().model.faces.len(), 1);
        assert_eq!(result.ir().model.faces[0].sense, expected);
        assert!(matches!(
            result.ir().model.surfaces[0].geometry,
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(_))
        ));
    }
    assert_eq!(positive.ir().model.surfaces, negative.ir().model.surfaces);
}

#[test]
fn native_nurbs_faces_follow_outer_control_without_transposing_the_carrier() {
    let positive = decode(
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/float_packed_native_nurbs_positive.catpart"
        ))
        .to_vec(),
    );
    let negative = decode(
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/float_packed_native_nurbs_negative.catpart"
        ))
        .to_vec(),
    );
    for (result, expected) in [(&positive, Sense::Forward), (&negative, Sense::Reversed)] {
        assert_valid(result);
        assert_eq!(result.ir().model.faces.len(), 1);
        assert_eq!(result.ir().model.faces[0].sense, expected);
        let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface)) =
            &result.ir().model.surfaces[0].geometry
        else {
            panic!("direct native NURBS carrier");
        };
        assert!(!surface.normal_reversed());
        assert_eq!(surface.u_degree(), 2);
        assert_eq!(surface.v_degree(), 2);
    }
    assert_eq!(positive.ir().model.surfaces, negative.ir().model.surfaces);
}

#[test]
fn native_revolution_line_faces_follow_outer_control_in_profile_angle_order() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;
    let positive = decode(
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/float_packed_native_revolution_line_positive.catpart"
        ))
        .to_vec(),
    );
    let negative = decode(
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/float_packed_native_revolution_line_negative.catpart"
        ))
        .to_vec(),
    );
    for (result, expected) in [(&positive, Sense::Forward), (&negative, Sense::Reversed)] {
        assert_valid(result);
        let model = &result.ir().model;
        assert_eq!(model.faces.len(), 1);
        assert_eq!(model.edges.len(), 4);
        assert_eq!(model.loops.len(), 1);
        assert_eq!(model.vertices.len(), 4);
        assert_eq!(model.faces[0].sense, expected);
        assert_eq!(model.procedural_surfaces.len(), 1);
        assert!(matches!(
            model.procedural_surfaces[0].definition(),
            ProceduralSurfaceDefinition::Revolution(_)
        ));
        let SurfaceGeometry::Procedural {
            cache: Some(SolvedSurfaceGeometry::Nurbs(surface)),
            ..
        } = &model.surfaces[0].geometry
        else {
            panic!("revolution cache");
        };
        assert_eq!(surface.u_degree(), 1);
        assert_eq!(surface.v_degree(), 2);
        assert!(!surface.normal_reversed());
    }
    assert_eq!(positive.ir().model.surfaces, negative.ir().model.surfaces);
    assert_eq!(
        positive.ir().model.procedural_surfaces,
        negative.ir().model.procedural_surfaces
    );
}

#[test]
fn native_revolution_arc_source_abstentions_remain_native() {
    for bytes in [
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/float_packed_native_revolution_arc_positive.catpart"
        ))
        .as_slice(),
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/float_packed_native_revolution_arc_negative.catpart"
        ))
        .as_slice(),
    ] {
        let result = decode(bytes.to_vec());
        assert_valid(&result);
        assert!(result.ir().model.faces.is_empty());
        assert!(result.ir().model.surfaces.is_empty());
        assert!(result
            .source_fidelity()
            .retained_records()
            .values()
            .any(|record| record.data().is_some()));
    }
}

#[test]
fn native_positive_torus_face_keeps_major_minor_chart_order() {
    let result = decode(
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/float_packed_native_torus_positive.catpart"
        ))
        .to_vec(),
    );
    assert_valid(&result);
    let model = &result.ir().model;
    assert_eq!(model.faces.len(), 1);
    assert_eq!(model.faces[0].sense, Sense::Forward);
    assert_eq!(model.edges.len(), 4);
    assert_eq!(model.coedges.len(), 4);
    assert_eq!(model.loops.len(), 1);
    assert_eq!(model.vertices.len(), 4);
    assert!(matches!(
        model.surfaces[0].geometry,
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(_))
    ));
}

#[test]
fn native_negative_torus_face_keeps_carrier_and_reverses_sense() {
    let positive = decode(
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/float_packed_native_torus_positive.catpart"
        ))
        .to_vec(),
    );
    let negative = decode(
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/float_packed_native_torus_negative.catpart"
        ))
        .to_vec(),
    );
    assert_valid(&positive);
    assert_valid(&negative);
    assert_eq!(negative.ir().model.faces.len(), 1);
    assert_eq!(negative.ir().model.faces[0].sense, Sense::Reversed);
    assert_eq!(positive.ir().model.surfaces, negative.ir().model.surfaces);
    assert_eq!(
        positive.ir().model.faces[0].surface,
        negative.ir().model.faces[0].surface
    );
}

#[test]
fn native_cone_source_controls_preserve_positive_chart_sense() {
    let positive = decode(
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/float_packed_native_cone_hplus_positive.catpart"
        ))
        .to_vec(),
    );
    let negative = decode(
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/float_packed_native_cone_hplus_negative.catpart"
        ))
        .to_vec(),
    );
    for (result, sense) in [(&positive, Sense::Forward), (&negative, Sense::Reversed)] {
        assert_valid(result);
        assert_eq!(result.ir().model.faces.len(), 1);
        assert_eq!(result.ir().model.faces[0].sense, sense);
        assert!(matches!(
            result.ir().model.surfaces[0].geometry,
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(_))
        ));
    }
    assert_eq!(positive.ir().model.surfaces, negative.ir().model.surfaces);
}

#[test]
fn native_cone_source_controls_reverse_negative_chart_sense() {
    let positive = decode(
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/float_packed_native_cone_hminus_positive.catpart"
        ))
        .to_vec(),
    );
    let negative = decode(
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/float_packed_native_cone_hminus_negative.catpart"
        ))
        .to_vec(),
    );
    for (result, sense) in [(&positive, Sense::Reversed), (&negative, Sense::Forward)] {
        assert_valid(result);
        assert_eq!(result.ir().model.faces.len(), 1);
        assert_eq!(result.ir().model.faces[0].sense, sense);
        assert!(matches!(
            result.ir().model.surfaces[0].geometry,
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(_))
        ));
    }
    assert_eq!(positive.ir().model.surfaces, negative.ir().model.surfaces);
}
