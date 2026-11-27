// SPDX-License-Identifier: Apache-2.0
//! Design parameter frames through the full decode.

use std::io::{Cursor, Write};

use super::decode;
use crate::test_support::manifest_test::write_synthetic_manifests;
use crate::test_support::native_test::f3d_native;

/// A compact null-owner parameter frame decodes through the full decode as a
/// parameter without an owner binding, and its set trailer byte 3 reports the
/// uninterpreted-flag loss.
#[test]
fn compact_null_owner_parameter_reports_its_owner_and_trailer_flag() {
    const TAIL_TYPE_GUID: &str = "B0556F58-EE09-46A7-B18F-E296F48E5B13";
    let target = 19u32;
    let mut frame = Vec::new();
    frame.extend_from_slice(&3u32.to_le_bytes());
    frame.extend_from_slice(b"372");
    frame.extend_from_slice(&901u32.to_le_bytes());
    frame.extend_from_slice(&[0; 15]);
    frame.extend_from_slice(&87u32.to_le_bytes());
    frame.push(0);
    crate::test_support::lp_utf16(&mut frame, "6.35 mm");
    frame.extend_from_slice(&[0, 0, 0, 1, 0]);
    crate::test_support::lp_utf16(&mut frame, "Linear Dimension-2");
    crate::test_support::lp_utf16(&mut frame, "mm");
    crate::test_support::lp_utf16(&mut frame, "d87");
    frame.extend_from_slice(&0.635f64.to_le_bytes());
    frame.extend_from_slice(&[0, 1]);
    frame.extend_from_slice(&target.to_le_bytes());
    frame.extend_from_slice(&[0; 6]);
    let metadata = crate::test_support::streams_test::design_metastream_with_records(
        &[(
            TAIL_TYPE_GUID,
            "A7AEA631-985B-4DD1-8CE2-DE2C-14B54081",
            3,
            "CommonData",
            &[u64::from(target)],
        )],
        &[],
    );
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    write_synthetic_manifests(&mut zip, stored);
    for (name, payload) in [("BulkStream.dat", frame), ("MetaStream.dat", metadata)] {
        zip.start_file(format!("FusionAssetName[Active]/Design1/{name}"), stored)
            .unwrap();
        zip.write_all(&payload).unwrap();
    }
    let decoded = decode(zip.finish().unwrap().into_inner());
    let native = f3d_native(decoded.ir());
    let [parameter] = native.design_parameters.as_slice() else {
        panic!("expected the one null-owner parameter");
    };
    assert_eq!(parameter.name(), "d87");
    assert_eq!(parameter.owner_record_index(), None);
    let losses = &decoded.report().losses;
    assert!(losses.iter().any(|loss| {
        loss.code == crate::loss::F3dLossCode::ParameterTrailerFlagUninterpreted.kind()
    }));
    assert!(losses
        .iter()
        .any(|loss| loss.code == crate::loss::F3dLossCode::ParameterOwnerUnstored.kind()));
    assert!(!losses
        .iter()
        .any(|loss| loss.code == crate::loss::F3dLossCode::ParameterRecordUnparsed.kind()));
}

/// A discriminated parameter frame that sets the header flag decodes through
/// the full decode and reports the uninterpreted header flag.
#[test]
fn a_parameter_with_the_header_flag_decodes_and_reports_the_flag() {
    const TAIL_TYPE_GUID: &str = "B0556F58-EE09-46A7-B18F-E296F48E5B13";
    let mut frame = crate::design::test_support::parameter_record(
        None,
        "2.50 mm",
        "Thickness",
        Some("mm"),
        "d1",
        0.25,
    );
    frame[21] = 1;
    frame[22..26].copy_from_slice(&6u32.to_le_bytes());
    let expression_end = 36 + 4 + 2 * "2.50 mm".len();
    frame.splice(expression_end..expression_end + 9, [0; 10]);
    let metadata = crate::test_support::streams_test::design_metastream_with_records(
        &[(
            TAIL_TYPE_GUID,
            "A7AEA631-985B-4DD1-8CE2-DE2C-14B54081",
            3,
            "CommonData",
            &[19],
        )],
        &[],
    );
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    write_synthetic_manifests(&mut zip, stored);
    for (name, payload) in [("BulkStream.dat", frame), ("MetaStream.dat", metadata)] {
        zip.start_file(format!("FusionAssetName[Active]/Design1/{name}"), stored)
            .unwrap();
        zip.write_all(&payload).unwrap();
    }
    let decoded = decode(zip.finish().unwrap().into_inner());
    let native = f3d_native(decoded.ir());
    let [parameter] = native.design_parameters.as_slice() else {
        panic!("expected the one Thickness parameter");
    };
    assert_eq!(parameter.name(), "d1");
    assert_eq!(parameter.owner_record_index(), None);
    let losses = &decoded.report().losses;
    assert!(losses.iter().any(|loss| {
        loss.code == crate::loss::F3dLossCode::ParameterHeaderFlagUninterpreted.kind()
    }));
    assert!(!losses
        .iter()
        .any(|loss| loss.code == crate::loss::F3dLossCode::ParameterRecordUnparsed.kind()));
}

