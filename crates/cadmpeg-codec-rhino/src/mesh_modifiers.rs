// SPDX-License-Identifier: Apache-2.0
//! XML-backed mesh modifier userdata attached to object attributes.

use crate::chunks::{ArchiveVersion, BoundedReader, FramingError};
use crate::objects::AttributeUserdataDescriptor;
use crate::settings;
use crate::wire::Uuid;

const XML_USERDATA_VERSION: i32 = 2;
const DISPLACEMENT_ROOT: &str = "new-displacement-object-data";
const DISPLACEMENT_SUB: &str = "sub";
const EDGE_SOFTENING_ROOT: &str = "edge-softening-object-data";
const THICKENING_ROOT: &str = "thickening-object-data";
const CURVE_PIPING_ROOT: &str = "curve-piping-object-data";

/// The class UUID registered by `ON_DisplacementUserData`.
pub(crate) const DISPLACEMENT_CLASS: Uuid = Uuid::from_canonical([
    0xb8, 0xc0, 0x46, 0x04, 0xb4, 0xef, 0x43, 0xb7, 0x8c, 0x26, 0x1a, 0xfb, 0x8f, 0x1c, 0x54, 0xeb,
]);

/// The item UUID returned by `ON_DisplacementUserData::Uuid`.
pub(crate) const DISPLACEMENT_ITEM: Uuid = Uuid::from_canonical([
    0x82, 0x24, 0xa7, 0xc4, 0x55, 0x90, 0x4a, 0xc4, 0xa3, 0x2c, 0xde, 0x85, 0xdc, 0x2f, 0xfd, 0xae,
]);

/// The class UUID registered by `ON_EdgeSofteningUserData`.
pub(crate) const EDGE_SOFTENING_CLASS: Uuid = Uuid::from_canonical([
    0xcb, 0x5e, 0xb3, 0x95, 0xbf, 0x1b, 0x41, 0x12, 0x8f, 0x2f, 0xf7, 0x28, 0xfc, 0xe8, 0x16, 0x9c,
]);

/// The item UUID returned by `ON_EdgeSofteningUserData::Uuid`.
pub(crate) const EDGE_SOFTENING_ITEM: Uuid = Uuid::from_canonical([
    0x8c, 0xbe, 0x61, 0x60, 0x5c, 0xbd, 0x4b, 0x4d, 0x8c, 0xd2, 0x7c, 0xe0, 0xa7, 0xc8, 0xc2, 0xd8,
]);

/// The class UUID registered by `ON_ThickeningUserData`.
pub(crate) const THICKENING_CLASS: Uuid = Uuid::from_canonical([
    0xaa, 0x03, 0xd9, 0xc3, 0x4c, 0xcf, 0x44, 0x31, 0xa0, 0x6e, 0x25, 0xf3, 0x8c, 0xf3, 0x91, 0x3f,
]);

/// The item UUID returned by `ON_ThickeningUserData::Uuid`.
pub(crate) const THICKENING_ITEM: Uuid = Uuid::from_canonical([
    0x6a, 0xa7, 0xcc, 0xc3, 0x27, 0x21, 0x41, 0x0f, 0xaa, 0x56, 0xe8, 0xab, 0x4f, 0x3e, 0xce, 0x67,
]);

/// The class UUID registered by `ON_CurvePipingUserData`.
pub(crate) const CURVE_PIPING_CLASS: Uuid = Uuid::from_canonical([
    0x2d, 0x5a, 0xfe, 0xa9, 0xf4, 0x58, 0x40, 0x79, 0x99, 0x2f, 0xc2, 0xd4, 0x05, 0xd9, 0x38, 0x3b,
]);

/// The item UUID returned by `ON_CurvePipingUserData::Uuid`.
pub(crate) const CURVE_PIPING_ITEM: Uuid = Uuid::from_canonical([
    0x2b, 0x1a, 0x75, 0x8e, 0x7c, 0xb1, 0x45, 0xab, 0xa5, 0xbf, 0xdf, 0xcd, 0x6d, 0x3d, 0x13, 0x6d,
]);

/// The application UUID registered by `ON_MeshModifier::PlugInId`.
pub(crate) const MESH_MODIFIER_PLUGIN: Uuid = Uuid::from_canonical([
    0xf2, 0x93, 0xde, 0x5c, 0xd1, 0xff, 0x46, 0x7a, 0x9b, 0xd1, 0xca, 0xc8, 0xec, 0x4b, 0x2e, 0x6b,
]);

/// Typed mesh modifiers recovered from object-attributes userdata.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MeshModifiers {
    /// The displacement modifier, when the object carries one.
    pub(crate) displacement: Option<DisplacementModifier>,
    /// The edge-softening modifier, when the object carries one.
    pub(crate) edge_softening: Option<EdgeSofteningModifier>,
    /// The thickening modifier, when the object carries one.
    pub(crate) thickening: Option<ThickeningModifier>,
    /// The curve-piping modifier, when the object carries one.
    pub(crate) curve_piping: Option<CurvePipingModifier>,
}

/// The XML parameters written by `ON_DisplacementUserData`.
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct DisplacementModifier {
    /// `ON_XMLUserData` payload version.
    pub(crate) xml_version: i32,
    /// Whether displacement is enabled.
    pub(crate) on: bool,
    /// Displacement texture UUID; nil XML UUIDs are represented as `None`.
    pub(crate) texture: Option<Uuid>,
    /// Texture mapping channel.
    pub(crate) channel: i32,
    /// Black-point remapping value.
    pub(crate) black_point: f64,
    /// White-point remapping value.
    pub(crate) white_point: f64,
    /// Initial sweep quality (`sweep-pitch`).
    pub(crate) sweep_pitch: i32,
    /// Number of refinement steps.
    pub(crate) refine_steps: i32,
    /// Refinement sensitivity.
    pub(crate) refine_sensitivity: f64,
    /// Whether the final face-count limit is enabled.
    pub(crate) face_count_limit_enabled: bool,
    /// Final face-count limit.
    pub(crate) face_count_limit: i32,
    /// Post-weld angle in degrees.
    pub(crate) post_weld_angle: f64,
    /// Mesh memory limit in megabytes.
    pub(crate) mesh_memory_limit: i32,
    /// Whether fairing is enabled.
    pub(crate) fairing_enabled: bool,
    /// Fairing amount.
    pub(crate) fairing_amount: i32,
    /// Serialized sub-object count, when present.
    pub(crate) sub_object_count: Option<i32>,
    /// Sweep-resolution formula enum value.
    pub(crate) sweep_resolution_formula: i32,
    /// Ordered per-sub-object displacement overrides.
    pub(crate) sub_items: Vec<DisplacementSubItem>,
}

