// SPDX-License-Identifier: Apache-2.0
//! Assembly product, placement and mate admission.
use crate::test_support::{assembly::*, read_bytes, Report};
use cadmpeg_core::serde_erase::json;
use serde_json::json;
use std::path::Path;

fn read(definition: serde_json::Value) -> (crate::reader::Model, Report) {
    let mut report = Report::default();
    let model = read_bytes(
        Path::new("synthetic.onshape-bundle"),
        &bundle(definition),
        &mut report,
    )
    .unwrap();
    let validation = cadmpeg_ir::validate::validate_neutral(&model.ir, Vec::new()).unwrap();
    assert!(
        !validation
            .findings
            .iter()
            .any(|finding| finding.severity == cadmpeg_ir::report::Severity::Error),
        "{:?}",
        validation.findings
    );
    (model, report)
}
#[test]
fn shared_part_instances_keep_transforms_and_fixed_state() {
    let (model, report) = read(definition());
    assert_eq!(model.ir.geometry.bodies.len(), 1);
    assert_eq!(model.ir.model.occurrences.len(), 2);
    let first = model
        .ir
        .model
        .occurrences
        .iter()
        .find(|o| o.name.as_deref() == Some("first"))
        .unwrap();
    let second = model
        .ir
        .model
        .occurrences
        .iter()
        .find(|o| o.name.as_deref() == Some("second"))
        .unwrap();
    assert_eq!(first.prototype, second.prototype);
    assert_eq!(
        first.transform.unwrap().affine_rows(),
        [[0., -1., 0., 10.], [1., 0., 0., 20.], [0., 0., 1., 0.]]
    );
    assert_eq!(
        second.transform.unwrap().affine_rows(),
        [[0., -1., 0., 30.], [1., 0., 0., 40.], [0., 0., 1., 0.]]
    );
    let prototype = model
        .ir
        .model
        .product_definitions
        .iter()
        .find(|p| p.id == first.prototype)
        .unwrap();
    assert_eq!(
        prototype.bodies,
        vec![model.ir.geometry.bodies[0].id.clone()],
        "{:?}",
        report.losses()
    );
    assert_eq!(model.ir.model.assembly_joints.len(), 1);
}
#[test]
fn nested_assembly_uses_parent_relative_placement() {
    const NESTED_TRANSLATION_TOLERANCE: f64 = 1e-12;
    let mut definition = definition();
    definition["rootAssembly"]["instances"] = json!([{"id":"sub","type":"Assembly","documentId":"document","elementId":"nested","configuration":"default"}]);
    definition["rootAssembly"]["occurrences"] = json!([
        {"path":["sub"],"transform":[1.,0.,0.,0.01,0.,1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.]},
        {"path":["sub","child"],"transform":[1.,0.,0.,0.03,0.,1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.]}
    ]);
    definition["subAssemblies"] = json!([{"documentId":"document","elementId":"nested","configuration":"default","instances":[part("child")]}]);
    let (model, _) = read(definition);
    assert_eq!(model.ir.model.occurrences.len(), 2);
    let child = model
        .ir
        .model
        .occurrences
        .iter()
        .find(|o| o.name.as_deref() == Some("child"))
        .unwrap();
    assert!(
        (child.transform.unwrap().affine_rows()[0][3] - 20.).abs() < NESTED_TRANSLATION_TOLERANCE
    );
}
#[test]
fn suppressed_instance_retains_its_product() {
    let mut definition = definition();
    definition["rootAssembly"]["instances"][1]["suppressed"] = json!(true);
    let (model, _) = read(definition);
    let second = model
        .ir
        .model
        .occurrences
        .iter()
        .find(|o| o.name.as_deref() == Some("second"))
        .unwrap();
    assert_eq!(second.suppressed, Some(true));
}
#[test]
fn mates_keep_evaluated_frames_and_report_unresolved_limits() {
    let mut definition = definition();
    let mut revolute = mate("revolute", "REVOLUTE");
    revolute["featureData"]["limits"] = json!({"minimum":-0.5,"maximum":1.0});
    definition["rootAssembly"]["features"] = json!([
        mate("fastened", "FASTENED"),
        revolute,
        mate("unsupported", "GEAR")
    ]);
    let (model, report) = read(definition);
    assert_eq!(model.ir.model.assembly_joints.len(), 3);
    let wire = json::to_value(&model.ir.model.assembly_joints).unwrap();
    let pair = wire
        .as_array()
        .unwrap()
        .iter()
        .find(|j| j["operands"]["value"]["kind"]["kind"] == "revolute")
        .unwrap();
    assert_eq!(
        pair["operands"]["value"]["connectors"][0]["frame"],
        json!([[1., 0., 0., 1.], [0., 1., 0., 2.], [0., 0., 1., 3.]])
    );
    assert!(report
        .losses()
        .iter()
        .any(|l| l.code.as_deref() == Some("onshape/assembly.mate-unsupported")));
    assert!(report
        .losses()
        .iter()
        .any(|l| l.code.as_deref() == Some("onshape/assembly.mate-limits-unresolved")));
}

