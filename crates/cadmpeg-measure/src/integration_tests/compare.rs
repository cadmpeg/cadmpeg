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
use crate::test_support::builders::{measure, merged, renamed, Builder};
use crate::value::{BoxKind, Quantity};

fn cube() -> CadIr {
    cadmpeg_ir::examples::unit_cube().expect("fixture")
}

fn comparison(a: &CadIr, b: &CadIr) -> Comparison {
    comparison_with(a, b, CompareOptions::default())
}

fn comparison_with(a: &CadIr, b: &CadIr, options: CompareOptions) -> Comparison {
    let report_a = measure(a, &Selection::default());
    let report_b = measure(b, &Selection::default());
    compare(a, &report_a, b, &report_b, &options).expect("within the pair allowance")
}

fn translated(ir: &CadIr, x: f64) -> CadIr {
    let mut moved = ir.clone();
    let shift = FiniteVector3::new(Vector3::new(x, 0.0, 0.0)).expect("finite");
    moved.geometry.bodies[0].transform = Some(Transform::identity().with_translation(shift));
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
    assert_ne!(copy.geometry.bodies[0].id, cube.geometry.bodies[0].id);
    let result = comparison(&cube, &copy);
    assert_eq!(result.verdict(), Verdict::Agree);
    let [pair] = result.pairs() else {
        panic!("one pair");
    };
    assert_eq!(pair.a(), cube.geometry.bodies[0].id.as_str());
    assert_eq!(pair.b(), copy.geometry.bodies[0].id.as_str());
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
    assert_eq!(twice.geometry.bodies.len(), 2);
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

/// The unit cube as a part, placed by the root definition once at each of
/// `placements`, in that order.
fn placed_cubes(placements: &[Transform]) -> CadIr {
    use cadmpeg_ir::ids::{OccurrenceId, ProductDefinitionId};
    use cadmpeg_ir::products::{
        ExternalDocument, Occurrence, ProductDefinition, ProductDefinitionKind, Storage,
    };
    let mut cube = cube();
    let definition = ProductDefinitionId::mint("synthetic:cube:product#1").expect("id");
    let mut part = ProductDefinition::new(definition.clone(), Storage::Embedded);
    part.kind = Some(ProductDefinitionKind::Part);
    part.bodies = vec![cube.geometry.bodies[0].id.clone()];
    let mut root = ProductDefinition::root(
        ProductDefinitionId::mint("synthetic:cube:product#root").expect("id"),
        ExternalDocument::default(),
    );
    for (at, transform) in placements.iter().enumerate() {
        let occurrence = OccurrenceId::mint(format!("synthetic:cube:occurrence#{at}")).expect("id");
        root.occurrences.push(occurrence.clone());
        cube.model.occurrences.push(Occurrence::new(
            occurrence,
            definition.clone(),
            Some(*transform),
        ));
    }
    cube.model.product_definitions.extend([part, root]);
    cube
}

fn shifted(x: f64, y: f64) -> Transform {
    Transform::identity()
        .with_translation(FiniteVector3::new(Vector3::new(x, y, 0.0)).expect("finite"))
}

fn document_row<'a>(result: &'a Comparison, quantity: &str) -> &'a crate::compare::Row {
    result
        .document()
        .iter()
        .find(|row| row.quantity() == quantity)
        .expect("document row")
}

#[test]
fn an_occurrence_that_moves_a_body_disagrees_with_the_body_in_place() {
    let plain = cube();
    let identity = placed_cubes(&[Transform::identity()]);
    assert_eq!(comparison(&plain, &identity).verdict(), Verdict::Agree);
    let moved = placed_cubes(&[shifted(15.0, 10.0)]);
    let result = comparison(&plain, &moved);
    assert_eq!(result.verdict(), Verdict::Disagree);
    let occurrences = document_row(&result, "occurrences");
    assert_eq!(
        occurrences.b(),
        Some(&Operand::Text("occurrences".to_owned()))
    );
    assert_eq!(occurrences.agrees(), None);
    let placement = document_row(&result, "instance_placement");
    assert_eq!(placement.agrees(), Some(false));
    let distance = placement.difference().expect("one instance on each side");
    assert!((distance - 325.0_f64.sqrt()).abs() < 1e-9);
}

