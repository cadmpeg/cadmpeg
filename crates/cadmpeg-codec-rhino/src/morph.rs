// SPDX-License-Identifier: Apache-2.0
//! Morph-control payload decoding.

use std::ops::Range;

use cadmpeg_ir::geometry::{NurbsCurve, NurbsSurface};

use crate::cage::Cage;
use crate::chunks::{checked_count_bytes, chunk_at, ArchiveVersion, BoundedReader, FramingError};
use crate::curves::GeometryError;
use crate::mesh::MeshExpand;
use crate::settings::{interval, point, vector, xform};
use crate::wire::{scaled_coordinate, Uuid};

const ANONYMOUS: u32 = 0x4000_8000;
const MAX_LOCALIZERS: usize = 1 << 16;
const MAX_CAPTIVES: usize = 1 << 20;
pub(crate) const CLASS: Uuid = Uuid::from_canonical([
    0xd3, 0x79, 0xe6, 0xd8, 0x7c, 0x31, 0x44, 0x07, 0xa9, 0x13, 0xe3, 0xb7, 0x04, 0x0d, 0x03, 0x4a,
]);

#[derive(Debug, Clone)]
pub(crate) enum Control {
    Curve {
        start: NurbsCurve,
        end: NurbsCurve,
    },
    Surface {
        start: NurbsSurface,
        end: NurbsSurface,
    },
    Cage {
        start_transform: [f64; 16],
        end: Cage,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct Localizer {
    pub(crate) kind: i32,
    pub(crate) point: [f64; 3],
    pub(crate) vector: [f64; 3],
    pub(crate) interval: [f64; 2],
    pub(crate) curve: Option<NurbsCurve>,
    pub(crate) surface: Option<NurbsSurface>,
}

#[derive(Debug, Clone)]
pub(crate) struct Morph {
    pub(crate) source_range: Range<usize>,
    pub(crate) control: Control,
    pub(crate) captive_ids: Vec<Uuid>,
    pub(crate) localizers: Vec<Localizer>,
    pub(crate) tolerance: f64,
    pub(crate) quick_preview: bool,
    pub(crate) preserve_structure: bool,
}

fn malformed(offset: usize, message: impl Into<String>) -> GeometryError {
    GeometryError::Malformed(FramingError::Structural {
        offset,
        message: message.into(),
    })
}

fn anonymous<'a>(
    data: &'a [u8],
    offset: usize,
    end: usize,
    archive: ArchiveVersion,
    family: &str,
) -> Result<(BoundedReader<'a>, usize, i32, i32), GeometryError> {
    let chunk = chunk_at(data, offset, end, archive, false)?;
    if chunk.typecode != ANONYMOUS || chunk.short {
        return Err(malformed(offset, format!("{family} is not anonymous")));
    }
    let mut reader = BoundedReader::new(data, chunk.body.start, chunk.body.end)?;
    let major = reader.i32()?;
    let minor = reader.i32()?;
    Ok((reader, chunk.next_offset, major, minor))
}

fn count(
    reader: &mut BoundedReader<'_>,
    element_size: usize,
    cap: usize,
) -> Result<usize, GeometryError> {
    let offset = reader.position();
    let value = reader.i32()?;
    checked_count_bytes(value, element_size, reader.remaining(), cap, offset)?;
    usize::try_from(value).map_err(|_| malformed(offset, "morph-control count overflows"))
}

fn uuid(reader: &mut BoundedReader<'_>) -> Result<Uuid, GeometryError> {
    Ok(Uuid::from_wire(reader.array()?))
}

fn captive_ids(
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
) -> Result<Vec<Uuid>, GeometryError> {
    let (mut ids, next, major, minor) = anonymous(
        data,
        reader.position(),
        reader.end(),
        archive,
        "captive UUID list",
    )?;
    if major != 1 || minor != 0 {
        return Err(GeometryError::UnsupportedVersion {
            offset: ids.position() - 8,
            message: format!("unsupported captive UUID-list version {major}.{minor}"),
        });
    }
    let count = count(&mut ids, 16, MAX_CAPTIVES)?;
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        values.push(uuid(&mut ids)?);
    }
    if ids.remaining() != 0 {
        return Err(malformed(
            ids.position(),
            "captive UUID list has trailing bytes",
        ));
    }
    reader.skip(next - reader.position())?;
    Ok(values)
}

