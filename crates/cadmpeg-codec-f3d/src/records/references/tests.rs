use super::{
    DESIGN_MATERIAL_ASSIGNMENT_CLONE_COUNT, DesignMaterialAssignment, DesignMaterialAssignmentWire,
    LOST_EDGE_REFERENCE_CLONE_COUNT, LostEdgeReference, LostEdgeReferenceWire,
};

fn lost_edge() -> LostEdgeReference {
    serde_json::from_str(
        r#"{"id":"f3d:native:lost-edge#0","class_tag":"419","record_index":299,"next_class_tag":"326","next_record_index":300}"#,
    )
    .unwrap()
}

fn material(extra_fields: &str) -> DesignMaterialAssignment {
    let wire = format!(
        r#"{{"id":"f3d:native:material-assignment#0","body_binding_id":"f3d:native:design-body-binding#0","asm_body_key":42,"entity_id":"0_985","visual_guid":"11111111-2222-3333-4444-555555555555"{extra_fields}}}"#
    );
    serde_json::from_str(&wire).unwrap()
}

#[test]
fn lost_edge_reference_borrowed_wire_matches_owned_wire_bytes() {
    let record = lost_edge();
    let owned = LostEdgeReferenceWire::from(record.clone());
    assert_eq!(
        serde_json::to_vec(&record).unwrap(),
        serde_json::to_vec(&owned).unwrap()
    );
}

#[test]
fn lost_edge_reference_native_retained_limit_refuses_before_record_clone() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let record = lost_edge();
    let arena_name = "lost_edge_references";
    let needed = serde_json::to_vec(&record).unwrap().len();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(needed).unwrap() - 1;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    LOST_EDGE_REFERENCE_CLONE_COUNT.with(|count| count.set(0));
    let error = namespace
        .set_arena(&limited, arena_name, std::slice::from_ref(&record))
        .unwrap_err();
    LOST_EDGE_REFERENCE_CLONE_COUNT.with(|count| assert_eq!(count.get(), 0));
    assert!(matches!(
        cadmpeg_core::CodecError::from(error),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "serialize native record"
    ));

    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    namespace
        .set_arena(&service, arena_name, std::slice::from_ref(&record))
        .unwrap();
    assert_eq!(
        serde_json::to_value(&namespace.arenas()[arena_name][0]).unwrap(),
        serde_json::to_value(&record).unwrap()
    );
}

#[test]
fn material_assignment_borrowed_wire_matches_owned_wire_bytes() {
    for extra_fields in [
        "",
        r#", "physical_token":"""#,
        r#", "visual_preset":"Prism-002""#,
        r#", "physical_token":"Mat","visual_preset":"Prism-002""#,
    ] {
        let record = material(extra_fields);
        let owned = DesignMaterialAssignmentWire::from(record.clone());
        assert_eq!(
            serde_json::to_vec(&record).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }
}

#[test]
fn material_assignment_native_retained_limit_refuses_before_record_clone() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let record = material(r#", "physical_token":"Mat","visual_preset":"Prism-002""#);
    let arena_name = "design_material_assignments";
    let needed = serde_json::to_vec(&record).unwrap().len();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(needed).unwrap() - 1;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    DESIGN_MATERIAL_ASSIGNMENT_CLONE_COUNT.with(|count| count.set(0));
    let error = namespace
        .set_arena(&limited, arena_name, std::slice::from_ref(&record))
        .unwrap_err();
    DESIGN_MATERIAL_ASSIGNMENT_CLONE_COUNT.with(|count| assert_eq!(count.get(), 0));
    assert!(matches!(
        cadmpeg_core::CodecError::from(error),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "serialize native record"
    ));

    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    namespace
        .set_arena(&service, arena_name, std::slice::from_ref(&record))
        .unwrap();
    assert_eq!(
        serde_json::to_value(&namespace.arenas()[arena_name][0]).unwrap(),
        serde_json::to_value(&record).unwrap()
    );
}

#[test]
fn material_assignment_walks_semantic_binding_identity_without_source_locations() {
    use cadmpeg_ir::schema::rewrite::typed::{IdentityMap, RewriteIdentities};
    let record = material("");
    assert!(record.asm_body_key_offset.is_none());
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut references = Vec::new();
    record
        .visit_identity_references(&ctx, &mut |id| {
            references.push(id.to_string());
            Ok(())
        })
        .unwrap();
    assert!(
        references
            .iter()
            .any(|id| id == "f3d:native:design-body-binding#0")
    );
    let mut map = IdentityMap::new(|id: &str| {
        Ok(if id == "f3d:native:design-body-binding#0" {
            "f3d:native:design-body-binding#authored".to_owned()
        } else {
            id.to_owned()
        })
    });
    let rewritten = record.rewrite_identities(&ctx, &mut map).unwrap();
    assert_eq!(
        rewritten.body_binding_id.as_str(),
        "f3d:native:design-body-binding#authored"
    );
    assert!(rewritten.asm_body_key_offset.is_none());
}
