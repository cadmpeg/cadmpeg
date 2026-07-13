// SPDX-License-Identifier: Apache-2.0
//! Rhino appearance, grouping, and lighting presentation records.

use std::collections::BTreeMap;
use std::ops::Range;

use cadmpeg_ir::document::CadIr;
use serde::Serialize;

use crate::chunks::{chunk_at, ArchiveVersion, BoundedReader, FramingError};
use crate::container::{Record, Scan};
use crate::objects::parse_class_wrapper;
use crate::settings::utf16;
use crate::wire::{scaled_coordinate, Uuid};

const ANONYMOUS: u32 = 0x4000_8000;
const MODEL_ATTRIBUTES: u32 = 0x4000_8002;
const MATERIAL_TABLE: u32 = 0x1000_0010;
const LIGHT_TABLE: u32 = 0x1000_0012;
const GROUP_TABLE: u32 = 0x1000_0018;
const MATERIAL: Uuid = Uuid::from_canonical([
    0x60, 0xb5, 0xdb, 0xbc, 0xe6, 0x60, 0x11, 0xd3, 0xbf, 0xe4, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
]);
const LIGHT: Uuid = Uuid::from_canonical([
    0x85, 0xa0, 0x85, 0x13, 0xf3, 0x83, 0x11, 0xd3, 0xbf, 0xe7, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
]);
const GROUP: Uuid = Uuid::from_canonical([
    0x72, 0x1d, 0x9f, 0x97, 0x36, 0x45, 0x44, 0xc4, 0x8b, 0xe6, 0xb2, 0xcf, 0x69, 0x7d, 0x25, 0xce,
]);

#[derive(Debug)]
struct Component {
    index: Option<i32>,
    id: Uuid,
    name: String,
}

#[derive(Debug, Serialize)]
struct GroupRecord {
    id: String,
    source_offset: u64,
    archive_index: i32,
    source_uuid: Option<String>,
    name: String,
    links: Vec<String>,
}

#[derive(Debug, Serialize)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent material render switches"
)]
struct MaterialRecord {
    id: String,
    source_offset: u64,
    archive_index: i32,
    source_uuid: Option<String>,
    name: String,
    plugin_uuid: String,
    ambient: [u8; 4],
    diffuse: [u8; 4],
    emission: [u8; 4],
    specular: [u8; 4],
    reflection: [u8; 4],
    transparent: [u8; 4],
    index_of_refraction: f64,
    reflectivity: f64,
    shine: f64,
    transparency: f64,
    texture_count: usize,
    shareable: bool,
    disable_lighting: bool,
    fresnel_reflections: bool,
    reflection_glossiness: Option<f64>,
    refraction_glossiness: Option<f64>,
    fresnel_index_of_refraction: Option<f64>,
    rdk_instance_uuid: Option<String>,
    diffuse_texture_alpha_transparency: Option<bool>,
}

#[derive(Debug, Serialize)]
struct LightRecord {
    id: String,
    source_offset: u64,
    source_uuid: String,
    archive_index: i32,
    name: String,
    enabled: bool,
    style: i32,
    intensity: f64,
    watts: f64,
    ambient: [u8; 4],
    diffuse: [u8; 4],
    specular: [u8; 4],
    direction: [f64; 3],
    location: [f64; 3],
    spot_angle_radians: f64,
    spot_exponent: f64,
    attenuation: [f64; 3],
    shadow_intensity: f64,
    length: [f64; 3],
    width: [f64; 3],
    hotspot: f64,
    links: Vec<String>,
}

fn structural(offset: usize, message: impl Into<String>) -> FramingError {
    FramingError::Structural {
        offset,
        message: message.into(),
    }
}

fn uuid(reader: &mut BoundedReader<'_>) -> Result<Uuid, FramingError> {
    Ok(Uuid::from_wire(reader.array()?))
}

fn finite(reader: &BoundedReader<'_>, value: f64, label: &str) -> Result<f64, FramingError> {
    value
        .is_finite()
        .then_some(value)
        .ok_or_else(|| structural(reader.position() - 8, format!("{label} is not finite")))
}

