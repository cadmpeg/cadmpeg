// SPDX-License-Identifier: Apache-2.0
use crate::design::feature_project::scope_properties;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn scope_reference_property_refuses_collection_limit() {
    let scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#10",
        DesignFeatureKind::Combine, 10);
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = scope_properties(Some(&ctx), &scope, "f3d:Design/BulkStream.dat", &[]);
    assert!(matches!(result, Err(CodecError::ResourceLimit(failure))
        if failure.dimension == ResourceDimension::CollectionItems
            && failure.operation == "f3d scope reference property"));

    let properties = scope_properties(None, &scope, "f3d:Design/BulkStream.dat", &[]).unwrap();
    assert_eq!(properties.len(), 1);
    assert_eq!(properties[&cadmpeg_core::nonblank_literal!("reference:0")], "10");
}

#[test]
fn scope_profile_property_refuses_collection_limit() {
    use crate::records::feature::scope::{DesignExtrudeScope, DesignScopePayloadMut};
    use crate::records::sketch_placement::{DesignSketchFrame, DesignSketchFrameForm,
        DesignSketchPlacement};
    use crate::records::topology::sketch_profile::{DesignSketchProfileOperand,
        DesignSketchProfileOperandDraft};

    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#10",
        DesignFeatureKind::Extrude, 10);
    let entity_id = crate::records::identity::DesignEntityId::try_from("0_172".to_owned()).unwrap();
    let profile = DesignSketchProfileOperand::try_new(DesignSketchProfileOperandDraft {
        scope_reference_ordinal: 0,
        record_index: 100,
        byte_offset: 300,
        class_tag: crate::records::references::DesignClassTag::try_from("308".to_owned()).unwrap(),
        asset_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
            "e72ed0d8-58b4-4b8e-800d-5eaeea9c0c4b".to_owned()).unwrap(),
        asset_id_offset: 330,
        entity_id: entity_id.clone(),
        entity_reference_offset: 420,
        region_selection: None,
        paired_class_tag: crate::records::references::DesignClassTag::try_from("259".to_owned()).unwrap(),
        paired_byte_offset: 520,
    }).unwrap();
    if let DesignScopePayloadMut::Extrude(slot) = scope.payload_mut() {
        *slot = Some(DesignExtrudeScope { extrude_profile: Some(profile), ..Default::default() });
    }
    let placement = DesignSketchPlacement {
        id: "f3d:Design/BulkStream.dat:placement#200".into(),
        scope_record_index: Some(10),
        entity_id,
        visibility: None,
        class_tag: crate::records::references::DesignClassTag::try_from("300".to_owned()).unwrap(),
        record_index: 200,
        paired_class_tag: crate::records::references::DesignClassTag::try_from("260".to_owned()).unwrap(),
        frame: DesignSketchFrame::new(600, DesignSketchFrameForm::ScopeCompact).unwrap(),
    };
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 1;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = scope_properties(Some(&ctx), &scope, "f3d:Design/BulkStream.dat",
        std::slice::from_ref(&placement));
    assert!(matches!(result, Err(CodecError::ResourceLimit(failure))
        if failure.dimension == ResourceDimension::CollectionItems
            && failure.operation == "f3d scope profile property"));
}
