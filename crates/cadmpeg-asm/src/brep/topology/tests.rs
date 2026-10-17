// SPDX-License-Identifier: Apache-2.0

use super::{keep_faces_and_carriers, walk_reachable_topology, TopologyContext};
use crate::brep::{AsmBrep, Carriers, DecodePurpose, Reachable};
use crate::kernel_header::RefWidth;
use crate::nurbs;
use crate::nurbs::proc_surface::DecodedProceduralSurfaceDefinition;
use crate::sab::{Record, Token};
use cadmpeg_ir::geometry::RevisionCacheForm;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use std::collections::HashSet;

fn ref_record(index: usize, name: &str, refs: &[i64]) -> Record {
    Record {
        index,
        name: name.into(),
        tokens: refs.iter().copied().map(Token::Ref).collect(),
        offset: 0,
        len: 0,
    }
}

#[test]
fn chains_list_only_children_that_are_emitted() {
    let records = [
        ref_record(0, "body", &[-1, -1, -1, 1]),
        ref_record(1, "lump", &[-1, -1, -1, 3, 2, 0]),
        ref_record(2, "shell", &[-1, -1, -1, 4, -1, -1, -1, 1]),
        ref_record(3, "lump", &[-1, -1, -1, -1, -1, 9]),
        ref_record(4, "face", &[-1, -1, -1, -1]),
    ];
    let by_index = records
        .iter()
        .map(|record| {
            (
                i64::try_from(record.index).expect("test value fits"),
                record,
            )
        })
        .collect();
    let format = crate::asm_format!("f3d");
    {
        let regions = super::region_chain(&records[0], &by_index, format);
        assert_eq!(
            regions.len(),
            1,
            "a lump owned by another body ends the chain"
        );
        let shells = super::shell_chain(
            &records[1],
            &by_index,
            &std::collections::HashSet::from([2]),
            format,
        );
        assert_eq!(shells.len(), 1, "a face record is not a shell");
        let missing = ref_record(5, "body", &[-1, -1, -1, 7]);
        assert!(super::region_chain(&missing, &by_index, format).is_empty());
    };
}

fn index_records(records: &[Record]) -> std::collections::HashMap<i64, &Record> {
    records
        .iter()
        .map(|record| {
            (
                i64::try_from(record.index).expect("test value fits"),
                record,
            )
        })
        .collect()
}

fn as_ref(index: usize) -> i64 {
    i64::try_from(index).expect("test value fits")
}

#[test]
fn faces_sharing_one_loop_walk_and_list_its_ring_once() {
    // Walking the ring once per face would take 2^24 steps.
    const FACES: usize = 1 << 12;
    const COEDGES: usize = 1 << 12;
    let shared = FACES;
    let first_coedge = FACES + 1;
    let mut records = (0..FACES)
        .map(|face| ref_record(face, "face", &[-1, -1, -1, -1, as_ref(shared), 7]))
        .collect::<Vec<_>>();
    records.push(ref_record(
        shared,
        "loop",
        &[-1, -1, -1, -1, as_ref(first_coedge), 0],
    ));
    records.extend((0..COEDGES).map(|coedge| {
        let next = first_coedge + (coedge + 1) % COEDGES;
        let fields = [-1, -1, -1, as_ref(next), -1, -1, -1, -1, as_ref(shared)];
        ref_record(first_coedge + coedge, "coedge", &fields)
    }));
    let by_index = index_records(&records);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::default(),
    )
    .expect("test decode context");
    let table = nurbs::toks::SubtypeTable::from_records(&records);
    let mut reach = Reachable {
        faces: (0..FACES).map(as_ref).collect(),
        ..Reachable::default()
    };
    let format = crate::asm_format!("f3d");
    walk_reachable_topology(
        TopologyContext {
            ctx: &ctx,
            by_index: &by_index,
            token_table: &table,
            purpose: DecodePurpose::Model,
            format,
        },
        &mut AsmBrep::default(),
        &mut Carriers::default(),
        &mut reach,
    )
    .expect("shared loop is within resource limits");
    assert_eq!(reach.loops, HashSet::from([as_ref(shared)]));
    assert_eq!(reach.coedges.len(), COEDGES);

    let owner = super::loop_chain(&records[0], &by_index, &reach.loops, format);
    assert_eq!(owner.len(), 1);
    let listed = (1..FACES)
        .map(|face| super::loop_chain(&records[face], &by_index, &reach.loops, format).len())
        .sum::<usize>();
    assert_eq!(listed, 0, "a loop owned by another face ends the chain");
    let ring = super::ring_coedges(&records[shared], &by_index, &reach.coedges, format);
    assert_eq!(ring.len(), COEDGES);
    let borrowed = ref_record(
        FACES + COEDGES + 1,
        "loop",
        &[-1, -1, -1, -1, as_ref(first_coedge), 1],
    );
    assert!(super::ring_coedges(&borrowed, &by_index, &reach.coedges, format).is_empty());
}

