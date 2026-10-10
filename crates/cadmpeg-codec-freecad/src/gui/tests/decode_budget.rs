// SPDX-License-Identifier: Apache-2.0
//! Indexed GUI updates and early-exit resource admission.

use cadmpeg_core::decode::{DecodeContext, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::topology::{Body, BodyKind};

use super::super::{AppearancePlan, Assignment, BodyUpdate};

fn body(id: &str) -> Body {
    Body {
        id: BodyId::mint(id).expect("body identity"),
        kind: BodyKind::default(),
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    }
}

#[test]
fn gui_indexed_body_updates_keep_first_identity_and_source_order() {
    crate::test_support::with_service_context(&[], |ctx| {
        let mut ir = cadmpeg_ir::CadIr::empty();
        ir.model.bodies = vec![
            body("fcstd:model:body#a"),
            body("fcstd:model:body#b"),
            body("fcstd:model:body#a"),
        ];
        let mut plan = AppearancePlan::new(ctx).expect("plan storage");
        for (id, visible) in [
            ("fcstd:model:body#a", Some(false)),
            ("fcstd:model:body#b", Some(true)),
            ("fcstd:model:body#a", Some(true)),
            ("fcstd:model:body#missing", Some(false)),
        ] {
            plan.body_updates.push(BodyUpdate {
                id: BodyId::mint(id).expect("update identity"),
                visible: Assignment::Set(visible),
                color: None,
            });
        }
        plan.apply(ctx, &mut ir).expect("indexed updates");
        assert_eq!(ir.model.bodies[0].visible, Some(true));
        assert_eq!(ir.model.bodies[1].visible, Some(true));
        assert_eq!(ir.model.bodies[2].visible, None);
    });
}

#[test]
fn gui_body_index_lookup_refuses_before_body_mutation() {
    crate::test_support::refusal_at(
        ResourceDimension::WorkUnits,
        &[],
        "FCStd GUI body update lookup",
        |ctx| {
            let mut ir = cadmpeg_ir::CadIr::empty();
            ir.model.bodies.push(body("fcstd:model:body#a"));
            let mut plan = AppearancePlan::new(ctx)?;
            plan.body_updates.push(BodyUpdate {
                id: ir.model.bodies[0].id.clone(),
                visible: Assignment::Set(Some(false)),
                color: None,
            });
            let result = plan.apply(ctx, &mut ir);
            if let Err(CodecError::ResourceLimit(limit)) = &result {
                if limit.operation == "FCStd GUI body update lookup" {
                    assert_eq!(ir.model.bodies[0].visible, None);
                }
            }
            result
        },
    );
}

#[test]
fn gui_duplicate_child_search_stops_before_large_suffix() {
    let mut first_used = None;
    for suffix in [String::new(), "<Other/>".repeat(4096)] {
        let xml = format!("<Provider><Properties/><Properties/>{suffix}</Provider>");
        let tree = roxmltree::Document::parse(&xml).expect("provider XML");
        let error = crate::test_support::refusal_at(
            ResourceDimension::WorkUnits,
            &[],
            "FCStd GUI duplicate child container search",
            |ctx| super::super::unique_child(ctx, tree.root_element(), "Properties"),
        );
        let CodecError::ResourceLimit(limit) = error else {
            panic!("work refusal");
        };
        if let Some(used) = first_used {
            assert_eq!(limit.used, used);
        } else {
            first_used = Some(limit.used);
        }
        // The duplicate search has one raw child step before its tag query.
        assert_eq!(limit.additional, 1);
    }
}

#[test]
fn gui_attribute_lookup_refuses_before_provider_record_copy() {
    let xml = "<ViewProvider a='0' b='0' name='P'><Properties Count='0'/></ViewProvider>";
    let tree = roxmltree::Document::parse(xml).expect("provider XML");
    crate::test_support::refusal_at(
        ResourceDimension::WorkUnits,
        &[],
        "FCStd GUI provider attribute",
        |ctx| {
            let mut providers = Vec::new();
            let result = super::super::append_native_provider(
                ctx,
                xml,
                tree.root_element(),
                0,
                None,
                &mut providers,
                &mut Vec::new(),
            );
            if let Err(CodecError::ResourceLimit(limit)) = &result {
                if limit.operation == "FCStd GUI provider attribute" {
                    assert!(providers.is_empty());
                }
            }
            result
        },
    );
}

#[test]
fn gui_validator_buffers_use_materialized_admission() {
    let xml = roxmltree::Document::parse("<StringList count='1'><String value='x'/></StringList>")
        .expect("list XML");
    crate::test_support::materialized_refusal_at("FCStd GUI StringList child nodes", |ctx| {
        super::super::validate_gui_string_list(ctx, xml.root_element(), "names")
    });
    let bytes = [
        1_u32.to_le_bytes().as_slice(),
        0_f64.to_le_bytes().as_slice(),
    ]
    .concat();
    crate::test_support::materialized_refusal_at("FCStd GUI float-list entries", |ctx| {
        super::super::parse_float_list(
            ctx,
            cadmpeg_core::decode::View::over_retained(&bytes),
            "values",
        )
    });
}

#[test]
fn gui_nested_element_search_does_not_admit_unvisited_siblings() {
    for suffix in [String::new(), "<Other/>".repeat(4096)] {
        let xml = format!("<Value><Nested/>{suffix}</Value>");
        let tree = roxmltree::Document::parse(&xml).expect("value XML");
        let error = crate::test_support::refusal_at(
            ResourceDimension::WorkUnits,
            &[],
            "FCStd GUI nested XML element search",
            |ctx: &DecodeContext<'_>| {
                super::super::has_nested_gui_elements(ctx, tree.root_element())
            },
        );
        let CodecError::ResourceLimit(limit) = error else {
            panic!("work refusal");
        };
        assert_eq!(limit.used, 0);
        assert_eq!(limit.additional, 1);
    }
}

