// SPDX-License-Identifier: Apache-2.0
//! GUI XML admission and retained-copy tests.

use crate::test_support::test_archive::archive_entries;
use crate::FcstdCodec;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

fn populated_appearance_plan() -> super::super::AppearancePlan {
    use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
    use cadmpeg_ir::ids::{AppearanceBindingId, AppearanceId, BodyId};
    use cadmpeg_ir::presentation::{PresentationDocument, PresentationId, ViewPresentation};

    let appearance_id = AppearanceId::mint("fcstd:appearance:object#sample")
        .expect("valid appearance identity");
    let mut plan = super::super::AppearancePlan::default();
    plan.appearances.push(Appearance {
        id: appearance_id.clone(),
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
    });
    plan.bindings.push(AppearanceBinding {
        id: AppearanceBindingId::mint("fcstd:appearance:binding#sample")
            .expect("valid binding identity"),
        target: AppearanceTarget::Body(BodyId::mint("fcstd:model:body#sample")
            .expect("valid body identity")),
        appearance: appearance_id,
        source_entity_id: None,
        object_type: None,
        visible: None,
        channels: std::collections::BTreeMap::new(),
    });
    plan.presentation_documents.push(PresentationDocument::new(
        PresentationId::mint("fcstd:model:presentation-document#sample")
            .expect("valid presentation identity"),
    ));
    plan.view_presentations.push(ViewPresentation {
        id: PresentationId::mint("fcstd:model:presentation-view#sample")
            .expect("valid view identity"),
        object: None,
        order: 0,
        expanded: None,
        visible: None,
        display_mode: None,
        selection_style: None,
        line_width: None,
        point_size: None,
        properties: std::collections::BTreeMap::new(),
        native_ref: None,
    });
    plan
}

fn assert_plan_application_refusal(limit: u64, operation: &str) {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is within policy");
    let error = populated_appearance_plan()
        .apply(&ctx, &mut cadmpeg_ir::CadIr::empty())
        .expect_err("plan application must charge its target collection");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(ref failure)
        if failure.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && failure.operation == operation), "{error:?}");
}

#[test]
fn neutral_appearances_refuse_at_caller_limit() {
    assert_plan_application_refusal(0, "FCStd neutral appearances");
}

#[test]
fn neutral_appearance_bindings_refuse_at_caller_limit() {
    assert_plan_application_refusal(1, "FCStd neutral appearance bindings");
}

#[test]
fn neutral_presentation_documents_refuse_at_caller_limit() {
    assert_plan_application_refusal(2, "FCStd neutral presentation documents");
}

#[test]
fn neutral_view_presentations_refuse_at_caller_limit() {
    assert_plan_application_refusal(3, "FCStd neutral view presentations");
}

#[test]
fn gui_color_list_refuses_at_caller_limit() {
    let bytes = [1_u32.to_le_bytes(), 0x1122_3344_u32.to_le_bytes()].concat();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is within policy");
    let error = super::super::parse_color_list(
        &ctx,
        cadmpeg_core::decode::View::over_retained(&bytes),
        "colors.bin",
        false,
    )
    .expect_err("color list must charge before retaining its entries");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(ref failure)
        if failure.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && failure.operation == "FCStd GUI color-list entries"), "{error:?}");
}

fn assert_gui_binary_list_refusal(
    payload_len: usize,
    limit: u64,
    operation: &str,
    parse: impl FnOnce(
        &cadmpeg_core::decode::DecodeContext<'_>,
        cadmpeg_core::decode::View<'_>,
    ) -> Result<(), cadmpeg_core::CodecError>,
) {
    let mut bytes = 1_u32.to_le_bytes().to_vec();
    bytes.extend(std::iter::repeat_n(0_u8, payload_len));
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is within policy");
    let error = parse(&ctx, cadmpeg_core::decode::View::over_retained(&bytes))
        .expect_err("binary list must charge before retaining its records");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(ref failure)
        if failure.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && failure.operation == operation), "{error:?}");
}

