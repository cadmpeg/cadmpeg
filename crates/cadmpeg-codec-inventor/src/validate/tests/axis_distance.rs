// SPDX-License-Identifier: Apache-2.0
//! Validation of distance dimensions measured to an axis line.

use super::{empty_data, TestResult};
use cadmpeg_ir::report::check::Finding;

const SEGMENT: &str = "generated";

fn list() -> serde_json::Value {
    serde_json::json!({"marker": 2, "metadata": null, "references": []})
}

fn header() -> serde_json::Value {
    let reference = serde_json::json!({"index": 3, "qualified": true});
    serde_json::json!({
        "header_value": 0, "header_id": 0, "next": reference, "flags": 0,
        "context": reference, "source_index": 0
    })
}

fn line(ordinal: u32, entity_flags: u32) -> TestResult<crate::sketch::PmDcSketchEntity> {
    Ok(serde_json::from_value(serde_json::json!({
        "id": format!("inventor:pmdc:sketch-entity#{SEGMENT}-{ordinal}"),
        "entity_flags": entity_flags,
        "header": header(),
        "kind": {
            "form": "line", "points": list(), "auxiliary": [],
            "origin": [0.0, 0.0], "direction": [1.0, 0.0]
        },
        "record_ordinal": ordinal,
        "save_version_major": 19,
        "segment_token": SEGMENT,
        "sketch": {"index": 1, "qualified": false},
        "type_id": "55df52ced011d0d20008ccbc0663dc09"
    }))?)
}

fn distance(ordinal: u32, operands: [u32; 2]) -> TestResult<crate::sketch::PmDcSketchConstraint> {
    let reference = |index: u32| serde_json::json!({"index": index, "qualified": false});
    Ok(serde_json::from_value(serde_json::json!({
        "id": format!("inventor:pmdc:sketch-constraint#{SEGMENT}-{ordinal}"),
        "header": {
            "content": header(),
            "group": reference(0),
            "parameter": reference(5),
            "reference_map": {"entries": [], "metadata": null},
            "scalar_map": {"entries": [], "metadata": null},
            "state": -1
        },
        "kind": {
            "form": "distance",
            "first": reference(operands[0]),
            "second": reference(operands[1]),
            "value": 0,
            "values": [0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
        },
        "record_ordinal": ordinal,
        "save_version_major": 19,
        "segment_token": SEGMENT,
        "type_id": "58850511d211e39560000cb38932edb0"
    }))?)
}

/// A neutral constraint on the distance record at `ordinal`, in the given
/// definition form.
fn neutral(
    ordinal: u32,
    definition: &serde_json::Value,
) -> TestResult<cadmpeg_ir::SketchConstraint> {
    Ok(serde_json::from_value(serde_json::json!({
        "id": format!("inventor:design:sketch-constraint#{SEGMENT}-{ordinal}"),
        "sketch": "inventor:design:sketch#generated-1",
        "definition": definition,
        "native_ref": format!("inventor:pmdc:sketch-constraint#{SEGMENT}-{ordinal}")
    }))?)
}

fn entity_id(ordinal: u32) -> String {
    format!("inventor:design:sketch-entity#{SEGMENT}-{ordinal}")
}

fn findings(
    flags: [u32; 2],
    definition: impl Fn(u32, u32) -> serde_json::Value,
) -> TestResult<Vec<Finding>> {
    let mut data = empty_data();
    data.pm_dc_sketch_entities = vec![line(7, flags[0])?, line(3, flags[1])?];
    data.pm_dc_sketch_constraints = vec![distance(11, [8, 4])?];
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model
        .sketch_constraints
        .push(neutral(11, &definition(7, 3))?);
    let mut findings = Vec::new();
    super::super::validate_sketches(&data, &ir, &mut findings);
    // Entity and sketch link findings do not depend on the constraint form.
    findings.retain(|finding| finding.message.contains("distance dimension"));
    Ok(findings)
}

fn loci(first: u32, second: u32) -> serde_json::Value {
    serde_json::json!({
        "kind": "distance_loci",
        "first": {"kind": "entity", "entity": entity_id(first)},
        "second": {"kind": "entity", "entity": entity_id(second)},
        "parameter": "inventor:design:parameter#generated-4"
    })
}

fn native(first: u32, second: u32) -> serde_json::Value {
    serde_json::json!({
        "kind": "native",
        "native_kind": "axis_distance_dimension",
        "entities": [entity_id(first), entity_id(second)],
        "parameter": "inventor:design:parameter#generated-4",
        "operands": [{
            "native_kind": "record_reference",
            "field": {"name": "first"},
            "object_index": 8,
            "native_ref": format!("inventor:pmdc:sketch-entity#{SEGMENT}-7")
        }]
    })
}

#[test]
fn a_neutral_distance_to_an_axis_line_is_rejected() -> TestResult {
    assert_eq!(findings([0x8000, 0], loci)?.len(), 1);
    assert_eq!(findings([0, 0x8000], loci)?.len(), 1);
    assert_eq!(findings([0x8000 | 0x8_0000, 0], loci)?.len(), 1);
    Ok(())
}

#[test]
fn other_distances_and_the_native_form_are_admitted() -> TestResult {
    assert!(findings([0, 0], loci)?.is_empty());
    assert!(findings([0x8_0000, 0x40], loci)?.is_empty());
    assert!(findings([0x8000, 0], native)?.is_empty());
    Ok(())
}
