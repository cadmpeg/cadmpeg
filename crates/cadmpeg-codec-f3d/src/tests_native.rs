// SPDX-License-Identifier: Apache-2.0
//! Design-owned Form cage dispatcher tests. Remain on the crate-root router
//! until the design owner lands.
use super::*;

#[test]
fn form_dispatcher_binds_the_legacy_single_cage_gate() {
    let stream = "FusionAssetName[Active]/FusionDesignSegmentType1/BulkStream.dat";
    let mut bulk = Vec::new();
    let mut cage_list = vec![0; 100];
    cage_list[..4].copy_from_slice(&3u32.to_le_bytes());
    cage_list[4..7].copy_from_slice(b"355");
    cage_list[7..11].copy_from_slice(&205u32.to_le_bytes());
    cage_list[21] = 1;
    cage_list[22..30].copy_from_slice(&201u64.to_le_bytes());
    cage_list[32..36].copy_from_slice(&1u32.to_le_bytes());
    cage_list[36] = 1;
    cage_list[37..45].copy_from_slice(&971u64.to_le_bytes());
    cage_list[47..49].copy_from_slice(&[0xfc, 0]);
    bulk.extend_from_slice(&cage_list);

    let mut paired = vec![0; 15];
    paired[..4].copy_from_slice(&3u32.to_le_bytes());
    paired[4..7].copy_from_slice(b"262");
    paired[7..11].copy_from_slice(&205u32.to_le_bytes());
    bulk.extend_from_slice(&paired);

    let mut object = vec![0; 15];
    object[..4].copy_from_slice(&3u32.to_le_bytes());
    object[4..7].copy_from_slice(b"325");
    object[7..11].copy_from_slice(&971u32.to_le_bytes());
    bulk.extend_from_slice(&object);

    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    write_synthetic_manifests(&mut archive, stored);
    archive.start_file(stream, stored).unwrap();
    archive.write_all(&bulk).unwrap();
    let archive = archive.finish().unwrap().into_inner();

    let mut scope = crate::records::DesignParameterScope::empty(
        &format!("f3d:{stream}:scope#201"),
        "Form",
        201,
    );
    scope.reference_members = vec![205];
    let feature_id = crate::ids::neutral_feature_id(&scope);
    let mut features = vec![cadmpeg_ir::features::Feature {
        id: feature_id,
        ordinal: 0,
        name: None,
        suppressed: None,
        parent: None,
        dependencies: Vec::new(),
        source_properties: Default::default(),
        source_tag: Some("Form".into()),
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: cadmpeg_ir::features::FeatureDefinition::Native {
            kind: "Form".into(),
            parameters: Default::default(),
            properties: Default::default(),
        },
        native_ref: Some(scope.id.clone()),
    }];
    let cages = [cadmpeg_ir::SubdSurface {
        id: cadmpeg_ir::ids::SubdId("f3d:model:subd#1".into()),
        scheme: cadmpeg_ir::subd::SubdScheme::CatmullClark,
        vertices: Vec::new(),
        edges: Vec::new(),
        faces: Vec::new(),
        source_object: None,
    }];

    crate::tests::with_scan(&archive, |scan| {
        crate::design::feature_project::bind_form_cages(
            scan,
            std::slice::from_ref(&scope),
            &mut features,
            &cages,
        )
    })
    .expect("legacy Form cage binding");
    assert_eq!(
        features[0].definition,
        cadmpeg_ir::features::FeatureDefinition::Form {
            cages: vec![cages[0].id.clone()],
        }
    );
}

#[test]
fn form_dispatcher_binds_a_unique_long_cage_list() {
    let stream = "FusionAssetName[Active]/FusionDesignSegmentType1/BulkStream.dat";
    let mut cage_list = vec![0; 99];
    cage_list[..4].copy_from_slice(&3u32.to_le_bytes());
    cage_list[4..7].copy_from_slice(b"415");
    cage_list[7..11].copy_from_slice(&205u32.to_le_bytes());
    cage_list[21] = 1;
    cage_list[22..30].copy_from_slice(&201u64.to_le_bytes());
    cage_list[32..36].copy_from_slice(&1u32.to_le_bytes());
    cage_list[36] = 1;
    cage_list[37..45].copy_from_slice(&971u64.to_le_bytes());
    let mut paired = vec![0; 15];
    paired[..4].copy_from_slice(&3u32.to_le_bytes());
    paired[4..7].copy_from_slice(b"258");
    paired[7..11].copy_from_slice(&205u32.to_le_bytes());
    let mut bulk = cage_list;
    bulk.extend_from_slice(&paired);

    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    write_synthetic_manifests(&mut archive, stored);
    archive.start_file(stream, stored).unwrap();
    archive.write_all(&bulk).unwrap();
    let archive = archive.finish().unwrap().into_inner();

    let mut scope = crate::records::DesignParameterScope::empty(
        &format!("f3d:{stream}:scope#201"),
        "Form",
        201,
    );
    scope.reference_members = vec![205];
    let feature_id = crate::ids::neutral_feature_id(&scope);
    let mut features = vec![cadmpeg_ir::features::Feature {
        id: feature_id,
        ordinal: 0,
        name: None,
        suppressed: None,
        parent: None,
        dependencies: Vec::new(),
        source_properties: Default::default(),
        source_tag: Some("Form".into()),
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: cadmpeg_ir::features::FeatureDefinition::Native {
            kind: "Form".into(),
            parameters: Default::default(),
            properties: Default::default(),
        },
        native_ref: Some(scope.id.clone()),
    }];
    let cages = [cadmpeg_ir::SubdSurface {
        id: cadmpeg_ir::ids::SubdId("f3d:model:subd#1".into()),
        scheme: cadmpeg_ir::subd::SubdScheme::CatmullClark,
        vertices: Vec::new(),
        edges: Vec::new(),
        faces: Vec::new(),
        source_object: None,
    }];

    crate::tests::with_scan(&archive, |scan| {
        crate::design::feature_project::bind_form_cages(
            scan,
            std::slice::from_ref(&scope),
            &mut features,
            &cages,
        )
    })
    .expect("long Form cage binding");
    assert_eq!(
        features[0].definition,
        cadmpeg_ir::features::FeatureDefinition::Form {
            cages: vec![cages[0].id.clone()],
        }
    );
}
