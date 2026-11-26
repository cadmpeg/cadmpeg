// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::report::Severity;
use cadmpeg_ir::unknown::part::{record_unreadable_kind, DecodeParts};

use super::super::tests::CONE_WITH_POINT_APEX_SAT;
use super::super::{decode_with_header, AsmBrep, DecodePurpose};
use crate::ids::IdFormat;

const FORMAT: IdFormat = crate::asm_format!("f3d");

/// The closed cone of the B-rep tests: face 3 (cone) and face 4 (plane) on
/// shell 2, loops 5, 6 and 9, coedges 10 to 12, edges 13 and 14, vertices 15
/// and 16, points 18 and 19.
fn cone() -> &'static str {
    std::str::from_utf8(CONE_WITH_POINT_APEX_SAT).expect("ASCII fixture")
}

/// A part the decode must give up: its kind, the record line it frames,
/// the cause its loss states, and the ids it drops.
struct Expected {
    kind: &'static str,
    line: &'static str,
    cause: &'static str,
    dropped: &'static [&'static str],
}

fn decode(source: &str, purpose: DecodePurpose) -> AsmBrep {
    decode_and_give_up(source, purpose).0
}

/// The decode of `source`, and the loss of each withheld record given up
/// as a part over its bytes, as an embedding codec gives it up.
fn decode_and_give_up(source: &str, purpose: DecodePurpose) -> (AsmBrep, Vec<LossNote>) {
    let bytes = source.as_bytes();
    crate::test_support::with_service_context(bytes, |ctx| {
        let parsed = crate::sat::parse(ctx, bytes).expect("synthetic SAT stream");
        let header = parsed.header.as_kernel_header();
        let brep = decode_with_header(
            ctx,
            &parsed.records,
            bytes,
            Some(&header),
            "stream",
            FORMAT,
            purpose,
        )
        .expect("synthetic cone decodes");
        let losses = brep
            .withheld
            .iter()
            .map(|record| {
                let part = record.part("f3d", bytes).expect("record span in stream");
                ctx.give_up_part(part, record.error())
                    .expect("part given up")
                    .loss()
            })
            .collect();
        (brep, losses)
    })
    .expect("service decode context")
}

fn edited(from: &str, to: &str) -> String {
    assert_eq!(cone().matches(from).count(), 1, "edit target {from:?}");
    cone().replacen(from, to, 1)
}

fn assert_given_up(source: &str, expected: &[Expected]) {
    let (_, mut losses) = decode_and_give_up(source, DecodePurpose::Model);
    losses.sort_by_key(|loss| loss.message.clone());
    let mut expected = expected
        .iter()
        .map(|part| {
            let offset = source
                .find(&format!("{}\n", part.line))
                .unwrap_or_else(|| panic!("fixture holds {:?}", part.line));
            (
                format!("part {} stream=\"\" offset={offset} ", part.kind),
                part.cause,
                format!("; dropped=[{}]", part.dropped.join(", ")),
            )
        })
        .collect::<Vec<_>>();
    expected.sort();
    assert_eq!(losses.len(), expected.len(), "{losses:#?}");
    for (loss, (head, cause, tail)) in losses.iter().zip(&expected) {
        assert_eq!(loss.code, record_unreadable_kind());
        assert_eq!(loss.severity, Severity::Error);
        assert!(loss.message.starts_with(head.as_str()), "{}", loss.message);
        assert!(loss.message.contains(cause), "{}", loss.message);
        assert!(loss.message.ends_with(tail.as_str()), "{}", loss.message);
    }
}

#[test]
fn a_complete_solid_gives_up_no_record() {
    assert!(decode(cone(), DecodePurpose::Model).withheld.is_empty());
}

