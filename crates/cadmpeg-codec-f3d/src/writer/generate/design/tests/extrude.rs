// SPDX-License-Identifier: Apache-2.0
//! The sketch-driven `Extrude` a design writes in place of its Base Feature
//! (f3d.md §3.1, "Records of a sketch-driven Extrude", "Side faces" and
//! "Side-face walk"), decoded back and read from the archive's Design
//! segment.

use cadmpeg_ir::design::container::Container;
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::shared::{Limit, Span};
use cadmpeg_ir::design::ops::sketch::{DefiningRole, PlanarCurve};
use cadmpeg_ir::design::ops::swept::{Extrude, ExtrudeDirection, ExtrudeOrigin};
use cadmpeg_ir::design::options::{
    BodyForm, Boolean, NativeOptions, SolidOutput, SymmetricMeasure,
};
use cadmpeg_ir::design::parameter::ParameterValue;
use cadmpeg_ir::design::refs::{Member, Position};
use cadmpeg_ir::design::selection::{
    Anchor, Members, PickTarget, Picked, Picks, PrincipalEntity, Selection,
};
use cadmpeg_ir::design::units::UnitSymbol;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::extents::{ExtrudeExtent, LinearTermination};
use cadmpeg_ir::features::operation_settings::BooleanOp;
use cadmpeg_ir::features::parameters::ParameterValue as V10Value;
use cadmpeg_ir::features::profiles::{PlanarProfileRef, ProfileRef};
use cadmpeg_ir::features::FeatureId;
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use cadmpeg_ir::scalar::Length;
use cadmpeg_ir::sketches::SketchId;

use super::super::extrude::profile::{profile, LoopEntry, ProfileLoop};
use super::super::sketch::{CurveGeometry, OriginPlane, PlannedCurve, PlannedSketch};
use super::roster::segment;
use super::sketch::{
    datum, feature, feature_id, fixed_clock_tables, frame, picked, point2, principal, sketch_id,
    Source,
};
use super::{
    loss, neutral, number, parameter_id, scoped_cube, write, write_bytes, EXTRUDE, SKETCH,
};

mod arcs;
mod faces;
mod holes;

const TOLERANCE: f64 = 1e-9;
/// The cube's edge, in mm.
const SIDE: f64 = 10.0;
/// The loss code of a feature written without its state.
const EVALUATION_MISSING: &str = "export/evaluation.missing";

/// The cube's bottom square on `XY`, drawn counterclockwise about +Z as
/// four lines whose corners are four points with two roles each.
fn square() -> Source {
    square_drawn([false; 4])
}

