// SPDX-License-Identifier: Apache-2.0
//! Root ownership of bodies without component assignments.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::products::{
    Occurrence, OccurrenceParent, ProductDefinition, ProductDefinitionKind, PrototypeReference,
};
use cadmpeg_ir::topology::BodyKind;
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;
use cadmpeg_measure::test_support::round_trip::measures_agree;
use cadmpeg_test_support::polyhedron::{box_faces, polyhedron};

use crate::export::write_step;
use crate::loss::StepLossCode;
use crate::parse::{Exchange, Value};
use crate::{StepCodec, StepSchema, StepWriteOptions};

fn product(name: &str, bodies: Vec<BodyId>) -> ProductDefinition {
    ProductDefinition {
        id: format!("test:assembly:product-definition#{name}")
            .try_into()
            .expect("product identity"),
        kind: ProductDefinitionKind::Part,
        source_name: None,
        label: Some(name.into()),
        description: None,
        part_number: Some(name.into()),
        bom_properties: BTreeMap::new(),
        bodies,
        sketches: Vec::default(),
        native_ref: None,
    }
}

fn occurrence(name: &str, parent: Option<&str>, x: f64) -> Occurrence {
    Occurrence {
        id: format!("test:assembly:occurrence#{name}")
            .try_into()
            .expect("occurrence identity"),
        prototype: PrototypeReference::Local {
            definition: product(name, Vec::new()).id,
        },
        parent: parent.map_or(OccurrenceParent::Root {}, |parent| {
            OccurrenceParent::Occurrence {
                occurrence: format!("test:assembly:occurrence#{parent}")
                    .try_into()
                    .expect("parent identity"),
            }
        }),
        ordinal: 0,
        transform: Some(translation(x)),
        linked_prototype: None,
        name: Some(name.into()),
        visible: None,
        link: None,
        native_ref: None,
    }
}

fn translation(x: f64) -> Transform {
    Transform::affine([
        [1.0, 0.0, 0.0, x],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ])
    .expect("rigid translation")
}

fn assembly(claim_first: bool) -> CadIr {
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("cube");
    let second = polyhedron(
        &[
            [20.0, 0.0, 0.0],
            [23.0, 0.0, 0.0],
            [23.0, 3.0, 0.0],
            [20.0, 3.0, 0.0],
            [20.0, 0.0, 3.0],
            [23.0, 0.0, 3.0],
            [23.0, 3.0, 3.0],
            [20.0, 3.0, 3.0],
        ],
        &box_faces([0, 1, 2, 3, 4, 5, 6, 7])
            .iter()
            .map(<[usize; 4]>::as_slice)
            .collect::<Vec<_>>(),
    )
    .expect("second cube");
    ir.model.bodies.extend(second.model.bodies);
    ir.model.regions.extend(second.model.regions);
    ir.model.shells.extend(second.model.shells);
    ir.model.faces.extend(second.model.faces);
    ir.model.loops.extend(second.model.loops);
    ir.model.coedges.extend(second.model.coedges);
    ir.model.edges.extend(second.model.edges);
    ir.model.vertices.extend(second.model.vertices);
    ir.model.points.extend(second.model.points);
    ir.model.curves.extend(second.model.curves);
    ir.model.surfaces.extend(second.model.surfaces);
    ir.model.product_definitions = vec![
        product(
            "first",
            if claim_first {
                vec![ir.model.bodies[0].id.clone()]
            } else {
                Vec::new()
            },
        ),
        product("root", Vec::new()),
        product("second", Vec::new()),
    ];
    ir.model.occurrences = vec![
        occurrence("root", None, 0.0),
        occurrence("first", Some("root"), 100.0),
        occurrence("second", Some("root"), 200.0),
    ];
    ir
}

fn reference(value: &Value) -> u64 {
    let Value::Reference(id) = value else {
        panic!("entity reference");
    };
    *id
}

fn parameters<'a>(exchange: &'a Exchange, id: u64, name: &str) -> &'a [Value] {
    &exchange.records()[&id]
        .partials
        .iter()
        .find(|partial| partial.name == name)
        .expect("entity type")
        .parameters
}

fn entities(exchange: &Exchange, name: &str) -> Vec<u64> {
    exchange
        .records()
        .iter()
        .filter(|(_, record)| record.partials.iter().any(|partial| partial.name == name))
        .map(|(&id, _)| id)
        .collect()
}

