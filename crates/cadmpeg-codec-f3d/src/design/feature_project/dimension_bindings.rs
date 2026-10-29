// SPDX-License-Identifier: Apache-2.0
//! Resolve source parameter identities for dimension-bearing construction slots.

use crate::records::parameters::DesignParameter;
use cadmpeg_ir::features::{
    Dimension, ExtrudeExtent, ExtrudeSide, ExtrudeStart, LinearTermination,
};

pub(super) fn parameter<Q>(source: &DesignParameter) -> Dimension<Q> {
    Dimension::Parameter(crate::ids::neutral_parameter_id(source))
}

/// Bind an explicitly referenced source owner; the source scalar remains a
/// literal only when that source parameter has not been represented.
pub(super) fn owner<Q>(
    parameters: &[(u32, &DesignParameter)],
    owner: u32,
    literal: Q,
) -> Dimension<Q> {
    let mut matching = parameters
        .iter()
        .map(|(_, parameter)| *parameter)
        .filter(|parameter| parameter.owner_record_index() == Some(owner));
    match (matching.next(), matching.next()) {
        (Some(source), None) => parameter(source),
        _ => Dimension::Literal(literal),
    }
}

/// Count quantities come from typed source pattern controls, never a float cast.
pub(super) fn count_cache(
    parameter: &DesignParameter,
    scopes: &[crate::records::feature::scope::DesignParameterScope],
) -> Option<u32> {
    let owner = parameter.owner_record_index()?;
    let stream = crate::ids::native_stream(&parameter.id)?;
    let mut counts = Vec::new();
    for scope in scopes
        .iter()
        .filter(|scope| crate::ids::native_stream(&scope.id) == Some(stream))
    {
        if let Some(pattern) = scope.rectangular_pattern_construction() {
            for (index, count) in [pattern.u_count(), pattern.v_count()]
                .into_iter()
                .enumerate()
            {
                if pattern.owner_record_indices[index] == owner {
                    counts.push(count);
                }
            }
        }
        if let Some(pattern) = scope.circular_pattern_construction() {
            if pattern.count_record_index == owner {
                counts.push(pattern.count.get());
            }
        }
    }
    let count = *counts.first()?;
    (counts.iter().all(|candidate| *candidate == count)
        && parameter.evaluated_value().get() == f64::from(count))
    .then_some(count)
}

/// State implicit literal units in canonical expression text.
pub(super) fn expression(parameter: &DesignParameter) -> String {
    let source = parameter.expression();
    if source.trim().parse::<f64>().is_ok() {
        if let Some(unit) = parameter.unit().map(|unit| unit.value.as_str()) {
            if super::design_length_unit(unit) || super::design_angle_unit(unit) {
                return format!("{} {unit}", source.trim());
            }
        }
    }
    source.to_owned()
}

fn unique<'a>(
    parameters: &[(u32, &'a DesignParameter)],
    kind: &str,
) -> Option<&'a DesignParameter> {
    let mut matching = parameters
        .iter()
        .map(|(_, parameter)| *parameter)
        .filter(|parameter| parameter.source_kind() == kind);
    let source = matching.next()?;
    matching.next().is_none().then_some(source)
}

pub(super) fn extrude(
    start: &mut ExtrudeStart,
    extent: &mut ExtrudeExtent,
    parameters: &[(u32, &DesignParameter)],
) {
    if let Some(source) = unique(parameters, "ProfileOffset") {
        match start {
            ExtrudeStart::OffsetProfilePlane { offset } => *offset = parameter(source),
            ExtrudeStart::FromFace { offset, .. } => *offset = Some(parameter(source)),
            ExtrudeStart::Unresolved {}
            | ExtrudeStart::ProfilePlane {}
            | ExtrudeStart::FromPlane { .. } => {}
        }
    }
    match extent {
        ExtrudeExtent::OneSided { side } | ExtrudeExtent::Symmetric { side } => {
            bind_side(
                side,
                parameters,
                "AlongDistance",
                "Side1Offset",
                "TaperAngle",
            );
        }
        ExtrudeExtent::TwoSided { first, second } => {
            bind_side(
                first,
                parameters,
                "AlongDistance",
                "Side1Offset",
                "TaperAngle",
            );
            bind_side(
                second,
                parameters,
                "AgainstDistance",
                "Side2Offset",
                "Side2TaperAngle",
            );
        }
    }
}

fn bind_side(
    side: &mut ExtrudeSide,
    parameters: &[(u32, &DesignParameter)],
    distance: &str,
    offset_kind: &str,
    draft: &str,
) {
    match &mut side.termination {
        LinearTermination::Blind { length } => {
            if let Some(source) = unique(parameters, distance) {
                *length = parameter(source);
            }
        }
        LinearTermination::ToFace { offset, .. } => {
            if let Some(source) = unique(parameters, offset_kind) {
                *offset = Some(parameter(source));
            }
        }
        _ => {}
    }
    if side.draft.is_some() {
        if let Some(source) = unique(parameters, draft) {
            side.draft = Some(parameter(source));
        }
    }
}

