// SPDX-License-Identifier: Apache-2.0
//! #213: existing GUI projections retain their actual property owners.

use super::*;
use cadmpeg_ir::schema::rewrite::typed::{IdentityMap, RewriteIdentities};

const SOURCE: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../corpus/freecad_fcstd/fixtures/gui_appearance.FCStd"
));

fn decoded() -> cadmpeg_ir::CadIr {
    FcstdCodec
        .decode(&mut Cursor::new(SOURCE), &DecodeOptions::default())
        .unwrap()
        .ir()
        .clone()
}

fn native_properties(ir: &cadmpeg_ir::CadIr) -> Vec<crate::native::GuiPropertyRecord> {
    ir.native
        .namespace("fcstd")
        .unwrap()
        .arena_as("gui_properties")
        .unwrap()
}

#[test]
fn projected_gui_appearances_and_assignments_keep_actual_property_native_references() {
    let ir = decoded();
    let properties = native_properties(&ir);
    let providers = ir
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::GuiViewProviderRecord>("gui_view_providers")
        .unwrap();
    assert!(!ir.model.appearances.is_empty());
    assert!(!ir.model.appearance_bindings.is_empty());
    for appearance in &ir.model.appearances {
        let native = appearance
            .native_ref
            .as_deref()
            .expect("actual GUI field owner");
        let property = properties
            .iter()
            .find(|record| record.id == native)
            .unwrap();
        assert!(providers
            .iter()
            .any(|provider| provider.id == property.owner));
        let (name, runtime_type) = match appearance.schema.as_deref().unwrap() {
            "FCStd ShapeAppearance" => ("ShapeAppearance", "App::PropertyMaterialList"),
            "FCStd ViewProvider ShapeMaterial" => ("ShapeMaterial", "App::PropertyMaterial"),
            "FCStd ViewProvider line style" => ("LineColor", "App::PropertyColor"),
            "FCStd ViewProvider point style" => ("PointColor", "App::PropertyColor"),
            "FCStd DiffuseColor" => ("DiffuseColor", "App::PropertyColorList"),
            "FCStd LineColorArray" => ("LineColorArray", "App::PropertyColorList"),
            "FCStd PointColorArray" => ("PointColorArray", "App::PropertyColorList"),
            other => panic!("the authored GUI fixture has an unexpected appearance schema {other}"),
        };
        assert_eq!(property.name, name);
        assert_eq!(property.type_name, runtime_type);
        assert!(!property.values.is_empty());
    }
    for binding in &ir.model.appearance_bindings {
        let appearance = ir
            .model
            .appearances
            .iter()
            .find(|record| record.id == binding.appearance)
            .unwrap();
        assert_eq!(binding.native_ref, appearance.native_ref);
        let native = binding.native_ref.as_deref().unwrap();
        assert!(properties.iter().any(|record| record.id == native));
    }
    let report = cadmpeg_ir::validate_neutral(&ir, Vec::new()).unwrap();
    assert!(!report
        .findings
        .iter()
        .any(|finding| finding.check == cadmpeg_ir::report::check::Check::NativeLinks));
}

#[test]
fn gui_property_native_references_survive_opaque_rewrite_and_arena_reordering() {
    let mut ir = decoded();
    let mut properties = native_properties(&ir);
    let property = properties
        .iter_mut()
        .find(|record| record.name == "LineColor")
        .unwrap();
    let original = property.id.clone();
    let changed = "opaque:native:GUI-property#line-color";
    let mut expected_property = property.clone();
    expected_property.id = changed.to_owned();
    property.id = changed.to_owned();
    assert!(ir
        .model
        .appearances
        .iter()
        .any(|record| record.native_ref.as_deref() == Some(original.as_str())));
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut map = IdentityMap::new(|id: &str| {
        Ok(if id == original {
            changed.to_owned()
        } else {
            id.to_owned()
        })
    });
    ir.model.appearances = std::mem::take(&mut ir.model.appearances)
        .rewrite_identities(&ctx, &mut map)
        .unwrap();
    ir.model.appearance_bindings = std::mem::take(&mut ir.model.appearance_bindings)
        .rewrite_identities(&ctx, &mut map)
        .unwrap();
    map.finish().unwrap();
    properties.reverse();
    ir.native
        .namespace_mut("fcstd")
        .set_arena(&ctx, "gui_properties", &properties)
        .unwrap();
    ir.model.appearances.reverse();
    ir.model.appearance_bindings.reverse();
    assert!(ir
        .model
        .appearances
        .iter()
        .any(|record| record.native_ref.as_deref() == Some(changed)));
    assert!(!ir
        .model
        .appearances
        .iter()
        .any(|record| record.native_ref.as_deref() == Some(original.as_str())));
    assert_eq!(
        native_properties(&ir)
            .into_iter()
            .find(|record| record.id == changed)
            .unwrap(),
        expected_property
    );
    let report = cadmpeg_ir::validate_neutral(&ir, Vec::new()).unwrap();
    assert!(!report
        .findings
        .iter()
        .any(|finding| finding.check == cadmpeg_ir::report::check::Check::NativeLinks));
}

