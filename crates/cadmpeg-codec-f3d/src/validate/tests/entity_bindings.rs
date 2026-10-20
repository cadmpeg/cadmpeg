// SPDX-License-Identifier: Apache-2.0

fn validation_entity_header(
    references: Vec<crate::records::identity::Located<u32>>,
) -> crate::records::entity_header::DesignEntityHeader {
    use crate::records::entity_header::{
        DesignEntityHeader, DesignEntityRegistration, SketchHeaderReferences,
    };
    use crate::records::identity::{DesignEntityId, ReferenceRun};
    DesignEntityHeader {
        id: "f3d:Design/BulkStream.dat:entity-header#1".into(),
        byte_offset: 10,
        entity_id: DesignEntityId::from_parts("sketch", 1),
        class_tag: crate::records::references::DesignClassTag::try_from("112".to_owned()).unwrap(),
        optional_slot_present: false,
        registration: DesignEntityRegistration::new(
            Some(crate::records::entity_header::DESIGN_MODULE_SKETCH.into()),
            (!references.is_empty()).then_some(SketchHeaderReferences {
                record_reference: None,
                record_reference_offset: 10,
                references,
            }),
            ReferenceRun::unlocated(Vec::new()),
        )
        .unwrap(),
    }
}

fn validation_body_bounds(with_binding: bool) -> crate::native::F3dNative {
    use crate::records::bodies::{
        DesignBodyBinding, DesignBodyBindingWire, DesignBodyBounds, DesignBodyBoundsWire,
    };
    use crate::records::entity_header::{
        DesignEntityHeader, DesignEntityRegistration, DESIGN_MODULE_BODY,
    };
    use crate::records::identity::{DesignEntityId, ReferenceRun};

    let binding_id = "f3d:Design/BulkStream.dat:design-body-binding#50";
    let bounds = DesignBodyBounds::try_from(DesignBodyBoundsWire {
        id: "f3d:Design/BulkStream.dat:design-body-bounds#10".into(),
        entity_suffix: 1,
        entity_byte_offset: 10,
        record_indices: [2, 3, 4],
        record_byte_offsets: [20, 30, 40],
        value_byte_offsets: [21, 31, 41],
        body_binding_ids: if with_binding {
            vec![binding_id.to_owned().try_into().unwrap()]
        } else {
            Default::default()
        },
        maximum: cadmpeg_ir::math::Point3::new(1.0, 1.0, 1.0),
        minimum: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
    })
    .unwrap();
    let mut native = crate::native::F3dNative::default();
    native.design_body_bounds.push(bounds);
    if with_binding {
        native.design_body_bindings.push(
            DesignBodyBinding::try_from(DesignBodyBindingWire {
                id: binding_id.into(),
                stream: "Design/BulkStream.dat".into(),
                pair_count: 1,
                pair_ordinal: 0,
                asm_body_key: 1,
                asm_body_key_offset: 50,
                entity_suffix: 1,
                entity_suffix_offset: 58,
                blob_name: "BREP.body".into(),
                blob_name_offset: 60,
                body: None,
            })
            .unwrap(),
        );
        native.design_entity_headers.push(DesignEntityHeader {
            id: "f3d:Design/BulkStream.dat:entity-header#1".into(),
            byte_offset: 10,
            entity_id: DesignEntityId::from_parts("body", 1),
            class_tag: crate::records::references::DesignClassTag::try_from("112".to_owned())
                .unwrap(),
            optional_slot_present: false,
            registration: DesignEntityRegistration::new(
                Some(DESIGN_MODULE_BODY.into()),
                None,
                ReferenceRun::unlocated(Vec::new()),
            )
            .unwrap(),
        });
    }
    native
}

#[test]
fn body_bounds_valid_binding_order_has_no_finding() {
    crate::test_support::with_decode_context(|service_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = validation_body_bounds(true);
        let ctx = super::super::Ctx::new(&ir, &native, service_ctx);
        let mut findings = Vec::new();
        super::super::validate_body_bounds(&ctx, &mut findings);
        assert!(findings.is_empty());
    })
}

#[test]
fn body_binding_valid_group_has_no_finding() {
    crate::test_support::with_decode_context(|service_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = validation_body_bounds(true);
        let ctx = super::super::Ctx::new(&ir, &native, service_ctx);
        let mut findings = Vec::new();
        super::super::validate_body_bindings(&ctx, &mut findings).unwrap();
        assert!(findings.is_empty());
    })
}

fn validation_placement(
    scope_record_index: Option<u32>,
    ordinal: Option<u32>,
    member: bool,
) -> crate::records::sketch_placement::DesignSketchPlacement {
    use crate::records::sketch_placement::{
        DesignSketchFrame, DesignSketchFrameForm, DesignSketchPlacement, DesignSketchVisibility,
    };
    DesignSketchPlacement {
        id: "f3d:Design/BulkStream.dat:sketch-placement#1".into(),
        scope_record_index,
        entity_id: crate::records::identity::DesignEntityId::from_parts("sketch", 1),
        visibility: ordinal.map(|ordinal| {
            DesignSketchVisibility::new(std::num::NonZeroU32::new(ordinal).unwrap(), 100, true)
                .unwrap()
        }),
        class_tag: crate::records::references::DesignClassTag::try_from("112".to_owned()).unwrap(),
        record_index: 1,
        paired_class_tag: crate::records::references::DesignClassTag::try_from("112".to_owned())
            .unwrap(),
        frame: DesignSketchFrame::new(
            10,
            if member {
                DesignSketchFrameForm::MemberCompact {
                    paired_byte_offset: 20,
                }
            } else {
                DesignSketchFrameForm::ScopeCompact
            },
        )
        .unwrap(),
    }
}

fn placement_native(
    placement: crate::records::sketch_placement::DesignSketchPlacement,
) -> crate::native::F3dNative {
    let mut native = crate::native::F3dNative::default();
    if placement.visibility.is_some() {
        native
            .design_entity_headers
            .push(validation_entity_header(Vec::new()));
    }
    native.design_sketch_placements.push(placement);
    native
}

#[test]
fn placement_contiguous_visibility_has_no_finding() {
    crate::test_support::with_decode_context(|service_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = placement_native(validation_placement(None, Some(1), true));
        let ctx = super::super::Ctx::new(&ir, &native, service_ctx);
        let mut findings = Vec::new();
        super::super::validate_sketch_placements(&ctx, &mut findings);
        assert!(findings.is_empty());
    })
}
