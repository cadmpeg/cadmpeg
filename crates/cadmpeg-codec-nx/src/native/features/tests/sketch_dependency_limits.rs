// SPDX-License-Identifier: Apache-2.0

use crate::native::features::{
    feature_sketch_datum_csys_dependencies, FeatureBinary64ScalarToken,
    FeatureDatumCsysConstruction, FeatureOperationLabel, FeatureSketchPointUse,
    FeatureSketchPointUseReference, OffsetStoreNamedPoint,
};

fn dependency_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let label = |id: &str, ordinal| FeatureOperationLabel {
        id: id.into(), section_link: "section".into(), ordinal,
        value: id.into(), objects: crate::om::header_references::HeaderReferences([None; 4]),
        stable_identity: None, source_offset: 100 + u64::from(ordinal),
    };
    let labels = [label("csys", 0), label("sketch", 1)];
    let point = OffsetStoreNamedPoint {
        id: "point".into(), name: "Point1".into(),
        data_blocks: vec!["first".into(), "shared".into()],
        values: [(1.0, 200), (2.0, 220)].map(|(value, source_offset)| {
            FeatureBinary64ScalarToken {
                scalar: crate::om::scalar::ShiftedBinary64::try_from(
                    crate::test_support::test_bytes::shifted_f64_bytes(value),
                ).expect("shifted scalar"),
                source_offset,
            }
        }),
        source_offset: 190,
    };
    let point_use = FeatureSketchPointUse {
        id: "point-use".into(), operation_label: "sketch".into(),
        references: vec![FeatureSketchPointUseReference {
            sketch_reference: "reference".into(), block_use: "block-use".into(),
            source_offset: 300,
        }],
        sketch_point_group: "point-group".into(), named_point: point.id.clone(),
    };
    let mut blocks = std::array::from_fn(|index| format!("block-{index}"));
    blocks[3] = "shared".into();
    let construction = FeatureDatumCsysConstruction {
        id: "datum-csys-construction".into(), operation_label: "csys".into(),
        frame: crate::om::datum_csys::DatumCsysFrame::new(19, 386,
            blocks.map(|data_block| {
                (crate::om::reference_index::PayloadIndexToken::from_wire(0, &[0xf0, 0])
                    .expect("payload index"), data_block)
            }),
        ).expect("datum frame"),
    };
    let decode = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        feature_sketch_datum_csys_dependencies(ctx, &labels,
            std::slice::from_ref(&point), std::slice::from_ref(&point_use),
            std::slice::from_ref(&construction), &[])
    };
    assert_eq!(crate::test_support::with_decode_context(|ctx| decode(ctx))
        .expect("admitted datum dependency").len(), 1);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty test root");
    decode(&ctx).expect_err("datum dependency resource limit")
}

#[test]
fn sketch_datum_dependency_refuses_collection_limit() {
    let error = dependency_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
}

#[test]
fn sketch_datum_dependency_refuses_retained_limit() {
    let error = dependency_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
}

#[test]
fn sketch_datum_dependency_refuses_scoped_limit() {
    let error = dependency_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes));
}

#[test]
fn sketch_datum_dependency_refuses_work_limit() {
    let error = dependency_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
}
