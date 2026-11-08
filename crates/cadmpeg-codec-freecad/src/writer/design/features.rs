// SPDX-License-Identifier: Apache-2.0
//! Inverse mappings from neutral feature operations to typed `FreeCAD` properties.

use super::super::xml::{self, Object};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::Feature;
use cadmpeg_ir::report::loss::LossNote;
use std::collections::HashMap;

mod construction;
mod holes;
mod modifiers;
mod patterns;
mod primitives;
mod profiles;
mod properties;

use construction::{axis_placement, construction, frame_placement};
use holes::hole;
use modifiers::{binder, dress_up, shell};
use patterns::pattern;
use primitives::primitive;
use profiles::{extrude, loft, revolve, sweep};
use properties::{
    angle, boolean, enumeration, length, link, links, name, number, selection, selector, tag, text,
    unsupported, vector,
};

#[derive(Debug)]
pub(in crate::writer) enum FeatureError {
    Unsupported(String),
    Codec(CodecError),
}

impl From<CodecError> for FeatureError {
    fn from(error: CodecError) -> Self {
        Self::Codec(error)
    }
}

/// Generated construction objects accompany their consuming feature.
pub(in crate::writer) struct FeatureOutput {
    pub(in crate::writer) losses: Vec<LossNote>,
    pub(in crate::writer) objects: Vec<Object>,
    pub(in crate::writer) entries: Vec<(String, Vec<u8>)>,
}

/// Write a neutral operation. Unsupported content is an internal admission
/// signal; the caller drops that feature and charges its export loss.
/// Retained native XML for one property, when the decoder holds its value.
/// Returns the rendered `<Property ...>` element for comparing or replaying.
fn retained_xml(
    ir: &cadmpeg_ir::CadIr,
    names: &HashMap<String, String>,
    owner: Option<&str>,
    name: &str,
) -> Option<String> {
    let owner = owner?;
    let namespace = ir.native.namespace("fcstd")?;
    let properties = namespace
        .arena_as::<crate::native::PropertyRecord>("properties")
        .ok()?;
    let property = properties
        .into_iter()
        .find(|property| property.owner == owner && property.name == name)?;
    xml::native_named(&property, names, &std::collections::HashSet::new())
        .ok()
        .flatten()
}

/// Read a retained axis-reversal flag from the native property arena.
/// The decoder holds these flags on native records of the feature's
/// object; absent flags read as clear, matching the decoder default.
fn retained_flag(ir: &cadmpeg_ir::CadIr, owner: Option<&str>, name: &str) -> bool {
    let Some(owner) = owner else {
        return false;
    };
    let Some(namespace) = ir.native.namespace("fcstd") else {
        return false;
    };
    namespace
        .arena_as::<crate::native::PropertyRecord>("properties")
        .ok()
        .and_then(|properties| {
            properties
                .into_iter()
                .find(|property| property.owner == owner && property.name == name)
        })
        .and_then(|property| {
            property
                .values()
                .first()
                .and_then(|value| value.attributes.get("value"))
                .and_then(|value| value.parse::<bool>().ok())
        })
        .unwrap_or(false)
}

/// Negate a pattern direction vector written with a reversal flag set.
fn negate_direction(value: &mut serde_json::Value) {
    let Some(direction) = value
        .get_mut("direction")
        .and_then(serde_json::Value::as_object_mut)
    else {
        return;
    };
    for key in ["x", "y", "z"] {
        let component = direction
            .get(key)
            .and_then(serde_json::Value::as_number)
            .and_then(serde_json::Number::as_f64);
        if let Some(component) = component {
            if let Some(slot) = direction.get_mut(key) {
                *slot = serde_json::json!(-component);
            }
        }
    }
}