#[test]
fn gui_body_payload_index_preserves_nested_keys_and_repeated_sources() {
    crate::test_support::with_service_context(&[], |ctx| {
        let mut ir = cadmpeg_ir::CadIr::empty();
        ir.model.bodies = vec![
            body("fcstd:model:body#a:child:1"),
            body("fcstd:model:body#ab:1"),
            body("fcstd:model:body#a:2"),
        ];
        let mut index = super::super::TopologyIndex::new(&ir);
        index
            .ensure_bodies(
                ctx,
                [
                    "fcstd:payload#a:child",
                    "fcstd:payload#a",
                    "fcstd:payload#a:child",
                ],
            )
            .expect("body payload index");
        let selected = super::super::select_shape_bodies(
            ctx,
            &index.bodies,
            [
                "fcstd:payload#a:child",
                "fcstd:payload#a",
                "fcstd:payload#a:child",
            ],
        )
        .expect("payload selection");
        assert_eq!(
            selected,
            [
                &ir.model.bodies[0].id,
                &ir.model.bodies[0].id,
                &ir.model.bodies[2].id,
                &ir.model.bodies[0].id
            ]
        );
    });
}

#[test]
fn gui_blank_property_values_need_no_retained_copy() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let value = "x".repeat(65536);
    let (kept, refused, _storage) = super::super::gui_named_entries(
        &ctx,
        || ctx.copy_retained_text("record", "record name"),
        [("   ", value.as_str())].into_iter(),
    )
    .expect("blank values stay borrowed");
    assert!(kept.is_empty());
    assert!(
        matches!(refused.as_slice(), [cadmpeg_core::text::NamedEntryError::Blank { record }] if record == "record")
    );
}