fn scale_point(
    value: crate::settings::Point3,
    scale: f64,
    offset: usize,
) -> Result<[f64; 3], GeometryError> {
    let mut result = [0.0; 3];
    for (target, coordinate) in result.iter_mut().zip(value.0) {
        *target = scaled_coordinate(coordinate, scale)
            .ok_or_else(|| malformed(offset, "scaled morph-control coordinate is invalid"))?;
    }
    Ok(result)
}

fn scale_interval(value: [f64; 2], scale: f64, offset: usize) -> Result<[f64; 2], GeometryError> {
    Ok([
        scaled_coordinate(value[0], scale)
            .ok_or_else(|| malformed(offset, "scaled localizer interval is invalid"))?,
        scaled_coordinate(value[1], scale)
            .ok_or_else(|| malformed(offset + 8, "scaled localizer interval is invalid"))?,
    ])
}

fn optional_curve(
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    scale: f64,
    archive: ArchiveVersion,
) -> Result<Option<NurbsCurve>, GeometryError> {
    let (mut child, next, major, minor) = anonymous(
        data,
        reader.position(),
        reader.end(),
        archive,
        "localizer curve",
    )?;
    if major != 1 || minor != 0 {
        return Err(GeometryError::UnsupportedVersion {
            offset: child.position() - 8,
            message: format!("unsupported localizer-curve version {major}.{minor}"),
        });
    }
    let value = child
        .bool()?
        .then(|| crate::surfaces::read_nurbs_curve(&mut child, scale))
        .transpose()?;
    if child.remaining() != 0 {
        return Err(malformed(
            child.position(),
            "localizer curve has trailing bytes",
        ));
    }
    reader.skip(next - reader.position())?;
    Ok(value)
}

fn optional_surface(
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    scale: f64,
    archive: ArchiveVersion,
) -> Result<Option<NurbsSurface>, GeometryError> {
    let (mut child, next, major, minor) = anonymous(
        data,
        reader.position(),
        reader.end(),
        archive,
        "localizer surface",
    )?;
    if major != 1 || minor != 0 {
        return Err(GeometryError::UnsupportedVersion {
            offset: child.position() - 8,
            message: format!("unsupported localizer-surface version {major}.{minor}"),
        });
    }
    let value = child
        .bool()?
        .then(|| crate::surfaces::read_nurbs_surface(&mut child, scale))
        .transpose()?;
    if child.remaining() != 0 {
        return Err(malformed(
            child.position(),
            "localizer surface has trailing bytes",
        ));
    }
    reader.skip(next - reader.position())?;
    Ok(value)
}

fn localizer(
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    scale: f64,
    archive: ArchiveVersion,
) -> Result<Localizer, GeometryError> {
    let (mut value, next, major, minor) =
        anonymous(data, reader.position(), reader.end(), archive, "localizer")?;
    if major != 1 || minor != 0 {
        return Err(GeometryError::UnsupportedVersion {
            offset: value.position() - 8,
            message: format!("unsupported localizer version {major}.{minor}"),
        });
    }
    let kind = value.i32()?;
    if !(0..=6).contains(&kind) {
        return Err(malformed(value.position() - 4, "invalid localizer type"));
    }
    let offset = value.position();
    let point = scale_point(point(&mut value)?, scale, offset)?;
    let vector = vector(&mut value)?.0;
    let offset = value.position();
    let interval = scale_interval(interval(&mut value)?.0, scale, offset)?;
    let curve = optional_curve(data, &mut value, scale, archive)?;
    let surface = optional_surface(data, &mut value, scale, archive)?;
    if value.remaining() != 0 {
        return Err(malformed(value.position(), "localizer has trailing bytes"));
    }
    reader.skip(next - reader.position())?;
    Ok(Localizer {
        kind,
        point,
        vector,
        interval,
        curve,
        surface,
    })
}

fn control_child<'a>(
    data: &'a [u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    label: &str,
) -> Result<(BoundedReader<'a>, usize), GeometryError> {
    let (child, next, major, minor) =
        anonymous(data, reader.position(), reader.end(), archive, label)?;
    if major != 1 || minor != 0 {
        return Err(GeometryError::UnsupportedVersion {
            offset: child.position() - 8,
            message: format!("unsupported {label} version {major}.{minor}"),
        });
    }
    Ok((child, next))
}

