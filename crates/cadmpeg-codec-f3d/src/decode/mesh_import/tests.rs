use super::*;
use crate::records::feature::scope::DesignFeatureKind;

fn prefix(flagged: bool, bodies: &[u32]) -> Vec<u8> {
    let mut bytes = 3u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(b"714");
    bytes.extend_from_slice(&7u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 9]);
    bytes.push(u8::from(flagged));
    if flagged {
        bytes.extend_from_slice(&9u32.to_le_bytes());
    }
    bytes.extend_from_slice(&u32::try_from(bodies.len()).unwrap().to_le_bytes());
    for body in bodies {
        bytes.push(1);
        bytes.extend_from_slice(&u64::from(*body).to_le_bytes());
        bytes.extend_from_slice(&[0; 2]);
    }
    bytes
}

fn import_feature(native_ref: Option<String>) -> Feature {
    Feature {
        product_definition: None,
        id: cadmpeg_ir::features::FeatureId::mint("synthetic:model:feature#import").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("Base Mesh Feature".into()),
        source_text: None,
        source_content: Default::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Native {
                kind: "Base Mesh Feature".into(),
                parameters: Default::default(),
            }),
        ),
        native_ref,
    }
}

#[test]
fn import_body_prefix_preserves_order_and_flagged_word() {
    for flagged in [false, true] {
        let bytes = prefix(flagged, &[9, 3]);
        let parsed = crate::test_support::with_decode_context(|ctx| {
            read_bodies(ctx, View::over_retained(&bytes))
        })
        .unwrap();
        assert_eq!(parsed.bodies, [9, 3]);
        assert_eq!(parsed.prefix_word, flagged.then_some(9));
    }
}

#[test]
fn import_body_count_is_bounded_before_allocation() {
    let mut bytes = prefix(false, &[]);
    bytes[21..25].copy_from_slice(&u32::MAX.to_le_bytes());
    let parsed = crate::test_support::with_decode_context(|ctx| {
        read_bodies(ctx, View::over_retained(&bytes))
    });
    assert!(matches!(parsed, Err(ImportError::Framing)));
}

#[test]
fn truncated_import_prefix_preserves_its_source_location() {
    let bytes = [0u8; 13];
    let view = View::over_retained(&bytes).child(2, 13).unwrap();
    let parsed = crate::test_support::with_decode_context(|ctx| read_bodies(ctx, view));
    let Err(ImportError::Parse(error)) = parsed else {
        panic!("truncated prefix");
    };
    assert_eq!(error.location.offset, 13);
    assert_eq!(error.location.space, view.location().space);
}

#[test]
fn unjoined_imports_retain_decoded_body_sources_and_named_losses() {
    use std::io::{Cursor, Write};

    const ENTRY: &str = "FusionAssetName[Active]/Design1/BulkStream.dat";
    for (bodies, expected_code) in [
        (vec![9, 3], "feature.mesh-import-tessellations-unresolved"),
        (vec![], "feature.mesh-import-empty-source-assumed"),
        (vec![9, 9], "feature.mesh-import-source-unparsed"),
    ] {
        let mut bytes = prefix(false, &bodies);
        bytes.resize(128, 0);
        let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, stored);
        zip.start_file(ENTRY, stored).unwrap();
        zip.write_all(&bytes).unwrap();
        let archive = zip.finish().unwrap().into_inner();
        let scope_id = format!("f3d:{ENTRY}:design-parameter-scope#0");
        let scope = DesignParameterScope::empty(&scope_id, DesignFeatureKind::BaseMeshFeature, 7);
        let feature = import_feature(Some(scope_id.clone()));
        let mut features = vec![feature];
        let projection = MeshProjection {
            count: 0,
            tessellations_by_scope: Default::default(),
        };
        let mut losses = Vec::new();
        crate::test_support::zip_test::with_scan(&archive, |scan| {
            crate::test_support::with_decode_context(|ctx| {
                bind_features(ctx, scan, &mut features, &[scope], &projection, &mut losses)
            })
            .unwrap();
        });
        let FeatureDefinition::Operation(FeatureOperation::MeshImport {
            tessellations: MeshSelection::Native { native, members },
        }) = features[0].evaluation.definition()
        else {
            panic!("partial mesh import");
        };
        assert!(losses
            .iter()
            .any(|loss| loss.code.local_code() == expected_code));
        assert_eq!(native.as_str(), scope_id);
        if bodies == [9, 3] {
            assert_eq!(members.as_ref().unwrap().as_slice(), ["9", "3"]);
            assert_eq!(features[0].source_properties["mesh-body-reference:0"], "9");
            assert_eq!(features[0].source_properties["mesh-body-reference:1"], "3");
        } else {
            assert!(members.is_none());
            if bodies.is_empty() {
                assert_eq!(features[0].source_properties["mesh-body-count"], "0");
            } else {
                assert_eq!(features[0].source_properties["mesh-body-reference:0"], "9");
                assert_eq!(features[0].source_properties["mesh-body-reference:1"], "9");
            }
        }
        assert_eq!(features[0].source_tag.as_deref(), Some("Base Mesh Feature"));
    }
}

#[test]
fn imports_without_native_scopes_retain_typed_originating_features() {
    use std::io::Cursor;

    let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, stored);
    let archive = zip.finish().unwrap().into_inner();
    for native_ref in [
        None,
        Some(" ".to_owned()),
        Some("f3d:Synthetic/BulkStream.dat:design-parameter-scope#7".to_owned()),
    ] {
        let mut feature = import_feature(native_ref);
        let mut losses = Vec::new();
        let selection = crate::test_support::zip_test::with_scan(&archive, |scan| {
            crate::test_support::with_decode_context(|ctx| {
                native_source(ctx, scan, &mut feature, None, &mut losses)
            })
            .unwrap()
        });
        assert_eq!(
            selection,
            MeshSelection::Unresolved {
                feature: feature.id.clone()
            }
        );
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code.local_code(),
            "feature.mesh-import-source-unparsed"
        );
    }
}