/// A displacement override for one sub-object face index.
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct DisplacementSubItem {
    /// Sub-object face index.
    pub(crate) face_index: i32,
    /// Whether the sub-object displacement is enabled.
    pub(crate) on: bool,
    /// Sub-object texture UUID; nil XML UUIDs are represented as `None`.
    pub(crate) texture: Option<Uuid>,
    /// Sub-object texture mapping channel.
    pub(crate) channel: i32,
    /// Sub-object black-point remapping value.
    pub(crate) black_point: f64,
    /// Sub-object white-point remapping value.
    pub(crate) white_point: f64,
}

/// The XML parameters written by `ON_EdgeSofteningUserData`.
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct EdgeSofteningModifier {
    /// `ON_XMLUserData` payload version.
    pub(crate) xml_version: i32,
    /// Whether edge softening is enabled.
    pub(crate) on: bool,
    /// Edge-softening radius.
    pub(crate) softening: f64,
    /// Whether softened edges are chamfered.
    pub(crate) chamfer: bool,
    /// Whether edges are left faceted; serialized as `unweld`.
    pub(crate) faceted: bool,
    /// Whether to soften edges despite an excessive radius.
    pub(crate) force_softening: bool,
    /// Adjacent-face angle threshold in degrees.
    pub(crate) edge_angle_threshold: f64,
}

/// The XML parameters written by `ON_ThickeningUserData`.
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct ThickeningModifier {
    /// `ON_XMLUserData` payload version.
    pub(crate) xml_version: i32,
    /// Whether thickening is enabled.
    pub(crate) on: bool,
    /// Whether an open mesh receives side walls.
    pub(crate) solid: bool,
    /// Whether thickening is applied to both sides.
    pub(crate) both_sides: bool,
    /// Whether only the offset surface is produced.
    pub(crate) offset_only: bool,
    /// Thickening distance.
    pub(crate) distance: f64,
}

/// The XML parameters written by `ON_CurvePipingUserData`.
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct CurvePipingModifier {
    /// `ON_XMLUserData` payload version.
    pub(crate) xml_version: i32,
    /// Whether curve piping is enabled.
    pub(crate) on: bool,
    /// Pipe radius.
    pub(crate) radius: f64,
    /// Number of pipe segments.
    pub(crate) segments: i32,
    /// Whether the pipe is faceted; serialized as the inverse `weld` value.
    pub(crate) faceted: bool,
    /// Pipe accuracy from 0 through 100.
    pub(crate) accuracy: i32,
    /// Cap type: `none`, `flat`, `box`, or `dome`.
    pub(crate) cap_type: String,
}

/// Reads the first matching mesh-modifier items from an object-attributes userdata stream.
pub(crate) fn parse_attribute_userdata(
    bytes: &[u8],
    descriptors: &[AttributeUserdataDescriptor],
    archive: ArchiveVersion,
    warnings: &mut Vec<String>,
) -> Option<MeshModifiers> {
    let displacement_descriptor =
        first_matching_descriptor(descriptors, DISPLACEMENT_CLASS, DISPLACEMENT_ITEM);
    let edge_softening_descriptor =
        first_matching_descriptor(descriptors, EDGE_SOFTENING_CLASS, EDGE_SOFTENING_ITEM);
    let thickening_descriptor =
        first_matching_descriptor(descriptors, THICKENING_CLASS, THICKENING_ITEM);
    let curve_piping_descriptor =
        first_matching_descriptor(descriptors, CURVE_PIPING_CLASS, CURVE_PIPING_ITEM);
    if displacement_descriptor.is_none()
        && edge_softening_descriptor.is_none()
        && thickening_descriptor.is_none()
        && curve_piping_descriptor.is_none()
    {
        return None;
    }

    let displacement = displacement_descriptor.and_then(|descriptor| {
        let Some(payload_range) = descriptor.payload_range.clone() else {
            warnings.push(format!(
                "displacement userdata at {} has no bounded payload",
                descriptor.range.start
            ));
            return None;
        };
        match parse_displacement(bytes, payload_range, archive) {
            Ok(displacement) => Some(displacement),
            Err(error) => {
                warnings.push(format!(
                    "displacement userdata at {} dropped: {error}",
                    descriptor.range.start
                ));
                None
            }
        }
    });
    let edge_softening = edge_softening_descriptor.and_then(|descriptor| {
        let Some(payload_range) = descriptor.payload_range.clone() else {
            warnings.push(format!(
                "edge-softening userdata at {} has no bounded payload",
                descriptor.range.start
            ));
            return None;
        };
        match parse_edge_softening(bytes, payload_range) {
            Ok(edge_softening) => Some(edge_softening),
            Err(error) => {
                warnings.push(format!(
                    "edge-softening userdata at {} dropped: {error}",
                    descriptor.range.start
                ));
                None
            }
        }
    });
    let thickening = thickening_descriptor.and_then(|descriptor| {
        let Some(payload_range) = descriptor.payload_range.clone() else {
            warnings.push(format!(
                "thickening userdata at {} has no bounded payload",
                descriptor.range.start
            ));
            return None;
        };
        match parse_thickening(bytes, payload_range) {
            Ok(thickening) => Some(thickening),
            Err(error) => {
                warnings.push(format!(
                    "thickening userdata at {} dropped: {error}",
                    descriptor.range.start
                ));
                None
            }
        }
    });
    let curve_piping = curve_piping_descriptor.and_then(|descriptor| {
        let Some(payload_range) = descriptor.payload_range.clone() else {
            warnings.push(format!(
                "curve-piping userdata at {} has no bounded payload",
                descriptor.range.start
            ));
            return None;
        };
        match parse_curve_piping(bytes, payload_range) {
            Ok(curve_piping) => Some(curve_piping),
            Err(error) => {
                warnings.push(format!(
                    "curve-piping userdata at {} dropped: {error}",
                    descriptor.range.start
                ));
                None
            }
        }
    });
    (displacement.is_some()
        || edge_softening.is_some()
        || thickening.is_some()
        || curve_piping.is_some())
    .then_some(MeshModifiers {
        displacement,
        edge_softening,
        thickening,
        curve_piping,
    })
}

