// SPDX-License-Identifier: Apache-2.0
//! Partition attributes past `0x7fff` take the 4-byte XMT pointer form
//! (`parasolid.md` §1, `sldprt.md` §4) and read back through the decoder.

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::topology::Color;

/// The unit cube with a colour on every face, so the partition also holds
/// the colour `ATTRIBUTE`, `REAL_VALUES` and attribute-definition records.
fn coloured_cube() -> CadIr {
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    for (index, face) in ir.geometry.faces.iter_mut().enumerate() {
        let channel = f64::from(u8::try_from(index).expect("six faces")) / 8.0;
        face.color = Some(Color::new(channel, 0.5, 1.0 - channel, 1.0).expect("valid colour"));
    }
    ir
}

/// A triangle on a NURBS surface bounded by a NURBS edge, decoded from its
/// native record image.
fn nurbs_triangle() -> CadIr {
    let native = crate::test_support::container::sldprt_with_body(
        &crate::test_support::parasolid::nurbs_triangle_body(),
    );
    crate::SldprtCodec
        .decode(&mut Cursor::new(native), &DecodeOptions::default())
        .expect("native triangle decodes")
        .ir()
        .clone()
}

fn read_back(ir: &CadIr, first: u16) -> CadIr {
    let body = crate::writer::brep_body_from(ir, 0.001, false, first)
        .unwrap_or_else(|error| panic!("partition from attr {first:#x}: {error}"));
    let native = crate::test_support::container::sldprt_with_body(&body);
    crate::SldprtCodec
        .decode(&mut Cursor::new(native), &DecodeOptions::default())
        .expect("written partition decodes")
        .ir()
        .clone()
}

/// What a reader can observe of the written B-rep, independent of the
/// attribute numbers that the decoded identities carry.
#[derive(Debug, PartialEq)]
struct Observed {
    counts: [usize; 8],
    points: Vec<[u64; 3]>,
    colours: Vec<String>,
    senses: Vec<String>,
    curves: Vec<String>,
    surfaces: Vec<String>,
}

fn observed(ir: &CadIr) -> Observed {
    let geometry = &ir.geometry;
    let mut points = geometry
        .points
        .iter()
        .map(|point| {
            let p = point.position().get();
            [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()]
        })
        .collect::<Vec<_>>();
    points.sort_unstable();
    let mut colours = geometry
        .faces
        .iter()
        .map(|face| format!("{:?}", face.color))
        .collect::<Vec<_>>();
    colours.sort();
    let mut senses = geometry
        .faces
        .iter()
        .map(|face| format!("{:?}", face.sense))
        .collect::<Vec<_>>();
    senses.sort();
    let mut curves = geometry
        .curves
        .iter()
        .map(|curve| format!("{:?}", curve.geometry))
        .collect::<Vec<_>>();
    curves.sort();
    let mut surfaces = geometry
        .surfaces
        .iter()
        .map(|surface| format!("{:?}", surface.geometry))
        .collect::<Vec<_>>();
    surfaces.sort();
    Observed {
        counts: [
            geometry.bodies.len(),
            geometry.regions.len(),
            geometry.shells.len(),
            geometry.faces.len(),
            geometry.loops.len(),
            geometry.coedges.len(),
            geometry.edges.len(),
            geometry.vertices.len(),
        ],
        points,
        colours,
        senses,
        curves,
        surfaces,
    }
}

#[test]
fn a_part_whose_attributes_cross_the_short_pointer_range_reads_back_as_written_below_it() {
    // 0x7fff - 40 crosses the boundary inside the topology records; 0x8000
    // makes every attribute wide, the first 256 of them with a cell that
    // begins `ff` like the optional first-of-type byte; 65 500 crosses the
    // largest one-quotient value (65 534) and 0x1_0000.
    for (name, ir) in [("cube", coloured_cube()), ("nurbs", nurbs_triangle())] {
        let narrow = observed(&read_back(&ir, 2));
        assert_ne!(
            narrow.counts[3], 0,
            "{name}: the narrow write keeps its faces"
        );
        for first in [0x7fff - 40, 0x8000, 65_500] {
            let wide = read_back(&ir, first);
            assert_eq!(
                observed(&wide),
                narrow,
                "{name} written from attr {first:#x} reads back as written from attr 2"
            );
            assert!(
                cadmpeg_ir::validate_neutral(&wide, Vec::new())
                    .expect("validation runs")
                    .is_ok(),
                "{name} from attr {first:#x}"
            );
        }
    }
}

#[test]
fn a_compact_record_past_the_short_range_stores_its_attr_in_the_native_four_byte_cell() {
    // The writer gives the cube's six planes the first attributes, in order,
    // as `00 32 attr ordinal refs[5] marker values[9]` (§7.1).
    let body = crate::writer::brep_body_from(&coloured_cube(), 0.001, false, 0x7fff)
        .expect("partition is written");
    let record = 2 + 2 + 4 + 10 + 1 + 9 * 8;
    assert_eq!(body[..4], [0x00, 0x32, 0x7f, 0xff], "0x7fff is one word");
    // 0x8000 = 1 * 32767 + 1: remainder word -1, quotient word 1.
    assert_eq!(
        body[record..record + 10],
        [0x00, 0x32, 0xff, 0xff, 0x00, 0x01, 0, 0, 0, 0],
        "0x8000 is the int16 pair, and the ordinal follows it"
    );
    let second = record + 2 + 4 + 4 + 10 + 1 + 9 * 8;
    assert_eq!(
        body[second..second + 6],
        [0x00, 0x32, 0xff, 0xfe, 0x00, 0x01]
    );
}
