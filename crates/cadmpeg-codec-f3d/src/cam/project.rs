// SPDX-License-Identifier: Apache-2.0
//! Transfer source CAM definitions to the process arenas.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::native::NativeConvertError;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use super::records::{Manufacturing, ManufacturingParameter};

fn native_error(error: NativeConvertError) -> CodecError {
    CodecError::from(error)
}

fn model<T: DeserializeOwned>(value: Value) -> Result<T, CodecError> {
    serde_json::from_value(value).map_err(CodecError::malformed)
}

fn native_name(value: &str) -> Value {
    json!({"codec":"f3d", "value":{"kind":"text", "value":if value.trim().is_empty() { "unknown" } else { value }}})
}

pub(crate) fn store(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    cam: &Manufacturing,
) -> Result<(), CodecError> {
    let namespace = ir.native.namespace_mut(cadmpeg_ir::codec_id!("f3d"));
    namespace
        .set_arena_from(ctx, "cam_setups", &cam.setups)
        .map_err(native_error)?;
    namespace
        .set_arena_from(ctx, "cam_operations", &cam.operations)
        .map_err(native_error)?;
    namespace
        .set_arena_from(ctx, "cam_tools", &cam.tools)
        .map_err(native_error)?;
    Ok(())
}

pub(crate) fn retained_for_decode(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
) -> Result<Manufacturing, CodecError> {
    let Some(namespace) = ir.native.namespace("f3d") else {
        return Ok(Manufacturing::default());
    };
    let mut cam = Manufacturing {
        setups: namespace
            .arena_as_for_decode(ctx, "cam_setups")
            .map_err(native_error)?,
        operations: namespace
            .arena_as_for_decode(ctx, "cam_operations")
            .map_err(native_error)?,
        tools: namespace
            .arena_as_for_decode(ctx, "cam_tools")
            .map_err(native_error)?,
    };
    cam.setups.sort_by(|left, right| {
        (&left.source.member, left.source.start).cmp(&(&right.source.member, right.source.start))
    });
    cam.operations.sort_by(|left, right| {
        (&left.source.member, left.source.start).cmp(&(&right.source.member, right.source.start))
    });
    cam.tools.sort_by(|left, right| {
        (&left.source.member, left.source.start).cmp(&(&right.source.member, right.source.start))
    });
    Ok(cam)
}

#[cfg(test)]
pub(crate) fn retained(ir: &CadIr) -> Result<Manufacturing, CodecError> {
    retained_for_decode(
        &cadmpeg_core::decode::test_support::service_decode_context(),
        ir,
    )
}