/// [`square`] with line `l<i + 1>` drawn from its counterclockwise end to
/// its start where `against[i]`.
fn square_drawn(against: [bool; 4]) -> Source {
    let mut sketch = Source::new(
        "profile",
        picked(principal(PrincipalEntity::XyPlane)),
        frame([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
    );
    let corners = [(0.0, 0.0), (SIDE, 0.0), (SIDE, SIDE), (0.0, SIDE)];
    let role = |line: usize, start: bool| {
        if start == against[line] {
            DefiningRole::End
        } else {
            DefiningRole::Start
        }
    };
    for (index, (u, v)) in corners.iter().enumerate() {
        let before = (index + 3) % 4;
        let start_of = format!("l{}", index + 1);
        let end_of = format!("l{}", before + 1);
        sketch.entity(
            &format!("p{}", index + 1),
            PlanarCurve::Point(point2(*u, *v)),
            &[
                (start_of.as_str(), role(index, true)),
                (end_of.as_str(), role(before, false)),
            ],
        );
    }
    for (index, corner) in corners.iter().enumerate() {
        let (start, end) = if against[index] {
            (corners[(index + 1) % 4], *corner)
        } else {
            (*corner, corners[(index + 1) % 4])
        };
        sketch.entity(
            &format!("l{}", index + 1),
            PlanarCurve::Line {
                start: point2(start.0, start.1),
                end: point2(end.0, end.1),
            },
            &[],
        );
    }
    sketch
}

fn extrude(span: Span<Length>) -> Extrude {
    Extrude {
        profile: Selection {
            at: Anchor::Input,
            picks: Picks::Named(
                Members::new(vec![Picked {
                    target: PickTarget::Sketch(sketch_id("profile")),
                    context: None,
                    native: None,
                    domain: None,
                    witness: None,
                    resolved: None,
                }])
                .expect("one pick"),
            ),
            native: None,
        },
        loops: None,
        direction: Some(ExtrudeDirection::ProfileNormal { reversed: None }),
        origin: Some(ExtrudeOrigin::ProfilePlane),
        span: Some(span),
        draft: None,
        fit: None,
        outside: None,
        sheet_cut: None,
        output: SolidOutput {
            body: Some(BodyForm::Solid),
            boolean: Some(Boolean::NewBody),
        },
        native_options: NativeOptions::new(),
        source_settings: NativeOptions::new(),
    }
}

fn blind() -> Span<Length> {
    Span::Between {
        start: Box::new(Limit::AtOrigin),
        end: Box::new(Limit::Amount(Dimension::Parameter(parameter_id("d1")))),
    }
}

/// The unit cube as `Sketch` (the bottom square on `XY`), `Extrude` (one
/// side, the cube's height, `D1 = 10 mm`), and a datum plane after it, with
/// the current state after the datum. `later` adds a `Sketch` of one line
/// after the extrude.
fn sketch_and_extrude(span: Span<Length>, later: bool) -> CadIr {
    sketch_and_extrude_of(square(), span, later)
}

/// [`sketch_and_extrude`] with `profile` as the extrude's sketch.
fn sketch_and_extrude_of(profile: Source, span: Span<Length>, later: bool) -> CadIr {
    let mut ir = scoped_cube();
    let extrude_id = FeatureId::mint(EXTRUDE).expect("identity grammar");
    ir.design
        .features
        .retain(|record| record.id.as_str() != SKETCH && record.id != extrude_id);
    let sketch = profile.add_to(&mut ir);
    ir.design.features.push(feature(
        "extrude",
        FeatureOperation::Extrude(Box::new(extrude(span))),
    ));
    let mut members = vec![Member::Feature(sketch), Member::Feature(extrude_id)];
    if later {
        let mut line = Source::new(
            "later",
            picked(principal(PrincipalEntity::XyPlane)),
            frame([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        );
        line.entity(
            "line",
            PlanarCurve::Line {
                start: point2(1.0, 1.0),
                end: point2(3.0, 2.0),
            },
            &[],
        );
        members.push(Member::Feature(line.add_to(&mut ir)));
    }
    let after = datum("after", PrincipalEntity::XzPlane);
    let after_id = after.id.clone();
    ir.design.features.push(after);
    members.push(Member::Feature(after_id.clone()));
    let Some(Container::Ordered {
        members: timeline, ..
    }) = ir.design.containers.first_mut()
    else {
        panic!("the cube's timeline");
    };
    *timeline = members;
    ir.design.scopes[0].current[0].at = Position::After(Member::Feature(after_id));
    let depth = ir
        .design
        .parameters
        .iter_mut()
        .find(|parameter| parameter.id == parameter_id("d1"))
        .expect("the extrude's depth");
    depth.definition = neutral(number(SIDE, Some(UnitSymbol::Mm)));
    depth.value = Some(ParameterValue::Number(crate::test_support::real(SIDE)));
    ir
}

/// The decoded features' kinds in timeline order.
fn kinds(decoded: &CadIr) -> Vec<&'static str> {
    let mut features = decoded.model.features.iter().collect::<Vec<_>>();
    features.sort_by_key(|feature| feature.ordinal);
    features
        .iter()
        .filter_map(|feature| match feature.evaluation.definition() {
            cadmpeg_ir::features::FeatureDefinition::Operation(
                cadmpeg_ir::features::FeatureOperation::Sketch { .. },
            ) => Some("sketch"),
            cadmpeg_ir::features::FeatureDefinition::Operation(
                cadmpeg_ir::features::FeatureOperation::Extrude { .. },
            ) => Some("extrude"),
            cadmpeg_ir::features::FeatureDefinition::Operation(
                cadmpeg_ir::features::FeatureOperation::BaseFeature { .. },
            ) => Some("base"),
            _ => None,
        })
        .collect()
}

fn decoded_extrude(decoded: &CadIr) -> &cadmpeg_ir::features::Feature {
    decoded
        .model
        .features
        .iter()
        .find(|feature| {
            matches!(
                feature.evaluation.definition(),
                cadmpeg_ir::features::FeatureDefinition::Operation(
                    cadmpeg_ir::features::FeatureOperation::Extrude { .. }
                )
            )
        })
        .expect("a decoded Extrude")
}

#[test]
fn the_extrude_replaces_the_base_feature() {
    let (report, decoded) = write(&sketch_and_extrude(blind(), false));
    assert_eq!(
        kinds(&decoded),
        ["sketch", "extrude"],
        "{:#?}",
        report.losses
    );
    let cadmpeg_ir::features::FeatureDefinition::Operation(
        cadmpeg_ir::features::FeatureOperation::Extrude {
            profile,
            extent,
            op,
            solid,
            ..
        },
    ) = decoded_extrude(&decoded).evaluation.definition()
    else {
        panic!("an Extrude");
    };
    assert!(
        matches!(
            profile,
            ProfileRef::Planar(
                PlanarProfileRef::Sketch(_) | PlanarProfileRef::SketchProfiles { .. }
            )
        ),
        "{profile:?}"
    );
    let ExtrudeExtent::OneSided { side } = extent else {
        panic!("one side: {extent:?}");
    };
    let LinearTermination::Blind { length } = &side.termination else {
        panic!("blind: {side:?}");
    };
    assert!((length.get() - SIDE).abs() <= TOLERANCE, "{length:?}");
    assert_eq!(*op, BooleanOp::NewBody);
    assert_ne!(*solid, Some(false));
    assert!(
        loss(&report, EVALUATION_MISSING)
            .iter()
            .all(|message| !message.contains(EXTRUDE)),
        "{:#?}",
        report.losses
    );
}

#[test]
fn an_extrude_design_charges_a_hidden_body_and_every_fixed_clock_table() {
    let mut ir = sketch_and_extrude(blind(), false);
    ir.geometry.bodies[0].visible = Some(false);
    let body = ir.geometry.bodies[0].id.to_string();
    let (report, decoded, bytes) = write_bytes(&ir);
    assert_eq!(
        kinds(&decoded),
        ["sketch", "extrude"],
        "{:#?}",
        report.losses
    );
    assert!(
        loss(&report, "export/value.unwritable")
            .iter()
            .any(|message| message.contains("as visible") && message.contains(&body)),
        "{:#?}",
        report.losses
    );
    let tables = fixed_clock_tables(&report, &bytes);
    // The component table, three of the sketch and six of the Extrude:
    // its scope, profile group, start record and profile selection, and
    // one of each owned parameter. The Base Feature tables 210 and 214
    // are not written.
    assert_eq!(tables.len(), 1 + 3 + 6, "{tables:?}");
    assert!(
        !tables.contains(&210) && !tables.contains(&214),
        "{tables:?}"
    );
}

#[test]
fn an_extrude_design_names_no_base_feature_in_its_notes() {
    let mut ir = sketch_and_extrude(blind(), false);
    let guid = "11111111-2222-3333-4444-555555555555";
    ir.model.appearances = vec![cadmpeg_ir::appearance::Appearance {
        id: cadmpeg_ir::ids::AppearanceId::mint("generated:test:appearance#0")
            .expect("identity grammar"),
        name: Some("Prism-Generated".into()),
        asset_guid: Some(guid.into()),
        library_id: None,
        visual_guid: Some(guid.into()),
        physical_token: Some("PrismMaterial-Generated".into()),
        schema: Some("GenericSchema".into()),
        category: Some("Plastic/Generated".into()),
        base_color: Some(
            cadmpeg_ir::topology::Color::new(0.15, 0.35, 0.75, 1.0).expect("valid color"),
        ),
        properties: std::collections::BTreeMap::from([(
            cadmpeg_core::nonblank_literal!("refraction_index"),
            cadmpeg_ir::scalar::FiniteReal::new(1.5).expect("finite scalar"),
        )]),
        textures: Vec::new(),
        native_ref: None,
    }];
    let (report, decoded) = write(&ir);
    assert_eq!(
        kinds(&decoded),
        ["sketch", "extrude"],
        "{:#?}",
        report.losses
    );
    let messages = report
        .losses
        .iter()
        .map(|loss| loss.message.as_str())
        .collect::<Vec<_>>();
    // The appearance and Sketch-item notes, each of which named a Base
    // Feature in an Extrude design.
    for charged in [
        "the one Protein package its asset manager names",
        "record 199 (owner collection: the order of the Sketch and Extrude members)",
        "as the Extrude item tail)",
    ] {
        assert!(
            messages.iter().any(|message| message.contains(charged)),
            "{charged}: {messages:#?}"
        );
    }
    for message in messages {
        assert!(!message.contains("Base Feature"), "{message}");
    }
}

#[test]
fn the_distance_is_an_owned_parameter_of_the_extrude() {
    let (_, decoded) = write(&sketch_and_extrude(blind(), false));
    let extrude = decoded_extrude(&decoded).id.clone();
    let owned = decoded
        .model
        .parameters
        .iter()
        .filter(|parameter| parameter.owner.as_ref() == Some(&extrude))
        .collect::<Vec<_>>();
    let depth = owned
        .iter()
        .find(|parameter| parameter.name == "D1")
        .unwrap_or_else(|| panic!("D1 is owned by the Extrude: {owned:#?}"));
    let Some(V10Value::Length(length)) = &depth.value else {
        panic!("D1 is a length: {depth:#?}");
    };
    assert!((length.get() - SIDE).abs() <= TOLERANCE);
    assert_eq!(owned.len(), 2, "{owned:#?}");
    let taper = owned
        .iter()
        .find(|parameter| parameter.name != "D1")
        .expect("the taper");
    assert!(
        matches!(&taper.value, Some(V10Value::Angle(angle)) if angle.get() == 0.0),
        "{taper:#?}"
    );
    assert!(decoded
        .model
        .parameters
        .iter()
        .any(|parameter| parameter.name == "Width" && parameter.owner.is_none()));
}

/// The LP-UTF16 string at `at` and the offset after it.
fn utf16_at(bytes: &[u8], at: usize) -> (String, usize) {
    let units = usize::try_from(u32_at(bytes, at)).expect("length");
    let end = at + 4 + 2 * units;
    let units = bytes[at + 4..end]
        .chunks(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect::<Vec<_>>();
    (String::from_utf16(&units).expect("UTF-16"), end)
}

/// Each parameter record is framed by the version of the type the
/// `MetaStream` registers for it (f3d.md §3.1): the eleven-byte header,
/// and the nine-byte expression trailer at version 7 or the ten-byte one at
/// version 8, whose last byte is the document flag.
#[test]
fn every_parameter_record_closes_with_the_trailer_of_its_type_version() {
    let mut ir = sketch_and_extrude(blind(), false);
    ir.design
        .parameters
        .iter_mut()
        .find(|parameter| parameter.id == parameter_id("width"))
        .expect("Width")
        .comment = Some("overall width".into());
    let (_, _, bytes) = write_bytes(&ir);
    let design = segment(&bytes, "FusionDesignSegmentType1");
    let mut written = Vec::new();
    for (index, version) in design.records_of(super::super::types::PARAMETER.guid) {
        let record = design.record(index);
        let trailer = match version {
            7 => 9,
            8 => 10,
            other => panic!("parameter type version {other}"),
        };
        assert_eq!(&record[11..22], &[0; 11], "record {index}");
        assert!(u32_at(record, 22) <= 6, "record {index}");
        assert_eq!(record[26], 0, "record {index}");
        let (comment, at) = utf16_at(record, 27);
        let (owned, expression_at) = match record[at + 4] {
            0 => (false, at + 5),
            1 => {
                assert_eq!(&record[at + 13..at + 15], &[0; 2], "record {index}");
                (true, at + 15)
            }
            marker => panic!("record {index}: owner marker {marker}"),
        };
        let (_, end) = utf16_at(record, expression_at);
        let mut expected = vec![0; trailer];
        expected[trailer - 1] = u8::from(!owned);
        assert_eq!(&record[end..end + trailer], expected, "record {index}");
        let (kind, end) = utf16_at(record, end + trailer);
        assert_eq!(u32_at(record, end), 0, "record {index}: the text value");
        written.push((kind, comment));
    }
    for expected in [
        ("AlongDistance", ""),
        ("TaperAngle", ""),
        ("User Parameter", "overall width"),
    ] {
        assert!(
            written
                .iter()
                .any(|(kind, comment)| (kind.as_str(), comment.as_str()) == expected),
            "{expected:?} in {written:?}"
        );
    }
}

#[test]
fn the_extrude_commits_the_head_state() {
    let (_, decoded) = write(&sketch_and_extrude(blind(), false));
    let [body] = decoded.geometry.bodies.as_slice() else {
        panic!("one body");
    };
    assert_eq!(
        decoded_extrude(&decoded).evaluation.outputs().as_slice(),
        std::slice::from_ref(&body.id)
    );
}

/// The plane of each decoded face: a point on it and its normal.
fn planes(decoded: &CadIr) -> Vec<(&cadmpeg_ir::ids::FaceId, [f64; 3], [f64; 3])> {
    decoded
        .geometry
        .faces
        .iter()
        .filter_map(|face| {
            let surface = decoded
                .geometry
                .surfaces
                .iter()
                .find(|surface| surface.id == face.surface)?;
            let Some(SolvedSurfaceGeometry::Plane(plane)) = surface.geometry.solved() else {
                return None;
            };
            let origin = plane.origin().get();
            let normal = plane.frame().axis().as_raw();
            Some((
                &face.id,
                [origin.x, origin.y, origin.z],
                [normal.x, normal.y, normal.z],
            ))
        })
        .collect()
}

#[test]
fn caps_and_sides_are_named_by_the_walk() {
    let (_, decoded) = write(&sketch_and_extrude(blind(), false));
    let native = crate::test_support::native_test::f3d_native(&decoded);
    let planes = planes(&decoded);
    // Token, the axis the face is normal to, and its offset along it: the
    // walk starts on l4 (the line written last, at x = 0) and runs
    // counterclockwise.
    let expected = [
        ("1", 2, 0.0),
        ("2", 2, SIDE),
        ("3", 0, 0.0),
        ("4", 1, 0.0),
        ("5", 0, SIDE),
        ("6", 1, SIDE),
    ];
    for (token, axis, offset) in expected {
        let tag = native
            .persistent_subentity_tags
            .iter()
            .find(|tag| tag.token.as_str() == token)
            .unwrap_or_else(|| panic!("token {token}: {:#?}", native.persistent_subentity_tags));
        assert_eq!(tag.design_references, [301], "token {token}");
        assert_eq!(tag.selector, 1, "token {token}");
        let cadmpeg_ir::attributes::AttributeTarget::Face(face) = &tag.target else {
            panic!("token {token} names a face");
        };
        let (_, origin, normal) = planes
            .iter()
            .find(|(id, _, _)| *id == face)
            .expect("a planar face");
        assert!(
            (normal[axis].abs() - 1.0).abs() <= TOLERANCE,
            "token {token}"
        );
        assert!((origin[axis] - offset).abs() <= TOLERANCE, "token {token}");
    }
    assert_eq!(native.persistent_subentity_tags.len(), 6);
    // The curve identities from 100: the origin point, p1 to p4, l1 to l4.
    let face_of = |coedge: &cadmpeg_ir::ids::CoedgeId| {
        let owner = &decoded
            .geometry
            .coedges
            .iter()
            .find(|candidate| &candidate.id == coedge)
            .expect("a coedge")
            .owner_loop;
        decoded
            .geometry
            .loops
            .iter()
            .find(|face_loop| &face_loop.id == owner)
            .map(|face_loop| face_loop.face.clone())
            .expect("a loop")
    };
    let token_of = |face: &cadmpeg_ir::ids::FaceId| {
        native
            .persistent_subentity_tags
            .iter()
            .find(|tag| tag.target == cadmpeg_ir::attributes::AttributeTarget::Face(face.clone()))
            .map(|tag| tag.token.as_str().to_owned())
            .expect("a named face")
    };
    assert_eq!(native.sketch_curve_links.len(), 8);
    let mut on_cap = 0;
    for link in &native.sketch_curve_links {
        let cadmpeg_ir::attributes::AttributeTarget::Coedge(coedge) = &link.target else {
            panic!("a link names a coedge");
        };
        let token = token_of(&face_of(coedge));
        if token == "1" {
            on_cap += 1;
            continue;
        }
        let expected = match token.as_str() {
            "3" => 108,
            "4" => 105,
            "5" => 106,
            "6" => 107,
            other => panic!("a link on face {other}"),
        };
        assert_eq!(link.sketch_curve_id, expected, "token {token}");
    }
    assert_eq!(on_cap, 4);
}

/// f3d.md §3.1, "Sketch links of the start cap": the sense of a link is
/// the traversal flag of its exterior loop entry. `l4` (identity 108) is
/// drawn against the ring, so its entry's flag is 1.
#[test]
fn a_cap_link_takes_the_traversal_flag_of_its_loop_entry() {
    let profile = square_drawn([false, false, false, true]);
    let (report, decoded) = write(&sketch_and_extrude_of(profile, blind(), false));
    assert_eq!(
        kinds(&decoded),
        ["sketch", "extrude"],
        "{:#?}",
        report.losses
    );
    let native = crate::test_support::native_test::f3d_native(&decoded);
    let mut senses = native
        .sketch_curve_links
        .iter()
        .map(|link| (link.sketch_curve_id, link.sense.map(i64::from)))
        .collect::<Vec<_>>();
    senses.sort_unstable();
    assert_eq!(
        senses,
        [
            (105, Some(0)),
            (105, Some(0)),
            (106, Some(0)),
            (106, Some(0)),
            (107, Some(0)),
            (107, Some(0)),
            (108, Some(1)),
            (108, Some(1)),
        ]
    );
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    super::u32_at(bytes, at)
}

fn u64_at(bytes: &[u8], at: usize) -> u64 {
    super::u64_at(bytes, at)
}

/// The record index a plain reference at `at` names.
fn reference_at(bytes: &[u8], at: usize) -> u64 {
    u64::from(u32_at(bytes, at + 1))
}

#[test]
fn the_records_follow_fusions_sketch_and_extrude_design() {
    let (_, _, bytes) = write_bytes(&sketch_and_extrude(blind(), false));
    let design = segment(&bytes, "FusionDesignSegmentType1");
    let timeline = design.record(207);
    assert_eq!(u32_at(timeline, 32), 2);
    let (sketch, scope) = (reference_at(timeline, 36), reference_at(timeline, 47));
    assert!(sketch > 224 && scope > sketch, "{sketch} {scope}");
    let (guid, version) = design.type_of(scope);
    assert!(guid.eq_ignore_ascii_case("DD405BC2-D673-44F0-8833-5CB2A1C186C7"));
    assert_eq!(version, 8);
    for absent in [209, 210, 211, 212, 213, 214, 223] {
        assert!(
            design.primary.iter().all(|(index, _)| *index != absent),
            "{absent}"
        );
    }
    // The browser node of the body names the Extrude scope after its GUID,
    // two flag bytes and the presentation owner.
    assert_eq!(reference_at(design.record(224), 110), scope);

    let table = design.record(203);
    assert_eq!(u32_at(table, 21), 2);
    assert_eq!(
        (u64_at(table, 25), u32_at(table, 33), u64_at(table, 37)),
        (sketch, 1, 300)
    );
    assert_eq!(
        (
            u64_at(table, 45),
            u32_at(table, 53),
            u64_at(table, 57),
            u64_at(table, 65)
        ),
        (scope, 2, 301, 302)
    );
    assert_eq!(u32_at(table, 73), 3);
    let pairs = (0..3)
        .map(|pair| (u64_at(table, 77 + 16 * pair), u64_at(table, 85 + 16 * pair)))
        .collect::<Vec<_>>();
    assert_eq!(pairs, [(300, sketch), (301, scope), (302, scope)]);

    // The scope's tail names its tracked-results table: four side entries
    // with keys 3 to 6, the body entry with key 7, next key 8.
    let frame = design.record(scope);
    let tracked = reference_at(frame, 349 + 19);
    let results = design.record(tracked);
    assert_eq!(u32_at(results, 21), 5);
    let keys = (0..5)
        .map(|entry| u64_at(results, 25 + 19 * entry + 11))
        .collect::<Vec<_>>();
    assert_eq!(keys, [3, 4, 5, 6, 7]);
    assert_eq!(u64_at(results, 25 + 19 * 5), 8);
    let body_entry = design.record(reference_at(results, 25 + 19 * 4));
    assert_eq!(u32_at(body_entry, 106), 4);
    assert_eq!(&body_entry[110..114], b"3456");
    // The tail: ordinal 1, preceding state 2; the current state 3 before
    // the family name.
    assert_eq!(u32_at(frame, 349), 1);
    assert_eq!(u32_at(frame, 349 + 31), 2);
    assert_eq!(u32_at(frame, 327), 3);

    let context = design.record(198);
    let mut at = 21 + 4 + 11 * usize::try_from(u32_at(context, 21)).expect("count");
    let counters = u32_at(context, at);
    at += 4;
    let mut names = Vec::new();
    for _ in 0..counters {
        let length = usize::try_from(u32_at(context, at)).expect("length");
        names.push((
            String::from_utf8(context[at + 4..at + 4 + length].to_vec()).expect("ASCII"),
            u64_at(context, at + 4 + length),
        ));
        at += 4 + length + 8;
    }
    assert_eq!(
        names,
        [
            ("DcExtrudeFeatureMetaType".to_owned(), 1),
            ("DcSketchMetaType".to_owned(), 1)
        ]
    );
    assert_eq!(u64_at(context, at), 2);
}

#[test]
fn a_symmetric_extrude_is_written_in_the_base_feature() {
    let span = Span::Symmetric {
        amount: Dimension::Parameter(parameter_id("d1")),
        measure: Some(SymmetricMeasure::Total),
    };
    let (report, decoded) = write(&sketch_and_extrude(span, false));
    assert_eq!(kinds(&decoded), ["sketch", "base"]);
    let missing = loss(&report, EVALUATION_MISSING).join("\n");
    assert!(
        missing.contains(EXTRUDE) && missing.contains("its extent is not one-sided blind"),
        "{missing}"
    );
}

#[test]
fn a_sketch_after_the_extrude_follows_it() {
    let (_, decoded, bytes) = write_bytes(&sketch_and_extrude(blind(), true));
    assert_eq!(kinds(&decoded), ["sketch", "extrude", "sketch"]);
    let design = segment(&bytes, "FusionDesignSegmentType1");
    let timeline = design.record(207);
    assert_eq!(u32_at(timeline, 32), 3);
    let scopes = (0..3)
        .map(|item| reference_at(timeline, 36 + 11 * item))
        .collect::<Vec<_>>();
    assert!(scopes[0] < scopes[1] && scopes[1] < scopes[2], "{scopes:?}");
    let table = design.record(203);
    let pairs_at = 25 + 20 + 28 + 20;
    assert_eq!(u32_at(table, pairs_at), 4);
    let pairs = (0..4)
        .map(|pair| {
            let at = pairs_at + 4 + 16 * pair;
            (u64_at(table, at), u64_at(table, at + 8))
        })
        .collect::<Vec<_>>();
    assert_eq!(
        pairs,
        [
            (300, scopes[0]),
            (301, scopes[1]),
            (302, scopes[1]),
            (303, scopes[2])
        ]
    );
}

/// A planned sketch on `XY` of `lines`, each from its first to its second
/// point, in cm.
fn planned<'a>(
    id: &'a FeatureId,
    sketch: &'a SketchId,
    lines: &[([f64; 2], [f64; 2])],
) -> PlannedSketch<'a> {
    let mut points = Vec::new();
    let mut curves = Vec::new();
    for (start, end) in lines {
        points.extend([*start, *end]);
        curves.push(PlannedCurve {
            geometry: CurveGeometry::Line {
                start: points.len() - 2,
                end: points.len() - 1,
            },
            construction: false,
        });
    }
    PlannedSketch {
        feature: id,
        sketch,
        plane: OriginPlane::Xy,
        mirrored: false,
        points,
        curves,
        relations: Vec::new(),
        candidates: Vec::new(),
        dimensions: Vec::new(),
    }
}

fn entry(curve: usize, reversed: bool) -> LoopEntry {
    LoopEntry { curve, reversed }
}

/// The one loop of `sketch`'s profile.
fn ring(sketch: &PlannedSketch<'_>, tolerance: f64) -> Result<Vec<LoopEntry>, &'static str> {
    match profile(sketch, tolerance)?.as_slice() {
        [ProfileLoop {
            exterior: true,
            entries,
        }] => Ok(entries.clone()),
        other => panic!("one exterior loop: {other:?}"),
    }
}

#[test]
fn the_walk_starts_at_the_last_line_and_runs_counterclockwise() {
    let id = feature_id("walk");
    let sketch = sketch_id("walk");
    let (a, b, c, d) = ([0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]);
    let counterclockwise = planned(&id, &sketch, &[(a, b), (b, c), (c, d), (d, a)]);
    assert_eq!(
        ring(&counterclockwise, TOLERANCE),
        Ok(vec![
            entry(3, false),
            entry(0, false),
            entry(1, false),
            entry(2, false)
        ])
    );
    // The last line drawn against the ring: token 3 is the line after it.
    let last_reversed = planned(&id, &sketch, &[(a, b), (b, c), (c, d), (a, d)]);
    assert_eq!(
        ring(&last_reversed, TOLERANCE),
        Ok(vec![
            entry(0, false),
            entry(1, false),
            entry(2, false),
            entry(3, true)
        ])
    );
    // Drawn clockwise: the ring runs every line from its end.
    let clockwise = planned(&id, &sketch, &[(a, d), (d, c), (c, b), (b, a)]);
    assert_eq!(
        ring(&clockwise, TOLERANCE),
        Ok(vec![
            entry(2, true),
            entry(1, true),
            entry(0, true),
            entry(3, true)
        ])
    );
    let open = planned(&id, &sketch, &[(a, b), (b, c), (c, d)]);
    assert!(ring(&open, TOLERANCE).is_err());
    let circle = PlannedSketch {
        feature: &id,
        sketch: &sketch,
        plane: OriginPlane::Xy,
        mirrored: false,
        points: vec![[0.0, 0.0]],
        curves: vec![PlannedCurve {
            geometry: CurveGeometry::Circle {
                center: 0,
                radius: 1.0,
            },
            construction: false,
        }],
        relations: Vec::new(),
        candidates: Vec::new(),
        dimensions: Vec::new(),
    };
    assert_eq!(ring(&circle, TOLERANCE), Ok(vec![entry(0, false)]));
}

#[test]
fn a_failure_in_the_extrude_records_keeps_the_sketches_and_the_base_feature() {
    super::super::FAIL_NEXT_EXTRUDE.with(|fail| fail.set(true));
    let (report, decoded) = write(&sketch_and_extrude(blind(), false));
    assert_eq!(kinds(&decoded), ["sketch", "base"], "{:#?}", report.losses);
    let missing = loss(&report, EVALUATION_MISSING).join("\n");
    assert!(
        missing.contains(&format!(
            "{EXTRUDE} (the Base Feature holds it: the writer cannot write its records (not implemented yet: a test failure of the Extrude records)"
        )),
        "{missing}"
    );
}