fn product_representation(exchange: &Exchange, name: &str) -> u64 {
    let definition = entities(exchange, "PRODUCT_DEFINITION")
        .into_iter()
        .find(|&id| {
            parameters(exchange, id, "PRODUCT_DEFINITION")[0]
                == Value::String(name.as_bytes().to_vec())
        })
        .expect("product definition");
    let shape = entities(exchange, "PRODUCT_DEFINITION_SHAPE")
        .into_iter()
        .find(|&id| {
            reference(&parameters(exchange, id, "PRODUCT_DEFINITION_SHAPE")[2]) == definition
        })
        .expect("product shape");
    entities(exchange, "SHAPE_DEFINITION_REPRESENTATION")
        .into_iter()
        .find_map(|id| {
            let params = parameters(exchange, id, "SHAPE_DEFINITION_REPRESENTATION");
            (reference(&params[0]) == shape).then(|| reference(&params[1]))
        })
        .expect("product representation")
}

fn reachable_solids(exchange: &Exchange, representation: u64) -> BTreeSet<u64> {
    let mut pending = vec![representation];
    let mut visited = BTreeSet::new();
    let mut solids = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !visited.insert(id) {
            continue;
        }
        for partial in &exchange.records()[&id].partials {
            match partial.name.as_str() {
                "MANIFOLD_SOLID_BREP" | "BREP_WITH_VOIDS" => {
                    solids.insert(id);
                }
                "SHAPE_REPRESENTATION" | "ADVANCED_BREP_SHAPE_REPRESENTATION" => {
                    let Value::List(items) = &partial.parameters[1] else {
                        panic!("representation items");
                    };
                    pending.extend(items.iter().map(reference));
                }
                "MAPPED_ITEM" => pending.push(reference(&partial.parameters[1])),
                "REPRESENTATION_MAP" => pending.push(reference(&partial.parameters[1])),
                _ => {}
            }
        }
        for relationship in entities(exchange, "SHAPE_REPRESENTATION_RELATIONSHIP") {
            let record = &exchange.records()[&relationship];
            if record.partials.len() != 1 {
                continue;
            }
            let params = parameters(exchange, relationship, "SHAPE_REPRESENTATION_RELATIONSHIP");
            let (left, right) = (reference(&params[2]), reference(&params[3]));
            if left == id {
                pending.push(right);
            } else if right == id {
                pending.push(left);
            }
        }
    }
    solids
}

fn assert_assembly(claim_first: bool, body_placement: bool) {
    let mut ir = assembly(claim_first);
    if body_placement {
        ir.model.bodies[1].transform = Some(translation(40.0));
    }
    for schema in [
        StepSchema::Ap203Edition1,
        StepSchema::Ap214,
        StepSchema::Ap242Edition3,
    ] {
        let mut bytes = Vec::new();
        let report = write_step(&ir, &mut bytes, schema, &StepWriteOptions::default())
            .expect("write assembly");
        let note = report
            .losses
            .iter()
            .find(|note| note.code == StepLossCode::BodyWithoutProductRepresentation.kind())
            .expect("root assignment note");
        assert_eq!(note.message, format!(
            "{} body record(s) not assigned to a component were written in the root product in model coordinates",
            if claim_first { 1 } else { 2 }
        ));
        let (exchange, _) =
            crate::test_support::with_service_context(&bytes, crate::parse::parse_inner)
                .expect("parse emitted assembly");
        let solids = entities(&exchange, "MANIFOLD_SOLID_BREP")
            .into_iter()
            .collect::<BTreeSet<_>>();
        assert_eq!(solids.len(), 2);
        assert_eq!(
            entities(&exchange, "NEXT_ASSEMBLY_USAGE_OCCURRENCE").len(),
            2
        );
        let root = reachable_solids(&exchange, product_representation(&exchange, "root"));
        let first = reachable_solids(&exchange, product_representation(&exchange, "first"));
        let second = reachable_solids(&exchange, product_representation(&exchange, "second"));
        assert_eq!(root.len(), if claim_first { 1 } else { 2 });
        assert_eq!(first.len(), usize::from(claim_first));
        assert!(second.is_empty());
        assert!(root.is_disjoint(&first));
        assert_eq!(root.union(&first).copied().collect::<BTreeSet<_>>(), solids);
        let mut shape_items = solids.clone();
        shape_items.extend(entities(&exchange, "MAPPED_ITEM"));
        for shape_item in &shape_items {
            let references = exchange
                .records()
                .values()
                .flat_map(|record| &record.partials)
                .filter(|partial| {
                    matches!(
                        partial.name.as_str(),
                        "SHAPE_REPRESENTATION" | "ADVANCED_BREP_SHAPE_REPRESENTATION"
                    )
                })
                .map(|partial| match &partial.parameters[1] {
                    Value::List(items) => items
                        .iter()
                        .filter(|item| **item == Value::Reference(*shape_item))
                        .count(),
                    _ => panic!("representation items"),
                })
                .sum::<usize>();
            assert_eq!(references, 1, "each body shape item is represented once");
        }
        let decoded = StepCodec::default()
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("read assembly");
        assert_eq!(decoded.ir().model.bodies.len(), 2);
        assert!(decoded
            .ir()
            .model
            .bodies
            .iter()
            .all(|body| body.kind == BodyKind::Solid));
        assert_eq!(decoded.ir().model.occurrences.len(), 3);
        for (name, count) in [("root", root.len()), ("first", first.len()), ("second", 0)] {
            let product = decoded
                .ir()
                .model
                .product_definitions
                .iter()
                .find(|product| product.part_number.as_deref() == Some(name))
                .expect("decoded product");
            assert_eq!(product.bodies.len(), count, "{name} body ownership");
        }
        let mut expected = ir.clone();
        let mut actual = decoded.ir().clone();
        for (name, x) in [("first", 100.0), ("second", 200.0)] {
            let occurrence = actual
                .model
                .occurrences
                .iter()
                .find(|occurrence| occurrence.name.as_deref() == Some(name))
                .expect("decoded child occurrence");
            assert_eq!(occurrence.transform, Some(translation(x)));
        }
        expected.model.occurrences.clear();
        actual.model.occurrences.clear();
        measures_agree("assembly body assignment", &expected, &actual)
            .expect("stored body geometry and placements survive");
    }
}