#[test]
fn gui_float_list_refuses_at_caller_limit() {
    assert_gui_binary_list_refusal(8, 0, "FCStd GUI float-list entries",
        |ctx, view| super::super::parse_float_list(ctx, view, "floats"));
}

#[test]
fn gui_vector_list_refuses_at_caller_limit() {
    assert_gui_binary_list_refusal(24, 0, "FCStd GUI vector-list entries",
        |ctx, view| super::super::parse_vector_list(ctx, view, "vectors"));
}

#[test]
fn gui_placement_list_refuses_at_caller_limit() {
    assert_gui_binary_list_refusal(56, 0, "FCStd GUI placement-list entries",
        |ctx, view| super::super::parse_placement_list(ctx, view, "placements"));
}

#[test]
fn gui_fillet_edge_list_refuses_at_caller_limit() {
    assert_gui_binary_list_refusal(20, 0, "FCStd GUI fillet-edge entries",
        |ctx, view| super::super::parse_fillet_edges(ctx, view, "fillets"));
}

#[test]
fn gui_raw_material_list_refuses_at_caller_limit() {
    assert_gui_binary_list_refusal(24, 0, "FCStd GUI raw material entries",
        |ctx, view| super::super::parse_material_list(ctx, view, 2, "material", false).map(|_| ()));
}

#[test]
fn gui_material_list_refuses_at_caller_limit() {
    assert_gui_binary_list_refusal(24, 1, "FCStd GUI material entries",
        |ctx, view| super::super::parse_material_list(ctx, view, 2, "material", false).map(|_| ()));
}

#[test]
fn gui_material_string_refuses_at_caller_limit() {
    let mut bytes = 1_u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(&[0_u8; 24]);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.push(b'x');
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is within policy");
    let error = super::super::parse_material_list(
        &ctx, cadmpeg_core::decode::View::over_retained(&bytes), 3, "material", false,
    )
    .err()
    .expect("material string must charge before retaining its bytes");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(ref failure)
        if failure.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
            && failure.operation == "FCStd GUI material string"), "{error:?}");
}

#[test]
fn gui_property_identity_refuses_at_retained_limit() {
    let text = r#"<ViewProvider name="Model"><Properties Count="1"><Property name="Visible" type="App::PropertyBool"><Bool value="true"/></Property></Properties></ViewProvider>"#;
    let xml = roxmltree::Document::parse(text).expect("GUI provider XML");
    crate::test_support::assert_retained_refusal_at(text.as_bytes(),
        "FreeCAD native child identity", |ctx| {
            super::super::append_native_provider(ctx, text, xml.root_element(), 0, None,
                &mut Vec::new(), &mut Vec::new())
        });
}

#[test]
fn gui_provider_identity_refuses_at_retained_limit() {
    let text = r#"<ViewProvider name="Provider With Spaces"><Properties Count="0"/></ViewProvider>"#;
    let xml = roxmltree::Document::parse(text).expect("GUI provider XML");
    let id_len = crate::native::native_id("gui-view-provider", "Provider With Spaces").len();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(id_len - 1).expect("identity length fits u64");
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(text.as_bytes(), &arena, &policy)
        .expect("provider XML is within the root limit");
    let error = super::super::append_native_provider(
        &ctx, text, xml.root_element(), 0, None, &mut Vec::new(), &mut Vec::new(),
    )
    .expect_err("provider identity must charge before construction");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(ref failure)
        if failure.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
            && failure.operation == "FreeCAD native identity"), "{error:?}");
}

#[test]
fn gui_provider_record_identity_copy_refuses_at_retained_limit() {
    let text = r#"<ViewProvider name="Provider With Spaces"><Properties Count="0"/></ViewProvider>"#;
    let xml = roxmltree::Document::parse(text).expect("GUI provider XML");
    crate::test_support::assert_retained_refusal_at(text.as_bytes(),
        "FCStd GUI provider record identity", |ctx| {
            super::super::append_native_provider(ctx, text, xml.root_element(), 0, None,
                &mut Vec::new(), &mut Vec::new())
        });
}