#[test]
fn a_face_chain_lists_only_the_faces_its_shell_owns() {
    let records = [
        ref_record(0, "shell", &[-1, -1, -1, -1, -1, 2]),
        ref_record(1, "shell", &[-1, -1, -1, -1, -1, 2]),
        ref_record(2, "face", &[-1, -1, -1, 3, -1, 0]),
        ref_record(3, "face", &[-1, -1, -1, -1, -1, 0]),
    ];
    let by_index = index_records(&records);
    let kept = HashSet::from([2, 3]);
    let format = crate::asm_format!("f3d");
    assert_eq!(
        super::shell_faces(&records[0], &by_index, &kept, format).len(),
        2
    );
    assert!(super::shell_faces(&records[1], &by_index, &kept, format).is_empty());
}

#[test]
fn deep_subshell_chains_resolve_their_shell_in_linear_time() {
    // Walking upward once per subshell would take 2^31 steps.
    const DEPTH: usize = 1 << 16;
    let mut records = vec![ref_record(0, "shell", &[-1; 8])];
    records.extend(
        (1..=DEPTH)
            .map(|subshell| ref_record(subshell, "subshell", &[-1, -1, -1, as_ref(subshell - 1)])),
    );
    let cycle = DEPTH + 1;
    records.push(ref_record(
        cycle,
        "subshell",
        &[-1, -1, -1, as_ref(cycle + 1)],
    ));
    records.push(ref_record(
        cycle + 1,
        "subshell",
        &[-1, -1, -1, as_ref(cycle)],
    ));
    records.push(ref_record(
        cycle + 2,
        "subshell",
        &[-1, -1, -1, as_ref(cycle)],
    ));
    let by_index = index_records(&records);
    let shells = super::subshell_ancestor_shells(&records, &by_index).unwrap();
    assert_eq!(shells.len(), DEPTH);
    assert!((1..=DEPTH).all(|subshell| shells.get(&as_ref(subshell)) == Some(&0)));
}

fn ident(bytes: &mut Vec<u8>, name: &str) {
    bytes.extend_from_slice(&[0x0d, u8::try_from(name.len()).unwrap()]);
    bytes.extend_from_slice(name.as_bytes());
}

fn integer(bytes: &mut Vec<u8>, tag: u8, value: i64, width: RefWidth) {
    bytes.push(tag);
    match width {
        RefWidth::Four => bytes.extend_from_slice(&i32::try_from(value).unwrap().to_le_bytes()),
        RefWidth::Eight => bytes.extend_from_slice(&value.to_le_bytes()),
    }
}

