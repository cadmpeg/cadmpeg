// SPDX-License-Identifier: Apache-2.0
//! Rows in the instance space of an occurrence, with expectations from the
//! stated placements: a body is placed by its own transform first, then by
//! the occurrence chain from the root.

use cadmpeg_ir::ids::{OccurrenceId, ProductDefinitionId};
use cadmpeg_ir::products::{
    ExternalDocument, Occurrence, OccurrenceParent, ProductDefinition, ProductDefinitionKind,
    PrototypeReference,
};
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;

use crate::reason::{Category, UnavailableReason};
use crate::report::{MeasureReport, RowPlacement};
use crate::select::Selection;
use crate::session::{MeasureError, MeasureInput, MeasureOptions, Measurer};
use crate::test_support::{assert_measured, cuboid, service_context, TEST_ABSOLUTE_FLOOR};
use crate::value::Quantity;

fn occurrence_id(key: &str) -> OccurrenceId {
    OccurrenceId::mint(format!("test:model:occurrence#{key}")).expect("identity")
}

fn definition_id() -> ProductDefinitionId {
    ProductDefinitionId::mint("test:model:product-definition#block").expect("identity")
}

fn occurrence(key: &str, parent: Option<&str>, transform: [[f64; 4]; 3]) -> Occurrence {
    Occurrence {
        id: occurrence_id(key),
        prototype: PrototypeReference::Local {
            definition: definition_id(),
        },
        parent: parent.map_or(OccurrenceParent::Root {}, |parent| {
            OccurrenceParent::Occurrence {
                occurrence: occurrence_id(parent),
            }
        }),
        ordinal: 0,
        transform: Transform::affine(transform).expect("transform"),
        linked_prototype: None,
        scale: [FiniteReal::ONE; 3],
        name: None,
        visible: None,
        link: None,
        native_ref: None,
    }
}

/// The box from (0, 0, 0) to (2, 3, 4), its body moved by (0, 0, 5), in a
/// definition placed by `top` at the root and `inner` inside it. `top`
/// moves by (100, 0, 0); `inner` turns +z onto +x and +x onto −z and moves
/// by (0, 50, 0).
fn placed_block() -> CadIr {
    let mut ir = cuboid([0.0; 3], [2.0, 3.0, 4.0]);
    ir.model.bodies[0].transform = Transform::affine([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 5.0],
    ]);
    ir.model.product_definitions.push(ProductDefinition {
        id: definition_id(),
        kind: ProductDefinitionKind::Part,
        source_name: None,
        label: None,
        description: None,
        part_number: None,
        bom_properties: std::collections::BTreeMap::default(),
        bodies: vec![ir.model.bodies[0].id.clone()],
        native_ref: None,
    });
    ir.model.occurrences.push(occurrence(
        "top",
        None,
        [
            [1.0, 0.0, 0.0, 100.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ],
    ));
    ir.model.occurrences.push(occurrence(
        "inner",
        Some("top"),
        [
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 1.0, 0.0, 50.0],
            [-1.0, 0.0, 0.0, 0.0],
        ],
    ));
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

/// `top`: the body's centroid (1, 1.5, 2) moves to (1, 1.5, 7), then to
/// (101, 1.5, 7); its box to (100, 0, 5)..(102, 3, 9).
///
/// `inner`: `top ∘ inner ∘ body` maps `p` to
/// `R(p + (0, 0, 5)) + (0, 50, 0) + (100, 0, 0)` with `R(x, y, z) = (z, y,
/// −x)`. The centroid (1, 1.5, 7)
/// turns to (7, 1.5, −1) and moves to (107, 51.5, −1); the box
/// x ∈ [0, 2], y ∈ [0, 3], z ∈ [5, 9] turns to x ∈ [5, 9], z ∈ [−2, 0] and
/// moves to (105, 50, −2)..(109, 53, 0).
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

    for (key, centroid, low, high) in [
        (
            "top",
            [101.0, 1.5, 7.0],
            [100.0, 0.0, 5.0],
            [102.0, 3.0, 9.0],
        ),
        (
            "inner",
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
        assert_eq!(block.prototype().as_deref(), Some(definition_id().as_str()));
    }
    let report = in_occurrence(&ir, "inner");
    assert_eq!(
        report.occurrence().as_ref().expect("block").chain(),
        &[
            occurrence_id("top").as_str().to_owned(),
            occurrence_id("inner").as_str().to_owned()
        ]
    );
}

fn assert_no_rows(report: &MeasureReport, reason: UnavailableReason, entity: &str) {
    assert!(report.bodies().is_empty());
    assert!(report.faces().is_empty());
    let unavailable = report
        .occurrence()
        .as_ref()
        .and_then(|block| block.unavailable().as_ref())
        .expect("unavailable space");
    assert_eq!(unavailable.reason(), reason);
    assert_eq!(unavailable.entity(), Some(entity));
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
    top.scale = [
        FiniteReal::new(2.0).expect("scale"),
        FiniteReal::ONE,
        FiniteReal::ONE,
    ];
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
    ir.model.product_definitions[0].bodies =
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
