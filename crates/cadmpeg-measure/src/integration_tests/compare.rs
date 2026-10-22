// SPDX-License-Identifier: Apache-2.0
//! ID-free comparison of measured documents: copies, renamed and moved
//! copies, coincident bodies, signature bodies and occurrence placement.

use cadmpeg_ir::features::FiniteVector3;
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::topology::{BodyKind, Sense};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;

use crate::compare::{compare, CompareOptions, Comparison, Operand, Rule, Verdict};
use crate::reason::CompareCause;
use crate::select::Selection;
use crate::test_support::{measure, Builder};
use crate::value::{BoxKind, Quantity};

fn cube() -> CadIr {
    cadmpeg_ir::examples::unit_cube().expect("fixture")
}

/// `ir` with every ID prefix `from` replaced by `to`.
fn renamed(ir: &CadIr, from: &str, to: &str) -> CadIr {
    let text = ir.to_canonical_json().expect("json").replace(from, to);
    let mut copy = CadIr::from_json(&text).expect("renamed document");
    copy.finalize();
    copy
}

/// The model arenas of `a` and `b` in one document; `b`'s IDs must not
/// collide with `a`'s.
fn merged(a: &CadIr, b: &CadIr) -> CadIr {
    let mut first: serde_json::Value =
        serde_json::from_str(&a.to_canonical_json().expect("json")).expect("value");
    let second: serde_json::Value =
        serde_json::from_str(&b.to_canonical_json().expect("json")).expect("value");
    let (Some(model), Some(other)) = (first["model"].as_object_mut(), second["model"].as_object())
    else {
        panic!("documents have models");
    };
    for (arena, records) in other {
        if let (Some(target), Some(records)) = (
            model
                .get_mut(arena)
                .and_then(serde_json::Value::as_array_mut),
            records.as_array(),
        ) {
            target.extend(records.iter().cloned());
        }
    }
    let mut document = CadIr::from_json(&first.to_string()).expect("merged document");
    document.finalize();
    document
}

fn comparison(a: &CadIr, b: &CadIr) -> Comparison {
    comparison_with(a, b, CompareOptions::default())
}

fn comparison_with(a: &CadIr, b: &CadIr, options: CompareOptions) -> Comparison {
    let report_a = measure(a, &Selection::default());
    let report_b = measure(b, &Selection::default());
    compare(a, &report_a, b, &report_b, &options)
}

fn translated(ir: &CadIr, x: f64) -> CadIr {
    let mut moved = ir.clone();
    let shift = FiniteVector3::new(Vector3::new(x, 0.0, 0.0)).expect("finite");
    moved.model.bodies[0].transform = Some(Transform::identity().with_translation(shift));
    moved
}

#[test]
fn a_document_agrees_with_itself_and_with_a_renamed_copy() {
    let cube = cube();
    let result = comparison(&cube, &cube);
    assert_eq!(result.verdict(), Verdict::Agree);
    assert_eq!(result.pairs().len(), 1);
    assert!(result.inconclusive_causes().is_empty());
    let copy = renamed(&cube, "synthetic:cube:", "synthetic:copy:");
    assert_ne!(copy.model.bodies[0].id, cube.model.bodies[0].id);
    let result = comparison(&cube, &copy);
    assert_eq!(result.verdict(), Verdict::Agree);
    let [pair] = result.pairs() else {
        panic!("one pair");
    };
    assert_eq!(pair.a(), cube.model.bodies[0].id.as_str());
    assert_eq!(pair.b(), copy.model.bodies[0].id.as_str());
    assert!(pair
        .quantities()
        .iter()
        .any(|row| row.quantity() == "volume" && row.agrees() == Some(true)));
}

#[test]
fn a_moved_copy_leaves_both_bodies_unmatched() {
    let cube = cube();
    let moved = translated(&cube, 5.0);
    let result = comparison(&cube, &moved);
    assert_eq!(result.verdict(), Verdict::Disagree);
    assert!(result.pairs().is_empty());
    assert_eq!(result.unmatched_a().len(), 1);
    assert_eq!(result.unmatched_b().len(), 1);
    let row = |name: &str| {
        result
            .document()
            .iter()
            .find(|row| row.quantity() == name)
            .unwrap_or_else(|| panic!("document row {name}"))
    };
    assert_eq!(row("area").agrees(), Some(true));
    assert_eq!(row("closed_volume").agrees(), Some(true));
    assert_eq!(row("bbox.min.x").agrees(), Some(false));
}