pub(crate) fn collect(
    ctx: &DecodeContext<'_>,
    scan: &crate::container::ContainerScan<'_>,
    ir: &mut CadIr,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<(), CodecError> {
    let mut cam = Manufacturing::default();
    super::read(ctx, scan, &mut cam, losses)?;
    if cam.is_empty() {
        return Ok(());
    }
    losses.extend(crate::tree::state_root_definition(ctx, ir)?);
    let admitted = ir.entity_count();
    project(ir, &cam)?;
    ctx.charge_entities(
        cadmpeg_core::decode::u64_from_index(ir.entity_count() - admitted),
        "admit CAM process entities",
    )?;
    store(ctx, ir, &cam)
}

pub(crate) fn scope(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    member: &str,
) -> Result<(), CodecError> {
    let mut cam = retained_for_decode(ctx, ir)?;
    if cam.is_empty() {
        return Ok(());
    }
    super::scope_member(&mut cam, member)?;
    ir.process.setups.clear();
    ir.process.operations.clear();
    ir.process.tools.clear();
    ir.process.parameters.clear();
    project(ir, &cam)?;
    store(ctx, ir, &cam)
}

fn parameters(
    ir: &mut CadIr,
    owner: &Value,
    id: &str,
    source: &[ManufacturingParameter],
) -> Result<(), CodecError> {
    let mut names = std::collections::BTreeSet::new();
    for (ordinal, parameter) in source.iter().enumerate() {
        if parameter.name.trim().is_empty() || !names.insert(&parameter.name) {
            continue;
        }
        let expression = roxmltree::Document::parse(&parameter.native_xml)
            .ok()
            .and_then(|document| {
                document
                    .root_element()
                    .attribute("expression")
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| parameter.value.clone());
        let mut record = json!({
            "id":format!("{id}/parameter/{ordinal}"), "owner":owner, "name":parameter.name,
            "definition":{"kind":"expression", "value":{"kind":"source", "value":{"dialect":{"kind":"fusion_cam"},"tokens":[{"kind":"text","value":expression}]}}},
            "native_ref":id
        });
        if let Ok(number) = parameter.value.parse::<f64>() {
            if number.is_finite() {
                let (scale, quantity) = match parameter.unit.as_deref() {
                    Some("mm") => (1.0, Some((1, 0, 0))),
                    Some("cm") => (10.0, Some((1, 0, 0))),
                    Some("m") => (1000.0, Some((1, 0, 0))),
                    Some("in" | "inch") => (25.4, Some((1, 0, 0))),
                    Some("deg") => (std::f64::consts::PI / 180.0, Some((0, 1, 0))),
                    Some("rad") => (1.0, Some((0, 1, 0))),
                    Some("mm/min") => (1.0 / 60.0, Some((1, 0, -1))),
                    Some("rpm") => (std::f64::consts::TAU / 60.0, Some((0, 1, -1))),
                    _ => (1.0, None),
                };
                let converted = number * scale;
                if converted.is_finite() {
                    record["value"] = json!({"kind":"number","value":converted});
                    if let Some((length, angle, time)) = quantity {
                        record["kind"] = json!({"kind":"quantity","value":{"length":length,"mass":0,"angle":angle,"time":time}});
                    }
                }
            }
        } else if matches!(parameter.value.as_str(), "true" | "false") {
            record["kind"] = json!({"kind":"boolean"});
            record["value"] = json!({"kind":"boolean","value":parameter.value == "true"});
        } else {
            record["kind"] = json!({"kind":"text"});
            record["value"] = json!({"kind":"text","value":parameter.value});
        }
        ir.process.parameters.push(model(record)?);
    }
    Ok(())
}

fn number(parameters: &[ManufacturingParameter], names: &[&str]) -> Option<f64> {
    names.iter().find_map(|name| {
        parameters
            .iter()
            .find(|parameter| parameter.name == *name)
            .and_then(|parameter| parameter.value.parse::<f64>().ok())
            .filter(|value| value.is_finite())
    })
}

fn literal(value: f64) -> Value {
    json!({"kind":"literal", "value":value})
}

fn technology(parameters: &[ManufacturingParameter]) -> Value {
    let mut result = json!({"feeds":[]});
    let feeds: Vec<_> = [
        ("tool_feedCutting", "cutting"),
        ("tool_feedPlunge", "plunge"),
        ("tool_feedRamp", "ramp"),
        ("tool_feedEntry", "entry"),
        ("tool_feedExit", "exit"),
    ]
    .into_iter()
    .filter_map(|(name, role)| {
        number(parameters, &[name]).map(|value| {
            json!([
                {"kind":role}, {"kind":"per_time", "value":literal(value / 60.0)}
            ])
        })
    })
    .collect();
    result["feeds"] = json!(feeds);
    if let Some(speed) = number(parameters, &["tool_spindleSpeed"]) {
        result["spindle"] =
            json!({"kind":"rotation", "value":literal(speed * (std::f64::consts::TAU / 60.0))});
    }
    for (source, field) in [
        ("stepover", "stepover"),
        ("maximumStepdown", "stepdown"),
        ("tolerance", "tolerance"),
        ("stockToLeave", "stock_to_leave_radial"),
        ("verticalStockToLeave", "stock_to_leave_axial"),
    ] {
        if let Some(value) = number(parameters, &[source]) {
            result[field] = literal(value);
        }
    }
    result
}

fn process_id(id: &str) -> String {
    id.replace(":cam_setup#", ":setup#")
        .replace(":cam_operation#", ":operation#")
        .replace(":cam_tool#", ":tool#")
}

fn project(ir: &mut CadIr, cam: &Manufacturing) -> Result<(), CodecError> {
    let scope = ir
        .model
        .product_definitions
        .iter()
        .find(|definition| definition.is_root())
        .or_else(|| ir.model.product_definitions.first())
        .ok_or_else(|| CodecError::malformed("CAM setup requires a document definition"))?
        .id
        .clone();
    for setup in &cam.setups {
        let mut record = json!({"id":process_id(setup.id.as_str()),"scope":scope,"steps":[],"workpieces":[],"wcs":{},"work_offsets":[],"native_ref":setup.id});
        if let Some(name) = &setup.name {
            record["name"] = json!(name);
        }
        if let Some(mode) = setup
            .stock
            .kind
            .as_ref()
            .filter(|mode| !mode.trim().is_empty())
        {
            record["stock"] = json!({"value":{"kind":"native","value":mode},"parameters":[]});
            if mode == "default" || mode == "relativebox" {
                let offset = |names: &[&str]| number(&setup.parameters, names).map(literal);
                let sides = offset(&["job_stockOffsetSides"]);
                let low = [
                    sides.clone(),
                    sides.clone(),
                    offset(&["job_stockOffsetBottom"]),
                ];
                let high = [sides.clone(), sides, offset(&["job_stockOffsetTop"])];
                record["stock"]["value"] =
                    json!({"kind":"relative", "value":{"low":low,"high":high}});
            }
        }
        if let Some(mode) = setup
            .wcs
            .origin_mode
            .as_ref()
            .filter(|mode| !mode.trim().is_empty())
        {
            record["wcs"]["origin"] =
                json!({"value":{"kind":"native","value":mode},"parameters":[]});
        }
        if let Some(mode) = setup.wcs.orientation.iter().find(|parameter| {
            parameter.name == "wcs_orientation_mode" && !parameter.value.trim().is_empty()
        }) {
            record["wcs"]["orientation"] =
                json!({"value":{"kind":"native","value":mode.value},"parameters":[]});
            if mode.value == "modelOrientation" {
                record["wcs"]["orientation"]["value"] = json!({"kind":"model"});
            }
        }
        let steps: Vec<_> = cam
            .operations
            .iter()
            .filter(|operation| operation.setup == setup.id)
            .map(|operation| json!({"kind":"operation","value":process_id(operation.id.as_str())}))
            .collect();
        record["steps"] = json!(steps);
        ir.process.setups.push(model(record)?);
        parameters(
            ir,
            &json!({"kind":"setup","value":process_id(setup.id.as_str())}),
            setup.id.as_str(),
            &setup.parameters,
        )?;
    }
    for tool in &cam.tools {
        let mut record = json!({"id":process_id(tool.id.as_str()),"class":"unclassified","native_kind":native_name(tool.tool_type.as_deref().unwrap_or("unknown")),"nc":{},"cutter":{},"native_ref":tool.id});
        if let Some(number) = tool
            .number
            .as_ref()
            .and_then(|number| number.parse::<u32>().ok())
        {
            record["nc"]["number"] = json!({"kind":"literal","value":number});
        }
        let class = match tool.tool_type.as_deref() {
            Some("flat end mill") => "flat_endmill",
            Some("ball end mill") => "ball_endmill",
            Some("face mill") => "face_mill",
            Some("drill") => "drill",
            _ => "unclassified",
        };
        record["class"] = json!(class);
        let tool_scale = if tool.parameters.iter().any(|parameter| {
            parameter.name == "tool.unit"
                && matches!(parameter.value.as_str(), "inches" | "inch" | "in")
        }) {
            25.4
        } else {
            1.0
        };
        for (names, field) in [
            (["tool_diameter", "body.diameter"], "diameter"),
            (["tool_cornerRadius", "body.corner-radius"], "corner_radius"),
            (["tool_fluteLength", "body.flute-length"], "flute_length"),
            (
                ["tool_overallLength", "body.overall-length"],
                "overall_length",
            ),
        ] {
            if let Some(value) = number(&tool.parameters, &names) {
                let converted = value * tool_scale;
                if converted.is_finite() {
                    record["cutter"][field] = literal(converted);
                }
            }
        }
        let mut catalog = json!({});
        if let Some(vendor) = &tool.vendor {
            if !vendor.trim().is_empty() {
                catalog["vendor"] = json!(vendor);
            }
        }
        if let Some(product) = &tool.product {
            if !product.trim().is_empty() {
                catalog["product_id"] = json!(product);
            }
        }
        if catalog
            .as_object()
            .is_some_and(|catalog| !catalog.is_empty())
        {
            record["catalog"] = catalog;
        }
        ir.process.tools.push(model(record)?);
        parameters(
            ir,
            &json!({"kind":"tool","value":process_id(tool.id.as_str())}),
            tool.id.as_str(),
            &tool.parameters,
        )?;
    }
    for operation in &cam.operations {
        use super::records::ManufacturingStrategy;
        let class = match operation.family {
            ManufacturingStrategy::Face => json!({"kind":"milling","value":"plane"}),
            ManufacturingStrategy::Drilling => json!({"kind":"drilling","value":"drilling"}),
            ManufacturingStrategy::Adaptive2d | ManufacturingStrategy::Pocket2d => {
                json!({"kind":"milling","value":"bottom_and_side"})
            }
            ManufacturingStrategy::Contour2d => json!({"kind":"milling","value":"side"}),
            ManufacturingStrategy::Adaptive3d
            | ManufacturingStrategy::Parallel
            | ManufacturingStrategy::Scallop => json!({"kind":"milling","value":"freeform"}),
            ManufacturingStrategy::Other => json!({"kind":"unclassified"}),
        };
        let mut record = json!({"id":process_id(operation.id.as_str()),"kind":{"kind":"machining","class":class},"native_kind":native_name(&operation.strategy),"features":[],"groups":[],"geometry":[],"heights":{},"radii":{},"axial":{},"technology":{"feeds":[]},"configuration_states":{},"native_ref":operation.id});
        if let Some(name) = &operation.name {
            record["name"] = json!(name);
        }
        if let Some(tool) = &operation.tool {
            record["tooling"] = json!({"kind":"tool","value":process_id(tool.as_str())});
        }
        record["technology"] = technology(&operation.parameters);
        ir.process.operations.push(model(record)?);
        parameters(
            ir,
            &json!({"kind":"operation","value":process_id(operation.id.as_str())}),
            operation.id.as_str(),
            &operation.parameters,
        )?;
    }
    Ok(())
}

pub(crate) fn merge_native(
    target: &mut cadmpeg_ir::native::Native,
    source: &mut cadmpeg_ir::native::Native,
) {
    let source = source.namespace_mut(cadmpeg_ir::codec_id!("f3d"));
    let target = target.namespace_mut(cadmpeg_ir::codec_id!("f3d"));
    for name in ["cam_setups", "cam_operations", "cam_tools"] {
        if let Some((key, mut records)) = source.arenas_mut().remove_entry(name) {
            target
                .arenas_mut()
                .entry(key)
                .or_default()
                .append(&mut records);
        }
    }
}

pub(crate) fn ensure_scopes(ctx: &DecodeContext<'_>, ir: &mut CadIr) -> Result<(), CodecError> {
    let scopes: std::collections::BTreeSet<_> = ir
        .process
        .setups
        .iter()
        .map(|setup| setup.scope.clone())
        .collect();
    for scope in scopes {
        if ir
            .design
            .scopes
            .iter()
            .any(|design| design.definition == scope)
        {
            continue;
        }
        let definition = ir
            .model
            .product_definitions
            .iter()
            .find(|definition| definition.id == scope)
            .ok_or_else(|| CodecError::malformed("CAM setup scope names no product definition"))?;
        let occurrences = definition
            .occurrences
            .iter()
            .map(|id| {
                (
                    id.clone(),
                    cadmpeg_ir::design::scope::OccurrenceDesign {
                        configuration: None,
                        arrangements: Vec::new(),
                        grounded: None,
                        flexible: None,
                        parameters: std::collections::BTreeMap::new(),
                    },
                )
            })
            .collect();
        ctx.charge_entities(1, "admit CAM design scope")?;
        ir.design
            .scopes
            .push(cadmpeg_ir::design::scope::DesignScope {
                definition: scope,
                roots: Vec::new(),
                active_configuration: None,
                active_arrangements: std::collections::BTreeSet::new(),
                occurrences,
                current: Vec::new(),
                unproduced: definition.bodies.iter().cloned().collect(),
            });
    }
    Ok(())
}