fn read_finite(reader: &mut BoundedReader<'_>, label: &str) -> Result<f64, FramingError> {
    let value = reader.f64()?;
    finite(reader, value, label)
}

fn finite3(reader: &mut BoundedReader<'_>, label: &str) -> Result<[f64; 3], FramingError> {
    let value = [reader.f64()?, reader.f64()?, reader.f64()?];
    value
        .iter()
        .all(|value| value.is_finite())
        .then_some(value)
        .ok_or_else(|| structural(reader.position() - 24, format!("{label} is not finite")))
}

fn anonymous(
    data: &[u8],
    range: Range<usize>,
    archive: ArchiveVersion,
) -> Result<(BoundedReader<'_>, (i32, i32)), FramingError> {
    let chunk = chunk_at(data, range.start, range.end, archive, false)?;
    if chunk.typecode != ANONYMOUS || chunk.short || chunk.next_offset != range.end {
        return Err(structural(range.start, "presentation wrapper is invalid"));
    }
    let mut reader = BoundedReader::new(data, chunk.body.start, chunk.body.end)?;
    let version = (reader.i32()?, reader.i32()?);
    Ok((reader, version))
}

fn component(
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
) -> Result<Component, FramingError> {
    let chunk = chunk_at(data, reader.position(), reader.end(), archive, false)?;
    if chunk.typecode != MODEL_ATTRIBUTES || chunk.short {
        return Err(structural(
            reader.position(),
            "model-component attributes are missing",
        ));
    }
    let mut value = BoundedReader::new(data, chunk.body.start, chunk.body.end)?;
    if (value.i32()?, value.i32()?) != (1, 0) {
        return Err(structural(
            value.position(),
            "model-component version is unsupported",
        ));
    }
    match value.u8()? {
        0 | 2 => {}
        1 => value.skip(12)?,
        _ => return Err(structural(value.position() - 1, "invalid serial status")),
    }
    let id = match value.u8()? {
        0 | 2 => Uuid::nil(),
        1 => uuid(&mut value)?,
        _ => return Err(structural(value.position() - 1, "invalid UUID status")),
    };
    match value.u8()? {
        0 | 2 => {}
        1 => value.skip(4)?,
        _ => return Err(structural(value.position() - 1, "invalid type status")),
    }
    let index = match value.u8()? {
        0 | 2 => None,
        1 => Some(value.i32()?),
        _ => return Err(structural(value.position() - 1, "invalid index status")),
    };
    let name = match value.u8()? {
        0 | 2 => String::new(),
        1 => utf16(&mut value)?,
        _ => return Err(structural(value.position() - 1, "invalid name status")),
    };
    if value.remaining() != 0 {
        return Err(structural(
            value.position(),
            "model-component attributes have trailing bytes",
        ));
    }
    reader.skip(chunk.next_offset - reader.position())?;
    Ok(Component { index, id, name })
}

fn class_data(
    data: &[u8],
    record: &Record,
    archive: ArchiveVersion,
    expected: Uuid,
) -> Result<Range<usize>, FramingError> {
    let class = parse_class_wrapper(data, record.body.clone(), archive, &mut Vec::new())?;
    if class.class_uuid != expected {
        return Err(structural(
            record.range.start,
            "table record has the wrong class",
        ));
    }
    Ok(class.class_data_range)
}