fn first_matching_descriptor(
    descriptors: &[AttributeUserdataDescriptor],
    class_uuid: Uuid,
    item_uuid: Uuid,
) -> Option<&AttributeUserdataDescriptor> {
    descriptors.iter().find(|descriptor| {
        descriptor.class_uuid == Some(class_uuid)
            && descriptor.item_uuid == Some(item_uuid)
            && descriptor.application_uuid == Some(MESH_MODIFIER_PLUGIN)
    })
}

fn parse_displacement(
    bytes: &[u8],
    payload_range: std::ops::Range<usize>,
    archive: ArchiveVersion,
) -> Result<DisplacementModifier, FramingError> {
    let (xml_version, xml) = parse_xml_userdata(bytes, payload_range)?;
    parse_xml(&xml, xml_version, archive)
}

fn parse_edge_softening(
    bytes: &[u8],
    payload_range: std::ops::Range<usize>,
) -> Result<EdgeSofteningModifier, FramingError> {
    let (xml_version, xml) = parse_xml_userdata(bytes, payload_range)?;
    parse_edge_softening_xml(&xml, xml_version)
}

fn parse_thickening(
    bytes: &[u8],
    payload_range: std::ops::Range<usize>,
) -> Result<ThickeningModifier, FramingError> {
    let (xml_version, xml) = parse_xml_userdata(bytes, payload_range)?;
    parse_thickening_xml(&xml, xml_version)
}

fn parse_curve_piping(
    bytes: &[u8],
    payload_range: std::ops::Range<usize>,
) -> Result<CurvePipingModifier, FramingError> {
    let (xml_version, xml) = parse_xml_userdata(bytes, payload_range)?;
    parse_curve_piping_xml(&xml, xml_version)
}

fn parse_xml_userdata(
    bytes: &[u8],
    payload_range: std::ops::Range<usize>,
) -> Result<(i32, String), FramingError> {
    let mut reader = BoundedReader::new(bytes, payload_range.start, payload_range.end)?;
    let xml_version = reader.i32()?;
    let xml = match xml_version {
        1 => settings::utf16(&mut reader)?,
        XML_USERDATA_VERSION => {
            let length_offset = reader.position();
            let length = reader.i32()?;
            let length = usize::try_from(length).map_err(|_| {
                FramingError::structural(length_offset, "negative XML UTF-8 length")
            })?;
            if length > reader.remaining() {
                return Err(FramingError::structural(
                    length_offset,
                    format!(
                        "XML UTF-8 length {length} exceeds userdata payload boundary {}",
                        reader.remaining()
                    ),
                ));
            }
            let raw = reader.take(length)?;
            std::str::from_utf8(raw)
                .map(str::to_owned)
                .map_err(|_| FramingError::structural(length_offset, "XML payload is not UTF-8"))?
        }
        version => {
            return Err(FramingError::structural(
                payload_range.start,
                format!("XML userdata version {version} is unsupported"),
            ));
        }
    };
    reader.skip_remaining()?;
    Ok((xml_version, xml))
}

fn parse_xml(
    xml: &str,
    xml_version: i32,
    archive: ArchiveVersion,
) -> Result<DisplacementModifier, FramingError> {
    let document = roxmltree::Document::parse(xml).map_err(|error| {
        FramingError::structural(0, format!("invalid displacement XML: {error}"))
    })?;
    let root = document.root_element();
    if !same_name(root, "xml") {
        return Err(FramingError::structural(
            0,
            format!(
                "displacement XML root is `{}`, expected `xml`",
                root.tag_name().name()
            ),
        ));
    }
    let displacement = direct_child(root, DISPLACEMENT_ROOT).ok_or_else(|| {
        FramingError::structural(
            0,
            format!("displacement XML has no `{DISPLACEMENT_ROOT}` child"),
        )
    })?;
    let sweep_resolution_formula = field_i32_optional(displacement, "sweep-res-formula")
        .unwrap_or_else(|| i32::from(archive.value() < 60));
    let sub_items = displacement
        .children()
        .filter(|node| node.is_element() && same_name(*node, DISPLACEMENT_SUB))
        .map(parse_sub_item)
        .collect();
    Ok(DisplacementModifier {
        xml_version,
        on: field_bool(displacement, "on", false),
        texture: field_uuid(displacement, "texture"),
        channel: field_i32(displacement, "channel", 0),
        black_point: field_f64(displacement, "black-point", 0.0),
        white_point: field_f64(displacement, "white-point", 1.0),
        sweep_pitch: field_i32(displacement, "sweep-pitch", 1000),
        refine_steps: field_i32(displacement, "refine-steps", 1),
        refine_sensitivity: field_f64(displacement, "refine-sensitivity", 0.5),
        face_count_limit_enabled: field_bool(displacement, "face-count-limit-enabled", false),
        face_count_limit: field_i32(displacement, "face-count-limit", 10_000),
        post_weld_angle: field_f64(displacement, "post-weld-angle", 40.0),
        mesh_memory_limit: field_i32(displacement, "mesh-memory-limit", 512),
        fairing_enabled: field_bool(displacement, "fairing-enabled", false),
        fairing_amount: field_i32(displacement, "fairing-amount", 4),
        sub_object_count: field_i32_optional(displacement, "sub-object-count"),
        sweep_resolution_formula,
        sub_items,
    })
}