#[test]
fn gui_provider_object_identity_refuses_at_retained_limit() {
    let text = r#"<ViewProvider name="Provider"><Properties Count="0"/></ViewProvider>"#;
    let xml = roxmltree::Document::parse(text).expect("GUI provider XML");
    crate::test_support::assert_retained_refusal_at(text.as_bytes(),
        "FCStd GUI provider object identity", |ctx| {
            super::super::append_native_provider(ctx, text, xml.root_element(), 0,
                Some("fcstd:native:object#Owner With Spaces"), &mut Vec::new(), &mut Vec::new())
        });
}

#[test]
fn gui_provider_key_refuses_before_encoding() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = "A%20B%23".len() as u64 - 1;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is within policy");
    let error = super::super::provider_identity_key(&ctx, "A B#")
        .expect_err("encoded provider key must be charged");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(ref failure)
        if failure.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
            && failure.operation == "FCStd GUI provider key"), "{error:?}");
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is within policy");
    assert_eq!(super::super::provider_identity_key(&ctx, "A B#")
        .expect("service policy admits encoded key").as_str(), "A%20B%23");
}

#[test]
fn gui_object_appearance_identity_refuses_at_retained_limit() {
    let key = cadmpeg_ir::ids::IdentityKey::encode_segment("A B#");
    let identity = format!("fcstd:appearance:object#{key}");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = identity.len() as u64 - 1;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is within policy");
    let error = super::super::object_appearance_id(&ctx, &key)
        .expect_err("appearance identity must charge before construction");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(ref failure)
        if failure.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
            && failure.operation == "FCStd GUI object appearance identity"), "{error:?}");
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is within policy");
    assert_eq!(super::super::object_appearance_id(&ctx, &key)
        .expect("service policy admits appearance").as_str(), identity);
}

fn assert_gui_identity_refusal(
    expected: &str,
    operation: &str,
    make: impl Fn(&cadmpeg_core::decode::DecodeContext<'_>) -> Result<String, cadmpeg_core::CodecError>,
) {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = expected.len() as u64 - 1;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is within policy");
    let error = make(&ctx).expect_err("identity must charge before construction");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(ref failure)
        if failure.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
            && failure.operation == operation), "{error:?}");
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is within policy");
    assert_eq!(make(&ctx).expect("service policy admits identity"), expected);
}

#[test]
fn gui_edge_appearance_identity_refuses_at_retained_limit() {
    let key = cadmpeg_ir::ids::IdentityKey::encode_segment("A B#");
    assert_gui_identity_refusal("fcstd:appearance:edge#A%20B%23",
        "FCStd GUI edge appearance identity",
        |ctx| super::super::edge_appearance_id(ctx, &key).map(|id| id.to_string()));
}

#[test]
fn gui_vertex_appearance_identity_refuses_at_retained_limit() {
    let key = cadmpeg_ir::ids::IdentityKey::encode_segment("A B#");
    assert_gui_identity_refusal("fcstd:appearance:vertex#A%20B%23",
        "FCStd GUI vertex appearance identity",
        |ctx| super::super::vertex_appearance_id(ctx, &key).map(|id| id.to_string()));
}

#[test]
fn gui_shape_material_appearance_identity_refuses_at_retained_limit() {
    let key = cadmpeg_ir::ids::IdentityKey::encode_segment("A B#");
    assert_gui_identity_refusal("fcstd:appearance:shape-material#A%20B%23:2",
        "FCStd GUI shape material appearance identity",
        |ctx| super::super::shape_material_appearance_id(ctx, &key, 1).map(|id| id.to_string()));
}

#[test]
fn gui_topology_appearance_identity_refuses_at_retained_limit() {
    let key = cadmpeg_ir::ids::IdentityKey::encode_segment("A B#");
    assert_gui_identity_refusal("fcstd:appearance:face#A%20B%23:2",
        "FCStd GUI topology appearance identity",
        |ctx| super::super::topology_appearance_id(ctx,
            super::super::TopologyColorKind::Face, &key, 1).map(|id| id.to_string()));
}