#[test]
fn gui_planned_legacy_removal_preserves_other_appearance_order() {
    use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
    use cadmpeg_ir::ids::{AppearanceBindingId, AppearanceId};
    let appearance = |key: &str| Appearance {
        id: AppearanceId::mint(format!("fcstd:appearance:object#{key}")).expect("appearance id"),
        name: None,
        asset_guid: None,
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: None,
        category: None,
        base_color: None,
        textures: Vec::new(),
        properties: std::collections::BTreeMap::new(),
    };
    let binding = |key: &str, appearance: &Appearance| AppearanceBinding {
        id: AppearanceBindingId::mint(format!("fcstd:appearance:binding#{key}"))
            .expect("binding id"),
        target: AppearanceTarget::Body(BodyId::mint("fcstd:model:body#a").expect("body id")),
        appearance: appearance.id.clone(),
        source_entity_id: None,
        object_type: None,
        visible: None,
        channels: std::collections::BTreeMap::new(),
    };
    crate::test_support::with_service_context(&[], |ctx| {
        let mut ir = cadmpeg_ir::CadIr::empty();
        let mut plan = AppearancePlan::new(ctx).expect("plan storage");
        let legacy = appearance("legacy");
        let old = appearance("old");
        let new = appearance("new");
        ir.model.appearance_bindings = vec![binding("old-legacy", &legacy), binding("old", &old)];
        plan.bindings = vec![binding("new-legacy", &legacy), binding("new", &new)];
        plan.remove_appearances.insert(legacy.id.clone());
        ir.model.appearances = vec![appearance("legacy"), old];
        plan.appearances = vec![legacy, new];
        plan.apply(ctx, &mut ir).expect("apply removals");
        assert_eq!(
            ir.model
                .appearances
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            ["fcstd:appearance:object#old", "fcstd:appearance:object#new"]
        );
        assert_eq!(
            ir.model
                .appearance_bindings
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            [
                "fcstd:appearance:binding#old",
                "fcstd:appearance:binding#new"
            ]
        );
    });
}

#[test]
fn gui_deferred_removal_keeps_material_face_binding_identity() {
    use crate::native::element_map::{ElementMapGroup, ElementMappedName};
    use cadmpeg_ir::appearance::{AppearanceBinding, AppearanceTarget};
    use cadmpeg_ir::ids::{AppearanceBindingId, AppearanceId, FaceId, ShellId, SurfaceId};
    crate::test_support::with_service_context(&[], |ctx| {
        let mut ir = cadmpeg_ir::CadIr::empty();
        ir.model.faces.push(cadmpeg_ir::topology::Face {
            id: FaceId::mint("fcstd:model:face#face").expect("face id"),
            shell: ShellId::mint("fcstd:model:shell#shell").expect("shell id"),
            surface: SurfaceId::mint("fcstd:model:surface#surface").expect("surface id"),
            sense: cadmpeg_ir::topology::Sense::Forward,
            loops: cadmpeg_ir::topology::FaceLoops::unspecified(Vec::new()),
            name: None,
            color: None,
            tolerance: None,
        });
        let make_binding = |key: &str, appearance: &str| AppearanceBinding {
            id: AppearanceBindingId::mint(format!("fcstd:appearance:binding#{key}"))
                .expect("binding id"),
            target: AppearanceTarget::Body(BodyId::mint("fcstd:model:body#a").expect("body id")),
            appearance: AppearanceId::mint(appearance).expect("appearance id"),
            source_entity_id: None,
            object_type: None,
            visible: None,
            channels: std::collections::BTreeMap::new(),
        };
        ir.model
            .appearance_bindings
            .push(make_binding("existing", "fcstd:appearance:object#other"));
        let topology = super::super::TopologyIndex::new(&ir);
        let shape = super::super::ShapeIndex::new(ctx, &[], &[], &[]).expect("shape index");
        let graph = super::super::Graph {
            providers: vec![crate::native::GuiViewProviderRecord {
                id: "provider".into(),
                object: Some(
                    cadmpeg_core::text::NonBlankString::try_from("object").expect("object"),
                ),
                name: "P".into(),
                expanded: None,
                order: 0,
                raw_xml: String::new(),
            }],
            properties: vec![crate::native::GuiPropertyRecord {
                id: "material".into(),
                owner: "provider".into(),
                name: "ShapeAppearance".into(),
                type_name: "App::PropertyMaterialList".into(),
                status: None,
                order: 0,
                values: Vec::new(),
                side_entries: Vec::new(),
                xml: crate::native::RetainedXml::from_text("<Property/>".into(), 0).expect("XML"),
            }],
            ..Default::default()
        };
        let zero = cadmpeg_ir::scalar::FiniteBinary32::new(0.0).expect("zero");
        let materials = std::collections::HashMap::from([(
            "material",
            vec![super::super::GuiMaterial {
                ambient: 0,
                diffuse: 0,
                specular: 0,
                emissive: 0,
                shininess: zero,
                transparency: zero,
                uuid: "",
            }],
        )]);
        let mut plan = AppearancePlan::new(ctx).expect("plan");
        plan.bindings = vec![
            make_binding("legacy-1", "fcstd:appearance:object#P"),
            make_binding("keep", "fcstd:appearance:object#other"),
            make_binding("legacy-2", "fcstd:appearance:object#P"),
        ];
        let entries = std::collections::BTreeMap::new();
        let sources = super::super::GuiSources {
            entries: &entries,
            objects: &[],
            properties: &[],
            payloads: &[],
            element_maps: &[],
            requires_alpha_conversion: false,
        };
        let mut shape_index = Some(shape);
        let mut topology_index = Some(topology);
        super::super::transfer_shape_appearances(
            ctx,
            &mut plan,
            &graph,
            &materials,
            &sources,
            &ir,
            &mut shape_index,
            &mut topology_index,
            &mut Vec::new(),
        )
        .expect("single material replacement");
        let group = ElementMapGroup {
            indexed_name: "Face".into(),
            children: Vec::new(),
            names: vec![
                Vec::new(),
                vec![ElementMappedName {
                    encoded: "Face1".into(),
                    resolved: None,
                    string_ids: Vec::new(),
                    topology_ids: vec!["fcstd:model:face#face".into()],
                }],
            ],
        };
        let key = cadmpeg_ir::identity_key!("Q");
        let appearance =
            AppearanceId::mint("fcstd:appearance:shape-material#Q:1").expect("appearance");
        let topology = topology_index.as_mut().expect("topology index");
        super::super::bind_material_faces(
            ctx,
            topology,
            &mut plan,
            &group,
            0,
            &appearance,
            (&key, "other"),
        )
        .expect("face binding");
        // One existing binding plus one surviving planned binding precedes this face.
        assert_eq!(
            plan.bindings.last().expect("face binding").id.as_str(),
            "fcstd:appearance:binding#shape-material:Q:2"
        );
        plan.apply(ctx, &mut ir).expect("apply");
        assert_eq!(ir.model.appearance_bindings.len(), 3);
    });
}