#[test]
fn a_face_whose_surface_names_no_record_drops_its_loops_and_coedges() {
    let face = "face $-1 -1 $-1 $-1 $6 $2 $-1 $99 forward single #";
    assert_given_up(
        &edited("face $-1 -1 $-1 $-1 $6 $2 $-1 $8 forward single #", face),
        &[Expected {
            kind: "face",
            line: face,
            cause: "the face record's surface reference names no record",
            dropped: &[
                "f3d:brep:entity#11",
                "f3d:brep:entity#4",
                "f3d:brep:entity#6",
            ],
        }],
    );
}

#[test]
fn an_edge_whose_vertex_reference_names_a_point_is_given_up() {
    let edge = "edge $-1 -1 $-1 $18 1 $18 0 $12 $-1 forward @7 unknown #";
    assert_given_up(
        &edited(
            "edge $-1 -1 $-1 $16 1 $16 0 $12 $-1 forward @7 unknown #",
            edge,
        ),
        &[Expected {
            kind: "edge",
            line: edge,
            cause: "the edge record's vertex reference names no vertex record",
            dropped: &["f3d:brep:entity#14"],
        }],
    );
}

#[test]
fn a_coedge_on_no_kept_loop_gives_up_it_and_its_emptied_loop() {
    let coedge = "coedge $-1 -1 $-1 $12 $12 $-1 $14 forward $99 0 $-1 #";
    assert_given_up(
        &edited(
            "coedge $-1 -1 $-1 $12 $12 $-1 $14 forward $9 0 $-1 #",
            coedge,
        ),
        &[
            Expected {
                kind: "coedge",
                line: coedge,
                cause: "the coedge record's loop is no loop of a kept face",
                dropped: &["f3d:brep:entity#12"],
            },
            Expected {
                kind: "loop",
                line: "loop $-1 -1 $-1 $-1 $12 $3 #",
                cause: "the loop record's coedge ring holds no coedge the loop owns",
                dropped: &["f3d:brep:entity#9"],
            },
        ],
    );
}

#[test]
fn a_vertex_with_no_point_is_given_up() {
    let vertex = "vertex $-1 -1 $-1 $14 0 $-1 #";
    assert_given_up(
        &edited("vertex $-1 -1 $-1 $14 0 $19 #", vertex),
        &[Expected {
            kind: "vertex",
            line: vertex,
            cause: "the vertex record states no point",
            dropped: &["f3d:brep:entity#16"],
        }],
    );
}

#[test]
fn a_point_with_no_position_is_given_up() {
    let point = "point $-1 -1 $-1 #";
    assert_given_up(
        &edited("point $-1 -1 $-1 0 0 10 #", point),
        &[Expected {
            kind: "point",
            line: point,
            cause: "the point record states no position",
            dropped: &["f3d:brep:entity#19"],
        }],
    );
}

#[test]
fn a_region_with_no_body_is_given_up() {
    let lump = "lump $-1 -1 $-1 $-1 $2 $-1 #";
    assert_given_up(
        &edited("lump $-1 -1 $-1 $-1 $2 $0 #", lump),
        &[Expected {
            kind: "region",
            line: lump,
            cause: "the region record names no body",
            dropped: &["f3d:brep:entity#1"],
        }],
    );
}

#[test]
fn a_face_on_no_shell_stays_retained() {
    let source = edited(
        "face $-1 -1 $-1 $-1 $6 $2 $-1 $8 forward single #",
        "face $-1 -1 $-1 $-1 $6 $-1 $-1 $8 forward single #",
    );
    let brep = decode(&source, DecodePurpose::Model);
    assert!(brep.withheld.is_empty());
    assert!(brep.unknowns.len() > decode(cone(), DecodePurpose::Model).unknowns.len());
}

#[test]
fn a_history_decode_gives_up_no_record() {
    let source = edited(
        "face $-1 -1 $-1 $-1 $6 $2 $-1 $8 forward single #",
        "face $-1 -1 $-1 $-1 $6 $2 $-1 $99 forward single #",
    );
    assert!(decode(&source, DecodePurpose::History).withheld.is_empty());
}
