// SPDX-License-Identifier: Apache-2.0
//! Text queries and related-record decode admission.

use super::dimension_native;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

#[test]
fn annotation_offset_scan_preserves_values_and_work_refusal() {
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(
            super::super::super::trailing_offset(ctx, "f3d:scope:17:not-an-offset").unwrap(),
            17
        );
        assert_eq!(
            super::super::super::trailing_offset(ctx, "no-offset").unwrap(),
            0
        );
        assert_eq!(super::super::super::trailing_offset(ctx, "29").unwrap(), 29);
        assert_eq!(
            super::super::super::trailing_offset(ctx, "f3d:scope:17:").unwrap(),
            17
        );
    });
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "scan F3D annotation offset components",
        0,
        |ctx| super::super::super::trailing_offset(ctx, "f3d:scope:17:not-an-offset"),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "scan F3D annotation offset components")
    );
}

#[test]
fn annotation_offset_parse_preserves_work_refusal() {
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "parse F3D annotation offset",
        0,
        |ctx| super::super::super::trailing_offset(ctx, "f3d:scope:17:not-an-offset"),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "parse F3D annotation offset")
    );
}

#[test]
fn brep_identity_namespace_prefix_preserves_values_and_work_refusal() {
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(
            super::super::super::brep_identity_namespace(ctx, "design/BREP.body").unwrap(),
            Some("body")
        );
        assert_eq!(
            super::super::super::brep_identity_namespace(ctx, "BREP.").unwrap(),
            Some("")
        );
        assert_eq!(
            super::super::super::brep_identity_namespace(ctx, "design/model.sab").unwrap(),
            None
        );
        assert_eq!(
            super::super::super::brep_identity_namespace(ctx, "").unwrap(),
            None
        );
    });
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "strip F3D BREP identity namespace prefix",
        0,
        |ctx| super::super::super::brep_identity_namespace(ctx, "design/BREP.body"),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "strip F3D BREP identity namespace prefix")
    );
}

#[test]
fn related_owner_links_preserve_order_and_work_refusal() {
    let native = dimension_native();
    crate::test_support::with_decode_context(|ctx| {
        let indices = super::super::super::collect_related_indices(
            ctx,
            super::super::super::RelatedRecordIndexSource::ParameterOwnerLinks(&native),
        )
        .unwrap();
        assert_eq!(
            indices,
            [
                ("f3d:test/BulkStream.dat".to_owned(), 1),
                ("f3d:test/BulkStream.dat".to_owned(), 28),
                ("f3d:test/BulkStream.dat".to_owned(), 30),
            ]
        );
    });
    for operation in [
        "scan F3D related owner links",
        "scan F3D related owner record links",
    ] {
        let error = crate::test_support::resource_refusal_at(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            operation,
            0,
            |ctx| {
                super::super::super::collect_related_indices(
                    ctx,
                    super::super::super::RelatedRecordIndexSource::ParameterOwnerLinks(&native),
                )
            },
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == operation)
        );
    }
}

#[test]
fn joined_text_brep_names_copy_preserves_values_and_work_refusal() {
    let names = [
        "FusionAssetName[Active]/Breps.BlobParts/BREP0.sat",
        "FusionAssetName[Active]/Breps.BlobParts/BREP1.sat",
    ];
    let bytes = crate::test_support::assembly_test::f3d_with_text_brep(&names);
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let scan = crate::container::scan(&ctx, root).unwrap();
    assert_eq!(
        super::super::super::join_text_brep_names(&ctx, &scan).unwrap(),
        names.join("`, `")
    );
    for operation in [
        "copy F3D text BREP name",
        "copy F3D text BREP name separator",
    ] {
        let error = crate::test_support::resource_refusal_at(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            operation,
            0,
            |limited| super::super::super::join_text_brep_names(limited, &scan),
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == operation)
        );
    }
}

#[test]
fn container_only_dimension_search_preserves_work_refusal() {
    use crate::records::dimensions::{
        DesignDimensionAnnotationOperand, DesignDimensionLocusPair, DesignDimensionLocusPairDraft,
    };
    use crate::records::references::DesignClassTag;
    let mut native = dimension_native();
    native.design_dimension_null_locus_pairs =
        vec![
            DesignDimensionLocusPair::try_new(DesignDimensionLocusPairDraft {
                id: "f3d:test/BulkStream.dat:design-dimension-locus-pair#31".into(),
                companion_record_index: 30,
                governing_companion_record_index: 99,
                byte_offset: 0,
                class_tag: DesignClassTag::try_from("423".to_owned()).unwrap(),
                record_index: 31,
                frame_length: 100,
                opaque_index: None,
                loci: [
                    DesignDimensionAnnotationOperand {
                        geometry_record_index: None,
                        geometry_reference_offset: 25,
                        role: 14,
                        role_offset: 35,
                    },
                    DesignDimensionAnnotationOperand {
                        geometry_record_index: std::num::NonZeroU32::new(40),
                        geometry_reference_offset: 40,
                        role: 3,
                        role_offset: 50,
                    },
                ],
                paired_class_tag: DesignClassTag::try_from("259".to_owned()).unwrap(),
                paired_byte_offset: 100,
            })
            .unwrap(),
        ]
        .try_into()
        .unwrap();
    crate::test_support::with_decode_context(|ctx| {
        let parameters =
            super::super::super::container_only_dimension_parameters(ctx, &native).unwrap();
        assert_eq!(parameters.len(), 1);
        assert!(parameters.contains(&crate::ids::neutral_parameter_id(
            &native.design_parameters[0]
        )));
    });
    for operation in [
        "find F3D container-only dimension companion",
        "find F3D container-only dimension parameter",
        "compare F3D dimension parameter streams",
    ] {
        let error = crate::test_support::resource_refusal_at(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            operation,
            0,
            |ctx| super::super::super::container_only_dimension_parameters(ctx, &native),
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == operation)
        );
    }
}

