// SPDX-License-Identifier: Apache-2.0
//! Partial attachments write known fields without claiming unknown defaults.

use super::*;
use cadmpeg_ir::codec::write::{NativeDisposition, NativeDispositions};

#[test]
fn unknown_attachment_supports_and_frame_are_named_without_inventing_defaults() {
    let offset = crate::native::frame::FiniteFrame::try_from([
        [1.0, 0.0, 0.0, 2.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ])
    .unwrap();
    for partial in [false, true] {
        let record = if partial {
            AttachmentRecord::try_new_partial(
                "custom:native:attachment#current".into(),
                "custom:native:object#current".into(),
                None,
                Some(crate::attachment::MapModeIndex::try_from("5").unwrap()),
                None,
                Some(offset),
                None,
            )
        } else {
            AttachmentRecord::try_new(
                "custom:native:attachment#current".into(),
                "custom:native:object#current".into(),
                Vec::new(),
                None,
                None,
                Some(offset),
            )
        }
        .unwrap();
        let ctx = cadmpeg_test_support::service_decode_context();
        let mut ir = CadIr::empty();
        ir.native
            .namespace_mut("fcstd")
            .set_arena(&ctx, "attachments", &[record])
            .unwrap();
        let mut objects = vec![Object::new(
            "Current".into(),
            "App::FeaturePython",
            "Current",
        )];
        let mut losses = Vec::new();
        let mut outcomes =
            NativeDispositions::for_namespace("fcstd", ["attachments"], NativeDisposition::Dropped);
        write(
            &ir,
            &mut objects,
            &HashMap::from([("custom:native:object#current".into(), "Current".into())]),
            &HashMap::new(),
            &mut losses,
            &mut outcomes,
        )
        .unwrap();
        let properties = &objects[0].properties;
        assert!(!properties.contains_key("Support"));
        assert!(!properties.contains_key("Placement"));
        let offset_xml = &properties["AttachmentOffset"];
        let xml = roxmltree::Document::parse(offset_xml).unwrap();
        assert_eq!(
            xml.root_element()
                .first_element_child()
                .unwrap()
                .attribute("Px"),
            Some("2")
        );
        if partial {
            let mode = roxmltree::Document::parse(&properties["MapMode"]).unwrap();
            assert_eq!(
                mode.root_element()
                    .first_element_child()
                    .unwrap()
                    .attribute("value"),
                Some("5")
            );
            assert_eq!(losses.len(), 1);
            assert_eq!(losses[0].code.local_code(), "source.undecoded-omitted");
            assert_eq!(losses[0].severity, cadmpeg_ir::report::Severity::Warning);
            assert!(losses[0]
                .message
                .contains("custom:native:attachment#current"));
            assert!(losses[0].message.contains("supports, effective_frame"));
            assert_eq!(
                outcomes.record_disposition(
                    "fcstd",
                    "attachments",
                    "custom:native:attachment#current"
                ),
                NativeDisposition::Dropped
            );
        } else {
            assert!(!properties.contains_key("MapMode"));
            assert!(losses.is_empty(), "ordinary optional absence stays quiet");
            assert_eq!(
                outcomes.record_disposition(
                    "fcstd",
                    "attachments",
                    "custom:native:attachment#current"
                ),
                NativeDisposition::Written
            );
        }
    }
}
