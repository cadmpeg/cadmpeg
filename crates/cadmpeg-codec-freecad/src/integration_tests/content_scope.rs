// SPDX-License-Identifier: Apache-2.0
//! Neutral projection scope and typed source-fidelity admission.

use crate::native::{ObjectRecord, PropertyRecord};
use crate::test_support::test_archive::archive;
use crate::FcstdCodec;
use cadmpeg_core::decode::DecodeDepth;
use cadmpeg_ir::document::{ArenaClass, ArenaClasses, ContentScope};
use cadmpeg_ir::report::{loss::LossTaxonomy, Severity};
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

const TYPED_EXTENSION: &str = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object name="Extension" type="PartDesign::VendorFeature"/></Objects>
<ObjectData Count="1"><Object name="Extension"><Properties Count="1"><Property name="ProxyState" type="App::PropertyString"><String value="typed vendor state"/></Property></Properties></Object></ObjectData>
</Document>"#;

fn decode(document: &str, depth: DecodeDepth) -> cadmpeg_ir::codec::DecodeResult {
    FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions {
                depth,
                ..DecodeOptions::default()
            },
        )
        .unwrap()
}

#[test]
fn geometry_projection_declares_scope_before_its_content_digest() {
    let decoded = decode(TYPED_EXTENSION, DecodeDepth::Geometry);
    assert_eq!(decoded.ir().scope, ContentScope::geometry());
    assert_eq!(decoded.report().scope(), ContentScope::geometry());
    assert!(decoded.ir().model.features.is_empty());
    let native = decoded.ir().native.namespace("fcstd").unwrap();
    assert_eq!(native.arena_as::<ObjectRecord>("objects").unwrap().len(), 1);
    assert_eq!(
        native
            .arena_as::<PropertyRecord>("properties")
            .unwrap()
            .len(),
        1
    );
    let (ir, _, fidelity) = decoded.into_parts();
    assert!(fidelity.unrepresented().is_empty());
    let ctx = cadmpeg_test_support::service_decode_context();
    let recorded = fidelity
        .metadata
        .get(cadmpeg_ir::hash::DOCUMENT_FINGERPRINT_METADATA_KEY)
        .unwrap();
    assert_eq!(
        *recorded,
        crate::writer::target::document_local_sha256(&ctx, &ir).unwrap()
    );
    let mut full_scope = ir.clone();
    full_scope.set_scope(ContentScope::full()).unwrap();
    assert_ne!(
        cadmpeg_ir::hash::document_local_sha256(
            &ctx,
            &ir,
            ir.source.as_ref(),
            "fcstd",
            crate::fidelity::SOURCE_IMAGE
        )
        .unwrap(),
        cadmpeg_ir::hash::document_local_sha256(
            &ctx,
            &full_scope,
            full_scope.source.as_ref(),
            "fcstd",
            crate::fidelity::SOURCE_IMAGE
        )
        .unwrap()
    );
}

