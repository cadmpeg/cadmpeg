// SPDX-License-Identifier: Apache-2.0
//! Keep native attributes of analytic-surface support entities.

use super::Entity;
use cadmpeg_ir::geometry::Surface;
use cadmpeg_ir::native::NativeRecord;
use cadmpeg_ir::{CadIr, CodecFormat};
use serde_json::Value;

fn number_at(parameters: &[Value], index: usize) -> Option<f64> {
    let token = parameters.get(index)?.get("value")?;
    match token.get("kind")?.as_str()? {
        "integer" => token
            .get("value")?
            .as_i64()
            .and_then(cadmpeg_core::convert::f64_from_i64),
        "real" => token.get("value")?.as_f64(),
        _ => None,
    }
}

fn sequence_at(parameters: &[Value], index: usize) -> Option<u64> {
    parameters.get(index)?.get("value")?.get("value")?.as_u64()
}

fn label(record: &NativeRecord) -> Option<String> {
    let bytes: cadmpeg_ir::native::bytes::NativeBytes =
        serde_json::from_value(record.field("label")?).ok()?;
    let text = std::str::from_utf8(bytes.as_ref()).ok()?;
    (text.len() <= 8 && text.bytes().all(|b| b.is_ascii_graphic() || b == b' '))
        .then(|| text.to_owned())
}

/// Native integer/real token kinds are independent of the geometric value.
/// Retain them only where every numeric token still equals the current support.
pub(super) fn retain_numeric_tokens(entity: &mut Entity, record: &NativeRecord) {
    let Some(parameters) = record.field("parameters") else {
        return;
    };
    let Some(parameters) = parameters.as_array() else {
        return;
    };
    let Ok(body) = std::str::from_utf8(&entity.parameter_body) else {
        return;
    };
    let Some(body) = body.strip_suffix(';') else {
        return;
    };
    let values = body.split(',').collect::<Vec<_>>();
    if parameters.len() != values.len() + 1 {
        return;
    }
    let mut output = Vec::with_capacity(values.len());
    for (index, value) in values.iter().enumerate() {
        let Some(original) = number_at(parameters, index + 1) else {
            return;
        };
        let Ok(current) = value.replace(['D', 'd'], "E").parse::<f64>() else {
            return;
        };
        if original != current {
            return;
        }
        let token = &parameters[index + 1]["value"];
        let Some(kind) = token.get("kind").and_then(Value::as_str) else {
            return;
        };
        if kind == "integer" {
            let Some(integer) = token.get("value").and_then(Value::as_i64) else {
                return;
            };
            output.push(integer.to_string());
        } else if kind == "real" {
            output.push((*value).to_owned());
        } else {
            return;
        }
    }
    entity.parameter_body = format!("{};", output.join(",")).into_bytes();
}

pub(super) fn retain_point_parameters(
    ir: &CadIr,
    point: &cadmpeg_ir::topology::Point,
    entity: &mut Entity,
) {
    let Some(source) = point
        .source_object
        .as_ref()
        .filter(|source| source.format == CodecFormat::Iges)
    else {
        return;
    };
    let Some(sequence) = source.object_id.as_str().strip_prefix('D') else {
        return;
    };
    let id = format!("iges:entity:directory#{sequence}");
    let Some(record) = ir
        .native
        .namespace("iges")
        .and_then(|namespace| namespace.arenas().get("entities"))
        .and_then(|records| records.iter().find(|record| record.id() == id))
    else {
        return;
    };
    if record.field("entity_type").and_then(|value| value.as_i64()) != Some(116) {
        return;
    }
    let Some(parameters) = record.field("parameters") else {
        return;
    };
    let Some(parameters) = parameters.as_array() else {
        return;
    };
    let symbol = if parameters.len() == 4 {
        false
    } else if parameters.len() >= 5
        && parameters[4]["value"]["kind"].as_str() == Some("integer")
        && parameters[4]["value"]["value"].as_i64() == Some(0)
    {
        true
    } else {
        return;
    };
    let coordinates = point.position().coordinates();
    let mut values = Vec::new();
    for (index, coordinate) in coordinates.into_iter().enumerate() {
        let Some(value) = number_at(parameters, index + 1) else {
            return;
        };
        if value != coordinate.get() {
            return;
        }
        let token = &parameters[index + 1]["value"];
        match token.get("kind").and_then(Value::as_str) {
            Some("integer") => {
                let Some(integer) = token.get("value").and_then(Value::as_i64) else {
                    return;
                };
                values.push(integer.to_string());
            }
            Some("real") => {
                let mut text = value.to_string();
                if text.len() > 24 {
                    text = format!("{value:e}");
                }
                if !text.contains(['.', 'e', 'E']) {
                    text.push_str(".0");
                }
                values.push(text);
            }
            _ => return,
        }
    }
    if symbol {
        values.push("0".into());
    }
    entity.parameter_body = format!("{};", values.join(",")).into_bytes();
}