fn skip_objects(
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
) -> Result<usize, FramingError> {
    let chunk = chunk_at(data, reader.position(), reader.end(), archive, false)?;
    if chunk.typecode != ANONYMOUS || chunk.short {
        return Err(structural(
            reader.position(),
            "texture array is not anonymous",
        ));
    }
    let mut values = BoundedReader::new(data, chunk.body.start, chunk.body.end)?;
    if (values.i32()?, values.i32()?) != (1, 0) {
        return Err(structural(
            values.position(),
            "texture array version is unsupported",
        ));
    }
    let count = values.i32()?;
    let count = usize::try_from(count)
        .map_err(|_| structural(values.position() - 4, "negative texture count"))?;
    if count > 1 << 16 {
        return Err(structural(
            values.position() - 4,
            "texture count exceeds limit",
        ));
    }
    for _ in 0..count {
        let object = chunk_at(data, values.position(), values.end(), archive, false)?;
        if object.short {
            return Err(structural(
                values.position(),
                "texture object is short-framed",
            ));
        }
        values.skip(object.next_offset - values.position())?;
    }
    if values.remaining() != 0 {
        return Err(structural(
            values.position(),
            "texture array has trailing bytes",
        ));
    }
    reader.skip(chunk.next_offset - reader.position())?;
    Ok(count)
}

fn parse_material(
    data: &[u8],
    range: Range<usize>,
    archive: ArchiveVersion,
    source_offset: usize,
) -> Result<MaterialRecord, FramingError> {
    let modern = data.get(range.start).copied() == Some(0);
    let (mut reader, component, minor) = if modern {
        let (mut reader, version) = anonymous(data, range, archive)?;
        if version.0 != 1 || version.1 != 0 {
            return Err(structural(
                reader.position(),
                "material version is unsupported",
            ));
        }
        let component = component(data, &mut reader, archive)?;
        (reader, component, 6)
    } else {
        let mut outer = BoundedReader::new(data, range.start, range.end)?;
        // The first packed byte is the fixed outer material version 2.0.
        if outer.u8()? != 0x20 {
            return Err(structural(
                range.start,
                "legacy material outer version is unsupported",
            ));
        }
        let chunk = chunk_at(data, outer.position(), outer.end(), archive, false)?;
        let mut reader = BoundedReader::new(data, chunk.body.start, chunk.body.end)?;
        let version = (reader.i32()?, reader.i32()?);
        if version.0 != 1 || !(0..=6).contains(&version.1) {
            return Err(structural(
                reader.position(),
                "legacy material version is unsupported",
            ));
        }
        let id = uuid(&mut reader)?;
        let index = reader.i32()?;
        let name = utf16(&mut reader)?;
        (
            reader,
            Component {
                index: Some(index),
                id,
                name,
            },
            version.1,
        )
    };
    let plugin = uuid(&mut reader)?;
    let ambient = reader.array()?;
    let diffuse = reader.array()?;
    let emission = reader.array()?;
    let specular = reader.array()?;
    let reflection = reader.array()?;
    let transparent = reader.array()?;
    let index_of_refraction = read_finite(&mut reader, "index of refraction")?;
    let reflectivity = read_finite(&mut reader, "reflectivity")?;
    let shine = read_finite(&mut reader, "shine")?;
    let transparency = read_finite(&mut reader, "transparency")?;
    let texture_count = skip_objects(data, &mut reader, archive)?;
    if !modern && minor >= 1 {
        let _obsolete_library = utf16(&mut reader)?;
    }
    if minor >= 2 || modern {
        let count = reader.i32()?;
        let bytes = crate::chunks::checked_count_bytes(
            count,
            20,
            reader.remaining(),
            1 << 16,
            reader.position(),
        )?;
        reader.skip(bytes)?;
    }
    let shareable = if minor >= 3 || modern {
        reader.bool()?
    } else {
        false
    };
    let disable_lighting = if minor >= 3 || modern {
        reader.bool()?
    } else {
        false
    };
    let fresnel_reflections = if minor >= 4 || modern {
        reader.bool()?
    } else {
        false
    };
    let reflection_glossiness = if minor >= 4 || modern {
        Some(read_finite(&mut reader, "reflection glossiness")?)
    } else {
        None
    };
    let refraction_glossiness = if minor >= 4 || modern {
        Some(read_finite(&mut reader, "refraction glossiness")?)
    } else {
        None
    };
    let fresnel_index_of_refraction = if minor >= 4 || modern {
        Some(read_finite(&mut reader, "Fresnel index")?)
    } else {
        None
    };
    let rdk = if minor >= 5 || modern {
        Some(uuid(&mut reader)?)
    } else {
        None
    };
    let alpha = if minor >= 6 || modern {
        Some(reader.bool()?)
    } else {
        None
    };
    if reader.remaining() != 0 {
        return Err(structural(reader.position(), "material has trailing bytes"));
    }
    let key = if component.id.is_nil() {
        format!("record-{source_offset}")
    } else {
        component.id.to_string()
    };
    Ok(MaterialRecord {
        id: format!("rhino:presentation:material#{key}"),
        source_offset: source_offset as u64,
        archive_index: component.index.unwrap_or(-1),
        source_uuid: (!component.id.is_nil()).then(|| component.id.to_string()),
        name: component.name,
        plugin_uuid: plugin.to_string(),
        ambient,
        diffuse,
        emission,
        specular,
        reflection,
        transparent,
        index_of_refraction,
        reflectivity,
        shine,
        transparency,
        texture_count,
        shareable,
        disable_lighting,
        fresnel_reflections,
        reflection_glossiness,
        refraction_glossiness,
        fresnel_index_of_refraction,
        rdk_instance_uuid: rdk.filter(|id| !id.is_nil()).map(|id| id.to_string()),
        diffuse_texture_alpha_transparency: alpha,
    })
}

