// SPDX-License-Identifier: Apache-2.0
//! Types 304, 310 and 312 are rebuilt from typed font and glyph records.

use super::{Entity, Fields, Value, boolean, coordinates, count, entity, integer, real};

fn font_reference(fields: &mut Fields<'_>, code: &str, definition: &str) -> Result<String, String> {
    if fields.value(definition).is_some() {
        return Ok(format!("-{}", fields.pointer(definition)?));
    }
    let code = fields.integer(code)?;
    if code.starts_with('-') {
        return Err("a negative font code has no current font definition".into());
    }
    Ok(code)
}

pub(super) fn encode(
    arena: &str,
    fields: &mut Fields<'_>,
    type_code: u32,
    form: i64,
) -> Result<Entity, String> {
    let mut values = Vec::new();
    match arena {
        "line_fonts" if form == 1 => {
            values.push(fields.boolean("tangent_oriented")?);
            values.push(fields.pointer("template")?);
            values.push(fields.real("spacing")?);
            values.push(fields.real("scale")?);
        }
        "line_fonts" => {
            let lengths = fields.array("lengths")?;
            values.push(count(&fields.values, "segment_count", lengths.len())?);
            for length in &lengths {
                values.push(real(Some(length))?);
            }
            let pattern = fields
                .value("hexadecimal_pattern")
                .ok_or("line pattern has no typed hexadecimal string")?;
            let pattern = crate::text::field(pattern)?;
            if !pattern.iter().all(u8::is_ascii_hexdigit) {
                return Err("line pattern contains a nonhexadecimal character".into());
            }
            values.push(fields.text("hexadecimal_pattern")?);
        }
        "text_templates" => {
            values.extend(coordinates(fields.value("character_box"), 2)?);
            values.push(font_reference(fields, "font_code", "font_definition")?);
            values.push(fields.real("slant_angle")?);
            values.push(fields.real("rotation_angle")?);
            values.push(fields.integer("mirror")?);
            values.push(fields.integer("vertical")?);
            values.extend(coordinates(fields.value("origin_or_increment"), 3)?);
        }
        "text_fonts" => {
            values.push(fields.integer("font_code")?);
            values.push(fields.text("name")?);
            values.push(font_reference(
                fields,
                "supersedes_code",
                "supersedes_definition",
            )?);
            values.push(fields.integer("grid_units_per_text_height")?);
            let characters = fields.array("characters")?;
            values.push(count(
                &fields.values,
                "declared_character_count",
                characters.len(),
            )?);
            for character in characters {
                let glyph = character.as_object().ok_or("glyph has no typed fields")?;
                values.push(integer(glyph.get("character_code"))?);
                let next = glyph
                    .get("next_origin")
                    .and_then(Value::as_array)
                    .filter(|values| values.len() == 2)
                    .ok_or("glyph next origin has no two-coordinate lane")?;
                for coordinate in next {
                    values.push(integer(Some(coordinate))?);
                }
                let motions = glyph
                    .get("motions")
                    .and_then(Value::as_array)
                    .ok_or("glyph has no typed motion list")?;
                values.push(count(glyph, "declared_motion_count", motions.len())?);
                for motion in motions {
                    let motion = motion
                        .as_object()
                        .ok_or("glyph motion has no typed fields")?;
                    values.push(boolean(motion.get("pen_up"))?);
                    let point = motion
                        .get("point")
                        .and_then(Value::as_array)
                        .filter(|values| values.len() == 2)
                        .ok_or("glyph motion point has no two-coordinate lane")?;
                    for coordinate in point {
                        values.push(integer(Some(coordinate))?);
                    }
                }
            }
        }
        _ => return Err("record has no font grammar".into()),
    }
    Ok(entity(type_code, form, values))
}