fn retain_surface_numbers(entity: &mut Entity, surface: &Surface, parameters: &[Value]) {
    let Ok(body) = std::str::from_utf8(&entity.parameter_body) else {
        return;
    };
    let Some(body) = body.strip_suffix(';') else {
        return;
    };
    let values = body.split(',').collect::<Vec<_>>();
    if parameters.len() < values.len() + 1 {
        return;
    }
    let mut output = Vec::with_capacity(values.len());
    for (index, value) in values.iter().enumerate() {
        let token = &parameters[index + 1]["value"];
        let original = number_at(parameters, index + 1);
        let current = value.replace(['D', 'd'], "E").parse::<f64>().ok();
        let same = match (original, current, surface.geometry.solved()) {
            (
                Some(original),
                Some(_),
                Some(cadmpeg_ir::geometry::SolvedSurfaceGeometry::Cone(cone)),
            ) if index == 3 => original.to_radians() == cone.half_angle().get(),
            (Some(original), Some(current), _) => original == current,
            _ => false,
        };
        let preserved = if same {
            match token.get("kind").and_then(Value::as_str) {
                Some("integer") => token
                    .get("value")
                    .and_then(Value::as_i64)
                    .map(|n| n.to_string()),
                Some("real") => original
                    .and_then(cadmpeg_ir::scalar::FiniteReal::new)
                    .map(super::number),
                _ => None,
            }
        } else {
            None
        };
        output.push(preserved.unwrap_or_else(|| (*value).to_owned()));
    }
    entity.parameter_body = format!("{};", output.join(",")).into_bytes();
}

pub(super) fn retain(ir: &CadIr, surface: &Surface, entities: &mut Vec<Entity>, base: usize) {
    if entities.len() != 4 {
        return;
    }
    let Some(source) = surface
        .source_object
        .as_ref()
        .filter(|source| source.format == CodecFormat::Iges)
    else {
        return;
    };
    let Some(owner) = source
        .object_id
        .as_str()
        .strip_prefix('D')
        .and_then(|s| s.parse::<u64>().ok())
    else {
        return;
    };
    let Some(records) = ir
        .native
        .namespace("iges")
        .and_then(|ns| ns.arenas().get("entities"))
    else {
        return;
    };
    let find = |sequence| {
        records.iter().find(|record| {
            record.field("directory_sequence").and_then(|v| v.as_u64()) == Some(sequence)
        })
    };
    let Some(parent) = find(owner) else { return };
    let Some(kind) = parent.field("entity_type").and_then(|v| v.as_u64()) else {
        return;
    };
    if kind != u64::from(entities[3].type_code) {
        return;
    }
    let last = match kind {
        190 => 3,
        192 | 196 => 4,
        194 | 198 => 5,
        _ => return,
    };
    let Some(parameters) = parent.field("parameters") else {
        return;
    };
    let Some(parameters) = parameters.as_array() else {
        return;
    };
    retain_surface_numbers(&mut entities[3], surface, parameters);
    for (entity, index) in entities.iter_mut().take(3).zip([1, 2, last]) {
        let Some(record) = sequence_at(parameters, index).and_then(find) else {
            continue;
        };
        if record.field("entity_type").and_then(|v| v.as_u64()) != Some(u64::from(entity.type_code))
        {
            continue;
        }
        if let Some(label) = label(record) {
            entity.label = label;
        }
        entity.source_targets.push(record.id().to_owned());
        retain_numeric_tokens(entity, record);
    }
    implicit_plane(ir, surface, entities, base);
}

fn implicit_plane(ir: &CadIr, surface: &Surface, entities: &mut Vec<Entity>, base: usize) {
    let Some(cadmpeg_ir::geometry::SolvedSurfaceGeometry::Plane(plane)) = surface.geometry.solved()
    else {
        return;
    };
    if entities.len() != 4 || *plane.frame().reference() != plane.frame().axis().derived_reference()
    {
        return;
    }
    let Some(source) = surface
        .source_object
        .as_ref()
        .filter(|source| source.format == CodecFormat::Iges)
    else {
        return;
    };
    let Some(sequence) = source.object_id.as_str().strip_prefix('D') else {
        return;
    };
    let id = format!("iges:entity:directory#{sequence}");
    let Some(record) = ir
        .native
        .namespace("iges")
        .and_then(|ns| ns.arenas().get("entities"))
        .and_then(|records| records.iter().find(|record| record.id() == id))
    else {
        return;
    };
    if record.field("entity_type").and_then(|v| v.as_i64()) != Some(190)
        || record.field("form").and_then(|v| v.as_i64()) != Some(0)
        || record.field("transform").and_then(|v| v.as_i64()) != Some(0)
    {
        return;
    }
    let Some(axis) = base.checked_add(1) else {
        return;
    };
    entities.remove(2);
    let Some(entity) = entities.last_mut() else {
        return;
    };
    entity.form = 0;
    entity.parameter_body = format!(
        "{},{};",
        super::reference_marker(base),
        super::reference_marker(axis)
    )
    .into_bytes();
}

