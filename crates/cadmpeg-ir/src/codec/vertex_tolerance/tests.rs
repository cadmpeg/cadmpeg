// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

use super::widen;
use crate::annotations::{AnnotationBuilder, Annotations};
use crate::codec::loss::DecodeLossCode;
use crate::codec::{DecodeBody, DecodeResult, Decoded, FormatId};
use crate::examples::unit_cube;
use crate::features::geometry::FinitePoint3;
use crate::math::Point3;
use crate::provenance::Exactness;
use crate::source_fidelity::SourceFidelity;
use crate::CadIr;

/// The unit cube (side 10, each edge a line ranged from its start corner)
/// with the point of `corner` moved to `position`.
fn cube_with_corner_at(corner: usize, position: Point3) -> CadIr {
    let mut ir = unit_cube().unwrap();
    let suffix = format!("point#{corner}");
    ir.geometry
        .points
        .iter_mut()
        .find(|point| point.id.as_str().ends_with(&suffix))
        .unwrap()
        .set_position(FinitePoint3::new(position).unwrap());
    ir
}

fn vertex_tolerance(ir: &CadIr, corner: usize) -> Option<f64> {
    let suffix = format!("vertex#{corner}");
    ir.geometry
        .vertices
        .iter()
        .find(|vertex| vertex.id.as_str().ends_with(&suffix))
        .unwrap()
        .tolerance
        .map(crate::scalar::PositiveLength::get)
}

#[test]
fn a_vertex_off_the_ends_of_its_edges_states_its_largest_distance_marked_derived() {
    // Corner 0 off each of its three edges' start by (0.02, 0.02, 0.02): a
    // distance of 0.02·√3 from each end, 0.02·√2 from each line.
    let ir = cube_with_corner_at(0, Point3::new(-0.02, -0.02, -0.02));
    let decoded = Decoded {
        ir,
        body: DecodeBody::new(crate::report::decode::DecodeTransfer::full(true)),
        source_fidelity: SourceFidelity::new(crate::hash::digest::Sha256Digest::digest(
            b"test source",
        )),
    };
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = DecodeResult::from_decoded(&ctx, decoded, FormatId::new("test")).unwrap();
    ctx.finish_session().unwrap();

    let expected = 0.02 * 3.0_f64.sqrt();
    let stated = vertex_tolerance(result.ir(), 0).unwrap();
    assert!(
        (stated - expected).abs() <= 16.0 * f64::EPSILON * expected,
        "{stated}"
    );
    for corner in 1..8 {
        assert_eq!(vertex_tolerance(result.ir(), corner), None);
    }
    let vertex = result
        .ir()
        .geometry
        .vertices
        .iter()
        .find(|vertex| vertex.id.as_str().ends_with("vertex#0"))
        .unwrap();
    let note = &result.source_fidelity().annotations.exactness()[vertex.id.as_str()];
    assert_eq!(note.entity(), Exactness::ByteExact);
    assert_eq!(
        note.fields()
            .iter()
            .map(|(field, exactness)| (field.as_str(), *exactness))
            .collect::<Vec<_>>(),
        [("tolerance", Exactness::Derived)]
    );
    let widened = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == DecodeLossCode::VertexToleranceWidened.note("").code)
        .collect::<Vec<_>>();
    let [loss] = widened.as_slice() else {
        panic!("one loss for the widened vertex: {widened:?}");
    };
    assert_eq!(loss.severity, crate::report::Severity::Info);
    assert!(
        loss.message.starts_with("widened 1 vertex tolerance(s)"),
        "{}",
        loss.message
    );
}

#[test]
fn a_vertex_on_its_edges_ends_keeps_its_tolerance() {
    let mut ir = unit_cube().unwrap();
    let mut annotations = Annotations::default();
    assert_eq!(
        widen(&mut ir, &mut annotations, &DecodePolicy::default()).unwrap(),
        None
    );
    assert_eq!(ir, unit_cube().unwrap());
    assert_eq!(annotations, Annotations::default());
}

#[test]
fn a_vertex_on_a_carrier_far_from_its_end_keeps_its_tolerance() {
    // Corner 1 moved to the middle of edge 0's line: 5 from that line's end
    // and on the line itself, so the edge's range is off, not the vertex.
    let mut ir = cube_with_corner_at(1, Point3::new(5.0, 0.0, 0.0));
    let before = ir.clone();
    let mut annotations = Annotations::default();
    assert_eq!(
        widen(&mut ir, &mut annotations, &DecodePolicy::default()).unwrap(),
        None
    );
    assert_eq!(ir, before);
}

#[test]
fn a_vertex_whose_point_the_decoder_computed_keeps_its_tolerance() {
    let mut ir = cube_with_corner_at(0, Point3::new(-0.02, -0.02, -0.02));
    let point = ir
        .geometry
        .points
        .iter()
        .find(|point| point.id.as_str().ends_with("point#0"))
        .unwrap()
        .id
        .clone();
    let mut builder = AnnotationBuilder::new();
    builder.exactness(point.as_str(), Exactness::Derived);
    let mut annotations = builder.build();
    let before = ir.clone();
    assert_eq!(
        widen(&mut ir, &mut annotations, &DecodePolicy::default()).unwrap(),
        None
    );
    assert_eq!(ir, before);
}
