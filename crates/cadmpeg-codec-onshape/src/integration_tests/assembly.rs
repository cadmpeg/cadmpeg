// SPDX-License-Identifier: Apache-2.0
//! Assembly product, placement and mate admission.
use crate::test_support::{assembly::*, read_bytes, Report};
use cadmpeg_core::json;
use cadmpeg_core::serde_erase::json;
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
    assert!(
        result
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == unreadable),
        "{:#?}",
        result.report().losses
    );
    let ir = result.ir();
    let faces_of = |shell: &cadmpeg_ir::topology::Shell| {
        ir.geometry
            .faces
            .iter()
            .filter(|face| face.shell == shell.id)
            .count()
    };
    let mut counts = ir.geometry.shells.iter().map(faces_of).collect::<Vec<_>>();
    counts.sort_unstable();
    // The other studio's cube loses the face on the given-up plane; this
    // studio's cube keeps all six.
    assert_eq!(counts, [5, 6], "{:#?}", ir.geometry.faces);
}

/// `text` with the placement of its first plane holding a token that is no
/// parameter, so the placement is given up and the plane and its face drop
/// with it.
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
    let instance = |line: &str| {
        let (label, rest) = line.split_once('=')?;
        Some((label.trim().to_owned(), rest.trim_start().to_owned()))
    };
    let plane = text
        .lines()
        .filter_map(instance)
        .find(|(_, rest)| rest.starts_with("PLANE("))
        .expect("a plane");
    let placement = format!("#{}", refs(&plane.1).last().cloned().expect("a placement"));
    text.lines()
        .map(|line| match instance(line) {
            Some((label, rest)) if label == placement => {
                let location = refs(&rest).first().cloned().expect("a location");
                format!("{label}=AXIS2_PLACEMENT_3D('',#{location},%,%);")
            }
            _ => line.to_owned(),
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

/// The `record.unreadable` losses of `report` that name the source record at
/// `address`.
fn charges(report: &Report, address: &str) -> Vec<String> {
    let record = crate::archive::source_record(address);
    let code = cadmpeg_ir::unknown::part::record_unreadable_kind().to_string();
    report
        .losses()
        .iter()
        .filter(|loss| {
            loss.code.as_deref() == Some(code.as_str())
                && loss
                    .detail
                    .split_whitespace()
                    .any(|word| word == record.as_str())
        })
        .map(|loss| loss.detail.clone())
        .collect()
}

const DEFINITION: &str = "assemblies/assembly/definition.json";

#[test]
fn an_instance_without_a_type_is_given_up_with_the_mates_that_need_it() {
    let mut definition = definition();
    definition["rootAssembly"]["instances"][1]
        .as_object_mut()
        .unwrap()
        .remove("type");
    definition["rootAssembly"]["features"] = json!([mate("fastened", "FASTENED")]);
    let (model, report) = read(definition);
    let charges = charges(&report, &format!("{DEFINITION}#/rootAssembly/instances/1"));
    let [charge] = charges.as_slice() else {
        panic!("one charge: {:?}", report.losses());
    };
    let joint = crate::reader::id("joint", "[]:fastened");
    assert!(
        charge.ends_with(&format!("; dropped=[{joint}]")),
        "{charge}"
    );
    assert!(model
        .ir
        .model
        .assembly_joints
        .iter()
        .all(|placed| placed.id.as_str() != joint));
    assert!(model
        .ir
        .model
        .occurrences
        .iter()
        .all(|placed| placed.name.as_deref() != Some("second")));
    assert_eq!(
        report
            .losses()
            .iter()
            .filter(|loss| loss.detail.contains(&joint))
            .count(),
        1,
        "{:?}",
        report.losses()
    );
}

/// A given-up instance names among its dropped records only the joints its
/// mates map to: a planar mate's sense needs the lost placement, and a mate
/// whose other connector has no frame maps to no joint.
#[test]
fn a_given_up_instance_names_only_the_joints_its_mates_map_to() {
    let mut definition = definition();
    definition["rootAssembly"]["instances"][1]
        .as_object_mut()
        .unwrap()
        .remove("type");
    let mut frameless = mate("frameless", "FASTENED");
    frameless["featureData"]["matedEntities"][0]
        .as_object_mut()
        .unwrap()
        .remove("matedCS");
    definition["rootAssembly"]["features"] = json!([
        mate("planar", "PLANAR"),
        frameless,
        mate("fastened", "FASTENED")
    ]);
    let (_, report) = read(definition);
    let charges = charges(&report, &format!("{DEFINITION}#/rootAssembly/instances/1"));
    let [charge] = charges.as_slice() else {
        panic!("one charge: {:?}", report.losses());
    };
    let joint = crate::reader::id("joint", "[]:fastened");
    assert!(
        charge.ends_with(&format!("; dropped=[{joint}]")),
        "{charge}"
    );
}

#[test]
fn a_repeated_instance_id_is_given_up_only_where_its_content_differs() {
    let repeat = format!("{DEFINITION}#/rootAssembly/instances/2");
    let mut same = definition();
    let first = same["rootAssembly"]["instances"][0].clone();
    same["rootAssembly"]["instances"]
        .as_array_mut()
        .unwrap()
        .push(first.clone());
    let (_, report) = read(same);
    assert!(
        charges(&report, &repeat).is_empty(),
        "{:?}",
        report.losses()
    );
    let mut different = definition();
    let mut other = first;
    other["name"] = json!("renamed");
    different["rootAssembly"]["instances"]
        .as_array_mut()
        .unwrap()
        .push(other);
    let (model, report) = read(different);
    assert_eq!(charges(&report, &repeat).len(), 1, "{:?}", report.losses());
    assert_eq!(model.ir.model.occurrences.len(), 2);
}

#[test]
fn assembly_rows_that_are_no_array_are_given_up() {
    for (path, address) in [
        (
            ["rootAssembly", "instances"].as_slice(),
            "#/rootAssembly/instances",
        ),
        (["parts"].as_slice(), "#/parts"),
        (
            ["rootAssembly", "features"].as_slice(),
            "#/rootAssembly/features",
        ),
    ] {
        let mut definition = definition();
        let field = path
            .iter()
            .fold(&mut definition, |value, key| &mut value[*key]);
        *field = json!(path.last());
        let (_, report) = read(definition);
        assert_eq!(
            charges(&report, &format!("{DEFINITION}{address}")).len(),
            1,
            "{address}: {:?}",
            report.losses()
        );
    }
}

#[test]
fn a_repeated_mate_id_is_given_up_only_where_its_content_differs() {
    let repeat = format!("{DEFINITION}#/rootAssembly/features/1");
    let mut same = definition();
    same["rootAssembly"]["features"] =
        json!([mate("fastened", "FASTENED"), mate("fastened", "FASTENED")]);
    let (_, report) = read(same);
    assert!(
        charges(&report, &repeat).is_empty(),
        "{:?}",
        report.losses()
    );
    let mut different = definition();
    different["rootAssembly"]["features"] =
        json!([mate("fastened", "FASTENED"), mate("fastened", "REVOLUTE")]);
    let (model, report) = read(different);
    assert_eq!(charges(&report, &repeat).len(), 1, "{:?}", report.losses());
    assert_eq!(model.ir.model.assembly_joints.len(), 2);
}
