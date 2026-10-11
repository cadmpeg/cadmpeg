// SPDX-License-Identifier: Apache-2.0

use crate::sketches::{SketchConstraintDefinitionInput, SketchLocus};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn sketch_constraint_loci_preserve_borrowed_order_and_release_storage() {
    let id = "test:model:entity#loci".try_into().unwrap();
    let definition = SketchConstraintDefinitionInput::Group {
        elements: vec![
            SketchLocus::Start(id),
            SketchLocus::End("test:model:entity#end".try_into().unwrap()),
            SketchLocus::Center("test:model:entity#center".try_into().unwrap()),
        ],
    };
    let SketchConstraintDefinitionInput::Group { elements } = &definition else {
        panic!("group fixture");
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 256;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    {
        let mut loci = [None; 3];
        let mut count = 0;
        super::super::visit_constraint_loci(&ctx, &definition, |locus| {
            loci[count] = Some(locus);
            count += 1;
            Ok(())
        })
        .unwrap();
        assert_eq!(count, elements.len());
        for (actual, expected) in loci.iter().zip(elements) {
            assert!(std::ptr::eq(actual.unwrap(), expected));
        }
    }
    drop(ctx.reserve_scoped(256, "locus scopes released").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn sketch_constraint_loci_preserve_first_and_later_original_refusals() {
    let definition = SketchConstraintDefinitionInput::Group {
        elements: vec![
            SketchLocus::Entity("test:model:entity#first".try_into().unwrap()),
            SketchLocus::Entity("test:model:entity#last".try_into().unwrap()),
        ],
    };
    for (dimension, cap) in [
        (ResourceDimension::MaterializedBytes, 0),
        (ResourceDimension::CollectionItems, 0),
        (ResourceDimension::CollectionItems, 1),
        (ResourceDimension::WorkUnits, 0),
        (ResourceDimension::WorkUnits, 2),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = cap,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = cap,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = cap,
            _ => panic!("test dimension"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut count = 0;
        let result = super::super::visit_constraint_loci(&ctx, &definition, |_| {
            count += 1;
            Ok(())
        });
        if dimension == ResourceDimension::WorkUnits && cap == 0 {
            let Err(CodecError::ResourceLimit(limit)) = result else {
                panic!("first locus visit must refuse");
            };
            assert_eq!(limit.dimension, dimension);
            assert_eq!(count, 0);
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
            );
        } else {
            result.unwrap();
            assert_eq!(count, 2);
            ctx.finish_session().unwrap();
        }
    }
}

#[test]
fn sketch_nurbs_endpoints_read_rational_poles_without_copying_all_rows() {
    let curve = crate::geometry::pcurve::PcurveNurbs::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        1,
        vec![0.0, 0.0, 0.5, 1.0, 1.0],
        vec![
            crate::math::Point2::new(0.0, 0.0),
            crate::math::Point2::new(2.0, 8.0),
            crate::math::Point2::new(3.0, 4.0),
        ],
        Some(vec![1.0, 2.0, 1.0]),
        false,
    )
    .expect("fixture pcurve construction admission")
    .unwrap();
    let geometry = crate::sketches::SketchGeometry::nurbs(curve);
    let endpoints = (
        crate::math::Point2::new(0.0, 0.0),
        crate::math::Point2::new(3.0, 4.0),
    );
    assert_eq!(
        super::super::oriented_endpoints(&geometry, false),
        Some(endpoints)
    );
    assert_eq!(
        super::super::oriented_endpoints(&geometry, true),
        Some((endpoints.1, endpoints.0))
    );
}

fn index_fixture(kind: usize) -> crate::CadIr {
    use crate::sketches::{
        SketchEntity, SketchGeometry, SketchGeometryDefinition, SpatialSketch, SpatialSketchEntity,
        SpatialSketchGeometry, SpatialSketchGeometryDefinition,
    };
    let mut ir = crate::CadIr::empty();
    for key in ["first", "second"] {
        match kind {
            0 => ir.model.sketch_entities.push(SketchEntity::new(
                format!("test:model:entity#{key}").try_into().unwrap(),
                "test:model:sketch#owner".try_into().unwrap(),
                SketchGeometry::try_from(SketchGeometryDefinition::Point {
                    position: crate::math::Point2::new(1.0, 2.0),
                })
                .unwrap(),
            )),
            1 => ir.model.spatial_sketches.push(SpatialSketch {
                id: format!("test:model:sketch#{key}").try_into().unwrap(),
                name: None,
                configuration: None,
                visible: None,
                profiles: vec![],
                native_ref: None,
            }),
            2 => ir
                .model
                .spatial_sketch_entities
                .push(SpatialSketchEntity::new(
                    format!("test:model:entity#{key}").try_into().unwrap(),
                    "test:model:sketch#owner".try_into().unwrap(),
                    SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Point {
                        position: crate::math::Point3::new(1.0, 2.0, 3.0),
                    })
                    .unwrap(),
                )),
            3 => ir.model.parameters.push(crate::features::DesignParameter {
                id: format!("test:model:parameter#{key}").try_into().unwrap(),
                owner: None,
                ordinal: 0,
                name: key.into(),
                expression: "1 mm".into(),
                display: None,
                value: Some(crate::features::ParameterValue::Length(
                    crate::scalar::Length::new(1.0).unwrap(),
                )),
                dependencies: crate::features::DistinctMembers::default(),
                properties: std::collections::BTreeMap::default(),
                pmi: None,
                native_ref: None,
            }),
            _ => panic!("index fixture kind"),
        }
    }
    ir
}

#[test]
fn sketch_indexes_preserve_first_and_later_storage_and_hash_refusals() {
    for kind in 0..4 {
        let ir = index_fixture(kind);
        for (dimension, cap) in [
            (ResourceDimension::MaterializedBytes, 0),
            (ResourceDimension::CollectionItems, 0),
            (ResourceDimension::CollectionItems, 1),
            (ResourceDimension::WorkUnits, 0),
            (ResourceDimension::WorkUnits, 1),
        ] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = cap,
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = cap,
                ResourceDimension::WorkUnits => policy.limits.max_work_units = cap,
                _ => panic!("test dimension"),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut findings = vec![];
            let Err(CodecError::ResourceLimit(limit)) =
                super::super::check_sketches(&ctx, &ir, &mut findings)
            else {
                panic!("index {kind} must refuse {dimension:?} at {cap}");
            };
            assert_eq!(limit.dimension, dimension);
            assert!(findings.is_empty());
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
            );
        }
    }
}

