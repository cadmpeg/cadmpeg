// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureEvaluation, FeatureOperation};
use cadmpeg_ir::ids::SubdId;
use cadmpeg_ir::subd::{SubdCage, SubdScheme, SubdSurface};

use crate::design::feature_project::form_cages::bind_form_cages;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};

fn fixture() -> (Vec<u8>, DesignParameterScope, Feature, [SubdSurface; 1]) {
    use std::io::{Cursor, Write};
    use zip::CompressionMethod;
    let stream = "FusionAssetName[Active]/FusionDesignSegmentType1/BulkStream.dat";
    let mut list = super::indexed_frame(b"355", 205, 100);
    list[21] = 1;
    list[22..30].copy_from_slice(&201u64.to_le_bytes());
    list[32..36].copy_from_slice(&1u32.to_le_bytes());
    list[36] = 1;
    list[37..45].copy_from_slice(&971u64.to_le_bytes());
    list[47..49].copy_from_slice(&[0xfc, 0]);
    let bulk = [
        list,
        super::indexed_frame(b"262", 205, 15),
        super::indexed_frame(b"325", 971, 15),
    ]
    .concat();
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    crate::test_support::manifest_test::write_synthetic_manifests(&mut archive, stored);
    archive.start_file(stream, stored).unwrap();
    archive.write_all(&bulk).unwrap();
    let archive = archive.finish().unwrap().into_inner();
    let mut scope = DesignParameterScope::empty(
        &format!("f3d:{stream}:scope#201"),
        DesignFeatureKind::Form,
        201,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![205]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let feature = native_feature(&scope);
    let cages = [SubdSurface {
        id: SubdId::mint("synthetic:test:subd#form-cage").unwrap(),
        scheme: SubdScheme::CatmullClark,
        source_object: None,
        cage: SubdCage::default(),
    }];
    (archive, scope, feature, cages)
}

fn native_feature(scope: &DesignParameterScope) -> Feature {
    Feature {
        id: crate::ids::neutral_feature_id(scope),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("Form".into()),
        source_text: None,
        source_content: Default::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::Native {
                kind: "Form".into(),
                parameters: Default::default(),
            },
        )),
        native_ref: Some(scope.id.clone()),
    }
}

#[test]
fn form_binder_refuses_actual_source_and_index_work() {
    let (archive, scope, feature, cages) = fixture();
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        for operation in [
            "f3d form scope scan",
            "f3d form stream cache query",
            "f3d form cage list reference source",
            "f3d form feature source",
            "f3d form feature duplicate",
            "f3d form feature index",
            "f3d form feature lookup",
            "f3d form cage output source",
        ] {
            let error = crate::test_support::resource_refusal_at(
                ResourceDimension::WorkUnits,
                operation,
                0,
                |ctx| {
                    bind_form_cages(
                        ctx,
                        scan,
                        std::slice::from_ref(&scope),
                        &mut [feature.clone()],
                        &cages,
                    )
                },
            );
            assert!(matches!(error, CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::WorkUnits && limit.operation == operation));
        }
    });
}

#[test]
fn form_binder_refuses_live_index_storage() {
    let (archive, scope, feature, cages) = fixture();
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        for operation in [
            "f3d indexed record offset",
            "f3d form stream cache entry",
            "f3d form feature index",
        ] {
            let error = crate::test_support::resource_refusal_at(
                ResourceDimension::MaterializedBytes,
                operation,
                0,
                |ctx| {
                    bind_form_cages(
                        ctx,
                        scan,
                        std::slice::from_ref(&scope),
                        &mut [feature.clone()],
                        &cages,
                    )
                },
            );
            assert!(matches!(error, CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::MaterializedBytes && limit.operation == operation));
        }
    });
}

#[test]
fn form_binder_keeps_first_feature_and_copies_no_ids_for_an_existing_definition() {
    let (archive, scope, feature, cages) = fixture();
    let admitted = FeatureDefinition::Operation(FeatureOperation::Form {
        cages: vec![cages[0].id.clone()],
    });
    let mut first = feature.clone();
    first.evaluation.set_definition(admitted.clone());
    let mut features = [first, feature];
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 1024 * 1024;
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        crate::test_support::with_decode_policy(&policy, |ctx| {
            bind_form_cages(
                ctx,
                scan,
                std::slice::from_ref(&scope),
                &mut features,
                &cages,
            )
            .unwrap();
            assert_eq!(*features[0].evaluation.definition(), admitted);
            assert!(matches!(
                features[1].evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::Native { .. })
            ));
            let _released = ctx
                .reserve_scoped(
                    policy.limits.max_materialized_bytes,
                    "released form binder storage",
                )
                .unwrap();
            assert!(ctx.resource_refusal().is_none());
        });
    });
}

