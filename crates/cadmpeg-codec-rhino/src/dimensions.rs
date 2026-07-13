// SPDX-License-Identifier: Apache-2.0
//! Modern Rhino dimension payload decoding.

use std::ops::Range;

use crate::chunks::{chunk_at, ArchiveVersion, BoundedReader, FramingError};
use crate::objects::parse_class_wrapper;
use crate::settings::{plane, utf16, Plane};
use crate::wire::{scaled_coordinate, Uuid};

const ANONYMOUS: u32 = 0x4000_8000;
pub(crate) const LINEAR: Uuid = Uuid::from_canonical([
    0xe5, 0x50, 0x88, 0x2b, 0xf4, 0x4d, 0x41, 0x54, 0xa1, 0xef, 0x6e, 0x50, 0xcb, 0xbb, 0xf5, 0x43,
]);
pub(crate) const ANGULAR: Uuid = Uuid::from_canonical([
    0xd4, 0x17, 0x78, 0x6b, 0xf6, 0xcd, 0x4f, 0x12, 0x9e, 0x1f, 0x06, 0x3f, 0x41, 0x4d, 0xbe, 0xb6,
]);
pub(crate) const RADIAL: Uuid = Uuid::from_canonical([
    0xfc, 0x74, 0x9c, 0x2f, 0x4c, 0x00, 0x41, 0xfd, 0x98, 0x40, 0x26, 0xd9, 0x4f, 0x04, 0x7a, 0xd3,
]);

/// Dimension family and defining plane-space geometry.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Definition {
    Linear {
        definition_point: [f64; 2],
        dimension_line_point: [f64; 2],
    },
    Angular {
        first_direction: [f64; 2],
        second_direction: [f64; 2],
        first_extension_offset: f64,
        second_extension_offset: f64,
        dimension_line_point: [f64; 2],
    },
    Radial {
        radius_point: [f64; 2],
        dimension_line_point: [f64; 2],
        diameter: bool,
    },
}

/// Complete common and family-specific dimension semantics.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Dimension {
    pub(crate) source_range: Range<usize>,
    pub(crate) annotation_type: i32,
    pub(crate) rich_text: String,
    pub(crate) user_text: String,
    pub(crate) dimstyle_id: Uuid,
    pub(crate) plane: Plane,
    pub(crate) horizontal_direction: [f64; 2],
    pub(crate) allow_text_scaling: bool,
    pub(crate) use_default_text_point: bool,
    pub(crate) user_text_point: [f64; 2],
    pub(crate) flip_arrows: [bool; 2],
    pub(crate) detail_measured: Uuid,
    pub(crate) distance_scale: f64,
    pub(crate) definition: Definition,
    pub(crate) measurement: f64,
}

struct Annotation {
    rich_text: String,
    dimstyle_id: Uuid,
    plane: Plane,
    kind: i32,
    horizontal_direction: [f64; 2],
    allow_text_scaling: bool,
}

pub(crate) fn supported_class(class: Uuid) -> bool {
    matches!(class, LINEAR | ANGULAR | RADIAL)
}

fn structural(offset: usize, message: impl Into<String>) -> FramingError {
    FramingError::Structural {
        offset,
        message: message.into(),
    }
}

fn anonymous(
    data: &[u8],
    offset: usize,
    end: usize,
    archive: ArchiveVersion,
) -> Result<(BoundedReader<'_>, usize, i32), FramingError> {
    let chunk = chunk_at(data, offset, end, archive, false)?;
    if chunk.typecode != ANONYMOUS || chunk.short {
        return Err(structural(offset, "expected dimension anonymous chunk"));
    }
    let mut reader = BoundedReader::new(data, chunk.body.start, chunk.body.end)?;
    if reader.i32()? != 1 {
        return Err(structural(
            chunk.body.start,
            "unsupported dimension chunk major version",
        ));
    }
    let version = reader.i32()?;
    if version < 0 {
        return Err(structural(
            chunk.body.start + 4,
            "negative dimension content version",
        ));
    }
    Ok((reader, chunk.next_offset, version))
}

