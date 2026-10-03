use crate::features::PlanarProfileRef;
use crate::features::{
    ProfileRef, SketchProfileBoundaryUse, SketchProfileLoops, SketchProfileRegion,
    SketchProfileRegions,
};
use crate::geometry::DirectedParameterRange;
use crate::sketches::{SketchEntityId, SketchId};

#[test]
fn boundary_parameter_ranges_reject_invalid_values_and_preserve_direction() {
    for endpoints in [
        [0.0, 0.0],
        [f64::NAN, 1.0],
        [0.0, f64::INFINITY],
        [f64::NEG_INFINITY, 0.0],
    ] {
        assert!(DirectedParameterRange::new(endpoints).is_err());
    }
    for endpoints in [[0.0, 1.0], [5.0, 2.0]] {
        for reversed in [false, true] {
            let boundary = SketchProfileBoundaryUse {
                entity: SketchEntityId::mint("test:test:sketch-entity#one").unwrap(),
                parameter_range: DirectedParameterRange::new(endpoints).unwrap(),
                reversed,
            };
            let wire = serde_json::to_value(&boundary).unwrap();
            assert_eq!(wire["parameter_range"], serde_json::json!(endpoints));
            assert_eq!(
                serde_json::from_value::<SketchProfileBoundaryUse>(wire.clone()).unwrap(),
                boundary
            );
            for invalid in [
                serde_json::json!([2.0, 2.0]),
                serde_json::json!([null, 2.0]),
            ] {
                let mut wire = wire.clone();
                wire["parameter_range"] = invalid;
                assert!(serde_json::from_value::<SketchProfileBoundaryUse>(wire)
                    .unwrap_err()
                    .to_string()
                    .contains("parameter_range"));
            }
        }
    }
}

#[test]
fn region_loops_reject_repeated_and_outer_holes() {
    for holes in [vec![1, 1], vec![0], vec![1, 0]] {
        assert!(SketchProfileLoops::new(
            0,
            holes.clone(),
            &cadmpeg_test_support::service_decode_context()
        )
        .expect("fixture loop-region admission")
        .is_err());
        assert!(SketchProfileRegion::loops(
            0,
            holes.clone(),
            &cadmpeg_test_support::service_decode_context()
        )
        .expect("fixture loop-region admission")
        .is_err());
        let wire = serde_json::json!({"outer": 0, "holes": holes});
        assert!(serde_json::from_value::<SketchProfileLoops>(wire.clone())
            .unwrap_err()
            .to_string()
            .contains("holes"));
        let region = serde_json::json!({"region": "loops", "outer": 0, "holes": holes});
        assert!(serde_json::from_value::<SketchProfileRegion>(region)
            .unwrap_err()
            .to_string()
            .contains("holes"));
    }
    for holes in [vec![], vec![2, 1]] {
        let region = SketchProfileRegion::loops(
            0,
            holes.clone(),
            &cadmpeg_test_support::service_decode_context(),
        )
        .expect("fixture loop-region admission")
        .unwrap();
        let SketchProfileRegion::Loops { loops } = &region else {
            panic!("loop region")
        };
        assert_eq!(loops.outer(), 0);
        assert_eq!(loops.holes(), holes);
        let wire = serde_json::to_value(&region).unwrap();
        assert_eq!(wire["region"], "loops");
        assert_eq!(wire["outer"], 0);
        if holes.is_empty() {
            assert!(wire.get("holes").is_none());
        } else {
            assert_eq!(wire["holes"], serde_json::json!(holes));
        }
        assert_eq!(
            serde_json::from_value::<SketchProfileRegion>(wire).unwrap(),
            region
        );
    }
}

#[test]
fn trimmed_regions_require_nonempty_rings_and_region_selections_are_distinct() {
    let boundary = SketchProfileBoundaryUse {
        entity: SketchEntityId::mint("test:test:sketch-entity#one").unwrap(),
        parameter_range: DirectedParameterRange::new([5.0, 2.0]).unwrap(),
        reversed: false,
    };
    assert!(SketchProfileRegion::trimmed(vec![], vec![]).is_err());
    assert!(SketchProfileRegion::trimmed(vec![boundary.clone()], vec![vec![]]).is_err());
    for wire in [
        serde_json::json!({"region": "trimmed", "outer_boundary": []}),
        serde_json::json!({"region": "trimmed", "outer_boundary": [boundary], "hole_boundaries": [[]]}),
    ] {
        let error = serde_json::from_value::<SketchProfileRegion>(wire)
            .unwrap_err()
            .to_string();
        assert!(error.contains("must not be empty"), "{error}");
    }
    let repeated_ring = vec![boundary.clone(), boundary];
    let trimmed = SketchProfileRegion::trimmed(
        repeated_ring.clone(),
        vec![repeated_ring.clone(), repeated_ring],
    )
    .unwrap();
    let loops =
        SketchProfileRegion::loops(2, vec![], &cadmpeg_test_support::service_decode_context())
            .expect("fixture loop-region admission")
            .unwrap();
    let sketch = SketchId::mint("test:test:sketch#one").unwrap();
    for regions in [
        vec![],
        vec![trimmed.clone(), trimmed.clone()],
        vec![loops.clone(), loops.clone()],
    ] {
        assert!(SketchProfileRegions::try_from(regions.clone()).is_err());
        assert!(PlanarProfileRef::sketch_regions(sketch.clone(), regions.clone()).is_err());
        let wire = serde_json::json!({"kind": "sketch_regions", "value": {"sketch": sketch, "regions": regions}});
        assert!(serde_json::from_value::<ProfileRef>(wire.clone()).is_err());
        assert!(serde_json::from_value::<PlanarProfileRef>(wire)
            .unwrap_err()
            .to_string()
            .contains("regions"));
    }
    let profile = PlanarProfileRef::sketch_regions(sketch, vec![trimmed, loops]).unwrap();
    let wire = serde_json::to_value(&profile).unwrap();
    assert_eq!(
        serde_json::from_value::<ProfileRef>(wire).unwrap(),
        ProfileRef::Planar(profile)
    );
}

