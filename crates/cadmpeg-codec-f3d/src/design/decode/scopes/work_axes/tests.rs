use super::*;
use crate::records::feature::scope::{DesignFeatureKind, DesignScopePayloadMut};
use crate::test_support::{indexed_header, lp_ascii, push_marked_reference};
use cadmpeg_ir::features::{FeatureOperation, UnresolvedFamily};

/// CADIR §3.3: each give-up states its own cause. A nonfinite scalar breaks
/// the frame, and the adjacent form without a reading conforms to it.
#[test]
fn work_axis_failures_of_different_causes_stay_separate() {
    let mut bytes = f64::NAN.to_le_bytes().to_vec();
    bytes.extend_from_slice(&[0; 8]);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, source) = DecodeContext::from_root_bytes(
        &bytes,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    let mut reader = Reader::new(source, 0, bytes.len()).unwrap();
    assert_eq!(reader.real(), ReadField::Unread);
    reader.tail(
        8,
        Cause::Undecoded,
        "WorkAxis carrier has an unreadable class tail",
    );
    let spans = reader
        .failures
        .iter()
        .map(|failure| (failure.start, failure.end, failure.cause))
        .collect::<Vec<_>>();
    assert_eq!(
        spans,
        [(0, 8, Cause::Unreadable), (8, 16, Cause::Undecoded)]
    );
    assert!(matches!(
        reader.failures[1].error,
        CodecError::NotImplemented(_)
    ));
    ctx.finish_session().unwrap();
}

#[test]
fn work_axis_damaged_reference_counts_preserve_each_readable_prefix_occurrence() {
    for (declared, reference) in [
        (2u32, vec![0]),
        (u32::MAX, {
            let mut reference = Vec::new();
            push_marked_reference(&mut reference, 1600);
            reference
        }),
    ] {
        let mut bytes = declared.to_le_bytes().to_vec();
        bytes.extend_from_slice(&reference);
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, source) = DecodeContext::from_root_bytes(
            &bytes,
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();
        let mut reader = Reader::new(source, 0, bytes.len()).unwrap();
        let run = reader.references(&ctx).unwrap();
        assert_eq!(run.declared, ReadField::Value(declared));
        assert_eq!(run.references.len(), 1);
        assert!(run.references[0].complete);
        assert_eq!(run.references[0].present, ReadField::Value(reference[0]));
        if reference[0] == 1 {
            assert_eq!(run.references[0].local(), Some((1600, None)));
        } else {
            assert_eq!(run.references[0].target, ReadField::Absent);
        }
        assert!(!run.complete());
        assert_eq!(reader.failures.len(), 1);
        ctx.finish_session().unwrap();
    }
}

#[test]
fn work_axis_damaged_property_count_preserves_complete_and_partial_properties() {
    for partial in [false, true] {
        let mut bytes = vec![0, 1];
        bytes.extend_from_slice(&3u32.to_le_bytes());
        lp_ascii(&mut bytes, "kept");
        lp_ascii(&mut bytes, "IntrinsicMetaTypeuint64");
        bytes.extend_from_slice(&42u64.to_le_bytes());
        if partial {
            lp_ascii(&mut bytes, "next");
            bytes.extend_from_slice(&[36, 0]);
        }
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, source) = DecodeContext::from_root_bytes(
            &bytes,
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();
        let mut reader = Reader::new(source, 0, bytes.len()).unwrap();
        let mut prefix = AxisCarrierPrefix::default();
        assert!(!reader.prologue(&ctx, &mut prefix).unwrap());
        assert_eq!(prefix.property_count, ReadField::Value(3));
        assert_eq!(prefix.properties.len(), if partial { 2 } else { 1 });
        assert_eq!(prefix.properties[0].key, ReadField::Value("kept".into()));
        assert_eq!(
            prefix.properties[0].value,
            ReadField::Value(AxisCarrierPropertyValue::Unsigned(42))
        );
        if partial {
            assert_eq!(prefix.properties[1].key, ReadField::Value("next".into()));
            assert_eq!(prefix.properties[1].intrinsic_type, ReadField::Unread);
            assert_eq!(prefix.properties[1].value, ReadField::Unread);
        }
        assert_eq!(reader.failures.len(), 1);
        ctx.finish_session().unwrap();
    }
}

