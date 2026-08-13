// SPDX-License-Identifier: Apache-2.0
//! Design-domain synthetic tests and fixtures.

use super::*;

#[test]
fn validation_requires_timeline_items_to_resolve_through_the_type_table() {
    let meta_stream = "f3d:FusionAssetName[Active]/Design1/MetaStream.dat";
    let bulk_entry = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let design_type = |id: &str, type_guid: &str, entities: Vec<u64>| crate::records::SegmentType {
        id: id.into(),
        byte_offset: 0,
        type_guid: type_guid.into(),
        type_guid_offset: 4,
        base_type_guid: (type_guid == crate::design::decode::meta::FEATURE_TIMELINE_TYPE_GUID)
            .then(|| crate::design::decode::meta::FEATURE_TIMELINE_BASE_TYPE_GUID.into()),
        base_type_guid_offset: (type_guid
            == crate::design::decode::meta::FEATURE_TIMELINE_TYPE_GUID)
            .then_some(8),
        version: if type_guid == crate::design::decode::meta::FEATURE_TIMELINE_TYPE_GUID {
            crate::design::decode::meta::FEATURE_TIMELINE_TYPE_VERSIONS[1]
        } else {
            1
        },
        version_offset: 44,
        module: crate::records::DESIGN_MODULE_FUSION.into(),
        entity_id_offsets: vec![100; entities.len()],
        entity_ids: entities,
    };
    let mut native = crate::native::F3dNative {
        design_types: vec![
            design_type(
                &format!("{meta_stream}:design-type#0"),
                crate::design::decode::meta::FEATURE_TIMELINE_TYPE_GUID,
                vec![35],
            ),
            design_type(
                &format!("{meta_stream}:design-type#1"),
                "11111111-2222-3333-4444-555555555555",
                vec![17, 101],
            ),
        ],
        design_feature_timelines: vec![crate::records::DesignFeatureTimeline {
            id: crate::ids::native_design_feature_timeline_id(bulk_entry, 200),
            byte_offset: 200,
            class_tag: "256".into(),
            record_index: 35,
            source_ordinal: 0,
            frame_length: 60,
            context_record_index: 17,
            context_record_index_offset: 220,
            item_count_offset: 240,
            item_record_indices: vec![101],
            item_record_index_offsets: vec![245],
        }],
        ..crate::native::F3dNative::default()
    };
    let mut ir = cadmpeg_ir::examples::unit_cube();
    native.store(ir.native.namespace_mut("f3d")).unwrap();
    let findings = crate::validate::validate_native(&ir);
    assert!(
        !findings.iter().any(|finding| {
            finding.message.contains("feature timeline")
                || finding.message.contains("feature-timeline")
        }),
        "{findings:#?}"
    );

    let mut duplicate_type_owner = native.clone();
    duplicate_type_owner.design_types[1].entity_ids.push(35);
    duplicate_type_owner.design_types[1]
        .entity_id_offsets
        .push(108);
    duplicate_type_owner
        .store(ir.native.namespace_mut("f3d"))
        .unwrap();
    assert!(crate::validate::validate_native(&ir).iter().any(|finding| {
        finding.entity.as_deref()
            == Some(duplicate_type_owner.design_feature_timelines[0].id.as_str())
            && finding.message == "Fusion Design feature timeline has an invalid typed frame"
    }));

    let mut invalid_offsets = native.clone();
    invalid_offsets.design_feature_timelines[0].item_record_index_offsets[0] = 244;
    invalid_offsets
        .store(ir.native.namespace_mut("f3d"))
        .unwrap();
    assert!(crate::validate::validate_native(&ir).iter().any(|finding| {
        finding.entity.as_deref() == Some(invalid_offsets.design_feature_timelines[0].id.as_str())
            && finding.message == "Fusion Design feature timeline has an invalid typed frame"
    }));

    native.design_feature_timelines[0].item_record_indices[0] = 102;
    native.store(ir.native.namespace_mut("f3d")).unwrap();
    assert!(crate::validate::validate_native(&ir).iter().any(|finding| {
        finding.entity.as_deref() == Some(native.design_feature_timelines[0].id.as_str())
            && finding.message == "Fusion Design feature timeline has an invalid typed frame"
    }));
}

#[test]
fn generated_source_less_writes_design_type_metastream() {
    use crate::records::SegmentType;

    let mut source_less = cadmpeg_ir::examples::unit_cube();
    let mut native = f3d_native_mut(&mut source_less);
    native.design_types = vec![
        SegmentType {
            id: "generated:design-type#0".into(),
            byte_offset: 0,
            module: "Fusion".to_owned(),
            entity_ids: vec![1, 2],
            entity_id_offsets: Vec::new(),
            type_guid: "11111111-2222-3333-4444-555555555555".into(),
            type_guid_offset: 0,
            base_type_guid: None,
            base_type_guid_offset: None,
            version: 7,
            version_offset: 0,
        },
        SegmentType {
            id: "generated:design-type#1".into(),
            byte_offset: 0,
            module: crate::records::DESIGN_MODULE_SKETCH.to_owned(),
            entity_ids: vec![277],
            entity_id_offsets: Vec::new(),
            type_guid: "22222222-3333-4444-5555-666666666666".into(),
            type_guid_offset: 0,
            base_type_guid: Some("11111111-2222-3333-4444-555555555555".into()),
            base_type_guid_offset: None,
            version: 9,
            version_offset: 0,
        },
        SegmentType {
            id: "generated:design-type#2".into(),
            byte_offset: 0,
            module: "FutureFeature".to_owned(),
            entity_ids: vec![999],
            entity_id_offsets: Vec::new(),
            type_guid: "33333333-4444-5555-6666-777777777777".into(),
            type_guid_offset: 0,
            base_type_guid: Some("11111111-2222-3333-4444-555555555555".into()),
            base_type_guid_offset: None,
            version: 11,
            version_offset: 0,
        },
    ];

    drop(native);
    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less Design MetaStream encode");
    let mut guid_module = source_less.clone();
    f3d_native_mut(&mut guid_module).design_types[2].module =
        "11111111-2222-3333-4444-555555555555".into();
    let error = F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &guid_module,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("a GUID-shaped Design module name must not be emitted");
    assert!(error
        .to_string()
        .contains("Design type module name is GUID-shaped"));
    f3d_native_mut(&mut source_less).design_types[0].base_type_guid =
        Some("22222222-3333-4444-5555-666666666666".into());
    let error = F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("a cyclic Design type hierarchy must not be emitted");
    assert!(error
        .to_string()
        .contains("Design type hierarchy contains a cycle"));
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less Design MetaStream round trip");
    let types = &f3d_native(round_trip.ir()).design_types;
    assert_eq!(types.len(), 3);
    let fusion = types
        .iter()
        .find(|design_type| design_type.type_guid == "11111111-2222-3333-4444-555555555555")
        .expect("Fusion type");
    assert_eq!(fusion.module, "Fusion");
    assert_eq!(fusion.entity_ids, [1, 2]);
    assert_eq!(fusion.version, 7);
    assert_eq!(fusion.base_type_guid, None);
    let sketch = types
        .iter()
        .find(|design_type| design_type.module == crate::records::DESIGN_MODULE_SKETCH)
        .expect("sketch-module type");
    assert_eq!(sketch.entity_ids, [277]);
    assert_eq!(
        sketch.base_type_guid.as_deref(),
        Some("11111111-2222-3333-4444-555555555555")
    );
    assert_eq!(sketch.version, 9);
    let future = types
        .iter()
        .find(|design_type| design_type.module == "FutureFeature")
        .expect("forward-compatible module");
    assert_eq!(future.entity_ids, [999]);
    assert_eq!(future.version, 11);
}

#[test]
fn generated_source_less_writes_design_recipes_and_persistent_references() {
    use crate::records::{
        ConstructionRecipe, ConstructionRecipeKind, LostEdgeReference, PersistentReference,
        PersistentReferenceKind,
    };

    let mut source_less = cadmpeg_ir::examples::unit_cube();
    let mut native = f3d_native_mut(&mut source_less);
    native.construction_recipes = [
        ConstructionRecipeKind::Body,
        ConstructionRecipeKind::Face,
        ConstructionRecipeKind::BoundedFace,
        ConstructionRecipeKind::Edge,
        ConstructionRecipeKind::Vertex,
    ]
    .into_iter()
    .enumerate()
    .map(|(ordinal, kind)| ConstructionRecipe {
        id: format!("generated:recipe#{ordinal}"),
        byte_offset: 0,
        record_index_offset: None,
        kind,
        design_id: Some(format!("{}", 320 + ordinal)),
        design_id_offset: None,
        design_selector: None,
        recipe_index: 0,
        record_index: 100 + i32::try_from(ordinal).unwrap(),
    })
    .collect();
    native.persistent_references = vec![
        PersistentReference {
            id: "generated:persistent-reference#0".into(),
            byte_offset: 0,
            value_offset: 0,
            kind: PersistentReferenceKind::Point,
            value: 900,
        },
        PersistentReference {
            id: "generated:persistent-reference#1".into(),
            byte_offset: 0,
            value_offset: 0,
            kind: PersistentReferenceKind::CurvePrimary,
            value: 100,
        },
        PersistentReference {
            id: "generated:persistent-reference#2".into(),
            byte_offset: 0,
            value_offset: 0,
            kind: PersistentReferenceKind::CurveSecondary,
            value: 500,
        },
    ];
    native.lost_edge_references = vec![LostEdgeReference {
        id: "generated:lost-edge-reference#0".into(),
        record_byte_offset: 0,
        class_tag_offset: 0,
        class_tag: "419".into(),
        record_index: 4645,
        record_index_offset: 0,
        byte_offset: 0,
        next_byte_offset: 0,
        next_class_tag: "419".into(),
        next_record_index: 4646,
    }];

    drop(native);
    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less Design BulkStream encode");
    f3d_native_mut(&mut source_less).construction_recipes[0].recipe_index = 1;
    let error = F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("recipe group indices must not be renumbered");
    assert!(error
        .to_string()
        .contains("has noncontiguous group index 1"));
    let mut archive = zip::ZipArchive::new(Cursor::new(&encoded)).expect("generated F3D ZIP");
    let mut bulkstream = Vec::new();
    archive
        .by_name("FusionAssetName[Active]/Design1/BulkStream.dat")
        .expect("generated Design BulkStream")
        .read_to_end(&mut bulkstream)
        .expect("read generated Design BulkStream");
    for name in [
        b"body_recipe_data".as_slice(),
        b"face_recipe_data".as_slice(),
        b"bounded_face_recipe_data".as_slice(),
        b"edge_recipe_data".as_slice(),
        b"vertex_recipe_data".as_slice(),
    ] {
        let offset = bulkstream
            .windows(name.len())
            .position(|window| window == name)
            .expect("generated recipe name");
        assert_eq!(
            u32::from_le_bytes(bulkstream[offset - 4..offset].try_into().unwrap()),
            u32::try_from(name.len()).unwrap()
        );
        let payload = offset + name.len();
        assert_eq!(
            i64::from_le_bytes(bulkstream[payload..payload + 8].try_into().unwrap()),
            -1
        );
        assert_eq!(
            (0..5)
                .map(|ordinal| {
                    let at = payload + 8 + ordinal * 4;
                    i32::from_le_bytes(bulkstream[at..at + 4].try_into().unwrap())
                })
                .collect::<Vec<_>>(),
            [2, 0, -1, 1, -1]
        );
    }
    for name in [
        b"pt_tag".as_slice(),
        b"crv_primary_id".as_slice(),
        b"crv_secondary_id".as_slice(),
    ] {
        let offset = bulkstream
            .windows(name.len())
            .position(|window| window == name)
            .expect("generated persistent-reference name");
        let payload = offset + name.len();
        assert_eq!(
            &bulkstream[payload..payload + 8],
            &[2, 0, 0, 0, 14, 0, 0, 0]
        );
        assert_eq!(&bulkstream[payload + 8..payload + 22], &[0; 14]);
        assert_eq!(
            u32::from_le_bytes(bulkstream[payload + 22..payload + 26].try_into().unwrap()),
            23
        );
        assert_eq!(
            &bulkstream[payload + 26..payload + 49],
            b"IntrinsicMetaTypeuint64"
        );
    }
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less Design BulkStream round trip");
    let native = f3d_native(round_trip.ir());
    assert_eq!(native.construction_recipes.len(), 5);
    let body_recipe = native
        .construction_recipes
        .iter()
        .find(|recipe| recipe.kind == ConstructionRecipeKind::Body)
        .expect("body recipe");
    assert_eq!(body_recipe.record_index, 100);
    assert_eq!(body_recipe.design_id.as_deref(), Some("320"));
    assert!(native
        .construction_recipes
        .iter()
        .any(|recipe| recipe.kind == ConstructionRecipeKind::BoundedFace));
    let bounded = native
        .construction_recipes
        .iter()
        .find(|recipe| recipe.kind == ConstructionRecipeKind::BoundedFace)
        .expect("bounded-face recipe");
    assert_eq!(bounded.design_id.as_deref(), Some("322"));
    assert_eq!(bounded.record_index, 102);
    assert_eq!(native.persistent_references.len(), 3);
    assert_eq!(
        native
            .persistent_references
            .iter()
            .map(|reference| reference.value)
            .collect::<Vec<_>>(),
        [900, 100, 500]
    );
    assert_eq!(
        native.persistent_references[1].kind,
        PersistentReferenceKind::CurvePrimary
    );
    assert_eq!(native.lost_edge_references.len(), 1);
    assert_eq!(native.lost_edge_references[0].class_tag, "419");
    assert_eq!(native.lost_edge_references[0].record_index, 4645);
    assert_eq!(native.lost_edge_references[0].next_class_tag, "419");
    assert_eq!(native.lost_edge_references[0].next_record_index, 4646);
}