pub(in crate::writer) fn write(
    ir: &cadmpeg_ir::CadIr,
    feature: &Feature,
    object: &mut Object,
    names: &HashMap<String, String>,
    schema: &str,
) -> Result<FeatureOutput, FeatureError> {
    let definition = match ir.resolved_feature_definition(feature.evaluation.definition()) {
        Ok(definition) => definition,
        Err(cadmpeg_ir::features::DimensionResolutionError::UnevaluatedParameter { parameter })
            if is_given_up_blind_length(feature, &parameter) =>
        {
            resolve_without_blind_length(ir, feature, &parameter)?
        }
        Err(error) => return Err(FeatureError::Unsupported(error.to_string())),
    };
    let value = if definition["definition"] == "post_process" {
        definition["operation"].clone()
    } else {
        definition
    };
    let mut output = FeatureOutput {
        losses: Vec::new(),
        objects: Vec::new(),
        entries: Vec::new(),
    };
    let kind = tag(&value);
    object.set(
        "Label",
        xml::string("Label", feature.name.as_deref().unwrap_or(&object.name)),
    );
    if let Some(suppressed) = feature.suppressed {
        boolean(object, "Suppressed", suppressed);
    }
    if feature.needs_recompute {
        if schema == "2" {
            // The schema 2 grammar carries no recompute marker, so the
            // writer reports the stored staleness instead of dropping it
            // (#209 §2.5).
            output.losses.push(
                cadmpeg_ir::codec::write::ExportLossCode::EvaluationStale.note(format!(
                    "feature {} needs recompute and the schema 2 grammar carries no recompute marker",
                    feature.id
                )),
            );
        }
        object
            .declaration_attributes
            .insert("Touched".into(), "1".into());
    } else {
        object.declaration_attributes.remove("Touched");
    }
    object.dependencies = feature
        .dependencies
        .iter()
        .map(|id| name(id.as_str(), names))
        .collect::<Result<_, _>>()?;
    match kind {
        "tree_node" => {
            object.kind = if value["role"] == "solid_bodies" {
                "PartDesign::Body"
            } else {
                "App::DocumentObjectGroup"
            }
            .into();
            links(object, "Group", &value["children"]["children"], names)?;
            if !value["children"]["active_child"].is_null() {
                link(object, "Tip", &value["children"]["active_child"], names)?;
            }
        }
        "stored_geometry" | "base_feature" => object.kind = "PartDesign::Feature".into(),
        "sketch" => object.kind = "Sketcher::SketchObject".into(),
        "block" | "primitive" => {
            if !matches!(value["op"].as_str(), Some("new_body" | "join" | "cut")) {
                return unsupported(
                    "primitive boolean operation has no native construction encoding",
                );
            }
            if kind == "block" {
                let dimensions = value["dimensions"]
                    .as_array()
                    .filter(|values| values.len() == 3)
                    .ok_or_else(|| {
                        FeatureError::Unsupported("block dimensions are unresolved".into())
                    })?;
                object.kind = "Part::Box".into();
                for (dimension, field) in dimensions.iter().zip(["Length", "Width", "Height"]) {
                    length(object, field, number(dimension, field)?);
                }
            } else {
                primitive(&value["solid"], object)?;
            }
            if !value["placement"].is_null() {
                let transform: cadmpeg_ir::transform::Transform =
                    serde_json::from_value(value["placement"].clone())
                        .map_err(|error| FeatureError::Unsupported(error.to_string()))?;
                object.set("Placement", super::placement(transform)?);
            }
            if value["op"] == "join" {
                object.kind = object.kind.replace("Part::", "PartDesign::Additive");
            }
            if value["op"] == "cut" {
                object.kind = object.kind.replace("Part::", "PartDesign::Subtractive");
            }
        }
        "extrude" => extrude(&value, object, names, &mut output.losses)?,
        "revolve" => revolve(&value, object, names)?,
        "helical_sweep" => profiles::helical_sweep(&value, object, names)?,
        "loft" => loft(&value, object, names)?,
        "sweep" => sweep(&value, object, names)?,
        "hole" => hole(&value, object, names)?,
        "fillet" | "chamfer" => dress_up(&value, object, names)?,
        "shell" | "offset_shape" => shell(&value, object, names)?,
        "draft" => {
            object.kind = "PartDesign::Draft".into();
            selection(object, "Base", &value["faces"], names)?;
            selection(object, "NeutralPlane", &value["anchor"]["plane"], names)?;
            angle(object, "Angle", number(&value["angle"], "draft angle")?);
            boolean(
                object,
                "Reversed",
                value["outward"].as_bool().unwrap_or(false),
            );
        }
        "combine" => {
            let standalone = match (feature.source_tag.as_deref(), value["op"].as_str()) {
                (Some("Part::Cut"), Some("cut")) => Some("Part::Cut"),
                (Some("Part::Fuse"), Some("join")) => Some("Part::Fuse"),
                (Some("Part::Common"), Some("intersect")) => Some("Part::Common"),
                _ => None,
            };
            if let Some(kind) = standalone.filter(|_| {
                properties::references(&value["operands"]["tools"], names)
                    .is_ok_and(|tools| tools.len() == 1)
            }) {
                object.kind = kind.into();
                link(object, "Base", &value["operands"]["target"], names)?;
                link(object, "Tool", &value["operands"]["tools"], names)?;
            } else {
                object.kind = "PartDesign::Boolean".into();
                link(object, "BaseFeature", &value["operands"]["target"], names)?;
                links(object, "Group", &value["operands"]["tools"], names)?;
                enumeration(
                    object,
                    "Type",
                    selector(&value["op"], &["join", "cut", "intersect"])?,
                );
            }
        }
        "pattern" => {
            links(object, "Originals", &value["seeds"], names)?;
            let pattern_value = &value["pattern"];
            // The neutral pattern vector is the resolved final direction.
            // FreeCAD reads `Direction` back through the `Reversed` flag,
            // so a reversed pattern writes its raw (unreversed) vector
            // and re-states the flag value unchanged.
            let reversed1 = retained_flag(ir, feature.native_ref.as_deref(), "Reversed");
            let reversed2 = retained_flag(ir, feature.native_ref.as_deref(), "Reversed2");
            let mut pattern_json = pattern_value.clone();
            // A native link-sub axis definition re-defines the resolved
            // direction form entirely; the generated PropertyVector would
            // lose its DatumAxis identity. Emitting the retained link lets
            // the decoder read the same datum again.
            let direction_link =
                retained_xml(ir, names, feature.native_ref.as_deref(), "Direction")
                    .filter(|xml| xml.contains("App::PropertyLinkSub"));
            let direction2_link =
                retained_xml(ir, names, feature.native_ref.as_deref(), "Direction2")
                    .filter(|xml| xml.contains("App::PropertyLinkSub"));
            if matches!(tag(pattern_value), "linear" | "linear_offsets") {
                if reversed1 {
                    negate_direction(&mut pattern_json);
                }
                if reversed2 {
                    if let Some(second) = pattern_json.get_mut("second") {
                        negate_direction(second);
                    }
                }
            }
            boolean(object, "Reversed", reversed1);
            boolean(object, "Reversed2", reversed2);
            pattern(
                &pattern_json,
                object,
                names,
                &mut output.objects,
                &mut output.entries,
                &mut output.losses,
            )?;
            if let Some(xml) = direction_link {
                object.set("Direction", xml);
            }
            if let Some(xml) = direction2_link {
                object.set("Direction2", xml);
            }
        }
        "datum_plane" | "datum_three_point_plane" => {
            object.kind = "PartDesign::Plane".into();
            frame_placement(object, &value["frame"])?;
        }
        "datum_axis" => {
            object.kind = "PartDesign::Line".into();
            axis_placement(object, &value["origin"], &value["direction"])?;
        }
        "datum_point" | "point_geometry" => {
            object.kind = if kind == "datum_point" {
                "PartDesign::Point"
            } else {
                "Part::Vertex"
            }
            .into();
            if kind == "datum_point" {
                axis_placement(
                    object,
                    &value["position"],
                    &serde_json::json!({"x":0.0,"y":0.0,"z":1.0}),
                )?;
            } else {
                for (property, coordinate) in [("X", "x"), ("Y", "y"), ("Z", "z")] {
                    length(
                        object,
                        property,
                        number(&value["position"][coordinate], coordinate)?,
                    );
                }
            }
        }
        "datum_coordinate_system" => {
            object.kind = "PartDesign::CoordinateSystem".into();
            frame_placement(object, &value["frame"])?;
        }
        "binder" => binder(&value, object, names)?,
        "compound" => {
            object.kind = "Part::Compound".into();
            links(object, "Links", &value["members"], names)?;
        }
        "refine_shape" | "reverse_shape" => {
            object.kind = if kind == "refine_shape" {
                "Part::Refine"
            } else {
                "Part::Reverse"
            }
            .into();
            link(object, "Source", &value["source"], names)?;
        }
        "derived_geometry" => {
            object.kind = "PartDesign::FeatureBase".into();
            link(object, "BaseFeature", &value["source"], names)?;
        }
        "imported_geometry" => {
            object.kind = match value["format"].as_str() {
                Some("step") => "Part::ImportStep",
                Some("iges") => "Part::ImportIges",
                Some("brep") => "Part::ImportBrep",
                _ => return unsupported("import format has no FreeCAD reader mapping"),
            }
            .into();
            object.set(
                "FileName",
                xml::string("FileName", text(&value["path"], "import path")?),
            );
        }
        "face_from_shapes" => {
            object.kind = "Part::Face".into();
            links(object, "Sources", &value["sources"], names)?;
            object.set(
                "FaceMakerClass",
                xml::string(
                    "FaceMakerClass",
                    text(&value["face_maker_class"], "face maker")?,
                ),
            );
        }
        "ruled_between_curves" => {
            object.kind = "Part::RuledSurface".into();
            selection(object, "Curve1", &value["first"], names)?;
            selection(object, "Curve2", &value["second"], names)?;
            enumeration(
                object,
                "Orientation",
                selector(&value["orientation"], &["automatic", "forward", "reversed"])?,
            );
        }
        "section_shape" => {
            object.kind = "Part::Section".into();
            link(object, "Base", &value["operands"]["first"], names)?;
            link(object, "Tool", &value["operands"]["second"], names)?;
            boolean(
                object,
                "Approximation",
                value["approximate"].as_bool().unwrap_or(false),
            );
        }
        "mirror_shape" => {
            object.kind = "Part::Mirroring".into();
            link(object, "Source", &value["source"], names)?;
            vector(object, "Base", &value["plane_origin"])?;
            vector(object, "Normal", &value["plane_normal"])?;
        }
        "project_on_surface" => {
            object.kind = "Part::ProjectOnSurface".into();
            selection(object, "Projection", &value["sources"], names)?;
            selection(object, "SupportFace", &value["support_face"], names)?;
            enumeration(
                object,
                "Mode",
                selector(&value["mode"], &["all", "faces", "edges"])?,
            );
            vector(object, "Direction", &value["direction"])?;
            length(
                object,
                "Height",
                number(&value["height"], "projection height")?,
            );
            length(
                object,
                "Offset",
                number(&value["offset"], "projection offset")?,
            );
        }
        "line_segment"
        | "circular_arc"
        | "elliptic_arc"
        | "polyline"
        | "regular_polygon_curve"
        | "planar_patch"
        | "helix"
        | "scale" => construction(&value, object, names, &mut output.entries)?,
        "native" => {} // The orchestrator emits this feature's validated native properties.
        _ => {
            return unsupported(format!(
                "neutral feature operation `{kind}` has no FreeCAD construction mapping"
            ))
        }
    }
    if let cadmpeg_ir::features::FeatureDefinition::PostProcess {
        refine,
        fuzzy_tolerance,
        ..
    } = feature.evaluation.definition()
    {
        boolean(object, "Refine", *refine);
        let tolerance = match fuzzy_tolerance {
            cadmpeg_ir::features::FuzzyTolerance::KernelDefault => 0.0,
            cadmpeg_ir::features::FuzzyTolerance::Automatic => -1.0,
            cadmpeg_ir::features::FuzzyTolerance::Explicit(value) => ir
                .dimension_value(value)
                .ok_or_else(|| {
                    FeatureError::Unsupported(
                        "fuzzy tolerance parameter has no evaluated positive length".into(),
                    )
                })?
                .get(),
        };
        // A kernel-default tolerance carries no distinct value the decoder
        // needs, and writing one appends a property outside the source
        // declaration sequence, noisily reordering applications and
        // schemas downstream. Emit only non-default values.
        if !matches!(
            fuzzy_tolerance,
            cadmpeg_ir::features::FuzzyTolerance::KernelDefault
        ) {
            object.set(
                "FuzzyTolerance",
                xml::scalar(
                    "FuzzyTolerance",
                    "App::PropertyFloatConstraint",
                    "Float",
                    tolerance,
                ),
            );
        }
    }
    if kind == "tree_node" {
        object.mark_grouping_only();
    }
    // The `<Object type>` attribute is the feature source tag's native home:
    // the writer keeps the tag when it names the derived construction type.
    // An opaque definition (stored geometry or a base feature) is encoded by
    // whatever type holds it, so a present tag is kept without a loss;
    // otherwise the writer keeps the derived type and charges the difference
    // (#238 §5.5).
    if let Some(tag) = feature.source_tag.as_deref() {
        if tag != object.kind {
            if matches!(kind, "stored_geometry" | "base_feature") {
                object.kind = tag.into();
            } else {
                output.losses.push(
                    crate::loss::FreecadLossCode::FeatureSourceTagNotTransferred.note(format!(
                        "feature {} source tag {tag} is written as {}",
                        feature.id, object.kind
                    )),
                );
            }
        }
    }
    Ok(output)
}