pub(super) fn coil(
    construction: &mut cadmpeg_ir::features::CoilConstruction,
    parameters: &[(u32, &DesignParameter)],
) {
    use cadmpeg_ir::features::{CoilExtent, CoilSection};
    if let Some(source) = unique(parameters, "Diameter") {
        construction.diameter = parameter(source);
    }
    if let Some(source) = unique(parameters, "TaperAngle") {
        construction.taper = parameter(source);
    }
    if let Some(source) = unique(parameters, "SectionSize") {
        match &mut construction.section {
            CoilSection::Circular { diameter } => *diameter = parameter(source),
            CoilSection::Square { size }
            | CoilSection::ExternalTriangle { size }
            | CoilSection::InternalTriangle { size } => *size = parameter(source),
        }
    }
    match &mut construction.extent {
        CoilExtent::RevolutionsHeight {
            revolutions,
            height,
        } => {
            if let Some(source) = unique(parameters, "Revolutions") {
                *revolutions = parameter(source);
            }
            if let Some(source) = unique(parameters, "Height") {
                *height = parameter(source);
            }
        }
        CoilExtent::RevolutionsPitch { revolutions, pitch } => {
            if let Some(source) = unique(parameters, "Revolutions") {
                *revolutions = parameter(source);
            }
            if let Some(source) = unique(parameters, "Pitch") {
                *pitch = parameter(source);
            }
        }
        CoilExtent::HeightPitch { height, pitch } => {
            if let Some(source) = unique(parameters, "Height") {
                *height = parameter(source);
            }
            if let Some(source) = unique(parameters, "Pitch") {
                *pitch = parameter(source);
            }
        }
        CoilExtent::Spiral {
            revolutions,
            radial_pitch,
        } => {
            if let Some(source) = unique(parameters, "Revolutions") {
                *revolutions = parameter(source);
            }
            if let Some(source) = unique(parameters, "Pitch") {
                *radial_pitch = parameter(source);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::decode::parameters::parse_design_parameter_record;
    use crate::design::test_support::parameter_record;
    use cadmpeg_ir::scalar::NonZeroLength;

    #[test]
    fn source_parameter_binding_has_no_second_dimension_value() {
        let source = parse_design_parameter_record(&parameter_record(
            Some(94),
            "-10 mm",
            "AlongDistance",
            Some("mm"),
            "depth",
            -1.0,
        ))
        .unwrap();
        let binding: Dimension<NonZeroLength> = parameter(&source);
        assert_eq!(
            binding.parameter(),
            Some(&crate::ids::neutral_parameter_id(&source))
        );
        assert!(binding.literal().is_none());
        let wire = serde_json::to_value(binding).unwrap();
        assert_eq!(wire.as_object().unwrap().len(), 2);
        assert_eq!(wire["kind"], "parameter");
    }

    #[test]
    fn travel_and_zero_face_offset_bind_their_distinct_parameters() {
        let distance = parse_design_parameter_record(&parameter_record(
            Some(94),
            "-10 mm",
            "AlongDistance",
            Some("mm"),
            "travel",
            -1.0,
        ))
        .unwrap();
        let offset = parse_design_parameter_record(&parameter_record(
            Some(95),
            "0 mm",
            "ProfileOffset",
            Some("mm"),
            "start",
            0.0,
        ))
        .unwrap();
        let mut start = ExtrudeStart::OffsetProfilePlane {
            offset: cadmpeg_ir::scalar::Length::new(0.0).unwrap().into(),
        };
        let mut extent = ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: LinearTermination::Blind {
                    length: NonZeroLength::new(-10.0).unwrap().into(),
                },
                draft: None,
            },
        };
        extrude(&mut start, &mut extent, &[(0, &distance), (1, &offset)]);
        let ExtrudeStart::OffsetProfilePlane { offset: binding } = start else {
            panic!("offset start")
        };
        assert_eq!(
            binding.parameter(),
            Some(&crate::ids::neutral_parameter_id(&offset))
        );
        let ExtrudeExtent::OneSided { side } = extent else {
            panic!("one-sided extent")
        };
        let LinearTermination::Blind { length } = side.termination else {
            panic!("blind travel")
        };
        assert_eq!(
            length.parameter(),
            Some(&crate::ids::neutral_parameter_id(&distance))
        );
    }
    #[test]
    fn implicit_source_literal_unit_is_explicit_after_projection() {
        let source = parse_design_parameter_record(&parameter_record(
            Some(94),
            "2.5",
            "AlongDistance",
            Some("cm"),
            "depth",
            2.5,
        ))
        .unwrap();
        let canonical: cadmpeg_ir::features::Expression = expression(&source).into();
        let cache = cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(25.0).unwrap(),
        );
        assert_eq!(canonical.literal_text().as_deref(), Some("2.5 cm"));
        assert_eq!(canonical.literal_value_as(&cache), Some(cache));
        let dependent = parse_design_parameter_record(&parameter_record(
            Some(95),
            "depth * 2",
            "AlongDistance",
            Some("cm"),
            "other",
            5.0,
        ))
        .unwrap();
        assert_eq!(expression(&dependent), "depth * 2");
    }
}