#[test]
fn gui_appearance_binding_identity_refuses_at_retained_limit() {
    assert_gui_identity_refusal("fcstd:appearance:binding#face:A%20B%23:2:Face1",
        "FCStd GUI appearance binding identity",
        |ctx| super::super::binding_id(ctx,
            format_args!("fcstd:appearance:binding#face:A%20B%23:2:Face1"))
            .map(|id| id.to_string()));
}

#[test]
fn y4_2_decode_refuses_unadmitted_gui_text_copy() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="0"/><ObjectData Count="0"/></Document>"#;
    let gui = br#"<Document SchemaVersion="1"><Camera settings=""/></Document>"#;
    let bytes = archive_entries(&[("Document.xml", document), ("GuiDocument.xml", gui)]);
    FcstdCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .expect("service profile admits the GUI state");

    let mut options = DecodeOptions::default();
    // The ZIP snapshot retains four copies of each decoded central-directory name.
    let zip_names = 4 * ("Document.xml".len() + "GuiDocument.xml".len());
    options.policy.limits.max_retained_bytes = (zip_names + document.len() + gui.len()) as u64;
    let mut error = None;
    for _ in 0..256 {
        let refused = FcstdCodec
            .decode(&mut Cursor::new(&bytes), &options)
            .expect_err("GUI text copy must be admitted");
        let cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit)) =
            &refused
        else {
            panic!("expected retained refusal: {refused:?}");
        };
        assert_eq!(
            limit.dimension,
            cadmpeg_core::decode::ResourceDimension::RetainedBytes
        );
        if limit.operation.starts_with("FCStd GUI ") {
            let exact = limit.used + limit.additional - 1;
            options.policy.limits.max_retained_bytes = exact;
            error = Some(
                FcstdCodec
                    .decode(&mut Cursor::new(&bytes), &options)
                    .expect_err("one byte below GUI text need refuses"),
            );
            break;
        }
        let next = limit.used + limit.additional;
        assert!(next > options.policy.limits.max_retained_bytes);
        options.policy.limits.max_retained_bytes = next;
    }
    let error = error.expect("GUI charge reached within fixture admissions");
    assert!(
        matches!(
            &error,
            cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                    && limit.operation.starts_with("FCStd GUI ")
        ),
        "{error:?}"
    );
}

#[test]
fn y4_2_gui_xml_tree_is_admitted_before_allocation() {
    let xml = br#"<Document SchemaVersion="1"><Camera settings=""/></Document>"#;
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(xml, &arena, &policy)
        .expect("service GUI context");
    super::super::transfer(
        &ctx,
        &mut cadmpeg_ir::CadIr::empty(),
        xml,
        &std::collections::BTreeMap::new(),
        &[],
        &[],
        &[],
        &[],
        false,
    )
    .expect("service profile admits the GUI document");

    let mut limited = cadmpeg_core::decode::DecodePolicy::service();
    limited.limits.max_collection_items = 1;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(xml, &arena, &limited)
        .expect("limited GUI context");
    let error = super::super::transfer(
        &ctx,
        &mut cadmpeg_ir::CadIr::empty(),
        xml,
        &std::collections::BTreeMap::new(),
        &[],
        &[],
        &[],
        &[],
        false,
    )
    .err()
    .expect("GUI XML node count must be charged before parsing");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                && limit.operation == "FCStd GUI XML node tree"
    ));
}

#[test]
fn gui_state_records_refuse_at_caller_limit() {
    let text = "<Document><Camera/></Document>";
    let xml = roxmltree::Document::parse(text).expect("GUI document XML");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(text.as_bytes(), &arena, &policy)
        .expect("GUI document context");
    assert!(matches!(super::super::transfer_schema_one(&ctx, &cadmpeg_ir::CadIr::empty(), text,
        &xml, None, None, &std::collections::BTreeMap::new(), &[], &[], &[], &[], false),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "FCStd GUI state records"));
}

