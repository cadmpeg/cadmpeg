// SPDX-License-Identifier: Apache-2.0

use crate::design::units::UnitSymbol;
use crate::process::feature::{MachiningFeature, MachiningFeatureClass};
use crate::process::groups::{GroupKind, ProcessGroup, ProcessOwner};
use crate::process::operation::{GeometryCombine, Tooling};
use crate::process::setup::{FunctionStep, NcFunction, ProgramStep, Step};
use crate::process::tool::{NcNumbers, ToolPocket};
use crate::validate::process::fixture::{
    assert_error, assert_no_process_finding, document, id, native, parameter, text,
};

#[test]
fn tooling_and_machines_resolve() {
    let mut ir = document();
    ir.process.operations[2].tooling = Some(Tooling::Preset(id("preset", "missing")));
    assert_error(&ir, "which is no preset");

    let mut ir = document();
    ir.process.presets[0].tool = id("tool", "missing");
    assert_error(&ir, "preset's tool test:process:tool#missing is no tool");

    let mut ir = document();
    ir.process.setups[0].machine = Some(id("machine", "missing"));
    assert_error(&ir, "machine test:process:machine#missing is no machine");

    let mut ir = document();
    ir.process.programs[0].machine = Some(id("machine", "missing"));
    assert_error(&ir, "machine test:process:machine#missing is no machine");
}

#[test]
fn no_machine_has_two_pockets_holding_one_tool() {
    let mut ir = document();
    let pocket = ir.process.machines[0].pockets[0].clone();
    ir.process.machines[0].pockets.push(pocket);
    assert_error(&ir, "machine has two pockets holding test:process:tool#1");

    let mut ir = document();
    ir.process.machines[0].pockets.push(ToolPocket {
        tool: id("tool", "missing"),
        nc: NcNumbers {
            number: None,
            length_offset: None,
            diameter_offset: None,
            turret: None,
        },
    });
    assert_error(
        &ir,
        "machine pocket holds test:process:tool#missing, which is no tool",
    );
}

#[test]
fn machining_features_resolve_and_contain_no_cycle() {
    let mut ir = document();
    ir.process.operations[2]
        .features
        .push(id("feature", "missing"));
    assert_error(&ir, "which is no machining feature");

    let mut ir = document();
    ir.process.toolpaths[0].feature = Some(id("feature", "missing"));
    assert_error(
        &ir,
        "toolpath's feature test:process:feature#missing is no machining feature",
    );

    let mut ir = document();
    ir.process.operations[2].features.clear();
    ir.process.toolpaths[0].feature = Some(id("feature", "hole"));
    assert_error(&ir, "is not one its operation lists");

    let mut ir = document();
    ir.process.features[0].class = MachiningFeatureClass::Compound {
        members: vec![id("feature", "slot")],
    };
    ir.process.features.push(MachiningFeature {
        id: id("feature", "slot"),
        name: None,
        class: MachiningFeatureClass::Replicate {
            base: id("feature", "hole"),
        },
        native_kind: native("slot"),
        geometry: Vec::new(),
        native_ref: None,
    });
    assert_error(&ir, "contains itself through `Replicate` and `Compound`");
}

fn group(key: &str, kind: GroupKind) -> ProcessGroup {
    ProcessGroup {
        id: id("group", key),
        kind,
        name: None,
        parent: None,
        native_ref: None,
    }
}

#[test]
fn groups_resolve_by_kind_and_nest_without_a_cycle() {
    let mut ir = document();
    ir.process.operations[2].groups.push(id("group", "missing"));
    assert_error(&ir, "which is no group");

    let mut ir = document();
    ir.process.groups.push(group("other", GroupKind::Method));
    ir.process.operations[2].groups.push(id("group", "other"));
    assert_error(&ir, "a second group of one kind");

    let mut ir = document();
    ir.process.groups[0].parent = Some(id("group", "missing"));
    assert_error(&ir, "group's parent test:process:group#missing is no group");

    let mut ir = document();
    ir.process.groups.push(group(
        "part",
        GroupKind::Geometry {
            geometry: Vec::new(),
        },
    ));
    ir.process.groups[0].parent = Some(id("group", "part"));
    assert_error(&ir, "is of another kind");

    let mut ir = document();
    ir.process.groups[0].parent = Some(id("group", "method"));
    assert_error(&ir, "group is its own ancestor through `parent`");
}

