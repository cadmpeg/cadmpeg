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

#[test]
fn base_feature_without_result_body_binding_keeps_a_neutral_snapshot() {
    for source_kind in [
        "Base Feature",
        "Elemento base",
        "Temel Özellik",
        "Basiselement",
    ] {
        use crate::layout::base_feature_wide_zero_body as prefix;
        use cadmpeg_ir::features::{BodySelection, FeatureDefinition, FeatureOperation};
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
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, InsertedComponents};
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
        scope.id = format!("f3d:xref/fixture/{}", scope.id.trim_start_matches("f3d:"));
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
