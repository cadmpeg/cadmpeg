// SPDX-License-Identifier: Apache-2.0
//! Transfer source CAM definitions to the process arenas.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

use cadmpeg_ir::design::ids::SetupId;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::loss::LossNote;

use crate::loss::F3dLossCode;
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
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let mut cam = Manufacturing::default();
    super::read(ctx, scan, &mut cam, losses)?;
    if cam.is_empty() {
        return Ok(());
    }
    admit(ctx, ir, &cam, losses)
}

/// Projects the read definitions onto the process arenas and retains their
/// native records. A definition set that does not project is a loss and
/// leaves the process arenas as they were.
fn admit(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    cam: &Manufacturing,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    losses.extend(crate::tree::state_root_definition(ctx, ir)?);
    let admitted = ir.entity_count();
    let lengths = (
        ir.process.setups.len(),
        ir.process.operations.len(),
        ir.process.tools.len(),
        ir.process.parameters.len(),
    );
    let mut projected_losses = Vec::new();
    match project(ctx, ir, cam, &mut projected_losses) {
        Ok(()) => losses.append(&mut projected_losses),
        Err(CodecError::Malformed(error)) => {
            ir.process.setups.truncate(lengths.0);
            ir.process.operations.truncate(lengths.1);
            ir.process.tools.truncate(lengths.2);
            ir.process.parameters.truncate(lengths.3);
            losses.push(F3dLossCode::CamDocumentUnreadable.note(format!(
                "CAM definitions did not project onto process records: {error}"
            )));
            return Ok(());
        }
        Err(error) => return Err(error),
    }
    ctx.charge_entities(
        cadmpeg_core::decode::u64_from_index(ir.entity_count() - admitted),
        "admit CAM process entities",
    )?;
    store(ctx, ir, cam)
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
    project(ctx, ir, &cam, &mut Vec::new())?;
    store(ctx, ir, &cam)
}

/// A process dimension or process value of a source parameter.
type Dim = (i32, i32, i32);
const LENGTH: Dim = (1, 0, 0);
const ANGLE: Dim = (0, 1, 0);
const FEED: Dim = (1, 0, -1);
const SPEED: Dim = (0, 1, -1);

/// The factor to CADIR units (millimeters, radians, seconds) and the
/// quantity of a parameter `unit` text.
fn unit_scale(unit: &str) -> Option<(f64, Dim)> {
    Some(match unit {
        "mm" => (1.0, LENGTH),
        "cm" => (10.0, LENGTH),
        "m" => (1000.0, LENGTH),
        "in" | "inch" | "inches" => (25.4, LENGTH),
        "deg" => (std::f64::consts::PI / 180.0, ANGLE),
        "rad" => (1.0, ANGLE),
        "mm/min" => (1.0 / 60.0, FEED),
        "cm/min" => (10.0 / 60.0, FEED),
        "m/min" => (1000.0 / 60.0, FEED),
        "in/min" => (25.4 / 60.0, FEED),
        "rpm" => (std::f64::consts::TAU / 60.0, SPEED),
        _ => return None,
    })
}

/// The finite value of a numeric parameter in CADIR units, with its
/// quantity, where its `unit` names one.
fn measure(parameter: &ManufacturingParameter) -> Option<(f64, Dim)> {
    let number = parameter.value.parse::<f64>().ok()?;
    let (scale, quantity) = unit_scale(parameter.unit.as_deref()?)?;
    let converted = number * scale;
    (number.is_finite() && converted.is_finite()).then_some((converted, quantity))
}

/// The process parameter ids of one owner, by source name. The first
/// parameter of a name owns it.
type Ids = BTreeMap<String, String>;