#[test]
fn several_studios_have_distinct_geometry_identities() {
    let mut definition = definition();
    definition["rootAssembly"]["instances"][1]["elementId"] = json!("other");
    let mut other = definition["parts"][0].clone();
    other["elementId"] = json!("other");
    definition["parts"].as_array_mut().unwrap().push(other);
    let bytes = bundle_with_studios(definition, true);
    let mut report = Report::default();
    let model = read_bytes(Path::new("synthetic.onshape-bundle"), &bytes, &mut report).unwrap();
    assert_eq!(model.ir.geometry.bodies.len(), 2);
    assert_ne!(
        model.ir.geometry.bodies[0].id,
        model.ir.geometry.bodies[1].id
    );
    let first = model
        .ir
        .model
        .occurrences
        .iter()
        .find(|o| o.name.as_deref() == Some("first"))
        .unwrap();
    let second = model
        .ir
        .model
        .occurrences
        .iter()
        .find(|o| o.name.as_deref() == Some("second"))
        .unwrap();
    assert_ne!(first.prototype, second.prototype);
}

/// A record one studio drops leaves every record of another studio, also
/// where both studios' STEP text states the same instance labels.
#[test]
fn a_record_one_studio_drops_leaves_the_records_of_another_studio() {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use std::io::Cursor;
    let mut definition = definition();
    definition["rootAssembly"]["instances"][1]["elementId"] = json!("other");
    let mut other = definition["parts"][0].clone();
    other["elementId"] = json!("other");
    definition["parts"].as_array_mut().unwrap().push(other);
    let text = String::from_utf8(crate::test_support::step().to_vec()).unwrap();
    let unread = unreadable_plane_placement(&text);
    let bytes = bundle_with_other_studio(definition, Some(unread.as_bytes()));
    let result = crate::test_support::codec()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let unreadable = cadmpeg_ir::unknown::part::record_unreadable_kind();
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == unreadable));
    let ir = result.ir();
    assert!(
        ir.geometry
            .shells
            .iter()
            .any(|shell| shell.members.len() == 6),
        "{:#?}",
        ir.geometry.shells
    );
}

/// `text` with the placement of its first plane stating its location as
/// its axes, so the placement does not read and the plane drops with it.
fn unreadable_plane_placement(text: &str) -> String {
    let refs = |line: &str| {
        line.split('#')
            .skip(1)
            .map(|tail| {
                tail.chars()
                    .take_while(char::is_ascii_digit)
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
    };
    let plane = text
        .lines()
        .find(|line| line.contains("=PLANE("))
        .expect("a plane");
    let placement = refs(plane).last().cloned().expect("a placement");
    let head = format!("#{placement}=");
    text.lines()
        .map(|line| match line.strip_prefix(&head) {
            Some(rest) => {
                let location = refs(rest).first().cloned().expect("a location");
                format!("{head}AXIS2_PLACEMENT_3D('',#{location},#{location},#{location});")
            }
            None => line.to_owned(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn malformed_placement_and_external_parts_retain_other_instances() {
    let mut definition = definition();
    definition["rootAssembly"]["occurrences"][1]["transform"] = json!([1., 2.]);
    definition["rootAssembly"]["instances"][1]["documentId"] = json!("external");
    let (model, report) = read(definition);
    assert_eq!(model.ir.model.occurrences.len(), 2);
    assert!(report
        .losses()
        .iter()
        .any(|loss| loss.code.as_deref() == Some("onshape/assembly.record-unsupported")));
    assert!(report
        .losses()
        .iter()
        .any(|loss| loss.code.as_deref() == Some("onshape/assembly.reference-unresolved")));
}

#[test]
fn evaluated_supported_mate_families_keep_their_degrees_of_freedom() {
    let mut definition = definition();
    definition["rootAssembly"]["features"] = json!([
        mate("slider", "SLIDER"),
        mate("cylindrical", "CYLINDRICAL"),
        mate("planar", "PLANAR"),
        mate("ball", "BALL")
    ]);
    let (model, _) = read(definition);
    let joints = json::to_value(model.ir.model.assembly_joints).unwrap();
    for expected in ["slider", "cylindrical", "plane_coincidence", "ball"] {
        assert!(joints
            .as_array()
            .unwrap()
            .iter()
            .any(|joint| joint["operands"]["value"]["kind"]["kind"] == expected));
    }
}

#[test]
fn unresolved_mate_quantities_remain_in_the_definition_asset() {
    use cadmpeg_ir::assets::AssetContent;
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use std::io::Cursor;
    let mut definition = definition();
    let mut revolute = mate("revolute", "REVOLUTE");
    revolute["featureData"]["limits"] = json!({"minimum": -0.5, "maximum": 1.0});
    definition["rootAssembly"]["features"] = json!([revolute]);
    let bytes = bundle(definition.clone());
    let result = crate::test_support::codec()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let (ir, _, _) = result.into_parts();
    let asset = ir
        .model
        .assets
        .iter()
        .find(|asset| {
            asset
                .name
                .as_ref()
                .is_some_and(|name| name.as_str() == "assemblies/assembly/definition.json")
        })
        .unwrap();
    let AssetContent::Embedded { data } = &asset.content else {
        panic!("definition asset must be embedded");
    };
    assert_eq!(
        json::from_slice::<serde_json::Value>(data.as_bytes()).unwrap(),
        definition
    );
}
