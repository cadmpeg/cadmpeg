// SPDX-License-Identifier: Apache-2.0
//! Dimensions of a feature-tree node that is no timeline member.
#![allow(clippy::unwrap_used)]

use super::*;

/// A reference dimension of the annotations node states no design parameter,
/// and the note names its decoder's parameter.
#[test]
fn a_dimension_of_the_annotations_node_is_named_in_a_note() {
    let result = SldprtCodec
        .decode(
            &mut Cursor::new(native_envelope_fixture(&with_history(
                sldprt_with_body(&triangle_body()),
                br#"<Keywords Name="Bracket">
                    <Feature Name="Annotations" Type="Annotations" id="101">
                        <Dimension Name="RD1">7.5mm</Dimension>
                    </Feature>
                    <Extrusion Name="Boss" Type="BossExtrude" id="7">
                        <Dimension Name="D1">12.5mm</Dimension>
                    </Extrusion>
                </Keywords>"#,
                &[("moDetailCabinet_c", "Annotations", 101)],
            ))),
            &DecodeOptions::default(),
        )
        .unwrap();
    let ir = result.ir();
    let legacy = ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "RD1")
        .unwrap();
    assert!(ir
        .design
        .parameters
        .iter()
        .all(|parameter| parameter.id != legacy.id));
    let note = note_of(
        &result.report().losses,
        SldprtLossCode::DesignParameterOwnerOutsideTimeline,
    )
    .expect("the note names the dimension");
    assert!(
        note.message.ends_with(legacy.id.as_str()),
        "{}",
        note.message
    );
    assert_eq!(
        parameter_named(ir, "D1").name.as_ref().unwrap().as_str(),
        "D1"
    );
}
