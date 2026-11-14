// SPDX-License-Identifier: Apache-2.0
//! Non-boundary edge uses retain geometry and leave face bounds continuous.

use std::io::Cursor;

use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::topology::{BodyKind, LoopBoundary, Sense};
use cadmpeg_ir::{Codec, DecodeOptions, DecodeResult};

use crate::loss::FreecadLossCode;
use crate::test_support::test_archive::{archive_entries, assert_valid_document};
use crate::FcstdCodec;

const CYLINDER: &str = "CASCADE Topology V1, (c) Matra-Datavision
Locations 2
1
1 0 0 10
0 1 0 20
0 0 1 30
2 1 1 1 -1 0
Curve2ds 5
1 0 0 1 0
1 0 2 1 0
1 0 0 0 1
1 6.283185307179586 0 0 1
1 3.141592653589793 0.5 0 1
Curves 4
2 0 0 0 0 0 1 1 0 0 0 1 0 1
2 0 0 2 0 0 1 1 0 0 0 1 0 1
1 1 0 0 0 0 1
1 -1 0 0.5 0 0 1
Polygon3D 0
PolygonOnTriangulations 0
Surfaces 3
1 0 0 0 0 0 -1 1 0 0 0 -1 0
1 0 0 2 0 0 1 1 0 0 0 1 0
2 0 0 0 0 0 1 1 0 0 0 1 0 1
Triangulations 0
TShapes 17
Ve 0.0000001 1 0 0 0 0 1001000 *
Ve 0.0000001 1 0 2 0 0 1001000 *
Ve 0.0000001 -1 0 0.5 0 0 1001000 *
Ve 0.0000001 -1 0 1.5 0 0 1001000 *
Ed 0.0000001 1 1 0 1 1 0 0 6.283185307179586 2 1 3 0 0 6.283185307179586 0 1001000 +17 0 -17 0 *
Ed 0.0000001 1 1 0 1 2 0 0 6.283185307179586 2 2 3 0 0 6.283185307179586 0 1001000 +16 0 -16 0 *
Ed 0.0000001 1 1 0 1 3 0 0 2 3 3 4 C0 3 0 0 2 0 1001000 +17 0 -16 0 *
Ed 0.0000001 1 1 0 1 4 0 0 1 2 5 3 0 0 1 0 1001000 +15 0 -14 0 *
Wi 1001000 -13 0 *
Wi 1001000 +12 0 *
Wi 1001000 +13 0 +11 0 -12 0 -11 0 *
Wi 1001000 i10 0 *
Fa 0 0.0000001 1 0 1001000 +9 0 *
Fa 0 0.0000001 2 0 1001000 +8 0 *
Fa 0 0.0000001 3 0 1001000 +7 0 +6 2 *
Sh 1001000 +5 0 +4 0 +3 0 *
So 1001000 +2 0 *
+1 1 *";

fn decode(brep: &str) -> DecodeResult {
    let document = br#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Part::Feature" name="Cylinder"/></Objects>
<ObjectData Count="1"><Object name="Cylinder"><Properties Count="1">
<Property name="Shape" type="Part::PropertyPartShape"><Part file="Cylinder.brp"/></Property>
</Properties></Object></ObjectData></Document>"#;
    let bytes = archive_entries(&[
        ("Document.xml", document),
        ("Cylinder.brp", brep.as_bytes()),
    ]);
    FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("cylinder with additional edge topology")
}

