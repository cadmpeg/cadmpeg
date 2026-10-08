// SPDX-License-Identifier: Apache-2.0
//! Legacy field positions and coordinate conversion.

use crate::brep::{decode_with_header, DecodePurpose};
use crate::sab::Token;

#[test]
fn attached_terminators_preserve_legacy_record_indices_and_topology() {
    let source = b"105 0 1 0\n\
body $1 $2 $-1 $-1#\n\
opaque-attrib $-1 $-1 $-1 $0 4 name 4 blue#\n\
lump $-1 $-1 $3 $0#\n\
shell $-1 $-1 $-1 $4 $2#\n\
face $-1 $-1 $-1 $3 $-1 $5 0 0#\n\
plane-surface $-1 0 0 0 0 0 1 1 0 0 0#\nEnd-of-ACIS-data\n";
    crate::test_support::with_service_context(source, |ctx| {
        let parsed = crate::sat::parse(ctx, source).unwrap();
        assert_eq!(parsed.records.len(), 6);
        assert_eq!(parsed.records[3].head(), "shell");
        assert_eq!(parsed.records[4].ref_at(5), Some(3));
        let header = parsed.header.as_kernel_header(ctx).unwrap();
        let brep = decode_with_header(
            ctx,
            &parsed.records,
            source,
            Some(&header),
            "stream",
            crate::asm_format!("sat"),
            DecodePurpose::Model,
        )
        .unwrap();
        assert_eq!(
            (
                brep.bodies.len(),
                brep.regions.len(),
                brep.shells.len(),
                brep.faces.len(),
                brep.surfaces.len()
            ),
            (1, 1, 1, 1, 1)
        );
    })
    .unwrap();
}

#[test]
fn delimiters_are_data_inside_counted_payloads_and_tokens_outside_them() {
    let source = b"700 0 1 0\n1 T 4 ACIS 1 D\n1 0.01 0.001\n\
opaque @5 a#{}b#\nopaque {label}#\n\
point $-1 -1 $-1 1 2 3#\nEnd-of-ACIS-data\n";
    crate::test_support::with_service_context(source, |ctx| {
        let parsed = crate::sat::parse(ctx, source).unwrap();
        assert_eq!(parsed.records.len(), 3);
        assert_eq!(
            parsed.records[0].tokens.as_ref(),
            &[Token::Str("a#{}b".into())]
        );
        assert_eq!(
            parsed.records[1].tokens.as_ref(),
            &[
                Token::SubtypeOpen,
                Token::Ident("label".into()),
                Token::SubtypeClose
            ]
        );
        assert_eq!(parsed.records[2].head(), "point");
    })
    .unwrap();
}

#[test]
fn a_counted_string_requires_its_payload_separator() {
    let source = b"700 0 1 0\n1 T 4 ACIS 1 D\n1 0.01 0.001\nopaque @1#X#\nEnd-of-ACIS-data\n";
    crate::test_support::with_service_context(source, |ctx| {
        assert!(crate::sat::parse(ctx, source).is_err());
    })
    .unwrap();
}

#[test]
fn legacy_wire_fields_keep_the_shell_edge_and_endpoint_graph() {
    let source = b"107 0 1 0\n\
body $-1 $1 $-1 $-1#\n\
lump $-1 $-1 $2 $0#\n\
shell $-1 $-1 $-1 $-1 $3 $1#\n\
wire $-1 $-1 $4 $2 $-1 0#\n\
coedge $-1 $4 $4 $-1 $5 0 $3 $-1#\n\
edge $-1 $6 $7 $4 $8 0#\n\
vertex $-1 $5 $9#\nvertex $-1 $5 $10#\n\
straight-curve $-1 0 0 0 1 0 0 I I#\n\
point $-1 0 0 0#\npoint $-1 5 0 0#\nEnd-of-ACIS-data\n";
    crate::test_support::with_service_context(source, |ctx| {
        let parsed = crate::sat::parse(ctx, source).unwrap();
        assert_eq!(parsed.records[3].ref_at(4), Some(4));
        assert_eq!(parsed.records[3].ref_at(5), Some(2));
        assert_eq!(parsed.records[3].chunk(7), Some(&Token::False));
        let header = parsed.header.as_kernel_header(ctx).unwrap();
        let brep = decode_with_header(
            ctx,
            &parsed.records,
            source,
            Some(&header),
            "stream",
            crate::asm_format!("sat"),
            DecodePurpose::Model,
        )
        .unwrap();
        assert_eq!(
            (
                brep.bodies.len(),
                brep.curves.len(),
                brep.edges.len(),
                brep.vertices.len(),
                brep.points.len()
            ),
            (1, 1, 1, 2, 2)
        );
    })
    .unwrap();
}