#[test]
fn generated_source_less_writes_design_ownership_and_record_headers() {
    use crate::records::{DesignBodyMember, DesignEntityHeader, DesignRecordHeader, SegmentType};

    let mut source_less = cadmpeg_ir::examples::unit_cube();
    let mut native = f3d_native_mut(&mut source_less);
    native.design_types = vec![SegmentType {
        id: "generated:design-type#0".into(),
        byte_offset: 0,
        module: crate::records::DESIGN_MODULE_SKETCH.to_owned(),
        entity_ids: vec![277],
        entity_id_offsets: Vec::new(),
        type_guid: "22222222-3333-4444-5555-666666666666".into(),
        type_guid_offset: 0,
        base_type_guid: None,
        base_type_guid_offset: None,
        version: 4,
        version_offset: 0,
    }];
    native.design_body_members = vec![
        DesignBodyMember {
            id: "generated:body-member#0".into(),
            byte_offset: 0,
            entity_suffix: 985,
            flags: 0,
        },
        DesignBodyMember {
            id: "generated:body-member#1".into(),
            byte_offset: 0,
            entity_suffix: 8422,
            flags: 3,
        },
    ];
    native.design_entity_headers = vec![DesignEntityHeader {
        id: "generated:entity-header#0".into(),
        byte_offset: 0,
        entity_suffix: 277,
        entity_id: "0_277".into(),
        class_tag: "256".into(),
        optional_slot_present: true,
        module: Some(crate::records::DESIGN_MODULE_SKETCH.to_owned()),
        record_reference: Some(584),
        record_reference_offset: None,
        declared_reference_count: Some(2),
        reference_indices: vec![33, 44],
        reference_offsets: Vec::new(),
        member_indices: Vec::new(),
        member_offsets: Vec::new(),
    }];
    native.design_record_headers = vec![
        DesignRecordHeader {
            id: "generated:record-header#0".into(),
            record_index: 33,
            class_tag: "350".into(),
            byte_offset: 0,
        },
        DesignRecordHeader {
            id: "generated:record-header#1".into(),
            record_index: 44,
            class_tag: "351".into(),
            byte_offset: 0,
        },
    ];

    drop(native);
    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less Design ownership encode");
    f3d_native_mut(&mut source_less).design_entity_headers[0].declared_reference_count = Some(3);
    let mut normalized = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut normalized))
        .expect("source sketch reference count is regenerated");
    let normalized = F3dCodec
        .decode(&mut Cursor::new(normalized), &DecodeOptions::default())
        .expect("regenerated sketch reference count round trip");
    assert_eq!(
        f3d_native(normalized.ir()).design_entity_headers[0].declared_reference_count,
        Some(2)
    );
    {
        let mut native = f3d_native_mut(&mut source_less);
        native.design_entity_headers[0].declared_reference_count = Some(2);
        native.design_entity_headers[0].module = Some("Body".to_owned());
    }
    let error = F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("cross-stream modules must not diverge");
    assert!(error
        .to_string()
        .contains("module conflicts with MetaStream ownership"));
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less Design ownership round trip");
    let native = f3d_native(round_trip.ir());
    assert_eq!(native.design_body_members.len(), 2);
    assert_eq!(native.design_body_members[0].entity_suffix, 985);
    assert_eq!(native.design_body_members[1].flags, 3);
    assert_eq!(native.design_entity_headers.len(), 1);
    assert_eq!(native.design_entity_headers[0].entity_id, "0_277");
    assert_eq!(native.design_entity_headers[0].record_reference, Some(584));
    assert_eq!(native.design_entity_headers[0].reference_indices, [33, 44]);
    assert_eq!(native.design_record_headers.len(), 2);
    assert_eq!(native.design_record_headers[0].record_index, 33);
    assert_eq!(native.design_record_headers[1].class_tag, "351");
}