#[test]
fn history_projection_scans_preserve_notes_and_work_refusal() {
    use crate::history_records::{
        AsmDeltaState, AsmHistory, AsmHistoryRecord, AsmHistoryRecordFraming, AsmTopologyCache,
    };
    let native = crate::native::F3dNative {
        asm_histories: vec![AsmHistory {
            id: "f3d:test:asm-history#1".into(),
            byte_offset: 0,
            preamble: None,
            record_table_binding_budget_exceeded: true,
            states: vec![AsmDeltaState {
                id: "f3d:test:asm-delta-state#1".into(),
                parent: "f3d:test:asm-history#1".into(),
                byte_offset: 0,
                state_id: 1,
                version_flag: 0,
                state_flag: 0,
                previous_ref: None,
                next_ref: None,
                node_index: 0,
                partner_ref: None,
                owner_ref: 0,
                bulletin_boards: Vec::new(),
                records: vec![AsmHistoryRecord {
                    id: "f3d:test:asm-history-record#1".into(),
                    parent: "f3d:test:asm-delta-state#1".into(),
                    revision_id: None,
                    byte_offset: 0,
                    framing: AsmHistoryRecordFraming::Opaque {
                        error: "synthetic framing detail".into(),
                    },
                    raw_bytes: Vec::new(),
                }],
                entity_versions: Vec::new(),
                topology_cache: AsmTopologyCache::Absent,
                transition: None,
            }],
        }],
        ..crate::native::F3dNative::default()
    };
    let ir = cadmpeg_ir::CadIr::empty();
    crate::test_support::with_decode_context(|ctx| {
        let mut report = cadmpeg_ir::codec::DecodeBody::new(
            cadmpeg_ir::report::decode::DecodeTransfer::ContainerOnly {},
        );
        super::super::super::report_design_projection_gaps(ctx, &mut report, &ir, &native).unwrap();
        assert_eq!(report.losses[0].message, "1 ASM history stream(s) retain no historical topology because their binding work exceeded the decoder safety budget.");
        assert_eq!(report.losses[1].message, "An ASM history span remains opaque because record framing failed: synthetic framing detail.");
    });
    for operation in [
        "scan F3D history binding budget results",
        "scan F3D history framing results",
        "scan F3D history framing states",
        "scan F3D history framing records",
    ] {
        let error = crate::test_support::resource_refusal_at(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            operation,
            0,
            |ctx| {
                super::super::super::report_design_projection_gaps(
                    ctx,
                    &mut cadmpeg_ir::codec::DecodeBody::new(
                        cadmpeg_ir::report::decode::DecodeTransfer::ContainerOnly {},
                    ),
                    &ir,
                    &native,
                )
            },
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == operation)
        );
    }
}

#[test]
fn text_brep_error_format_preserves_messages_and_work_refusal() {
    let name = "FusionAssetName[Active]/Breps.BlobParts/BREP0.sat";
    let bytes = crate::test_support::assembly_test::f3d_with_text_brep(&[name]);
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let mut scan = crate::container::scan(&ctx, root).unwrap();
    for (unsupported, operation) in [
        (false, "format F3D malformed text BREP error"),
        (true, "format F3D unsupported text BREP error"),
    ] {
        let failure = cadmpeg_asm::stream_error::StreamError {
            format: cadmpeg_asm::stream_error::StreamFormat::Text,
            offset: 7,
            reason: "synthetic reason".into(),
        };
        scan.text_breps.insert(
            name.into(),
            if unsupported {
                crate::container::TextBrepFraming::UnsupportedLength(failure)
            } else {
                crate::container::TextBrepFraming::Malformed(failure)
            },
        );
        let error = super::super::super::try_decode_text_model(&ctx, &scan)
            .err()
            .expect("text BREP framing returns an error");
        match error {
            cadmpeg_core::CodecError::NotImplemented(message) if unsupported => {
                assert_eq!(message, "SAT parse failed at byte 7: synthetic reason");
            }
            cadmpeg_core::CodecError::Malformed(message) if !unsupported => {
                assert_eq!(message, "SAT parse failed at byte 7: synthetic reason");
            }
            error => panic!("unexpected text BREP error: {error:?}"),
        }
        let error = crate::test_support::resource_refusal_at(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            operation,
            0,
            |limited| super::super::super::try_decode_text_model(limited, &scan),
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == operation)
        );
    }
}
