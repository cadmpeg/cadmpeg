// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::kernel_header::RefWidth;
use crate::nurbs::proc_surface::DecodedProceduralSurfaceDefinition;
use cadmpeg_ir::geometry::RevisionCacheForm;

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
            for reference in [-1, -1, -1, -1, -1, -1, -1, 0] {
                integer(&mut bytes, 0x0c, reference, width);
            }
            bytes.push(0x11);

            let records = crate::sab::frame(&bytes, 0, bytes.len(), width).unwrap();
            let by_index = records
                .iter()
                .map(|record| (record.index as i64, record))
                .collect();
            let table = nurbs::toks::SubtypeTable::from_records(&records);
            let decoded =
                nurbs::proc_surface::procedural_surface_resolving_refs(&records[0].tokens, &table)
                    .unwrap();
            let DecodedProceduralSurfaceDefinition::Sum {
                revision_form: Some(form),
                ..
            } = decoded.definition()
            else {
                panic!("expected a revision Sum");
            };
            assert!(matches!(form.cache, RevisionCacheForm::SolvedCache { .. }));
            assert_eq!(form.cache.fit_tolerance(), Some(tolerance * 10.0));
            assert_eq!(decoded.cache_fit_tolerance(), Some(tolerance * 10.0));
            assert_eq!(decoded.legacy_cache_fit_tolerance(), None);

            let mut out = AsmBrep::default();
            let mut carriers = Carriers::default();
            let mut reach = Reachable::default();
            let format = IdFormat("f3d");
            keep_faces_and_carriers(
                &mut out,
                &records,
                &by_index,
                &table,
                &mut carriers,
                &mut reach,
                DecodePurpose::Model,
                format,
            );
            assert_eq!(out.stats.nurbs_surfaces, 1);
            crate::brep::emit::emit_carrier_records(
                &mut out,
                &records,
                &mut carriers,
                &reach,
                &HashSet::new(),
                &HashSet::new(),
                format,
            )
            .expect("valid carrier fixture");
            assert_eq!(out.surfaces.len(), 1);
            assert!(matches!(
                out.surfaces[0].geometry,
                SurfaceGeometry::Nurbs(_)
            ));
        }
    }
}

#[test]
fn history_pcurve_use_has_no_invented_parameter_interval() {
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
    let by_index = records.iter().map(|r| (r.index as i64, r)).collect();
    let table = nurbs::toks::SubtypeTable::from_records(&records);
    let mut carriers = Carriers::default();
    let mut reach = Reachable {
        faces: HashSet::from([0]),
        ..Reachable::default()
    };
    let mut out = AsmBrep::default();
    walk_reachable_topology(
        &mut out,
        &by_index,
        &table,
        &mut carriers,
        &mut reach,
        DecodePurpose::History,
        IdFormat("f3d"),
    );
    super::super::emit::emit_coedges(
        &mut out,
        &records,
        &table,
        None,
        &carriers,
        &reach,
        IdFormat("f3d"),
    );
    assert_eq!(out.coedges.len(), 1);
    assert_eq!(out.coedges[0].pcurves.len(), 1);
    assert_eq!(out.coedges[0].pcurves[0].parameter_range, None);
}