fn parameters(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    owner: &Value,
    id: &str,
    source: &[ManufacturingParameter],
) -> Result<Ids, CodecError> {
    let mut ids = Ids::new();
    for (ordinal, parameter) in source.iter().enumerate() {
        if parameter.name.trim().is_empty() || ids.contains_key(&parameter.name) {
            continue;
        }
        let parameter_id = format!("{id}/parameter/{ordinal}");
        ids.insert(parameter.name.clone(), parameter_id.clone());
        let expression = expression(ctx, parameter)?;
        let mut record = json!({
            "id":parameter_id, "owner":owner, "name":parameter.name,
            "definition":{"kind":"expression", "value":{"kind":"source", "value":{"dialect":{"kind":"fusion_cam"},"tokens":[{"kind":"text","value":expression}]}}},
            "native_ref":id
        });
        if let Ok(number) = parameter.value.parse::<f64>() {
            if number.is_finite() {
                let (converted, quantity) = match measure(parameter) {
                    Some((converted, quantity)) => (converted, Some(quantity)),
                    None => (number, None),
                };
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
    Ok(ids)
}

/// The `expression` attribute of a parameter's source element, or its value
/// text where the element states none.
fn expression(
    ctx: &DecodeContext<'_>,
    parameter: &ManufacturingParameter,
) -> Result<String, CodecError> {
    let document = match ctx.parse_xml(&parameter.native_xml, "parse CAM parameter XML") {
        Ok(document) => document,
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(_) => return Ok(parameter.value.clone()),
    };
    Ok(document
        .root_element()
        .attribute("expression")
        .map_or_else(|| parameter.value.clone(), str::to_owned))
}

/// A setup's or operation's parameters, with the ids their records carry.
struct Named<'a> {
    source: &'a [ManufacturingParameter],
    ids: &'a Ids,
}

impl Named<'_> {
    fn parameter(&self, name: &str) -> Option<&ManufacturingParameter> {
        self.source.iter().find(|parameter| parameter.name == name)
    }

    fn text(&self, name: &str) -> Option<&str> {
        self.parameter(name)
            .map(|parameter| parameter.value.as_str())
            .filter(|text| !text.trim().is_empty())
    }

    /// The ids of the parameters a choice is read from.
    fn choice(&self, names: &[&str]) -> Vec<&str> {
        names
            .iter()
            .filter_map(|name| self.ids.get(*name).map(String::as_str))
            .collect()
    }

    /// A dimension bound to the process parameter `name`, where that
    /// parameter states a number in the quantity `want` by its unit.
    fn dimension(&self, name: &str, want: Dim) -> Option<Value> {
        let (_, quantity) = measure(self.parameter(name)?)?;
        let id = self.ids.get(name)?;
        (quantity == want)
            .then(|| json!({"kind":"parameter", "value":{"kind":"process", "value":id}}))
    }
}

fn technology(named: &Named<'_>) -> Value {
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
        named
            .dimension(name, FEED)
            .map(|value| json!([{"kind":role}, {"kind":"per_time", "value":value}]))
    })
    .collect();
    result["feeds"] = json!(feeds);
    if let Some(speed) = named.dimension("tool_spindleSpeed", SPEED) {
        result["spindle"] = json!({"kind":"rotation", "value":speed});
    }
    for (source, field) in [
        ("stepover", "stepover"),
        ("maximumStepdown", "stepdown"),
        ("tolerance", "tolerance"),
        ("stockToLeave", "stock_to_leave_radial"),
        ("verticalStockToLeave", "stock_to_leave_axial"),
    ] {
        if let Some(value) = named.dimension(source, LENGTH) {
            result[field] = value;
        }
    }
    result
}

fn literal(value: f64) -> Value {
    json!({"kind":"literal", "value":value})
}

/// The stock rule. `job_stockMode` `default` is a relative box, whose
/// offsets `job_stockOffsetMode` `simple` states; any other stock mode, or an
/// offset mode other than `simple`, keeps the source's mode name.
fn stock(named: &Named<'_>, losses: &mut Vec<LossNote>) -> Option<Value> {
    let mode = named.text("job_stockMode")?;
    let mut value = json!({"kind":"native","value":mode});
    let mut read = vec!["job_stockMode"];
    if mode == "default" {
        read.push("job_stockOffsetMode");
        if named.text("job_stockOffsetMode") == Some("simple") {
            let sides = named.dimension("job_stockOffsetSides", LENGTH);
            let top = named.dimension("job_stockOffsetTop", LENGTH);
            let bottom = named.dimension("job_stockOffsetBottom", LENGTH);
            value = json!({"kind":"relative", "value":{
                "low":[sides.clone(), sides.clone(), bottom],
                "high":[sides.clone(), sides, top]}});
        } else {
            losses.push(F3dLossCode::CamStockOffsetModeUnread.note(format!(
                "CAM stock offset mode {:?} is not the simple mode; the stock keeps its source mode name and the offsets stay in the setup parameters",
                named.text("job_stockOffsetMode").unwrap_or("")
            )));
        }
    }
    Some(json!({"value":value, "parameters":named.choice(&read)}))
}

