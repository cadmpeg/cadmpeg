// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::topology::Unstated;
use cadmpeg_ir::unknown::part::{Cause, DecodeParts};

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
/// the cause of its give-up and the reason its loss states, and the ids it
/// drops and marks.
struct Expected {
    kind: &'static str,
    line: &'static str,
    given_up: Cause,
    cause: &'static str,
    dropped: &'static [&'static str],
    marked: &'static [&'static str],
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
                ctx.give_up_part(part, record.cause, record.error())
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
                part.given_up.kind(),
                part.given_up.severity(),
                part.cause,
                if part.marked.is_empty() {
                    format!("; dropped=[{}]", part.dropped.join(", "))
                } else {
                    format!(
                        "; dropped=[{}]; marked=[{}]",
                        part.dropped.join(", "),
                        part.marked.join(", ")
                    )
                },
            )
        })
        .collect::<Vec<_>>();
    expected.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(losses.len(), expected.len(), "{losses:#?}");
    for (loss, (head, code, severity, cause, tail)) in losses.iter().zip(&expected) {
        assert_eq!(&loss.code, code);
        assert_eq!(&loss.severity, severity);
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
            given_up: Cause::Unreadable,
            cause: "the face record's surface reference names no record",
            dropped: &[
                "f3d:brep:entity#11",
                "f3d:brep:entity#4",
                "f3d:brep:entity#6",
            ],
            marked: &["f3d:brep:entity#2"],
        }],
    );
}

/// The cone's face 3 stands on a surface reference that names no record.
/// Its edge 14, vertex 16 and point 19 are reached only through it; edge 13
/// and its vertex, point and curve stay with face 4, and shell 2 states
/// unstated members.
#[test]
fn a_face_whose_surface_names_no_record_drops_what_only_it_reaches_and_marks_its_shell() {
    let face = "face $-1 -1 $-1 $4 $5 $2 $-1 $99 forward single #";
    let source = edited("face $-1 -1 $-1 $4 $5 $2 $-1 $7 forward single #", face);
    assert_given_up(
        &source,
        &[Expected {
            kind: "face",
            line: face,
            given_up: Cause::Unreadable,
            cause: "the face record's surface reference names no record",
            dropped: &[
                "f3d:brep:entity#10",
                "f3d:brep:entity#12",
                "f3d:brep:entity#14",
                "f3d:brep:entity#16",
                "f3d:brep:entity#19",
                "f3d:brep:entity#3",
                "f3d:brep:entity#5",
                "f3d:brep:entity#9",
            ],
            marked: &["f3d:brep:entity#2"],
        }],
    );
    let brep = decode(&source, DecodePurpose::Model);
    assert_eq!(brep.shells.len(), 1);
    assert_eq!(brep.shells[0].id.as_str(), "f3d:brep:entity#2");
    assert_eq!(brep.shells[0].unstated, Some(Unstated::Members));
}

/// A single-face sheet whose face stands on a surface reference that names
/// no record, with `vertex` as its record 7.
const SHEET_FACE: &str = "face $-1 -1 $-1 $-1 $4 $2 $-1 $99 forward single #";

fn sheet(vertex: &str) -> String {
    format!(
        "23200 0 1 0 \n\
14 cadmpeg-tests 4 ACIS 0 \n\
1 1e-06 1e-10 \n\
body $-1 -1 $-1 $1 $-1 $-1 #\n\
lump $-1 -1 $-1 $-1 $2 $0 #\n\
shell $-1 -1 $-1 $-1 $-1 $3 $-1 $1 #\n\
{SHEET_FACE}\n\
loop $-1 -1 $-1 $-1 $5 $3 #\n\
coedge $-1 -1 $-1 $5 $5 $-1 $6 forward $4 0 $-1 #\n\
edge $-1 -1 $-1 $7 0 $7 6.283185307179586 $5 $8 forward @7 unknown #\n\
{vertex}\n\
ellipse-curve $-1 -1 $-1 0 0 0 0 0 1 5 0 0 1 I I #\n\
point $-1 -1 $-1 5 0 0 #\n\
End-of-ASM-data \n"
    )
}

fn assert_sheet_face_given_up(source: &str, dropped: &'static [&'static str]) {
    assert_given_up(
        source,
        &[Expected {
            kind: "face",
            line: SHEET_FACE,
            given_up: Cause::Unreadable,
            cause: "the face record's surface reference names no record",
            dropped,
            marked: &[],
        }],
    );
}

/// The sheet's loop, coedge, edge, curve, vertex and point drop with its
/// face, and its shell, which keeps no face, leaves the B-rep.
#[test]
fn a_sheet_face_on_a_dangling_surface_drops_its_edges_vertices_and_points() {
    let source = sheet("vertex $-1 -1 $-1 $6 0 $9 #");
    assert_sheet_face_given_up(
        &source,
        &[
            "f3d:brep:entity#3",
            "f3d:brep:entity#4",
            "f3d:brep:entity#5",
            "f3d:brep:entity#6",
            "f3d:brep:entity#7",
            "f3d:brep:entity#8",
            "f3d:brep:entity#9",
        ],
    );
    let brep = decode(&source, DecodePurpose::Model);
    assert!(brep.shells.is_empty());
    assert!(brep.edges.is_empty() && brep.vertices.is_empty() && brep.points.is_empty());
}

/// A tolerant vertex with a negative tolerance stays as a retained record,
/// so the face's part does not name it.
#[test]
fn a_sheet_face_on_a_dangling_surface_names_no_retained_record() {
    let source = sheet("tvertex $-1 -1 $-1 $6 0 $9 $-1 0 -0.5 #");
    let brep = decode(&source, DecodePurpose::Model);
    assert!(
        brep.unknowns
            .iter()
            .any(|record| record.id().as_str().ends_with("tvertex#7")),
        "{:?}",
        brep.unknowns
            .iter()
            .map(cadmpeg_ir::UnknownRecord::id)
            .collect::<Vec<_>>()
    );
    assert_sheet_face_given_up(
        &source,
        &[
            "f3d:brep:entity#3",
            "f3d:brep:entity#4",
            "f3d:brep:entity#5",
            "f3d:brep:entity#6",
            "f3d:brep:entity#8",
            "f3d:brep:entity#9",
        ],
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
            given_up: Cause::Unreadable,
            cause: "the edge record's vertex reference names no vertex record",
            dropped: &["f3d:brep:entity#14"],
            marked: &[],
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
                given_up: Cause::Unreadable,
                cause: "the coedge record's loop is no loop of a kept face",
                dropped: &["f3d:brep:entity#12"],
                marked: &[],
            },
            Expected {
                kind: "loop",
                line: "loop $-1 -1 $-1 $-1 $12 $3 #",
                given_up: Cause::Undecoded,
                cause: "the loop record's coedge ring holds no coedge the loop owns",
                dropped: &["f3d:brep:entity#9"],
                marked: &[],
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
            given_up: Cause::Unreadable,
            cause: "the vertex record states no point",
            dropped: &["f3d:brep:entity#16"],
            marked: &[],
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
            given_up: Cause::Unreadable,
            cause: "the point record states no position",
            dropped: &["f3d:brep:entity#19"],
            marked: &[],
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
            given_up: Cause::Unreadable,
            cause: "the region record names no body",
            dropped: &["f3d:brep:entity#1"],
            marked: &[],
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
