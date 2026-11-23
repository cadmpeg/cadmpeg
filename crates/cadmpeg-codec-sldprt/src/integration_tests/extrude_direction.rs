// SPDX-License-Identifier: Apache-2.0

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::features::extents::ExtrudeDirection;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
use cadmpeg_ir::math::Vector3;

use crate::loss::SldprtLossCode;
use crate::test_support::container::{
    add_solidworks_version, make_block, native_envelope_fixture, sldprt_with_body,
};
use crate::test_support::history::resolved_feature_classes_with_ids;
use crate::test_support::parasolid::owned_triangle;
use crate::SldprtCodec;

fn extrusion(native: bool, direction: Option<&str>) -> Vec<u8> {
    let direction = direction
        .map(|value| format!(" Direction=\"{value}\""))
        .unwrap_or_default();
    let keywords = format!(
        "<Keywords><Extrusion Name=\"Extrude\" Type=\"BossExtrude\" id=\"31\" EndCondition=\"Blind\"{direction}><Dimension Name=\"Depth\">10mm</Dimension></Extrusion></Keywords>"
    );
    let mut source = sldprt_with_body(&owned_triangle(0, 700, 0.0));
    source.extend(make_block(0x42, "Contents/Keywords", keywords.as_bytes()));
    add_solidworks_version(&mut source, 18000);
    if native {
        source.extend(make_block(
            0x43,
            "Contents/Config-0-ResolvedFeatures",
            &resolved_feature_classes_with_ids(&[("moExtrusion_c", "Extrude", 31)]),
        ));
    }
    native_envelope_fixture(&source)
}

#[test]
fn uncontradicted_travel_preserves_native_and_mirrored_controls() {
    for (native, vector) in [
        (false, None),
        (true, None),
        (true, Some(("0,0,1", 1.0))),
        (true, Some(("0,0,-1", -1.0))),
    ] {
        let decoded = SldprtCodec
            .decode(
                &mut Cursor::new(extrusion(native, vector.map(|(literal, _)| literal))),
                &DecodeOptions::default(),
            )
            .expect("synthetic extrusion decodes");
        let direction = decoded
            .ir()
            .model
            .features
            .iter()
            .find_map(|feature| match feature.evaluation.definition() {
                FeatureDefinition::Operation(FeatureOperation::Extrude { direction, .. }) => {
                    Some(direction)
                }
                _ => None,
            })
            .expect("extrusion definition");
        match vector {
            Some((_, z)) => assert!(matches!(direction,
                ExtrudeDirection::Explicit { vector, .. }
                    if vector.get() == Vector3::new(0.0, 0.0, z))),
            None => assert!(matches!(direction, ExtrudeDirection::ProfileNormal {})),
        }
        assert!(!decoded
            .report()
            .losses
            .iter()
            .any(|loss| { loss.code == SldprtLossCode::FeatureExtrudeDirectionUnresolved.kind() }));
    }
}

/// The decoded direction, whether the decoder left the reversal flag
/// unread, and the loss codes and messages of an extrusion of `operation`
/// and `attributes` over a start cap at `z = 0` facing `-z` and an end cap
/// at `z = side` facing `side`.
fn cap_pair(
    side: f64,
    operation: &str,
    attributes: &str,
) -> (
    ExtrudeDirection,
    bool,
    Vec<(cadmpeg_ir::report::loss::LossKind, String)>,
) {
    use crate::test_support::history::sldprt_with_nested_sketch_profile;
    let mut body = cap(0, 700, 0.0, 1.0);
    body.extend(cap(200, 701, side, side));
    let mut source = sldprt_with_nested_sketch_profile(&body);
    let keywords = format!(
        r#"<Keywords><Sketch Name="Sketch1" Type="Sketch" id="1"/><Extrusion Name="Extrude" Type="BossExtrude" id="31" Profile="1" Operation="{operation}" EndCondition="Blind"{attributes}><Dimension Name="Depth">1m</Dimension></Extrusion></Keywords>"#
    );
    source.extend(make_block(0x42, "Contents/Keywords", keywords.as_bytes()));
    add_solidworks_version(&mut source, 18000);
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(native_envelope_fixture(&source)),
            &DecodeOptions::default(),
        )
        .expect("synthetic cap pair");
    let (direction, unread) = decoded
        .ir()
        .model
        .features
        .iter()
        .find_map(|f| match f.evaluation.definition().operation() {
            FeatureOperation::Extrude { direction, .. } => Some((
                direction.clone(),
                crate::history::extrude_direction::reversal_unread(&f.source_properties),
            )),
            _ => None,
        })
        .expect("extrusion");
    let losses = decoded
        .report()
        .losses
        .iter()
        .map(|loss| (loss.code.clone(), loss.message.clone()))
        .collect();
    (direction, unread, losses)
}