#[test]
fn unclaimed_assembly_bodies_are_reachable_once_from_the_root() {
    assert_assembly(false, false);
}

#[test]
fn mixed_assembly_body_ownership_keeps_the_claimed_body_in_its_component() {
    assert_assembly(true, false);
}

#[test]
fn unclaimed_assembly_bodies_retain_body_placements_once() {
    assert_assembly(false, true);
}

#[test]
fn unclaimed_bodies_without_occurrences_keep_a_product_representation() {
    for sole_product in [true, false] {
        let mut ir = assembly(false);
        ir.model.occurrences.clear();
        if sole_product {
            ir.model
                .product_definitions
                .retain(|product| product.label.as_deref() == Some("root"));
        }
        let mut bytes = Vec::new();
        write_step(
            &ir,
            &mut bytes,
            StepSchema::Ap242Edition3,
            &StepWriteOptions::default(),
        )
        .expect("write bodies without a root occurrence");
        let decoded = StepCodec::default()
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("read bodies without a root occurrence");
        assert_eq!(decoded.ir().model.bodies.len(), 2);
        let products = &decoded.ir().model.product_definitions;
        assert_eq!(products.len(), if sole_product { 1 } else { 4 });
        let owners = products
            .iter()
            .filter(|product| !product.bodies.is_empty())
            .collect::<Vec<_>>();
        assert_eq!(owners.len(), 1);
        assert_eq!(owners[0].bodies.len(), 2);
        if sole_product {
            assert_eq!(owners[0].part_number.as_deref(), Some("root"));
        }
    }
}

fn assert_solid_representation_membership(exchange: &Exchange, solid_count: usize) {
    let advanced = entities(exchange, "ADVANCED_BREP_SHAPE_REPRESENTATION")
        .into_iter()
        .collect::<BTreeSet<_>>();
    let solids = entities(exchange, "MANIFOLD_SOLID_BREP")
        .into_iter()
        .chain(entities(exchange, "BREP_WITH_VOIDS"))
        .collect::<BTreeSet<_>>();
    assert_eq!(solids.len(), solid_count);
    let mut represented_solids = BTreeMap::<u64, usize>::new();
    for &representation in &advanced {
        let Value::List(items) = &parameters(
            exchange,
            representation,
            "ADVANCED_BREP_SHAPE_REPRESENTATION",
        )[1] else {
            panic!("representation items");
        };
        let mut has_shape = false;
        for item in items.iter().map(reference) {
            let record = &exchange.records()[&item];
            if solids.contains(&item) {
                *represented_solids.entry(item).or_default() += 1;
                has_shape = true;
            } else if record
                .partials
                .iter()
                .any(|partial| partial.name == "MAPPED_ITEM")
            {
                let map = reference(&parameters(exchange, item, "MAPPED_ITEM")[1]);
                let source = reference(&parameters(exchange, map, "REPRESENTATION_MAP")[1]);
                assert!(
                    advanced.contains(&source),
                    "mapped advanced B-rep source has the same subtype"
                );
                has_shape = true;
            } else {
                assert!(record
                    .partials
                    .iter()
                    .any(|partial| partial.name == "AXIS2_PLACEMENT_3D"));
            }
        }
        assert!(
            has_shape,
            "advanced B-rep representations contain a solid or mapped solid"
        );
    }
    assert_eq!(represented_solids.len(), solids.len());
    assert!(represented_solids.values().all(|&count| count == 1));
}