#[test]
fn gui_object_name_index_refuses_at_caller_limit() {
    let text = "<Document><Camera/></Document>";
    let xml = roxmltree::Document::parse(text).expect("GUI document XML");
    let object = crate::native::ObjectRecord {
        id: "fcstd:native:object#P".into(), name: "P".into(), type_name: "Part::Feature".into(),
        persistent_id: None, view_type: None, attributes: std::collections::BTreeMap::new(),
        dependencies: Vec::new(), dependency_allow_partial: None, order: 0, data: None,
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(text.as_bytes(), &arena, &policy)
        .expect("GUI document context");
    assert!(matches!(super::super::transfer_schema_one(&ctx, &cadmpeg_ir::CadIr::empty(), text,
        &xml, None, None, &std::collections::BTreeMap::new(), &[object], &[], &[], &[], false),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "FCStd GUI object names"));
}

#[test]
fn gui_presentation_document_refuses_at_caller_limit() {
    let graph = super::super::Graph {
        documents: vec![crate::native::GuiDocumentRecord {
            id: "fcstd:gui:document#0".into(), schema_version: None,
            attributes: std::collections::BTreeMap::new(), states: Vec::new(),
        }],
        ..Default::default()
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is within policy");
    assert!(matches!(super::super::transfer_neutral_presentation(&ctx,
        &mut super::super::AppearancePlan::default(), &graph, None, &mut Vec::new()),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "FCStd presentation documents"));
}

#[test]
fn gui_view_presentation_refuses_at_caller_limit() {
    let graph = super::super::Graph {
        providers: vec![crate::native::GuiViewProviderRecord {
            id: "fcstd:gui:view-provider#P".into(), object: None, name: "P".into(),
            expanded: None, order: 0, raw_xml: String::new(),
        }],
        ..Default::default()
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is within policy");
    assert!(matches!(super::super::transfer_neutral_presentation(&ctx,
        &mut super::super::AppearancePlan::default(), &graph, None, &mut Vec::new()),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "FCStd view presentations"));
}

#[test]
fn gui_presentation_states_refuse_at_caller_limit() {
    let graph = super::super::Graph {
        documents: vec![crate::native::GuiDocumentRecord {
            id: "fcstd:gui:document#0".into(), schema_version: None,
            attributes: std::collections::BTreeMap::new(),
            states: vec![crate::native::GuiStateRecord {
                id: "fcstd:gui:state#0".into(), kind: "Other".into(),
                attributes: std::collections::BTreeMap::new(), values: Vec::new(), side_entries: Vec::new(),
                xml: crate::native::RetainedXml::from_text("<Other/>".into(), 0).expect("valid state XML"),
            }],
        }],
        ..Default::default()
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is within policy");
    assert!(matches!(super::super::transfer_neutral_presentation(&ctx,
        &mut super::super::AppearancePlan::default(), &graph, None, &mut Vec::new()),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "FCStd presentation states"));
}

#[test]
fn gui_presentation_assets_refuse_at_caller_limit() {
    let graph = super::super::Graph {
        documents: vec![crate::native::GuiDocumentRecord {
            id: "fcstd:gui:document#0".into(), schema_version: None,
            attributes: std::collections::BTreeMap::new(),
            states: vec![crate::native::GuiStateRecord {
                id: "fcstd:gui:state#0".into(), kind: "Other".into(),
                attributes: std::collections::BTreeMap::new(), values: Vec::new(),
                side_entries: vec!["asset".into()],
                xml: crate::native::RetainedXml::from_text("<Other/>".into(), 0).expect("valid state XML"),
            }],
        }],
        ..Default::default()
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is within policy");
    assert!(matches!(super::super::transfer_neutral_presentation(&ctx,
        &mut super::super::AppearancePlan::default(), &graph, None, &mut Vec::new()),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "FCStd presentation assets"));
}

#[test]
fn gui_asset_identity_refuses_at_retained_limit() {
    let document_id = "fcstd:gui:document#0";
    let graph = super::super::Graph {
        documents: vec![crate::native::GuiDocumentRecord {
            id: document_id.into(), schema_version: None,
            attributes: std::collections::BTreeMap::new(),
            states: vec![crate::native::GuiStateRecord {
                id: "fcstd:gui:state#0".into(), kind: "Other".into(),
                attributes: std::collections::BTreeMap::new(), values: Vec::new(),
                side_entries: vec!["asset".into()],
                xml: crate::native::RetainedXml::from_text("<Other/>".into(), 0).expect("valid state XML"),
            }],
        }],
        ..Default::default()
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = (document_id.len() + "Other".len()
        + crate::native::native_id("entry", "asset").len() - 1) as u64;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is within policy");
    assert!(matches!(super::super::transfer_neutral_presentation(&ctx,
        &mut super::super::AppearancePlan::default(), &graph, None, &mut Vec::new()),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "FreeCAD native identity"));
}

#[test]
fn gui_state_identity_refuses_at_retained_limit() {
    let xml = "<Camera/>";
    let document = roxmltree::Document::parse(xml).expect("GUI state XML");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = ("Camera".len() + xml.len() + "Camera:0".len()
        + crate::native::native_id("gui-state", "Camera:0").len() - 1) as u64;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &policy)
        .expect("GUI state context");
    assert!(matches!(super::super::gui_state(&ctx, xml, 0, document.root_element()),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "FreeCAD native identity"));
}

#[test]
fn gui_presentation_property_map_refuses_at_caller_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is within policy");
    assert!(matches!(super::super::gui_named_entries(&ctx, || Ok("record".into()), [("key", "value")]),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "FCStd GUI presentation property map"));
}

#[test]
fn gui_refused_property_list_refuses_at_caller_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is within policy");
    assert!(matches!(super::super::gui_named_entries(&ctx, || Ok("record".into()),
        [("key", "first"), ("key", "second")]),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "FCStd GUI refused property keys"));
}

#[test]
fn gui_refused_property_loss_refuses_at_caller_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is within policy");
    let refused = [cadmpeg_core::text::NamedEntryError::Blank { record: "record".into() }];
    assert!(matches!(super::super::charge_refused_gui_keys(&ctx, &mut Vec::new(), &refused),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "FCStd GUI refused property losses"));
}

fn assert_gui_state_service(xml: &str) {
    let document = roxmltree::Document::parse(xml).expect("GUI state XML");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &policy)
            .expect("service GUI state context");
    super::super::gui_state(&ctx, xml, 0, document.root_element())
        .expect("service profile admits the GUI state");
}

fn assert_gui_provider_service(xml: &str) {
    let document = roxmltree::Document::parse(xml).expect("GUI provider XML");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &policy)
            .expect("service GUI provider context");
    let mut providers = Vec::new();
    let mut properties = Vec::new();
    super::super::append_native_provider(
        &ctx,
        xml,
        document.root_element(),
        0,
        None,
        &mut providers,
        &mut properties,
    )
    .expect("service profile admits the GUI provider");
}