fn uuid(reader: &mut BoundedReader<'_>) -> Result<Uuid, FramingError> {
    Ok(Uuid::from_wire(reader.array()?))
}

fn point2(reader: &mut BoundedReader<'_>) -> Result<[f64; 2], FramingError> {
    let value = [reader.f64()?, reader.f64()?];
    if value.iter().all(|value| value.is_finite()) {
        Ok(value)
    } else {
        Err(structural(
            reader.position() - 16,
            "dimension point is not finite",
        ))
    }
}

fn text_content(
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
) -> Result<String, FramingError> {
    let (mut text, next, version) = anonymous(data, reader.position(), reader.end(), archive)?;
    if version != 0 {
        return Err(structural(
            text.position(),
            "unsupported text-content version",
        ));
    }
    let rich_text = utf16(&mut text)?;
    plane(&mut text)?;
    for label in ["text rectangle width", "text rotation"] {
        let value = text.f64()?;
        if !value.is_finite() {
            return Err(structural(
                text.position() - 8,
                format!("{label} is not finite"),
            ));
        }
    }
    text.i32()?;
    text.i32()?;
    if !text.f64()?.is_finite() {
        return Err(structural(
            text.position() - 8,
            "obsolete text height is not finite",
        ));
    }
    text.bool()?;
    if text.remaining() != 0 {
        return Err(structural(
            text.position(),
            "text content has trailing bytes",
        ));
    }
    reader.skip(next - reader.position())?;
    Ok(rich_text)
}

fn annotation(
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
) -> Result<Annotation, FramingError> {
    let (mut annotation, next, version) =
        anonymous(data, reader.position(), reader.end(), archive)?;
    if version > 4 {
        return Err(structural(
            annotation.position(),
            "unsupported annotation version",
        ));
    }
    let rich_text = text_content(data, &mut annotation, archive)?;
    let dimstyle_id = uuid(&mut annotation)?;
    let plane = plane(&mut annotation)?;
    let annotation_type = if version >= 1 { annotation.i32()? } else { 0 };
    if version >= 2 {
        let (mut overrides, override_next, override_version) =
            anonymous(data, annotation.position(), annotation.end(), archive)?;
        if override_version != 1 {
            return Err(structural(
                overrides.position(),
                "unsupported dimension override version",
            ));
        }
        if overrides.bool()? {
            let wrapper = chunk_at(data, overrides.position(), overrides.end(), archive, false)?;
            let mut warnings = Vec::new();
            parse_class_wrapper(
                data,
                overrides.position()..wrapper.next_offset,
                archive,
                &mut warnings,
            )?;
            overrides.skip(wrapper.next_offset - overrides.position())?;
        }
        if overrides.remaining() != 0 {
            return Err(structural(
                overrides.position(),
                "dimension overrides have trailing bytes",
            ));
        }
        annotation.skip(override_next - annotation.position())?;
    }
    let horizontal_direction = if version >= 3 {
        point2(&mut annotation)?
    } else {
        [1.0, 0.0]
    };
    let allow_text_scaling = version < 4 || annotation.bool()?;
    if annotation.remaining() != 0 {
        return Err(structural(
            annotation.position(),
            "annotation has trailing bytes",
        ));
    }
    reader.skip(next - reader.position())?;
    Ok(Annotation {
        rich_text,
        dimstyle_id,
        plane,
        kind: annotation_type,
        horizontal_direction,
        allow_text_scaling,
    })
}

fn scaled_point(value: [f64; 2], scale: f64, offset: usize) -> Result<[f64; 2], FramingError> {
    Ok([
        scaled_coordinate(value[0], scale)
            .ok_or_else(|| structural(offset, "scaled dimension point is invalid"))?,
        scaled_coordinate(value[1], scale)
            .ok_or_else(|| structural(offset, "scaled dimension point is invalid"))?,
    ])
}