#[test]
fn work_axis_legacy_discovery_retains_registered_siblings_and_actual_unsupported_revision() {
    use crate::layout::work_axis_endpoint_carrier as endpoint;
    use crate::layout::work_axis_two_point_carrier as axis;
    for unsupported in [false, true] {
        for sibling in [false, true] {
            let mut bytes = Vec::new();
            indexed_header(&mut bytes, *b"701", 40);
            bytes.extend_from_slice(&[0; 10]);
            bytes.extend_from_slice(&8u32.to_le_bytes());
            for value in [0.0_f64, 0.0, 2.0, 0.0, 0.0, 1.0, 0.0, 0.0] {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            bytes.resize(axis::REFERENCE_COUNT, 0);
            bytes.extend_from_slice(&2u32.to_le_bytes());
            push_marked_reference(&mut bytes, 41);
            push_marked_reference(&mut bytes, 42);
            bytes.resize(axis::LEN, 0);
            indexed_header(&mut bytes, *b"702", 40);
            for (index, z) in [(41, 2.0_f64), (42, 3.0)] {
                let start = bytes.len();
                indexed_header(&mut bytes, *b"703", index);
                bytes.resize(start + endpoint::POSITION, 0);
                for value in [0.0, 0.0, z] {
                    bytes.extend_from_slice(&value.to_le_bytes());
                }
                bytes.resize(start + endpoint::LEN, 0);
                indexed_header(&mut bytes, *b"702", index);
            }
            if sibling {
                indexed_header(&mut bytes, *b"704", 43);
                bytes.extend_from_slice(&0u32.to_le_bytes());
                lp_ascii(&mut bytes, "sibling");
                bytes.extend_from_slice(&[0, 0]);
                bytes.extend_from_slice(&8u32.to_le_bytes());
                for value in [4.0_f64, 5.0, 6.0, 1.0, 0.0, 0.0, 0.0, 0.0] {
                    bytes.extend_from_slice(&value.to_le_bytes());
                }
                bytes.extend_from_slice(&19u32.to_le_bytes());
                bytes.extend_from_slice(&1u32.to_le_bytes());
                push_marked_reference(&mut bytes, 41);
                indexed_header(&mut bytes, *b"702", 43);
            }
            let mut scope = DesignParameterScope::empty(
                "f3d:test/BulkStream.dat:scope#1",
                DesignFeatureKind::WorkAxis,
                1,
            );
            scope
                .try_edit(|draft| {
                    draft.reference_members =
                        crate::records::identity::ReferenceRun::unlocated(vec![
                            40,
                            if sibling { 43 } else { 45 },
                            41,
                            44,
                            42,
                        ]);
                    draft.layout_fixture_references();
                    draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
                    draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
                    draft.layout_fixture_tail();
                })
                .unwrap();
            let mut entities = HashMap::new();
            let mut classes = HashMap::new();
            if unsupported {
                entities.insert(40, (POINT_DIRECTION_TYPE_GUID, 99));
                classes.insert(701, (POINT_DIRECTION_TYPE_GUID, 99));
            }
            if sibling {
                entities.insert(43, (POINT_DIRECTION_TYPE_GUID, 1));
                classes.insert(704, (POINT_DIRECTION_TYPE_GUID, 1));
            }
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let (ctx, source) = DecodeContext::from_root_bytes(
                &bytes,
                &arena,
                &cadmpeg_core::decode::DecodePolicy::service(),
            )
            .unwrap();
            let evidence = decode_work_axis(
                &ctx,
                source,
                &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
                &scope,
                "test/BulkStream.dat",
                WorkAxisRegistrations {
                    entities: &entities,
                    classes: &classes,
                },
                &mut Vec::new(),
            )
            .unwrap();
            assert_eq!(evidence.candidates.len(), if sibling { 2 } else { 1 });
            assert_eq!(
                evidence.candidates[0].version,
                if unsupported {
                    ReadField::Value(99)
                } else {
                    ReadField::Absent
                }
            );
            assert!(
                (evidence.candidates[0].origin[2].value().unwrap().get() - 2.0).abs() < 1.0e-12
            );
            if sibling {
                assert_eq!(evidence.candidates[1].record_index, Some(43));
                assert!(
                    (evidence.candidates[1].origin[0].value().unwrap().get() - 4.0).abs() < 1.0e-12
                );
                assert!(matches!(&evidence.candidates[1].source,
                    Some(DesignWorkAxisSource::PointDirection { reference_type: ReadField::Value(19), inputs, .. }) if inputs.references.len() == 1));
            }
            let DesignScopePayloadMut::WorkAxis(slot) = scope.payload_mut() else {
                panic!("axis payload")
            };
            *slot = Some(evidence);
            let scopes = std::slice::from_ref(&scope);
            let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
            let projected =
                crate::design::feature_project::project_parameter_design_with_edge_identities(
                    &ctx,
                    &crate::design::feature_project::ProjectInputs {
                        scopes,
                        timelines: &timelines,
                        ..Default::default()
                    },
                )
                .unwrap();
            let operation = projected.features[0].evaluation.definition().operation();
            if sibling {
                assert!(matches!(
                    operation,
                    FeatureOperation::Unresolved {
                        family: UnresolvedFamily::DatumAxis
                    }
                ));
                assert!(projected
                    .projection_losses
                    .iter()
                    .any(|loss| loss.code.to_string()
                        == "f3d/feature.work-axis-candidates-unresolved"));
            } else {
                assert!(
                    matches!(operation, FeatureOperation::DatumAxis { axis } if axis.geometry().is_some())
                );
                if unsupported {
                    assert!(projected
                        .projection_losses
                        .iter()
                        .any(|loss| loss.code.to_string()
                            == "f3d/feature.work-axis-unsupported-version-legacy-layout-assumed"));
                    let FeatureOperation::DatumAxis { axis } = operation else {
                        panic!("axis")
                    };
                    assert!(axis
                        .inputs()
                        .iter()
                        .all(|input| input.role.is_none() && input.reference.is_some()));
                }
            }
            ctx.finish_session().unwrap();
        }
    }
}
