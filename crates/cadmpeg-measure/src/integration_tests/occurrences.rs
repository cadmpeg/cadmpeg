// SPDX-License-Identifier: Apache-2.0
//! Rows in the instance space of an occurrence, with expectations from the
//! stated placements: a body is placed by its own transform first, then by
//! the occurrence chain from the root.

use cadmpeg_ir::ids::{OccurrenceId, ProductDefinitionId};
use cadmpeg_ir::products::{
    ExternalDocument, Occurrence, OccurrenceParent, ProductDefinition, ProductDefinitionKind,
    PrototypeReference,
};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;

use crate::reason::{Category, UnavailableReason};
use crate::report::{MeasureReport, RowPlacement};
use crate::select::Selection;
use crate::session::{MeasureError, MeasureInput, MeasureOptions, Measurer};
use crate::test_support::builders::{
    assert_measured, cuboid, service_context, TEST_ABSOLUTE_FLOOR,
};
use crate::value::Quantity;

fn occurrence_id(key: &str) -> OccurrenceId {
    OccurrenceId::mint(format!("test:model:occurrence#{key}")).expect("identity")
}

fn definition_id() -> ProductDefinitionId {
    ProductDefinitionId::mint("test:model:product-definition#block").expect("identity")
}

fn assembly_id() -> ProductDefinitionId {
    ProductDefinitionId::mint("test:model:product-definition#assembly").expect("identity")
}

fn definition(id: ProductDefinitionId, bodies: Vec<cadmpeg_ir::ids::BodyId>) -> ProductDefinition {
    ProductDefinition {
        id,
        kind: ProductDefinitionKind::Part,
        source_name: None,
        label: None,
        description: None,
        part_number: None,
        bom_properties: std::collections::BTreeMap::default(),
        bodies,
        native_ref: None,
    }
}

fn occurrence(key: &str, parent: Option<&str>, transform: [[f64; 4]; 3]) -> Occurrence {
    placing(definition_id(), key, parent, transform)
}

fn placing(
    prototype: ProductDefinitionId,
    key: &str,
    parent: Option<&str>,
    transform: [[f64; 4]; 3],
) -> Occurrence {
    Occurrence {
        id: occurrence_id(key),
        prototype: PrototypeReference::Local {
            definition: prototype,
        },
        parent: parent.map_or(OccurrenceParent::Root {}, |parent| {
            OccurrenceParent::Occurrence {
                occurrence: occurrence_id(parent),
            }
        }),
        ordinal: 0,
        transform: Some(Transform::affine(transform).expect("transform")),
        linked_prototype: None,
        name: None,
        visible: None,
        link: None,
        native_ref: None,
    }
}

/// The move by (100, 0, 0).
const MOVE_X: [[f64; 4]; 3] = [
    [1.0, 0.0, 0.0, 100.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
];

/// The turn of +z onto +x and +x onto −z, then the move by (0, 50, 0).
const TURN_AND_MOVE_Y: [[f64; 4]; 3] = [
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 1.0, 0.0, 50.0],
    [-1.0, 0.0, 0.0, 0.0],
];

/// The box from (0, 0, 0) to (2, 3, 4), its body moved by (0, 0, 5), in the
/// `block` definition. `single` places `block` at the root by (100, 0, 0).
/// `top` places the `assembly` definition, which lists no body, at the root
/// by (100, 0, 0); `inner` places `block` inside `top`, turning +z onto +x
/// and +x onto −z and moving by (0, 50, 0).
fn placed_block() -> CadIr {
    let mut ir = cuboid([0.0; 3], [2.0, 3.0, 4.0]);
    ir.model.bodies[0].transform = Transform::affine([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 5.0],
    ]);
    ir.model.product_definitions.push(definition(
        definition_id(),
        vec![ir.model.bodies[0].id.clone()],
    ));
    ir.model
        .product_definitions
        .push(definition(assembly_id(), Vec::new()));
    ir.model
        .occurrences
        .push(placing(assembly_id(), "top", None, MOVE_X));
    let mut single = occurrence("single", None, MOVE_X);
    single.ordinal = 1;
    ir.model.occurrences.push(single);
    ir.model
        .occurrences
        .push(occurrence("inner", Some("top"), TURN_AND_MOVE_Y));
    ir.finalize();
    ir
}