#[test]
fn legacy_topology_and_analytic_carriers_use_their_declared_grammar() {
    for version in [102, 103, 104, 105, 106, 107, 200, 201, 300, 400, 500, 600] {
        let r = |index: i64| {
            if version < 103 {
                index.to_string()
            } else {
                format!("${index}")
            }
        };
        let n = r(-1);
        let face_tail = if version < 105 { "0" } else { "forward single" };
        let shell_tail = if version < 107 {
            String::new()
        } else {
            format!("{n} ")
        };
        let plane_tail = if version < 103 {
            ""
        } else if version < 200 {
            " 1 0 0 0"
        } else {
            " 1 0 0 forward_v"
        };
        let bounds = if version < 106 { "" } else { " I I I I" };
        let header = if version < 200 {
            String::new()
        } else {
            "1 T 4 ACIS 1 D\n2 0.01 0.001\n".into()
        };
        let source = format!("{version} 0 1 0\n{header}body {n} {} {n} {n} #\nlump {n} {n} {} {} #\nshell {n} {n} {n} {} {shell_tail}{} #\nface {n} {n} {n} {} {n} {} {face_tail} #\nplane-surface {n} 30 40 50 0 0 1{plane_tail}{bounds} #\nEnd-of-ACIS-data\n", r(1), r(2), r(0), r(3), r(1), r(2), r(4));
        crate::test_support::with_service_context(source.as_bytes(), |ctx| {
            let parsed = crate::sat::parse(ctx, source.as_bytes()).unwrap();
            let header = parsed.header.as_kernel_header(ctx).unwrap();
            let brep = decode_with_header(
                ctx,
                &parsed.records,
                source.as_bytes(),
                Some(&header),
                "stream",
                crate::asm_format!("sat"),
                DecodePurpose::Model,
            )
            .unwrap();
            assert_eq!(
                (
                    brep.bodies.len(),
                    brep.regions.len(),
                    brep.shells.len(),
                    brep.faces.len(),
                    brep.surfaces.len()
                ),
                (1, 1, 1, 1, 1),
                "save format {version}"
            );
            let expected = if version < 200 {
                [3.0, 4.0, 5.0]
            } else {
                [6.0, 8.0, 10.0]
            };
            assert_eq!(
                parsed.records[4].chunk(3),
                Some(&Token::Position(expected)),
                "save format {version}"
            );
        })
        .unwrap();
    }
}

#[test]
fn old_edges_omit_parameters_instead_of_inventing_an_interval() {
    for version in [106, 200, 400, 500, 600] {
        let header = if version < 200 {
            ""
        } else {
            "1 T 4 ACIS 1 D\n1 0.01 0.001\n"
        };
        let fields = if version < 500 {
            "$1 $2 $3 $4"
        } else {
            "$1 2.0 $2 9.0 $3 $4"
        };
        let sense = if version < 200 { "0" } else { "forward" };
        let continuity = if version >= 600 { " @7 unknown" } else { "" };
        let source = format!(
            "{version} 0 1 0\n{header}edge $-1 {fields} {sense}{continuity} #\nEnd-of-ACIS-data\n"
        );
        crate::test_support::with_service_context(source.as_bytes(), |ctx| {
            let parsed = crate::sat::parse(ctx, source.as_bytes()).unwrap();
            let edge = &parsed.records[0];
            assert_eq!(edge.ref_at(3), Some(1));
            assert_eq!(edge.ref_at(5), Some(2));
            assert_eq!(edge.ref_at(7), Some(3));
            assert_eq!(edge.ref_at(8), Some(4));
            if version < 500 {
                assert_eq!(edge.chunk(4), Some(&Token::False));
                assert_eq!(edge.chunk(6), Some(&Token::False));
            } else {
                assert_eq!(edge.chunk(4), Some(&Token::Double(2.0)));
                assert_eq!(edge.chunk(6), Some(&Token::Double(9.0)));
            }
        })
        .unwrap();
    }
}

#[test]
fn legacy_edge_continuity_uses_a_bare_byte_count() {
    for version in [500, 600] {
        for (continuity, expected) in [("7 unknown", "unknown"), ("0", "")] {
            let source = format!("{version} 0 1 0\n1 T 4 ACIS 1 D\n1 0.01 0.001\nedge $-1 $1 2 $2 9 $3 $4 forward {continuity} #\nEnd-of-ACIS-data\n");
            crate::test_support::with_service_context(source.as_bytes(), |ctx| {
                let parsed = crate::sat::parse(ctx, source.as_bytes()).unwrap();
                let edge = &parsed.records[0];
                assert_eq!(edge.ref_at(3), Some(1));
                assert_eq!(edge.ref_at(5), Some(2));
                assert_eq!(edge.chunk(10), Some(&Token::Str(expected.into())));
            })
            .unwrap();
        }
    }
}