#[test]
fn generated_source_less_writes_sketch_points_curves_and_constraints() {
    use crate::records::{
        DesignEntityHeader, SegmentType, SketchConstraintKind, SketchCurveGeometry,
        SketchCurveIdentity, SketchPoint, SketchRelation,
    };
    use cadmpeg_ir::math::{Point2, Point3, Vector3};

    let mut source_less = cadmpeg_ir::examples::unit_cube();
    let mut native = f3d_native_mut(&mut source_less);
    native.design_types = vec![
        SegmentType {
            id: "generated:sketch-type-00-object#0".into(),
            byte_offset: 0,
            module: crate::records::DESIGN_MODULE_SKETCH.to_owned(),
            entity_ids: vec![277],
            entity_id_offsets: Vec::new(),
            type_guid: crate::design::decode::sketch::SKETCH_CONTAINER_TYPE_GUID.into(),
            type_guid_offset: 0,
            base_type_guid: None,
            base_type_guid_offset: None,
            version: 1,
            version_offset: 0,
        },
        SegmentType {
            id: "generated:sketch-type-01-relation#0".into(),
            byte_offset: 1,
            module: crate::records::DESIGN_MODULE_SKETCH.to_owned(),
            entity_ids: vec![33],
            entity_id_offsets: Vec::new(),
            type_guid: "60403D47-0C49-49B0-BDE8-1679608164A2".into(),
            type_guid_offset: 0,
            base_type_guid: None,
            base_type_guid_offset: None,
            version: 1,
            version_offset: 0,
        },
        SegmentType {
            id: "generated:sketch-type-02-point#0".into(),
            byte_offset: 2,
            module: "Geometry".into(),
            entity_ids: vec![100],
            entity_id_offsets: Vec::new(),
            type_guid: "C2CEDAE7-1716-47C1-B7B1-07B70081D0FB".into(),
            type_guid_offset: 0,
            base_type_guid: None,
            base_type_guid_offset: None,
            version: 11,
            version_offset: 0,
        },
        SegmentType {
            id: "generated:sketch-type-03-line#0".into(),
            byte_offset: 3,
            module: "Geometry".into(),
            entity_ids: vec![600],
            entity_id_offsets: Vec::new(),
            type_guid: "DCA267ED-D615-4934-B64F-AD805E8003E2".into(),
            type_guid_offset: 0,
            base_type_guid: None,
            base_type_guid_offset: None,
            version: 2,
            version_offset: 0,
        },
        SegmentType {
            id: "generated:sketch-type-04-circular#0".into(),
            byte_offset: 4,
            module: "Geometry".into(),
            entity_ids: vec![601],
            entity_id_offsets: Vec::new(),
            type_guid: "F0130424-8B7E-4092-93C9-1CA807482534".into(),
            type_guid_offset: 0,
            base_type_guid: None,
            base_type_guid_offset: None,
            version: 0,
            version_offset: 0,
        },
        SegmentType {
            id: "generated:sketch-type-05-nurbs#0".into(),
            byte_offset: 5,
            module: crate::records::DESIGN_MODULE_SKETCH.to_owned(),
            entity_ids: vec![602],
            entity_id_offsets: Vec::new(),
            type_guid: "D82E012F-6DDD-4AED-BDE1-C0F7F9100B9B".into(),
            type_guid_offset: 0,
            base_type_guid: None,
            base_type_guid_offset: None,
            version: 3,
            version_offset: 0,
        },
        SegmentType {
            id: "generated:sketch-type-06-point-companion#0".into(),
            byte_offset: 6,
            module: "Geometry".into(),
            entity_ids: vec![101],
            entity_id_offsets: Vec::new(),
            type_guid: crate::design::decode::sketch::SKETCH_POINT_COMPANION_TYPE
                .0
                .into(),
            type_guid_offset: 0,
            base_type_guid: None,
            base_type_guid_offset: None,
            version: crate::design::decode::sketch::SKETCH_POINT_COMPANION_TYPE.1,
            version_offset: 0,
        },
    ];
    native.design_entity_headers = vec![DesignEntityHeader {
        id: "generated:sketch-header#0".into(),
        byte_offset: 0,
        entity_suffix: 277,
        entity_id: "0_277".into(),
        class_tag: "256".into(),
        optional_slot_present: true,
        module: Some(crate::records::DESIGN_MODULE_SKETCH.to_owned()),
        record_reference: Some(584),
        record_reference_offset: None,
        declared_reference_count: Some(1),
        reference_indices: vec![33],
        reference_offsets: Vec::new(),
        member_indices: Vec::new(),
        member_offsets: Vec::new(),
    }];
    native.sketch_points = vec![SketchPoint {
        id: "generated:sketch-point#0".into(),
        record_index: 100,
        owner_reference: Some(277),
        class_tag: "258".into(),
        byte_offset: 0,
        coordinate_offset: 89,
        entity_genesis: Some(900),
        record_form: crate::records::SketchPointRecordForm::Version11 {
            padded_paired_reference: false,
        },
        persistent_id: Some(500),
        paired_reference: 101,
        flags: [0; 8],
        coordinates: Point2::new(12.5, -25.0),
        depth: 0.0,
        closure: Some(crate::records::SketchPointClosure {
            selector: 0,
            state: 1,
        }),
        companion: Some(crate::records::SketchPointCompanion {
            prefix_present_zero: false,
            reference_encoding: crate::records::SketchPointCompanionReferenceEncoding::SameSegment,
            incident_curves: Vec::new(),
        }),
    }];
    native.sketch_curve_identities = vec![
        SketchCurveIdentity {
            id: "generated:sketch-curve#0".into(),
            record_index: 600,
            owner_reference: Some(277),
            class_tag: "259".into(),
            byte_offset: 0,
            geometry_offset: 133,
            entity_genesis: Some(901),
            primary_id: 700,
            secondary_id: 701,
            geometry: Some(SketchCurveGeometry::Line {
                start: Point3::new(10.0, 20.0, 0.0),
                end: Point3::new(40.0, 20.0, 0.0),
                direction: Vector3::new(1.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
            }),
        },
        SketchCurveIdentity {
            id: "generated:sketch-curve#1".into(),
            record_index: 601,
            owner_reference: Some(277),
            class_tag: "260".into(),
            byte_offset: 0,
            geometry_offset: 133,
            entity_genesis: None,
            primary_id: 702,
            secondary_id: 703,
            geometry: Some(SketchCurveGeometry::Arc {
                center: Point3::new(5.0, 6.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                reference_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 30.0,
                start_angle: 0.25,
                end_angle: 2.5,
            }),
        },
        SketchCurveIdentity {
            id: "generated:sketch-curve#2".into(),
            record_index: 602,
            owner_reference: Some(277),
            class_tag: "261".into(),
            byte_offset: 0,
            geometry_offset: 133,
            entity_genesis: None,
            primary_id: 704,
            secondary_id: 705,
            geometry: Some(SketchCurveGeometry::Nurbs {
                carrier_reference: None,
                subtype_class_tag: "365".into(),
                subtype_record_index: 602,
                degree: 2,
                fit_tolerance: 1.0e-8,
                scalar_width: 8,
                knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                weights: vec![1.0, 0.8, 1.0],
                control_points: vec![
                    Point3::new(0.0, 0.0, 0.0),
                    Point3::new(10.0, 20.0, 0.0),
                    Point3::new(30.0, 10.0, 0.0),
                ],
            }),
        },
    ];
    native.sketch_relations = vec![SketchRelation {
        id: "generated:sketch-relation#0".into(),
        record_index: 33,
        class_tag: "257".into(),
        byte_offset: 0,
        state_offset: 0,
        owner_reference: 277,
        owner_entity_id: String::new(),
        owner_reference_offset: 0,
        auxiliary_references: Vec::new(),
        auxiliary_reference_offsets: Vec::new(),
        rectangular_counted_reference_count: None,
        members: vec![100, 600],
        resolved_members: Vec::new(),
        member_offsets: Vec::new(),
        state: 0x11,
        constraint_kinds: vec![
            SketchConstraintKind::Coincident,
            SketchConstraintKind::Parallel,
        ],
        unknown_constraint_bits: 0,
        member_relation_ordinals: Vec::new(),
        entity_genesis: None,
        pattern: None,
        return_members: vec![600, 100],
        resolved_return_members: Vec::new(),
        return_member_offsets: Vec::new(),
        raw_bytes: Vec::new(),
    }];

    let expected_geometries = native
        .sketch_curve_identities
        .iter()
        .map(|curve| curve.geometry.clone().unwrap())
        .collect::<Vec<_>>();
    drop(native);
    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less sketch BulkStream encode");
    let mut extended_source_less = source_less.clone();
    {
        let mut archive = zip::ZipArchive::new(Cursor::new(&encoded)).expect("generated F3D ZIP");
        let mut bulkstream = Vec::new();
        archive
            .by_name("FusionAssetName[Active]/Design1/BulkStream.dat")
            .expect("generated Design BulkStream")
            .read_to_end(&mut bulkstream)
            .expect("read generated Design BulkStream");
        let mut companion = Vec::new();
        companion.extend_from_slice(&3u32.to_le_bytes());
        companion.extend_from_slice(b"262");
        companion.extend_from_slice(&101u32.to_le_bytes());
        companion.extend_from_slice(&[0; 15]);
        companion.push(1);
        companion.extend_from_slice(&100u64.to_le_bytes());
        companion.extend_from_slice(&[0; 2]);
        assert_eq!(companion.len(), 37);
        assert!(bulkstream
            .windows(companion.len())
            .any(|window| window == companion));
    }
    f3d_native_mut(&mut source_less).sketch_points[0].owner_reference = None;
    let error = F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("source-less points require their direct owner backlink");
    assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
    f3d_native_mut(&mut source_less).sketch_points[0].owner_reference = Some(277);
    f3d_native_mut(&mut source_less).design_types[6]
        .entity_ids
        .clear();
    let error = F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("source-less points require a registered inverse companion");
    assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
    f3d_native_mut(&mut source_less).design_types[6].entity_ids = vec![101];
    f3d_native_mut(&mut source_less).design_types[2].version = 10;
    let error = F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("source-less points require the current writable class version");
    assert!(matches!(error, cadmpeg_core::CodecError::NotImplemented(_)));
    f3d_native_mut(&mut source_less).design_types[2].version = 11;
    {
        let relation = &mut f3d_native_mut(&mut source_less).sketch_relations[0];
        relation.members = vec![100, 600, 100, 600, 100, 600, 100, 600];
        relation.return_members = relation.members.iter().rev().copied().collect();
    }
    let mut variable_relation = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut variable_relation))
        .expect("source-less variable-width sketch relation encode");
    let variable_round_trip = F3dCodec
        .decode(
            &mut Cursor::new(variable_relation),
            &DecodeOptions::default(),
        )
        .expect("source-less variable-width sketch relation round trip");
    assert_eq!(
        f3d_native(variable_round_trip.ir()).sketch_relations[0].members,
        [100, 600, 100, 600, 100, 600, 100, 600]
    );
    assert!(
        f3d_native(variable_round_trip.ir()).sketch_relations[0]
            .raw_bytes
            .len()
            > 101
    );
    {
        let relation = &mut f3d_native_mut(&mut source_less).sketch_relations[0];
        relation.members = vec![100, 600];
        relation.return_members = vec![600, 100];
    }
    f3d_native_mut(&mut source_less).sketch_relations[0].owner_reference = 999;
    let error = F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("relations with missing sketch owners must not disappear");
    assert!(error
        .to_string()
        .contains("references missing sketch owner"));
    {
        let mut native = f3d_native_mut(&mut source_less);
        native.sketch_relations[0].owner_reference = 277;
        native.sketch_points[0].record_index = 600;
    }
    let error = F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("duplicate typed sketch indices must not be deduplicated");
    assert!(error.to_string().contains("share record index 600"));
    f3d_native_mut(&mut source_less).sketch_points[0].record_index = 100;
    f3d_native_mut(&mut source_less).sketch_relations[0].constraint_kinds =
        vec![SketchConstraintKind::Horizontal];
    let error = F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("inconsistent generated sketch constraint mask must be rejected");
    assert!(error
        .to_string()
        .contains("mask inconsistent with its typed constraint kinds"));
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less sketch BulkStream round trip");
    let native = f3d_native(round_trip.ir());
    assert_eq!(native.sketch_points.len(), 1);
    assert_eq!(native.sketch_points[0].persistent_id, Some(500));
    assert_eq!(native.sketch_points[0].entity_genesis, Some(900));
    assert_eq!(native.sketch_points[0].coordinate_offset, 141);
    assert_eq!(native.sketch_points[0].owner_reference, Some(277));
    assert_eq!(native.sketch_points[0].depth, 0.0);
    assert_eq!(
        native.sketch_points[0].closure,
        Some(crate::records::SketchPointClosure {
            selector: 0,
            state: 1,
        })
    );
    assert_eq!(
        native.sketch_points[0].companion,
        Some(crate::records::SketchPointCompanion {
            prefix_present_zero: false,
            reference_encoding: crate::records::SketchPointCompanionReferenceEncoding::SameSegment,
            incident_curves: Vec::new(),
        })
    );
    assert_eq!(
        native.sketch_points[0].coordinates,
        Point2::new(12.5, -25.0)
    );
    assert_eq!(native.sketch_curve_identities.len(), 3);
    let genesis_curve = native
        .sketch_curve_identities
        .iter()
        .find(|curve| curve.primary_id == 700)
        .expect("genesis curve");
    assert_eq!(genesis_curve.entity_genesis, Some(901));
    assert_eq!(genesis_curve.geometry_offset, 185);
    assert_eq!(genesis_curve.owner_reference, Some(277));
    assert!(native
        .sketch_curve_identities
        .iter()
        .all(|curve| curve.owner_reference == Some(277)));
    for expected in expected_geometries {
        assert!(native
            .sketch_curve_identities
            .iter()
            .any(|curve| curve.geometry.as_ref() == Some(&expected)));
    }
    assert_eq!(native.sketch_relations.len(), 1);
    assert_eq!(native.sketch_relations[0].members, [100, 600]);
    assert!(native.sketch_relations[0].auxiliary_references.is_empty());
    assert_eq!(native.sketch_relations[0].owner_reference, 277);
    assert_eq!(native.sketch_relations[0].owner_entity_id, "0_277");
    assert_eq!(native.sketch_relations[0].state, 0x11);
    assert_eq!(native.sketch_relations[0].return_members, [600, 100]);
    assert_eq!(
        native.sketch_relations[0].resolved_members,
        [
            crate::records::SketchRelationOperand::Point {
                record_index: 100,
                persistent_id: Some(500),
            },
            crate::records::SketchRelationOperand::Curve {
                record_index: 600,
                primary_id: 700,
                secondary_id: 701,
            },
        ]
    );
    assert_eq!(
        native.sketch_relations[0].resolved_return_members,
        [
            crate::records::SketchRelationOperand::Curve {
                record_index: 600,
                primary_id: 700,
                secondary_id: 701,
            },
            crate::records::SketchRelationOperand::Point {
                record_index: 100,
                persistent_id: Some(500),
            },
        ]
    );
    assert!(crate::validate::validate_native(round_trip.ir()).is_empty());

    {
        let point = &mut f3d_native_mut(&mut extended_source_less).sketch_points[0];
        point.depth = 7.5;
        point.flags = [1, 0, 0, 1, 0, 1, 0, 1];
        point.record_form = crate::records::SketchPointRecordForm::Version11 {
            padded_paired_reference: true,
        };
        point.closure = Some(crate::records::SketchPointClosure {
            selector: 4,
            state: 0,
        });
        point.companion = Some(crate::records::SketchPointCompanion {
            prefix_present_zero: true,
            reference_encoding: crate::records::SketchPointCompanionReferenceEncoding::SameSegment,
            incident_curves: vec![600],
        });
    }
    let mut extended_encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &extended_source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut extended_encoded))
        .expect("source-less extended sketch point encode");
    let extended_round_trip = F3dCodec
        .decode(
            &mut Cursor::new(extended_encoded),
            &DecodeOptions::default(),
        )
        .expect("source-less extended sketch point round trip");
    let extended_native = f3d_native(extended_round_trip.ir());
    let extended_point = &extended_native.sketch_points[0];
    assert_eq!(extended_point.depth, 7.5);
    assert_eq!(extended_point.flags, [1, 0, 0, 1, 0, 1, 0, 1]);
    assert_eq!(
        extended_point.closure,
        Some(crate::records::SketchPointClosure {
            selector: 4,
            state: 0,
        })
    );
    assert_eq!(
        extended_point.companion,
        Some(crate::records::SketchPointCompanion {
            prefix_present_zero: true,
            reference_encoding: crate::records::SketchPointCompanionReferenceEncoding::SameSegment,
            incident_curves: vec![600],
        })
    );
    assert!(crate::validate::validate_native(extended_round_trip.ir()).is_empty());

    let mut inconsistent = round_trip.ir().clone();
    f3d_native_mut(&mut inconsistent).sketch_relations[0]
        .resolved_members
        .swap(0, 1);
    assert!(crate::validate::validate_native(&inconsistent)
        .iter()
        .any(|finding| {
            finding.check == cadmpeg_ir::Check::NativeLinks
                && finding.message.contains("typed operands disagree")
        }));

    let mut points = native.sketch_points.clone();
    let mut curves = native.sketch_curve_identities.clone();
    let mut relations = native.sketch_relations.clone();
    let mut conflicting_relation = relations[0].clone();
    let relation_scope = relations[0]
        .id
        .rsplit_once(':')
        .expect("generated relation identity has a stream")
        .0;
    conflicting_relation.id = format!("{relation_scope}:sketch-relation-conflict#1");
    conflicting_relation.owner_reference = 278;
    relations.push(conflicting_relation);
    let mut entities = native.design_entity_headers.clone();
    let mut second_owner = entities[0].clone();
    let entity_scope = entities[0]
        .id
        .rsplit_once(':')
        .expect("generated entity identity has a stream")
        .0;
    second_owner.id = format!("{entity_scope}:sketch-header-conflict#1");
    second_owner.entity_suffix = 278;
    second_owner.entity_id = "0_278".into();
    entities.push(second_owner);
    let error = crate::design::decode::sketch::bind_sketch_graph(
        &entities,
        &mut points,
        &mut curves,
        &mut [],
        &mut relations,
    )
    .expect_err("typed sketch geometry cannot belong to two sketches");
    assert!(error.to_string().contains("belongs to multiple sketches"));
}