fn measure_in(ir: &CadIr, selection: &Selection) -> Result<MeasureReport, MeasureError> {
    let ctx = service_context();
    let input = MeasureInput {
        ir,
        fidelity: None,
        geometry_transferred: true,
    };
    Measurer::new(input, MeasureOptions::default()).measure(&ctx, selection)
}

fn in_occurrence(ir: &CadIr, key: &str) -> MeasureReport {
    measure_in(
        ir,
        &Selection {
            occurrence: Some(occurrence_id(key)),
            ..Selection::default()
        },
    )
    .expect("measured")
}

fn assert_point(quantity: &Quantity<crate::value::Point>, expected: [f64; 3]) {
    let measured = quantity.measured().expect("a measured point");
    let point = measured.value().value.get();
    for (value, expected) in [point.x, point.y, point.z].into_iter().zip(expected) {
        assert!(
            (value - expected).abs()
                <= measured.numeric_error().get() + TEST_ABSOLUTE_FLOOR * 100.0,
            "{value} vs {expected}"
        );
    }
}

/// `single`: the body's centroid (1, 1.5, 2) moves to (1, 1.5, 7), then to
/// (101, 1.5, 7); its box to (100, 0, 5)..(102, 3, 9).
///
/// `inner`: `top ∘ inner ∘ body` maps `p` to
/// `R(p + (0, 0, 5)) + (0, 50, 0) + (100, 0, 0)` with `R(x, y, z) = (z, y,
/// −x)`. The centroid (1, 1.5, 7)
/// turns to (7, 1.5, −1) and moves to (107, 51.5, −1); the box
/// x ∈ [0, 2], y ∈ [0, 3], z ∈ [5, 9] turns to x ∈ [5, 9], z ∈ [−2, 0] and
/// moves to (105, 50, −2)..(109, 53, 0).
///
/// `top`: its prototype lists no body; its subtree places the body through
/// `inner` by the same composition, so the rows are those of `inner`.
#[test]
fn rows_are_placed_by_the_body_transform_then_the_occurrence_chain() {
    let ir = placed_block();
    let model = measure_in(&ir, &Selection::default()).expect("measured");
    assert!(model.occurrence().is_none());
    let [body] = model.bodies().as_slice() else {
        panic!("one body row");
    };
    assert_eq!(*body.placement(), RowPlacement::Model {});
    assert_point(body.centroid().as_ref().expect("centroid"), [1.0, 1.5, 7.0]);

    for (key, prototype, centroid, low, high) in [
        (
            "single",
            definition_id(),
            [101.0, 1.5, 7.0],
            [100.0, 0.0, 5.0],
            [102.0, 3.0, 9.0],
        ),
        (
            "inner",
            definition_id(),
            [107.0, 51.5, -1.0],
            [105.0, 50.0, -2.0],
            [109.0, 53.0, 0.0],
        ),
        (
            "top",
            assembly_id(),
            [107.0, 51.5, -1.0],
            [105.0, 50.0, -2.0],
            [109.0, 53.0, 0.0],
        ),
    ] {
        let report = in_occurrence(&ir, key);
        let [body] = report.bodies().as_slice() else {
            panic!("one body row");
        };
        assert_eq!(
            *body.placement(),
            RowPlacement::Occurrence {
                occurrence: occurrence_id(key).as_str().to_owned()
            }
        );
        assert_measured(body.volume().as_ref().expect("volume"), 24.0, 24.0);
        assert_point(body.centroid().as_ref().expect("centroid"), centroid);
        let bbox = body.bbox().measured().expect("box").value();
        for (value, expected) in [
            bbox.min.get().x,
            bbox.min.get().y,
            bbox.min.get().z,
            bbox.max.get().x,
            bbox.max.get().y,
            bbox.max.get().z,
        ]
        .into_iter()
        .zip(low.into_iter().chain(high))
        {
            assert!(
                (value - expected).abs() <= TEST_ABSOLUTE_FLOOR * 100.0,
                "{key}"
            );
        }
        let block = report.occurrence().as_ref().expect("occurrence block");
        assert!(block.unavailable().is_none());
        assert_eq!(block.bodies(), &[ir.model.bodies[0].id.as_str().to_owned()]);
        assert_eq!(block.prototype().as_deref(), Some(prototype.as_str()));
    }
    let top = in_occurrence(&ir, "top");
    let [descendant] = top
        .occurrence()
        .as_ref()
        .expect("block")
        .descendants()
        .as_slice()
    else {
        panic!("one descendant");
    };
    assert_eq!(descendant.occurrence(), occurrence_id("inner").as_str());
    assert_eq!(descendant.parent(), occurrence_id("top").as_str());
    assert_eq!(
        descendant.bodies(),
        &[ir.model.bodies[0].id.as_str().to_owned()]
    );
    let expected = [
        [0.0, 0.0, 1.0, 100.0],
        [0.0, 1.0, 0.0, 50.0],
        [-1.0, 0.0, 0.0, 0.0],
    ];
    assert_eq!(descendant.transform(), &Some(expected));
    let report = in_occurrence(&ir, "inner");
    assert_eq!(
        report.occurrence().as_ref().expect("block").chain(),
        &[
            occurrence_id("top").as_str().to_owned(),
            occurrence_id("inner").as_str().to_owned()
        ]
    );
}