#[test]
fn unprojected_design_objects_report_blocking_loss_but_remain_typed() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="4"><Object name="PartDesignExtension" type="PartDesign::VendorFeature"/><Object name="SketchExtension" type="Sketcher::VendorFeature"/><Object name="PartExtension" type="Part::VendorFeature"/><Object name="SheetExtension" type="Spreadsheet::VendorSheet"/></Objects>
<ObjectData Count="4"><Object name="PartDesignExtension"><Properties Count="0"/></Object><Object name="SketchExtension"><Properties Count="0"/></Object><Object name="PartExtension"><Properties Count="0"/></Object><Object name="SheetExtension"><Properties Count="0"/></Object></ObjectData></Document>"#;
    let decoded = decode(document, DecodeDepth::Full);
    let losses: Vec<_> = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| {
            loss.code.namespace() == "fcstd"
                && loss.code.local_code() == "feature.native-kind-retained"
        })
        .collect();
    assert_eq!(losses.len(), 4);
    assert!(losses.iter().all(|loss| loss.severity == Severity::Blocking
        && loss.code.taxonomy() == LossTaxonomy::FeatureHistoryRetained));
    assert_eq!(decoded.ir().model.features.len(), 4);
    for (name, expected_kind) in [
        ("PartDesignExtension", "PartDesign::VendorFeature"),
        ("SketchExtension", "Sketcher::VendorFeature"),
        ("PartExtension", "Part::VendorFeature"),
        ("SheetExtension", "Spreadsheet::VendorSheet"),
    ] {
        let native_id = crate::native::native_id("object", name);
        let feature = decoded
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.native_ref.as_deref() == Some(native_id.as_str()))
            .unwrap();
        assert_eq!(feature.name.as_deref(), Some(name));
        assert!(matches!(feature.definition().operation(),
            cadmpeg_ir::features::FeatureOperation::Native { kind, .. }
                if kind.as_str() == expected_kind));
        assert_eq!(
            losses
                .iter()
                .filter(|loss| loss
                    .provenance
                    .as_ref()
                    .and_then(|source| source.tag.as_deref())
                    == Some(native_id.as_str())
                    && loss.message.contains(expected_kind))
                .count(),
            1
        );
    }
    assert_eq!(
        decoded
            .ir()
            .native
            .namespace("fcstd")
            .unwrap()
            .arena_as::<ObjectRecord>("objects")
            .unwrap()
            .len(),
        4
    );
    assert_eq!(decoded.ir().scope, ContentScope::full());
    assert!(decoded.into_parts().2.unrepresented().is_empty());
}

#[test]
fn unprojected_workbench_objects_keep_native_history_outside_app_and_gui() {
    let workbenches = ["Mesh", "Points", "Image", "TechDraw", "Drawing", "Assembly"];
    let declarations = workbenches
        .iter()
        .chain(["App", "Gui"].iter())
        .map(|namespace| format!("<Object name='{namespace}' type='{namespace}::VendorObject'/>"))
        .collect::<String>();
    let data = workbenches
        .iter()
        .chain(["App", "Gui"].iter())
        .map(|namespace| format!("<Object name='{namespace}'><Properties Count='0'/></Object>"))
        .collect::<String>();
    let document = format!("<Document SchemaVersion='4'><Objects Count='8'>{declarations}</Objects><ObjectData Count='8'>{data}</ObjectData></Document>");
    let decoded = decode(&document, DecodeDepth::Full);
    assert_eq!(decoded.ir().model.features.len(), workbenches.len());
    for namespace in workbenches {
        let owner = crate::native::native_id("object", namespace);
        let kind = format!("{namespace}::VendorObject");
        let feature = decoded
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.native_ref.as_deref() == Some(owner.as_str()))
            .unwrap();
        assert!(matches!(feature.definition().operation(),
            cadmpeg_ir::features::FeatureOperation::Native { kind: actual, parameters }
                if actual.as_str() == kind && parameters.is_empty()));
        assert_eq!(
            decoded
                .report()
                .losses
                .iter()
                .filter(|loss| {
                    loss.code.local_code() == "feature.native-kind-retained"
                        && loss.severity == Severity::Blocking
                        && loss.message.contains(&kind)
                        && loss
                            .provenance
                            .as_ref()
                            .and_then(|source| source.tag.as_deref())
                            == Some(owner.as_str())
                })
                .count(),
            1
        );
    }
    assert!(decoded
        .ir()
        .model
        .features
        .iter()
        .all(|feature| { !matches!(feature.name.as_deref(), Some("App" | "Gui")) }));
    assert_eq!(
        decoded
            .ir()
            .native
            .namespace("fcstd")
            .unwrap()
            .arena_as::<ObjectRecord>("objects")
            .unwrap()
            .len(),
        8
    );
    assert!(crate::test_support::validate_native(decoded.ir()).is_empty());
    crate::test_support::test_archive::assert_valid_document(decoded.ir());
}