fn has(losses: &[(cadmpeg_ir::report::loss::LossKind, String)], code: SldprtLossCode) -> bool {
    losses.iter().any(|(kind, _)| *kind == code.kind())
}

#[test]
fn decoded_opposite_cap_retains_travel_with_a_mirrored_cap_control() {
    for side in [-1.0, 1.0] {
        let (direction, _, losses) = cap_pair(side, "Join", "");
        assert_eq!(
            matches!(direction, ExtrudeDirection::Unresolved {}),
            side < 0.0,
            "side={side}",
        );
        assert_eq!(
            has(&losses, SldprtLossCode::FeatureExtrudeDirectionUnresolved),
            side < 0.0
        );
    }
}

/// A reversal whose caps run along the profile normal travels along it and
/// charges `feature.extrude-reversal-contradicted`, which names the
/// extrusion; caps against the normal keep the reversal.
#[test]
fn caps_along_the_normal_override_a_decoded_reversal() {
    for side in [-1.0, 1.0] {
        let (direction, unread, losses) = cap_pair(side, "Join", r#" Reverse="true""#);
        let expected = if side > 0.0 {
            ExtrudeDirection::ProfileNormal {}
        } else {
            ExtrudeDirection::ReversedProfileNormal {}
        };
        assert_eq!(direction, expected, "side={side}");
        assert_eq!(unread, side > 0.0);
        let contradicted = losses
            .iter()
            .filter(|(kind, _)| *kind == SldprtLossCode::FeatureExtrudeReversalContradicted.kind())
            .map(|(_, message)| message.as_str())
            .collect::<Vec<_>>();
        assert_eq!(contradicted.len(), usize::from(side > 0.0), "{losses:?}");
        assert!(
            contradicted
                .iter()
                .all(|message| message.contains("Extrusion \"Extrude\" (")),
            "{contradicted:?}"
        );
        assert!(!has(
            &losses,
            SldprtLossCode::FeatureExtrudeDirectionUnresolved
        ));
    }
}

/// A cut leaves its reversal flag unread: it travels along the profile
/// normal on either side, as the base reading did, with no loss.
#[test]
fn a_cut_keeps_the_normal_under_the_reversal_flag() {
    for side in [-1.0, 1.0] {
        let (direction, unread, losses) = cap_pair(side, "Cut", r#" Reverse="true""#);
        assert_eq!(direction, ExtrudeDirection::ProfileNormal {}, "side={side}");
        assert!(unread);
        assert!(!has(
            &losses,
            SldprtLossCode::FeatureExtrudeReversalContradicted
        ));
    }
}

fn cap(base: u16, owner: u16, z: f64, normal: f64) -> Vec<u8> {
    use crate::test_support::parasolid::{
        bridge_owned, coedge, edge_use_with_canonical, line_carrier, loop_head, plane_carrier,
        typed_single_face_ownership, vertex_use, world_point,
    };
    let points = [[0.0, 0.0, z], [1.0, 0.0, z], [0.0, 1.0, z]];
    let mut body = plane_carrier(
        base + 100,
        [0.0, 0.0, z],
        [0.0, 0.0, normal],
        [1.0, 0.0, 0.0],
    );
    body.extend(bridge_owned(base + 10, base + 20, base + 100, owner));
    body.extend(loop_head(base + 20, base + 30, base + 10));
    for index in 0_u16..3 {
        let next = (index + 1) % 3;
        let start = points[usize::from(index)];
        let end = points[usize::from(next)];
        let direction = Vector3::new(end[0] - start[0], end[1] - start[1], end[2] - start[2])
            .unit()
            .expect("unit cap edge");
        body.extend(line_carrier(
            base + 90 + index,
            start,
            [direction.x, direction.y, direction.z],
        ));
        body.extend(coedge(
            base + 30 + index,
            base + 20,
            base + 30 + next,
            base + 50 + index,
            0,
            base + 40 + index,
            false,
        ));
        body.extend(edge_use_with_canonical(
            base + 40 + index,
            0,
            base + 90 + index,
            owner,
        ));
        body.extend(vertex_use(base + 50 + index, base + 60 + index));
        body.extend(world_point(base + 60 + index, start));
    }
    typed_single_face_ownership(
        &mut body,
        owner,
        base + 70,
        base + 80,
        base + 10,
        base + 100,
        1,
    );
    body
}

/// A part whose join `Extrude` (id 31) states no reversal in Keywords and
/// whose feature-input lane holds its one-direction blind end spec with
/// reversal flag `1`, and the offset of that end spec's anchor in the lane.
/// With `caps`, the body holds a start cap at `z = 0` and an end cap at
/// `z = caps`, and the extrusion's profile is a placed sketch; without, the
/// solid can neither hold nor contradict the travel.
fn lane_flagged(caps: Option<f64>) -> (Vec<u8>, usize) {
    use crate::test_support::history::resolved_features_payload;
    use crate::test_support::parasolid::{parasolid_with_body, triangle_body};
    let (mut source, mut payload, keywords) = match caps {
        Some(side) => {
            let mut body = cap(0, 700, 0.0, 1.0);
            body.extend(cap(200, 701, side, side));
            let mut payload = resolved_features_payload(&[0, 1, 1, 1]);
            payload.extend(parasolid_with_body(
                "feature input sketch",
                "SCH_SW_33103_11000",
                &triangle_body(),
            ));
            (
                sldprt_with_body(&body),
                payload,
                r#"<Keywords><Sketch Name="Sketch1" Type="Sketch" id="1"/><Extrusion Name="Extrude" Type="BossExtrude" id="31" Profile="1" Operation="Join" EndCondition="Blind"><Dimension Name="Depth">1m</Dimension></Extrusion></Keywords>"#,
            )
        }
        None => (
            sldprt_with_body(&owned_triangle(0, 700, 0.0)),
            Vec::new(),
            r#"<Keywords><Extrusion Name="Extrude" Type="BossExtrude" id="31" Operation="Join" EndCondition="Blind"><Dimension Name="Depth">10mm</Dimension></Extrusion></Keywords>"#,
        ),
    };
    payload.extend(resolved_feature_classes_with_ids(&[(
        "moExtrusion_c",
        "Extrude",
        31,
    )]));
    let anchor = payload.len();
    payload.resize(anchor + 26, 0);
    payload[anchor..anchor + 2].copy_from_slice(&[0x0c, 0x8e]);
    payload[anchor + 4] = 1;
    payload[anchor + 12] = 1;
    payload.extend_from_slice(b"\xff\xff\x01\x00\x16\x00moDisplayDistanceDim_c");
    let block = payload.len();
    payload.resize(block + 16, 0);
    payload[block + 9] = 0x20;
    payload.extend_from_slice(&[0xff, 0xff, 0, 0, 3]);
    payload.extend_from_slice(&[0xff, 0xff, 0xff, 0xff]);
    payload.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0x80, 0xbf]);
    payload.resize(anchor + 200, 0);
    source.extend(make_block(0x42, "Contents/Keywords", keywords.as_bytes()));
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &payload,
    ));
    add_solidworks_version(&mut source, 18000);
    (native_envelope_fixture(&source), anchor)
}

