// SPDX-License-Identifier: Apache-2.0

use crate::decode::holes::placement::ExtrusionSpan;
use crate::decode::sweep::nurbs::extruded_nurbs_surface;
use crate::decode::sweep::profiles::{
    circular_pcurve, extrusion_cap_pcurve, extrusion_profile_signed_area, extrusion_side_uvs,
    ordered_extrusion_profiles, oriented_arc_parameterization, oriented_full_turn_angles,
    point_on_profile_arc, profile_arc, resolved_sketch_profiles, ProfileEntity,
};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::math::{Point2, Point3};
use cadmpeg_ir::scalar::{Angle, Length};
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry,
    SketchGeometryDefinition, SketchId,
};

#[test]
fn spline_extrusion_preserves_directrix_basis_and_weights() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("test decode context");
    let directrix = NurbsCurve::from_lanes(&cadmpeg_test_support::service_decode_context(), 
        2,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![
            Point3::new(1.0, 2.0, 3.0),
            Point3::new(4.0, 5.0, 6.0),
            Point3::new(7.0, 8.0, 9.0),
        ],
        Some(vec![1.0, 0.5, 1.0]),
        false,
    ).expect("fixture constructor admission")
    .expect("valid directrix");
    let surface = extruded_nurbs_surface(
        &ctx,
        &directrix,
        [0.0, 0.0, 4.0],
        &"extrusion directrix fixture",
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    .expect("extrusion resources")
    .expect("valid extrusion surface");

    assert_eq!((surface.u_degree(), surface.v_degree()), (2, 1));
    assert_eq!((surface.u_count(), surface.v_count()), (3, 2));
    assert_eq!(surface.u_knots(), directrix.knots());
    assert_eq!(surface.v_knots().as_slice(), [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(
        surface.poles(),
        [
            Point3::new(1.0, 2.0, 3.0),
            Point3::new(1.0, 2.0, 7.0),
            Point3::new(4.0, 5.0, 6.0),
            Point3::new(4.0, 5.0, 10.0),
            Point3::new(7.0, 8.0, 9.0),
            Point3::new(7.0, 8.0, 13.0),
        ]
    );
    assert_eq!(
        surface.pole_grid().weights().map(|rows| rows.concat()),
        Some([1.0, 1.0, 0.5, 0.5, 1.0, 1.0].to_vec())
    );
}

#[test]
fn reversed_arc_uses_opposite_axis_and_canonical_increasing_domain() {
    let (axis_sign, range) = oriented_arc_parameterization(
        true,
        -std::f64::consts::FRAC_PI_2,
        std::f64::consts::FRAC_PI_2,
    );

    assert_eq!(axis_sign, -1.0);
    assert_eq!(
        range,
        [
            3.0 * std::f64::consts::FRAC_PI_2,
            5.0 * std::f64::consts::FRAC_PI_2
        ]
    );
}

#[test]
fn extrusion_arc_pcurve_is_exact_in_both_directions() {
    for (start, end, expected_middle) in [
        (0.0, std::f64::consts::PI, Point2::new(2.0, 5.0)),
        (std::f64::consts::PI, 0.0, Point2::new(2.0, 5.0)),
    ] {
        let pcurve = crate::decode::with_test_decode_ctx(|ctx| {
            circular_pcurve(
                ctx,
                [2.0, 2.0],
                3.0,
                start,
                end,
                &"circular pcurve fixture",
                &mut crate::lane_refusal::LaneRefusals::new(),
            )
        })
        .expect("resource admission")
        .expect("circular pcurve fixture");
        let first = cadmpeg_ir::eval::decode::pcurve_uv(cadmpeg_ir::eval::admission::EvaluationAdmission::Standard, &pcurve, 0.0).expect("first endpoint");
        let middle = cadmpeg_ir::eval::decode::pcurve_uv(cadmpeg_ir::eval::admission::EvaluationAdmission::Standard, &pcurve, 0.5).expect("arc midpoint");
        let last = cadmpeg_ir::eval::decode::pcurve_uv(cadmpeg_ir::eval::admission::EvaluationAdmission::Standard, &pcurve, 1.0).expect("last endpoint");
        assert!((first.u - (2.0 + 3.0 * start.cos())).abs() < 1.0e-12);
        assert!((first.v - (2.0 + 3.0 * start.sin())).abs() < 1.0e-12);
        assert!((middle.u - expected_middle.u).abs() < 1.0e-12);
        assert!((middle.v - expected_middle.v).abs() < 1.0e-12);
        assert!((last.u - (2.0 + 3.0 * end.cos())).abs() < 1.0e-12);
        assert!((last.v - (2.0 + 3.0 * end.sin())).abs() < 1.0e-12);
    }
}

#[test]
fn extrusion_profile_area_includes_oriented_arc_sector() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root admitted");
    let arc = SketchGeometry::try_from(SketchGeometryDefinition::Arc {
        center: Point2::new(0.0, 0.0),
        radius: Length::new(1.0).expect("finite length fixture"),
        start_angle: Angle::new(0.0).expect("finite angle fixture"),
        end_angle: Angle::new(std::f64::consts::PI).expect("finite angle fixture"),
    })
    .expect("valid test fixture");
    let line = SketchGeometry::try_from(SketchGeometryDefinition::Line {
        start: Point2::new(-1.0, 0.0),
        end: Point2::new(1.0, 0.0),
    })
    .expect("valid test fixture");
    let counterclockwise = vec![
        ProfileEntity::new(&ctx, arc.clone(), false)
            .expect("service resources")
            .expect("valid profile entity"),
        ProfileEntity::new(&ctx, line.clone(), false)
            .expect("service resources")
            .expect("valid profile entity"),
    ];
    let clockwise = vec![
        ProfileEntity::new(&ctx, arc, true)
            .expect("service resources")
            .expect("valid profile entity"),
        ProfileEntity::new(&ctx, line, true)
            .expect("service resources")
            .expect("valid profile entity"),
    ];
    assert!(
        (extrusion_profile_signed_area(&ctx, &counterclockwise)
            .expect("service area resources")
            .expect("positive area")
            .get()
            - std::f64::consts::FRAC_PI_2)
            .abs()
            < 1.0e-12
    );
    assert!(
        (extrusion_profile_signed_area(&ctx, &clockwise)
            .expect("service area resources")
            .expect("negative area")
            .get()
            + std::f64::consts::FRAC_PI_2)
            .abs()
            < 1.0e-12
    );
}

#[test]
fn full_turn_arc_remains_a_closed_extrusion_profile() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root admitted");
    let profile = vec![ProfileEntity::new(
        &ctx,
        SketchGeometry::try_from(SketchGeometryDefinition::Arc {
            center: Point2::new(0.0, 0.0),
            radius: Length::new(2.0).expect("finite length fixture"),
            start_angle: Angle::new(0.0).expect("finite angle fixture"),
            end_angle: Angle::new(std::f64::consts::TAU).expect("finite angle fixture"),
        })
        .expect("valid test fixture"),
        false,
    )
    .expect("service resources")
    .expect("valid profile entity")];
    let profiles = ordered_extrusion_profiles(&ctx, vec![profile.clone()])
        .expect("service ordering resources")
        .expect("a full-turn arc is a closed profile");
    let area = profiles[0].area();
    assert_eq!(
        profiles
            .iter()
            .map(|profile| profile.entities().clone())
            .collect::<Vec<_>>(),
        vec![profile]
    );
    assert!((area - 4.0 * std::f64::consts::PI).abs() < 1.0e-12);
    assert_eq!(
        oriented_arc_parameterization(false, 0.0, std::f64::consts::TAU).1,
        [0.0, std::f64::consts::TAU]
    );
    assert_eq!(
        oriented_arc_parameterization(true, 0.0, std::f64::consts::TAU).1,
        [0.0, std::f64::consts::TAU]
    );
}