#[test]
fn generated_source_less_rejects_act_without_segment_metadata() {
    use crate::records::ActEntity;

    let mut source_less = cadmpeg_ir::examples::unit_cube();
    let mut native = f3d_native_mut(&mut source_less);
    native.act_entities = vec![ActEntity {
        id: "generated:act-entity#0".into(),
        record_index: 7,
        table_record_index_offset: None,
        channel_record_index_offset: None,
        entity_id: "0_985".into(),
        table_entity_id_offset: None,
        channel_entity_id_offset: None,
        in_table: true,
        channel_class_tag: None,
        channels: Default::default(),
        channel_guid_offsets: Default::default(),
    }];
    drop(native);
    let error = F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("ACT generation without its record registry must fail atomically");
    assert!(error
        .to_string()
        .contains("requires a retained MetaStream record registry"));
}

#[test]
fn generated_source_less_writes_unassigned_protein_appearance() {
    use std::collections::BTreeMap;

    use cadmpeg_ir::appearance::Appearance;
    use cadmpeg_ir::ids::AppearanceId;
    use cadmpeg_ir::topology::Color;

    let visual_guid = "11111111-2222-3333-4444-555555555555";
    let appearance_id = AppearanceId("generated:appearance#0".into());
    let mut source_less = cadmpeg_ir::examples::unit_cube();
    source_less.model.appearances = vec![Appearance {
        id: appearance_id.clone(),
        name: Some("Prism-Generated".into()),
        asset_guid: Some(visual_guid.into()),
        library_id: None,
        visual_guid: Some(visual_guid.into()),
        physical_token: Some("PrismMaterial-Generated".into()),
        schema: Some("GenericSchema".into()),
        category: Some("Plastic/Generated".into()),
        base_color: Some(Color {
            r: 0.15,
            g: 0.35,
            b: 0.75,
            a: 1.0,
        }),
        properties: BTreeMap::from([
            ("reflectivity_at_0deg".into(), 0.25),
            ("refraction_index".into(), 1.5),
        ]),
        textures: Vec::new(),
    }];
    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less Protein appearance encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less Protein appearance round trip");
    assert_eq!(round_trip.ir().model.appearances.len(), 1);
    let appearance = &round_trip.ir().model.appearances[0];
    assert_eq!(appearance.name.as_deref(), Some("Prism-Generated"));
    assert_eq!(appearance.visual_guid.as_deref(), Some(visual_guid));
    assert_eq!(appearance.schema.as_deref(), Some("GenericSchema"));
    assert_eq!(appearance.category.as_deref(), Some("Plastic/Generated"));
    assert_eq!(
        appearance.base_color,
        Some(Color {
            r: 0.15,
            g: 0.35,
            b: 0.75,
            a: 1.0,
        })
    );
    assert_eq!(
        appearance.properties.get("reflectivity_at_0deg"),
        Some(&0.25)
    );
    assert_eq!(appearance.properties.get("refraction_index"), Some(&1.5));
    assert!(round_trip.ir().model.appearance_bindings.is_empty());
    assert!(crate::validate::validate_native(round_trip.ir()).is_empty());
    let validation = cadmpeg_ir::validate::validate_neutral(round_trip.ir(), Vec::new());
    assert!(
        validation.is_ok(),
        "validation findings: {:?}",
        validation.findings
    );
}

#[test]
fn generated_source_less_rejects_material_assignment_without_presentation_graph() {
    use crate::records::DesignMaterialAssignment;

    let mut source_less = cadmpeg_ir::examples::unit_cube();
    f3d_native_mut(&mut source_less).design_material_assignments = vec![DesignMaterialAssignment {
        id: "generated:material-assignment#0".into(),
        asm_body_key: 42,
        asm_body_key_offset: 0,
        entity_suffix: 985,
        entity_suffix_offset: 0,
        entity_id: "0_985".into(),
        entity_id_offset: 0,
        visual_guid: "11111111-2222-3333-4444-555555555555".into(),
        visual_guid_offset: 0,
        physical_token: Some("PrismMaterial-Generated".into()),
        physical_token_offset: None,
        visual_preset: None,
        visual_preset_offset: None,
    }];

    let error = F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("an incomplete generated presentation graph must be refused");
    assert!(error
        .to_string()
        .contains("requires a typed body-presentation B-rep and scene graph"));
}

#[test]
fn generated_source_less_rejects_collapsed_visibility_body_bindings() {
    let mut source_less = cadmpeg_ir::examples::unit_cube();
    source_less.model.bodies[0].visible = Some(false);
    let body = source_less.model.bodies[0].id.clone();
    f3d_native_mut(&mut source_less).body_visibilities = [985, 986]
        .into_iter()
        .enumerate()
        .map(|(ordinal, entity_suffix)| crate::records::BodyVisibility {
            id: format!("generated:body-visibility#{ordinal}"),
            body: body.clone(),
            stream: "generated/Design1/BulkStream.dat".into(),
            byte_offset: 0,
            asm_body_key_offset: 0,
            asm_body_key: 42,
            entity_suffix,
            visible: false,
        })
        .collect();

    let error = F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("conflicting body-map rows must not collapse");
    assert!(error
        .to_string()
        .contains("conflicts with the body-map key/suffix bijection"));
}

#[test]
fn generated_f3d_rewrites_native_sketch_point_coordinates() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let expected = update_f3d_native(&mut edited, |native| {
        let point = &mut native.sketch_points[0];
        point.coordinates.u += 12.5;
        point.coordinates.v -= 7.5;
        point.coordinates
    });

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut regenerated)
        .expect("native sketch-point regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated F3D decode");
    assert_eq!(
        f3d_native(round_trip.ir()).sketch_points[0].coordinates,
        expected
    );
}

#[test]
fn generated_f3d_rewrites_native_sketch_arc_geometry() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let expected = update_f3d_native(&mut edited, |native| {
        let curve = &mut native.sketch_curve_identities[0];
        let Some(crate::records::SketchCurveGeometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
            ..
        }) = &mut curve.geometry
        else {
            panic!("generated sketch curve must be an arc")
        };
        center.x += 20.0;
        *radius = 35.0;
        *start_angle = 0.25;
        *end_angle = 2.75;
        curve.geometry.clone()
    });

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut regenerated)
        .expect("native sketch-arc regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated F3D decode");
    assert_eq!(
        f3d_native(round_trip.ir()).sketch_curve_identities[0].geometry,
        expected
    );
}

#[test]
fn generated_f3d_rewrites_native_sketch_constraint_mask() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let expected_references = update_f3d_native(&mut edited, |native| {
        let relation = &mut native.sketch_relations[0];
        relation.state = 0x40;
        relation.constraint_kinds = vec![crate::records::SketchConstraintKind::Horizontal];
        relation.unknown_constraint_bits = 0;
        relation.members.reverse();
        for reference in &mut relation.auxiliary_references {
            *reference = reference.saturating_add(1);
        }
        relation.return_members.reverse();
        (
            relation.members.clone(),
            relation.auxiliary_references.clone(),
            relation.owner_reference,
            relation.return_members.clone(),
        )
    });

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut regenerated)
        .expect("native sketch-constraint regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated F3D decode");
    let native = f3d_native(round_trip.ir());
    let relation = &native.sketch_relations[0];
    assert_eq!(relation.state, 0x40);
    assert_eq!(
        relation.constraint_kinds,
        [crate::records::SketchConstraintKind::Horizontal]
    );
    assert_eq!(relation.unknown_constraint_bits, 0);
    assert_eq!(relation.members, expected_references.0);
    assert_eq!(relation.auxiliary_references, expected_references.1);
    assert_eq!(relation.owner_reference, expected_references.2);
    assert_eq!(relation.return_members, expected_references.3);
}

#[test]
fn validation_rejects_wrong_sketch_constraint_kind_with_equal_cardinality() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let (mut ir, _, _) = decoded.into_parts();
    let relation_id = {
        let relation = &mut f3d_native_mut(&mut ir).sketch_relations[0];
        assert_eq!(relation.constraint_kinds.len(), 1);
        relation.constraint_kinds = vec![crate::records::SketchConstraintKind::Horizontal];
        relation.id.clone()
    };

    let findings = crate::validate::validate_native(&ir);
    assert!(findings.iter().any(|finding| {
        finding.check == cadmpeg_ir::Check::ReferentialIntegrity
            && finding.entity.as_deref() == Some(relation_id.as_str())
    }));
}