#[test]
fn gui_provider_record_refuses_at_caller_limit() {
    let xml = "<ViewProvider name=\"P\"><Properties Count=\"0\"/></ViewProvider>";
    let document = roxmltree::Document::parse(xml).expect("GUI provider XML");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &policy)
        .expect("GUI provider context");
    assert!(matches!(super::super::append_native_provider(&ctx, xml, document.root_element(), 0,
        None, &mut Vec::new(), &mut Vec::new()),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "FCStd GUI provider records"));
}

#[test]
fn gui_provider_property_nodes_refuse_at_caller_limit() {
    let xml = "<ViewProvider name=\"P\"><Properties Count=\"1\"><Property name=\"A\" type=\"T\"/></Properties></ViewProvider>";
    let document = roxmltree::Document::parse(xml).expect("GUI provider XML");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &policy)
        .expect("GUI provider context");
    assert!(matches!(super::super::append_native_provider(&ctx, xml, document.root_element(), 0,
        None, &mut Vec::new(), &mut Vec::new()),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "FCStd GUI provider property nodes"));
}

#[test]
fn gui_side_entry_reference_refuses_at_caller_limit() {
    let xml = "<ViewProvider name=\"P\"><Properties Count=\"1\"><Property name=\"A\" type=\"T\"><X file=\"asset\"/></Property></Properties></ViewProvider>";
    assert_gui_provider_service(xml);
    let document = roxmltree::Document::parse(xml).expect("GUI provider XML");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 4;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &policy)
        .expect("GUI provider context");
    assert!(matches!(super::super::append_native_provider(&ctx, xml, document.root_element(), 0,
        None, &mut Vec::new(), &mut Vec::new()),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "FCStd GUI side entry references"));
}