fn parse_edge_softening_xml(
    xml: &str,
    xml_version: i32,
) -> Result<EdgeSofteningModifier, FramingError> {
    let document = roxmltree::Document::parse(xml).map_err(|error| {
        FramingError::structural(0, format!("invalid edge-softening XML: {error}"))
    })?;
    let root = document.root_element();
    if !same_name(root, "xml") {
        return Err(FramingError::structural(
            0,
            format!(
                "edge-softening XML root is `{}`, expected `xml`",
                root.tag_name().name()
            ),
        ));
    }
    let edge_softening = direct_child(root, EDGE_SOFTENING_ROOT).ok_or_else(|| {
        FramingError::structural(
            0,
            format!("edge-softening XML has no `{EDGE_SOFTENING_ROOT}` child"),
        )
    })?;
    Ok(EdgeSofteningModifier {
        xml_version,
        on: field_bool(edge_softening, "on", false),
        softening: field_f64(edge_softening, "softening", 0.1),
        chamfer: field_bool(edge_softening, "chamfer", false),
        faceted: field_bool(edge_softening, "unweld", false),
        force_softening: field_bool(edge_softening, "force-softening", false),
        edge_angle_threshold: field_f64(edge_softening, "edge-threshold", 5.0),
    })
}

fn parse_thickening_xml(xml: &str, xml_version: i32) -> Result<ThickeningModifier, FramingError> {
    let document = roxmltree::Document::parse(xml)
        .map_err(|error| FramingError::structural(0, format!("invalid thickening XML: {error}")))?;
    let root = document.root_element();
    if !same_name(root, "xml") {
        return Err(FramingError::structural(
            0,
            format!(
                "thickening XML root is `{}`, expected `xml`",
                root.tag_name().name()
            ),
        ));
    }
    let thickening = direct_child(root, THICKENING_ROOT).ok_or_else(|| {
        FramingError::structural(
            0,
            format!("thickening XML has no `{THICKENING_ROOT}` child"),
        )
    })?;
    Ok(ThickeningModifier {
        xml_version,
        on: field_bool(thickening, "on", false),
        solid: field_bool(thickening, "solid", true),
        both_sides: field_bool(thickening, "both-sides", false),
        offset_only: field_bool(thickening, "offset-only", false),
        distance: field_f64(thickening, "distance", 0.1),
    })
}

fn parse_curve_piping_xml(
    xml: &str,
    xml_version: i32,
) -> Result<CurvePipingModifier, FramingError> {
    let document = roxmltree::Document::parse(xml).map_err(|error| {
        FramingError::structural(0, format!("invalid curve-piping XML: {error}"))
    })?;
    let root = document.root_element();
    if !same_name(root, "xml") {
        return Err(FramingError::structural(
            0,
            format!(
                "curve-piping XML root is `{}`, expected `xml`",
                root.tag_name().name()
            ),
        ));
    }
    let curve_piping = direct_child(root, CURVE_PIPING_ROOT).ok_or_else(|| {
        FramingError::structural(
            0,
            format!("curve-piping XML has no `{CURVE_PIPING_ROOT}` child"),
        )
    })?;
    Ok(CurvePipingModifier {
        xml_version,
        on: field_bool(curve_piping, "on", false),
        radius: field_f64(curve_piping, "radius", 1.0),
        segments: field_i32(curve_piping, "segments", 16),
        faceted: !field_bool(curve_piping, "weld", true),
        accuracy: field_i32(curve_piping, "accuracy", 50),
        cap_type: field_cap_type(curve_piping, "cap-type"),
    })
}

fn parse_sub_item(node: roxmltree::Node<'_, '_>) -> DisplacementSubItem {
    DisplacementSubItem {
        face_index: field_i32(node, "sub-index", -1),
        on: field_bool(node, "sub-on", false),
        texture: field_uuid(node, "sub-texture"),
        channel: field_i32(node, "sub-channel", 0),
        black_point: field_f64(node, "sub-black-point", 0.0),
        white_point: field_f64(node, "sub-white-point", 1.0),
    }
}

fn direct_child<'a, 'input>(
    parent: roxmltree::Node<'a, 'input>,
    name: &str,
) -> Option<roxmltree::Node<'a, 'input>> {
    parent
        .children()
        .find(|node| node.is_element() && same_name(*node, name))
}

fn typed_child<'a, 'input>(
    parent: roxmltree::Node<'a, 'input>,
    name: &str,
) -> Option<roxmltree::Node<'a, 'input>> {
    let node = direct_child(parent, name)?;
    attribute(node, "type")?;
    Some(node)
}

fn attribute<'a>(node: roxmltree::Node<'a, '_>, name: &str) -> Option<&'a str> {
    node.attributes()
        .find(|attribute| attribute.name().eq_ignore_ascii_case(name))
        .map(|attribute| attribute.value())
}

fn field_bool(parent: roxmltree::Node<'_, '_>, name: &str, default: bool) -> bool {
    let Some(node) = typed_child(parent, name) else {
        return default;
    };
    let text = node.text().unwrap_or_default().trim();
    let kind = attribute(node, "type").unwrap_or_default();
    if kind.eq_ignore_ascii_case("string") {
        text.eq_ignore_ascii_case("true")
            || text.eq_ignore_ascii_case("t")
            || text.parse::<i32>().is_ok_and(|value| value != 0)
    } else if kind.eq_ignore_ascii_case("bool") {
        text.eq_ignore_ascii_case("true") || text.eq_ignore_ascii_case("t") || text == "1"
    } else if matches!(
        kind.to_ascii_lowercase().as_str(),
        "int" | "short" | "char" | "long" | "float" | "double" | "real"
    ) {
        text.parse::<f64>().is_ok_and(|value| value != 0.0)
    } else {
        false
    }
}

