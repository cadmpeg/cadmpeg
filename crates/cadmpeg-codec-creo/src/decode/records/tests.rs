// SPDX-License-Identifier: Apache-2.0
use crate::decode::records::{
    expanded_section_records, feature_operation_state_records, feature_reference_name_records,
    feature_row_records, reference_circle_records, reference_conic_records,
    reference_ellipse_records, reference_line_records,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use std::collections::BTreeSet;

fn expanded_records_with_limits(
    max_materialized_bytes: u64,
    max_collection_items: u64,
) -> Result<Vec<crate::decode::records::CreoExpandedSectionRecord>, cadmpeg_core::CodecError> {
    let mut scan = crate::test_support::empty_container_scan();
    scan.framing
        .expanded_sections
        .push(crate::container::ExpandedSection {
            name: "Body".to_string(),
            source_offset: 0,
            compressed_length: 3,
            data: b"abc".to_vec(),
        });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = max_materialized_bytes;
    policy.limits.max_collection_items = max_collection_items;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    expanded_section_records(&ctx, &scan).map(|(records, _storage)| records)
}

#[test]
fn native_expanded_section_id_refuses_retained_limit() {
    let cap = crate::test_support::allocation_limit_at(
        ResourceDimension::MaterializedBytes,
        Some("creo native expanded section IDs"),
        |cap| expanded_records_with_limits(cap, u64::MAX),
    );
    let error = expanded_records_with_limits(cap, u64::MAX)
        .expect_err("expanded-section ID needs full retained length");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::MaterializedBytes
                && resource.operation == "creo native expanded section IDs")
    );
}

#[test]
fn native_expanded_section_name_refuses_retained_limit() {
    let cap = crate::test_support::allocation_limit_at(
        ResourceDimension::MaterializedBytes,
        Some("creo native expanded section names"),
        |cap| expanded_records_with_limits(cap, u64::MAX),
    );
    let error = expanded_records_with_limits(cap, u64::MAX)
        .expect_err("section name needs four retained bytes");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::MaterializedBytes
                && resource.operation == "creo native expanded section names")
    );
}

#[test]
fn native_expanded_section_hash_refuses_retained_limit() {
    let cap = crate::test_support::allocation_limit_at(
        ResourceDimension::MaterializedBytes,
        Some("creo native expanded section hashes"),
        |cap| expanded_records_with_limits(cap, u64::MAX),
    );
    let error = expanded_records_with_limits(cap, u64::MAX)
        .expect_err("SHA-256 hex needs 64 retained bytes");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::MaterializedBytes
                && resource.operation == "creo native expanded section hashes")
    );
}

#[test]
fn native_expanded_section_row_refuses_collection_limit() {
    let cap = crate::test_support::allocation_limit_at(
        ResourceDimension::CollectionItems,
        Some("creo native expanded section records"),
        |cap| expanded_records_with_limits(u64::MAX, cap),
    );
    let error = expanded_records_with_limits(u64::MAX, cap)
        .expect_err("one expanded section needs one output row");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == "creo native expanded section records")
    );
    let records =
        expanded_records_with_limits(u64::MAX, u64::MAX).expect("the expanded section is admitted");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].id, "creo:container:expanded_section#Body:0");
    assert_eq!(records[0].name, "Body");
    assert_eq!(records[0].sha256, cadmpeg_ir::hash::sha256_hex(b"abc"));
}