#[test]
fn circle_remains_a_closed_extrusion_profile() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root admitted");
    let sketch_id =
        SketchId::mint("creo:model:sketch#circle".to_string()).expect("valid test fixture");
    let entity_id = SketchEntityId::mint("creo:model:sketch_entity#circle".to_string())
        .expect("valid test fixture");
    let circle = SketchGeometry::try_from(SketchGeometryDefinition::Circle {
        center: Point2::new(1.0, -2.0),
        radius: Length::new(3.0).expect("finite length fixture"),
    })
    .expect("valid test fixture");
    let seam = [4.0, -2.0];
    let mut ir = CadIr::empty();
    ir.model.sketches.push(Sketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Unresolved {},
        profiles: cadmpeg_ir::sketches::SketchProfiles::try_from(vec![vec![SketchEntityUse {
            entity: entity_id.clone(),
            reversed: false,
        }]])
        .expect("valid test fixture"),
        native_ref: None,
    });
    ir.model.sketch_entities.push(SketchEntity::new(
        entity_id,
        sketch_id.clone(),
        circle.clone(),
    ));

    let profiles = crate::decode::with_test_decode_ctx(|ctx| {
        resolved_sketch_profiles(
            ctx,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            &sketch_id,
            1,
        )
    })
    .expect("resource admission")
    .expect("one circle profile");
    assert_eq!(
        profiles,
        vec![vec![ProfileEntity::new(&ctx, circle.clone(), false)
            .expect("service resources")
            .expect("valid profile entity")]]
    );
    let ordered = ordered_extrusion_profiles(&ctx, profiles.clone())
        .expect("service ordering resources")
        .expect("closed circle");
    let area = ordered[0].area();
    assert_eq!(
        ordered
            .iter()
            .map(|profile| profile.entities().clone())
            .collect::<Vec<_>>(),
        profiles
    );
    assert!((area - 9.0 * std::f64::consts::PI).abs() < 1.0e-12);

    for reversed in [false, true] {
        let pcurve = crate::decode::with_test_decode_ctx(|ctx| {
            extrusion_cap_pcurve(
                ctx,
                &circle,
                reversed,
                seam,
                seam,
                &"extrusion cap fixture",
                &mut crate::lane_refusal::LaneRefusals::new(),
            )
        })
        .expect("resource admission")
        .expect("extrusion cap pcurve fixture");
        let first = cadmpeg_ir::eval::decode::pcurve_uv(cadmpeg_ir::eval::admission::EvaluationAdmission::Standard, &pcurve, 0.0).expect("circle seam");
        let middle = cadmpeg_ir::eval::decode::pcurve_uv(cadmpeg_ir::eval::admission::EvaluationAdmission::Standard, &pcurve, 0.5).expect("circle midpoint");
        let last = cadmpeg_ir::eval::decode::pcurve_uv(cadmpeg_ir::eval::admission::EvaluationAdmission::Standard, &pcurve, 1.0).expect("circle seam");
        assert!((first.u - seam[0]).abs() < 1.0e-12);
        assert!((first.v - seam[1]).abs() < 1.0e-12);
        assert!((middle.u - (1.0 - 3.0)).abs() < 1.0e-12);
        assert!((middle.v + 2.0).abs() < 1.0e-12);
        assert!((last.u - seam[0]).abs() < 1.0e-12);
        assert!((last.v - seam[1]).abs() < 1.0e-12);
        assert_eq!(
            extrusion_side_uvs(
                &circle,
                reversed,
                seam,
                seam,
                ExtrusionSpan::new(-1.0, 2.0).expect("valid span fixture"),
            )[0],
            [
                [oriented_full_turn_angles(reversed)[0], -1.0],
                [oriented_full_turn_angles(reversed)[1], -1.0],
            ]
        );
        assert_eq!(
            profile_arc(
                &ProfileEntity::new(&ctx, circle.clone(), reversed)
                    .expect("service resources")
                    .expect("valid profile entity")
            ),
            Some((
                [1.0, -2.0],
                3.0,
                0.0,
                if reversed {
                    -std::f64::consts::TAU
                } else {
                    std::f64::consts::TAU
                },
            ))
        );
    }
    assert!(point_on_profile_arc(
        seam,
        profile_arc(
            &ProfileEntity::new(&ctx, circle, false)
                .expect("service resources")
                .expect("valid profile entity")
        )
        .expect("circle arc"),
        1.0e-9,
    ));
    assert_eq!(
        oriented_full_turn_angles(false),
        [0.0, std::f64::consts::TAU]
    );
    assert_eq!(
        oriented_full_turn_angles(true),
        [std::f64::consts::TAU, 0.0]
    );
}