/// The WCS rules: `wcs_orientation_mode` `modelOrientation` is the model
/// axes; every other mode keeps its source name.
fn wcs(named: &Named<'_>) -> Value {
    let mut wcs = json!({});
    if let Some(mode) = named.text("wcs_origin_mode") {
        wcs["origin"] = json!({"value":{"kind":"native","value":mode},
            "parameters":named.choice(&["wcs_origin_mode"])});
    }
    if let Some(mode) = named.text("wcs_orientation_mode") {
        let value = if mode == "modelOrientation" {
            json!({"kind":"model"})
        } else {
            json!({"kind":"native","value":mode})
        };
        wcs["orientation"] =
            json!({"value":value, "parameters":named.choice(&["wcs_orientation_mode"])});
    }
    wcs
}

fn process_id(id: &str) -> String {
    id.replace(":cam_setup#", ":setup#")
        .replace(":cam_operation#", ":operation#")
        .replace(":cam_tool#", ":tool#")
}

fn project(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    cam: &Manufacturing,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let scope = ir
        .model
        .product_definitions
        .iter()
        .find(|definition| definition.is_root())
        .or_else(|| ir.model.product_definitions.first())
        .ok_or_else(|| CodecError::malformed("CAM setup requires a document definition"))?
        .id
        .clone();
    let mut steps: BTreeMap<&SetupId, Vec<Value>> = BTreeMap::new();
    for operation in &cam.operations {
        steps
            .entry(&operation.setup)
            .or_default()
            .push(json!({"kind":"operation","value":process_id(operation.id.as_str())}));
    }
    for setup in &cam.setups {
        let ids = parameters(
            ctx,
            ir,
            &json!({"kind":"setup","value":process_id(setup.id.as_str())}),
            setup.id.as_str(),
            &setup.parameters,
        )?;
        let named = Named {
            source: &setup.parameters,
            ids: &ids,
        };
        let mut record = json!({"id":process_id(setup.id.as_str()),"scope":scope,"steps":[],"workpieces":[],"wcs":{},"work_offsets":[],"native_ref":setup.id});
        if let Some(name) = &setup.name {
            record["name"] = json!(name);
        }
        if let Some(stock) = stock(&named, losses) {
            record["stock"] = stock;
        }
        record["wcs"] = wcs(&named);
        record["steps"] = json!(steps.get(&setup.id).map_or(&[][..], Vec::as_slice));
        ir.process.setups.push(model(record)?);
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
            if let Some(value) = names.iter().find_map(|name| {
                tool.parameters
                    .iter()
                    .find(|parameter| parameter.name == *name)
                    .and_then(|parameter| parameter.value.parse::<f64>().ok())
                    .filter(|value| value.is_finite())
            }) {
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
            ctx,
            ir,
            &json!({"kind":"tool","value":process_id(tool.id.as_str())}),
            tool.id.as_str(),
            &tool.parameters,
        )?;
    }
    let setups: std::collections::BTreeSet<&SetupId> =
        cam.setups.iter().map(|setup| &setup.id).collect();
    for operation in &cam.operations {
        use super::records::ManufacturingStrategy;
        if !setups.contains(&operation.setup) {
            return Err(CodecError::malformed(
                "CAM operation names a setup that no CAM record defines",
            ));
        }
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
        let ids = parameters(
            ctx,
            ir,
            &json!({"kind":"operation","value":process_id(operation.id.as_str())}),
            operation.id.as_str(),
            &operation.parameters,
        )?;
        record["technology"] = technology(&Named {
            source: &operation.parameters,
            ids: &ids,
        });
        ir.process.operations.push(model(record)?);
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

#[cfg(test)]
mod tests;