#[test]
fn unreadable_design_property_has_an_unrepresented_design_payload() {
    let document = TYPED_EXTENSION.replace("<Property name=\"ProxyState\" type=\"App::PropertyString\"><String value=\"typed vendor state\"/></Property>", "<Property name=\"Input\" type=\"App::PropertyLink\"/>");
    let decoded = decode(&document, DecodeDepth::Geometry);
    let namespace = decoded.ir().native.namespace("fcstd").unwrap();
    let properties = namespace.arena_as::<PropertyRecord>("properties").unwrap();
    // #224/#215: a missing required Link value gives up its Property;
    // there is no persisted failure-body carrier in the native property arena.
    assert!(properties.is_empty());
    let failed = decoded
        .source_fidelity()
        .parts()
        .iter()
        .filter(|(_, part)| {
            part.state() == &cadmpeg_ir::source_fidelity::parts::PartState::GivenUp {}
        })
        .collect::<Vec<_>>();
    let [(home, property)] = failed.as_slice() else {
        panic!("one actual missing-Link property home")
    };
    let raw = "<Property name=\"Input\" type=\"App::PropertyLink\"/>";
    let start = u64::try_from(document.find(raw).unwrap()).unwrap();
    assert_eq!(property.level().as_str(), "freecad.property");
    assert_eq!(property.offset(), start);
    assert_eq!(property.byte_len(), u64::try_from(raw.len()).unwrap());
    assert_eq!(
        decoded
            .source_fidelity()
            .retained_record(home.as_str())
            .unwrap()
            .data(),
        Some(raw.as_bytes())
    );
    assert_eq!(
        decoded
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code.local_code() == "record.unreadable")
            .count(),
        1
    );

    // #214 §3.1 classifies bytes from actual neutral owners, not the runtime
    // namespace of an unprojected native object. This Geometry decode has no
    // neutral record; the unheld SchemaVersion key independently proves the
    // conservative stream default, even after stored XML framing is projected.
    assert_eq!(decoded.ir().model.entity_count(), 0);
    let fidelity = decoded.source_fidelity();
    let layout = &fidelity.record_layout()[&crate::native::native_id("document", "0")];
    let key = u64::try_from(document.find("SchemaVersion").unwrap()).unwrap();
    let key_end = key + u64::try_from("SchemaVersion".len()).unwrap();
    assert!(fidelity.record_layout().values().all(|record| record
        .reads()
        .get(layout.stream())
        .is_none_or(|reads| reads
            .iter()
            .all(|read| read.end <= key || key_end <= read.start))));
    assert!(fidelity
        .parts()
        .values()
        .filter(|part| part.stream() == layout.stream())
        .flat_map(|part| part.framing_bytes())
        .all(|frame| frame.end <= key || key_end <= frame.start));
    let span = fidelity.b1_spans()[layout.stream()]
        .spans()
        .iter()
        .find(|span| {
            let range = span.range();
            range.start <= key && key_end <= range.end
        })
        .expect("unheld nonframing schema metadata remains one genuine B1 span");
    let default = ArenaClasses::from([ArenaClass::Model, ArenaClass::Design]);
    assert_eq!(span.classes(), default);
    assert_eq!(fidelity.unrepresented(), default);
}

#[test]
fn container_projection_does_not_treat_source_bytes_as_typed_records() {
    let decoded = decode(TYPED_EXTENSION, DecodeDepth::Container);
    assert_eq!(decoded.ir().scope, ContentScope::container());
    assert!(decoded.report().container_only());
    assert!(decoded.ir().model.features.is_empty());
    assert_eq!(
        decoded.into_parts().2.unrepresented(),
        ArenaClasses::from([ArenaClass::Model, ArenaClass::Design])
    );
}