#[test]
fn validation_scopes_direct_body_operand_ordinals_by_owning_scope() {
    use crate::records::{
        ConstructionRecipe, ConstructionRecipeKind, ConstructionRecipeSelector,
        DesignBodyRecipeOperand, DesignBodyRecipeOperandOwner, DesignBodyRecipeReference,
        DesignCombineBodySelection, DesignCombineForm, DesignCombineOperation,
        DesignExtrudeOperation, DesignParameterScope, DesignRecordHeader,
    };

    let stream = "f3d:Design/BulkStream.dat";
    let mut ir = cadmpeg_ir::examples::unit_cube();
    let mut scopes = Vec::new();
    let mut headers = Vec::new();
    let mut recipes = Vec::new();
    let mut operands = Vec::new();
    for ordinal in 0..2u32 {
        let scope_record_index = 10 + ordinal;
        let operand_record_index = 100 + ordinal * 10;
        let byte_offset = 1_000 + u64::from(ordinal) * 1_000;
        let recipe_id = format!("{stream}:construction-recipe#{ordinal}");
        let mut scope = DesignParameterScope::empty(
            &format!("{stream}:design-parameter-scope#{scope_record_index}"),
            "Combine",
            scope_record_index,
        );
        scope.reference_members = vec![1, 2, 3, 4, 5, operand_record_index];
        scope.combine_operation = Some(DesignCombineOperation {
            form: DesignCombineForm::Standard,
            operation: DesignExtrudeOperation::Join,
            operation_offset: 0,
            keep_tools: false,
            keep_tools_offset: 0,
            target: DesignCombineBodySelection {
                record_index: operand_record_index,
                external_identity: None,
            },
            tools: vec![DesignCombineBodySelection {
                record_index: operand_record_index + 1,
                external_identity: None,
            }],
        });
        scopes.push(scope);
        headers.push(DesignRecordHeader {
            id: format!("{stream}:design-record-header#{operand_record_index}"),
            record_index: operand_record_index,
            class_tag: "365".into(),
            byte_offset,
        });
        recipes.push(ConstructionRecipe {
            id: recipe_id.clone(),
            byte_offset: byte_offset + 220,
            record_index_offset: None,
            kind: ConstructionRecipeKind::Body,
            design_id: Some("301".into()),
            design_id_offset: Some(byte_offset + 197),
            design_selector: Some(ConstructionRecipeSelector {
                value: operand_record_index + 4,
                byte_offset: byte_offset + 200,
            }),
            recipe_index: ordinal,
            record_index: i32::try_from(operand_record_index + 3).unwrap(),
        });
        operands.push(DesignBodyRecipeOperand {
            id: format!("{stream}:design-body-recipe-operand#{operand_record_index}"),
            scope_record_index,
            owner: DesignBodyRecipeOperandOwner::ScopeReference {
                scope_reference_ordinal: 5,
            },
            record_index: operand_record_index,
            byte_offset,
            class_tag: "365".into(),
            asset_id: "11111111-1111-4111-8111-111111111111".into(),
            asset_id_offset: byte_offset + 56,
            context_id: "22222222-2222-4222-8222-222222222222".into(),
            context_id_offset: byte_offset + 136,
            references: vec![DesignBodyRecipeReference {
                design_reference: u64::from(300 + ordinal),
                design_reference_offset: byte_offset + 25,
                form: 3,
                form_offset: byte_offset + 33,
                candidate_faces: Vec::new(),
                preceding_candidate_faces: Vec::new(),
                preceding_body_slots: Vec::new(),
            }],
            nested_record_index: u64::from(operand_record_index + 3),
            nested_record_index_offset: byte_offset + 38,
            recipe_id,
            resolved_face_slot: None,
            resolved_body_state_id: None,
            resolved_body_slot: None,
            resolved_body_face_slots: Vec::new(),
            next_record_index: operand_record_index + 4,
            next_byte_offset: byte_offset + 300,
        });
    }
    {
        let mut native = f3d_native_mut(&mut ir);
        native.design_parameter_scopes = scopes;
        native.design_record_headers = headers;
        native.construction_recipes = recipes;
        native.design_body_recipe_operands = operands;
    }

    let findings = crate::validate::validate_native(&ir);
    let invalid_operands = findings
        .iter()
        .filter(|finding| {
            finding.message == "Fusion Design body recipe operand has an invalid nested frame"
        })
        .collect::<Vec<_>>();
    assert!(invalid_operands.is_empty(), "{invalid_operands:#?}");
}

#[test]
fn validation_rejects_duplicate_sketch_geometry_persistent_identities() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let (mut ir, _, _) = decoded.into_parts();
    let (point_id, curve_id) = {
        let mut native = f3d_native_mut(&mut ir);
        assert!(native.sketch_points.len() >= 2);
        assert!(native.sketch_curve_identities.len() >= 2);
        native.sketch_points[1].persistent_id = native.sketch_points[0].persistent_id;
        native.sketch_points[0].owner_reference = Some(100);
        native.sketch_points[1].owner_reference = Some(100);
        native.sketch_curve_identities[1].primary_id = native.sketch_curve_identities[0].primary_id;
        native.sketch_curve_identities[1].secondary_id =
            native.sketch_curve_identities[0].secondary_id;
        native.sketch_curve_identities[0].owner_reference = Some(100);
        native.sketch_curve_identities[1].owner_reference = Some(100);
        (
            native.sketch_points[1].id.clone(),
            native.sketch_curve_identities[1].id.clone(),
        )
    };

    let findings = crate::validate::validate_native(&ir);
    assert!(findings.iter().any(|finding| {
        finding.check == cadmpeg_ir::Check::NativeLinks
            && finding.entity.as_deref() == Some(point_id.as_str())
            && finding.message.contains("persistent identity")
    }));
    assert!(findings.iter().any(|finding| {
        finding.check == cadmpeg_ir::Check::NativeLinks
            && finding.entity.as_deref() == Some(curve_id.as_str())
            && finding.message.contains("persistent identity")
    }));
}

#[test]
fn validation_accepts_sketch_geometry_persistent_identities_reused_by_another_owner() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let (mut ir, _, _) = decoded.into_parts();
    let (point_id, curve_id) = {
        let mut native = f3d_native_mut(&mut ir);
        assert!(native.sketch_points.len() >= 2);
        assert!(native.sketch_curve_identities.len() >= 2);
        native.sketch_points[1].persistent_id = native.sketch_points[0].persistent_id;
        native.sketch_points[0].owner_reference = Some(100);
        native.sketch_points[1].owner_reference = Some(101);
        native.sketch_curve_identities[1].primary_id = native.sketch_curve_identities[0].primary_id;
        native.sketch_curve_identities[1].secondary_id =
            native.sketch_curve_identities[0].secondary_id;
        native.sketch_curve_identities[0].owner_reference = Some(100);
        native.sketch_curve_identities[1].owner_reference = Some(101);
        (
            native.sketch_points[1].id.clone(),
            native.sketch_curve_identities[1].id.clone(),
        )
    };

    assert!(
        !crate::validate::validate_native(&ir).iter().any(|finding| {
            finding.check == cadmpeg_ir::Check::NativeLinks
                && (finding.entity.as_deref() == Some(point_id.as_str())
                    || finding.entity.as_deref() == Some(curve_id.as_str()))
                && finding.message.contains("persistent identity")
        })
    );
}

#[test]
fn validation_accepts_sketch_geometry_identities_with_unknown_owner() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let (mut ir, _, _) = decoded.into_parts();
    {
        let mut native = f3d_native_mut(&mut ir);
        assert!(native.sketch_points.len() >= 2);
        assert!(native.sketch_curve_identities.len() >= 2);
        native.sketch_points[1].persistent_id = native.sketch_points[0].persistent_id;
        native.sketch_points[0].owner_reference = None;
        native.sketch_points[1].owner_reference = None;
        native.sketch_curve_identities[1].primary_id = native.sketch_curve_identities[0].primary_id;
        native.sketch_curve_identities[1].secondary_id =
            native.sketch_curve_identities[0].secondary_id;
        native.sketch_curve_identities[0].owner_reference = None;
        native.sketch_curve_identities[1].owner_reference = None;
    }

    assert!(
        !crate::validate::validate_native(&ir).iter().any(|finding| {
            finding.check == cadmpeg_ir::Check::NativeLinks
                && finding.message.contains("persistent identity")
        })
    );
}

#[test]
fn validation_rejects_aliased_sketch_geometry_records() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let (mut ir, _, _) = decoded.into_parts();
    let curve_id = {
        let mut native = f3d_native_mut(&mut ir);
        let point_record_index = native.sketch_points[0].record_index;
        native.sketch_curve_identities[0].record_index = point_record_index;
        native.sketch_curve_identities[0].id.clone()
    };

    assert!(crate::validate::validate_native(&ir).iter().any(|finding| {
        finding.check == cadmpeg_ir::Check::NativeLinks
            && finding.entity.as_deref() == Some(curve_id.as_str())
            && finding
                .message
                .contains("aliases another typed indexed record")
    }));
}

#[test]
fn validation_rejects_duplicate_design_entity_suffixes() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let (mut ir, _, _) = decoded.into_parts();
    let duplicate_id = {
        let mut native = f3d_native_mut(&mut ir);
        let mut duplicate = native
            .design_entity_headers
            .first()
            .expect("generated Design entity header")
            .clone();
        duplicate.id.push_str("-duplicate");
        duplicate.entity_id.push_str(":duplicate");
        let id = duplicate.entity_id.clone();
        native.design_entity_headers.push(duplicate);
        id
    };

    assert!(crate::validate::validate_native(&ir).iter().any(|finding| {
        finding.check == cadmpeg_ir::Check::NativeLinks
            && finding.entity.as_deref() == Some(duplicate_id.as_str())
            && finding.message.contains("entity suffix is duplicated")
    }));
}

#[test]
fn validation_rejects_invalid_design_parameter_family_and_owner() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let (mut ir, _, _) = decoded.into_parts();
    let parameter = crate::records::DesignParameter {
        id: "generated:design-parameter#0".into(),
        byte_offset: 100,
        class_tag: "305".into(),
        record_index: 900,
        family_discriminator: Some(0),
        family_discriminator_offset: Some(122),
        source_ordinal: 0,
        owner_record_index: None,
        expression: "60 mm".into(),
        expression_offset: 136,
        source_kind: "User Parameter".into(),
        source_kind_offset: 166,
        kind: crate::records::DesignParameterKind::User,
        unit: Some("mm".into()),
        unit_offset: Some(210),
        name: "Width".into(),
        name_offset: 220,
        evaluated_value: 6.0,
        evaluated_value_offset: 234,
    };
    f3d_native_mut(&mut ir).design_parameters.push(parameter);
    assert!(crate::validate::validate_native(&ir).is_empty());

    f3d_native_mut(&mut ir).design_parameters[0].family_discriminator = Some(7);
    assert!(crate::validate::validate_native(&ir).iter().any(|finding| {
        finding.check == cadmpeg_ir::Check::NativeLinks
            && finding.entity.as_deref() == Some("generated:design-parameter#0")
            && finding.message.contains("family discriminator")
    }));
    f3d_native_mut(&mut ir).design_parameters[0].family_discriminator = Some(0);

    {
        let parameter = &mut f3d_native_mut(&mut ir).design_parameters[0];
        parameter.family_discriminator = None;
        parameter.family_discriminator_offset = None;
    }
    assert!(crate::validate::validate_native(&ir).iter().any(|finding| {
        finding.check == cadmpeg_ir::Check::NativeLinks
            && finding.entity.as_deref() == Some("generated:design-parameter#0")
            && finding.message.contains("family discriminator")
    }));
    {
        let parameter = &mut f3d_native_mut(&mut ir).design_parameters[0];
        parameter.family_discriminator = Some(0);
        parameter.family_discriminator_offset = Some(122);
    }

    {
        let mut native = f3d_native_mut(&mut ir);
        native.design_parameters[0].kind = crate::records::DesignParameterKind::Feature;
        native.design_parameters[0].owner_record_index = Some(1234);
    }
    assert!(crate::validate::validate_native(&ir).iter().any(|finding| {
        finding.check == cadmpeg_ir::Check::NativeLinks
            && finding.entity.as_deref() == Some("generated:design-parameter#0")
    }));
}