#[test]
fn a_sketch_profile_region_states_which_boundary_form_it_uses() {
    let boundary = SketchProfileBoundaryUse {
        entity: SketchEntityId::mint("test:test:sketch-entity#one").unwrap(),
        parameter_range: DirectedParameterRange::new([5.0, 2.0]).unwrap(),
        reversed: false,
    };
    let loops =
        SketchProfileRegion::loops(0, vec![2], &cadmpeg_test_support::service_decode_context())
            .expect("fixture loop-region admission")
            .unwrap();
    let trimmed = SketchProfileRegion::trimmed(vec![boundary.clone()], Vec::new()).unwrap();

    let loops_wire = serde_json::to_value(&loops).unwrap();
    assert_eq!(loops_wire["region"], "loops");
    let trimmed_wire = serde_json::to_value(&trimmed).unwrap();
    assert_eq!(trimmed_wire["region"], "trimmed");
    assert_eq!(
        serde_json::from_value::<SketchProfileRegion>(loops_wire.clone()).unwrap(),
        loops
    );
    assert_eq!(
        serde_json::from_value::<SketchProfileRegion>(trimmed_wire.clone()).unwrap(),
        trimmed
    );

    let mut cross = loops_wire;
    cross["outer_boundary"] = serde_json::json!([boundary]);
    let error = serde_json::from_value::<SketchProfileRegion>(cross)
        .unwrap_err()
        .to_string();
    assert!(error.contains("outer_boundary"), "{error}");

    let mut stray = trimmed_wire;
    stray["outer"] = serde_json::json!(0);
    let error = serde_json::from_value::<SketchProfileRegion>(stray)
        .unwrap_err()
        .to_string();
    assert!(error.contains("outer"), "{error}");

    let error = serde_json::from_value::<SketchProfileRegion>(serde_json::json!({"outer": 0}))
        .unwrap_err()
        .to_string();
    assert!(error.contains("region"), "{error}");
}

#[test]
fn whole_loop_constructors_preserve_original_refusals_and_release_scoped_members() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    for region in [false, true] {
        for dimension in [
            ResourceDimension::MaterializedBytes,
            ResourceDimension::CollectionItems,
            ResourceDimension::WorkUnits,
        ] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = 1,
                ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
                _ => panic!("test dimension"),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let result = if region {
                SketchProfileRegion::loops(0, vec![2, 1], &ctx).map(|_| ())
            } else {
                SketchProfileLoops::new(0, vec![2, 1], &ctx).map(|_| ())
            };
            let limit = result.unwrap_err();
            assert_eq!(limit.dimension, dimension);
            assert_eq!(limit.operation, "validate distinct decoded members");
            assert!(
                matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(original)) if original == limit)
            );
        }
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 512;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let loops = if region {
            let SketchProfileRegion::Loops { loops } =
                SketchProfileRegion::loops(0, vec![2, 1], &ctx)
                    .unwrap()
                    .unwrap()
            else {
                panic!("loop region");
            };
            loops
        } else {
            SketchProfileLoops::new(0, vec![2, 1], &ctx)
                .unwrap()
                .unwrap()
        };
        assert_eq!(loops.outer(), 0);
        assert_eq!(loops.holes(), &[2, 1]);
        assert_eq!(
            SketchProfileLoops::new(0, vec![2, 2], &ctx)
                .unwrap()
                .unwrap_err(),
            "holes must be distinct"
        );
        assert_eq!(
            SketchProfileRegion::loops(0, vec![2, 0], &ctx)
                .unwrap()
                .unwrap_err(),
            "holes must not contain outer"
        );
        let reservation = ctx
            .reserve_scoped_limit(512, "released loop-member storage")
            .unwrap();
        drop(reservation);
        ctx.finish_session().unwrap();
    }
}