fn double(bytes: &mut Vec<u8>, value: f64) {
    bytes.push(0x06);
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn triple(bytes: &mut Vec<u8>, tag: u8, values: [f64; 3]) {
    bytes.push(tag);
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
}

#[test]
fn revision_sum_solved_cache_remains_a_nurbs_face_carrier() {
    let asm_decode_arena = cadmpeg_core::decode::DecodeArena::new();
    let (asm_decode_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &asm_decode_arena,
        &cadmpeg_core::decode::DecodePolicy::default(),
    )
    .expect("test decode context");
    for width in [RefWidth::Four, RefWidth::Eight] {
        for tolerance in [0.0, 0.125] {
            let mut bytes = Vec::new();
            ident(&mut bytes, "spline");
            bytes.extend_from_slice(&[0x0b, 0x0f]);
            ident(&mut bytes, "sum_spl_sur");
            integer(&mut bytes, 0x04, 23_100, width);
            for direction in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
                ident(&mut bytes, "straight");
                triple(&mut bytes, 0x13, [0.0; 3]);
                triple(&mut bytes, 0x14, direction);
                bytes.extend_from_slice(&[0x0b, 0x0b]);
            }
            triple(&mut bytes, 0x13, [0.0; 3]);
            integer(&mut bytes, 0x15, 0, width);
            ident(&mut bytes, "nubs");
            for _ in 0..2 {
                integer(&mut bytes, 0x04, 1, width);
            }
            for _ in 0..4 {
                integer(&mut bytes, 0x15, 0, width);
            }
            for _ in 0..2 {
                integer(&mut bytes, 0x04, 2, width);
            }
            for _ in 0..2 {
                for knot in [0.0, 1.0] {
                    double(&mut bytes, knot);
                    integer(&mut bytes, 0x04, 1, width);
                }
            }
            for pole in [
                [0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [1.0, 0.0, 0.0],
                [1.0, 1.0, 0.0],
            ] {
                for coordinate in pole {
                    double(&mut bytes, coordinate);
                }
            }
            double(&mut bytes, tolerance);
            for _ in 0..6 {
                integer(&mut bytes, 0x04, 0, width);
            }
            bytes.extend_from_slice(&[0x0b, 0x10, 0x0b, 0x0b, 0x0b, 0x0b, 0x11]);
            ident(&mut bytes, "face");
            for reference in [-1, -1, -1, -1, -1, 2, -1, 0] {
                integer(&mut bytes, 0x0c, reference, width);
            }
            bytes.push(0x11);
            for (name, references) in [
                ("shell", &[-1, -1, -1, -1, -1, 1, -1, 3][..]),
                ("lump", &[-1, -1, -1, -1, 2, 4][..]),
                ("body", &[-1, -1, -1, 3][..]),
            ] {
                ident(&mut bytes, name);
                for &reference in references {
                    integer(&mut bytes, 0x0c, reference, width);
                }
                bytes.push(0x11);
            }

            let records = crate::test_support::sab::frame(&bytes, 0, bytes.len(), width).unwrap();
            let by_index = records
                .iter()
                .map(|record| {
                    (
                        i64::try_from(record.index).expect("test value fits"),
                        record,
                    )
                })
                .collect();
            let table = nurbs::toks::SubtypeTable::from_records(&records);
            let decoded = nurbs::proc_surface::procedural_surface_resolving_refs(
                &asm_decode_ctx,
                &records[0].tokens,
                &table,
            )
            .transpose()
            .expect("decode the procedural surface")
            .unwrap();
            let DecodedProceduralSurfaceDefinition::Sum {
                revision_form: Some(form),
                ..
            } = decoded.definition()
            else {
                panic!("expected a revision Sum");
            };
            assert!(matches!(form.cache, RevisionCacheForm::SolvedCache { .. }));
            assert_eq!(
                form.cache
                    .fit_tolerance()
                    .map(cadmpeg_ir::geometry::FitTolerance::get),
                Some(tolerance * 10.0)
            );
            assert_eq!(decoded.cache_fit_tolerance(), Some(tolerance * 10.0));
            assert_eq!(decoded.legacy_cache_fit_tolerance(), None);

            let mut out = AsmBrep::default();
            let mut carriers = Carriers::default();
            let mut reach = Reachable::default();
            let format = crate::asm_format!("f3d");
            keep_faces_and_carriers(
                TopologyContext {
                    ctx: &asm_decode_ctx,
                    by_index: &by_index,
                    token_table: &table,
                    purpose: DecodePurpose::Model,
                    format,
                },
                &mut out,
                &records,
                &mut carriers,
                &mut reach,
            )
            .expect("generated identities are valid");
            assert_eq!(out.stats.nurbs_surfaces, 1);
            crate::brep::emit::emit_carrier_records(
                &mut out,
                &records,
                &mut carriers,
                &reach,
                crate::brep::emit::CurveSenseRefs {
                    reversed_curve_refs: &HashSet::new(),
                    forward_curve_refs: &HashSet::new(),
                },
                format,
            )
            .expect("valid carrier fixture");
            assert_eq!(out.surfaces.len(), 1);
            assert!(matches!(
                out.surfaces[0].geometry,
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(_))
            ));
        }
    }
}

#[test]
fn history_pcurve_use_has_no_invented_parameter_interval() {
    let asm_decode_arena = cadmpeg_core::decode::DecodeArena::new();
    let (asm_decode_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &asm_decode_arena,
        &cadmpeg_core::decode::DecodePolicy::default(),
    )
    .expect("test decode context");
    let record = |index, name: &str, fields: &[i64]| Record {
        index,
        name: name.into(),
        tokens: fields.iter().copied().map(Token::Ref).collect(),
        offset: 0,
        len: 0,
    };
    let records = [
        record(0, "face", &[-1, -1, -1, -1, 1]),
        record(1, "loop", &[-1, -1, -1, -1, 2]),
        record(2, "coedge", &[-1, -1, -1, 2, 2, -1, 3, -1, 1, 4]),
        record(3, "edge", &[-1; 9]),
        record(4, "pcurve", &[]),
    ];
    let by_index = records
        .iter()
        .map(|r| (i64::try_from(r.index).expect("test value fits"), r))
        .collect();
    let table = nurbs::toks::SubtypeTable::from_records(&records);
    let mut carriers = Carriers::default();
    let mut reach = Reachable {
        faces: HashSet::from([0]),
        ..Reachable::default()
    };
    let mut out = AsmBrep::default();
    walk_reachable_topology(
        TopologyContext {
            ctx: &asm_decode_ctx,
            by_index: &by_index,
            token_table: &table,
            purpose: DecodePurpose::History,
            format: crate::asm_format!("f3d"),
        },
        &mut out,
        &mut carriers,
        &mut reach,
    )
    .expect("history topology is within resource limits");
    super::super::emit::emit_coedges(
        &mut out,
        &records,
        crate::brep::emit::CoedgeDecodeInputs {
            token_table: &table,
            save_format_major: None,
        },
        &carriers,
        &reach,
        crate::asm_format!("f3d"),
    )
    .unwrap();
    assert_eq!(out.coedges.len(), 1);
    assert_eq!(out.coedges[0].pcurves.len(), 1);
    assert_eq!(out.coedges[0].pcurves[0].parameter_range, None);
}