#[test]
fn form_binder_reuses_shared_stream_storage_and_releases_it() {
    let (archive, mut scope, _, _) = fixture();
    scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![206]);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let scopes = [scope.clone(), scope];
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::MaterializedBytes,
            "f3d form stream cache entry",
            0,
            |ctx| bind_form_cages(ctx, scan, &scopes, &mut [], &[]),
        );
        let CodecError::ResourceLimit(limit) = error else {
            panic!("expected materialized refusal");
        };
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = limit.used + limit.additional;
        crate::test_support::with_decode_policy(&policy, |ctx| {
            bind_form_cages(ctx, scan, &scopes, &mut [], &[]).unwrap();
            let _released = ctx
                .reserve_scoped(
                    policy.limits.max_materialized_bytes,
                    "released shared form stream",
                )
                .unwrap();
            assert!(ctx.resource_refusal().is_none());
        });
    });
}

#[test]
fn form_frame_uniqueness_stops_at_the_second_match() {
    let bytes = (0..8)
        .flat_map(|_| super::indexed_frame(b"258", 42, 15))
        .collect::<Vec<_>>();
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        "f3d form frame unique suffix",
        0,
        |ctx| super::super::unique_indexed_frame_by(ctx, &records, 42, |_, _| true),
    );
    let CodecError::ResourceLimit(limit) = error else {
        panic!("expected work refusal");
    };
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = limit.used + limit.additional;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert!(
            super::super::unique_indexed_frame_by(ctx, &records, 42, |_, _| true)
                .unwrap()
                .is_none()
        );
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn form_rejected_first_cage_object_does_not_allocate_slots() {
    let mut list = super::indexed_frame(b"415", 205, 99);
    list[21] = 1;
    list[22..30].copy_from_slice(&201u64.to_le_bytes());
    list[32..36].copy_from_slice(&1u32.to_le_bytes());
    // The first object marker stays zero.
    let bytes = [list, super::indexed_frame(b"258", 205, 15)].concat();
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert!(
            super::super::form_cage_objects(ctx, &bytes, &records, 205, 201)
                .unwrap()
                .is_none()
        );
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn form_serializers_release_names_and_indexes_without_retained_copies() {
    let entry_name = "TSpline.00000000-0000-0000-0000-000000000000.tsm";
    let mut serializer = super::indexed_frame(b"315", 8305, 132);
    serializer[21..25].copy_from_slice(&48u32.to_le_bytes());
    for (ordinal, unit) in entry_name.encode_utf16().enumerate() {
        serializer[25 + 2 * ordinal..27 + 2 * ordinal].copy_from_slice(&unit.to_le_bytes());
    }
    serializer[121] = 1;
    serializer[122..130].copy_from_slice(&8304u64.to_le_bytes());
    let bytes = [serializer, super::indexed_frame(b"457", 8306, 15)].concat();
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 1024 * 1024;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let serializers = super::super::form_cage_serializers(ctx, &bytes, &records).unwrap();
        assert_eq!(serializers.entry_name(ctx, 8304).unwrap(), Some(entry_name));
        drop(serializers);
        let _released = ctx
            .reserve_scoped(
                policy.limits.max_materialized_bytes,
                "released form serializer storage",
            )
            .unwrap();
        assert!(ctx.resource_refusal().is_none());
    });
}

fn named_fixture() -> (Vec<u8>, DesignParameterScope, Feature, [SubdSurface; 1]) {
    use cadmpeg_ir::{CodecFormat, SourceObjectAssociation};
    use std::io::{Cursor, Write};
    use zip::CompressionMethod;
    let stream = "FusionAssetName[Active]/FusionDesignSegmentType1/BulkStream.dat";
    let entry_name = "TSpline.00000000-0000-0000-0000-000000000000.tsm";
    let (mut bulk, _, mut scope) = super::class_328_form_fixture();
    scope
        .try_edit(|draft| {
            draft.id = format!("f3d:{stream}:scope#201");
            draft.class_tag = "328".to_owned().try_into().unwrap();
        })
        .unwrap();
    let mut serializer = super::indexed_frame(b"315", 8305, 132);
    serializer[21..25].copy_from_slice(&48u32.to_le_bytes());
    for (ordinal, unit) in entry_name.encode_utf16().enumerate() {
        serializer[25 + 2 * ordinal..27 + 2 * ordinal].copy_from_slice(&unit.to_le_bytes());
    }
    serializer[121] = 1;
    serializer[122..130].copy_from_slice(&8304u64.to_le_bytes());
    bulk.extend(serializer);
    bulk.extend(super::indexed_frame(b"457", 8306, 15));
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    crate::test_support::manifest_test::write_synthetic_manifests(&mut archive, stored);
    archive.start_file(stream, stored).unwrap();
    archive.write_all(&bulk).unwrap();
    let archive = archive.finish().unwrap().into_inner();
    let feature = native_feature(&scope);
    let source = SourceObjectAssociation {
        format: CodecFormat::F3d,
        object_id: format!("synthetic:test/{entry_name}").try_into().unwrap(),
        name: None,
        color: None,
        visible: None,
        layer: None,
        instance_path: Vec::new(),
    };
    let cages = [SubdSurface {
        id: SubdId::mint("synthetic:test:subd#form-cage").unwrap(),
        scheme: SubdScheme::CatmullClark,
        source_object: Some(source),
        cage: SubdCage::default(),
    }];
    (archive, scope, feature, cages)
}