#[test]
fn validation_accepts_grouped_and_direct_extrude_profiles() {
    use crate::records::{
        DesignConstructionOperandGroup, DesignExtrudeExtent, DesignExtrudeOperandRole,
        DesignExtrudeOperation, DesignExtrudePrologue, DesignExtrudeStart, DesignParameterScope,
        DesignSketchProfileOperand,
    };

    let mut ir = cadmpeg_ir::examples::unit_cube();
    let profile = DesignSketchProfileOperand {
        scope_reference_ordinal: 0,
        record_index: 20,
        byte_offset: 200,
        class_tag: "300".into(),
        asset_id: "asset".into(),
        asset_id_offset: 230,
        entity_id: "0_10".into(),
        entity_suffix: 10,
        entity_reference_offset: 250,
        region_selection: None,
        paired_class_tag: "260".into(),
        paired_byte_offset: 300,
    };
    let scope = DesignParameterScope {
        id: "f3d:test:scope#10".into(),
        byte_offset: 100,
        class_tag: "301".into(),
        record_index: 10,
        frame_length: 200,
        kind: "Extrude".into(),
        kind_offset: 210,
        extrude_prologue: Some(DesignExtrudePrologue::ReferenceAware {
            reference: None,
            operation: DesignExtrudeOperation::NewBody,
            operation_offset: 128,
            direction_face_extend_values: [1, 2],
            side_extent_discriminators: [1, 0],
            side_extent_discriminator_offsets: [177, 190],
            first_side_target_ordinal: None,
            extent: DesignExtrudeExtent::OneSidedDistance,
            direction_face_extend_offsets: [132, 136],
            direction_reversed: false,
            direction_reversed_offset: 140,
            solid_operation: true,
            solid_operation_offset: 141,
            start: DesignExtrudeStart::ProfilePlane,
            start_offset: 142,
        }),
        coil_operation: None,
        coil_operation_offset: None,
        coil_extent: None,
        coil_extent_offset: None,
        coil_section: None,
        coil_section_offset: None,
        coil_section_placement: None,
        coil_section_placement_offset: None,
        coil_clockwise: None,
        coil_clockwise_offset: None,
        coil_placement: None,
        coil_transform: None,
        feature_ordinal: 1,
        feature_ordinal_offset: 220,
        history_state_id: None,
        history_state_id_offset: 224,
        previous_history_state_id: None,
        previous_history_state_id_offset: 228,
        reference_count_offset: 180,
        reference_members: vec![20, 30],
        reference_member_offsets: vec![184, 195],
        solid_primitive: None,
        direct_face_operation: None,
        move_operation: None,
        scale_operation: None,
        surface_stitch_operation: None,
        surface_extend_operation: None,
        surface_offset_operation: None,
        ruled_surface_operation: None,
        surface_patch_boundaries: Vec::new(),
        base_flange_operation: None,
        edge_flange_operation: None,
        hem_operation: None,
        fixed_extrude_parameters: None,
        fixed_fillet_parameters: None,
        fixed_chamfer_parameters: None,
        path_feature_construction: None,
        combine_operation: None,
        thread_construction: None,
        draft_operation: None,
        copy_paste_bodies_operation: None,
        base_feature_construction: None,
        work_plane_transform: None,
        work_plane_transform_offset: None,
        work_plane_reference: None,
        work_plane_reference_offset: None,
        work_plane_construction: None,
        work_axis_construction: None,
        joint_origin_transform: None,
        joint_origin_transform_offset: None,
        joint_origin_reference: None,
        joint_origin_reference_offset: None,
        work_point_construction: None,
        unclosed_construction_operand_groups: Vec::new(),
        hole_construction: None,
        extrude_profile: Some(profile),
        sweep_profile: None,
        circular_pattern_construction: None,
        rectangular_pattern_construction: None,
        assembly_alignment: None,
        component_insert_construction: None,
        copy_paste_component_operation: None,
        mirror_construction: None,
        base_flange_profile: None,
        entity_id: None,
        entity_suffix: None,
        entity_reference_offset: None,
        paired_class_tag: "261".into(),
        paired_byte_offset: 300,
    };
    let group = DesignConstructionOperandGroup {
        id: "f3d:test:operand-group#30".into(),
        scope_record_index: 10,
        scope_reference_ordinal: 1,
        record_index: 30,
        byte_offset: 400,
        class_tag: "302".into(),
        members: vec![20],
        lost_edge_references: Vec::new(),
        member_offsets: vec![424],
        frame: crate::records::DesignConstructionOperandGroupFrame {
            member_count_offset: 420,
            auxiliary_record_indices: Vec::new(),
            auxiliary_record_offsets: Vec::new(),
            auxiliary_paths: Vec::new(),
            trailing_record_indices: vec![31],
            trailing_record_offsets: vec![440],
            trailing_transforms: Vec::new(),
            trailing_dual_transforms: Vec::new(),
            trailing_flags: Vec::new(),
            opaque_index: 1,
            opaque_index_offset: 460,
            opaque_scalar: 0.5,
            opaque_scalar_offset: 464,
            variant: false,
        },
        role: 0x0000_0041_0000_0000,
        extrude_role: Some(DesignExtrudeOperandRole::Profile),
        extrude_face_role: None,
        role_offset: 450,

        paired_class_tag: "262".into(),
        paired_byte_offset: 500,
    };
    {
        let mut native = f3d_native_mut(&mut ir);
        native.design_parameter_scopes.push(scope);
        native
            .design_construction_operand_groups
            .push(group.clone());
    }
    let profile_message = |finding: &cadmpeg_ir::Finding| {
        finding.message == "Fusion Design Extrude profile conflicts with its profile operand group"
    };
    let findings = crate::validate::validate_native(&ir);
    assert!(!findings.iter().any(profile_message));
    assert!(!findings
        .iter()
        .any(|finding| finding.message.contains("no counted selection group")));

    f3d_native_mut(&mut ir)
        .design_construction_operand_groups
        .push(group);
    assert!(crate::validate::validate_native(&ir)
        .iter()
        .any(profile_message));

    let profile = f3d_native_mut(&mut ir).design_parameter_scopes[0]
        .extrude_profile
        .take();
    assert!(!crate::validate::validate_native(&ir)
        .iter()
        .any(profile_message));
    f3d_native_mut(&mut ir).design_parameter_scopes[0].extrude_profile = profile;

    f3d_native_mut(&mut ir)
        .design_construction_operand_groups
        .clear();
    assert!(!crate::validate::validate_native(&ir)
        .iter()
        .any(profile_message));

    f3d_native_mut(&mut ir).design_parameter_scopes[0]
        .extrude_profile
        .as_mut()
        .expect("test Extrude profile")
        .scope_reference_ordinal = 1;
    assert!(crate::validate::validate_native(&ir)
        .iter()
        .any(profile_message));
}

#[test]
fn validation_accepts_unindexed_construction_identity_terminal() {
    use crate::records::{
        DesignConstructionOperandGroup, DesignConstructionOperandGroupFrame,
        DesignConstructionOperandIdentity, DesignConstructionPersistentIdentity,
        DesignRecordHeader,
    };

    let stream = "f3d:Design/BulkStream.dat";
    let mut ir = cadmpeg_ir::examples::unit_cube();
    let group = DesignConstructionOperandGroup {
        id: format!("{stream}:operand-group#100"),
        scope_record_index: 10,
        scope_reference_ordinal: 0,
        record_index: 100,
        byte_offset: 1_000,
        class_tag: "271".into(),
        members: Vec::new(),
        lost_edge_references: Vec::new(),
        member_offsets: Vec::new(),
        frame: DesignConstructionOperandGroupFrame {
            member_count_offset: 1_021,
            auxiliary_record_indices: Vec::new(),
            auxiliary_record_offsets: Vec::new(),
            auxiliary_paths: Vec::new(),
            trailing_record_indices: vec![101],
            trailing_record_offsets: vec![1_025],
            trailing_transforms: Vec::new(),
            trailing_dual_transforms: Vec::new(),
            trailing_flags: Vec::new(),
            opaque_index: 1,
            opaque_index_offset: 1_029,
            opaque_scalar: 0.0,
            opaque_scalar_offset: 1_033,
            variant: false,
        },
        role: 0,
        extrude_role: None,
        extrude_face_role: None,
        role_offset: 1_041,
        paired_class_tag: "261".into(),
        paired_byte_offset: 1_050,
    };
    let identity = DesignConstructionOperandIdentity {
        id: format!("{stream}:operand-identity#1100"),
        group_record_index: 100,
        wrapper_record_indices: vec![101],
        wrapper_byte_offsets: vec![1_100],
        wrapper_class_tags: vec!["384".into()],
        following_record_index: 102,
        following_byte_offset: 1_124,
        following_class_tag: "395".into(),
        tracking_path: None,
        persistent_identity: Some(DesignConstructionPersistentIdentity {
            local_id: 167,
            local_id_offset: 1_145,
            asset_id: "2d0697b6-f6c5-4f86-bb58-4a2f413c99d3".into(),
            asset_id_offset: 1_157,
            context_id: "9dea94a1-729a-4032-930b-d4ba4eaadb0c".into(),
            context_id_offset: 1_233,
            tail_slot_present: false,
            tail_slot_offset: 1_309,
            next_record_index: 103,
            next_byte_offset: 1_314,
        }),
    };
    let wrapper = DesignRecordHeader {
        id: format!("{stream}:record-header#1100"),
        record_index: 101,
        class_tag: "384".into(),
        byte_offset: 1_100,
    };
    let following = DesignRecordHeader {
        id: format!("{stream}:record-header#1124"),
        record_index: 102,
        class_tag: "395".into(),
        byte_offset: 1_124,
    };
    let identity_id = identity.id.clone();
    let mut native = crate::native::F3dNative::default();
    native.design_construction_operand_groups.push(group);
    native.design_construction_operand_identities.push(identity);
    native.design_record_headers.extend([wrapper, following]);
    native.store(ir.native.namespace_mut("f3d")).unwrap();

    let invalid_identity = |finding: &cadmpeg_ir::Finding| {
        finding.entity.as_deref() == Some(identity_id.as_str())
            && finding.message.contains("invalid nested frame")
    };
    assert!(!crate::validate::validate_native(&ir)
        .iter()
        .any(invalid_identity));

    let mut native = crate::native::F3dNative::load(ir.native.namespace("f3d").unwrap()).unwrap();
    native.design_record_headers.push(DesignRecordHeader {
        id: format!("{stream}:record-header#1315"),
        record_index: 103,
        class_tag: "301".into(),
        byte_offset: 1_315,
    });
    native.store(ir.native.namespace_mut("f3d")).unwrap();
    assert!(crate::validate::validate_native(&ir)
        .iter()
        .any(invalid_identity));
}

#[test]
fn generated_f3d_rewrites_native_sketch_nurbs_values() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let expected = update_f3d_native(&mut edited, |native| {
        let curve = &mut native.sketch_curve_identities[1];
        let Some(crate::records::SketchCurveGeometry::Nurbs {
            fit_tolerance,
            control_points,
            ..
        }) = &mut curve.geometry
        else {
            panic!("generated sketch curve must be NURBS")
        };
        *fit_tolerance = 0.125;
        control_points[1].x += 15.0;
        control_points[1].y -= 5.0;
        curve.geometry.clone()
    });

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut regenerated)
        .expect("native sketch-NURBS regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated F3D decode");
    assert_eq!(
        f3d_native(round_trip.ir()).sketch_curve_identities[1].geometry,
        expected
    );
}