#[test]
fn gui_material_text_stays_borrowed_until_output_transfer() {
    let fields = [
        b"image".as_slice(),
        b"image-path".as_slice(),
        b"material-guid".as_slice(),
    ];
    let mut bytes = 1_u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(&[0_u8; 24]);
    for text in fields {
        bytes.extend_from_slice(
            &u32::try_from(text.len())
                .expect("field length")
                .to_le_bytes(),
        );
        bytes.extend_from_slice(text);
    }
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let (materials, _storage) = ctx
        .with_scoped_storage("material cache", || {
            super::super::parse_material_list(
                &ctx,
                cadmpeg_core::decode::View::over_retained(&bytes),
                3,
                "material",
                false,
            )
        })
        .expect("no retained scratch copies");
    assert_eq!(materials[0].uuid, "material-guid");
    assert_eq!(
        materials[0].uuid.as_ptr(),
        bytes[bytes.len() - fields[2].len()..].as_ptr()
    );
}

#[test]
fn gui_material_text_validates_each_field_before_borrowing() {
    for invalid in 0..3 {
        let mut bytes = 1_u32.to_le_bytes().to_vec();
        bytes.extend_from_slice(&[0_u8; 24]);
        for field in 0..3 {
            bytes.extend_from_slice(&1_u32.to_le_bytes());
            bytes.push(if field == invalid { 0xff } else { b'x' });
        }
        crate::test_support::with_service_context(&bytes, |ctx| {
            let error = super::super::parse_material_list(
                ctx,
                cadmpeg_core::decode::View::over_retained(&bytes),
                3,
                "material",
                false,
            )
            .err()
            .expect("invalid material UTF-8");
            assert!(
                matches!(error, CodecError::Malformed(ref message) if message.contains("string is not UTF-8"))
            );
        });
    }
}