fn parse_group(
    data: &[u8],
    range: Range<usize>,
    source_offset: usize,
) -> Result<GroupRecord, FramingError> {
    let mut reader = BoundedReader::new(data, range.start, range.end)?;
    let packed = reader.u8()?;
    if packed >> 4 != 1 || packed & 0x0f > 1 {
        return Err(structural(range.start, "group version is unsupported"));
    }
    let index = reader.i32()?;
    let name = utf16(&mut reader)?;
    let id = if packed & 0x0f >= 1 {
        Some(uuid(&mut reader)?)
    } else {
        None
    };
    if reader.remaining() != 0 {
        return Err(structural(reader.position(), "group has trailing bytes"));
    }
    let key = id
        .filter(|id| !id.is_nil())
        .map_or_else(|| format!("index-{index}"), |id| id.to_string());
    Ok(GroupRecord {
        id: format!("rhino:presentation:group#{key}"),
        source_offset: source_offset as u64,
        archive_index: index,
        source_uuid: id.filter(|id| !id.is_nil()).map(|id| id.to_string()),
        name,
        links: Vec::new(),
    })
}

fn parse_light(
    data: &[u8],
    range: Range<usize>,
    scale: f64,
    source_offset: usize,
    link: Option<String>,
) -> Result<LightRecord, FramingError> {
    let mut reader = BoundedReader::new(data, range.start, range.end)?;
    let packed = reader.u8()?;
    if packed >> 4 != 1 || packed & 0x0f > 2 {
        return Err(structural(range.start, "light version is unsupported"));
    }
    let enabled = reader.i32()? != 0;
    let style = reader.i32()?;
    let intensity = read_finite(&mut reader, "light intensity")?;
    let watts = read_finite(&mut reader, "light watts")?;
    let ambient = reader.array()?;
    let diffuse = reader.array()?;
    let specular = reader.array()?;
    let direction = finite3(&mut reader, "light direction")?;
    let mut location = finite3(&mut reader, "light location")?;
    let spot_angle_radians = read_finite(&mut reader, "spot angle")?;
    let mut spot_exponent = read_finite(&mut reader, "spot exponent")?;
    let attenuation = finite3(&mut reader, "light attenuation")?;
    let shadow_intensity = read_finite(&mut reader, "shadow intensity")?;
    let index = reader.i32()?;
    let id = uuid(&mut reader)?;
    let name = utf16(&mut reader)?;
    let mut length = [0.0; 3];
    let mut width = [0.0; 3];
    if packed & 0x0f >= 1 {
        length = finite3(&mut reader, "light length")?;
        width = finite3(&mut reader, "light width")?;
    }
    let hotspot = if packed & 0x0f >= 2 {
        read_finite(&mut reader, "light hotspot")?
    } else {
        let value = (1.0 - spot_exponent / 128.0).clamp(0.0, 1.0);
        spot_exponent = 0.0;
        value
    };
    if reader.remaining() != 0 {
        return Err(structural(reader.position(), "light has trailing bytes"));
    }
    for vector in [&mut location, &mut length, &mut width] {
        for value in vector {
            *value = scaled_coordinate(*value, scale)
                .ok_or_else(|| structural(range.start, "scaled light geometry is invalid"))?;
        }
    }
    let key = if id.is_nil() {
        format!("record-{source_offset}")
    } else {
        id.to_string()
    };
    Ok(LightRecord {
        id: format!("rhino:presentation:light#{key}"),
        source_offset: source_offset as u64,
        source_uuid: id.to_string(),
        archive_index: index,
        name,
        enabled,
        style,
        intensity,
        watts,
        ambient,
        diffuse,
        specular,
        direction,
        location,
        spot_angle_radians,
        spot_exponent,
        attenuation,
        shadow_intensity,
        length,
        width,
        hotspot,
        links: link.into_iter().collect(),
    })
}