#[test]
fn generated_f3d_rewrites_body_transform() {
    let source = f3d_with_smbh(&synthetic_geometry_with_transform_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated F3D decode");
    assert_eq!(f3d_native(decoded.ir()).transform_hints.len(), 1);
    assert!(!f3d_native(decoded.ir()).transform_hints[0].rotation);
    let (mut edited, _, fidelity) = decoded.into_parts();
    let transform = edited.model.bodies[0]
        .transform
        .as_mut()
        .expect("generated body transform");
    transform.rows[0][3] = 125.0;
    transform.rows[1][3] = -75.0;
    transform.rows[2][3] = 50.0;
    transform.rows[3][3] = 2.0;
    let expected = *transform;
    f3d_native_mut(&mut edited).transform_hints[0].reflection = true;
    f3d_native_mut(&mut edited).body_native_keys[0].asm_body_key = Some(84);

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut regenerated)
        .expect("body-transform regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated F3D decode");
    assert_eq!(round_trip.ir().model.bodies[0].transform, Some(expected));
    assert!(!f3d_native(round_trip.ir()).transform_hints[0].rotation);
    assert!(f3d_native(round_trip.ir()).transform_hints[0].reflection);
    assert_eq!(
        f3d_native(round_trip.ir()).body_native_keys[0].asm_body_key,
        Some(84)
    );
}

#[test]
fn body_key_edit_does_not_rewrite_ordinal_design_selector() {
    let body = cadmpeg_ir::ids::BodyId("f3d:brep:entity#1".into());
    let mut baseline = crate::native::F3dNative::default();
    baseline
        .body_native_keys
        .push(cadmpeg_asm::brep::records::BodyNativeKey {
            id: "f3d:asm:body-native-key#1".into(),
            body: body.clone(),
            record_index: 1,
            body_ordinal: 0,
            source_brep: Some("BREP.source.smb".into()),
            asm_body_key: Some(436),
        });
    baseline
        .body_visibilities
        .push(crate::records::BodyVisibility {
            id: "f3d:design:body-visibility#1".into(),
            body,
            stream: "Design1/BulkStream.dat".into(),
            byte_offset: 20,
            asm_body_key_offset: 40,
            asm_body_key: 0,
            entity_suffix: 1,
            visible: true,
        });
    let mut target = baseline.clone();
    target.body_native_keys[0].asm_body_key = Some(500);

    let edits = crate::writer::patch::edits::validate_body_native_key_edits(
        crate::writer::patch::edits::PatchNatives {
            baseline: Some(&baseline),
            target: Some(&target),
        },
    )
    .expect("body-key edit");

    assert_eq!(edits.asm.get(&1), Some(&500));
    assert!(edits.design.is_empty());
}

#[test]
fn generated_f3d_rewrites_design_recipe_and_persistent_reference() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated Design decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let mut native = f3d_native(&edited);
    let reference = native
        .persistent_references
        .iter_mut()
        .find(|reference| reference.value == 439)
        .expect("generated persistent reference");
    assert!(reference.byte_offset > 0);
    assert!(reference.value_offset > 0);
    reference.value = 9_001;
    let recipe = &mut native.construction_recipes[0];
    assert!(recipe.byte_offset > 0);
    assert!(recipe.record_index_offset.is_some());
    assert!(recipe.design_id_offset.is_some());
    recipe.record_index = 777;
    recipe.design_id = Some("333".into());
    let member = native
        .design_body_members
        .iter_mut()
        .find(|member| member.entity_suffix == 985)
        .expect("generated body member");
    assert!(member.byte_offset > 0);
    member.entity_suffix = 12_345;
    member.flags = 7;
    let header = native
        .design_entity_headers
        .iter_mut()
        .find(|header| header.in_sketch_module())
        .expect("generated sketch entity header");
    assert!(header.byte_offset > 0);
    assert!(header.record_reference_offset.is_some());
    assert_eq!(header.reference_offsets.len(), 2);
    header.record_reference = Some(585);
    header.reference_indices.swap(0, 1);
    let object = native
        .design_types
        .iter_mut()
        .find(|design_type| design_type.entity_ids == [33, 44])
        .expect("generated relation design type");
    assert!(object.byte_offset < object.version_offset);
    assert_eq!(object.entity_id_offsets.len(), 2);
    object.type_guid = "91111111-2222-3333-4444-555555555555".into();
    object.base_type_guid = Some("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeef".into());
    object.version = 9;
    let act_guid = native
        .act_guids
        .iter_mut()
        .find(|guid| guid.guid == "eeeeeeee-1111-2222-3333-ffffffffffff")
        .expect("generated standalone ACT GUID");
    assert!(act_guid.guid_offset > act_guid.byte_offset);
    act_guid.guid = "ffffffff-1111-2222-3333-444444444444".into();
    native.act_registry_channels[0].guid = "dddddddd-1111-2222-3333-eeeeeeeeeeee".into();
    let act_root = &mut native.act_root_components[0];
    act_root.instance_root_record = 71;
    act_root.components_root_record = 72;
    act_root.registry_flag = 0;
    act_root.entity_id = "1_3".into();
    act_root.display_name = "(Renamed)".into();
    let act_entity = &mut native.act_entities[0];
    assert!(act_entity.table_entity_id_offset.is_some());
    assert!(act_entity.channel_entity_id_offset.is_some());
    act_entity.channels.insert(
        "Appearance".into(),
        "dddddddd-1111-2222-3333-eeeeeeeeeeee".into(),
    );
    let binding = &mut edited.model.appearance_bindings[0];
    binding.channels.insert(
        "Appearance".into(),
        "dddddddd-1111-2222-3333-eeeeeeeeeeee".into(),
    );
    let lost_edge = &mut native.lost_edge_references[0];
    assert!(lost_edge.class_tag_offset > lost_edge.record_byte_offset);
    assert!(lost_edge.class_tag_offset < lost_edge.byte_offset);
    lost_edge.class_tag = "420".into();
    lost_edge.record_index = 4_700;
    let assignment = &mut native.design_material_assignments[0];
    assert!(assignment.entity_id_offset > 0);
    assert!(assignment.asm_body_key_offset > 0);
    assignment.physical_token = Some("PrismMaterial-019".into());
    assignment.visual_preset = Some("Prism-002".into());
    native.body_native_keys[0].asm_body_key = Some(84);
    edited.model.appearances[0].physical_token = Some("PrismMaterial-019".into());
    edited.model.appearances[0].base_color = Some(cadmpeg_ir::topology::Color {
        r: 0.8,
        g: 0.6,
        b: 0.4,
        a: 1.0,
    });
    edited.model.appearances[0]
        .properties
        .insert("reflectivity_at_0deg".into(), 0.7);
    edited.model.appearances[0]
        .properties
        .insert("refraction_index".into(), 1.8);
    assert_eq!(
        native.act_entities[0].entity_id,
        native.design_material_assignments[0].entity_id
    );
    native.store(edited.native.namespace_mut("f3d")).unwrap();

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut regenerated)
        .expect("persistent-reference regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated Design decode");
    assert_eq!(
        f3d_native(round_trip.ir()).design_material_assignments[0].asm_body_key,
        84
    );
    assert!(f3d_native(round_trip.ir())
        .persistent_references
        .iter()
        .any(|reference| reference.value == 9_001));
    assert_eq!(
        f3d_native(round_trip.ir()).construction_recipes[0].record_index,
        777
    );
    assert_eq!(
        f3d_native(round_trip.ir()).construction_recipes[0]
            .design_id
            .as_deref(),
        Some("333")
    );
    assert!(f3d_native(round_trip.ir())
        .design_body_members
        .iter()
        .any(|member| member.entity_suffix == 12_345 && member.flags == 7));
    let header = f3d_native(round_trip.ir())
        .design_entity_headers
        .iter()
        .find(|header| header.in_sketch_module())
        .cloned()
        .expect("round-trip sketch entity header");
    assert_eq!(header.entity_suffix, 277);
    assert_eq!(header.entity_id, "0_277");
    assert_eq!(header.record_reference, Some(585));
    assert_eq!(header.reference_indices, [44, 33]);
    let object = f3d_native(round_trip.ir())
        .design_types
        .iter()
        .find(|design_type| design_type.entity_ids == [33, 44])
        .cloned()
        .expect("round-trip relation design type");
    assert_eq!(object.entity_ids, [33, 44]);
    assert_eq!(object.type_guid, "91111111-2222-3333-4444-555555555555");
    assert_eq!(
        object.base_type_guid.as_deref(),
        Some("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeef")
    );
    assert_eq!(object.version, 9);
    assert!(f3d_native(round_trip.ir())
        .act_guids
        .iter()
        .any(|guid| guid.guid == "ffffffff-1111-2222-3333-444444444444"));
    let act_root = &f3d_native(round_trip.ir()).act_root_components[0];
    assert_eq!(act_root.record_index, 9);
    assert_eq!(act_root.instance_root_record, 71);
    assert_eq!(act_root.components_root_record, 72);
    assert_eq!(act_root.registry_flag, 0);
    assert_eq!(act_root.entity_id, "1_3");
    assert_eq!(act_root.display_name, "(Renamed)");
    assert_eq!(
        f3d_native(round_trip.ir()).act_registry_channels[0].guid,
        "dddddddd-1111-2222-3333-eeeeeeeeeeee"
    );
    let act_entity = &f3d_native(round_trip.ir()).act_entities[0];
    assert_eq!(act_entity.entity_id, "0_985");
    assert_eq!(
        act_entity.channels.get("Appearance").map(String::as_str),
        Some("dddddddd-1111-2222-3333-eeeeeeeeeeee")
    );
    let binding = &round_trip.ir().model.appearance_bindings[0];
    assert_eq!(binding.source_entity_id.as_deref(), Some("0_985"));
    assert_eq!(
        binding.channels.get("Appearance").map(String::as_str),
        Some("dddddddd-1111-2222-3333-eeeeeeeeeeee")
    );
    let lost_edge = &f3d_native(round_trip.ir()).lost_edge_references[0];
    assert_eq!(lost_edge.class_tag, "420");
    assert_eq!(lost_edge.record_index, 4_700);
    assert_eq!(
        f3d_native(round_trip.ir()).design_material_assignments[0].entity_id,
        "0_985"
    );
    assert_eq!(
        f3d_native(round_trip.ir()).design_material_assignments[0]
            .visual_preset
            .as_deref(),
        Some("Prism-002")
    );
    assert_eq!(
        round_trip.ir().model.appearances[0]
            .physical_token
            .as_deref(),
        Some("PrismMaterial-019")
    );
    assert_eq!(
        round_trip.ir().model.appearances[0].base_color,
        Some(cadmpeg_ir::topology::Color {
            r: 0.8,
            g: 0.6,
            b: 0.4,
            a: 1.0,
        })
    );
    assert_eq!(
        round_trip.ir().model.appearances[0]
            .properties
            .get("reflectivity_at_0deg"),
        Some(&0.7)
    );
    assert_eq!(
        round_trip.ir().model.appearances[0]
            .properties
            .get("refraction_index"),
        Some(&1.8)
    );
}

#[test]
fn generated_f3d_rejects_act_binding_divergence() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated ACT decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    update_f3d_native(&mut edited, |native| {
        native.act_entities[0].channels.insert(
            "Appearance".into(),
            "dddddddd-1111-2222-3333-eeeeeeeeeeee".into(),
        );
    });

    let error = F3dCodec
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut Vec::new())
        .expect_err("divergent ACT and appearance binding must fail");
    assert!(matches!(error, cadmpeg_core::CodecError::NotImplemented(_)));
}