/// A discriminated parameter frame whose ten-byte trailer sets the second
/// flag byte decodes through the full decode as driven and reports the
/// driven-flag reading.
#[test]
fn ten_byte_trailer_driven_flag_reports_its_reading() {
    const TAIL_TYPE_GUID: &str = "B0556F58-EE09-46A7-B18F-E296F48E5B13";
    let mut frame = crate::design::test_support::parameter_record(
        Some(0x5d),
        "1",
        "Linear Dimension-2",
        None,
        "d4",
        1.0,
    );
    let expression_end = 46 + 4 + 2;
    frame.splice(
        expression_end..expression_end + 9,
        [0, 0, 0, 0, 0, 0, 1, 0, 0, 0],
    );
    let metadata = crate::test_support::streams_test::design_metastream_with_records(
        &[(
            TAIL_TYPE_GUID,
            "A7AEA631-985B-4DD1-8CE2-DE2C-14B54081",
            3,
            "CommonData",
            &[19],
        )],
        &[],
    );
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    write_synthetic_manifests(&mut zip, stored);
    for (name, payload) in [("BulkStream.dat", frame), ("MetaStream.dat", metadata)] {
        zip.start_file(format!("FusionAssetName[Active]/Design1/{name}"), stored)
            .unwrap();
        zip.write_all(&payload).unwrap();
    }
    let decoded = decode(zip.finish().unwrap().into_inner());
    let native = f3d_native(decoded.ir());
    let [parameter] = native.design_parameters.as_slice() else {
        panic!("expected the one driven parameter");
    };
    assert_eq!(parameter.driving(), Some(false));
    let reported = |code: crate::loss::F3dLossCode| {
        decoded
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == code.kind())
    };
    assert!(reported(
        crate::loss::F3dLossCode::ParameterDrivenFlagAssumed
    ));
    assert!(!reported(
        crate::loss::F3dLossCode::ParameterTrailerFlagUninterpreted
    ));
}