#[cfg(test)]
mod tests {
    use crate::test_support::plan_at;
    use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};
    use crate::{IgesCodec, IgesVersion};
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use cadmpeg_ir::geometry::analytic::PlaneSurface;
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
    use cadmpeg_ir::math::{Point3, Vector3};
    use std::io::Cursor;

    fn fixture() -> Vec<u8> {
        let specs = [
            (116, 0, "ORIGIN", "00010000", "116,1,2,3,0;"),
            (123, 0, "AXIS", "00010000", "123,0,0,1;"),
            (123, 0, "REFDIR", "00010000", "123,1,0,0;"),
            (190, 1, "PLANE", "00000000", "190,1,3,5;"),
        ];
        owned_test_file(
            &specs
                .into_iter()
                .map(
                    |(entity_type, form, label, status, parameters)| OwnedTestEntity {
                        entity_type,
                        form,
                        label: label.into(),
                        status,
                        parameters: parameters.into(),
                    },
                )
                .collect::<Vec<_>>(),
        )
    }

    pub(super) fn fixture_cone() -> Vec<u8> {
        let specs = [
            (116, 0, "ORIGIN", "00010000", "116,1,2,3,0;"),
            (123, 0, "AXIS", "00010000", "123,0,0,1;"),
            (123, 0, "REFDIR", "00010000", "123,1,0,0;"),
            (194, 1, "CONE", "00000000", "194,1,3,2,30,5;"),
        ];
        owned_test_file(
            &specs
                .into_iter()
                .map(
                    |(entity_type, form, label, status, parameters)| OwnedTestEntity {
                        entity_type,
                        form,
                        label: label.into(),
                        status,
                        parameters: parameters.into(),
                    },
                )
                .collect::<Vec<_>>(),
        )
    }

    pub(super) fn written(ir: &cadmpeg_ir::CadIr) -> cadmpeg_ir::CadIr {
        let plan = plan_at(IgesVersion::V5_3, ir, None).expect("valid test value");
        let mut bytes = Vec::new();
        plan.write_to(&mut bytes).expect("valid test value");
        IgesCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("valid test value")
            .ir()
            .clone()
    }

    #[test]
    fn analytic_support_keeps_native_labels_and_numeric_tokens_but_writes_edited_geometry() {
        let decoded = IgesCodec
            .decode(&mut Cursor::new(fixture()), &DecodeOptions::default())
            .expect("valid test value");
        let mut ir = decoded.ir().clone();
        let after = written(&ir);
        let entities = &after
            .native
            .namespace("iges")
            .expect("valid test value")
            .arenas()["entities"];
        let source = ir
            .native
            .namespace("iges")
            .expect("valid test value")
            .arenas()["entities"]
            .clone();
        for (before, after) in source.iter().take(3).zip(entities.iter()) {
            assert_eq!(before.field("label"), after.field("label"));
            assert_eq!(before.field("parameters"), after.field("parameters"));
        }
        ir.model.surfaces[0].geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            PlaneSurface::try_new(
                Point3::new(9.0, 2.0, 3.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("valid test value"),
        ));
        let edited = written(&ir);
        let Some(SolvedSurfaceGeometry::Plane(plane)) = edited.model.surfaces[0].geometry.solved()
        else {
            panic!("plane")
        };
        assert_eq!(plane.origin().get(), Point3::new(9.0, 2.0, 3.0));
        let entities = &edited
            .native
            .namespace("iges")
            .expect("valid test value")
            .arenas()["entities"];
        assert_eq!(source[0].field("label"), entities[0].field("label"));
    }
}

#[cfg(test)]
mod cone_tests {
    use super::tests::{fixture_cone, written};
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use cadmpeg_ir::geometry::analytic::ConeSurface;
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
    use cadmpeg_ir::math::{Point3, Vector3};
    use std::io::Cursor;

    #[test]
    fn cone_numbers_keep_current_native_values_and_follow_neutral_edits() {
        let decoded = crate::IgesCodec
            .decode(&mut Cursor::new(fixture_cone()), &DecodeOptions::default())
            .expect("valid test value");
        let mut ir = decoded.ir().clone();
        let before = ir
            .native
            .namespace("iges")
            .expect("valid test value")
            .arenas()["entities"][3]
            .field("parameters");
        let unchanged = written(&ir);
        assert_eq!(
            before,
            unchanged
                .native
                .namespace("iges")
                .expect("valid test value")
                .arenas()["entities"][3]
                .field("parameters")
        );
        ir.model.surfaces[0].geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
            ConeSurface::try_new(
                Point3::new(1.0, 2.0, 3.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                4.0,
                1.0,
                45.0_f64.to_radians(),
            )
            .expect("valid test value"),
        ));
        let edited = written(&ir);
        let Some(SolvedSurfaceGeometry::Cone(cone)) = edited.model.surfaces[0].geometry.solved()
        else {
            panic!("cone")
        };
        assert_eq!(cone.radius().get(), 4.0);
        assert_eq!(cone.half_angle().get(), 45.0_f64.to_radians());
    }
}
