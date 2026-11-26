// SPDX-License-Identifier: Apache-2.0
//! Body placements the writer cannot bake into model-space geometry.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::EncodeInput;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::container::sldprt_with_body;
use crate::test_support::parasolid::{entity51, owned_triangle};
use crate::SldprtCodec;

/// A body whose placement is no proper rigid motion cannot be baked into its
/// geometry: the write leaves out that body and the tessellation bound to it,
/// names both, and writes the other body and its tessellation (§3.4,
/// Content the target cannot hold).
#[test]
fn an_unbakeable_placement_drops_only_its_body_and_tessellation() {
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::tessellation::{Meshed, Tessellation, TessellationId, TessellationMesh};
    use cadmpeg_ir::transform::Transform;

    let mut body = Vec::new();
    body.extend(entity51(2, 500, 0x0017, &[700, 0, 0, 0, 0, 0]));
    body.extend(entity51(2, 501, 0x0017, &[701, 0, 0, 0, 0, 0]));
    body.extend(owned_triangle(0, 700, 0.0));
    body.extend(owned_triangle(200, 701, 10.0));
    let mut ir = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap()
        .into_parts()
        .0;
    ir.source = None;
    ir.native = cadmpeg_ir::Native::default();
    ir.geometry
        .bodies
        .iter_mut()
        .for_each(|body| body.name = None);
    ir.geometry
        .faces
        .iter_mut()
        .for_each(|face| face.name = None);
    let bodies = ir
        .geometry
        .bodies
        .iter()
        .map(|body| body.id.clone())
        .collect::<Vec<_>>();
    assert_eq!(bodies.len(), 2);
    ir.geometry.bodies[0].transform = Some(
        Transform::affine([
            [-1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ])
        .expect("affine transform"),
    );
    ir.geometry.bodies[1].transform = Some(
        Transform::affine([
            [1.0, 0.0, 0.0, 5.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ])
        .expect("affine transform"),
    );
    let tessellations = bodies
        .iter()
        .enumerate()
        .map(|(index, body)| {
            Tessellation::new(
                TessellationId::mint(format!("synthetic:test:tessellation#{index}")).unwrap(),
                TessellationMesh::from_strip_lanes(
                    vec![
                        Point3::new(0.0, 0.0, 0.0),
                        Point3::new(1.0, 0.0, 0.0),
                        Point3::new(0.0, 1.0, 0.0),
                    ],
                    Some(vec![Vector3::new(0.0, 0.0, 1.0); 3]),
                    &[3],
                )
                .unwrap(),
                Vec::new(),
            )
            .unwrap()
            .with_of(Meshed::Body {
                body: body.clone(),
                faces: Vec::new(),
            })
        })
        .collect::<Vec<_>>();
    let dropped_tessellation = tessellations[0].id.clone();
    ir.model.tessellations = tessellations;

    let mut encoded = Vec::new();
    let report = crate::test_support::plan_fixture_with_native_envelope(
        EncodeInput::new(&ir, None),
        TargetRequest::Unnamed,
    )
    .unwrap()
    .write_to(&mut encoded)
    .unwrap();

    let omitted = report
        .losses
        .iter()
        .filter(|loss| loss.code == crate::loss::SldprtLossCode::WriterBodiesOmitted.kind())
        .map(|loss| loss.message.as_str())
        .collect::<Vec<_>>();
    assert_eq!(omitted.len(), 1, "{:#?}", report.losses);
    assert!(
        omitted[0].contains(bodies[0].as_str())
            && omitted[0].contains(dropped_tessellation.as_str())
            && !omitted[0].contains(bodies[1].as_str()),
        "{}",
        omitted[0]
    );
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().geometry.bodies.len(), 1);
    assert_eq!(decoded.ir().model.tessellations.len(), 1);
}