fn cage_at(
    expand: MeshExpand<'_>,
    reader: &mut BoundedReader<'_>,
    scale: f64,
    archive: ArchiveVersion,
) -> Result<Cage, GeometryError> {
    let (cage, next) =
        crate::cage::decode_at(expand, reader.position(), reader.end(), scale, archive)?;
    reader.skip(next - reader.position())?;
    Ok(cage)
}

pub(crate) fn decode(
    expand: MeshExpand<'_>,
    range: Range<usize>,
    scale: f64,
    archive: ArchiveVersion,
) -> Result<Morph, GeometryError> {
    let data = expand.data();
    let (mut outer, next, major, minor) =
        anonymous(data, range.start, range.end, archive, "morph control")?;
    if next != range.end {
        return Err(malformed(range.start, "morph-control framing is invalid"));
    }
    if !matches!(major, 1 | 2)
        || (major == 1 && minor != 0)
        || (major == 2 && !(0..=1).contains(&minor))
    {
        return Err(GeometryError::UnsupportedVersion {
            offset: range.start,
            message: format!("unsupported morph-control version {major}.{minor}"),
        });
    }
    if major == 1 {
        let end = cage_at(expand, &mut outer, scale, archive)?;
        let captive_ids = captive_ids(data, &mut outer, archive)?;
        let mut start_transform = xform(&mut outer)?.0;
        for index in [3, 7, 11] {
            start_transform[index] =
                scaled_coordinate(start_transform[index], scale).ok_or_else(|| {
                    malformed(outer.position() - 128, "scaled cage transform is invalid")
                })?;
        }
        if outer.remaining() != 0 {
            return Err(malformed(
                outer.position(),
                "legacy morph control has trailing bytes",
            ));
        }
        return Ok(Morph {
            source_range: range,
            control: Control::Cage {
                start_transform,
                end,
            },
            captive_ids,
            localizers: Vec::new(),
            tolerance: 0.0,
            quick_preview: false,
            preserve_structure: false,
        });
    }

    let variant = outer.i32()?;
    if !(1..=3).contains(&variant) {
        return Err(malformed(
            outer.position() - 4,
            "invalid morph-control variant",
        ));
    }
    let (mut start, start_next) = control_child(data, &mut outer, archive, "morph start control")?;
    let start_curve = (variant == 1)
        .then(|| crate::surfaces::read_nurbs_curve(&mut start, scale))
        .transpose()?;
    let start_surface = (variant == 2)
        .then(|| crate::surfaces::read_nurbs_surface(&mut start, scale))
        .transpose()?;
    let mut start_transform = if variant == 3 {
        Some(xform(&mut start)?.0)
    } else {
        None
    };
    if let Some(transform) = &mut start_transform {
        for index in [3, 7, 11] {
            transform[index] = scaled_coordinate(transform[index], scale).ok_or_else(|| {
                malformed(start.position() - 128, "scaled cage transform is invalid")
            })?;
        }
    }
    if start.remaining() != 0 {
        return Err(malformed(
            start.position(),
            "morph start control has trailing bytes",
        ));
    }
    outer.skip(start_next - outer.position())?;

    let (mut end, end_next) = control_child(data, &mut outer, archive, "morph end control")?;
    let control = match variant {
        1 => Control::Curve {
            start: start_curve.expect("curve variant has start curve"),
            end: crate::surfaces::read_nurbs_curve(&mut end, scale)?,
        },
        2 => Control::Surface {
            start: start_surface.expect("surface variant has start surface"),
            end: crate::surfaces::read_nurbs_surface(&mut end, scale)?,
        },
        3 => Control::Cage {
            start_transform: start_transform.expect("cage variant has transform"),
            end: cage_at(expand, &mut end, scale, archive)?,
        },
        _ => unreachable!("validated morph variant"),
    };
    if end.remaining() != 0 {
        return Err(malformed(
            end.position(),
            "morph end control has trailing bytes",
        ));
    }
    outer.skip(end_next - outer.position())?;
    let captive_ids = captive_ids(data, &mut outer, archive)?;

    let (mut list, list_next, list_major, list_minor) = anonymous(
        data,
        outer.position(),
        outer.end(),
        archive,
        "morph localizers",
    )?;
    if list_major != 1 || list_minor != 0 {
        return Err(GeometryError::UnsupportedVersion {
            offset: list.position() - 8,
            message: format!("unsupported morph-localizer-list version {list_major}.{list_minor}"),
        });
    }
    let localizer_count = count(&mut list, 12, MAX_LOCALIZERS)?;
    let mut localizers = Vec::new();
    for _ in 0..localizer_count {
        localizers.push(localizer(data, &mut list, scale, archive)?);
    }
    if list.remaining() != 0 {
        return Err(malformed(
            list.position(),
            "morph localizer list has trailing bytes",
        ));
    }
    outer.skip(list_next - outer.position())?;
    let (tolerance, quick_preview, preserve_structure) = if minor >= 1 {
        let tolerance = scaled_coordinate(outer.f64()?, scale)
            .filter(|value| *value >= 0.0)
            .ok_or_else(|| malformed(outer.position() - 8, "invalid morph tolerance"))?;
        (tolerance, outer.bool()?, outer.bool()?)
    } else {
        (0.0, false, false)
    };
    if outer.remaining() != 0 {
        return Err(malformed(
            outer.position(),
            "morph control has trailing bytes",
        ));
    }
    Ok(Morph {
        source_range: range,
        control,
        captive_ids,
        localizers,
        tolerance,
        quick_preview,
        preserve_structure,
    })
}

