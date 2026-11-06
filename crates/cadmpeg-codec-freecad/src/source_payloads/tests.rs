// SPDX-License-Identifier: Apache-2.0
//! A filename and retained bytes cannot satisfy patch admission.

use std::io::Cursor;

use cadmpeg_core::decode::DecodeDepth;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder, SynthesisCause};
use cadmpeg_ir::report::export::WritePath;
use cadmpeg_ir::{ArenaClass, ArenaClasses, Codec, DecodeOptions};

use crate::FcstdCodec;

const DOCUMENT: &str = r#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object name="Box" type="Part::Box"/></Objects><ObjectData Count="1"><Object name="Box"><Properties Count="4"><Property name="Length" type="App::PropertyLength"><Float value="1"/></Property><Property name="Width" type="App::PropertyLength"><Float value="2"/></Property><Property name="Height" type="App::PropertyLength"><Float value="3"/></Property><Property name="Proxy" type="App::PropertyPythonObject"><Python file="Proxy.bin"/></Property></Properties></Object></ObjectData></Document>"#;

#[test]
fn typed_filename_never_admits_an_opaque_design_payload_to_patch() {
    let source = crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", DOCUMENT.as_bytes()),
        ("Proxy.bin", b"opaque source-only state"),
    ]);
    for depth in [DecodeDepth::Geometry, DecodeDepth::Full] {
        let decoded = FcstdCodec
            .decode(
                &mut Cursor::new(&source),
                &DecodeOptions {
                    depth,
                    ..DecodeOptions::default()
                },
            )
            .unwrap();
        assert_eq!(
            decoded.source_fidelity().unrepresented(),
            ArenaClasses::from([ArenaClass::Design])
        );
        assert!(decoded
            .report()
            .losses
            .iter()
            .any(|loss| loss.code.local_code() == "source.design-payload-unrepresented"));
        let plan = FcstdCodec
            .plan(
                EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
                TargetRequest::Inherit,
            )
            .unwrap();
        assert!(matches!(
            plan.report().write_path(),
            WritePath::Synthesized { .. }
        ));
        assert!(matches!(
            plan.report().synthesis_cause(),
            Some(SynthesisCause::SourceUnrepresented { classes })
                if *classes == ArenaClasses::from([ArenaClass::Design])
        ));
        let mut bytes = Vec::new();
        plan.write_to(&mut bytes).unwrap();
        let output = FcstdCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        let native = output.ir().native.namespace("fcstd").unwrap();
        assert!(native
            .arena_as::<crate::native::EntryRecord>("entries")
            .unwrap()
            .iter()
            .all(|entry| entry.name() != "Proxy.bin"));
        assert!(native
            .arena_as::<crate::native::PropertyRecord>("properties")
            .unwrap()
            .iter()
            .all(|property| property.name != "Proxy"));
        assert!(output.source_fidelity().unrepresented().is_empty());
    }
}

#[test]
fn unowned_archive_payload_is_reported_conservatively() {
    let document =
        r#"<Document SchemaVersion="4"><Objects Count="0"/><ObjectData Count="0"/></Document>"#;
    let source = crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", document.as_bytes()),
        ("Unknown.bin", b"unknown source-only state"),
    ]);
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        decoded.source_fidelity().unrepresented(),
        ArenaClasses::from([ArenaClass::Model, ArenaClass::Design])
    );
    for code in [
        "source.model-payload-unrepresented",
        "source.design-payload-unrepresented",
    ] {
        assert!(decoded
            .report()
            .losses
            .iter()
            .any(|loss| loss.code.local_code() == code));
    }
}

#[test]
fn inline_python_bytes_do_not_become_typed_content_by_xml_framing() {
    let document = DOCUMENT.replace(
        "<Python file=\"Proxy.bin\"/>",
        "<Python value=\"AAAB-opaque-pickle\"/>",
    );
    let source = crate::test_support::test_archive::archive(&document);
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        decoded.source_fidelity().unrepresented(),
        ArenaClasses::from([ArenaClass::Design])
    );
    let plan = FcstdCodec
        .plan(
            EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
            TargetRequest::Inherit,
        )
        .unwrap();
    assert!(matches!(
        plan.report().write_path(),
        WritePath::Synthesized { .. }
    ));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(output
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap()
        .iter()
        .all(|property| property.name != "Proxy"));
}

#[test]
fn json_python_values_are_typed_editable_data_not_encoded_content_sources() {
    let source = crate::test_support::test_archive::CORE_DESIGN_PRODUCT;
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(
        decoded.source_fidelity().unrepresented().is_empty(),
        "{:?}",
        decoded.report().losses
    );
    let mut ir = decoded.ir().clone();
    let mut properties = ir
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    let property = properties
        .iter_mut()
        .find(|property| property.name == "InertValue")
        .unwrap();
    let Some(crate::native::PropertyBody::Persisted { values, .. }) = &mut property.body else {
        panic!("typed JSON property")
    };
    assert_eq!(
        values[0].decoded_json,
        Some(serde_json::json!({"safe":true,"execute":false}).into())
    );
    values[0].decoded_json = Some(serde_json::json!({"safe":false,"edited":[1,2,3]}).into());
    // Leave the encoded attribute and diagnostic raw XML deliberately stale.
    crate::test_support::with_service_context(&[], |ctx| {
        ir.native
            .namespace_mut("fcstd")
            .set_arena(ctx, "properties", &properties)
            .unwrap();
    });
    let plan = FcstdCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit("fcstd:schema-4"),
        )
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let properties = output
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    let property = properties
        .iter()
        .find(|property| property.name == "InertValue")
        .unwrap();
    assert_eq!(
        property.values()[0].decoded_json,
        Some(serde_json::json!({"safe":false,"edited":[1,2,3]}).into())
    );
    assert!(output.source_fidelity().unrepresented().is_empty());
}
