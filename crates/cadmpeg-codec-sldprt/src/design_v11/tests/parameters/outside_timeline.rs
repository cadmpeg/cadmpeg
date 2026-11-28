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

/// A feature node under the annotations node states no design feature, a
/// note names it, and the note on its dimension names its owner as a feature
/// under that node.
#[test]
fn a_feature_under_the_annotations_node_is_named_with_its_dimension() {
    let result = SldprtCodec
        .decode(
            &mut Cursor::new(native_envelope_fixture(&with_history(
                sldprt_with_body(&triangle_body()),
                br#"<Keywords Name="Bracket">
                    <Feature Name="Annotations" Type="Annotations" id="101">
                        <Extrusion Name="Hidden" Type="BossExtrude" id="12">
                            <Dimension Name="D3">4mm</Dimension>
                        </Extrusion>
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
    let losses = &result.report().losses;
    let hidden = ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Hidden"))
        .unwrap();
    assert!(ir
        .design
        .features
        .iter()
        .all(|feature| feature.id != hidden.id));
    let note = note_of(losses, SldprtLossCode::DesignFeatureOutsideTimeline)
        .expect("the note names the feature");
    assert!(
        note.message.starts_with("1 feature nodes") && note.message.ends_with(hidden.id.as_str()),
        "{}",
        note.message
    );
    let dimension = ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "D3")
        .unwrap();
    let [owner] = losses
        .iter()
        .filter(|loss| loss.code == SldprtLossCode::DesignParameterOwnerOutsideTimeline.kind())
        .collect::<Vec<_>>()[..]
    else {
        panic!("one note: {losses:#?}");
    };
    assert!(
        owner
            .message
            .contains("owner is a feature under a feature-tree node")
            && owner.message.ends_with(dimension.id.as_str()),
        "{}",
        owner.message
    );
}