fn reference_scan() -> crate::container::ContainerScan<'static> {
    use cadmpeg_ir::features::FinitePoint3;
    use cadmpeg_ir::scalar::{FiniteReal, PositiveLength};
    use cadmpeg_ir::units::UnitVector3;
    let mut scan = crate::test_support::empty_container_scan();
    let point = |coordinates: [f64; 3]| {
        FinitePoint3::new(coordinates.into()).expect("finite reference point")
    };
    scan.references.lines.push(
        crate::reference::ReferenceLine::try_new(
            crate::reference::ReferenceLineKind::Line,
            point([0.0, 0.0, 0.0]),
            point([1.0, 0.0, 0.0]),
            0,
        )
        .expect("checked reference geometry"),
    );
    scan.references.circles.push(
        crate::reference::ReferenceCircle::try_new(
            7,
            crate::reference::ReferenceCircleCenter::Stored(point([0.0, 0.0, 0.0])),
            PositiveLength::new(1.0).expect("positive radius"),
            UnitVector3::new([0.0, 0.0, 1.0].into()).expect("unit axis"),
            [point([1.0, 0.0, 0.0]), point([0.0, 1.0, 0.0])],
            0,
        )
        .expect("checked reference geometry"),
    );
    scan.references
        .conics
        .push(crate::reference::ReferenceConic {
            entity_id: 8,
            type_id: crate::reference::ConicType::Ellipse,
            flip: 1,
            start: point([2.0, 0.0, 0.0]),
            end: point([0.0, 1.0, 0.0]),
            parameter_start: None,
            parameter_end: None,
            coefficient_1: FiniteReal::new(2.0).expect("finite coefficient"),
            coefficient_2: FiniteReal::new(1.0).expect("finite coefficient"),
            local_system: None,
            body: vec![0x31, 0x32],
            offset: 0,
        });
    scan.references.ellipses.push(
        crate::reference::ReferenceEllipse::try_new(
            8,
            point([0.0, 0.0, 0.0]),
            UnitVector3::new([0.0, 0.0, 1.0].into()).expect("unit axis"),
            UnitVector3::new([1.0, 0.0, 0.0].into()).expect("unit direction"),
            [
                PositiveLength::new(2.0).expect("positive radius"),
                PositiveLength::new(1.0).expect("positive radius"),
            ],
            0,
        )
        .expect("checked reference geometry"),
    );
    scan
}

fn reference_records_with_limits(
    max_materialized_bytes: u64,
    max_collection_items: u64,
    project: impl FnOnce(
        &DecodeContext<'_>,
        &crate::container::ContainerScan<'_>,
    ) -> Result<usize, cadmpeg_core::CodecError>,
) -> Result<usize, cadmpeg_core::CodecError> {
    let scan = reference_scan();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = max_materialized_bytes;
    policy.limits.max_collection_items = max_collection_items;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    project(&ctx, &scan)
}

#[test]
fn native_reference_line_id_refuses_retained_limit() {
    let cap = crate::test_support::allocation_limit_at(
        ResourceDimension::MaterializedBytes,
        Some("creo native reference line IDs"),
        |cap| {
            reference_records_with_limits(cap, u64::MAX, |ctx, scan| {
                reference_line_records(ctx, scan).map(|projection| projection.0.len())
            })
        },
    );
    let error = reference_records_with_limits(cap, u64::MAX, |ctx, scan| {
        reference_line_records(ctx, scan).map(|projection| projection.0.len())
    })
    .expect_err("line ID needs its full retained length");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::MaterializedBytes
                && resource.operation == "creo native reference line IDs")
    );
}

#[test]
fn native_reference_line_row_refuses_collection_limit() {
    let cap = crate::test_support::allocation_limit_at(
        ResourceDimension::CollectionItems,
        Some("creo native reference line records"),
        |cap| {
            reference_records_with_limits(u64::MAX, cap, |ctx, scan| {
                reference_line_records(ctx, scan).map(|projection| projection.0.len())
            })
        },
    );
    let error = reference_records_with_limits(u64::MAX, cap, |ctx, scan| {
        reference_line_records(ctx, scan).map(|projection| projection.0.len())
    })
    .expect_err("one line record needs an output row");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == "creo native reference line records")
    );
    assert_eq!(
        reference_records_with_limits(u64::MAX, u64::MAX, |ctx, scan| {
            reference_line_records(ctx, scan).map(|projection| projection.0.len())
        })
        .expect("one line record"),
        1
    );
}

