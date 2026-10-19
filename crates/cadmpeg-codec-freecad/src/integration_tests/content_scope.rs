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
        crate::native::PropertyBody::Unreadable(_)
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
