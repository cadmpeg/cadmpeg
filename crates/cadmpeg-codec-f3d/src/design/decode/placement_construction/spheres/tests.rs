use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope, DesignScopePayload};
use crate::records::placement_construction::DesignPlacementConstruction;

fn placement() -> DesignPlacementConstruction {
    let mut bytes = Vec::new();
    crate::test_support::indexed_header(&mut bytes, *b"317", 255);
    bytes.extend_from_slice(&[0; 10]);
    for value in [0.0_f64; 3] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&[0; 10]);
    let rows = [
        [1.0_f64, 0.0, 0.0, 0.1],
        [0.0, 0.0, 1.0, 0.2],
        [0.0, -1.0, 0.0, 0.3],
        [0.0, 0.0, 0.0, 1.0],
    ];
    for value in rows.into_iter().flatten() {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&0.0_f64.to_le_bytes());
    bytes.extend_from_slice(&0.0_f64.to_le_bytes());
    bytes.extend_from_slice(&0.0_f64.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 2]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    crate::test_support::with_decode_context(|ctx| {
        crate::design::decode::placement_construction::decode_frame(
            ctx,
            "f3d:Design/BulkStream.dat:design-placement-construction#0".into(),
            cadmpeg_core::decode::View::over_retained(&bytes),
            9,
        )
        .unwrap()
        .unwrap()
    })
}

fn scope() -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#1",
        DesignFeatureKind::SpherePrimitive,
        1,
    );
    let sphere = serde_json::from_value(serde_json::json!({
        "placement_record_index": 255,
        "transform": [[1.0,0.0,0.0,0.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0],[0.0,0.0,0.0,1.0]],
        "transform_offset": 20,
        "diameter": 1.7, "diameter_record_index": 259, "diameter_offset": 440,
        "operation": "new_body", "operation_offset": 20
    }))
    .unwrap();
    scope
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![251, 255, 259, 264]);
            draft.payload =
                crate::records::feature::primitives::DesignSolidPrimitive::Sphere(sphere).into();
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope
}

#[test]
fn sphere_binds_solved_frame_and_projects_millimetres() {
    let mut scope = scope();
    assert!(super::bind(std::slice::from_mut(&mut scope), &[placement()]).is_empty());
    let DesignScopePayload::SpherePrimitive(Some(sphere)) = scope.payload() else {
        panic!()
    };
    assert_eq!(sphere.transform[0][3], 0.1);
    assert_eq!(sphere.transform[1][3], 0.2);
    assert_eq!(sphere.transform[2][3], 0.3);
    let restored: DesignParameterScope =
        serde_json::from_value(serde_json::to_value(&scope).unwrap()).unwrap();
    assert_eq!(restored, scope);
}

#[test]
fn sphere_retains_dimensions_with_missing_duplicate_foreign_or_unusable_placement() {
    let mut foreign = placement();
    foreign.id = "f3d:Other/BulkStream.dat:placement#0".into();
    let mut unusable = placement();
    unusable.fields.matrix = None;
    unusable.fields.identity = None;
    for candidates in [
        vec![],
        vec![placement(), placement()],
        vec![foreign],
        vec![unusable],
    ] {
        let mut scope = scope();
        let losses = super::bind(std::slice::from_mut(&mut scope), &candidates);
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code,
            crate::loss::F3dLossCode::SpherePlacementFrameAssumed
                .note("")
                .code
        );
        let DesignScopePayload::SpherePrimitive(Some(sphere)) = scope.payload() else {
            panic!()
        };
        assert_eq!(sphere.diameter.get(), 1.7);
        assert_eq!(
            sphere.transform,
            crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY
        );
    }
}

#[test]
fn sphere_keeps_stored_frame_with_unresolved_flags_and_names_scalar_assumptions() {
    let mut placement = placement();
    placement.fields.inferred_x_axis.value = 1;
    placement.fields.z_flip.value = 1;
    let mut scope = scope();
    if let crate::records::feature::scope::DesignScopePayloadMut::SpherePrimitive(Some(sphere)) =
        scope.payload_mut()
    {
        sphere.diameter_owner_assumed = true;
        sphere.diameter_source_assumed = true;
    }
    let losses = super::bind(std::slice::from_mut(&mut scope), &[placement]);
    assert_eq!(losses.len(), 3);
    for (loss, code) in losses.iter().zip([
        crate::loss::F3dLossCode::SpherePlacementFlagsAssumed,
        crate::loss::F3dLossCode::SphereDiameterOwnerAssumed,
        crate::loss::F3dLossCode::SphereDiameterSourceAssumed,
    ]) {
        assert_eq!(loss.code, code.note("").code);
    }
    let DesignScopePayload::SpherePrimitive(Some(sphere)) = scope.payload() else {
        panic!()
    };
    assert_eq!(sphere.transform[2][3], 0.3);
}