#[test]
fn native_reference_circle_id_refuses_retained_limit() {
    let cap = crate::test_support::allocation_limit_at(
        ResourceDimension::MaterializedBytes,
        Some("creo native reference circle IDs"),
        |cap| {
            reference_records_with_limits(cap, u64::MAX, |ctx, scan| {
                reference_circle_records(ctx, scan).map(|projection| projection.0.len())
            })
        },
    );
    let error = reference_records_with_limits(cap, u64::MAX, |ctx, scan| {
        reference_circle_records(ctx, scan).map(|projection| projection.0.len())
    })
    .expect_err("circle ID needs its full retained length");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::MaterializedBytes
                && resource.operation == "creo native reference circle IDs")
    );
}

#[test]
fn native_reference_circle_row_refuses_collection_limit() {
    let cap = crate::test_support::allocation_limit_at(
        ResourceDimension::CollectionItems,
        Some("creo native reference circle records"),
        |cap| {
            reference_records_with_limits(u64::MAX, cap, |ctx, scan| {
                reference_circle_records(ctx, scan).map(|projection| projection.0.len())
            })
        },
    );
    let error = reference_records_with_limits(u64::MAX, cap, |ctx, scan| {
        reference_circle_records(ctx, scan).map(|projection| projection.0.len())
    })
    .expect_err("one circle record needs an output row");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == "creo native reference circle records")
    );
    assert_eq!(
        reference_records_with_limits(u64::MAX, u64::MAX, |ctx, scan| {
            reference_circle_records(ctx, scan).map(|projection| projection.0.len())
        })
        .expect("one circle record"),
        1
    );
}

#[test]
fn native_reference_conic_id_refuses_retained_limit() {
    let cap = crate::test_support::allocation_limit_at(
        ResourceDimension::MaterializedBytes,
        Some("creo native reference conic IDs"),
        |cap| {
            reference_records_with_limits(cap, u64::MAX, |ctx, scan| {
                reference_conic_records(ctx, scan).map(|projection| projection.0.len())
            })
        },
    );
    let error = reference_records_with_limits(cap, u64::MAX, |ctx, scan| {
        reference_conic_records(ctx, scan).map(|projection| projection.0.len())
    })
    .expect_err("conic ID needs its full retained length");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::MaterializedBytes
                && resource.operation == "creo native reference conic IDs")
    );
}

#[test]
fn native_reference_conic_row_refuses_collection_limit() {
    let cap = crate::test_support::allocation_limit_at(
        ResourceDimension::CollectionItems,
        Some("creo native reference conic records"),
        |cap| {
            reference_records_with_limits(u64::MAX, cap, |ctx, scan| {
                reference_conic_records(ctx, scan).map(|projection| projection.0.len())
            })
        },
    );
    let error = reference_records_with_limits(u64::MAX, cap, |ctx, scan| {
        reference_conic_records(ctx, scan).map(|projection| projection.0.len())
    })
    .expect_err("one conic record needs an output row");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == "creo native reference conic records")
    );
    assert_eq!(
        reference_records_with_limits(u64::MAX, u64::MAX, |ctx, scan| {
            reference_conic_records(ctx, scan).map(|projection| projection.0.len())
        })
        .expect("one conic record"),
        1
    );
}

#[test]
fn native_reference_ellipse_id_refuses_retained_limit() {
    let cap = crate::test_support::allocation_limit_at(
        ResourceDimension::MaterializedBytes,
        Some("creo native reference ellipse IDs"),
        |cap| {
            reference_records_with_limits(cap, u64::MAX, |ctx, scan| {
                reference_ellipse_records(ctx, scan).map(|projection| projection.0.len())
            })
        },
    );
    let error = reference_records_with_limits(cap, u64::MAX, |ctx, scan| {
        reference_ellipse_records(ctx, scan).map(|projection| projection.0.len())
    })
    .expect_err("ellipse ID needs its full retained length");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::MaterializedBytes
                && resource.operation == "creo native reference ellipse IDs")
    );
}