#[test]
fn coedge_numeric_sense_gate_includes_tolerant_coedges() {
    for version in [201, 202] {
        let sense = if version < 202 { "1" } else { "reversed" };
        let source = format!(
            "{version} 0 1 0\n1 T 4 ACIS 1 D\n1 0.01 0.001\n\
coedge $-1 $1 $2 $3 $4 {sense} $5 $6 #\n\
tcoedge $-1 $1 $2 $3 $4 {sense} $5 $6 2 9 #\nEnd-of-ACIS-data\n"
        );
        crate::test_support::with_service_context(source.as_bytes(), |ctx| {
            let parsed = crate::sat::parse(ctx, source.as_bytes()).unwrap();
            for record in &parsed.records {
                assert_eq!(record.ref_at(6), Some(4));
                assert_eq!(record.chunk(7), Some(&Token::True));
                assert_eq!(record.ref_at(8), Some(5));
                assert_eq!(record.ref_at(9), Some(6));
            }
        })
        .unwrap();
    }
}

#[test]
fn numeric_coedge_senses_remain_readable_in_later_legacy_saves() {
    let source = b"400 0 1 0\n1 T 4 ACIS 1 D\n1 0.01 0.001\n\
coedge $-1 $1 $2 $3 $4 0 $5 $6#\n\
tcoedge $-1 $1 $2 $3 $4 1 $5 $6 2 9#\nEnd-of-ACIS-data\n";
    crate::test_support::with_service_context(source, |ctx| {
        let parsed = crate::sat::parse(ctx, source).unwrap();
        assert_eq!(parsed.records[0].chunk(7), Some(&Token::False));
        assert_eq!(parsed.records[1].chunk(7), Some(&Token::True));
        for record in &parsed.records {
            assert_eq!(record.ref_at(6), Some(4));
            assert_eq!(record.ref_at(8), Some(5));
            assert_eq!(record.ref_at(9), Some(6));
        }
    })
    .unwrap();
}

#[test]
fn legacy_solved_spline_blocks_have_no_cache_form_field() {
    for version in [105, 300, 600] {
        let header = if version < 200 {
            ""
        } else {
            "1 T 4 ACIS 1 D\n2 0.01 0.001\n"
        };
        let sense = if version < 200 { "0" } else { "forward" };
        let bounds = if version < 106 { "" } else { " I I I I" };
        let curve_bounds = if version < 106 { "" } else { " I I" };
        let source = format!("{version} 0 1 0\n{header}\
body $-1 $1 $-1 $-1 #\n\
lump $-1 $-1 $2 $0 #\n\
shell $-1 $-1 $-1 $3 {}$1 #\n\
face $-1 $-1 $-1 $2 $-1 $4 {sense} single #\n\
spline-surface $-1 {sense} {{ exactsur nubs 1 1 open open none none 2 2 0 1 1 1 0 1 1 1 0 0 0 10 0 0 0 10 0 10 10 0 0 }}{bounds} #\n\
intcurve-curve $-1 {sense} {{ exactcur nubs 1 open 2 0 1 1 1 0 0 0 10 0 0 0 null_surface null_surface nullbs nullbs }}{curve_bounds} #\n\
End-of-ACIS-data\n", if version < 107 { "" } else { "$-1 " });
        crate::test_support::with_service_context(source.as_bytes(), |ctx| {
            let parsed = crate::sat::parse(ctx, source.as_bytes()).unwrap();
            let header = parsed.header.as_kernel_header(ctx).unwrap();
            let brep = decode_with_header(
                ctx,
                &parsed.records,
                source.as_bytes(),
                Some(&header),
                "stream",
                crate::asm_format!("sat"),
                DecodePurpose::Model,
            )
            .unwrap();
            assert_eq!(brep.stats.nurbs_surfaces, 1, "save format {version}");
            let table =
                crate::nurbs::toks::SubtypeTable::from_records(ctx, &parsed.records).unwrap();
            let decoded = crate::nurbs::proc_curve::procedural_curve_resolving_refs(
                ctx,
                &parsed.records[5].tokens,
                &table,
            )
            .unwrap()
            .unwrap();
            assert_eq!(decoded.curve.degree(), 1);
            let end = if version < 200 { 10.0 } else { 20.0 };
            assert_eq!(decoded.curve.control_points()[1].get().x, end);
        })
        .unwrap();
    }
}
