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
                && loss.code.local_code() == "feature.object-unprojected"
        })
        .collect();
    assert_eq!(losses.len(), 4);
    assert!(losses.iter().all(|loss| loss.severity == Severity::Blocking
        && loss.code.taxonomy() == LossTaxonomy::FeatureHistoryRetained));
    assert!(decoded.ir().model.features.is_empty());
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
fn unreadable_design_property_has_an_unrepresented_design_payload() {
    let document = TYPED_EXTENSION.replace("<Property name=\"ProxyState\" type=\"App::PropertyString\"><String value=\"typed vendor state\"/></Property>", "<Property name=\"Input\" type=\"App::PropertyLink\"/>");
    let decoded = decode(&document, DecodeDepth::Geometry);
    let namespace = decoded.ir().native.namespace("fcstd").unwrap();
    let properties = namespace.arena_as::<PropertyRecord>("properties").unwrap();
    assert!(matches!(
        properties[0].body,
        Some(crate::native::PropertyBody::Unreadable(_))
    ));
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "metadata.persistence-payload-unresolved"));
    assert_eq!(
        decoded.into_parts().2.unrepresented(),
        ArenaClasses::from([ArenaClass::Design])
    );
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
                    == vec![Some(cadmpeg_ir::attributes::AttributeValue::String(
                        "keep this extra".into()
                    ))]));
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
            vec![Some(
                cadmpeg_ir::attributes::AttributeValue::float(expected).unwrap()
            )]
        );
    }
}