#[test]
fn combine_needs_a_geometry_group() {
    let mut ir = document();
    ir.process.operations[2].geometry[0].combine = Some(GeometryCombine::Extend);
    assert_error(&ir, "the operation lists no geometry group");

    let mut ir = document();
    ir.process.operations[2].geometry[0].combine = Some(GeometryCombine::Replace);
    ir.process.groups.push(group(
        "part",
        GroupKind::Geometry {
            geometry: Vec::new(),
        },
    ));
    ir.process.operations[2].groups.push(id("group", "part"));
    crate::validate::process::fixture::assert_no_process_finding(&ir);

    let mut ir = document();
    let mut geometry = ir.process.operations[2].geometry.clone();
    geometry[0].combine = Some(GeometryCombine::Replace);
    ir.process.features[0].geometry = geometry;
    assert_error(&ir, "only an operation's own geometry states");
}

#[test]
fn parameter_owners_resolve_and_names_are_unique_per_owner() {
    let mut ir = document();
    ir.process.parameters[0].owner = ProcessOwner::Tool(id("tool", "missing"));
    assert_error(
        &ir,
        "parameter's owner test:process:tool#missing is no tool",
    );

    let mut ir = document();
    let mut twin = parameter("twin", ProcessOwner::Operation(id("operation", "mill")));
    twin.name = text("depth");
    ir.process.parameters.push(twin);
    assert_error(&ir, "parameter name `depth` repeats");
}

#[test]
fn native_kinds_name_any_codec_and_units_match_their_quantity() {
    let mut ir = document();
    ir.process.tools[0].native_kind.codec = crate::codec_id!("unknown");
    assert_no_process_finding(&ir);

    let mut ir = document();
    ir.process.tools[0].unit = Some(UnitSymbol::Deg);
    assert_error(&ir, "tool's unit `deg` is no length unit");

    let mut ir = document();
    ir.process.programs[0].output.unit =
        Some(crate::validate::process::fixture::constant(UnitSymbol::S));
    assert_error(&ir, "program's output unit `s` is no length unit");
}

/// A document whose `sldprt` namespace holds the one native record `record`.
fn with_native_record(record: &str) -> crate::document::CadIr {
    let mut ir = document();
    ir.native
        .namespace_mut(crate::codec_id!("sldprt"))
        .arenas_mut()
        .insert(
            cadmpeg_core::nonblank_literal!("records"),
            vec![crate::NativeRecord::new(
                crate::ids::Identity::new(record).unwrap(),
                serde_json::Map::new(),
            )
            .unwrap()],
        );
    ir.native.finalize();
    ir
}

#[test]
fn process_native_refs_resolve() {
    let record = "sldprt:native:setup#0";
    let missing = "sldprt:native:setup#missing";
    let mut ir = with_native_record(record);
    ir.process.setups[0].native_ref = Some(record.to_owned());
    ir.process.operations[0].native_ref = Some(record.to_owned());
    assert_no_process_finding(&ir);

    ir.process.operations[0].native_ref = Some(missing.to_owned());
    assert_error(
        &ir,
        "native_ref `sldprt:native:setup#missing` does not resolve",
    );

    let mut ir = with_native_record(record);
    ir.process.programs[0].steps.push(ProgramStep::Group {
        name: None,
        steps: vec![ProgramStep::Function(FunctionStep {
            function: NcFunction::Stop,
            suppressed: None,
            native_ref: Some(missing.to_owned()),
        })],
    });
    assert_error(
        &ir,
        "native_ref `sldprt:native:setup#missing` does not resolve",
    );

    let mut ir = with_native_record(record);
    ir.process.setups[0]
        .steps
        .push(Step::Function(FunctionStep {
            function: NcFunction::Stop,
            suppressed: None,
            native_ref: Some(missing.to_owned()),
        }));
    assert_error(
        &ir,
        "native_ref `sldprt:native:setup#missing` does not resolve",
    );
}