/// Whether the unevaluated parameter is a blind extrusion length: the
/// Pad-without-Length degradation (§7.11.2) applies only to that lane, while
/// any other unevaluated dimension still omits the whole feature.
fn is_given_up_blind_length(
    feature: &Feature,
    parameter: &cadmpeg_ir::features::ParameterId,
) -> bool {
    !blind_length_sides(feature, parameter).is_empty()
}

/// Sides of an extrude whose blind length binds the parameter, as resolved
/// definition paths (`side` for one-sided and symmetric, `first`/`second`
/// for two-sided).
fn blind_length_sides(
    feature: &Feature,
    parameter: &cadmpeg_ir::features::ParameterId,
) -> Vec<&'static str> {
    use cadmpeg_ir::features::{ExtrudeExtent, FeatureOperation, LinearTermination};
    let FeatureOperation::Extrude { extent, .. } = feature.evaluation.definition().operation()
    else {
        return Vec::new();
    };
    let sides: Vec<(&str, &cadmpeg_ir::features::ExtrudeSide)> = match extent {
        ExtrudeExtent::OneSided { side } => vec![("side", side)],
        ExtrudeExtent::Symmetric { side } => vec![("side", side)],
        ExtrudeExtent::TwoSided { first, second } => {
            vec![("first", first), ("second", second)]
        }
    };
    sides
        .into_iter()
        .filter_map(|(key, side)| {
            matches!(&side.termination, LinearTermination::Blind { length } if length.parameter() == Some(parameter))
                .then_some(key)
        })
        .collect()
}