#[test]
fn sketch_indexes_release_all_temporary_storage_without_retention() {
    let mut ir = index_fixture(0);
    ir.model.spatial_sketches = index_fixture(1).model.spatial_sketches;
    ir.model.parameters = index_fixture(3).model.parameters;
    ir.model.spatial_sketch_entities = index_fixture(2).model.spatial_sketch_entities;
    for entity in &mut ir.model.spatial_sketch_entities {
        entity.sketch = ir.model.spatial_sketches[0].id.clone();
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 4096;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut findings = vec![];
    super::super::check_sketches(&ctx, &ir, &mut findings).unwrap();
    assert!(findings.is_empty());
    drop(
        ctx.reserve_scoped(4096, "sketch index scopes released")
            .unwrap(),
    );
    ctx.finish_session().unwrap();
}

#[test]
fn sketch_index_helpers_keep_last_duplicate_values_and_original_lookup_refusals() {
    use crate::features::{ParameterId, ParameterValue};
    use crate::index::identities::BorrowedIdentities;
    use crate::sketches::{SketchGeometry, SketchGeometryDefinition};
    let fixture = cadmpeg_test_support::service_decode_context();
    let id: crate::sketches::SketchEntityId = "test:model:entity#same".try_into().unwrap();
    let point = |u, v| {
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: crate::math::Point2::new(u, v),
        })
        .unwrap()
    };
    let first = point(1.0, 2.0);
    let last = point(3.0, 4.0);
    let geometry = BorrowedIdentities::build(&fixture, |add| {
        add(id.as_str(), &first)?;
        add(id.as_str(), &last)
    })
    .unwrap();
    let locus = SketchLocus::Entity(id.clone());
    assert_eq!(
        super::super::sketch_locus_point(&fixture, &locus, &geometry).unwrap(),
        Some(crate::math::Point2::new(3.0, 4.0))
    );
    let restriction = SketchConstraintDefinitionInput::Midpoint {
        point: locus.clone(),
        entity: id.clone(),
    };
    assert_eq!(
        super::super::constraint_entity_kind_refusal(&fixture, &restriction, &geometry).unwrap(),
        Some("sketch midpoint constraint references an entity that is not a bounded curve")
    );
    let parameter: ParameterId = "test:model:parameter#same".try_into().unwrap();
    let first = Some(ParameterValue::Length(
        crate::scalar::Length::new(1.0).unwrap(),
    ));
    let last = Some(ParameterValue::Length(
        crate::scalar::Length::new(2.0).unwrap(),
    ));
    let parameters = BorrowedIdentities::build(&fixture, |add| {
        add(parameter.as_str(), &first)?;
        add(parameter.as_str(), &last)
    })
    .unwrap();
    assert!(super::super::spatial_length_parameter_matches(
        &fixture,
        Some(2.0),
        &parameter,
        &parameters
    )
    .unwrap());
    assert!(!super::super::spatial_length_parameter_matches(
        &fixture,
        Some(1.0),
        &parameter,
        &parameters
    )
    .unwrap());
    for kind in 0..3 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = match kind {
            0 => super::super::sketch_locus_point(&ctx, &locus, &geometry).map(|_| ()),
            1 => {
                super::super::spatial_length_parameter_matches(&ctx, None, &parameter, &parameters)
                    .map(|_| ())
            }
            _ => super::super::constraint_entity_kind_refusal(&ctx, &restriction, &geometry)
                .map(|_| ()),
        };
        let Err(CodecError::ResourceLimit(limit)) = result else {
            panic!("lookup must refuse before reporting a semantic result");
        };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(limit.operation, "hash validation identity lookup");
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
        );
    }
}