#[test]
fn base_feature_without_result_body_binding_keeps_a_neutral_snapshot() {
    for source_kind in [
        "Base Feature",
        "Elemento base",
        "Temel Özellik",
        "Basiselement",
    ] {
        use crate::layout::base_feature_wide_zero_body as prefix;
        use cadmpeg_ir::features::selections::BodySelection;
        use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
        let (mut bulk, records) =
            crate::test_support::streams_test::generated_design_base_feature_bulkstream();
        let start = usize::try_from(records.last().unwrap().1).unwrap();
        let mut kind = Vec::new();
        crate::test_support::lp_utf16(&mut kind, "Base Feature");
        let kind_at = start
            + bulk[start..]
                .windows(kind.len())
                .position(|bytes| bytes == kind)
                .unwrap();
        let mut source_kind_bytes = Vec::new();
        crate::test_support::lp_utf16(&mut source_kind_bytes, source_kind);
        bulk.splice(
            kind_at..kind_at + kind.len(),
            source_kind_bytes.iter().copied(),
        );
        let label_at = kind_at + source_kind_bytes.len() + 8;
        let mut label = Vec::new();
        crate::test_support::lp_utf16(&mut label, "Unbound result");
        bulk.splice(label_at..label_at + 4, label);
        bulk[start + prefix::ZERO_BODY_MARKER] = 1;
        bulk[start + 29] = 1;
        bulk[start + prefix::SHARED_METADATA_MARKER] = 1;
        bulk[start + prefix::SHARED_METADATA_RECORD..start + prefix::SHARED_METADATA_RECORD + 8]
            .copy_from_slice(&985u64.to_le_bytes());
        let mut metadata =
            crate::test_support::streams_test::generated_design_base_feature_metastream(&records);
        let scope_type = b"00000000-0000-0000-0000-000000001400";
        let type_at = metadata
            .windows(scope_type.len())
            .position(|bytes| bytes == scope_type)
            .unwrap();
        metadata[type_at..type_at + scope_type.len()]
            .copy_from_slice(b"D7015B81-AB4D-4C89-BED7-AA4411E1964A");
        let archive = crate::test_support::zip_test::f3d_with_smbh_and_instance_properties_and_design_with_metastream(
            &crate::test_support::smbh_geometry_test::synthetic_geometry_smbh(),
            &[], &bulk, &metadata,
        );
        let decoded = decode(archive);
        let feature = decoded
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.source_tag.as_deref() == Some(source_kind))
            .unwrap();
        assert!(matches!(feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::BaseFeature {
                bodies: BodySelection::Native(native),
            }) if Some(native.as_str()) == feature.native_ref.as_deref()));
        assert_eq!(
            feature.source_properties[&cadmpeg_core::nonblank_literal!("reference:0")],
            "985"
        );
        assert!(decoded
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == crate::loss::F3dLossCode::FeatureStoredBodiesUnbound.kind()));
    }
}

#[test]
fn derived_instance_with_unresolved_sources_keeps_a_neutral_component_selection() {
    use cadmpeg_ir::features::selections::InsertedComponents;
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
    for source_kind in ["DerivedInstance", "LocalizedScope!"] {
        let (mut bulk, records) =
            crate::test_support::streams_test::generated_design_copy_paste_bodies_bulkstream();
        let mut previous_kind = Vec::new();
        let mut next_kind = Vec::new();
        crate::test_support::lp_utf16(&mut previous_kind, "CopyPasteBodies");
        crate::test_support::lp_utf16(&mut next_kind, source_kind);
        assert_eq!(previous_kind.len(), next_kind.len());
        let at = bulk
            .windows(previous_kind.len())
            .position(|bytes| bytes == previous_kind)
            .unwrap();
        bulk[at..at + previous_kind.len()].copy_from_slice(&next_kind);
        let mut metadata =
            crate::test_support::streams_test::generated_design_copy_paste_bodies_metastream(
                &records,
            );
        let previous_type = b"00000000-0000-0000-0000-000000001400";
        let type_at = metadata
            .windows(previous_type.len())
            .position(|bytes| bytes == previous_type)
            .unwrap();
        metadata[type_at..type_at + previous_type.len()]
            .copy_from_slice(b"66E375EA-40F1-4115-8D83-0F23DD30593E");
        let archive = crate::test_support::zip_test::f3d_with_smbh_and_instance_properties_and_design_with_metastream(
            &crate::test_support::smbh_geometry_test::synthetic_geometry_smbh(), &[], &bulk, &metadata,
        );
        let decoded = decode(archive);
        let feature = decoded
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.source_tag.as_deref() == Some(source_kind))
            .unwrap();
        assert!(
            matches!(feature.evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::InsertComponents {
            components: InsertedComponents::Native { native }
        }) if Some(native.as_str()) == feature.native_ref.as_deref())
        );
        assert_eq!(
            feature.source_properties[&cadmpeg_core::nonblank_literal!("reference:0")],
            "1500"
        );
        assert_eq!(
            feature.source_properties[&cadmpeg_core::nonblank_literal!("reference:1")],
            "1600"
        );
        assert!(decoded.report().losses.iter().any(|loss| loss.code
            == crate::loss::F3dLossCode::FeatureDerivedInstanceComponentInsertAssumed.kind()));
        let mut scope = f3d_native(decoded.ir())
            .design_parameter_scopes
            .iter()
            .find(|scope| {
                scope.kind() == crate::records::feature::scope::DesignFeatureKind::DerivedInstance
            })
            .unwrap()
            .clone();
        scope.id = format!("f3d:fixture@{}", scope.id.trim_start_matches("f3d:"));
        let mut feature = feature.clone();
        feature.native_ref = Some(scope.id.clone());
        assert!(
            crate::design::components::project_derived_instance_features(
                std::slice::from_mut(&mut feature),
                std::slice::from_ref(&scope),
                &crate::design::components::paths::LocalOccurrencePaths::default(),
                &[],
            )
            .unwrap()
            .is_empty()
        );
        assert!(
            matches!(feature.evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::InsertComponents {
            components: InsertedComponents::Native { native }
        }) if native.as_str() == scope.id)
        );
    }
}