#[test]
fn native_reference_ellipse_source_id_refuses_retained_limit() {
    let cap = crate::test_support::allocation_limit_at(
        ResourceDimension::MaterializedBytes,
        Some("creo native reference ellipse source IDs"),
        |cap| {
            reference_records_with_limits(cap, u64::MAX, |ctx, scan| {
                reference_ellipse_records(ctx, scan).map(|projection| projection.0.len())
            })
        },
    );
    let error = reference_records_with_limits(cap, u64::MAX, |ctx, scan| {
        reference_ellipse_records(ctx, scan).map(|projection| projection.0.len())
    })
    .expect_err("source conic ID needs its full retained length");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::MaterializedBytes
                && resource.operation == "creo native reference ellipse source IDs")
    );
}

#[test]
fn native_reference_ellipse_row_refuses_collection_limit() {
    let cap = crate::test_support::allocation_limit_at(
        ResourceDimension::CollectionItems,
        Some("creo native reference ellipse records"),
        |cap| {
            reference_records_with_limits(u64::MAX, cap, |ctx, scan| {
                reference_ellipse_records(ctx, scan).map(|projection| projection.0.len())
            })
        },
    );
    let error = reference_records_with_limits(u64::MAX, cap, |ctx, scan| {
        reference_ellipse_records(ctx, scan).map(|projection| projection.0.len())
    })
    .expect_err("one ellipse record needs an output row");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == "creo native reference ellipse records")
    );
    assert_eq!(
        reference_records_with_limits(u64::MAX, u64::MAX, |ctx, scan| {
            reference_ellipse_records(ctx, scan).map(|projection| projection.0.len())
        })
        .expect("one ellipse record"),
        1
    );
}

fn reference_name_scan() -> crate::container::ContainerScan<'static> {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features
        .reference_names
        .push(crate::feature::operations::FeatureReferenceName {
            feature_id: 40,
            name_bytes: b"A\xff".to_vec(),
            own_reference_id: 3,
            reference_type: 1,
            offset: 0,
        });
    scan
}

fn reference_name_records_with_limits(
    max_materialized_bytes: u64,
    max_collection_items: u64,
) -> Result<Vec<crate::decode::records::CreoFeatureReferenceNameRecord>, cadmpeg_core::CodecError> {
    let scan = reference_name_scan();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = max_materialized_bytes;
    policy.limits.max_collection_items = max_collection_items;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    feature_reference_name_records(&ctx, &scan).map(|(records, _storage)| records)
}

fn operation_state_scan() -> crate::container::ContainerScan<'static> {
    use crate::feature::operations::{
        FeatureOperation, IdKeyword, OperationKind, OperationName, RecipeResolution, RecipeState,
    };
    let mut scan = crate::test_support::empty_container_scan();
    scan.features.operations.push(FeatureOperation {
        feature_id: 40,
        kind: OperationKind::Native,
        name: OperationName::Derived,
        recipe: RecipeResolution::None,
        display_state_conflict: false,
        depdb: None,
        offset: 0,
        state_offset: 0,
    });
    scan.features.operation_states.push(FeatureOperation {
        feature_id: 40,
        kind: OperationKind::Native,
        name: OperationName::Stored {
            bytes: b"A\xff".to_vec(),
            keyword: IdKeyword::Id,
            prefix: Some(b'~'),
        },
        recipe: RecipeState::None,
        display_state_conflict: false,
        depdb: None,
        offset: 0,
        state_offset: 0,
    });
    scan
}

fn operation_state_records_with_limits(
    max_materialized_bytes: u64,
    max_collection_items: u64,
) -> Result<Vec<serde_json::Value>, cadmpeg_core::CodecError> {
    let scan = operation_state_scan();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = max_materialized_bytes;
    policy.limits.max_collection_items = max_collection_items;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let records_parts = feature_operation_state_records(&ctx, &scan)?;
    let _records_storage = records_parts.1;
    let records = records_parts.0;
    Ok(records
        .iter()
        .map(|record| serde_json::to_value(record).expect("record JSON"))
        .collect())
}