fn assert_closed_cylinder(brep: &str) {
    let decoded = decode(brep);
    let ir = decoded.ir();
    assert_valid_document(ir);
    assert_eq!(ir.model.bodies.len(), 1);
    assert_eq!(ir.model.bodies[0].kind, BodyKind::Solid);
    assert_eq!(ir.model.faces.len(), 3);
    assert_eq!(ir.model.loops.len(), 3);
    assert_eq!(ir.model.edges.len(), 4);
    assert_eq!(ir.model.coedges.len(), 6);
    let [shell] = ir.model.shells.as_slice() else {
        panic!("one closed shell");
    };
    assert_eq!(shell.faces().len(), 3);
    let [wire_edge] = shell.wire_edges() else {
        panic!("additional edge remains shell wire geometry");
    };
    let edge = ir
        .model
        .edges
        .iter()
        .find(|edge| &edge.id == wire_edge)
        .unwrap();
    assert_ne!(edge.start, edge.end);
    assert!(edge.curve().is_some());
    assert!(ir
        .model
        .coedges
        .iter()
        .all(|coedge| &coedge.edge != wire_edge));
    for face_loop in &ir.model.loops {
        let LoopBoundary::Ring(ring) = &face_loop.boundary else {
            panic!("boundary ring");
        };
        let ends: Vec<_> = ring
            .coedges()
            .iter()
            .map(|id| {
                let coedge = ir
                    .model
                    .coedges
                    .iter()
                    .find(|coedge| &coedge.id == id)
                    .unwrap();
                let edge = ir
                    .model
                    .edges
                    .iter()
                    .find(|edge| edge.id == coedge.edge)
                    .unwrap();
                match coedge.sense {
                    Sense::Forward => (&edge.start, &edge.end),
                    Sense::Reversed => (&edge.end, &edge.start),
                }
            })
            .collect();
        assert!(ends
            .iter()
            .zip(ends.iter().cycle().skip(1))
            .all(|(a, b)| a.1 == b.0));
    }
    let incidence_loss = FreecadLossCode::TopologyNonBoundaryIncidenceRetained
        .note("")
        .code;
    let losses: Vec<_> = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == incidence_loss)
        .collect();
    assert_eq!(losses.len(), 1);
    let plan = cadmpeg_codec_step::StepCodec::default()
        .plan(EncodeInput::new(ir, None), TargetRequest::Explicit("ap214"))
        .expect("closed cylinder STEP plan");
    assert!(plan
        .report()
        .losses
        .iter()
        .all(|loss| loss.code.to_string() != "step/topology.loop-no-continuous-ordering"));
    let mut step = Vec::new();
    plan.write_to(&mut step).unwrap();
    let step = String::from_utf8(step).unwrap();
    assert_eq!(step.matches("ADVANCED_FACE(").count(), 3);
    assert_eq!(step.matches("CLOSED_SHELL(").count(), 1);
    assert_eq!(step.matches("MANIFOLD_SOLID_BREP(").count(), 1);
    assert_eq!(step.matches("EDGE_CURVE(").count(), 4);
}

#[test]
fn internal_edge_in_a_separate_located_wire_keeps_the_closed_face() {
    assert_closed_cylinder(CYLINDER);
}

#[test]
fn external_edge_keeps_the_closed_face_and_its_wire_geometry() {
    assert_closed_cylinder(&CYLINDER.replace("i10 0", "e10 0"));
}

#[test]
fn internal_wire_orientation_applies_to_its_forward_edge() {
    assert_closed_cylinder(&CYLINDER.replace("i10 0", "+10 0").replace("+6 2", "i6 2"));
}

#[test]
fn mixed_wire_keeps_boundary_and_internal_edge_roles() {
    assert_closed_cylinder(
        &CYLINDER
            .replace("-12 0 -11 0 *", "-12 0 i10 0 -11 0 *")
            .replace("+7 0 +6 2", "+7 0"),
    );
}

#[test]
fn forward_open_edge_still_fails_boundary_ordering() {
    let decoded = decode(&CYLINDER.replace("i10 0", "+10 0"));
    assert_eq!(decoded.ir().model.bodies[0].kind, BodyKind::Sheet);
    assert_eq!(decoded.ir().model.loops.len(), 4);
    let plan = cadmpeg_codec_step::StepCodec::default()
        .plan(
            EncodeInput::new(decoded.ir(), None),
            TargetRequest::Explicit("ap214"),
        )
        .unwrap();
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.to_string() == "step/topology.loop-no-continuous-ordering"));
}

#[test]
fn repeated_internal_edge_uses_share_one_shell_wire_edge() {
    assert_closed_cylinder(&CYLINDER.replace("i10 0 *", "i10 0 i10 0 *"));
}

#[test]
fn an_internal_use_of_a_boundary_edge_keeps_one_edge_owner() {
    let decoded = decode(&CYLINDER.replace("i10 0", "i11 0"));
    assert_valid_document(decoded.ir());
    assert_eq!(decoded.ir().model.bodies[0].kind, BodyKind::Solid);
    assert_eq!(decoded.ir().model.edges.len(), 3);
    assert!(decoded.ir().model.shells[0].wire_edges().is_empty());
}

#[test]
fn a_face_root_keeps_its_internal_edge_as_shell_wire_geometry() {
    let decoded = decode(&CYLINDER.replace("\n+1 1 *", "\n+3 1 *"));
    assert_valid_document(decoded.ir());
    assert_eq!(decoded.ir().model.bodies[0].kind, BodyKind::Sheet);
    assert_eq!(decoded.ir().model.faces.len(), 1);
    assert_eq!(decoded.ir().model.loops.len(), 1);
    assert_eq!(decoded.ir().model.shells[0].wire_edges().len(), 1);
}