#[test]
fn a_missing_gui_property_owner_is_not_replaced_by_another_provider_property() {
    let mut ir = decoded();
    let mut properties = native_properties(&ir);
    let removed = properties
        .iter()
        .find(|record| record.name == "LineColor")
        .unwrap()
        .clone();
    assert!(properties
        .iter()
        .any(|record| record.owner == removed.owner && record.id != removed.id));
    assert!(ir
        .model
        .appearances
        .iter()
        .any(|record| record.native_ref.as_deref() == Some(removed.id.as_str())));
    properties.retain(|record| record.id != removed.id);
    let ctx = cadmpeg_test_support::service_decode_context();
    ir.native
        .namespace_mut("fcstd")
        .set_arena(&ctx, "gui_properties", &properties)
        .unwrap();
    let report = cadmpeg_ir::validate_neutral(&ir, Vec::new()).unwrap();
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.check == cadmpeg_ir::report::check::Check::NativeLinks));
    assert!(ir
        .model
        .appearances
        .iter()
        .any(|record| record.native_ref.as_deref() == Some(removed.id.as_str())));
}

#[test]
fn selected_duplicate_gui_values_keep_their_actual_last_property_slot() {
    let xml = roxmltree::Document::parse(r#"<Properties><Property name="LineColor"><PropertyColor value="1"/></Property><Property name="LineColor"><PropertyColor value="2"/></Property></Properties>"#).unwrap();
    let ir = decoded();
    let properties = native_properties(&ir);
    let original = properties
        .iter()
        .find(|property| property.name == "LineColor")
        .unwrap();
    let mut first = original.clone();
    first.id = "opaque:native:GUI-property#earlier".to_owned();
    first.order = 0;
    let mut last = original.clone();
    last.id = "opaque:native:GUI-property#selected".to_owned();
    last.order = 1;
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut selected = super::super::SelectedGuiValues::default();
    for (property_order, property) in xml
        .root_element()
        .children()
        .filter(roxmltree::Node::is_element)
        .enumerate()
    {
        selected
            .insert(
                &ctx,
                property.attribute("name").unwrap(),
                super::super::SelectedGuiValue {
                    value: property
                        .children()
                        .find(roxmltree::Node::is_element)
                        .unwrap(),
                    property_order,
                },
            )
            .unwrap();
    }
    let rows = [first, last.clone()];
    let actual =
        super::super::projection_property(&rows, &last.owner, selected.get("LineColor")).unwrap();
    assert_eq!(actual.id, last.id);
    assert_eq!(
        selected.get("LineColor").unwrap().attribute("value"),
        Some("2")
    );
    assert_eq!(rows[0].id, "opaque:native:GUI-property#earlier");
}

#[test]
fn gui_native_reference_admission_charges_each_copy_and_propagates_existing_refusal() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let ir = decoded();
    let properties = native_properties(&ir);
    let property = properties
        .iter()
        .find(|record| record.name == "LineColor")
        .unwrap();
    let bytes = cadmpeg_core::decode::u64_from_index(property.id.len());
    for limit in [bytes - 1, bytes] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = super::super::projection_native_ref(
            &ctx,
            Some(super::super::GuiPropertyOrigin::Admitted(&property.id)),
        );
        if limit == bytes {
            assert_eq!(result.unwrap().as_deref(), Some(property.id.as_str()));
            assert!(matches!(
                super::super::projection_native_ref(
                    &ctx,
                    Some(super::super::GuiPropertyOrigin::Admitted(&property.id))
                ),
                Err(cadmpeg_core::CodecError::ResourceLimit(_))
            ));
        } else {
            assert!(matches!(
                result,
                Err(cadmpeg_core::CodecError::ResourceLimit(_))
            ));
        }
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let original = ctx
        .retain_bytes(1, "authored prior GUI refusal")
        .unwrap_err();
    assert_eq!(
        super::super::projection_native_ref(
            &ctx,
            Some(super::super::GuiPropertyOrigin::Admitted(&property.id))
        )
        .unwrap_err()
        .to_string(),
        original.to_string()
    );
    assert_eq!(
        super::super::projection_native_ref(&ctx, None)
            .unwrap_err()
            .to_string(),
        original.to_string()
    );
}

#[test]
fn refused_gui_native_reference_cannot_apply_a_partial_appearance_plan() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let source = decoded();
    let objects = source
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::ObjectRecord>("objects")
        .unwrap();
    let gui = br#"<Document SchemaVersion="1"><ViewProviderData Count="1"><ViewProvider name="ColoredModel"><Properties Count="1"><Property name="ShapeMaterial" type="App::PropertyMaterial"><PropertyMaterial ambientColor="1" diffuseColor="2" specularColor="3" emissiveColor="4" shininess="0.5" transparency="0.25"/></Property></Properties></ViewProvider></ViewProviderData></Document>"#;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(gui, &arena, &policy).unwrap();
    let entries = std::collections::BTreeMap::new();
    let sources = super::super::GuiSources {
        entries: &entries,
        objects: &objects,
        properties: &[],
        payloads: &[],
        topology_occurrences: &[],
        requires_alpha_conversion: false,
    };
    let mut ir = cadmpeg_ir::CadIr::empty();
    let before = ir.clone();
    let result = super::transfer(&ctx, &mut ir, gui, &sources);
    let Err(cadmpeg_core::CodecError::ResourceLimit(refusal)) = result else {
        panic!("the newly retained native reference must refuse");
    };
    assert_eq!(refusal.operation, "FreeCAD GUI native reference");
    assert_eq!(ir, before);
}