/// No row, no transform, and every document quantity unavailable with the
/// space's reason, which the document histogram counts once.
fn assert_no_rows(report: &MeasureReport, reason: UnavailableReason, entity: &str) {
    assert!(report.bodies().is_empty());
    assert!(report.faces().is_empty());
    let block = report.occurrence().as_ref().expect("occurrence block");
    assert!(block.transform().is_none());
    let unavailable = block.unavailable().as_ref().expect("unavailable space");
    assert_eq!(unavailable.reason(), reason);
    assert_eq!(unavailable.entity(), Some(entity));
    let document = report.document();
    let reasons = [
        document
            .area()
            .as_ref()
            .and_then(Quantity::unavailable)
            .map(crate::value::Unavailable::reason),
        document
            .bbox()
            .as_ref()
            .and_then(Quantity::unavailable)
            .map(crate::value::Unavailable::reason),
        document
            .closed_volume()
            .as_ref()
            .and_then(Quantity::unavailable)
            .map(crate::value::Unavailable::reason),
    ];
    assert_eq!(reasons, [Some(reason); 3]);
    let counted: usize = document
        .unavailable()
        .get(&unavailable.category())
        .and_then(|codes| codes.get(reason.code()))
        .copied()
        .unwrap_or_default();
    assert_eq!(counted, 1);
}

