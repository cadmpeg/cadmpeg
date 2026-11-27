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

/// Whether an `export:evaluation.stale` loss names `feature`: the writer
/// writes its edit and regenerates no state (§3.4, States).
fn stale_evaluation(
    losses: &[(cadmpeg_ir::report::loss::LossKind, String)],
    feature: &cadmpeg_ir::features::FeatureId,
) -> bool {
    losses.iter().any(|(kind, message)| {
        kind.local_code() == "evaluation.stale" && message.contains(feature.as_str())
    })
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
/// solid can neither hold nor contradict the travel. With `alternate`, the
/// part has a second, inactive configuration `Alternate` whose own lane
/// states the same end spec, with reversal flag `1` and that end-condition
/// code; the partition of `Default` makes it active. Where the code is not
/// blind (`0`), Keywords state no end condition, so that the lanes decide
/// it.
fn lane_flagged(caps: Option<f64>, alternate: Option<u8>) -> (Vec<u8>, usize) {
    flagged_part(caps, alternate, false)
}

/// The `lane_flagged` part, and with `round` a fillet `Round1` after the
/// extrusion whose radius `D1` each lane states as a scalar: 25 mm in the
/// first lane and 50 mm in the second.
fn flagged_part(caps: Option<f64>, alternate: Option<u8>, round: bool) -> (Vec<u8>, usize) {
    use crate::test_support::history::{
        resolved_features_payload, resolved_features_payload_with_names_relation_and_scalar,
    };
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
    let keywords = match alternate {
        Some(code) => {
            let keywords = keywords.replacen(
                "<Keywords>",
                r#"<Keywords><Configuration Name="Default" SourceIndex="0"/><Configuration Name="Alternate" SourceIndex="1"/>"#,
                1,
            );
            if code == 0 {
                keywords
            } else {
                keywords.replacen(r#" EndCondition="Blind""#, "", 1)
            }
        }
        None => keywords.to_owned(),
    };
    let keywords = if round {
        keywords.replacen(
            "</Keywords>",
            r#"<Fillet Name="Round1" Type="Fillet"><Dimension Name="D1">30mm</Dimension></Fillet></Keywords>"#,
            1,
        )
    } else {
        keywords
    };
    let lane = |radius: f64| {
        let mut lane = payload.clone();
        if round {
            lane.extend(resolved_features_payload_with_names_relation_and_scalar(
                &[],
                &["Round1", "D1"],
                "sgPntPntDist",
                radius,
            ));
        }
        lane
    };
    source.extend(make_block(0x42, "Contents/Keywords", keywords.as_bytes()));
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &lane(0.025),
    ));
    if let Some(code) = alternate {
        let mut payload = lane(0.050);
        payload[anchor + 18] = code;
        source.extend(make_block(
            0x45,
            "Contents/Config-1-ResolvedFeatures",
            &payload,
        ));
    }
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
fn clear_and_write(decoded: cadmpeg_ir::codec::DecodeResult) -> Written {
    edit_and_write(decoded, |ir| {
        let feature = ir
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
    })
    .expect("the cleared reversal writes")
}

/// The written bytes and the write's losses.
type Written = (Vec<u8>, Vec<(cadmpeg_ir::report::loss::LossKind, String)>);

/// Applies `edit` to the decoded part and writes it, with the write's
/// losses.
fn edit_and_write(
    decoded: cadmpeg_ir::codec::DecodeResult,
    edit: impl FnOnce(&mut cadmpeg_ir::CadIr),
) -> Result<Written, cadmpeg_ir::codec::write::EncodeError> {
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    {
        let mut ir = decoded.ir_mut();
        edit(&mut ir);
    }
    write_editable(&decoded)
}

/// Writes `decoded` with a request that names no target, with its sidecar.
fn write_editable(
    decoded: &cadmpeg_test_support::EditableDecodeResult,
) -> Result<Written, cadmpeg_ir::codec::write::EncodeError> {
    use cadmpeg_ir::codec::write::{EncodeInput, Encoder};
    let plan = SldprtCodec.plan(EncodeInput::new(
        decoded.ir(),
        Some(decoded.source_fidelity()),
    ))?;
    let losses = plan
        .report()
        .losses
        .iter()
        .map(|loss| (loss.code.clone(), loss.message.clone()))
        .collect();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("the write completes");
    Ok((bytes, losses))
}

/// A reversal read from the end-spec word alone travels against the normal
/// and names its assumption. Cleared by an edit, it is written as a `0`
/// word, and the written part decodes along the normal with no reversal.
#[test]
fn a_cleared_reversal_writes_a_zero_end_spec_word() {
    let (bytes, anchor) = lane_flagged(None, None);
    let decoded = decode(bytes);
    let (feature, direction, word) = extrusion_state(decoded.ir(), anchor);
    let extrude = feature.id.clone();
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
    assert!(
        stale_evaluation(&write_losses, &extrude),
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

/// An edit that clears a reversal whose stored caps still show it is
/// written, with the `0` word, and named by the general stale-solid loss.
#[test]
fn a_cleared_reversal_over_its_stored_caps_is_written_and_named() {
    let (bytes, anchor) = lane_flagged(Some(-1.0), None);
    let decoded = decode(bytes);
    let (feature, direction, word) = extrusion_state(decoded.ir(), anchor);
    let extrude = feature.id.clone();
    assert_eq!(direction, ExtrudeDirection::ReversedProfileNormal {});
    assert_eq!(word, [1, 0, 0, 0]);
    // The solid holds the reversal, so it rests on more than the flag.
    assert!(!has(
        &losses(&decoded),
        SldprtLossCode::FeatureExtrudeReversalAssumed
    ));

    let (written, write_losses) = clear_and_write(decoded);
    assert_eq!(write_losses.len(), 2, "{write_losses:?}");
    assert!(
        stale_evaluation(&write_losses, &extrude),
        "{write_losses:?}"
    );
    let stale = messages(&write_losses, SldprtLossCode::WriterSolidStaleAfterEdit);
    assert!(
        stale.len() == 1 && stale[0].contains("Feature \"Extrude\" ("),
        "{write_losses:?}"
    );
    assert!(!has(
        &write_losses,
        SldprtLossCode::WriterExtrudeReversalUnwritten
    ));

    let (_, _, word) = extrusion_state(decode(written).ir(), anchor);
    assert_eq!(word, [0, 0, 0, 0]);
}

/// The direction of `Extrude` in the inactive configuration `Alternate`.
fn alternate_direction(ir: &cadmpeg_ir::CadIr) -> ExtrudeDirection {
    let feature = ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Extrude"))
        .expect("the extrusion");
    let configuration = ir
        .model
        .configurations
        .iter()
        .find(|configuration| configuration.name.as_deref() == Some("Alternate"))
        .expect("the configuration Alternate");
    assert!(!configuration.active);
    let FeatureOperation::Extrude { direction, .. } = configuration.feature_states[&feature.id]
        .definition
        .operation()
    else {
        panic!("Extrude is no extrusion in Alternate");
    };
    direction.clone()
}

/// The reversal word of the end spec at `anchor` in each feature-input lane.
fn lane_words(ir: &cadmpeg_ir::CadIr, anchor: usize) -> Vec<Vec<u8>> {
    crate::test_support::native::sldprt_native(ir)
        .feature_input_lanes
        .iter()
        .map(|lane| lane.native_payload[anchor + 12..anchor + 16].to_vec())
        .collect()
}

/// With no solid to test it, the reversal of an inactive configuration rests
/// on the flag of its own lane, and the assumption names the configuration
/// and the extrusion.
#[test]
fn an_inactive_configuration_reversal_without_a_witness_is_named() {
    let (bytes, anchor) = lane_flagged(None, Some(0));
    let decoded = decode(bytes);
    let (_, direction, _) = extrusion_state(decoded.ir(), anchor);
    assert_eq!(direction, ExtrudeDirection::ReversedProfileNormal {});
    assert_eq!(
        alternate_direction(decoded.ir()),
        ExtrudeDirection::ReversedProfileNormal {}
    );
    let decode_losses = losses(&decoded);
    let assumed = messages(
        &decode_losses,
        SldprtLossCode::FeatureExtrudeReversalAssumed,
    );
    assert_eq!(assumed.len(), 2, "{decode_losses:?}");
    assert_eq!(
        assumed
            .iter()
            .filter(|message| message.contains("configuration \"Alternate\" (")
                && message.contains("\"Extrude\" ("))
            .count(),
        1,
        "{assumed:?}"
    );
}

/// The active solid witnesses the inactive state of the same extrusion with
/// the same definition: where it contradicts the reversal, the state falls
/// back with the model's feature and the contradiction names the
/// configuration; where it holds it, the state keeps it unnamed.
#[test]
fn an_inactive_configuration_reversal_follows_the_active_solid() {
    for side in [1.0, -1.0] {
        let (bytes, anchor) = lane_flagged(Some(side), Some(0));
        let decoded = decode(bytes);
        let expected = if side > 0.0 {
            ExtrudeDirection::ProfileNormal {}
        } else {
            ExtrudeDirection::ReversedProfileNormal {}
        };
        let (_, direction, _) = extrusion_state(decoded.ir(), anchor);
        assert_eq!(direction, expected, "side={side}");
        assert_eq!(alternate_direction(decoded.ir()), expected, "side={side}");
        let decode_losses = losses(&decoded);
        let contradicted = messages(
            &decode_losses,
            SldprtLossCode::FeatureExtrudeReversalContradicted,
        );
        assert_eq!(
            contradicted.len(),
            if side > 0.0 { 2 } else { 0 },
            "{decode_losses:?}"
        );
        assert_eq!(
            contradicted
                .iter()
                .filter(|message| message.contains("configuration \"Alternate\" ("))
                .count(),
            usize::from(side > 0.0),
            "{contradicted:?}"
        );
        assert!(
            !has(
                &decode_losses,
                SldprtLossCode::FeatureExtrudeReversalAssumed
            ),
            "side={side}: {decode_losses:?}"
        );
    }
}

/// The direction of `Extrude` in the active configuration `Default`.
fn default_direction(ir: &cadmpeg_ir::CadIr) -> ExtrudeDirection {
    let feature = ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Extrude"))
        .expect("the extrusion");
    let configuration = ir
        .model
        .configurations
        .iter()
        .find(|configuration| configuration.name.as_deref() == Some("Default"))
        .expect("the configuration Default");
    assert!(configuration.active);
    let FeatureOperation::Extrude { direction, .. } = configuration.feature_states[&feature.id]
        .definition
        .operation()
    else {
        panic!("Extrude is no extrusion in Default");
    };
    direction.clone()
}

/// Where the lanes state different end conditions under flag `1`, the model
/// feature travels along the normal, and the active solid tests the reversed
/// state of the active configuration itself. Where the solid contradicts
/// it, the state falls back and is named with its configuration, and the
/// inactive state of the same extrusion follows it. Where the solid holds
/// it, the state keeps it unnamed, and the inactive state, whose definition
/// differs, rests on its flag and is named.
#[test]
fn an_active_configuration_reversal_takes_the_active_solid_verdict() {
    for side in [1.0, -1.0] {
        let (bytes, anchor) = lane_flagged(Some(side), Some(1));
        let decoded = decode(bytes);
        let (_, direction, _) = extrusion_state(decoded.ir(), anchor);
        assert_ne!(
            direction,
            ExtrudeDirection::ReversedProfileNormal {},
            "side={side}"
        );
        let decode_losses = losses(&decoded);
        let contradicted = messages(
            &decode_losses,
            SldprtLossCode::FeatureExtrudeReversalContradicted,
        );
        let assumed = messages(
            &decode_losses,
            SldprtLossCode::FeatureExtrudeReversalAssumed,
        );
        if side > 0.0 {
            assert_eq!(
                default_direction(decoded.ir()),
                ExtrudeDirection::ProfileNormal {}
            );
            assert_eq!(
                alternate_direction(decoded.ir()),
                ExtrudeDirection::ProfileNormal {}
            );
            assert_eq!(contradicted.len(), 2, "{decode_losses:?}");
            assert!(
                contradicted
                    .iter()
                    .any(|message| message.contains("configuration \"Default\" (")
                        && message.contains("as the solid of that configuration shows")),
                "{contradicted:?}"
            );
            assert!(
                contradicted
                    .iter()
                    .any(|message| message.contains("configuration \"Alternate\" (")
                        && message.contains("as the active solid contradicts that flag")),
                "{contradicted:?}"
            );
            assert!(assumed.is_empty(), "{decode_losses:?}");
        } else {
            assert_eq!(
                default_direction(decoded.ir()),
                ExtrudeDirection::ReversedProfileNormal {}
            );
            assert_eq!(
                alternate_direction(decoded.ir()),
                ExtrudeDirection::ReversedProfileNormal {}
            );
            assert!(contradicted.is_empty(), "{decode_losses:?}");
            assert_eq!(assumed.len(), 1, "{decode_losses:?}");
            assert!(
                assumed[0].contains("configuration \"Alternate\" (")
                    && assumed[0].contains("the decoder tests no solid of that configuration"),
                "{assumed:?}"
            );
        }
    }
}

/// Clearing the model's reversal clears the flag in every lane. The write
/// names the inactive configuration's lane, whose state still travels
/// against the normal, and that configuration decodes along it. The edit
/// also carries the general stale-solid loss for the extrusion.
#[test]
fn a_cleared_reversal_names_the_inactive_configuration_lane_it_clears() {
    let (bytes, anchor) = lane_flagged(None, Some(0));
    let decoded = decode(bytes);
    let extrude = extrusion_state(decoded.ir(), anchor).0.id.clone();
    assert_eq!(
        lane_words(decoded.ir(), anchor),
        [[1, 0, 0, 0], [1, 0, 0, 0]]
    );
    let (written, write_losses) = clear_and_write(decoded);
    let cleared = messages(
        &write_losses,
        SldprtLossCode::WriterExtrudeReversalConfigurationCleared,
    );
    assert_eq!(cleared.len(), 1, "{write_losses:?}");
    assert!(
        cleared[0].contains("Extrusion \"Extrude\" (")
            && cleared[0].contains("configuration \"Alternate\" ("),
        "{cleared:?}"
    );
    assert!(!has(
        &write_losses,
        SldprtLossCode::WriterExtrudeReversalUnwritten
    ));
    let stale = messages(&write_losses, SldprtLossCode::WriterSolidStaleAfterEdit);
    assert!(
        stale.len() == 1 && stale[0].contains("Feature \"Extrude\" ("),
        "{write_losses:?}"
    );
    assert!(
        stale_evaluation(&write_losses, &extrude),
        "{write_losses:?}"
    );
    assert_eq!(write_losses.len(), 3, "{write_losses:?}");

    let again = decode(written);
    assert_eq!(lane_words(again.ir(), anchor), [[0, 0, 0, 0], [0, 0, 0, 0]]);
    assert_eq!(
        alternate_direction(again.ir()),
        ExtrudeDirection::ProfileNormal {}
    );
}

/// The `Depth` parameter of `Extrude` and its value in the configuration
/// `Alternate`.
fn alternate_depth(ir: &cadmpeg_ir::CadIr) -> (cadmpeg_ir::features::ParameterId, f64) {
    use cadmpeg_ir::features::parameters::ParameterValue;
    let feature = ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Extrude"))
        .expect("the extrusion");
    let parameter = ir
        .model
        .parameters
        .iter()
        .find(|parameter| {
            parameter.owner.as_ref() == Some(&feature.id) && parameter.name == "Depth"
        })
        .expect("the depth of the extrusion");
    let configuration = ir
        .model
        .configurations
        .iter()
        .find(|configuration| configuration.name.as_deref() == Some("Alternate"))
        .expect("the configuration Alternate");
    let Some(ParameterValue::Length(depth)) = configuration.parameter_values.get(&parameter.id)
    else {
        panic!("Alternate states no depth for Extrude");
    };
    (parameter.id.clone(), depth.get())
}

/// Doubles the depth of `Extrude` in the inactive configuration `Alternate`:
/// its parameter value and, where that state is blind, its length.
fn deepen_alternate(ir: &mut cadmpeg_ir::CadIr) {
    use cadmpeg_ir::features::extents::{ExtrudeExtent, LinearTermination};
    use cadmpeg_ir::features::parameters::ParameterValue;
    use cadmpeg_ir::scalar::{Length, NonZeroLength};
    let (parameter, depth) = alternate_depth(ir);
    let feature = ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Extrude"))
        .expect("the extrusion")
        .id
        .clone();
    let configuration = ir
        .model
        .configurations
        .iter_mut()
        .find(|configuration| configuration.name.as_deref() == Some("Alternate"))
        .expect("the configuration Alternate");
    configuration.parameter_values.insert(
        parameter,
        ParameterValue::Length(Length::new(2.0 * depth).expect("a finite depth")),
    );
    let state = configuration
        .feature_states
        .get_mut(&feature)
        .expect("the state of Extrude in Alternate");
    if let FeatureDefinition::Operation(FeatureOperation::Extrude {
        extent: ExtrudeExtent::OneSided { side },
        ..
    }) = &mut state.definition
    {
        if let LinearTermination::Blind { length } = &mut side.termination {
            *length = NonZeroLength::new(2.0 * depth).expect("a nonzero depth");
        }
    }
}

/// The four parts of the `lane_flagged` fixture with an inactive
/// configuration `Alternate` and caps: the lanes state the same end
/// condition (`0`) or not (`1`), and the solid holds the reversal (`-1`) or
/// contradicts it (`1`).
const SETTLED_CASES: [(u8, f64); 4] = [(0, 1.0), (0, -1.0), (1, 1.0), (1, -1.0)];

/// The travel of `Extrude` in the model, in `Default` and in `Alternate`.
fn directions(
    ir: &cadmpeg_ir::CadIr,
    anchor: usize,
) -> (ExtrudeDirection, ExtrudeDirection, ExtrudeDirection) {
    (
        extrusion_state(ir, anchor).1,
        default_direction(ir),
        alternate_direction(ir),
    )
}

/// An edit of a configuration parameter over configuration states whose
/// reversal the decoder settled against the active solid is no native edit:
/// the writer settles the states it reads back from the lanes as the
/// decoder settled them. The fixture states the depth only as a Keywords
/// dimension, so no lane scalar can carry the edit: the patch path declines
/// it, and the write names that alike whether the solid holds the reversal
/// or contradicts it.
#[test]
fn a_configuration_edit_over_settled_reversals_is_no_native_edit() {
    let outcomes = SETTLED_CASES.map(|(alternate, side)| {
        let (bytes, _) = lane_flagged(Some(side), Some(alternate));
        let (written, losses) =
            edit_and_write(decode(bytes), deepen_alternate).expect("the write completes");
        decode(written);
        (alternate, side, losses)
    });
    for (alternate, side, losses) in &outcomes {
        assert!(
            losses.iter().any(|(_, message)| message
                .contains("configuration design-state edit has no complete native lane encoding")),
            "alternate={alternate} side={side}: {outcomes:#?}"
        );
    }
}

/// A part whose configuration baselines are gone, as in a document built
/// from another source, writes over configuration states whose reversal
/// the decoder settled against the active solid, and decodes to the same
/// travel in every configuration.
#[test]
fn configuration_states_without_baselines_write_over_settled_reversals() {
    let mut refused = Vec::new();
    for (alternate, side) in SETTLED_CASES {
        let (bytes, anchor) = lane_flagged(Some(side), Some(alternate));
        let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decode(bytes));
        let decoded_directions = directions(decoded.ir(), anchor);
        let digests = &mut decoded.source_fidelity_mut().codec_digests;
        digests.remove("sldprt_configuration_feature_states_local_sha256");
        digests.remove("sldprt_configuration_parameter_values_local_sha256");
        let written = write_editable(&decoded);
        match written {
            Ok((written, _)) => assert_eq!(
                directions(decode(written).ir(), anchor),
                decoded_directions,
                "alternate={alternate} side={side}"
            ),
            Err(error) => refused.push(format!("alternate={alternate} side={side}: {error}")),
        }
    }
    assert!(refused.is_empty(), "{refused:#?}");
}

/// The value of `D1` of `Round1` in the configuration `Alternate`, and the
/// radius of that configuration's state of `Round1`.
fn alternate_round(ir: &cadmpeg_ir::CadIr) -> (f64, f64) {
    use cadmpeg_ir::features::edge_treatments::{FilletGroup, RadiusSpec};
    use cadmpeg_ir::features::parameters::ParameterValue;
    let feature = ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Round1"))
        .expect("the fillet");
    let parameter = ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.owner.as_ref() == Some(&feature.id) && parameter.name == "D1")
        .expect("the radius of the fillet");
    let configuration = ir
        .model
        .configurations
        .iter()
        .find(|configuration| configuration.name.as_deref() == Some("Alternate"))
        .expect("the configuration Alternate");
    let Some(ParameterValue::Length(value)) = configuration.parameter_values.get(&parameter.id)
    else {
        panic!("Alternate states no radius for Round1");
    };
    let FeatureOperation::Fillet { groups } = configuration.feature_states[&feature.id]
        .definition
        .operation()
    else {
        panic!("Round1 is no fillet in Alternate");
    };
    let [FilletGroup {
        radius: RadiusSpec::Constant { radius },
        ..
    }] = groups.as_slice()
    else {
        panic!("Round1 has no constant radius in Alternate");
    };
    (value.get(), radius.get())
}

/// Sets the radius of `Round1` in the configuration `Alternate`: the value of
/// `D1` there, and the radius of that configuration's state of `Round1`.
fn round_alternate(ir: &mut cadmpeg_ir::CadIr, radius: f64) {
    use cadmpeg_ir::features::edge_treatments::RadiusSpec;
    use cadmpeg_ir::features::parameters::ParameterValue;
    use cadmpeg_ir::scalar::{Length, PositiveLength};
    let feature = ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Round1"))
        .expect("the fillet")
        .id
        .clone();
    let parameter = ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.owner.as_ref() == Some(&feature) && parameter.name == "D1")
        .expect("the radius of the fillet")
        .id
        .clone();
    let configuration = ir
        .model
        .configurations
        .iter_mut()
        .find(|configuration| configuration.name.as_deref() == Some("Alternate"))
        .expect("the configuration Alternate");
    configuration.parameter_values.insert(
        parameter,
        ParameterValue::Length(Length::new(radius).expect("a finite radius")),
    );
    let Some(FeatureDefinition::Operation(FeatureOperation::Fillet { groups })) = configuration
        .feature_states
        .get_mut(&feature)
        .map(|state| &mut state.definition)
    else {
        panic!("Round1 is no fillet in Alternate");
    };
    groups[0].radius = RadiusSpec::Constant {
        radius: PositiveLength::new(radius).expect("a positive radius"),
    };
}