#[test]
fn native_feature_current_offset_refuses_node_limit() {
    let cap = crate::test_support::allocation_limit_at(
        ResourceDimension::CollectionItems,
        Some("creo native feature current-offset nodes"),
        |cap| operation_state_records_with_limits(u64::MAX, cap),
    );
    let error = operation_state_records_with_limits(u64::MAX, cap)
        .expect_err("one current offset needs a BTreeMap node");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == "creo native feature current-offset nodes")
    );
}

#[test]
fn native_feature_state_ordinal_refuses_node_limit() {
    let cap = crate::test_support::allocation_limit_at(
        ResourceDimension::CollectionItems,
        Some("creo native feature ordinal nodes"),
        |cap| operation_state_records_with_limits(u64::MAX, cap),
    );
    let error = operation_state_records_with_limits(u64::MAX, cap)
        .expect_err("one state ordinal needs a BTreeMap node");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == "creo native feature ordinal nodes")
    );
}

#[test]
fn native_feature_state_name_refuses_replacement_limit() {
    let error = operation_state_records_with_limits(
        crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
            Some("creo native feature state name"),
            |cap| operation_state_records_with_limits(cap, u64::MAX),
        ),
        u64::MAX,
    )
    .expect_err("one invalid name needs four replacement bytes");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::MaterializedBytes
                && resource.operation == "creo native feature state name")
    );
}

#[test]
fn native_feature_state_prefix_refuses_retained_limit() {
    let error = operation_state_records_with_limits(
        crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
            Some("creo native feature state prefix"),
            |cap| operation_state_records_with_limits(cap, u64::MAX),
        ),
        u64::MAX,
    )
    .expect_err("the source prefix needs another retained byte");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::MaterializedBytes
                && resource.operation == "creo native feature state prefix")
    );
}

#[test]
fn native_feature_state_id_refuses_retained_limit() {
    let error = operation_state_records_with_limits(
        crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
            Some("creo native feature state IDs"),
            |cap| operation_state_records_with_limits(cap, u64::MAX),
        ),
        u64::MAX,
    )
    .expect_err("the state ID needs its full retained length");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::MaterializedBytes
                && resource.operation == "creo native feature state IDs")
    );
}

#[test]
fn native_feature_state_row_refuses_collection_limit() {
    let cap = crate::test_support::allocation_limit_at(
        ResourceDimension::CollectionItems,
        Some("creo native feature state records"),
        |cap| operation_state_records_with_limits(u64::MAX, cap),
    );
    let error = operation_state_records_with_limits(u64::MAX, cap)
        .expect_err("one native state needs an output Vec row");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == "creo native feature state records")
    );
    let records = operation_state_records_with_limits(u64::MAX, u64::MAX)
        .expect("the service-profile state is admitted");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["id"], "creo:mdlstatus:feature_state#40:0");
    assert_eq!(records[0]["stored_name"], "A\u{fffd}");
    assert_eq!(
        records[0]["stored_name_bytes"],
        serde_json::json!([65, 255])
    );
    assert_eq!(records[0]["stored_name_prefix"], "~");
    assert_eq!(records[0]["identifier_keyword"], "id");
    assert_eq!(records[0]["family"], "Native Feature");
    assert_eq!(records[0]["current"], true);
}

#[test]
fn native_feature_reference_id_refuses_retained_limit() {
    let cap = crate::test_support::allocation_limit_at(
        ResourceDimension::MaterializedBytes,
        Some("creo native feature reference IDs"),
        |cap| reference_name_records_with_limits(cap, u64::MAX),
    );
    let error = reference_name_records_with_limits(cap, u64::MAX)
        .expect_err("native reference ID needs its full retained length");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::MaterializedBytes
                && resource.operation == "creo native feature reference IDs")
    );
}