#[test]
fn design_operand_and_sheet_ownership_is_independent_of_delivery_depth() {
    for cells in [
        r#"<Cells Count="0"/>"#,
        r#"<Cells Count="1"><Cell address="A1" content="12"/></Cells>"#,
    ] {
        let document = format!(
            r#"<Document SchemaVersion="4"><Objects Count="3"><Object name="Body" type="PartDesign::Body"/><Object name="Feature" type="PartDesign::Feature"/><Object name="Sheet" type="Spreadsheet::Sheet"/></Objects><ObjectData Count="3">
<Object name="Body"><Properties Count="1"><Property name="Group" type="App::PropertyLinkList"><LinkList count="1"><Link value="Feature"/></LinkList></Property></Properties></Object>
<Object name="Feature"><Properties Count="4">
<Property name="BaseFeature" type="App::PropertyLink"><Link value=""/></Property>
<Property name="Depth" type="App::PropertyLength"><Float value="6"/></Property>
<Property name="Length2" type="App::PropertyLength"><Float value="5"/></Property>
<Property name="UserNote" type="App::PropertyString"><String value="keep this extra"/></Property>
</Properties></Object>
<Object name="Sheet"><Properties Count="1"><Property name="cells" type="Spreadsheet::PropertySheet">{cells}</Property></Properties></Object>
</ObjectData></Document>"#
        );
        let full = decode(&document, DecodeDepth::Full);
        let geometry = decode(&document, DecodeDepth::Geometry);
        assert_eq!(full.ir().model.spreadsheets.len(), 1);
        for (name, expected) in [("Depth", 6.0), ("Length2", 5.0)] {
            let parameter = full
                .ir()
                .model
                .parameters
                .iter()
                .find(|parameter| parameter.name == name)
                .unwrap();
            assert!(
                matches!(&parameter.value, Some(cadmpeg_ir::features::ParameterValue::Length(length)) if length.get() == expected)
            );
        }
        assert!(geometry.ir().model.parameters.is_empty());
        assert!(geometry.ir().model.spreadsheets.is_empty());
        assert_eq!(geometry.ir().model.attributes, full.ir().model.attributes);
        assert_eq!(geometry.report().losses, full.report().losses);
        assert!(full
            .ir()
            .model
            .attributes
            .iter()
            .any(|attribute| attribute.name == "UserNote"
                && attribute.values
                    == vec![cadmpeg_ir::attributes::AttributeValue::String(
                        "keep this extra".into()
                    )]));
        for decoded in [&full, &geometry] {
            let properties = decoded
                .ir()
                .native
                .namespace("fcstd")
                .unwrap()
                .arena_as::<PropertyRecord>("properties")
                .unwrap();
            let sheet = properties
                .iter()
                .find(|property| property.name == "cells")
                .unwrap();
            let exact_xml =
                crate::test_support::source_fidelity::bytes(decoded.source_fidelity(), &sheet.id);
            assert_eq!(
                std::str::from_utf8(exact_xml).unwrap(),
                format!(
                    r#"<Property name="cells" type="Spreadsheet::PropertySheet">{cells}</Property>"#
                )
            );
        }
    }
}