/// Installs built-in appearance, group membership, and light semantics.
pub(crate) fn install(scan: &Scan, ir: &mut CadIr) {
    let scale = scan
        .metadata
        .settings
        .units
        .as_ref()
        .and_then(|units| units.millimeters_per_unit)
        .unwrap_or(1.0);
    let mut groups = Vec::new();
    let mut materials = Vec::new();
    let mut lights = Vec::new();
    for table in &scan.tables {
        let table_type = table.typecode & !0x0000_8000;
        for record in &table.records {
            if table_type == GROUP_TABLE {
                if let Ok(range) = class_data(&scan.data, record, scan.archive, GROUP) {
                    if let Ok(group) = parse_group(&scan.data, range, record.range.start) {
                        groups.push(group);
                    }
                }
            } else if table_type == MATERIAL_TABLE {
                if let Ok(range) = class_data(&scan.data, record, scan.archive, MATERIAL) {
                    if let Ok(material) =
                        parse_material(&scan.data, range, scan.archive, record.range.start)
                    {
                        materials.push(material);
                    }
                }
            } else if table_type == LIGHT_TABLE {
                if let Ok(range) = class_data(&scan.data, record, scan.archive, LIGHT) {
                    if let Ok(light) =
                        parse_light(&scan.data, range, scale, record.range.start, None)
                    {
                        lights.push(light);
                    }
                }
            }
        }
    }
    let mut group_members = BTreeMap::<i32, Vec<String>>::new();
    for (source_order, object) in scan.objects.iter().enumerate() {
        if let Some(attributes) = &object.attributes {
            for group in &attributes.groups {
                group_members
                    .entry(*group)
                    .or_default()
                    .push(format!("rhino:object:record#{source_order:06}"));
            }
        }
        if object.class_uuid == LIGHT {
            let link = format!("rhino:object:record#{source_order:06}");
            if let Ok(light) = parse_light(
                &scan.data,
                object.class_data_range.clone(),
                scale,
                object.range.start,
                Some(link),
            ) {
                lights.push(light);
            }
        }
    }
    for group in &mut groups {
        group.links = group_members
            .remove(&group.archive_index)
            .unwrap_or_default();
        group.links.sort();
    }
    let namespace = ir.native.namespace_mut("rhino");
    namespace.version = namespace.version.max(2);
    namespace
        .set_arena("groups", &groups)
        .expect("Rhino groups serialize");
    namespace
        .set_arena("materials", &materials)
        .expect("Rhino materials serialize");
    namespace
        .set_arena("lights", &lights)
        .expect("Rhino lights serialize");
}

#[cfg(test)]
mod tests {
    use super::{parse_group, parse_light, parse_material};
    use crate::chunks::ArchiveVersion;