fn decode(bytes: Vec<u8>) -> cadmpeg_ir::codec::DecodeResult {
    SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("the part decodes")
}

/// The extrusion `Extrude` of `ir`, its direction, and the reversal word of
/// its end spec at `anchor` in the first lane.
fn extrusion_state(
    ir: &cadmpeg_ir::CadIr,
    anchor: usize,
) -> (&cadmpeg_ir::features::Feature, ExtrudeDirection, Vec<u8>) {
    let feature = ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Extrude"))
        .expect("the extrusion");
    let FeatureOperation::Extrude { direction, .. } = feature.evaluation.definition().operation()
    else {
        panic!("Extrude is no extrusion");
    };
    let native = crate::test_support::native::sldprt_native(ir);
    let word = native.feature_input_lanes[0].native_payload[anchor + 12..anchor + 16].to_vec();
    (feature, direction.clone(), word)
}

fn messages(
    losses: &[(cadmpeg_ir::report::loss::LossKind, String)],
    code: SldprtLossCode,
) -> Vec<&str> {
    losses
        .iter()
        .filter(|(kind, _)| *kind == code.kind())
        .map(|(_, message)| message.as_str())
        .collect()
}

fn losses(
    decoded: &cadmpeg_ir::codec::DecodeResult,
) -> Vec<(cadmpeg_ir::report::loss::LossKind, String)> {
    decoded
        .report()
        .losses
        .iter()
        .map(|loss| (loss.code.clone(), loss.message.clone()))
        .collect()
}