#[test]
fn instances_of_a_reused_part_match_in_any_order() {
    let first = placed_cubes(&[shifted(5.0, 0.0), shifted(0.0, 7.0)]);
    let second = placed_cubes(&[shifted(0.0, 7.0), shifted(5.0, 0.0)]);
    let result = comparison(&first, &second);
    assert_eq!(result.verdict(), Verdict::Agree, "{result:#?}");
    assert_eq!(
        document_row(&result, "instances").a(),
        Some(&Operand::Count(2))
    );
    assert_eq!(
        document_row(&result, "instance_placement").agrees(),
        Some(true)
    );
    let moved = placed_cubes(&[shifted(0.0, 7.0), shifted(5.0, 1.0)]);
    let result = comparison(&first, &moved);
    assert_eq!(result.verdict(), Verdict::Disagree);
    let placement = document_row(&result, "instance_placement");
    assert!((placement.difference().expect("matched counts") - 1.0).abs() < 1e-9);
}

#[test]
fn a_nested_override_places_its_instance() {
    use cadmpeg_ir::design::selection::{OccurrencePath, PathStep, StepRef};
    use cadmpeg_ir::ids::{OccurrenceId, ProductDefinitionId};
    use cadmpeg_ir::products::{NestedOverride, Occurrence, ProductDefinition, Storage};
    // The root places a subassembly at the identity; the subassembly
    // places the part at x = 5, which an override of the subassembly
    // occurrence moves to x = 9.
    let nested = |override_x: Option<f64>| {
        let mut ir = placed_cubes(&[Transform::identity()]);
        let part = ir.model.occurrences[0].prototype.clone();
        let inner = OccurrenceId::mint("synthetic:cube:occurrence#inner").expect("id");
        let assembly_id = ProductDefinitionId::mint("synthetic:cube:product#asm").expect("id");
        let mut assembly = ProductDefinition::new(assembly_id.clone(), Storage::Embedded);
        assembly.occurrences.push(inner.clone());
        ir.model.occurrences.push(Occurrence::new(
            inner.clone(),
            part,
            Some(shifted(5.0, 0.0)),
        ));
        ir.model.occurrences[0].prototype = assembly_id;
        if let Some(x) = override_x {
            ir.model.occurrences[0].overrides.push(NestedOverride {
                path: OccurrencePath(vec![PathStep {
                    occurrence: StepRef::Occurrence(inner),
                    configuration: None,
                }]),
                transform: Some(shifted(x, 0.0)),
                suppressed: None,
                visible: None,
                representation: None,
            });
        }
        ir.model.product_definitions.push(assembly);
        ir
    };
    let overridden = nested(Some(9.0));
    assert_eq!(
        comparison(&overridden, &placed_cubes(&[shifted(9.0, 0.0)])).verdict(),
        Verdict::Agree
    );
    assert_eq!(
        comparison(&nested(None), &placed_cubes(&[shifted(5.0, 0.0)])).verdict(),
        Verdict::Agree
    );
}

/// The bilinear NURBS patch `(x, y) = (2u, v)` on `[0, 1]²` at `z = 0`,
/// bounded by its four domain edges: a 2 × 1 rectangle whose box is the
/// conservative control hull.
pub(super) fn nurbs_rectangle(transform: Option<Transform>) -> CadIr {
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
    ir.geometry.bodies[0].transform = transform;
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

/// Forty cubes 100 mm apart along `x` in each document. Only the two
/// copies of one cube overlap, so the comparison evaluates forty pairs,
/// not the 3160 pairs of eighty bodies.
#[test]
fn disjoint_bodies_evaluate_only_overlapping_pairs() {
    let count = 40;
    let row = |prefix: &str| {
        let cube = cube();
        (0..count)
            .map(|index| {
                let copy = renamed(&cube, "synthetic:cube:", &format!("{prefix}{index}:"));
                translated(&copy, 100.0 * f64::from(index))
            })
            .reduce(|all, next| merged(&all, &next))
            .expect("cubes")
    };
    let (a, b) = (row("synthetic:a"), row("synthetic:b"));
    assert_eq!(a.geometry.bodies.len(), 40);
    let result = comparison(&a, &b);
    assert_eq!(result.verdict(), Verdict::Agree);
    assert_eq!(result.pairs().len(), 40);
    assert_eq!(result.work().used(), 40);
}
