// SPDX-License-Identifier: Apache-2.0
use super::*;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

fn source(repeated: bool) -> CadIr {
    let xml = b"<Document SchemaVersion='4' FileVersion='1'><Objects Count='1'><Object type='Part::Feature' name='Shape' id='1'/></Objects><ObjectData Count='1'><Object name='Shape'><Properties Count='1'><Property name='Shape' type='Part::PropertyPartShape'><Part file='Shape.brp'/></Property></Properties></Object></ObjectData></Document>";
    let tail = if repeated {
        "+4 0 +4 0 *"
    } else {
        "+4 0 +1 0 *"
    };
    let brep = format!("CASCADE Topology V1, (c) Matra-Datavision\nLocations 0\nCurve2ds 0\nCurves 2\n1 0 0 0 1 0 0\n1 2 0 0 1 0 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 6\nVe 0.001 0 0 0 0 0 1001000 *\nVe 0.001 1 0 0 0 0 1001000 *\nEd 0.001 1 1 0 1 1 0 0 1 0 1001000 +6 0 -5 0 *\nVe 0.001 2 0 0 0 0 1001000 *\nVe 0.001 3 0 0 0 0 1001000 *\nEd 0.001 1 1 0 1 2 0 0 1 0 1001000 +3 0 -2 0 *\n{tail}");
    let bytes = crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", xml),
        ("Shape.brp", brep.as_bytes()),
    ]);
    crate::FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap()
        .ir()
        .clone()
}

fn emitted_edges(ir: &CadIr, order: &[usize]) -> Vec<(usize, usize)> {
    let bodies = order
        .iter()
        .map(|index| &ir.model.bodies[*index])
        .collect::<Vec<_>>();
    let serialized = super::super::serialize(ir, &bodies).unwrap().unwrap();
    let body_names = bodies
        .iter()
        .map(|body| (body.id.as_str().to_owned(), "Shape".to_owned()))
        .collect();
    let mut names = HashMap::new();
    let mut losses = Vec::new();
    assign_names(&serialized.topology, &body_names, &mut names, &mut losses);
    assert!(losses.is_empty(), "{losses:?}");
    let mut edges = Vec::new();
    for index in order {
        let body = &ir.model.bodies[*index];
        let current = ir
            .model
            .regions
            .iter()
            .filter(|region| region.body == body.id)
            .flat_map(|region| &region.shells)
            .flat_map(|id| ir.model.shells.iter().filter(move |shell| shell.id == *id))
            .flat_map(|shell| shell.wire_edges())
            .collect::<Vec<_>>();
        assert_eq!(current.len(), 1);
        let uses = serialized
            .topology
            .iter()
            .filter(|usage| {
                usage.body.as_deref() == Some(body.id.as_str())
                    && usage.indexed_name == "Edge"
                    && usage.topology_id == current[0].as_str()
            })
            .collect::<Vec<_>>();
        assert_eq!(uses.len(), 1);
        assert_eq!(names[current[0].as_str()], "Shape");
        assert_eq!(
            names[&format!("{}:subelement", current[0])],
            format!("Edge{}", uses[0].output_index)
        );
        edges.push((index + 1, uses[0].output_index));
    }
    edges.sort_unstable();
    edges
}

#[test]
fn current_topology_reordering_edits_and_deletion_change_emitted_ordinals() {
    let mut ir = source(false);
    assert_eq!(ir.model.bodies.len(), 2);
    assert_eq!(emitted_edges(&ir, &[0, 1]), [(1, 1), (2, 2)]);
    ir.model.bodies[0].transform = Some(
        Transform::affine([
            [1.0, 0.0, 0.0, 5.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ])
        .unwrap(),
    );
    assert_eq!(emitted_edges(&ir, &[1, 0]), [(1, 2), (2, 1)]);
    assert_eq!(emitted_edges(&ir, &[1]), [(2, 1)]);
}

#[test]
fn arbitrary_neutral_identity_rewrites_preserve_forward_only_mapping() {
    let mut ir = source(false);
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut renamed = HashMap::new();
    let mut next = 0;
    macro_rules! collect {
        ($($arena:ident),*) => {$(for row in &ir.model.$arena {
            next += 1;
            renamed.insert(row.id.as_str().to_owned(), format!("author:neutral:entity#{next}"));
        })*};
    }
    collect!(points, vertices, curves, edges, regions, shells, bodies);
    macro_rules! rewrite {
        ($($arena:ident),*) => {$(ir.model.$arena = cadmpeg_ir::schema::rewrite::identities(
            &ctx, ir.model.$arena, |id| Ok(renamed.get(id).cloned().unwrap_or_else(|| id.to_owned()))).unwrap();)*};
    }
    rewrite!(points, vertices, curves, edges, regions, shells, bodies);
    assert_eq!(emitted_edges(&ir, &[1, 0]), [(1, 2), (2, 1)]);
}

#[test]
fn indistinguishable_repeated_source_roots_have_no_invented_association() {
    let ir = source(true);
    assert_eq!(ir.model.bodies.len(), 2);
    let payloads =
        crate::brep::records::payloads_untracked(ir.native.namespace("fcstd").unwrap()).unwrap();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let ctx = cadmpeg_core::decode::DecodeContext::for_encode(
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    );
    let canonical =
        crate::topology_transfer::canonical_indices::for_payload(&ctx, &payloads[0]).unwrap();
    assert_eq!(
        canonical
            .into_iter()
            .filter(|(kind, _)| *kind == "Edge")
            .collect::<Vec<_>>(),
        [("Edge", 1)]
    );
    assert_eq!(emitted_edges(&ir, &[0, 1]), [(1, 1), (2, 2)]);
}