#[test]
fn dynamic_numeric_application_extras_keep_model_ownership_at_geometry_depth() {
    let document = r#"<Document SchemaVersion="4"><Objects Count="1"><Object name="Application" type="App::FeaturePython"/></Objects><ObjectData Count="1"><Object name="Application"><Properties Count="2">
<Property name="AngleMax" type="App::PropertyAngle" group="Assembly"><Float value="90"/></Property>
<Property name="AngleMin" type="App::PropertyAngle" group="Assembly"><Float value="-90"/></Property>
</Properties></Object></ObjectData></Document>"#;
    let full = decode(document, DecodeDepth::Full);
    let geometry = decode(document, DecodeDepth::Geometry);
    assert!(full.ir().model.parameters.is_empty());
    assert!(geometry.ir().model.parameters.is_empty());
    assert_eq!(geometry.ir().model.attributes, full.ir().model.attributes);
    assert_eq!(geometry.report().losses, full.report().losses);
    for decoded in [&full, &geometry] {
        assert!(decoded.ir().model.attributes.is_empty());
        let assert_properties = |ir: &cadmpeg_ir::CadIr| {
            let properties = ir
                .native
                .namespace("fcstd")
                .unwrap()
                .arena_as::<PropertyRecord>("properties")
                .unwrap();
            let owner = crate::native::native_id("object", "Application");
            let properties = properties
                .iter()
                .filter(|property| property.owner == owner)
                .collect::<Vec<_>>();
            assert_eq!(properties.len(), 2);
            for (name, expected) in [("AngleMax", "90"), ("AngleMin", "-90")] {
                let property = properties
                    .iter()
                    .find(|property| property.name == name)
                    .unwrap();
                assert_eq!(property.owner, owner);
                assert_eq!(property.type_name, "App::PropertyAngle");
                assert_eq!(property.dynamic.as_ref().unwrap().group, "Assembly");
                assert_eq!(property.values().len(), 1);
                assert_eq!(property.values()[0].tag, "Float");
                assert_eq!(
                    property.values()[0]
                        .attributes
                        .get("value")
                        .map(String::as_str),
                    Some(expected)
                );
            }
        };
        assert_properties(decoded.ir());
        use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
        let plan = FcstdCodec
            .plan(
                EncodeInput::new(decoded.ir(), None),
                TargetRequest::Explicit("fcstd:schema-4"),
            )
            .unwrap();
        let mut bytes = Vec::new();
        plan.write_to(&mut bytes).unwrap();
        let reread = FcstdCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        assert_properties(reread.ir());
    }
}

#[test]
fn dynamic_numeric_extras_link_the_real_model_owner_at_both_depths() {
    let document = r#"<Document SchemaVersion="4"><Objects Count="1"><Object name="Application" type="App::Part"/></Objects><ObjectData Count="1"><Object name="Application"><Properties Count="2">
<Property name="AngleMax" type="App::PropertyAngle" group="Assembly"><Float value="90"/></Property>
<Property name="AngleMin" type="App::PropertyAngle" group="Assembly"><Float value="-90"/></Property>
</Properties></Object></ObjectData></Document>"#;
    let full = decode(document, DecodeDepth::Full);
    let geometry = decode(document, DecodeDepth::Geometry);
    assert!(full.ir().model.parameters.is_empty());
    assert!(geometry.ir().model.parameters.is_empty());
    assert_eq!(geometry.ir().model.attributes, full.ir().model.attributes);
    assert_eq!(geometry.report().losses, full.report().losses);
    for (name, expected) in [("AngleMax", 90.0), ("AngleMin", -90.0)] {
        let attribute = full
            .ir()
            .model
            .attributes
            .iter()
            .find(|attribute| attribute.name == name)
            .unwrap();
        assert_eq!(
            attribute.values,
            vec![cadmpeg_ir::attributes::AttributeValue::float(expected).unwrap()]
        );
        assert!(
            matches!(&attribute.target, cadmpeg_ir::attributes::AttributeTarget::Record { arena, id } if arena.as_str() == "product_definitions" && full.ir().model.contains_record(*arena, id))
        );
    }
}

