// SPDX-License-Identifier: Apache-2.0
//! Writer-domain synthetic tests.
#![allow(clippy::unwrap_used)]
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::if_not_else,
    clippy::needless_pass_by_value,
    clippy::range_plus_one,
    clippy::semicolon_if_nothing_returned,
    clippy::trivially_copy_pass_by_ref
)]

use cadmpeg_ir::codec::write::EncodeInput;
use cadmpeg_ir::codec::write::TargetRequest;
use std::io::{Cursor, Read};

use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::*;
use crate::F3dCodec;

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
            entities: crate::records::ReferenceRun::Unlocated(vec![1, 2]),
            type_guid: "11111111-2222-3333-4444-555555555555".into(),
            type_guid_offset: 0,
            base_type_guid: None,
            version: 7,
            version_offset: 0,
        },
        SegmentType {
            id: "generated:design-type#1".into(),
            byte_offset: 0,
            module: crate::records::DESIGN_MODULE_SKETCH.to_owned(),
            entities: crate::records::ReferenceRun::Unlocated(vec![277]),
            type_guid: "22222222-3333-4444-5555-666666666666".into(),
            type_guid_offset: 0,
            base_type_guid: Some(crate::records::RecordedValue { value: "11111111-2222-3333-4444-555555555555".into(), offset: None }),
            version: 9,
            version_offset: 0,
        },
        SegmentType {
            id: "generated:design-type#2".into(),
            byte_offset: 0,
            module: "FutureFeature".to_owned(),
            entities: crate::records::ReferenceRun::Unlocated(vec![999]),
            type_guid: "33333333-4444-5555-6666-777777777777".into(),
            type_guid_offset: 0,
            base_type_guid: Some(crate::records::RecordedValue { value: "11111111-2222-3333-4444-555555555555".into(), offset: None }),
            version: 11,
            version_offset: 0,
        },
    ];

    drop(native);
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less Design MetaStream encode");
    let mut guid_module = source_less.clone();
    f3d_native_mut(&mut guid_module).design_types[2].module =
        "11111111-2222-3333-4444-555555555555".into();
    let error = F3dCodec
        .plan(EncodeInput::new(&guid_module, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("a GUID-shaped Design module name must not be emitted");
    assert!(error
        .to_string()
        .contains("Design type module name is GUID-shaped"));
    f3d_native_mut(&mut source_less).design_types[0].base_type_guid =
        Some(crate::records::RecordedValue { value: "22222222-3333-4444-5555-666666666666".into(), offset: None });
    let error = F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
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
    assert_eq!(fusion.entities.values().copied().collect::<Vec<_>>(), [1, 2]);
    assert_eq!(fusion.version, 7);
    assert_eq!(fusion.base_type_guid, None);
    let sketch = types
        .iter()
        .find(|design_type| design_type.module == crate::records::DESIGN_MODULE_SKETCH)
        .expect("sketch-module type");
    assert_eq!(sketch.entities.values().copied().collect::<Vec<_>>(), [277]);
    assert_eq!(
        sketch.base_type_guid.as_ref().map(|field| field.value.as_str()),
        Some("11111111-2222-3333-4444-555555555555")
    );
    assert_eq!(sketch.version, 9);
    let future = types
        .iter()
        .find(|design_type| design_type.module == "FutureFeature")
        .expect("forward-compatible module");
    assert_eq!(future.entities.values().copied().collect::<Vec<_>>(), [999]);
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
        design: Some(crate::records::ConstructionRecipeDesign { id: crate::records::RecordedValue { value: format!("{}", 320 + ordinal), offset: None }, selector: None }),
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
    native.lost_edge_references = vec![LostEdgeReference::new("generated:lost-edge-reference#0".into(), 0, "419".into(), 4645, "419".into(), 4646).expect("valid lost-edge record layout")];

    drop(native);
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less Design BulkStream encode");
    f3d_native_mut(&mut source_less).construction_recipes[0].recipe_index = 1;
    let error = F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
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
    assert_eq!(body_recipe.design.as_ref().map(|design| design.id.value.as_str()), Some("320"));
    assert!(native
        .construction_recipes
        .iter()
        .any(|recipe| recipe.kind == ConstructionRecipeKind::BoundedFace));
    let bounded = native
        .construction_recipes
        .iter()
        .find(|recipe| recipe.kind == ConstructionRecipeKind::BoundedFace)
        .expect("bounded-face recipe");
    assert_eq!(bounded.design.as_ref().map(|design| design.id.value.as_str()), Some("322"));
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
    assert_eq!(native.lost_edge_references[0].class_tag.as_str(), "419");
    assert_eq!(native.lost_edge_references[0].record_index, 4645);
    assert_eq!(native.lost_edge_references[0].next_class_tag.as_str(), "419");
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
        entities: crate::records::ReferenceRun::Unlocated(vec![277]),
        type_guid: "22222222-3333-4444-5555-666666666666".into(),
        type_guid_offset: 0,
        base_type_guid: None,
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

        entity_id: crate::records::DesignEntityId::try_from("0_277".to_owned()).expect("valid entity ID"),
        class_tag: crate::records::DesignClassTag::try_from("256".to_owned()).unwrap(),
        optional_slot_present: true,
        module: Some(crate::records::DESIGN_MODULE_SKETCH.to_owned()),
        record_reference: Some(584),
        record_reference_offset: None,
        reference_count_present: true,
        references: crate::records::ReferenceRun::Unlocated(vec![33, 44]),
        members: crate::records::ReferenceRun::Unlocated(Vec::new()),
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
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less Design ownership encode");
    {
        let mut native = f3d_native_mut(&mut source_less);
        native.design_entity_headers[0].module = Some("Body".to_owned());
    }
    let error = F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
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
    assert_eq!(native.design_entity_headers[0].entity_id.as_str(), "0_277");
    assert_eq!(native.design_entity_headers[0].declared_reference_count(), Some(2));
    assert_eq!(native.design_entity_headers[0].record_reference, Some(584));
    assert_eq!(native.design_entity_headers[0].references.values().copied().collect::<Vec<_>>(), [33, 44]);
    assert_eq!(native.design_record_headers.len(), 2);
    assert_eq!(native.design_record_headers[0].record_index, 33);
    assert_eq!(native.design_record_headers[1].class_tag, "351");
}

#[test]
fn generated_source_less_writes_sketch_points_curves_and_constraints() {
    use crate::records::{
        DesignEntityHeader, SegmentType, SketchCurveGeometry, SketchCurveIdentity, SketchPoint,
        SketchRelation, SketchRelationKind, SketchRelationMember, SketchRelationReturnMember,
    };
    use cadmpeg_ir::math::{Point2, Point3, Vector3};

    let mut source_less = cadmpeg_ir::examples::unit_cube();
    let mut native = f3d_native_mut(&mut source_less);
    native.design_types = vec![
        SegmentType {
            id: "generated:sketch-type-00-object#0".into(),
            byte_offset: 0,
            module: crate::records::DESIGN_MODULE_SKETCH.to_owned(),
            entities: crate::records::ReferenceRun::Unlocated(vec![277]),
            type_guid: crate::design::decode::sketch::SKETCH_CONTAINER_TYPE_GUID.into(),
            type_guid_offset: 0,
            base_type_guid: None,
            version: 1,
            version_offset: 0,
        },
        SegmentType {
            id: "generated:sketch-type-01-relation#0".into(),
            byte_offset: 1,
            module: crate::records::DESIGN_MODULE_SKETCH.to_owned(),
            entities: crate::records::ReferenceRun::Unlocated(vec![33]),
            type_guid: "60403D47-0C49-49B0-BDE8-1679608164A2".into(),
            type_guid_offset: 0,
            base_type_guid: None,
            version: 1,
            version_offset: 0,
        },
        SegmentType {
            id: "generated:sketch-type-02-point#0".into(),
            byte_offset: 2,
            module: "Geometry".into(),
            entities: crate::records::ReferenceRun::Unlocated(vec![100]),
            type_guid: "C2CEDAE7-1716-47C1-B7B1-07B70081D0FB".into(),
            type_guid_offset: 0,
            base_type_guid: None,
            version: 11,
            version_offset: 0,
        },
        SegmentType {
            id: "generated:sketch-type-03-line#0".into(),
            byte_offset: 3,
            module: "Geometry".into(),
            entities: crate::records::ReferenceRun::Unlocated(vec![600]),
            type_guid: "DCA267ED-D615-4934-B64F-AD805E8003E2".into(),
            type_guid_offset: 0,
            base_type_guid: None,
            version: 2,
            version_offset: 0,
        },
        SegmentType {
            id: "generated:sketch-type-04-circular#0".into(),
            byte_offset: 4,
            module: "Geometry".into(),
            entities: crate::records::ReferenceRun::Unlocated(vec![601]),
            type_guid: "F0130424-8B7E-4092-93C9-1CA807482534".into(),
            type_guid_offset: 0,
            base_type_guid: None,
            version: 0,
            version_offset: 0,
        },
        SegmentType {
            id: "generated:sketch-type-05-nurbs#0".into(),
            byte_offset: 5,
            module: crate::records::DESIGN_MODULE_SKETCH.to_owned(),
            entities: crate::records::ReferenceRun::Unlocated(vec![602]),
            type_guid: "D82E012F-6DDD-4AED-BDE1-C0F7F9100B9B".into(),
            type_guid_offset: 0,
            base_type_guid: None,
            version: 3,
            version_offset: 0,
        },
        SegmentType {
            id: "generated:sketch-type-06-point-companion#0".into(),
            byte_offset: 6,
            module: "Geometry".into(),
            entities: crate::records::ReferenceRun::Unlocated(vec![101]),
            type_guid: crate::design::decode::sketch::SKETCH_POINT_COMPANION_TYPE
                .0
                .into(),
            type_guid_offset: 0,
            base_type_guid: None,
            version: crate::design::decode::sketch::SKETCH_POINT_COMPANION_TYPE.1,
            version_offset: 0,
        },
    ];
    native.design_entity_headers = vec![DesignEntityHeader {
        id: "generated:sketch-header#0".into(),
        byte_offset: 0,

        entity_id: crate::records::DesignEntityId::try_from("0_277".to_owned()).expect("valid entity ID"),
        class_tag: crate::records::DesignClassTag::try_from("256".to_owned()).unwrap(),
        optional_slot_present: true,
        module: Some(crate::records::DESIGN_MODULE_SKETCH.to_owned()),
        record_reference: Some(584),
        record_reference_offset: None,
        reference_count_present: true,
        references: crate::records::ReferenceRun::Unlocated(vec![33]),
        members: crate::records::ReferenceRun::Unlocated(Vec::new()),
    }];
    native.sketch_points = vec![SketchPoint {
        id: "generated:sketch-point#0".into(),
        record_index: 100,
        owner_reference: Some(277),
        class_tag: "258".into(),
        byte_offset: 0,
        coordinate_offset: 89,
        entity_genesis: Some(900),
        record_form: crate::records::SketchPointRecordForm::version11(
            500,
            crate::records::SketchPointClosure::Selector0State1,
        ),
        paired_reference: 101,
        coordinates: Point2::new(12.5, -25.0),
        depth: 0.0,
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
                poles: crate::records::SketchNurbsPoles::Rational(vec![
                    crate::records::SketchNurbsPole { point: Point3::new(0.0, 0.0, 0.0), weight: 1.0 },
                    crate::records::SketchNurbsPole { point: Point3::new(10.0, 20.0, 0.0), weight: 0.8 },
                    crate::records::SketchNurbsPole { point: Point3::new(30.0, 10.0, 0.0), weight: 1.0 },
                ]),
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
        auxiliary_references: crate::records::ReferenceRun::Unlocated(Vec::new()),
        rectangular_counted_reference_count: None,
        members: vec![
            SketchRelationMember::from_index(100),
            SketchRelationMember::from_index(600),
        ],
        state: 0x11,
        entity_genesis: None,
        kind: SketchRelationKind::Unpatterned,
        return_members: vec![
            SketchRelationReturnMember::from_index(600),
            SketchRelationReturnMember::from_index(100),
        ],
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
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
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
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("source-less points require their direct owner backlink");
    assert!(matches!(error, cadmpeg_core::CodecError::InvalidInput(_)));
    f3d_native_mut(&mut source_less).sketch_points[0].owner_reference = Some(277);
    f3d_native_mut(&mut source_less).design_types[6].entities = crate::records::ReferenceRun::Unlocated(Vec::new());
    let error = F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("source-less points require a registered inverse companion");
    assert!(matches!(error, cadmpeg_core::CodecError::InvalidInput(_)));
    f3d_native_mut(&mut source_less).design_types[6].entities = crate::records::ReferenceRun::Unlocated(vec![101]);
    f3d_native_mut(&mut source_less).design_types[2].version = 10;
    let error = F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("source-less points require the current writable class version");
    assert!(matches!(error, cadmpeg_core::CodecError::NotImplemented(_)));
    f3d_native_mut(&mut source_less).design_types[2].version = 11;
    {
        let relation = &mut f3d_native_mut(&mut source_less).sketch_relations[0];
        relation.members = [100, 600, 100, 600, 100, 600, 100, 600]
            .into_iter()
            .map(SketchRelationMember::from_index)
            .collect();
        relation.return_members = relation
            .members
            .iter()
            .rev()
            .map(|member| SketchRelationReturnMember::from_index(member.record_index))
            .collect();
    }
    let mut variable_relation = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut variable_relation))
        .expect("source-less variable-width sketch relation encode");
    let variable_round_trip = F3dCodec
        .decode(
            &mut Cursor::new(variable_relation),
            &DecodeOptions::default(),
        )
        .expect("source-less variable-width sketch relation round trip");
    assert_eq!(
        f3d_native(variable_round_trip.ir()).sketch_relations[0].member_indices(),
        vec![100, 600, 100, 600, 100, 600, 100, 600]
    );
    assert!(
        f3d_native(variable_round_trip.ir()).sketch_relations[0]
            .raw_bytes
            .len()
            > 101
    );
    {
        let relation = &mut f3d_native_mut(&mut source_less).sketch_relations[0];
        relation.members = vec![
            SketchRelationMember::from_index(100),
            SketchRelationMember::from_index(600),
        ];
        relation.return_members = vec![
            SketchRelationReturnMember::from_index(600),
            SketchRelationReturnMember::from_index(100),
        ];
    }
    f3d_native_mut(&mut source_less).sketch_relations[0].owner_reference = 999;
    let error = F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
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
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("duplicate typed sketch indices must not be deduplicated");
    assert!(error.to_string().contains("share record index 600"));
    f3d_native_mut(&mut source_less).sketch_points[0].record_index = 100;
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less sketch BulkStream round trip");
    let native = f3d_native(round_trip.ir());
    assert_eq!(native.sketch_points.len(), 1);
    assert_eq!(native.sketch_points[0].persistent_id(), Some(500));
    assert_eq!(native.sketch_points[0].entity_genesis, Some(900));
    assert_eq!(native.sketch_points[0].coordinate_offset, 141);
    assert_eq!(native.sketch_points[0].owner_reference, Some(277));
    assert_eq!(native.sketch_points[0].depth, 0.0);
    assert_eq!(
        native.sketch_points[0].closure(),
        Some(crate::records::SketchPointClosure::Selector0State1)
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
    assert_eq!(native.sketch_relations[0].member_indices(), vec![100, 600]);
    assert!(native.sketch_relations[0].auxiliary_references.is_empty());
    assert_eq!(native.sketch_relations[0].owner_reference, 277);
    assert_eq!(native.sketch_relations[0].owner_entity_id, "0_277");
    assert_eq!(native.sketch_relations[0].state, 0x11);
    assert_eq!(
        native.sketch_relations[0].return_member_indices(),
        vec![600, 100]
    );
    assert_eq!(
        native.sketch_relations[0].resolved_members(),
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
        native.sketch_relations[0].resolved_return_members(),
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
        let persistent_id = point.persistent_id().unwrap_or(0);
        point.record_form = crate::records::SketchPointRecordForm::Version11 {
            padded_paired_reference: true,
            persistent_id,
            flags: [1, 0, 0, 1, 0, 1, 0, 1],
            closure: crate::records::SketchPointClosure::Selector4State0,
        };
        point.companion = Some(crate::records::SketchPointCompanion {
            prefix_present_zero: true,
            reference_encoding: crate::records::SketchPointCompanionReferenceEncoding::SameSegment,
            incident_curves: vec![600],
        });
    }
    let mut extended_encoded = Vec::new();
    F3dCodec
        .plan(
            EncodeInput::new(&extended_source_less, None),
            TargetRequest::Inherit,
        )
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
    assert_eq!(extended_point.flags(), [1, 0, 0, 1, 0, 1, 0, 1]);
    assert_eq!(
        extended_point.closure(),
        Some(crate::records::SketchPointClosure::Selector4State0)
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
        .resolved_members()
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
    second_owner.entity_id = crate::records::DesignEntityId::from_parts("0", 278);
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