fn field_i32(parent: roxmltree::Node<'_, '_>, name: &str, default: i32) -> i32 {
    field_i32_optional(parent, name).unwrap_or(default)
}

fn field_i32_optional(parent: roxmltree::Node<'_, '_>, name: &str) -> Option<i32> {
    let node = typed_child(parent, name)?;
    let text = node.text().unwrap_or_default().trim();
    let kind = attribute(node, "type").unwrap_or_default();
    let value = if kind.eq_ignore_ascii_case("bool") {
        i32::from(
            text.eq_ignore_ascii_case("true") || text.eq_ignore_ascii_case("t") || text == "1",
        )
    } else if matches!(
        kind.to_ascii_lowercase().as_str(),
        "float" | "double" | "real"
    ) {
        text.parse::<f64>().ok().map_or(0, |value| value as i32)
    } else if kind.eq_ignore_ascii_case("string") {
        if text.eq_ignore_ascii_case("true") || text.eq_ignore_ascii_case("t") {
            1
        } else {
            text.parse::<i32>().unwrap_or(0)
        }
    } else if matches!(
        kind.to_ascii_lowercase().as_str(),
        "int" | "short" | "char" | "long"
    ) {
        text.parse::<i32>().unwrap_or(0)
    } else {
        0
    };
    Some(value)
}

fn field_f64(parent: roxmltree::Node<'_, '_>, name: &str, default: f64) -> f64 {
    let Some(node) = typed_child(parent, name) else {
        return default;
    };
    let text = node.text().unwrap_or_default().trim();
    let kind = attribute(node, "type").unwrap_or_default();
    let value = if kind.eq_ignore_ascii_case("bool") {
        f64::from(
            text.eq_ignore_ascii_case("true") || text.eq_ignore_ascii_case("t") || text == "1",
        )
    } else if matches!(
        kind.to_ascii_lowercase().as_str(),
        "int" | "short" | "char" | "long" | "float" | "double" | "real"
    ) || kind.eq_ignore_ascii_case("string")
    {
        text.parse::<f64>().unwrap_or(0.0)
    } else {
        0.0
    };
    if value.is_finite() {
        value
    } else {
        0.0
    }
}

fn field_uuid(parent: roxmltree::Node<'_, '_>, name: &str) -> Option<Uuid> {
    let node = typed_child(parent, name)?;
    let kind = attribute(node, "type").unwrap_or_default();
    if !(kind.eq_ignore_ascii_case("uuid") || kind.eq_ignore_ascii_case("string")) {
        return None;
    }
    parse_uuid(node.text().unwrap_or_default().trim()).filter(|uuid| !uuid.is_nil())
}

fn field_cap_type(parent: roxmltree::Node<'_, '_>, name: &str) -> String {
    let Some(node) = typed_child(parent, name) else {
        return "none".into();
    };
    let kind = attribute(node, "type").unwrap_or_default();
    if !kind.eq_ignore_ascii_case("string") {
        return "none".into();
    }
    match node.text().unwrap_or_default().trim() {
        "flat" => "flat".into(),
        "box" => "box".into(),
        "dome" => "dome".into(),
        _ => "none".into(),
    }
}

fn parse_uuid(value: &str) -> Option<Uuid> {
    let value = value.trim_matches(|character| character == '{' || character == '}');
    let mut bytes = [0_u8; 16];
    let mut index = 0;
    let mut digits = value.chars().filter(|character| *character != '-');
    while index < bytes.len() {
        let high = digits.next()?.to_digit(16)?;
        let low = digits.next()?.to_digit(16)?;
        bytes[index] = ((high << 4) | low) as u8;
        index += 1;
    }
    digits.next().is_none().then(|| Uuid::from_canonical(bytes))
}