fn angular_measurement(first: [f64; 2], second: [f64; 2], line: [f64; 2]) -> f64 {
    let mut a1 = first[1].atan2(first[0]);
    let mut a2 = second[1].atan2(second[0]);
    let mut middle = line[1].atan2(line[0]);
    if a1.abs() < 2.328_306_436_538_696_3e-10 {
        a1 = 0.0;
    } else {
        a2 -= a1;
        middle -= a1;
        a1 = 0.0;
    }
    if a2 < 0.0 {
        a2 += std::f64::consts::TAU;
    }
    if middle < 0.0 {
        middle += std::f64::consts::TAU;
    }
    if middle > a1 && middle < a2 {
        a2 - a1
    } else if middle > a1 {
        a2
    } else {
        0.0
    }
}

/// Decodes one modern linear, angular, or radial dimension.
pub(crate) fn decode(
    data: &[u8],
    class: Uuid,
    range: Range<usize>,
    scale: f64,
    archive: ArchiveVersion,
) -> Result<Dimension, FramingError> {
    let (mut outer, outer_next, outer_version) = anonymous(data, range.start, range.end, archive)?;
    if outer_next != range.end || outer_version != 0 {
        return Err(structural(
            range.start,
            "unsupported dimension family version",
        ));
    }
    let (mut common, common_next, common_version) =
        anonymous(data, outer.position(), outer.end(), archive)?;
    if common_version > 1 {
        return Err(structural(
            common.position(),
            "unsupported common dimension version",
        ));
    }
    let annotation = annotation(data, &mut common, archive)?;
    let user_text = utf16(&mut common)?;
    if !common.f64()?.is_finite() {
        return Err(structural(
            common.position() - 8,
            "obsolete text rotation is not finite",
        ));
    }
    let use_default_text_point = common.bool()?;
    let text_offset = common.position();
    let user_text_point = scaled_point(point2(&mut common)?, scale, text_offset)?;
    let flip_arrows = [common.bool()?, common.bool()?];
    common.i32()?;
    let detail_measured = uuid(&mut common)?;
    let distance_scale = common.f64()?;
    if !distance_scale.is_finite() || distance_scale <= 0.0 {
        return Err(structural(
            common.position() - 8,
            "dimension distance scale is invalid",
        ));
    }
    if common_version >= 1 {
        common.i32()?;
    }
    if common.remaining() != 0 {
        return Err(structural(
            common.position(),
            "common dimension has trailing bytes",
        ));
    }
    outer.skip(common_next - outer.position())?;
    let definition = if class == LINEAR {
        if !matches!(annotation.kind, 1 | 5) {
            return Err(structural(
                outer.position(),
                "invalid linear annotation type",
            ));
        }
        let offset = outer.position();
        let definition_point = scaled_point(point2(&mut outer)?, scale, offset)?;
        let offset = outer.position();
        let dimension_line_point = scaled_point(point2(&mut outer)?, scale, offset)?;
        Definition::Linear {
            definition_point,
            dimension_line_point,
        }
    } else if class == ANGULAR {
        if !matches!(annotation.kind, 2 | 11) {
            return Err(structural(
                outer.position(),
                "invalid angular annotation type",
            ));
        }
        let first = point2(&mut outer)?;
        let second = point2(&mut outer)?;
        let first_extension_offset = scaled_coordinate(outer.f64()?, scale).ok_or_else(|| {
            structural(outer.position() - 8, "angular extension offset is invalid")
        })?;
        let second_extension_offset = scaled_coordinate(outer.f64()?, scale).ok_or_else(|| {
            structural(outer.position() - 8, "angular extension offset is invalid")
        })?;
        let offset = outer.position();
        let line = point2(&mut outer)?;
        let dimension_line_point = scaled_point(line, scale, offset)?;
        Definition::Angular {
            first_direction: first,
            second_direction: second,
            first_extension_offset,
            second_extension_offset,
            dimension_line_point,
        }
    } else if class == RADIAL {
        if !matches!(annotation.kind, 3 | 4) {
            return Err(structural(
                outer.position(),
                "invalid radial annotation type",
            ));
        }
        let offset = outer.position();
        let radius_point = scaled_point(point2(&mut outer)?, scale, offset)?;
        let offset = outer.position();
        let dimension_line_point = scaled_point(point2(&mut outer)?, scale, offset)?;
        Definition::Radial {
            radius_point,
            dimension_line_point,
            diameter: annotation.kind == 3,
        }
    } else {
        return Err(structural(range.start, "unsupported dimension class"));
    };
    if outer.remaining() != 0 {
        return Err(structural(outer.position(), "dimension has trailing bytes"));
    }
    let measurement = match &definition {
        Definition::Linear {
            definition_point, ..
        } => definition_point[0].abs() * distance_scale,
        Definition::Angular {
            first_direction,
            second_direction,
            dimension_line_point,
            ..
        } => angular_measurement(*first_direction, *second_direction, *dimension_line_point),
        Definition::Radial {
            radius_point,
            diameter,
            ..
        } => {
            radius_point[0].hypot(radius_point[1])
                * distance_scale
                * if *diameter { 2.0 } else { 1.0 }
        }
    };
    if !measurement.is_finite() {
        return Err(structural(range.start, "dimension measurement is invalid"));
    }
    Ok(Dimension {
        source_range: range,
        annotation_type: annotation.kind,
        rich_text: annotation.rich_text,
        user_text,
        dimstyle_id: annotation.dimstyle_id,
        plane: annotation.plane,
        horizontal_direction: annotation.horizontal_direction,
        allow_text_scaling: annotation.allow_text_scaling,
        use_default_text_point,
        user_text_point,
        flip_arrows,
        detail_measured,
        distance_scale,
        definition,
        measurement,
    })
}