#[test]
fn generated_f3d_rejects_act_record_index_edit_without_metastream_edit() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated ACT decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    update_f3d_native(&mut edited, |native| {
        native.act_root_components[0].record_index += 1;
    });

    let error = F3dCodec
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut Vec::new())
        .expect_err("an ACT record-index edit without its MetaStream index must fail");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::NotImplemented(message)
            if message.contains("ACT root edit changes fields")
    ));
}

#[test]
fn generated_f3d_rejects_material_assignment_divergence() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated material decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    update_f3d_native(&mut edited, |native| {
        native.design_material_assignments[0].physical_token = Some("PrismMaterial-019".into());
    });

    let error = F3dCodec
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut Vec::new())
        .expect_err("divergent assignment and appearance must fail");
    assert!(matches!(error, cadmpeg_core::CodecError::NotImplemented(_)));
}

#[test]
fn generated_f3d_rejects_partial_material_assignment_identity_edit() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated material decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    update_f3d_native(&mut edited, |native| {
        let assignment = &mut native.design_material_assignments[0];
        assignment.entity_id = "0_986".into();
        assignment.entity_suffix = 986;
    });

    let error = F3dCodec
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut Vec::new())
        .expect_err("a partial presentation-graph identity edit must fail");
    assert!(error.to_string().contains(
        "requires synchronized body-presentation, browser-node, B-rep, and scene graphs"
    ));
}

#[test]
fn generated_f3d_rejects_invalid_or_structural_protein_property_edits() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated Protein decode");

    let mut invalid = decoded.ir().clone();
    invalid.model.appearances[0]
        .properties
        .insert("refraction_index".into(), 0.5);
    let error = F3dCodec
        .write_preserved_with_source_fidelity(&invalid, decoded.source_fidelity(), &mut Vec::new())
        .expect_err("out-of-range refraction must be refused");
    assert!(
        matches!(error, cadmpeg_core::CodecError::Malformed(message) if message.contains("refraction_index"))
    );

    let (mut structural, _, fidelity) = decoded.into_parts();
    structural.model.appearances[0]
        .properties
        .insert("unserialized_property".into(), 0.5);
    let error = F3dCodec
        .write_preserved_with_source_fidelity(&structural, &fidelity, &mut Vec::new())
        .expect_err("new Protein property must be refused");
    assert!(
        matches!(error, cadmpeg_core::CodecError::NotImplemented(message) if message.contains("unchanged property set"))
    );
}

#[test]
fn generated_f3d_routes_appearance_edits_across_multiple_protein_assets() {
    let source = f3d_with_smbh_and_protein_guids(
        &synthetic_geometry_smbh(),
        &[
            "11111111-2222-3333-4444-555555555555",
            "99999999-2222-3333-4444-555555555555",
        ],
    );
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated multi-Protein decode");
    assert_eq!(decoded.ir().model.appearances.len(), 2);
    let (mut edited, _, fidelity) = decoded.into_parts();
    edited.model.appearances[0].base_color = Some(cadmpeg_ir::topology::Color {
        r: 0.2,
        g: 0.3,
        b: 0.4,
        a: 1.0,
    });
    edited.model.appearances[1].base_color = Some(cadmpeg_ir::topology::Color {
        r: 0.6,
        g: 0.7,
        b: 0.8,
        a: 1.0,
    });

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut regenerated)
        .expect("multi-Protein appearance regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated multi-Protein decode");
    assert_eq!(round_trip.ir().model.appearances, edited.model.appearances);
}

#[test]
fn generated_f3d_rewrites_prism_scalar_properties() {
    let source = f3d_with_smbh_and_instance_properties(
        &synthetic_geometry_smbh(),
        &[
            generated_prism_instance_properties(
                "PrismOpaqueSchema",
                "11111111-2222-3333-4444-555555555555",
            ),
            generated_prism_instance_properties(
                "PrismTransparentSchema",
                "99999999-2222-3333-4444-555555555555",
            ),
        ],
    );
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated Prism decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let opaque = edited
        .model
        .appearances
        .iter_mut()
        .find(|appearance| appearance.schema.as_deref() == Some("PrismOpaqueSchema"))
        .expect("opaque appearance");
    opaque.properties.insert("surface_roughness".into(), 0.75);
    let transparent = edited
        .model
        .appearances
        .iter_mut()
        .find(|appearance| appearance.schema.as_deref() == Some("PrismTransparentSchema"))
        .expect("transparent appearance");
    transparent
        .properties
        .insert("refraction_index".into(), 2.25);

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut regenerated)
        .expect("Prism scalar regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated Prism decode");
    assert!(round_trip.ir().model.appearances.iter().any(|appearance| {
        appearance.schema.as_deref() == Some("PrismOpaqueSchema")
            && appearance.properties.get("surface_roughness") == Some(&0.75)
    }));
    assert!(round_trip.ir().model.appearances.iter().any(|appearance| {
        appearance.schema.as_deref() == Some("PrismTransparentSchema")
            && appearance.properties.get("refraction_index") == Some(&2.25)
    }));
}

#[test]
fn generated_f3d_rewrites_body_rgb_color() {
    let source = f3d_with_smbh(&synthetic_geometry_with_body_color_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let expected = cadmpeg_ir::topology::Color {
        r: 0.7,
        g: 0.4,
        b: 0.2,
        a: 1.0,
    };
    edited.model.bodies[0].color = Some(expected);

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut regenerated)
        .expect("body-color regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated F3D decode");
    assert_eq!(round_trip.ir().model.bodies[0].color, Some(expected));
}

#[test]
fn generated_f3d_rewrites_the_winning_truecolor_attribute() {
    let source = f3d_with_smbh(&synthetic_geometry_with_body_truecolor_chain_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated truecolor F3D decode");
    assert_eq!(
        decoded.ir().model.bodies[0].color,
        Some(cadmpeg_ir::topology::Color {
            r: 32.0 / 255.0,
            g: 64.0 / 255.0,
            b: 96.0 / 255.0,
            a: 1.0,
        })
    );
    let (mut edited, _, fidelity) = decoded.into_parts();
    let expected = cadmpeg_ir::topology::Color {
        r: 64.0 / 255.0,
        g: 128.0 / 255.0,
        b: 192.0 / 255.0,
        a: 1.0,
    };
    edited.model.bodies[0].color = Some(expected);

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut regenerated)
        .expect("truecolor regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated truecolor decode");
    assert_eq!(round_trip.ir().model.bodies[0].color, Some(expected));
}

#[test]
fn generated_f3d_rewrites_fixed_width_decimal_color_text() {
    let source = f3d_with_smbh(&synthetic_geometry_with_body_decimal_color_chain_smbh(
        "04227264",
    ));
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated decimal-color F3D decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let expected = cadmpeg_ir::topology::Color {
        r: 1.0 / 255.0,
        g: 2.0 / 255.0,
        b: 3.0 / 255.0,
        a: 1.0,
    };
    edited.model.bodies[0].color = Some(expected);

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut regenerated)
        .expect("decimal-color regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated decimal-color decode");
    assert_eq!(round_trip.ir().model.bodies[0].color, Some(expected));
}

#[test]
fn generated_f3d_rejects_lossy_truecolor_edit() {
    let source = f3d_with_smbh(&synthetic_geometry_with_body_truecolor_chain_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated truecolor F3D decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    edited.model.bodies[0].color = Some(cadmpeg_ir::topology::Color {
        r: 0.5,
        g: 64.0 / 255.0,
        b: 96.0 / 255.0,
        a: 1.0,
    });

    let error = F3dCodec
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut Vec::new())
        .expect_err("nonrepresentable truecolor edit must be rejected");
    assert!(matches!(error, cadmpeg_core::CodecError::NotImplemented(_)));
}

#[test]
fn generated_f3d_rejects_decimal_color_text_growth() {
    let source = f3d_with_smbh(&synthetic_geometry_with_body_decimal_color_chain_smbh(
        "255",
    ));
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated decimal-color F3D decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    edited.model.bodies[0].color = Some(cadmpeg_ir::topology::Color {
        r: 1.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    });

    let error = F3dCodec
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut Vec::new())
        .expect_err("wider decimal-color text must be rejected");
    assert!(matches!(error, cadmpeg_core::CodecError::NotImplemented(_)));
}

#[test]
fn generated_f3d_rewrites_face_rgb_color_and_sense() {
    let source = f3d_with_smbh(&synthetic_geometry_with_face_color_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let expected = cadmpeg_ir::topology::Color {
        r: 0.6,
        g: 0.3,
        b: 0.9,
        a: 1.0,
    };
    edited.model.faces[0].color = Some(expected);
    edited.model.faces[0].sense = cadmpeg_ir::topology::Sense::Reversed;

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut regenerated)
        .expect("face-color regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated F3D decode");
    assert_eq!(round_trip.ir().model.faces[0].color, Some(expected));
    assert_eq!(
        round_trip.ir().model.faces[0].sense,
        cadmpeg_ir::topology::Sense::Reversed
    );
}

#[test]
fn generated_f3d_rewrites_edge_parameter_range() {
    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    edited.model.edges[0].param_range = Some([-2.5, 4.75]);

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut regenerated)
        .expect("edge-range regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated F3D decode");
    assert_eq!(
        round_trip.ir().model.edges[0].param_range,
        Some([-2.5, 4.75])
    );
}

#[test]
fn generated_f3d_rewrites_edge_native_metadata() {
    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let owner = edited.model.coedges[0].id.clone();
    {
        let mut native = f3d_native_mut(&mut edited);
        native.edge_continuities[0].continuity = "tangent".into();
        native.edge_continuities[0].sense = cadmpeg_ir::topology::Sense::Reversed;
        native.edge_ownerships[0].owner_coedge = Some(owner.clone());
    }

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut regenerated)
        .expect("edge-continuity regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated F3D decode");
    assert_eq!(
        f3d_native(round_trip.ir()).edge_continuities[0].continuity,
        "tangent"
    );
    assert_eq!(
        f3d_native(round_trip.ir()).edge_continuities[0].sense,
        cadmpeg_ir::topology::Sense::Reversed
    );
    assert_eq!(
        f3d_native(round_trip.ir()).edge_ownerships[0].owner_coedge,
        Some(owner)
    );
}

#[test]
fn generated_f3d_rewrites_vertex_ownership() {
    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let replacement = edited.model.edges[1].id.clone();
    {
        let mut native = f3d_native_mut(&mut edited);
        native.vertex_ownerships[1].owning_edge = replacement.clone();
        native.vertex_ownerships[1].endpoint_index = 0;
    }

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut regenerated)
        .expect("vertex-ownership regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated F3D decode");
    let ownership = &f3d_native(round_trip.ir()).vertex_ownerships[1];
    assert_eq!(ownership.owning_edge, replacement);
    assert_eq!(ownership.endpoint_index, 0);
}

#[test]
fn generated_f3d_rewrites_face_and_coedge_sense() {
    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    edited.model.faces[0].sense = cadmpeg_ir::topology::Sense::Reversed;
    edited.model.coedges[0].sense = cadmpeg_ir::topology::Sense::Reversed;

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut regenerated)
        .expect("orientation regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated F3D decode");
    assert_eq!(
        round_trip.ir().model.faces[0].sense,
        cadmpeg_ir::topology::Sense::Reversed
    );
    assert_eq!(
        round_trip.ir().model.coedges[0].sense,
        cadmpeg_ir::topology::Sense::Reversed
    );
}