/// Resolve the definition with a placeholder for the given-up blind length,
/// then null that lane out. The placeholder lets the remaining lanes
/// resolve; it is discarded before writing, so no fabricated value reaches
/// the output and the lane omission loss names the gap.
fn resolve_without_blind_length(
    ir: &cadmpeg_ir::CadIr,
    feature: &Feature,
    parameter: &cadmpeg_ir::features::ParameterId,
) -> Result<serde_json::Value, FeatureError> {
    let sides = blind_length_sides(feature, parameter);
    if sides.is_empty() {
        return Err(FeatureError::Unsupported(format!(
            "blind length parameter {parameter} has no resolved home"
        )));
    }
    let mut projection = ir.clone();
    let Some(record) = projection
        .model
        .parameters
        .iter_mut()
        .find(|record| record.id == *parameter)
    else {
        return Err(FeatureError::Unsupported(format!(
            "blind length parameter {parameter} has no resolved home"
        )));
    };
    record.value = Some(cadmpeg_ir::features::ParameterValue::Length(
        cadmpeg_ir::scalar::Length::new(1.0).ok_or_else(|| {
            FeatureError::Unsupported("placeholder blind length is inadmissible".into())
        })?,
    ));
    let mut value = projection
        .resolved_feature_definition(feature.evaluation.definition())
        .map_err(|error| FeatureError::Unsupported(error.to_string()))?;
    for side in sides {
        let path = format!("/extent/{side}/termination/length");
        if value.pointer(&path).is_some() {
            if let Some(length) = value.pointer_mut(&path) {
                *length = serde_json::Value::Null;
            }
        }
    }
    Ok(value)
}

#[cfg(test)]
mod tests;
