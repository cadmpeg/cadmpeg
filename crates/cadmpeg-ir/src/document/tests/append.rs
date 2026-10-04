// SPDX-License-Identifier: Apache-2.0

use crate::assets::{Asset, AssetContent, AssetData};
use crate::document::{CadIr, Model};
use crate::features::{Feature, FeatureDefinition, FeatureOperation};
use crate::math::Point3;
use crate::native::{Native, NativeRecord};
use crate::topology::Point;

fn staged_document() -> CadIr {
    let mut ir = CadIr::empty();
    ir.model.points.push(Point::new(
        "test:append:point#new".try_into().unwrap(),
        crate::features::FinitePoint3::new(Point3::new(1.0, 2.0, 3.0))
            .expect("a finite position is a point"),
        None,
    ));
    ir.model.assets.push(Asset {
        id: "test:append:asset#new".try_into().unwrap(),
        name: None,
        media_type: None,
        content: AssetContent::Embedded {
            data: AssetData::new(vec![1, 2, 3]).unwrap(),
        },
        native_ref: None,
    });
    for (ordinal, key) in ["parent", "child"].into_iter().enumerate() {
        ir.model.features.push(Feature {
            id: format!("test:append:feature#{key}").try_into().unwrap(),
            ordinal: cadmpeg_core::decode::u64_from_index(ordinal),
            name: None,
            suppressed: None,
            dependencies: crate::features::DistinctMembers::default(),
            source_properties: std::collections::BTreeMap::default(),
            source_tag: None,
            source_text: None,
            source_content: crate::features::FeatureContent::default(),
            evaluation: crate::features::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(FeatureOperation::StoredGeometry {}),
            ),
            native_ref: None,
        });
    }
    ir.model
        .set_feature_regeneration_parent(
            &cadmpeg_test_support::service_decode_context(),
            &("test:append:feature#child".try_into().unwrap()),
            &("test:append:feature#parent".try_into().unwrap()),
        )
        .unwrap();
    for (format, arena, key) in [
        ("test", "existing", "appended"),
        ("test", "new", "new-arena"),
        ("other", "new", "new-namespace"),
    ] {
        ir.native.namespace_mut(format).arenas_mut().insert(
            arena.into(),
            vec![NativeRecord::new(
                crate::ids::Identity::new(format!("test:append:record#{key}"))
                    .expect("valid identity"),
                serde_json::Map::new(),
            )
            .unwrap()],
        );
    }
    // Exercise the complete-document admission route before the append route.
    CadIr::from_json(&ir.to_canonical_json().unwrap()).unwrap()
}

#[test]
fn rejected_append_restores_neutral_native_and_parent_state() {
    let mut ir = crate::examples::unit_cube().unwrap();
    ir.native.namespace_mut("test").arenas_mut().insert(
        "existing".into(),
        vec![NativeRecord::new(
            crate::ids::Identity::new("test:append:record#original").expect("valid identity"),
            serde_json::Map::new(),
        )
        .unwrap()],
    );
    ir.native.namespace_mut("empty");
    let before = ir.clone();
    let staged = staged_document();
    let result = ir.try_append(
        &cadmpeg_test_support::service_decode_context(),
        staged.model,
        staged.native,
        |combined| {
            assert_eq!(combined.model.points.len(), before.model.points.len() + 1);
            assert_eq!(combined.model.assets.len(), 1);
            assert!(combined
                .model
                .feature_regeneration_parent(&"test:append:feature#child".try_into().unwrap())
                .is_some());
            assert_eq!(
                combined.native.namespace("test").unwrap().arenas()["existing"].len(),
                2
            );
            assert!(combined.native.namespace("other").is_some());
            Ok(Err::<(), _>("independent rejection"))
        },
    );
    assert_eq!(result.unwrap(), Err("independent rejection"));
    assert_eq!(ir, before);
}

#[test]
fn accepted_append_preserves_all_staged_records_and_parent_relations() {
    let mut ir = CadIr::empty();
    let staged = staged_document();
    ir.try_append(
        &cadmpeg_test_support::service_decode_context(),
        staged.model.clone(),
        staged.native.clone(),
        |_| Ok(Ok::<_, ()>(())),
    )
    .unwrap()
    .unwrap();
    assert_eq!(ir.model, staged.model);
    assert_eq!(ir.native, staged.native);
    assert_eq!(
        CadIr::from_json(&ir.to_canonical_json().unwrap()).unwrap(),
        ir
    );
}

#[test]
fn actual_admission_rejects_duplicate_identity_without_changing_the_document() {
    let mut ir = crate::examples::unit_cube().unwrap();
    let before = ir.clone();
    let model = Model {
        points: vec![ir.model.points[0].clone()],
        ..Model::default()
    };
    let result = ir.try_append(
        &cadmpeg_test_support::service_decode_context(),
        model,
        Native::default(),
        |combined| {
            let report = crate::admit(
                &cadmpeg_test_support::service_decode_context(),
                combined,
                crate::DRAFT_CORE_CHECKS,
                Vec::new(),
            )
            .expect("resource allocation did not fail");
            if report.is_ok() {
                Ok(Ok(()))
            } else {
                Ok(Err(report))
            }
        },
    );
    assert!(result.unwrap().is_err());
    assert_eq!(ir, before);
}

#[test]
fn resource_refused_append_restores_neutral_native_and_parent_state() {
    let mut ir = crate::examples::unit_cube().unwrap();
    let before = ir.clone();
    let staged = staged_document();
    let ctx = cadmpeg_test_support::service_decode_context();
    let result = ir.try_append(&ctx, staged.model, staged.native, |_| {
        ctx.charge_work(u64::MAX / 2, "test append callback refusal")?;
        Ok(Ok::<(), ()>(()))
    });
    let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = result else {
        panic!("callback resource refusal must stay outer");
    };
    assert_eq!(limit.operation, "test append callback refusal");
    assert_eq!(ir, before);
    assert!(
        matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(sticky)) if sticky == limit)
    );
}

#[test]
fn append_storage_refusal_precedes_visible_mutation() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    for dimension in [
        ResourceDimension::CollectionItems,
        ResourceDimension::MaterializedBytes,
        ResourceDimension::RetainedBytes,
        ResourceDimension::WorkUnits,
    ] {
        let mut ir = crate::examples::unit_cube().unwrap();
        ir.native
            .namespace_mut("test")
            .arenas_mut()
            .insert("existing".into(), Vec::new());
        let before = ir.clone();
        let staged = staged_document();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = 0,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
            _ => panic!("test dimension"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = ir.try_append(&ctx, staged.model, staged.native, |_| Ok(Ok::<(), ()>(())));
        let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = result else {
            panic!("append storage must refuse");
        };
        assert_eq!(limit.dimension, dimension);
        assert_eq!(ir, before);
        assert!(
            matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(sticky)) if sticky == limit)
        );
    }
}
