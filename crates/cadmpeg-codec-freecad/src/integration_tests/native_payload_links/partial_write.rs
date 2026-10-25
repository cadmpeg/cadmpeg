// SPDX-License-Identifier: Apache-2.0
//! Native-only partial tables keep known rows and report changed declared counts.
use super::*;
use cadmpeg_core::decode::DecodeDepth;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder, NativeDisposition};
use cadmpeg_ir::source_fidelity::parts::PartState;

#[test]
fn native_only_partial_table_writes_its_real_prefix_and_reports_count_normalization() {
    let document = b"<Document SchemaVersion='4' FileVersion='1'><Objects Count='1'><Object type='Part::Feature' name='Shape' id='1'/></Objects><ObjectData Count='1'><Object name='Shape'><Properties Count='1'><Property name='Shape' type='Part::PropertyPartShape'><Part file='Shape.brp'/></Property></Properties></Object></ObjectData></Document>";
    let member = b"CASCADE Topology V3, (c) Open Cascade\nLocations 2\n1 1 0 0 3 0 1 0 0 0 0 1 0\n255\nCurve2ds 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 0\n*\n";
    let bytes = crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", document),
        ("Shape.brp", member),
    ]);
    let options = DecodeOptions {
        depth: DecodeDepth::Container,
        ..DecodeOptions::default()
    };
    let before = FcstdCodec
        .decode(&mut Cursor::new(bytes), &options)
        .unwrap();
    let payload =
        crate::brep::records::payloads_untracked(before.ir().native.namespace("fcstd").unwrap())
            .unwrap()
            .remove(0);
    assert_eq!(payload.declared_text_counts().unwrap()["Locations"], 2);
    assert!(!payload.has_unread_tables());
    let prefix = crate::brep::Tables::from_payload(&payload)
        .unwrap()
        .locations
        .to_vec();
    assert_eq!(prefix.len(), 1);
    assert_eq!(prefix[0].transform.rows()[0][3], 3.0);
    assert!(before
        .source_fidelity()
        .parts()
        .values()
        .any(|part| part.level().as_str() == "freecad.brep_text_location"
            && matches!(part.state(), PartState::GivenUp {})));
    let original = serde_json::to_value(&payload).unwrap();
    let plan = FcstdCodec
        .plan(
            EncodeInput::new(before.ir(), None),
            TargetRequest::Explicit("fcstd:schema-4"),
        )
        .unwrap();
    let expected = cadmpeg_ir::codec::write::ExportLossCode::SourceUndecodedOmitted.note(format!(
        "B-rep payload {} retains its readable rows; original count fields cannot be re-emitted exactly: section_counts.Locations 2 -> 1", payload.id,
    ));
    assert!(plan.report().losses.contains(&expected));
    assert_eq!(
        plan.report().native_dispositions.record_disposition(
            "fcstd",
            "shape_payloads",
            &payload.id
        ),
        NativeDisposition::Dropped
    );
    let mut output = Vec::new();
    plan.write_to(&mut output).unwrap();
    let after = FcstdCodec
        .decode(&mut Cursor::new(output), &options)
        .unwrap();
    let current =
        crate::brep::records::payloads_untracked(after.ir().native.namespace("fcstd").unwrap())
            .unwrap();
    assert_eq!(current.len(), 1);
    assert_eq!(current[0].declared_text_counts().unwrap()["Locations"], 1);
    assert_eq!(
        crate::brep::Tables::from_payload(&current[0])
            .unwrap()
            .locations,
        prefix.as_slice()
    );
    assert_eq!(serde_json::to_value(&payload).unwrap(), original);
}