    fn utf16(value: &str) -> Vec<u8> {
        let mut units = value.encode_utf16().collect::<Vec<_>>();
        units.push(0);
        let mut bytes = (units.len() as u32).to_le_bytes().to_vec();
        for unit in units {
            bytes.extend(unit.to_le_bytes());
        }
        bytes
    }

    fn anonymous(minor: i32, body: &[u8]) -> Vec<u8> {
        let mut payload = 1_i32.to_le_bytes().to_vec();
        payload.extend(minor.to_le_bytes());
        payload.extend(body);
        payload.extend(crc32fast::hash(&payload).to_le_bytes());
        let mut bytes = 0x4000_8000_u32.to_le_bytes().to_vec();
        bytes.extend((payload.len() as i64).to_le_bytes());
        bytes.extend(payload);
        bytes
    }

    #[test]
    fn group_preserves_component_identity() {
        let mut bytes = vec![0x11];
        bytes.extend(7_i32.to_le_bytes());
        bytes.extend(utf16("fixtures"));
        bytes.extend([0x44; 16]);
        let group = parse_group(&bytes, 0..bytes.len(), 120).unwrap();
        assert_eq!(group.archive_index, 7);
        assert_eq!(group.name, "fixtures");
        assert_eq!(group.source_offset, 120);
    }

    #[test]
    fn light_scales_spatial_values_but_not_direction_or_angles() {
        let mut bytes = vec![0x12];
        bytes.extend(1_i32.to_le_bytes());
        bytes.extend(4_i32.to_le_bytes());
        bytes.extend(0.5_f64.to_le_bytes());
        bytes.extend(20.0_f64.to_le_bytes());
        bytes.extend([1, 2, 3, 4]);
        bytes.extend([5, 6, 7, 8]);
        bytes.extend([9, 10, 11, 12]);
        for value in [0.0_f64, 0.0, -1.0, 1.0, 2.0, 3.0] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend(0.25_f64.to_le_bytes());
        bytes.extend(16.0_f64.to_le_bytes());
        for value in [1.0_f64, 0.0, 0.0] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend(0.75_f64.to_le_bytes());
        bytes.extend(3_i32.to_le_bytes());
        bytes.extend([0x55; 16]);
        bytes.extend(utf16("key"));
        for value in [4.0_f64, 0.0, 0.0, 0.0, 5.0, 0.0] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend(0.8_f64.to_le_bytes());
        let light = parse_light(&bytes, 0..bytes.len(), 10.0, 0, None).unwrap();
        assert_eq!(light.location, [10.0, 20.0, 30.0]);
        assert_eq!(light.direction, [0.0, 0.0, -1.0]);
        assert_eq!(light.length, [40.0, 0.0, 0.0]);
        assert_eq!(light.spot_angle_radians, 0.25);
    }

    #[test]
    fn legacy_material_preserves_core_appearance_and_switches() {
        let mut body = vec![[0x11; 16].as_slice(), 2_i32.to_le_bytes().as_slice()].concat();
        body.extend(utf16("steel"));
        body.extend([0x22; 16]);
        for color in [
            [1, 2, 3, 4],
            [5, 6, 7, 8],
            [9, 10, 11, 12],
            [13, 14, 15, 16],
            [17, 18, 19, 20],
            [21, 22, 23, 24],
        ] {
            body.extend(color);
        }
        for value in [1.5_f64, 0.25, 64.0, 0.1] {
            body.extend(value.to_le_bytes());
        }
        body.extend(anonymous(0, &0_i32.to_le_bytes()));
        body.extend(utf16(""));
        body.extend(0_i32.to_le_bytes());
        body.extend([1, 0]);
        let inner = anonymous(3, &body);
        let mut bytes = vec![0x20];
        bytes.extend(inner);
        let material = parse_material(&bytes, 0..bytes.len(), ArchiveVersion::V5, 0).unwrap();
        assert_eq!(material.name, "steel");
        assert_eq!(material.diffuse, [5, 6, 7, 8]);
        assert_eq!(material.index_of_refraction, 1.5);
        assert!(material.shareable);
        assert!(!material.disable_lighting);
    }
}