#[test]
fn spatial_sketch_owner_comparison_preserves_refusal_and_absence_semantics() {
    let owner: crate::sketches::SpatialSketchId = "test:model:sketch#first".try_into().unwrap();
    let other: crate::sketches::SpatialSketchId = "test:model:sketch#other".try_into().unwrap();
    let fixture = cadmpeg_test_support::service_decode_context();
    assert!(super::super::same_spatial_owner(&fixture, Some(&owner), &owner).unwrap());
    assert!(!super::super::same_spatial_owner(&fixture, Some(&owner), &other).unwrap());
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(!super::super::same_spatial_owner(&ctx, None, &owner).unwrap());
    let Err(CodecError::ResourceLimit(limit)) =
        super::super::same_spatial_owner(&ctx, Some(&owner), &other)
    else {
        panic!("owner equality must refuse");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, "compare spatial sketch owner");
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
    );
}

#[test]
fn sketch_offset_predicate_preserves_fitted_frame_refusal() {
    let geometry = crate::sketches::SketchGeometry::nurbs(
        crate::geometry::pcurve::PcurveNurbs::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![
                crate::math::Point2::new(0.0, 0.0),
                crate::math::Point2::new(1.0, 0.0),
            ],
            None,
            false,
        )
        .expect("fixture pcurve construction admission")
        .unwrap(),
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(limit)) =
        super::super::sketch_curve_offset_matches(&ctx, &geometry, &geometry, 0.0, 0.0)
    else {
        panic!("offset predicate must preserve refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, "sketch NURBS endpoint knot scan");
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
    );
}

#[test]
fn projected_sketch_copy_keeps_missing_member_semantics_and_finding_order() {
    let mut ir = index_fixture(0);
    ir.model.sketch_entities[1].geometry = crate::sketches::SketchGeometry::try_from(
        crate::sketches::SketchGeometryDefinition::Point {
            position: crate::math::Point2::new(3.0, 4.0),
        },
    )
    .unwrap();
    for (key, result) in [
        ("mismatch", ir.model.sketch_entities[1].id().clone()),
        ("missing", "test:model:entity#missing".try_into().unwrap()),
    ] {
        ir.model
            .sketch_constraints
            .push(crate::sketches::SketchConstraint {
                id: format!("test:model:constraint#{key}").try_into().unwrap(),
                sketch: ir.model.sketch_entities[0].sketch.clone(),
                definition: SketchConstraintDefinitionInput::ProjectedCopy {
                    source: ir.model.sketch_entities[0].id().clone(),
                    result,
                }
                .try_into()
                .unwrap(),
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
    }
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut findings = vec![];
    super::super::check_sketches(&ctx, &ir, &mut findings).unwrap();
    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings[0].check,
        crate::report::check::Check::GeometricConsistency
    );
    assert_eq!(findings[0].severity, crate::report::Severity::Error);
    assert_eq!(
        findings[0].entity.as_deref(),
        Some("test:model:constraint#mismatch")
    );
    assert_eq!(
        findings[0].message,
        "projected-copy entities do not have identical geometry"
    );
    ctx.finish_session().unwrap();
}