#[test]
fn solids_keep_advanced_representations_across_product_and_body_placements() {
    for schema in [
        StepSchema::Ap203Edition1,
        StepSchema::Ap214,
        StepSchema::Ap242Edition3,
    ] {
        for (graph, claimed) in [(false, false), (true, false), (true, true)] {
            for placed in [false, true] {
                for standalone in [false, true] {
                    let mut ir = assembly(claimed);
                    if !graph {
                        ir.model.product_definitions.clear();
                        ir.model.occurrences.clear();
                    }
                    if placed {
                        ir.model.bodies[1].transform = Some(translation(40.0));
                    }
                    if standalone {
                        let mut point = ir.model.points[0].clone();
                        point.id =
                            cadmpeg_ir::ids::PointId::mint("test:representations:point#standalone")
                                .unwrap();
                        ir.model.points.push(point);
                    }
                    let mut bytes = Vec::new();
                    write_step(&ir, &mut bytes, schema, &StepWriteOptions::default()).unwrap();
                    let (exchange, _) = crate::test_support::with_service_context(
                        &bytes,
                        crate::parse::parse_inner,
                    )
                    .unwrap();
                    assert_solid_representation_membership(&exchange, 2);
                    let decoded = StepCodec::default()
                        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                        .unwrap();
                    assert_eq!(decoded.ir().model.bodies.len(), 2);
                    let mut actual = decoded.into_parts().0;
                    ir.model.product_definitions.clear();
                    ir.model.occurrences.clear();
                    actual.model.product_definitions.clear();
                    actual.model.occurrences.clear();
                    measures_agree("typed solid representations", &ir, &actual).unwrap();
                }
            }
        }
    }
}

#[test]
fn mixed_solid_and_sheet_geometry_uses_linked_representations() {
    for schema in [
        StepSchema::Ap203Edition1,
        StepSchema::Ap214,
        StepSchema::Ap242Edition3,
    ] {
        for graph in [false, true] {
            let mut ir = assembly(false);
            ir.model.bodies[1].kind = BodyKind::Sheet;
            if graph {
                ir.model.product_definitions[0].bodies =
                    ir.model.bodies.iter().map(|body| body.id.clone()).collect();
            } else {
                ir.model.product_definitions.clear();
                ir.model.occurrences.clear();
            }
            let mut bytes = Vec::new();
            write_step(&ir, &mut bytes, schema, &StepWriteOptions::default()).unwrap();
            let (exchange, _) =
                crate::test_support::with_service_context(&bytes, crate::parse::parse_inner)
                    .unwrap();
            assert_solid_representation_membership(&exchange, 1);
            assert_eq!(entities(&exchange, "SHELL_BASED_SURFACE_MODEL").len(), 1);
            assert!(!entities(&exchange, "SHAPE_REPRESENTATION_RELATIONSHIP").is_empty());
            let decoded = StepCodec::default()
                .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                .unwrap();
            assert_eq!(decoded.ir().model.bodies.len(), 2);
            assert_eq!(
                decoded
                    .ir()
                    .model
                    .bodies
                    .iter()
                    .filter(|body| body.kind == BodyKind::Solid)
                    .count(),
                1
            );
            if graph {
                let product = decoded
                    .ir()
                    .model
                    .product_definitions
                    .iter()
                    .find(|product| product.label.as_deref() == Some("first"))
                    .unwrap();
                assert_eq!(product.bodies.len(), 2);
            }
        }
    }
}

#[test]
fn omitted_solid_geometry_does_not_create_an_empty_advanced_representation() {
    for schema in [
        StepSchema::Ap203Edition1,
        StepSchema::Ap214,
        StepSchema::Ap242Edition3,
    ] {
        let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
        ir.model.regions.clear();
        ir.model.bodies[0].regions.clear();
        let mut bytes = Vec::new();
        write_step(&ir, &mut bytes, schema, &StepWriteOptions::default()).unwrap();
        let (exchange, _) =
            crate::test_support::with_service_context(&bytes, crate::parse::parse_inner).unwrap();
        assert_solid_representation_membership(&exchange, 0);
        assert!(entities(&exchange, "ADVANCED_BREP_SHAPE_REPRESENTATION").is_empty());
        assert_eq!(entities(&exchange, "SHAPE_REPRESENTATION").len(), 1);
    }
}