/// Projects a decoded dimension into one native operation and one evaluated parameter.
pub(crate) fn project(
    dimension: &Dimension,
    key: &str,
    name: Option<String>,
    native_ref: String,
) -> (
    cadmpeg_ir::features::Feature,
    cadmpeg_ir::features::DesignParameter,
) {
    use cadmpeg_ir::features::{
        Angle, DesignParameter, DimensionDisplay, Feature, FeatureDefinition, FeatureId, Length,
        ParameterId, ParameterValue,
    };
    use std::collections::BTreeMap;

    let feature_id = FeatureId(format!("rhino:dimension:feature#{key}"));
    let parameter_id = ParameterId(format!("rhino:dimension:parameter#{key}"));
    let (kind, value, display) = match dimension.definition {
        Definition::Linear { .. } => (
            "linear_dimension",
            ParameterValue::Length(Length(dimension.measurement)),
            None,
        ),
        Definition::Angular { .. } => (
            "angular_dimension",
            ParameterValue::Angle(Angle(dimension.measurement)),
            None,
        ),
        Definition::Radial { diameter, .. } => (
            if diameter {
                "diameter_dimension"
            } else {
                "radius_dimension"
            },
            ParameterValue::Length(Length(dimension.measurement)),
            Some(if diameter {
                DimensionDisplay::Diameter
            } else {
                DimensionDisplay::Radius
            }),
        ),
    };
    let mut parameters =
        BTreeMap::from([("measurement".to_string(), dimension.measurement.to_string())]);
    let mut properties = BTreeMap::from([
        (
            "annotation_type".to_string(),
            dimension.annotation_type.to_string(),
        ),
        ("dimstyle_id".to_string(), dimension.dimstyle_id.to_string()),
        (
            "detail_measured".to_string(),
            dimension.detail_measured.to_string(),
        ),
        (
            "distance_scale".to_string(),
            dimension.distance_scale.to_string(),
        ),
        ("rich_text".to_string(), dimension.rich_text.clone()),
        ("user_text".to_string(), dimension.user_text.clone()),
        (
            "use_default_text_point".to_string(),
            dimension.use_default_text_point.to_string(),
        ),
        (
            "user_text_point".to_string(),
            format!(
                "{},{}",
                dimension.user_text_point[0], dimension.user_text_point[1]
            ),
        ),
        (
            "flip_arrows".to_string(),
            format!("{},{}", dimension.flip_arrows[0], dimension.flip_arrows[1]),
        ),
        (
            "allow_text_scaling".to_string(),
            dimension.allow_text_scaling.to_string(),
        ),
        (
            "plane_origin".to_string(),
            dimension
                .plane
                .origin
                .0
                .iter()
                .map(f64::to_string)
                .collect::<Vec<_>>()
                .join(","),
        ),
        (
            "horizontal_direction".to_string(),
            dimension
                .horizontal_direction
                .iter()
                .map(f64::to_string)
                .collect::<Vec<_>>()
                .join(","),
        ),
    ]);
    match &dimension.definition {
        Definition::Linear {
            definition_point,
            dimension_line_point,
        } => {
            properties.insert(
                "definition_point".to_string(),
                format!("{},{}", definition_point[0], definition_point[1]),
            );
            properties.insert(
                "dimension_line_point".to_string(),
                format!("{},{}", dimension_line_point[0], dimension_line_point[1]),
            );
        }
        Definition::Angular {
            first_direction,
            second_direction,
            first_extension_offset,
            second_extension_offset,
            dimension_line_point,
        } => {
            properties.insert(
                "first_direction".to_string(),
                format!("{},{}", first_direction[0], first_direction[1]),
            );
            properties.insert(
                "second_direction".to_string(),
                format!("{},{}", second_direction[0], second_direction[1]),
            );
            properties.insert(
                "first_extension_offset".to_string(),
                first_extension_offset.to_string(),
            );
            properties.insert(
                "second_extension_offset".to_string(),
                second_extension_offset.to_string(),
            );
            properties.insert(
                "dimension_line_point".to_string(),
                format!("{},{}", dimension_line_point[0], dimension_line_point[1]),
            );
        }
        Definition::Radial {
            radius_point,
            dimension_line_point,
            ..
        } => {
            properties.insert(
                "radius_point".to_string(),
                format!("{},{}", radius_point[0], radius_point[1]),
            );
            properties.insert(
                "dimension_line_point".to_string(),
                format!("{},{}", dimension_line_point[0], dimension_line_point[1]),
            );
        }
    }
    parameters.insert("parameter_id".to_string(), parameter_id.0.clone());
    let feature = Feature {
        id: feature_id.clone(),
        ordinal: u64::try_from(dimension.source_range.start).expect("source offset fits u64"),
        name,
        suppressed: false,
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: Some("RhinoDimension".to_string()),
        source_text: None,
        source_content: vec![cadmpeg_ir::features::FeatureSourceContent::Parameter(
            parameter_id.clone(),
        )],
        outputs: Vec::new(),
        definition: FeatureDefinition::Native {
            kind: kind.to_string(),
            parameters,
            properties,
        },
        native_ref: Some(native_ref.clone()),
    };
    let parameter = DesignParameter {
        id: parameter_id,
        owner: feature_id,
        ordinal: 0,
        name: "measurement".to_string(),
        expression: if dimension.user_text.is_empty() {
            dimension.measurement.to_string()
        } else {
            dimension.user_text.clone()
        },
        display,
        value: Some(value),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: Some(native_ref),
    };
    (feature, parameter)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive_test_support::crc_chunk;

    fn utf16(value: &str) -> Vec<u8> {
        let mut units = value.encode_utf16().collect::<Vec<_>>();
        units.push(0);
        let mut bytes = (units.len() as u32).to_le_bytes().to_vec();
        for unit in units {
            bytes.extend(unit.to_le_bytes());
        }
        bytes
    }

    fn anonymous(version: i32, suffix: &[u8]) -> Vec<u8> {
        let mut body = 1_i32.to_le_bytes().to_vec();
        body.extend(version.to_le_bytes());
        body.extend(suffix);
        crc_chunk(ANONYMOUS, &body)
    }

    fn plane() -> Vec<u8> {
        [
            0.0, 0.0, 0.0, // origin
            1.0, 0.0, 0.0, // x axis
            0.0, 1.0, 0.0, // y axis
            0.0, 0.0, 1.0, // z axis
            0.0, 0.0, 1.0, 0.0, // equation
        ]
        .into_iter()
        .flat_map(f64::to_le_bytes)
        .collect()
    }

    fn payload(annotation_type: i32, family: &[u8]) -> Vec<u8> {
        let mut text = utf16("<>\n");
        text.extend(plane());
        text.extend(0.0_f64.to_le_bytes());
        text.extend(0.0_f64.to_le_bytes());
        text.extend(0_i32.to_le_bytes());
        text.extend(0_i32.to_le_bytes());
        text.extend(1.0_f64.to_le_bytes());
        text.push(0);

        let mut annotation = anonymous(0, &text);
        annotation.extend([0; 16]);
        annotation.extend(plane());
        annotation.extend(annotation_type.to_le_bytes());
        annotation.extend(anonymous(1, &[0]));
        annotation.extend(1.0_f64.to_le_bytes());
        annotation.extend(0.0_f64.to_le_bytes());
        annotation.push(1);

        let mut common = anonymous(4, &annotation);
        common.extend(utf16(""));
        common.extend(0.0_f64.to_le_bytes());
        common.push(1);
        common.extend(0.0_f64.to_le_bytes());
        common.extend(0.0_f64.to_le_bytes());
        common.extend([0, 0]);
        common.extend(0_i32.to_le_bytes());
        common.extend([0; 16]);
        common.extend(2.0_f64.to_le_bytes());
        common.extend(0_i32.to_le_bytes());

        let mut outer = anonymous(1, &common);
        outer.extend(family);
        anonymous(0, &outer)
    }

    #[test]
    fn decodes_dimension_families_and_measurements() {
        let archive = ArchiveVersion::V8;
        let linear_family = [3.0_f64, 4.0, 8.0, 9.0]
            .into_iter()
            .flat_map(f64::to_le_bytes)
            .collect::<Vec<_>>();
        let linear_bytes = payload(1, &linear_family);
        let linear = decode(&linear_bytes, LINEAR, 0..linear_bytes.len(), 10.0, archive).unwrap();
        assert_eq!(linear.measurement, 60.0);
        assert_eq!(linear.horizontal_direction, [1.0, 0.0]);

        let radial_family = [3.0_f64, 4.0, 8.0, 9.0]
            .into_iter()
            .flat_map(f64::to_le_bytes)
            .collect::<Vec<_>>();
        let radial_bytes = payload(3, &radial_family);
        let radial = decode(&radial_bytes, RADIAL, 0..radial_bytes.len(), 1.0, archive).unwrap();
        assert_eq!(radial.measurement, 20.0);

        let angular_family = [
            1.0_f64, 0.0, 0.0, 1.0, // directions
            2.0, 3.0, // extension offsets
            1.0, 1.0, // dimension-line point
        ]
        .into_iter()
        .flat_map(f64::to_le_bytes)
        .collect::<Vec<_>>();
        let angular_bytes = payload(2, &angular_family);
        let angular = decode(
            &angular_bytes,
            ANGULAR,
            0..angular_bytes.len(),
            1.0,
            archive,
        )
        .unwrap();
        assert_eq!(angular.measurement, std::f64::consts::FRAC_PI_2);
    }
}