#[test]
fn attributes_keep_model_record_owners_and_follow_design_owner_scope() {
    let xml = "<Document SchemaVersion='4' FileVersion='1'><Properties Count='1'><Property name='DocumentExtra' type='App::PropertyString'><String value='document'/></Property></Properties><Objects Count='2'><Object name='Part' type='App::Part'/><Object name='Feature' type='Part::Feature'/></Objects><ObjectData Count='2'><Object name='Part'><Properties Count='1'><Property name='ModelExtra' type='App::PropertyString'><String value='model'/></Property></Properties></Object><Object name='Feature'><Properties Count='1'><Property name='DesignExtra' type='App::PropertyString'><String value='design'/></Property></Properties></Object></ObjectData></Document>";
    let full = decode(xml, DecodeDepth::Full);
    let geometry = decode(xml, DecodeDepth::Geometry);
    let full_attribute = |name: &str| {
        full.ir()
            .model
            .attributes
            .iter()
            .find(|record| record.name == name)
            .unwrap()
    };
    let model = full_attribute("ModelExtra");
    assert!(
        matches!(&model.target, cadmpeg_ir::attributes::AttributeTarget::Record { arena, id } if arena.as_str() == "product_definitions" && full.ir().model.contains_record(*arena, id))
    );
    assert_eq!(
        geometry
            .ir()
            .model
            .attributes
            .iter()
            .find(|record| record.name == "ModelExtra")
            .unwrap(),
        model
    );
    assert!(
        matches!(&full_attribute("DesignExtra").target, cadmpeg_ir::attributes::AttributeTarget::Record { arena, id } if arena.as_str() == "features" && full.ir().model.contains_record(*arena, id))
    );
    assert!(!geometry
        .ir()
        .model
        .attributes
        .iter()
        .any(|record| record.name == "DesignExtra"));
    assert!(matches!(
        full_attribute("DocumentExtra").target,
        cadmpeg_ir::attributes::AttributeTarget::Document
    ));
    assert_eq!(
        cadmpeg_test_support::contract::neutral_view(full.ir(), &[ArenaClass::Design]),
        cadmpeg_test_support::contract::neutral_view(geometry.ir(), &[ArenaClass::Design])
    );
}

mod attribute_expressions {
    use super::*;

    #[test]
    fn document_owned_expression_agrees_across_full_and_geometry_depths() {
        let xml = "<Document SchemaVersion='4' FileVersion='1'><Properties Count='1'>\
            <Property name='ExpressionEngine' type='App::PropertyExpressionEngine'><ExpressionEngine count='1'>\
            <Expression path='Extra' expression='Sheet.width + 1' comment='held'/></ExpressionEngine></Property>\
            </Properties><Objects Count='1'><Object name='Sheet' type='Spreadsheet::Sheet'/></Objects>\
            <ObjectData Count='1'><Object name='Sheet'><Properties Count='1'>\
            <Property name='cells' type='Spreadsheet::PropertySheet'><Cells Count='1' xlink='1'>\
            <Cell address='A1' alias='width' content='5'/></Cells></Property>\
            </Properties></Object></ObjectData></Document>";
        let full = decode(xml, DecodeDepth::Full);
        let geometry = decode(xml, DecodeDepth::Geometry);
        let full_attr = full
            .ir()
            .model
            .attributes
            .iter()
            .find(|a| a.name == "ExpressionEngine")
            .unwrap();
        let geometry_attr = geometry
            .ir()
            .model
            .attributes
            .iter()
            .find(|a| a.name == "ExpressionEngine")
            .unwrap();
        assert_eq!(
            full_attr.target,
            cadmpeg_ir::attributes::AttributeTarget::Document
        );
        assert_eq!(full_attr.native_ref, geometry_attr.native_ref);
        let [cadmpeg_ir::attributes::AttributeValue::Expression(value)] =
            full_attr.values.as_slice()
        else {
            panic!("held expression")
        };
        assert_eq!(value.expression.dependencies().count(), 1);
        let [cadmpeg_ir::attributes::AttributeValue::Expression(value)] =
            geometry_attr.values.as_slice()
        else {
            panic!("held expression")
        };
        assert!(value.expression.dependencies().next().is_none());
        let resolver = crate::test_support::expression_names::resolver(full.ir())
            .unwrap()
            .unwrap();
        let (left, right) = cadmpeg_test_support::contract::expressions::neutral_views(
            full.ir(),
            geometry.ir(),
            &[ArenaClass::Design],
            resolver.as_ref(),
        )
        .unwrap();
        assert_eq!(left, right);
    }
}
