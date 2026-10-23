// SPDX-License-Identifier: Apache-2.0
//! Golden inspect and decode snapshots over code-built packages.
//!
//! Inputs come from [`crate::test_support`] builders. Shared harness:
//! [`cadmpeg_test_support::golden`]. `UPDATE_GOLDEN=1` rewrites goldens only.

use std::io::Cursor;

use cadmpeg_core::decode::InspectOptions;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_test_support::golden::{snapshot_text, Branch, Harness};
use cadmpeg_test_support::EditableDecodeResult;

use crate::test_support::{assembly, assembly_structure, bracket_member, package};
use crate::PvzCodec;

const REGENERATE: &str = "UPDATE_GOLDEN=1 cargo test-fast golden";

fn harness() -> Harness {
    Harness::new(env!("CARGO_MANIFEST_DIR"), "pvz", REGENERATE)
}

fn branches() -> [Branch; 2] {
    [
        Branch::named("inspect", inspect_snapshot),
        Branch::named("decode", decode_snapshot),
    ]
}

/// A package whose structure member is the XML form of a two-component
/// assembly (§3.14).
fn xml_package() -> Vec<u8> {
    let structure = br#"<PV_FILE type="PVS" version="0302">
<section_index><internal_section type="2"/><internal_section type="3"/></section_index>
<section_structure>
  <component name="TOP.ASM">
    <component_instance index="1" translation="0.1,0,0" orientation="0,1,0,-1,0,0,0,0,1" id="1"/>
  </component>
  <component name="BRACKET.PRT">
    <shape_source file_name="bracket.ol" bbox="0,0,0,0.01,0.01,0.01"/>
  </component>
</section_structure>
<section_properties>
  <property_component_ref><property name="Source_file_name" value="top.asm"/></property_component_ref>
</section_properties>
</PV_FILE>"#;
    package(&[("model.pvs", structure), ("bracket.ol", &bracket_member())])
}

fn inputs() -> Vec<(String, Vec<u8>)> {
    let mut inputs: Vec<(String, Vec<u8>)> = ["0301", "0302", "0303", "0304", "0305"]
        .into_iter()
        .map(|digits| {
            (
                format!("assembly_pvs_{digits}"),
                assembly(&format!("PV-PVS{digits}")),
            )
        })
        .collect();
    inputs.push(("xml_structure".to_owned(), xml_package()));
    inputs.push((
        "structure_without_geometry".to_owned(),
        package(&[("model.pvs", &assembly_structure("PV-PVS0304"))]),
    ));
    inputs
}

fn inspect_snapshot(bytes: &[u8]) -> String {
    let value = match PvzCodec.inspect(&mut Cursor::new(bytes), &InspectOptions::default()) {
        Ok(summary) => serde_json::to_value(summary).expect("serialize inspect summary"),
        Err(error) => serde_json::json!({ "inspect_error": error.to_string() }),
    };
    snapshot_text(&value)
}

fn decode_snapshot(bytes: &[u8]) -> String {
    let value = match PvzCodec.decode(&mut Cursor::new(bytes), &DecodeOptions::default()) {
        Ok(result) => {
            let result = EditableDecodeResult::from(result);
            serde_json::json!({
                "ir": result.ir(),
                "report": result.report(),
                "source_fidelity": result.source_fidelity(),
            })
        }
        Err(error) => serde_json::json!({ "decode_error": error.to_string() }),
    };
    snapshot_text(&value)
}

#[test]
fn golden_snapshots_hold() {
    harness().check_inputs(&inputs(), &branches());
}

#[test]
fn golden_output_is_deterministic() {
    harness().check_determinism_inputs(&inputs(), &branches());
}