/// An edit of a configuration parameter that a lane scalar carries, over
/// configuration states whose reversal the decoder settled against the
/// active solid, is written: the writer settles the states it reads back
/// from the patched lanes as it settled those it read before the patch. The
/// written part decodes the edited value and the same travel in every
/// configuration.
#[test]
fn a_lane_scalar_configuration_edit_writes_over_settled_reversals() {
    let mut failures = Vec::new();
    for (alternate, side) in SETTLED_CASES {
        let case = format!("alternate={alternate} side={side}");
        let (bytes, anchor) = flagged_part(Some(side), Some(alternate), true);
        let decoded = decode(bytes);
        assert_eq!(alternate_round(decoded.ir()), (50.0, 50.0), "{case}");
        assert_eq!(
            lane_words(decoded.ir(), anchor),
            [[1, 0, 0, 0], [1, 0, 0, 0]],
            "{case}"
        );
        let decoded_directions = directions(decoded.ir(), anchor);
        if side > 0.0 {
            assert_eq!(
                decoded_directions.1,
                ExtrudeDirection::ProfileNormal {},
                "{case}"
            );
        }
        match edit_and_write(decoded, |ir| round_alternate(ir, 75.0)) {
            Ok((written, _)) => {
                let again = decode(written);
                assert_eq!(alternate_round(again.ir()), (75.0, 75.0), "{case}");
                assert_eq!(directions(again.ir(), anchor), decoded_directions, "{case}");
            }
            Err(error) => failures.push(format!("{case}: {error}")),
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