#[test]
fn native_feature_reference_text_refuses_replacement_limit() {
    let cap = crate::test_support::allocation_limit_at(
        ResourceDimension::MaterializedBytes,
        Some("creo native feature reference text"),
        |cap| reference_name_records_with_limits(cap, u64::MAX),
    );
    let error = reference_name_records_with_limits(cap, u64::MAX)
        .expect_err("invalid UTF-8 needs four retained bytes");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::MaterializedBytes
                && resource.operation == "creo native feature reference text")
    );
}

#[test]
fn native_feature_reference_bytes_refuse_retained_limit() {
    let cap = crate::test_support::allocation_limit_at(
        ResourceDimension::MaterializedBytes,
        Some("creo native feature reference bytes"),
        |cap| reference_name_records_with_limits(cap, u64::MAX),
    );
    let error = reference_name_records_with_limits(cap, u64::MAX)
        .expect_err("source bytes need their full retained length");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::MaterializedBytes
                && resource.operation == "creo native feature reference bytes")
    );
}

#[test]
fn native_feature_reference_row_refuses_collection_limit() {
    let cap = crate::test_support::allocation_limit_at(
        ResourceDimension::CollectionItems,
        Some("creo native feature reference records"),
        |cap| reference_name_records_with_limits(u64::MAX, cap),
    );
    let error = reference_name_records_with_limits(u64::MAX, cap)
        .expect_err("one native record needs one collection item");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == "creo native feature reference records")
    );
    let records = reference_name_records_with_limits(u64::MAX, u64::MAX)
        .expect("one native record is admitted");
    assert_eq!(records[0].name, "A\u{fffd}");
    assert_eq!(records[0].name_bytes, b"A\xff");
}

#[test]
fn overlapping_feature_candidates_do_not_expose_short_headers() {
    let payload = [1, 0xe3, 2, 0, 0, 0xe3, 0xf6, 0x83, 0x8f, 0xe1];
    let mut scan = crate::test_support::empty_container_scan();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&payload, &arena, &policy)
        .expect("root input is admitted");
    scan.features.rows = crate::feature::rows::rows(&ctx, &payload, &BTreeSet::from([1, 2]), 0)
        .expect("feature rows are admitted");
    let records_parts =
        feature_row_records(&ctx, &scan).expect("feature row records are admitted");
    let _records_storage = records_parts.1;
    let records = records_parts.0;
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].owner_feature_id, 2);
    assert_eq!(records[0].header, [0, 0]);
    assert_eq!(records[0].body, &payload[3..]);
}
mod projection_admission;

mod curve_expression_projection_limit_tests;
mod curve_plane_projection_limit_tests;
mod curve_projection_limit_tests;
mod feature_choice_field_record_tests;
mod feature_definition_projection_limit_tests;
mod feature_entity_table_record_tests;
mod feature_projection_limit_tests;
mod pcurve_endpoint_projection_limit_tests;
mod sketch_projection_limit_tests;
mod surface_parameter_projection_limit_tests;
mod surface_projection_limit_tests;
mod topology_projection_limit_tests;

#[test]
fn native_projection_holds_scratch_storage_until_drop() {
    let cap = crate::test_support::allocation_limit_at(
        ResourceDimension::MaterializedBytes,
        None,
        |cap| {
            let scan = reference_scan();
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = cap;
            policy.limits.max_retained_bytes = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            reference_ellipse_records(&ctx, &scan).map(|_| ())
        },
    );
    let probe = |release: bool| {
        let scan = reference_scan();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = cap;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let records_parts = reference_ellipse_records(&ctx, &scan)?;
        let storage = records_parts.1;
        let records = records_parts.0;
        assert_eq!(records.len(), 1);
        if release {
            drop(records);
            drop(storage);
            ctx.reserve_scoped(cap, "native projection storage probe")
                .map(drop)
        } else {
            let result = ctx
                .reserve_scoped(cap, "native projection storage probe")
                .map(drop);
            drop(records);
            drop(storage);
            result
        }
    };
    let error = probe(false).expect_err("live projection consumes temporary storage");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::MaterializedBytes
            && resource.operation == "native projection storage probe")
    );
    probe(true).expect("dropping the projection releases its storage");
}