fn numbers(values: impl IntoIterator<Item = f64>) -> String {
    values
        .into_iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

fn points(values: &[cadmpeg_ir::math::Point3]) -> String {
    values
        .iter()
        .map(|point| format!("{},{},{}", point.x, point.y, point.z))
        .collect::<Vec<_>>()
        .join(";")
}

fn curve_properties(
    prefix: &str,
    curve: &NurbsCurve,
    properties: &mut std::collections::BTreeMap<String, String>,
) {
    properties.insert(format!("{prefix}_degree"), curve.degree.to_string());
    properties.insert(
        format!("{prefix}_knots"),
        numbers(curve.knots.iter().copied()),
    );
    properties.insert(
        format!("{prefix}_control_points"),
        points(&curve.control_points),
    );
    properties.insert(format!("{prefix}_periodic"), curve.periodic.to_string());
    if let Some(weights) = &curve.weights {
        properties.insert(
            format!("{prefix}_weights"),
            numbers(weights.iter().copied()),
        );
    }
}

fn surface_properties(
    prefix: &str,
    surface: &NurbsSurface,
    properties: &mut std::collections::BTreeMap<String, String>,
) {
    properties.insert(format!("{prefix}_u_degree"), surface.u_degree.to_string());
    properties.insert(format!("{prefix}_v_degree"), surface.v_degree.to_string());
    properties.insert(
        format!("{prefix}_u_knots"),
        numbers(surface.u_knots.iter().copied()),
    );
    properties.insert(
        format!("{prefix}_v_knots"),
        numbers(surface.v_knots.iter().copied()),
    );
    properties.insert(format!("{prefix}_u_count"), surface.u_count.to_string());
    properties.insert(format!("{prefix}_v_count"), surface.v_count.to_string());
    properties.insert(
        format!("{prefix}_control_points"),
        points(&surface.control_points),
    );
    properties.insert(
        format!("{prefix}_u_periodic"),
        surface.u_periodic.to_string(),
    );
    properties.insert(
        format!("{prefix}_v_periodic"),
        surface.v_periodic.to_string(),
    );
    if let Some(weights) = &surface.weights {
        properties.insert(
            format!("{prefix}_weights"),
            numbers(weights.iter().copied()),
        );
    }
}

fn cage_properties(
    prefix: &str,
    cage: &Cage,
    properties: &mut std::collections::BTreeMap<String, String>,
) {
    properties.insert(format!("{prefix}_dimension"), cage.dimension.to_string());
    properties.insert(format!("{prefix}_rational"), cage.rational.to_string());
    properties.insert(
        format!("{prefix}_orders"),
        format!("{},{},{}", cage.orders[0], cage.orders[1], cage.orders[2]),
    );
    properties.insert(
        format!("{prefix}_counts"),
        format!("{},{},{}", cage.counts[0], cage.counts[1], cage.counts[2]),
    );
    for (axis, knots) in ["u", "v", "w"].into_iter().zip(&cage.knots) {
        properties.insert(
            format!("{prefix}_{axis}_knots"),
            numbers(knots.iter().copied()),
        );
    }
    properties.insert(
        format!("{prefix}_control_points"),
        cage.control_points
            .iter()
            .map(|point| numbers(point.iter().copied()))
            .collect::<Vec<_>>()
            .join(";"),
    );
    if let Some(weights) = &cage.weights {
        properties.insert(
            format!("{prefix}_weights"),
            numbers(weights.iter().copied()),
        );
    }
}

pub(crate) fn project(
    morph: &Morph,
    key: &str,
    name: Option<String>,
    native_ref: String,
) -> cadmpeg_ir::features::Feature {
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId};
    use std::collections::BTreeMap;

    let (variant, mut properties) = match &morph.control {
        Control::Curve { start, end } => {
            let mut properties = BTreeMap::new();
            curve_properties("start", start, &mut properties);
            curve_properties("end", end, &mut properties);
            ("curve", properties)
        }
        Control::Surface { start, end } => {
            let mut properties = BTreeMap::new();
            surface_properties("start", start, &mut properties);
            surface_properties("end", end, &mut properties);
            ("surface", properties)
        }
        Control::Cage {
            start_transform,
            end,
        } => {
            let mut properties = BTreeMap::from([(
                "start_transform".to_string(),
                numbers(start_transform.iter().copied()),
            )]);
            cage_properties("end", end, &mut properties);
            ("cage", properties)
        }
    };
    for (index, localizer) in morph.localizers.iter().enumerate() {
        let prefix = format!("localizer_{index}");
        properties.insert(format!("{prefix}_type"), localizer.kind.to_string());
        properties.insert(format!("{prefix}_point"), numbers(localizer.point));
        properties.insert(format!("{prefix}_vector"), numbers(localizer.vector));
        properties.insert(format!("{prefix}_interval"), numbers(localizer.interval));
        if let Some(curve) = &localizer.curve {
            curve_properties(&format!("{prefix}_curve"), curve, &mut properties);
        }
        if let Some(surface) = &localizer.surface {
            surface_properties(&format!("{prefix}_surface"), surface, &mut properties);
        }
    }
    Feature {
        id: FeatureId(format!("rhino:morph:feature#{key}")),
        ordinal: u64::try_from(morph.source_range.start).expect("source offset fits u64"),
        name,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: Some("RhinoMorphControl".to_string()),
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Native {
            kind: "morph_control".to_string(),
            parameters: BTreeMap::from([
                ("variant".to_string(), variant.to_string()),
                (
                    "captive_ids".to_string(),
                    morph
                        .captive_ids
                        .iter()
                        .map(Uuid::to_string)
                        .collect::<Vec<_>>()
                        .join(","),
                ),
                ("tolerance".to_string(), morph.tolerance.to_string()),
                ("quick_preview".to_string(), morph.quick_preview.to_string()),
                (
                    "preserve_structure".to_string(),
                    morph.preserve_structure.to_string(),
                ),
            ]),
            properties,
        },
        native_ref: Some(native_ref),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive_test_support::crc_chunk;

    fn anonymous(major: i32, minor: i32, suffix: &[u8]) -> Vec<u8> {
        let mut body = major.to_le_bytes().to_vec();
        body.extend(minor.to_le_bytes());
        body.extend(suffix);
        crc_chunk(ANONYMOUS, &body)
    }

    fn cage() -> Vec<u8> {
        let mut body = 3_i32.to_le_bytes().to_vec();
        body.extend(0_i32.to_le_bytes());
        for _ in 0..6 {
            body.extend(2_i32.to_le_bytes());
        }
        for _ in 0..3 {
            body.extend(0.0_f64.to_le_bytes());
            body.extend(1.0_f64.to_le_bytes());
        }
        for index in 0..8 {
            for coordinate in [index as f64, 0.0, 0.0] {
                body.extend(coordinate.to_le_bytes());
            }
        }
        anonymous(1, 0, &body)
    }

    fn curve(end: f64) -> Vec<u8> {
        let mut bytes = vec![0x11];
        for value in [3_i32, 0, 2, 2, 0, 0] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend([0; 48]);
        bytes.extend(2_i32.to_le_bytes());
        bytes.extend(0.0_f64.to_le_bytes());
        bytes.extend(1.0_f64.to_le_bytes());
        bytes.extend(2_i32.to_le_bytes());
        for value in [0.0_f64, 0.0, 0.0, end, 0.0, 0.0] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.push(0);
        bytes
    }

    #[test]
    fn decodes_cage_morph_captives_options_and_unit_scaling() {
        let mut transform = Vec::new();
        for value in [
            1.0_f64, 0.0, 0.0, 2.0, 0.0, 1.0, 0.0, 3.0, 0.0, 0.0, 1.0, 4.0, 0.0, 0.0, 0.0, 1.0,
        ] {
            transform.extend(value.to_le_bytes());
        }
        let start = anonymous(1, 0, &transform);
        let end = anonymous(1, 0, &cage());
        let mut captives = 1_i32.to_le_bytes().to_vec();
        captives.extend([0; 16]);
        let captives = anonymous(1, 0, &captives);
        let localizers = anonymous(1, 0, &0_i32.to_le_bytes());
        let mut content = 3_i32.to_le_bytes().to_vec();
        content.extend(start);
        content.extend(end);
        content.extend(captives);
        content.extend(localizers);
        content.extend(0.01_f64.to_le_bytes());
        content.extend([1, 0]);
        let bytes = anonymous(2, 1, &content);

        let morph = crate::decode::with_expand_bytes(&bytes, |expand| {
            decode(expand, 0..bytes.len(), 10.0, ArchiveVersion::V8)
        })
        .expect("required invariant");
        assert_eq!(morph.captive_ids.len(), 1);
        assert_eq!(morph.tolerance, 0.1);
        assert!(morph.quick_preview);
        assert!(!morph.preserve_structure);
        let Control::Cage {
            start_transform,
            end,
        } = &morph.control
        else {
            panic!("expected cage morph");
        };
        assert_eq!(start_transform[3], 20.0);
        assert_eq!(start_transform[7], 30.0);
        assert_eq!(start_transform[11], 40.0);
        assert_eq!(end.control_points[7][0], 70.0);
        let feature = project(&morph, "test", None, "native".to_string());
        assert_eq!(feature.source_tag.as_deref(), Some("RhinoMorphControl"));
    }

    #[test]
    fn decodes_curve_morph_and_distance_localizer() {
        let start = anonymous(1, 0, &curve(1.0));
        let end = anonymous(1, 0, &curve(2.0));
        let captives = anonymous(1, 0, &0_i32.to_le_bytes());
        let mut localizer = 6_i32.to_le_bytes().to_vec();
        for value in [1.0_f64, 2.0, 3.0, 0.0, 0.0, 1.0, 4.0, 5.0] {
            localizer.extend(value.to_le_bytes());
        }
        localizer.extend(anonymous(1, 0, &[0]));
        localizer.extend(anonymous(1, 0, &[0]));
        let localizer = anonymous(1, 0, &localizer);
        let mut localizers = 1_i32.to_le_bytes().to_vec();
        localizers.extend(localizer);
        let localizers = anonymous(1, 0, &localizers);
        let mut content = 1_i32.to_le_bytes().to_vec();
        content.extend(start);
        content.extend(end);
        content.extend(captives);
        content.extend(localizers);
        content.extend(0.0_f64.to_le_bytes());
        content.extend([0, 1]);
        let bytes = anonymous(2, 1, &content);

        let morph = crate::decode::with_expand_bytes(&bytes, |expand| {
            decode(expand, 0..bytes.len(), 10.0, ArchiveVersion::V8)
        })
        .expect("required invariant");
        let Control::Curve { start, end } = &morph.control else {
            panic!("expected curve morph");
        };
        assert_eq!(start.control_points[1].x, 10.0);
        assert_eq!(end.control_points[1].x, 20.0);
        assert_eq!(morph.localizers[0].point, [10.0, 20.0, 30.0]);
        assert_eq!(morph.localizers[0].vector, [0.0, 0.0, 1.0]);
        assert_eq!(morph.localizers[0].interval, [40.0, 50.0]);
        assert!(morph.preserve_structure);
    }
}