fn same_name(node: roxmltree::Node<'_, '_>, name: &str) -> bool {
    node.tag_name().name().eq_ignore_ascii_case(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor(payload: &[u8], application_uuid: Option<Uuid>) -> AttributeUserdataDescriptor {
        descriptor_with_ids(
            0..payload.len(),
            DISPLACEMENT_CLASS,
            DISPLACEMENT_ITEM,
            application_uuid,
        )
    }

    fn edge_descriptor(
        payload: &[u8],
        application_uuid: Option<Uuid>,
    ) -> AttributeUserdataDescriptor {
        descriptor_with_ids(
            0..payload.len(),
            EDGE_SOFTENING_CLASS,
            EDGE_SOFTENING_ITEM,
            application_uuid,
        )
    }

    fn thickening_descriptor(
        payload: &[u8],
        application_uuid: Option<Uuid>,
    ) -> AttributeUserdataDescriptor {
        descriptor_with_ids(
            0..payload.len(),
            THICKENING_CLASS,
            THICKENING_ITEM,
            application_uuid,
        )
    }

    fn curve_piping_descriptor(
        payload: &[u8],
        application_uuid: Option<Uuid>,
    ) -> AttributeUserdataDescriptor {
        descriptor_with_ids(
            0..payload.len(),
            CURVE_PIPING_CLASS,
            CURVE_PIPING_ITEM,
            application_uuid,
        )
    }

    fn descriptor_with_ids(
        range: std::ops::Range<usize>,
        class_uuid: Uuid,
        item_uuid: Uuid,
        application_uuid: Option<Uuid>,
    ) -> AttributeUserdataDescriptor {
        AttributeUserdataDescriptor {
            range: range.clone(),
            known: true,
            class_uuid: Some(class_uuid),
            item_uuid: Some(item_uuid),
            application_uuid,
            writer_version: Some(2_348_836_140),
            payload_range: Some(range),
        }
    }

    fn v2_payload(xml: &str) -> Vec<u8> {
        let mut payload = XML_USERDATA_VERSION.to_le_bytes().to_vec();
        payload.extend((xml.len() as i32).to_le_bytes());
        payload.extend(xml.as_bytes());
        payload.extend([0xde, 0xad]);
        payload
    }

    fn v1_payload(xml: &str) -> Vec<u8> {
        let mut units = xml.encode_utf16().collect::<Vec<_>>();
        units.push(0);
        let mut payload = 1_i32.to_le_bytes().to_vec();
        payload.extend((units.len() as u32).to_le_bytes());
        for unit in units {
            payload.extend(unit.to_le_bytes());
        }
        payload
    }

    const XML: &str = "<xml><new-displacement-object-data>\
<on type=\"bool\">true</on>\
<channel type=\"int\">7</channel>\
<black-point type=\"double\">-0.25</black-point>\
<white-point type=\"double\">0.85</white-point>\
<sweep-pitch type=\"int\">12</sweep-pitch>\
<refine-steps type=\"int\">3</refine-steps>\
<refine-sensitivity type=\"double\">0.75</refine-sensitivity>\
<face-count-limit-enabled type=\"bool\">true</face-count-limit-enabled>\
<face-count-limit type=\"int\">5432</face-count-limit>\
<post-weld-angle type=\"double\">22.5</post-weld-angle>\
<mesh-memory-limit type=\"int\">1024</mesh-memory-limit>\
<fairing-enabled type=\"bool\">true</fairing-enabled>\
<fairing-amount type=\"int\">6</fairing-amount>\
<sub-object-count type=\"int\">1</sub-object-count>\
<sweep-res-formula type=\"int\">1</sweep-res-formula>\
<sub><sub-index type=\"int\">3</sub-index><sub-on type=\"bool\">true</sub-on>\
<sub-channel type=\"int\">9</sub-channel><sub-black-point type=\"double\">-0.1</sub-black-point>\
<sub-white-point type=\"double\">0.6</sub-white-point></sub>\
</new-displacement-object-data></xml>";

    const EDGE_SOFTENING_XML: &str = "<xml><edge-softening-object-data>\
<on type=\"bool\">true</on>\
<softening type=\"double\">0.25</softening>\
<chamfer type=\"bool\">true</chamfer>\
<unweld type=\"bool\">false</unweld>\
<force-softening type=\"bool\">true</force-softening>\
<edge-threshold type=\"double\">17.5</edge-threshold>\
</edge-softening-object-data></xml>";

    const THICKENING_XML: &str = "<xml><thickening-object-data>\
<on type=\"bool\">true</on>\
<solid type=\"bool\">false</solid>\
<both-sides type=\"bool\">true</both-sides>\
<offset-only type=\"bool\">true</offset-only>\
<distance type=\"double\">0.25</distance>\
</thickening-object-data></xml>";

    const CURVE_PIPING_XML: &str = "<xml><curve-piping-object-data>\
<on type=\"bool\">true</on>\
<radius type=\"double\">2.25</radius>\
<segments type=\"int\">12</segments>\
<weld type=\"bool\">true</weld>\
<accuracy type=\"int\">73</accuracy>\
<cap-type type=\"string\">flat</cap-type>\
</curve-piping-object-data></xml>";

    #[test]
    fn parses_v2_displacement_fields_and_sub_item() {
        let payload = v2_payload(XML);
        let mut warnings = Vec::new();
        let modifiers = parse_attribute_userdata(
            &payload,
            &[descriptor(&payload, Some(MESH_MODIFIER_PLUGIN))],
            ArchiveVersion::V6,
            &mut warnings,
        )
        .expect("displacement userdata");
        let displacement = modifiers.displacement.expect("displacement");
        assert_eq!(displacement.xml_version, 2);
        assert!(displacement.on);
        assert_eq!(displacement.channel, 7);
        assert_eq!(displacement.black_point, -0.25);
        assert_eq!(displacement.white_point, 0.85);
        assert_eq!(displacement.sweep_pitch, 12);
        assert_eq!(displacement.refine_steps, 3);
        assert_eq!(displacement.refine_sensitivity, 0.75);
        assert!(displacement.face_count_limit_enabled);
        assert_eq!(displacement.face_count_limit, 5432);
        assert_eq!(displacement.post_weld_angle, 22.5);
        assert_eq!(displacement.mesh_memory_limit, 1024);
        assert!(displacement.fairing_enabled);
        assert_eq!(displacement.fairing_amount, 6);
        assert_eq!(displacement.sub_object_count, Some(1));
        assert_eq!(displacement.sweep_resolution_formula, 1);
        assert_eq!(displacement.sub_items.len(), 1);
        assert_eq!(displacement.sub_items[0].face_index, 3);
        assert!(displacement.sub_items[0].on);
        assert_eq!(displacement.sub_items[0].channel, 9);
        assert_eq!(displacement.sub_items[0].black_point, -0.1);
        assert_eq!(displacement.sub_items[0].white_point, 0.6);
        assert!(warnings.is_empty());
    }

    #[test]
    fn parses_v2_edge_softening_fields() {
        let payload = v2_payload(EDGE_SOFTENING_XML);
        let mut warnings = Vec::new();
        let modifiers = parse_attribute_userdata(
            &payload,
            &[edge_descriptor(&payload, Some(MESH_MODIFIER_PLUGIN))],
            ArchiveVersion::V6,
            &mut warnings,
        )
        .expect("edge-softening userdata");
        let edge_softening = modifiers.edge_softening.expect("edge softening");
        assert_eq!(edge_softening.xml_version, 2);
        assert!(edge_softening.on);
        assert_eq!(edge_softening.softening, 0.25);
        assert!(edge_softening.chamfer);
        assert!(!edge_softening.faceted);
        assert!(edge_softening.force_softening);
        assert_eq!(edge_softening.edge_angle_threshold, 17.5);
        assert!(modifiers.displacement.is_none());
        assert!(warnings.is_empty());
    }

    #[test]
    fn edge_softening_defaults_and_xml_names_are_stable() {
        let xml = "<XML><EDGE-SOFTENING-OBJECT-DATA>\
<ON TYPE=\"BOOL\">true</ON>\
<softening>9</softening>\
<EDGE-THRESHOLD TYPE=\"DOUBLE\">12.5</EDGE-THRESHOLD>\
<unknown type=\"double\">99</unknown>\
</EDGE-SOFTENING-OBJECT-DATA></XML>";
        let payload = v2_payload(xml);
        let mut warnings = Vec::new();
        let modifiers = parse_attribute_userdata(
            &payload,
            &[edge_descriptor(&payload, Some(MESH_MODIFIER_PLUGIN))],
            ArchiveVersion::V6,
            &mut warnings,
        )
        .expect("edge-softening userdata");
        let edge_softening = modifiers.edge_softening.expect("edge softening");
        assert!(edge_softening.on);
        assert_eq!(edge_softening.softening, 0.1);
        assert!(!edge_softening.chamfer);
        assert!(!edge_softening.faceted);
        assert!(!edge_softening.force_softening);
        assert_eq!(edge_softening.edge_angle_threshold, 12.5);
        assert!(warnings.is_empty());
    }

    #[test]
    fn parses_v2_thickening_fields() {
        let payload = v2_payload(THICKENING_XML);
        let mut warnings = Vec::new();
        let modifiers = parse_attribute_userdata(
            &payload,
            &[thickening_descriptor(&payload, Some(MESH_MODIFIER_PLUGIN))],
            ArchiveVersion::V6,
            &mut warnings,
        )
        .expect("thickening userdata");
        let thickening = modifiers.thickening.expect("thickening");
        assert_eq!(thickening.xml_version, 2);
        assert!(thickening.on);
        assert!(!thickening.solid);
        assert!(thickening.both_sides);
        assert!(thickening.offset_only);
        assert_eq!(thickening.distance, 0.25);
        assert!(modifiers.displacement.is_none());
        assert!(modifiers.edge_softening.is_none());
        assert!(warnings.is_empty());
    }

    #[test]
    fn thickening_defaults_and_xml_names_are_stable() {
        let xml = "<XML><THICKENING-OBJECT-DATA>\
<ON TYPE=\"BOOL\">true</ON>\
<SOLID TYPE=\"BOOL\">false</SOLID>\
<BOTH-SIDES TYPE=\"BOOL\">true</BOTH-SIDES>\
<OFFSET-ONLY TYPE=\"BOOL\">true</OFFSET-ONLY>\
<distance>9</distance>\
<unknown type=\"double\">99</unknown>\
</THICKENING-OBJECT-DATA></XML>";
        let payload = v2_payload(xml);
        let mut warnings = Vec::new();
        let modifiers = parse_attribute_userdata(
            &payload,
            &[thickening_descriptor(&payload, Some(MESH_MODIFIER_PLUGIN))],
            ArchiveVersion::V6,
            &mut warnings,
        )
        .expect("thickening userdata");
        let thickening = modifiers.thickening.expect("thickening");
        assert!(thickening.on);
        assert!(!thickening.solid);
        assert!(thickening.both_sides);
        assert!(thickening.offset_only);
        assert_eq!(thickening.distance, 0.1);
        assert!(warnings.is_empty());
    }

    #[test]
    fn parses_v2_curve_piping_fields() {
        let payload = v2_payload(CURVE_PIPING_XML);
        let mut warnings = Vec::new();
        let modifiers = parse_attribute_userdata(
            &payload,
            &[curve_piping_descriptor(
                &payload,
                Some(MESH_MODIFIER_PLUGIN),
            )],
            ArchiveVersion::V6,
            &mut warnings,
        )
        .expect("curve-piping userdata");
        let curve_piping = modifiers.curve_piping.expect("curve piping");
        assert_eq!(curve_piping.xml_version, 2);
        assert!(curve_piping.on);
        assert_eq!(curve_piping.radius, 2.25);
        assert_eq!(curve_piping.segments, 12);
        assert!(!curve_piping.faceted);
        assert_eq!(curve_piping.accuracy, 73);
        assert_eq!(curve_piping.cap_type, "flat");
        assert!(modifiers.displacement.is_none());
        assert!(modifiers.edge_softening.is_none());
        assert!(modifiers.thickening.is_none());
        assert!(warnings.is_empty());
    }

    #[test]
    fn curve_piping_defaults_and_cap_names_are_stable() {
        let xml = "<XML><CURVE-PIPING-OBJECT-DATA>\
<ON TYPE=\"BOOL\">true</ON>\
<WELD TYPE=\"BOOL\">false</WELD>\
<CAP-TYPE TYPE=\"STRING\">FLAT</CAP-TYPE>\
</CURVE-PIPING-OBJECT-DATA></XML>";
        let payload = v2_payload(xml);
        let mut warnings = Vec::new();
        let modifiers = parse_attribute_userdata(
            &payload,
            &[curve_piping_descriptor(
                &payload,
                Some(MESH_MODIFIER_PLUGIN),
            )],
            ArchiveVersion::V6,
            &mut warnings,
        )
        .expect("curve-piping userdata");
        let curve_piping = modifiers.curve_piping.expect("curve piping");
        assert!(curve_piping.on);
        assert_eq!(curve_piping.radius, 1.0);
        assert_eq!(curve_piping.segments, 16);
        assert!(curve_piping.faceted);
        assert_eq!(curve_piping.accuracy, 50);
        assert_eq!(curve_piping.cap_type, "none");
        assert!(warnings.is_empty());
    }

    #[test]
    fn parses_all_mesh_modifiers_from_one_attributes_stream() {
        let displacement_payload = v2_payload("<xml><new-displacement-object-data/></xml>");
        let edge_start = displacement_payload.len();
        let edge_payload = v2_payload(EDGE_SOFTENING_XML);
        let thickening_start = edge_start + edge_payload.len();
        let thickening_payload = v2_payload(THICKENING_XML);
        let curve_piping_start = thickening_start + thickening_payload.len();
        let curve_piping_payload = v2_payload(CURVE_PIPING_XML);
        let mut bytes = displacement_payload;
        bytes.extend(&edge_payload);
        bytes.extend(&thickening_payload);
        bytes.extend(&curve_piping_payload);
        let descriptors = [
            descriptor(&bytes[..edge_start], Some(MESH_MODIFIER_PLUGIN)),
            descriptor_with_ids(
                edge_start..thickening_start,
                EDGE_SOFTENING_CLASS,
                EDGE_SOFTENING_ITEM,
                Some(MESH_MODIFIER_PLUGIN),
            ),
            descriptor_with_ids(
                thickening_start..curve_piping_start,
                THICKENING_CLASS,
                THICKENING_ITEM,
                Some(MESH_MODIFIER_PLUGIN),
            ),
            descriptor_with_ids(
                curve_piping_start..bytes.len(),
                CURVE_PIPING_CLASS,
                CURVE_PIPING_ITEM,
                Some(MESH_MODIFIER_PLUGIN),
            ),
        ];
        let mut warnings = Vec::new();
        let modifiers =
            parse_attribute_userdata(&bytes, &descriptors, ArchiveVersion::V6, &mut warnings)
                .expect("mesh modifiers");
        assert!(modifiers.displacement.is_some());
        assert!(modifiers.edge_softening.is_some());
        assert!(modifiers.thickening.is_some());
        assert!(modifiers.curve_piping.is_some());
        assert!(warnings.is_empty());
    }

    #[test]
    fn v1_xml_and_old_archive_formula_compatibility_are_admitted() {
        let xml = "<xml><new-displacement-object-data/></xml>";
        let payload = v1_payload(xml);
        for (archive, expected_formula) in [
            (ArchiveVersion::V4, 1),
            (ArchiveVersion::V5, 1),
            (ArchiveVersion::V6, 0),
        ] {
            let mut warnings = Vec::new();
            let modifiers = parse_attribute_userdata(
                &payload,
                &[descriptor(&payload, Some(MESH_MODIFIER_PLUGIN))],
                archive,
                &mut warnings,
            )
            .expect("v1 displacement userdata");
            let displacement = modifiers.displacement.expect("displacement");
            assert_eq!(displacement.xml_version, 1);
            assert_eq!(displacement.sweep_resolution_formula, expected_formula);
            assert!(!displacement.on);
            assert_eq!(displacement.channel, 0);
            assert_eq!(displacement.sweep_pitch, 1000);
            assert!(warnings.is_empty());
        }
    }

    #[test]
    fn parses_texture_uuid_fields_in_parameter_nodes() {
        let document = roxmltree::Document::parse(
            "<root><texture type=\"uuid\">12345678-1234-5678-90ab-cdef01234567</texture></root>",
        )
        .expect("texture XML");
        assert_eq!(
            field_uuid(document.root_element(), "texture"),
            Some(Uuid::from_canonical([
                0x12, 0x34, 0x56, 0x78, 0x12, 0x34, 0x56, 0x78, 0x90, 0xab, 0xcd, 0xef, 0x01, 0x23,
                0x45, 0x67,
            ]))
        );
    }

    #[test]
    fn malformed_xml_is_dropped_and_wrong_application_is_ignored() {
        let malformed = v2_payload("<xml><new-displacement-object-data>");
        let mut warnings = Vec::new();
        assert!(parse_attribute_userdata(
            &malformed,
            &[descriptor(&malformed, Some(MESH_MODIFIER_PLUGIN))],
            ArchiveVersion::V6,
            &mut warnings,
        )
        .is_none());
        assert!(warnings.iter().any(
            |warning| warning.contains("displacement userdata") && warning.contains("dropped")
        ));

        let mut ignored_warnings = Vec::new();
        assert!(parse_attribute_userdata(
            &malformed,
            &[descriptor(&malformed, None)],
            ArchiveVersion::V6,
            &mut ignored_warnings,
        )
        .is_none());
        assert!(ignored_warnings.is_empty());

        let malformed_edge = v2_payload("<xml><edge-softening-object-data>");
        let mut edge_warnings = Vec::new();
        assert!(parse_attribute_userdata(
            &malformed_edge,
            &[edge_descriptor(&malformed_edge, Some(MESH_MODIFIER_PLUGIN))],
            ArchiveVersion::V6,
            &mut edge_warnings,
        )
        .is_none());
        assert!(edge_warnings.iter().any(|warning| {
            warning.contains("edge-softening userdata") && warning.contains("dropped")
        }));

        let malformed_thickening = v2_payload("<xml><thickening-object-data>");
        let mut thickening_warnings = Vec::new();
        assert!(parse_attribute_userdata(
            &malformed_thickening,
            &[thickening_descriptor(
                &malformed_thickening,
                Some(MESH_MODIFIER_PLUGIN),
            )],
            ArchiveVersion::V6,
            &mut thickening_warnings,
        )
        .is_none());
        assert!(thickening_warnings.iter().any(|warning| {
            warning.contains("thickening userdata") && warning.contains("dropped")
        }));

        let malformed_curve_piping = v2_payload("<xml><curve-piping-object-data>");
        let mut curve_piping_warnings = Vec::new();
        assert!(parse_attribute_userdata(
            &malformed_curve_piping,
            &[curve_piping_descriptor(
                &malformed_curve_piping,
                Some(MESH_MODIFIER_PLUGIN),
            )],
            ArchiveVersion::V6,
            &mut curve_piping_warnings,
        )
        .is_none());
        assert!(curve_piping_warnings.iter().any(|warning| {
            warning.contains("curve-piping userdata") && warning.contains("dropped")
        }));
    }
}