/// Clears the decoded reversal of `Extrude` and writes the part, with the
/// write's losses.
fn clear_and_write(
    decoded: cadmpeg_ir::codec::DecodeResult,
) -> (Vec<u8>, Vec<(cadmpeg_ir::report::loss::LossKind, String)>) {
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    {
        let mut edit = decoded.ir_mut();
        let feature = edit
            .model
            .features
            .iter_mut()
            .find(|feature| feature.name.as_deref() == Some("Extrude"))
            .expect("the extrusion");
        feature.evaluation.edit(|definition, _| {
            if let FeatureDefinition::Operation(FeatureOperation::Extrude { direction, .. }) =
                definition
            {
                *direction = ExtrudeDirection::ProfileNormal {};
            }
        });
    }
    let plan = SldprtCodec
        .plan(
            EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
            TargetRequest::Inherit,
        )
        .expect("the cleared reversal writes");
    let losses = plan
        .report()
        .losses
        .iter()
        .map(|loss| (loss.code.clone(), loss.message.clone()))
        .collect();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("the write completes");
    (bytes, losses)
}

/// A reversal read from the end-spec word alone travels against the normal
/// and names its assumption. Cleared by an edit, it is written as a `0`
/// word, the written record drops its `Reverse` property, which a loss names,
/// and the written part decodes along the normal with no reversal.
#[test]
fn a_cleared_reversal_writes_a_zero_end_spec_word() {
    let (bytes, anchor) = lane_flagged(None);
    let decoded = decode(bytes);
    let (feature, direction, word) = extrusion_state(decoded.ir(), anchor);
    assert_eq!(direction, ExtrudeDirection::ReversedProfileNormal {});
    assert!(!crate::history::extrude_direction::reversal_unread(
        &feature.source_properties
    ));
    assert_eq!(word, [1, 0, 0, 0]);
    let decode_losses = losses(&decoded);
    let assumed = messages(
        &decode_losses,
        SldprtLossCode::FeatureExtrudeReversalAssumed,
    );
    assert_eq!(assumed.len(), 1, "{decode_losses:?}");
    assert!(assumed[0].contains("\"Extrude\" ("), "{assumed:?}");

    let (written, write_losses) = clear_and_write(decoded);
    assert!(!has(
        &write_losses,
        SldprtLossCode::WriterExtrudeReversalUnwritten
    ));
    assert_eq!(write_losses.len(), 2, "{write_losses:?}");
    let reshaped = messages(&write_losses, SldprtLossCode::WriterFeatureRecordReshaped);
    assert!(
        reshaped.len() == 1 && reshaped[0].contains("drops source property Reverse"),
        "{write_losses:?}"
    );
    let stale = messages(&write_losses, SldprtLossCode::WriterSolidStaleAfterEdit);
    assert!(
        stale.len() == 1 && stale[0].contains("Feature \"Extrude\" ("),
        "{write_losses:?}"
    );

    let again = decode(written);
    let (feature, direction, word) = extrusion_state(again.ir(), anchor);
    assert_eq!(direction, ExtrudeDirection::ProfileNormal {});
    assert!(!feature.source_properties.contains_key("Reverse"));
    assert_eq!(word, [0, 0, 0, 0]);
    assert!(!has(
        &losses(&again),
        SldprtLossCode::FeatureExtrudeReversalAssumed
    ));
}

/// An edit that clears a reversal whose stored caps still show it does not
/// read back as cleared: the decoder reads the reversal from the stored
/// solid as well. The feature is written with its stored record, which keeps
/// the end-spec flag, and a loss names the edit.
#[test]
fn a_cleared_reversal_over_its_stored_caps_is_named() {
    let (bytes, anchor) = lane_flagged(Some(-1.0));
    let decoded = decode(bytes);
    let (_, direction, word) = extrusion_state(decoded.ir(), anchor);
    assert_eq!(direction, ExtrudeDirection::ReversedProfileNormal {});
    assert_eq!(word, [1, 0, 0, 0]);
    // The solid holds the reversal, so it rests on more than the flag.
    assert!(!has(
        &losses(&decoded),
        SldprtLossCode::FeatureExtrudeReversalAssumed
    ));

    let (written, write_losses) = clear_and_write(decoded);
    assert_eq!(write_losses.len(), 1, "{write_losses:?}");
    let unwritten = messages(&write_losses, SldprtLossCode::WriterFeatureEditUnwritten);
    assert!(
        unwritten.len() == 1 && unwritten[0].contains("does not read back as edited"),
        "{write_losses:?}"
    );
    assert!(!has(
        &write_losses,
        SldprtLossCode::WriterSolidStaleAfterEdit
    ));

    let (_, direction, word) = extrusion_state(decode(written).ir(), anchor);
    assert_eq!(direction, ExtrudeDirection::ReversedProfileNormal {});
    assert_eq!(word, [1, 0, 0, 0]);
}
