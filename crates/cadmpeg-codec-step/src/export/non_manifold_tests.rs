// SPDX-License-Identifier: Apache-2.0
//! A solid with a non-manifold edge is written as a manifold closed shell.

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::topology::BodyKind;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Cursor;

struct Entity<'a> {
    name: &'a str,
    args: &'a str,
}

fn entities(text: &str) -> BTreeMap<&str, Entity<'_>> {
    text.lines()
        .filter_map(|line| {
            let (id, rest) = line.split_once(" = ")?;
            let (name, args) = rest.split_once('(')?;
            Some((id, Entity { name, args }))
        })
        .collect()
}

fn refs(args: &str) -> Vec<&str> {
    args.split(|c: char| c != '#' && !c.is_ascii_digit())
        .filter(|token| token.starts_with('#'))
        .collect()
}

#[test]
fn a_four_use_edge_is_split_into_two_material_use_pairs() {
    let ir = cadmpeg_test_support::polyhedron::edge_touching_cubes().unwrap();
    let checked = cadmpeg_ir::validate_neutral(&ir, Vec::new()).unwrap();
    assert!(checked.is_ok(), "{:?}", checked.findings);

    let mut bytes = Vec::new();
    let report = super::write_step(
        &ir,
        &mut bytes,
        crate::StepSchema::Ap214,
        &crate::StepWriteOptions::default(),
    )
    .unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert_eq!(text.matches("MANIFOLD_SOLID_BREP(").count(), 1);
    assert_eq!(text.matches("CLOSED_SHELL(").count(), 1);
    assert!(!text.contains("OPEN_SHELL("));
    assert!(!text.contains("SHELL_BASED_SURFACE_MODEL("));
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::StepLossCode::NonManifoldEdgeSplit.kind()));

    let entities = entities(&text);
    let mut users = HashMap::<&str, Vec<&str>>::new();
    for (id, entity) in &entities {
        for target in refs(entity.args) {
            users.entry(target).or_default().push(id);
        }
    }
    let edge_curves = entities
        .iter()
        .filter(|(_, entity)| entity.name == "EDGE_CURVE")
        .map(|(id, entity)| (*id, entity))
        .collect::<Vec<_>>();
    assert_eq!(edge_curves.len(), 24);
    for (id, _) in &edge_curves {
        let flags = users[id]
            .iter()
            .map(|user| {
                let entity = &entities[user];
                assert_eq!(entity.name, "ORIENTED_EDGE");
                entity.args.contains(".T.")
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(users[id].len(), 2, "{id} uses");
        assert_eq!(flags, BTreeSet::from([false, true]), "{id} senses");
    }

    let mut by_vertices = BTreeMap::<Vec<&str>, Vec<&str>>::new();
    for (id, entity) in &edge_curves {
        by_vertices
            .entry(refs(entity.args)[..2].to_vec())
            .or_default()
            .push(*id);
    }
    let split = by_vertices
        .values()
        .filter(|edges| edges.len() > 1)
        .collect::<Vec<_>>();
    assert_eq!(split.len(), 1);
    let normal = |face: &str| -> [i8; 3] {
        let plane = refs(entities[face].args)
            .into_iter()
            .find(|candidate| entities[candidate].name == "PLANE")
            .unwrap();
        let placement = refs(entities[plane].args)[0];
        let axis = refs(entities[placement].args)[1];
        let (_, lanes) = entities[axis].args.split_once('(').unwrap();
        let lanes = lanes
            .trim_end_matches(&[')', ';'][..])
            .split(',')
            .map(|lane| match lane.trim().parse::<f64>().unwrap() {
                value if value > 0.5 => 1,
                value if value < -0.5 => -1,
                _ => 0,
            })
            .collect::<Vec<_>>();
        [lanes[0], lanes[1], lanes[2]]
    };
    let mut pair_normals = BTreeSet::new();
    for edge in split[0] {
        let mut sum = [0_i8; 3];
        for oriented in &users[edge] {
            let edge_loop = users[oriented][0];
            let bound = users[edge_loop][0];
            let face = users[bound][0];
            assert_eq!(entities[face].name, "ADVANCED_FACE");
            for (lane, value) in sum.iter_mut().zip(normal(face)) {
                *lane += value;
            }
        }
        pair_normals.insert(sum);
    }
    assert_eq!(pair_normals, BTreeSet::from([[1, 1, 0], [-1, -1, 0]]));

    let decoded = crate::StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.bodies[0].kind, BodyKind::Solid);
    assert_eq!(decoded.ir().model.faces.len(), 12);
    assert_eq!(decoded.ir().model.edges.len(), 24);
}