#[test]
fn y4_2_gui_state_xml_copy_refuses_at_the_retained_byte_limit() {
    let xml = "<Camera/>";
    assert_gui_state_service(xml);
    let node = roxmltree::Document::parse(xml).expect("GUI state XML");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = "Camera".len() as u64;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &policy)
            .expect("GUI state context");
    let error = super::super::gui_state(&ctx, xml, 0, node.root_element())
        .expect_err("GUI state raw XML copy must be charged");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                && limit.operation == "FCStd GUI state XML"
    ));
}

#[test]
fn y4_2_gui_state_values_are_admitted_before_allocation() {
    let xml = "<Camera><Settings/></Camera>";
    assert_gui_state_service(xml);
    let document = roxmltree::Document::parse(xml).expect("GUI state XML");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &policy)
            .expect("GUI state context");
    let error = super::super::gui_state(&ctx, xml, 0, document.root_element())
        .expect_err("GUI state values must be charged before allocation");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                && limit.operation == "FCStd GUI state values"
    ));
}

#[test]
fn gui_state_side_entry_refuses_at_caller_limit() {
    let xml = "<Camera><Settings file=\"asset\"/></Camera>";
    assert_gui_state_service(xml);
    let document = roxmltree::Document::parse(xml).expect("GUI state XML");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 2;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &policy)
        .expect("GUI state context");
    assert!(matches!(super::super::gui_state(&ctx, xml, 0, document.root_element()),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "FCStd GUI side entry references"));
}

#[test]
fn y4_2_gui_provider_xml_copy_refuses_at_the_retained_byte_limit() {
    let xml = "<ViewProvider name=\"P\"><Properties Count=\"0\"/></ViewProvider>";
    assert_gui_provider_service(xml);
    let document = roxmltree::Document::parse(xml).expect("GUI provider XML");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 1;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &policy)
            .expect("GUI provider context");
    let mut providers = Vec::new();
    let mut properties = Vec::new();
    let error = super::super::append_native_provider(
        &ctx,
        xml,
        document.root_element(),
        0,
        None,
        &mut providers,
        &mut properties,
    )
    .expect_err("GUI provider raw XML copy must be charged");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                && limit.operation == "FCStd GUI provider XML"
    ));
}

#[test]
fn y4_2_gui_property_xml_copy_refuses_at_the_retained_byte_limit() {
    let xml = "<ViewProvider name=\"P\"><Properties Count=\"1\"><Property name=\"P\" type=\"T\"/></Properties></ViewProvider>";
    assert_gui_provider_service(xml);
    let document = roxmltree::Document::parse(xml).expect("GUI provider XML");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 1 + xml.len() as u64 + 1 + 1;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &policy)
            .expect("GUI property context");
    let mut providers = Vec::new();
    let mut properties = Vec::new();
    let error = super::super::append_native_provider(
        &ctx,
        xml,
        document.root_element(),
        0,
        None,
        &mut providers,
        &mut properties,
    )
    .expect_err("GUI property raw XML copy must be charged");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                && limit.operation == "FCStd GUI property XML"
    ));
}

#[test]
fn y4_2_gui_property_values_are_admitted_before_allocation() {
    let xml = "<ViewProvider name=\"P\"><Properties Count=\"1\"><Property name=\"P\" type=\"T\"><X/></Property></Properties></ViewProvider>";
    assert_gui_provider_service(xml);
    let document = roxmltree::Document::parse(xml).expect("GUI provider XML");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 2;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &policy)
            .expect("GUI property context");
    let mut providers = Vec::new();
    let mut properties = Vec::new();
    let error = super::super::append_native_provider(
        &ctx,
        xml,
        document.root_element(),
        0,
        None,
        &mut providers,
        &mut properties,
    )
    .expect_err("GUI property values must be charged before allocation");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                && limit.operation == "FCStd GUI property values"
    ));
}