#[test]
fn a_scaled_occurrence_in_the_chain_gives_no_rows() {
    let mut ir = placed_block();
    let top = ir
        .model
        .occurrences
        .iter_mut()
        .find(|occurrence| occurrence.id == occurrence_id("top"))
        .expect("top");
    let scale = Transform::affine([
        [2.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ])
    .expect("scale");
    top.transform = top
        .transform
        .map(|transform| transform.compose(scale).expect("finite scaled placement"));
    let report = in_occurrence(&ir, "inner");
    assert_no_rows(
        &report,
        UnavailableReason::PlacementOccurrenceScale,
        occurrence_id("top").as_str(),
    );
    let category = report
        .occurrence()
        .as_ref()
        .and_then(|block| block.unavailable().as_ref())
        .map(crate::value::Unavailable::category);
    assert_eq!(category, Some(Category::Placement));
}

#[test]
fn an_external_prototype_gives_no_rows() {
    let mut ir = placed_block();
    let inner = ir
        .model
        .occurrences
        .iter_mut()
        .find(|occurrence| occurrence.id == occurrence_id("inner"))
        .expect("inner");
    inner.prototype = PrototypeReference::External {
        document: ExternalDocument::Missing {},
        object: None,
    };
    let report = in_occurrence(&ir, "inner");
    assert_no_rows(
        &report,
        UnavailableReason::PlacementPrototypeUnresolved,
        occurrence_id("inner").as_str(),
    );
}

#[test]
fn an_external_prototype_in_the_subtree_gives_no_rows() {
    let mut ir = placed_block();
    let inner = ir
        .model
        .occurrences
        .iter_mut()
        .find(|occurrence| occurrence.id == occurrence_id("inner"))
        .expect("inner");
    inner.prototype = PrototypeReference::Unresolved {};
    let report = in_occurrence(&ir, "top");
    assert_no_rows(
        &report,
        UnavailableReason::PlacementPrototypeUnresolved,
        occurrence_id("inner").as_str(),
    );
}

/// `top` places `block` twice, through `inner` and through a second child:
/// one row per body cannot hold both instances.
#[test]
fn a_body_placed_twice_in_the_subtree_gives_no_rows() {
    let mut ir = placed_block();
    let mut twin = occurrence("twin", Some("top"), MOVE_X);
    twin.ordinal = 1;
    ir.model.occurrences.push(twin);
    ir.finalize();
    let report = in_occurrence(&ir, "top");
    assert_no_rows(
        &report,
        UnavailableReason::PlacementOccurrenceBodyRepeated,
        ir.model.bodies[0].id.as_str(),
    );
}

/// An occurrence of a definition that lists no body and has no child has
/// no body to measure; its document quantities are not a measured zero.
#[test]
fn an_occurrence_with_no_body_in_its_subtree_gives_no_rows() {
    let mut ir = placed_block();
    let mut empty = placing(assembly_id(), "empty", None, MOVE_X);
    empty.ordinal = 2;
    ir.model.occurrences.push(empty);
    ir.finalize();
    let report = in_occurrence(&ir, "empty");
    assert_no_rows(
        &report,
        UnavailableReason::PlacementOccurrenceEmpty,
        occurrence_id("empty").as_str(),
    );
    let category = report
        .occurrence()
        .as_ref()
        .and_then(|block| block.unavailable().as_ref())
        .map(crate::value::Unavailable::category);
    assert_eq!(category, Some(Category::Undefined));
}

#[test]
fn a_chain_with_a_missing_parent_gives_no_rows() {
    let mut ir = placed_block();
    ir.model
        .occurrences
        .retain(|occurrence| occurrence.id != occurrence_id("top"));
    let report = in_occurrence(&ir, "inner");
    assert_no_rows(
        &report,
        UnavailableReason::PlacementOccurrenceChainBroken,
        occurrence_id("inner").as_str(),
    );
}

#[test]
fn selectors_outside_the_prototype_and_unknown_occurrences_are_refused() {
    let mut ir = cuboid([10.0; 3], [11.0; 3]);
    let other_body = ir.model.bodies[0].id.clone();
    let block = placed_block();
    let unknown = measure_in(
        &block,
        &Selection {
            occurrence: Some(occurrence_id("missing")),
            ..Selection::default()
        },
    );
    assert!(matches!(
        unknown,
        Err(MeasureError::UnknownEntity {
            arena: "model.occurrences",
            ..
        })
    ));
    ir.model.product_definitions = block.model.product_definitions.clone();
    ir.model.occurrences = block.model.occurrences.clone();
    ir.model
        .product_definitions
        .iter_mut()
        .find(|definition| definition.id == definition_id())
        .expect("block")
        .bodies =
        vec![cadmpeg_ir::ids::BodyId::mint("test:model:body#elsewhere").expect("identity")];
    let outside = measure_in(
        &ir,
        &Selection {
            bodies: vec![other_body],
            occurrence: Some(occurrence_id("top")),
            ..Selection::default()
        },
    );
    assert!(matches!(
        outside,
        Err(MeasureError::OutsideOccurrence { .. })
    ));
}
