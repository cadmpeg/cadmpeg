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

/// Generated construction objects accompany their consuming feature.
pub(in crate::writer) struct FeatureOutput {
    pub(in crate::writer) losses: Vec<LossNote>,
    pub(in crate::writer) objects: Vec<Object>,
    pub(in crate::writer) entries: Vec<(String, Vec<u8>)>,
}

/// Write a neutral operation. Unsupported content is an internal admission
/// signal; the caller drops that feature and charges its export loss.
pub(in crate::writer) fn write(
    ir: &cadmpeg_ir::CadIr,
    feature: &Feature,
    object: &mut Object,
    names: &HashMap<String, String>,
) -> Result<FeatureOutput, CodecError> {
    let definition = ir
        .resolved_feature_definition(feature.evaluation.definition())
        .map_err(|error| CodecError::UnsupportedContent(error.to_string()))?;
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
                        CodecError::UnsupportedContent("block dimensions are unresolved".into())
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
                        .map_err(CodecError::malformed)?;
                object.set("Placement", super::placement(transform)?);
            }
            if value["op"] == "join" {
                object.kind = object.kind.replace("Part::", "PartDesign::Additive");
            }
            if value["op"] == "cut" {
                object.kind = object.kind.replace("Part::", "PartDesign::Subtractive");
            }
        }
        "extrude" => extrude(&value, object, names)?,
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
            pattern(
                &value["pattern"],
                object,
                names,
                &mut output.objects,
                &mut output.entries,
                &mut output.losses,
            )?;
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
                    CodecError::UnsupportedContent(
                        "fuzzy tolerance parameter has no evaluated positive length".into(),
                    )
                })?
                .get(),
        };
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
    if kind == "tree_node" {
        object.mark_grouping_only();
    }
    Ok(output)
}

#[cfg(test)]
mod tests;