/// A commented document parameter keeps its comment through a source-less
/// write in the comment discriminated form, a decode, a second write and a
/// second decode.
#[test]
fn commented_document_parameter_keeps_its_comment_through_a_rewrite() {
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};

    const COMMENT: &str = "bolt spacing Ø";
    fn write(ir: &cadmpeg_ir::document::CadIr) -> Vec<u8> {
        let mut bytes = Vec::new();
        crate::F3dCodec
            .plan(EncodeInput::new(ir, None), TargetRequest::Unnamed)
            .and_then(|plan| plan.write_to(&mut bytes))
            .expect("source-less document parameter encode");
        bytes
    }
    fn comment(ir: &cadmpeg_ir::document::CadIr) -> Option<String> {
        let native = f3d_native(ir);
        let [parameter] = native.design_parameters.as_slice() else {
            panic!("expected the one document parameter");
        };
        assert_eq!(parameter.name(), "Spacing");
        parameter.comment().map(str::to_owned)
    }

    let mut source = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let parameter = crate::records::parameters::DesignParameter::try_from(
        crate::records::parameters::DesignParameterDraft {
            id: "f3d:FusionAssetName[Active]/Design1/BulkStream.dat:design-parameter#0".into(),
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("305".to_owned())
                .unwrap(),
            record_index: 700,
            source_ordinal: 0,
            source: crate::records::parameters::DesignParameterSource::User {
                source_kind: cadmpeg_core::nonblank_literal!("User Parameter"),
                family_discriminator: Some(crate::records::identity::Located {
                    value: crate::records::parameters::DesignParameterDiscriminator::Code0,
                    offset: 22,
                }),
            },
            expression: "12 mm".into(),
            expression_offset: 36,
            source_kind_offset: 70,
            unit: Some(crate::records::identity::RecordedValue {
                value: "mm".into(),
                offset: 110,
            }),
            name: "Spacing".into(),
            name_offset: 120,
            evaluated_value: 1.2,
            evaluated_value_offset: 150,
        },
    )
    .unwrap()
    .with_comment(Some(crate::records::identity::RecordedValue {
        value: COMMENT.into(),
        offset: 30,
    }))
    .unwrap();
    crate::test_support::native_test::f3d_native_mut(&mut source)
        .design_parameters
        .push(parameter);
    let crate::design::feature_project::ProjectedParameterDesign { parameters, .. } =
        crate::test_support::with_decode_context(|ctx| {
            let scopes = &[];
            let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
            crate::design::feature_project::project_parameter_design_with_edge_identities(
                ctx,
                &crate::design::feature_project::ProjectInputs {
                    native: &f3d_native(&source).design_parameters,
                    scopes,
                    timelines: &timelines,
                    ..Default::default()
                },
            )
            .expect("test projection has a synthetic exact timeline")
        });
    source.model.parameters = parameters;

    let decoded = decode(write(&source));
    assert_eq!(comment(decoded.ir()).as_deref(), Some(COMMENT));
    let (mut rewritten, _, _) = decoded.into_parts();
    rewritten.source = None;
    rewritten
        .set_native_unknowns(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            "f3d",
            &[],
        )
        .unwrap();
    let round_trip = decode(write(&rewritten));
    assert_eq!(comment(round_trip.ir()).as_deref(), Some(COMMENT));
}