#[test]
fn form_named_cage_binding_keeps_the_serialized_identity() {
    let (archive, scope, feature, cages) = named_fixture();
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        crate::test_support::with_decode_context(|ctx| {
            let mut features = [feature];
            bind_form_cages(ctx, scan, &[scope], &mut features, &cages).unwrap();
            assert_eq!(
                *features[0].evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::Form {
                    cages: vec![cages[0].id.clone()]
                })
            );
        });
    });
}

#[test]
fn form_named_cage_binding_refuses_original_sources_and_key_operations() {
    let (archive, scope, feature, cages) = named_fixture();
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        for operation in [
            "f3d form cage entry source",
            "f3d form cage entry suffix",
            "f3d form cage entry duplicate",
            "f3d form cage entry index",
            "f3d form cage entry lookup",
            "f3d form serializer binding source",
            "f3d form cage uniqueness source",
            "f3d form cage uniqueness index",
        ] {
            let error = crate::test_support::resource_refusal_at(
                ResourceDimension::WorkUnits,
                operation,
                0,
                |ctx| {
                    bind_form_cages(
                        ctx,
                        scan,
                        std::slice::from_ref(&scope),
                        &mut [feature.clone()],
                        &cages,
                    )
                },
            );
            assert!(matches!(error, CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::WorkUnits && limit.operation == operation));
        }
        for operation in ["f3d form cage entry index", "f3d form candidate cage"] {
            let error = crate::test_support::resource_refusal_at(
                ResourceDimension::CollectionItems,
                operation,
                0,
                |ctx| {
                    bind_form_cages(
                        ctx,
                        scan,
                        std::slice::from_ref(&scope),
                        &mut [feature.clone()],
                        &cages,
                    )
                },
            );
            assert!(matches!(error, CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems && limit.operation == operation));
        }
    });
}

#[test]
fn form_duplicate_named_cages_remain_native_without_retained_copies() {
    let (archive, scope, feature, [cage]) = named_fixture();
    let mut duplicate = cage.clone();
    duplicate.id = SubdId::mint("synthetic:test:subd#duplicate-cage").unwrap();
    let cages = [cage, duplicate];
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 1024 * 1024;
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        crate::test_support::with_decode_policy(&policy, |ctx| {
            let mut features = [feature];
            bind_form_cages(ctx, scan, &[scope], &mut features, &cages).unwrap();
            assert!(matches!(
                features[0].evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::Native { .. })
            ));
            let _released = ctx
                .reserve_scoped(
                    policy.limits.max_materialized_bytes,
                    "released ambiguous form cage storage",
                )
                .unwrap();
            assert!(ctx.resource_refusal().is_none());
        });
    });
}

#[test]
fn form_named_cage_index_refuses_live_storage() {
    let (archive, scope, feature, [cage]) = named_fixture();
    let cages = (0..128)
        .map(|ordinal| {
            let mut value = cage.clone();
            value.id = SubdId::mint(format!("synthetic:test:subd#cage-{ordinal}")).unwrap();
            value.source_object.as_mut().unwrap().object_id =
                format!("synthetic:test/TSpline.cage-{ordinal}.tsm")
                    .try_into()
                    .unwrap();
            value
        })
        .collect::<Vec<_>>();
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::MaterializedBytes,
            "f3d form cage entry index",
            0,
            |ctx| {
                bind_form_cages(
                    ctx,
                    scan,
                    std::slice::from_ref(&scope),
                    &mut [feature.clone()],
                    &cages,
                )
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::MaterializedBytes
                && limit.operation == "f3d form cage entry index"));
    });
}
