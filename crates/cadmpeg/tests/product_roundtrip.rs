// SPDX-License-Identifier: Apache-2.0
//! Cross-codec product-structure regression tests.

#![allow(clippy::unwrap_used)]

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Cursor;

use cadmpeg_codec_freecad::FcstdCodec;
use cadmpeg_ir::codec::{CodecEntry, DecodeOptions, EncodeInput};
use cadmpeg_ir::products::{AssemblyGraph, Occurrence, OccurrenceParent, PrototypeReference};
use cadmpeg_ir::{CadIr, Encoder};
use cadmpeg_step::StepCodec;

const CORE_DESIGN_PRODUCT: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../corpus/freecad_fcstd/fixtures/core_design_product.FCStd"
));

fn definition_names(ir: &CadIr) -> HashMap<&str, String> {
    ir.model
        .product_definitions
        .iter()
        .map(|definition| {
            (
                definition.id.0.as_str(),
                definition
                    .part_number
                    .as_ref()
                    .or(definition.source_name.as_ref())
                    .or(definition.label.as_ref())
                    .expect("definition has a stable product name")
                    .clone(),
            )
        })
        .collect()
}

fn occurrence_paths(ir: &CadIr) -> BTreeMap<String, [[f64; 4]; 4]> {
    fn path(
        occurrence: &Occurrence,
        occurrences: &HashMap<&str, &Occurrence>,
        definitions: &HashMap<&str, String>,
        memo: &mut HashMap<String, String>,
    ) -> String {
        if let Some(path) = memo.get(occurrence.id.0.as_str()) {
            return path.clone();
        }
        let definition = match &occurrence.prototype {
            PrototypeReference::Local { definition } => definitions
                .get(definition.0.as_str())
                .expect("local prototype resolves"),
            _ => panic!("round-trip fixture contains only local prototypes"),
        };
        let segment = format!("{}:{definition}", occurrence.ordinal);
        let resolved = match &occurrence.parent {
            OccurrenceParent::Root => segment,
            OccurrenceParent::Occurrence { occurrence: parent } => format!(
                "{}/{}",
                path(
                    occurrences
                        .get(parent.0.as_str())
                        .expect("parent occurrence resolves"),
                    occurrences,
                    definitions,
                    memo,
                ),
                segment
            ),
        };
        memo.insert(occurrence.id.0.clone(), resolved.clone());
        resolved
    }

    let definitions = definition_names(ir);
    let occurrences = ir
        .model
        .occurrences
        .iter()
        .map(|occurrence| (occurrence.id.0.as_str(), occurrence))
        .collect::<HashMap<_, _>>();
    let graph = AssemblyGraph::new(&ir.model.occurrences).expect("valid assembly graph");
    let mut memo = HashMap::new();
    ir.model
        .occurrences
        .iter()
        .map(|occurrence| {
            (
                path(occurrence, &occurrences, &definitions, &mut memo),
                graph
                    .resolved_transform(&occurrence.id)
                    .expect("resolved transform")
                    .rows,
            )
        })
        .collect()
}

#[test]
fn fcstd_assembly_round_trips_through_step_without_losing_its_tree() {
    let mut source = FcstdCodec
        .decode(
            &mut Cursor::new(CORE_DESIGN_PRODUCT),
            &DecodeOptions::default(),
        )
        .expect("decode FCStd assembly")
        .ir;

    let assembly_root = source
        .model
        .occurrences
        .iter()
        .find(|occurrence| {
            matches!(occurrence.parent, OccurrenceParent::Root)
                && matches!(
                    &occurrence.prototype,
                    PrototypeReference::Local { definition }
                        if source.model.product_definitions.iter().any(|candidate| {
                            candidate.id == *definition
                                && candidate.source_name.as_deref() == Some("Product")
                        })
                )
        })
        .expect("Product assembly root")
        .id
        .clone();
    let mut retained = HashSet::from([assembly_root.clone()]);
    loop {
        let before = retained.len();
        for occurrence in &source.model.occurrences {
            if matches!(
                &occurrence.parent,
                OccurrenceParent::Occurrence { occurrence: parent }
                    if retained.contains(parent)
            ) {
                retained.insert(occurrence.id.clone());
            }
        }
        if retained.len() == before {
            break;
        }
    }
    source
        .model
        .occurrences
        .retain(|occurrence| retained.contains(&occurrence.id));
    source
        .model
        .occurrences
        .iter_mut()
        .find(|occurrence| occurrence.id == assembly_root)
        .expect("retained root")
        .ordinal = 0;

    assert_eq!(
        source
            .model
            .product_definitions
            .iter()
            .map(|definition| definition.bodies.len())
            .sum::<usize>(),
        source.model.bodies.len(),
        "each FCStd body belongs to its source product definition"
    );
    let expected_definitions = definition_names(&source)
        .into_values()
        .collect::<HashSet<_>>();
    let expected_occurrences = occurrence_paths(&source);
    assert_eq!(expected_definitions.len(), 6);
    assert_eq!(expected_occurrences.len(), 6);

    let mut step = Vec::new();
    StepCodec::default()
        .plan(EncodeInput {
            ir: &source,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut step))
        .expect("write STEP assembly");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(step), &DecodeOptions::default())
        .expect("decode STEP assembly")
        .ir;

    assert_eq!(
        definition_names(&decoded)
            .into_values()
            .collect::<HashSet<_>>(),
        expected_definitions
    );
    let actual_occurrences = occurrence_paths(&decoded);
    assert_eq!(
        actual_occurrences.keys().collect::<Vec<_>>(),
        expected_occurrences.keys().collect::<Vec<_>>()
    );
    for (path, expected) in expected_occurrences {
        let actual = actual_occurrences.get(&path).expect("same occurrence path");
        for row in 0..4 {
            for column in 0..4 {
                assert!(
                    (actual[row][column] - expected[row][column]).abs() <= 1.0e-9,
                    "resolved transform differs at {path}[{row}][{column}]: expected {}, got {}",
                    expected[row][column],
                    actual[row][column]
                );
            }
        }
    }
}