#[test]
fn the_relative_floor_widens_every_tolerance() {
    let cube = cube();
    let moved = translated(&cube, 1.0e-3);
    assert_eq!(comparison(&cube, &moved).verdict(), Verdict::Disagree);
    let wide = CompareOptions::new(1.0e-3).expect("floor");
    assert_eq!(
        comparison_with(&cube, &moved, wide).verdict(),
        Verdict::Agree
    );
    assert!(CompareOptions::new(1.0).is_none());
    assert!(CompareOptions::new(-1.0e-3).is_none());
}

#[test]
fn coincident_bodies_pair_by_multiplicity() {
    let cube = cube();
    let twice = merged(&cube, &renamed(&cube, "synthetic:cube:", "synthetic:twin:"));
    assert_eq!(twice.model.bodies.len(), 2);
    let copy = renamed(&twice, "synthetic:", "synthetic-b:");
    let result = comparison(&twice, &copy);
    assert_eq!(result.verdict(), Verdict::Agree);
    assert_eq!(result.pairs().len(), 2);
    let result = comparison(&twice, &cube);
    assert_eq!(result.verdict(), Verdict::Disagree);
    assert_eq!(result.unmatched_a().len(), 1);
    assert!(result.unmatched_b().is_empty());
}

/// A datum plane has no vertex, so nothing in its signature depends on its
/// position: the pair is made, and the verdict stays inconclusive.
#[test]
fn unbounded_faces_compare_by_signature() {
    let mut builder = Builder::new();
    let plane = builder.plane([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    builder.face(&plane, Sense::Forward, &[], false);
    let datum = builder.finish(BodyKind::Sheet);
    let result = comparison(&datum, &datum);
    assert_eq!(result.verdict(), Verdict::Inconclusive);
    assert_eq!(
        result.inconclusive_causes(),
        [CompareCause::SignatureUnplaced]
    );
    let [pair] = result.pairs() else {
        panic!("one pair");
    };
    for row in pair.quantities() {
        if row.quantity() == "signature.vertex_box" {
            assert_eq!(row.rule(), Rule::NotCompared);
            assert_eq!(row.cause(), Some(CompareCause::SignatureUnplaced));
        } else {
            assert_eq!(row.rule(), Rule::Signature, "{}", row.quantity());
            assert_eq!(row.agrees(), Some(true), "{}", row.quantity());
        }
    }
}

/// One plane face with two disjoint square loops, 10 mm wide and 10 mm
/// apart: no loop winds around the other, so the face has no region and the
/// body compares by its signature.
fn islands(shift: f64) -> CadIr {
    let mut builder = Builder::new();
    let plane = builder.plane([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let rings: Vec<Vec<_>> = [0.0, 20.0]
        .iter()
        .map(|x0| {
            let vertices: Vec<_> = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]]
                .iter()
                .map(|[x, y]| builder.vertex([x0 + x, *y, 0.0]))
                .collect();
            (0..4)
                .map(|index| {
                    (
                        builder.line(&vertices[index], &vertices[(index + 1) % 4]),
                        Sense::Forward,
                    )
                })
                .collect()
        })
        .collect();
    builder.face(&plane, Sense::Forward, &rings, false);
    let ir = builder.finish(BodyKind::Sheet);
    translated(&ir, shift)
}

#[test]
fn a_moved_signature_body_disagrees_by_its_vertex_box() {
    let here = islands(0.0);
    let report = measure(&here, &Selection::default());
    let body = &report.bodies()[0];
    assert!(body.area().as_ref().and_then(Quantity::measured).is_none());
    assert!(body.bbox().measured().is_none());
    let vertex_box = body
        .vertex_box()
        .as_ref()
        .and_then(Quantity::measured)
        .expect("vertex box");
    assert_eq!(vertex_box.value().max.get().x, 30.0);
    let same = comparison(&here, &islands(0.0));
    assert_eq!(same.verdict(), Verdict::Agree);
    let [pair] = same.pairs() else {
        panic!("one pair");
    };
    assert!(pair
        .quantities()
        .iter()
        .any(|row| row.quantity() == "signature.vertex_box.min.x" && row.agrees() == Some(true)));
    let moved = comparison(&here, &islands(100.0));
    assert_eq!(moved.verdict(), Verdict::Disagree);
    assert!(moved.pairs().is_empty());
    assert_eq!(moved.unmatched_a().len(), 1);
    assert_eq!(moved.unmatched_b().len(), 1);
}

#[test]
fn occurrences_that_place_bodies_keep_the_verdict_inconclusive() {
    use cadmpeg_ir::ids::{OccurrenceId, ProductDefinitionId};
    use cadmpeg_ir::products::{
        Occurrence, OccurrenceParent, ProductDefinition, ProductDefinitionKind, PrototypeReference,
    };
    let placed = |transform: Transform| {
        let mut cube = cube();
        let definition = ProductDefinitionId::mint("synthetic:cube:product#1").expect("id");
        cube.model.product_definitions.push(ProductDefinition {
            id: definition.clone(),
            kind: ProductDefinitionKind::Part,
            source_name: None,
            label: None,
            description: None,
            part_number: None,
            bom_properties: std::collections::BTreeMap::new(),
            bodies: vec![cube.model.bodies[0].id.clone()],
            native_ref: None,
        });
        let one = cadmpeg_ir::scalar::FiniteReal::new(1.0).expect("finite");
        cube.model.occurrences.push(Occurrence {
            id: OccurrenceId::mint("synthetic:cube:occurrence#1").expect("id"),
            prototype: PrototypeReference::Local { definition },
            parent: OccurrenceParent::Root {},
            ordinal: 0,
            transform,
            linked_prototype: None,
            scale: [one; 3],
            name: None,
            visible: None,
            link: None,
            native_ref: None,
        });
        cube
    };
    let plain = cube();
    let identity = placed(Transform::identity());
    assert_eq!(comparison(&plain, &identity).verdict(), Verdict::Agree);
    let shift = FiniteVector3::new(Vector3::new(15.0, 10.0, 0.0)).expect("finite");
    let moved = placed(Transform::identity().with_translation(shift));
    let result = comparison(&plain, &moved);
    assert_eq!(result.verdict(), Verdict::Inconclusive);
    assert_eq!(
        result.inconclusive_causes(),
        [CompareCause::OccurrencePlacement]
    );
    let occurrences = result
        .document()
        .iter()
        .find(|row| row.quantity() == "occurrences")
        .expect("occurrence row");
    assert_eq!(occurrences.rule(), Rule::NotCompared);
    assert_eq!(
        occurrences.b(),
        Some(&Operand::Text("occurrences".to_owned()))
    );
}

/// The bilinear NURBS patch `(x, y) = (2u, v)` on `[0, 1]²` at `z = 0`,
/// bounded by its four domain edges: a 2 × 1 rectangle whose box is the
/// conservative control hull.
fn nurbs_rectangle(transform: Option<Transform>) -> CadIr {
    let mut builder = Builder::new();
    let surface = builder.nurbs(
        (1, vec![0.0, 0.0, 1.0, 1.0]),
        (1, vec![0.0, 0.0, 1.0, 1.0]),
        &[
            vec![([0.0, 0.0, 0.0], 1.0), ([0.0, 1.0, 0.0], 1.0)],
            vec![([2.0, 0.0, 0.0], 1.0), ([2.0, 1.0, 0.0], 1.0)],
        ],
    );
    let corners = [[0.0, 0.0], [2.0, 0.0], [2.0, 1.0], [0.0, 1.0]];
    let vertices: Vec<_> = corners
        .iter()
        .map(|[x, y]| builder.vertex([*x, *y, 0.0]))
        .collect();
    let ring: Vec<_> = (0..4)
        .map(|index| {
            (
                builder.line(&vertices[index], &vertices[(index + 1) % 4]),
                Sense::Forward,
            )
        })
        .collect();
    builder.face(&surface, Sense::Forward, &[ring], false);
    let mut ir = builder.finish(BodyKind::Sheet);
    ir.model.bodies[0].transform = transform;
    ir
}

#[test]
fn a_nurbs_body_turned_about_its_centroid_disagrees_by_its_box() {
    let plain = nurbs_rectangle(None);
    let turn = Transform::affine([
        [0.0, -1.0, 0.0, 1.5],
        [1.0, 0.0, 0.0, -0.5],
        [0.0, 0.0, 1.0, 0.0],
    ])
    .expect("finite rotation");
    let turned = nurbs_rectangle(Some(turn));
    let result = comparison(&plain, &turned);
    assert_eq!(result.verdict(), Verdict::Disagree);
    let report = measure(&plain, &Selection::default());
    let bbox = report.bodies()[0].bbox().measured().expect("body box");
    assert_eq!(bbox.value().kind, BoxKind::Conservative);
    assert!(bbox.value().boundary.is_some());
    assert_eq!(result.unmatched_a().len(), 1);
    assert_eq!(result.unmatched_b().len(), 1);
    let row = result
        .document()
        .iter()
        .find(|row| row.quantity() == "bbox")
        .expect("document box row");
    assert_eq!(row.rule(), Rule::Containment);
    assert_eq!(row.agrees(), Some(false));
    let violation = row.difference().expect("violation");
    assert!((violation - 0.5).abs() <= 1.0e-9, "{violation}");
    assert_eq!(comparison(&plain, &plain).verdict(), Verdict::Agree);
}
