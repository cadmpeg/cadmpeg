// SPDX-License-Identifier: Apache-2.0
//! Decode source attribute chains into typed attribute values, colors, names,
//! and transforms.

use crate::ids::{brep_id, IdFormat};
use crate::nurbs::reader::LEN_TO_MM;
use crate::sab::{Record, Token};
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::ids::{AttributeId, Identity, IdentityComponent, UnknownId};
use cadmpeg_ir::topology::Color;
use std::collections::{hash_map::RandomState, HashMap, HashSet};

/// Follow `entity`'s attribute chain, emitting each record not yet in
/// `emitted` as a [`SourceAttribute`] bound to `target`.
pub fn collect_attributes(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    entity: &Record,
    target: &AttributeTarget,
    by_index: &HashMap<i64, &Record, RandomState>,
    emitted: (&mut HashSet<i64, RandomState>, &mut cadmpeg_core::decode::ScopedReservation<'_>),
    out: &mut Vec<SourceAttribute>,
    format: IdFormat,
) -> Result<(), cadmpeg_core::CodecError> {
    let (emitted, emitted_storage) = emitted;
    let mut current = entity.ref_at(0);
    let mut chain = HashSet::new();
    let mut chain_storage = ctx.reserve_scoped(0, "ASM attribute chain")?;
    while let Some(index) = current {
        ctx.charge_work(1, "ASM attribute chain walk")?;
        if !chain_storage.with_storage(|| ctx.insert_hash_set(&mut chain, index, "ASM attribute chain"))? {
            break;
        }
        let Some(record) = by_index.get(&index) else {
            break;
        };
        if emitted_storage.with_storage(|| ctx.insert_hash_set(emitted, index, "ASM emitted attributes"))? {
            ctx.reserve_vec(out, 1, "ASM source attributes")?;
            out.push(source_attribute(
                ctx,
                record,
                target.try_clone_for_decode(ctx, "ASM source attribute target")?,
                format,
            )?);
        }
        current = attribute_next(record);
    }
    Ok(())
}

fn is_integer(token: Option<&Token>) -> bool {
    matches!(
        token,
        Some(Token::Char(_) | Token::Short(_) | Token::Long(_) | Token::Enum(_) | Token::Int64(_))
    )
}

#[derive(Clone, Copy)]
enum AttributeBase {
    Current,
    Legacy,
    Compact,
}

impl AttributeBase {
    fn next(self) -> usize {
        match self {
            Self::Current => 2,
            Self::Legacy => 1,
            Self::Compact => 0,
        }
    }

    fn owner(self) -> Option<usize> {
        match self {
            Self::Current => Some(4),
            Self::Legacy => Some(3),
            Self::Compact => None,
        }
    }

    fn payload(self) -> usize {
        match self {
            Self::Current => 5,
            Self::Legacy => 4,
            Self::Compact => 1,
        }
    }
}

fn attribute_base(record: &Record) -> Option<AttributeBase> {
    let current = matches!(
        (
            record.chunk(0),
            record.chunk(2),
            record.chunk(3),
            record.chunk(4),
        ),
        (
            Some(Token::Ref(_)),
            Some(Token::Ref(_)),
            Some(Token::Ref(_)),
            Some(Token::Ref(_)),
        )
    ) && is_integer(record.chunk(1));
    if current {
        return Some(AttributeBase::Current);
    }
    let legacy = matches!(
        (
            record.chunk(0),
            record.chunk(1),
            record.chunk(2),
            record.chunk(3),
        ),
        (
            Some(Token::Ref(_)),
            Some(Token::Ref(_)),
            Some(Token::Ref(_)),
            Some(Token::Ref(_)),
        )
    );
    if legacy {
        return Some(AttributeBase::Legacy);
    }
    matches!(record.chunk(0), Some(Token::Ref(_))).then_some(AttributeBase::Compact)
}

/// The next record in an attribute chain.
///
/// A current ASM attribute starts with `reserved, marker, next, previous,
/// owner`; a legacy attribute omits `marker`. Source-less streams written by
/// older cadmpeg versions used a compact record whose first field was `next`;
/// retain read compatibility with all three forms.
fn attribute_next(record: &Record) -> Option<i64> {
    record.ref_at(attribute_base(record)?.next())
}

/// The topology or parent-attribute owner of a current or legacy attribute.
pub(super) fn attribute_owner(record: &Record) -> Option<i64> {
    record.ref_at(attribute_base(record)?.owner()?)
}

/// The numeric record-index key of an attribute id
/// (`<format>:brep:attribute#<index>`), used to key records derived from that
/// attribute.
pub fn attribute_key<'attribute>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    attribute: &'attribute SourceAttribute,
) -> Result<&'attribute str, cadmpeg_core::CodecError> {
    Ok(ctx.rsplit_once(attribute.id.as_str(), "#", "ASM attribute identity key")?
        .map_or(attribute.id.as_str(), |(_, key)| key))
}

/// Serialize one attribute record's value chunks as a [`SourceAttribute`]
/// bound to `target`. A NaN or infinite number refuses the record.
pub fn source_attribute(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    record: &Record,
    target: AttributeTarget,
    format: IdFormat,
) -> Result<SourceAttribute, cadmpeg_core::CodecError> {
    // Chunks, not raw tokens: the serialized value list is defined over the
    // value tokens, and a payload identifier names an embedded construction
    // rather than carrying an attribute value.
    let mut values = Vec::new();
    for token in ctx.admit_iter(record.tokens.as_ref(), "ASM source attribute tokens")?.filter(|token| !token.is_payload_ident()) {
        ctx.reserve_vec(&mut values, 1, "ASM attribute values")?;
        let value = attribute_value(ctx, token, format)?.ok_or_else(|| {
            cadmpeg_core::CodecError::malformed(format_args!(
                "attribute record {} ({}) holds a non-finite number",
                record.index, record.name
            ))
        })?;
        values.push(value);
    }
    Ok(SourceAttribute {
        id: brep_id!(format, AttributeId, "attribute", record.index),
        target,
        name: ctx.copy_retained_text(&record.name, "ASM attribute record name")?,
        values,
    })
}

/// The attribute value one token carries, or `None` for a number that is not
/// finite.
fn attribute_value(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    token: &Token,
    format: IdFormat,
) -> Result<Option<AttributeValue>, cadmpeg_core::CodecError> {
    Ok(Some(match token {
        Token::Char(value) => AttributeValue::Integer(i64::from(*value)),
        Token::Short(value) => AttributeValue::Integer(i64::from(*value)),
        Token::Long(value) | Token::Enum(value) | Token::Int64(value) => {
            AttributeValue::Integer(*value)
        }
        Token::Float(value) => match AttributeValue::float(f64::from(*value)) {
            Some(value) => value,
            None => return Ok(None),
        },
        Token::Double(value) => match AttributeValue::float(*value) {
            Some(value) => value,
            None => return Ok(None),
        },
        Token::Str(value) => {
            AttributeValue::String(ctx.copy_retained_text(value, "ASM attribute string")?)
        }
        Token::True => AttributeValue::Boolean(true),
        Token::False => AttributeValue::Boolean(false),
        Token::Ref(value) => {
            AttributeValue::Reference(brep_id!(format, Identity, "entity", value).into_string())
        }
        Token::SubtypeOpen => AttributeValue::String(ctx.copy_retained_text("subtype_open", "ASM attribute subtype marker")?),
        Token::SubtypeClose => AttributeValue::String(ctx.copy_retained_text("subtype_close", "ASM attribute subtype marker")?),
        Token::Position(value) | Token::Vector3(value) => match AttributeValue::vector(*value) {
            Some(value) => value,
            None => return Ok(None),
        },
        Token::Vector2(value) => match AttributeValue::vector(*value) {
            Some(value) => value,
            None => return Ok(None),
        },
        Token::Ident(value) | Token::SubIdent(value) => {
            AttributeValue::String(ctx.copy_retained_text(value, "ASM attribute identifier")?)
        }
    }))
}

/// Decode a native transform record into an IR affine transform, scaling the
/// translation into millimetres.
pub fn decode_transform(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    record: &Record,
    header_scale: f64,
) -> Result<Option<cadmpeg_ir::transform::Transform>, cadmpeg_core::CodecError> {
    let mut vectors = [None; 4];
    let mut vector_count = 0;
    let mut scale = None;
    for token in ctx.admit_iter(record.tokens.as_ref(), "ASM transform tokens")? {
        match token {
            Token::Position(value) | Token::Vector3(value) => {
                if let Some(slot) = vectors.get_mut(vector_count) {
                    *slot = Some(*value);
                }
                vector_count += 1;
            }
            Token::Double(value) => scale = Some(*value),
            _ => {}
        }
    }
    let [Some(x), Some(y), Some(z), Some(translation)] = vectors else {
        return Ok(None);
    };
    // The attribute stores a homogeneous scale in the `w` slot. Only the
    // unscaled spelling maps to an affine transform.
    if vector_count != 4 || scale != Some(1.0) {
        return Ok(None);
    }
    Ok(cadmpeg_ir::transform::Transform::affine([
        [x[0], y[0], z[0], translation[0] * header_scale * LEN_TO_MM],
        [x[1], y[1], z[1], translation[1] * header_scale * LEN_TO_MM],
        [x[2], y[2], z[2], translation[2] * header_scale * LEN_TO_MM],
    ]))
}

/// Storage form and payload-field location of an exact direct-color attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectColorCarrier {
    /// Three normalized f64 channel fields.
    NormalizedRgb {
        /// Payload-field indices of red, green, and blue.
        fields: [usize; 3],
    },
    /// One Autodesk method-and-color packed integer field.
    AutodeskTrueColor {
        /// Payload-field index of the packed integer.
        field: usize,
    },
    /// One decimal-text packed RGB field.
    DecimalRgb {
        /// Payload-field index of the decimal text.
        field: usize,
    },
}

/// A decoded exact direct-color attribute and the carrier that supplied it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DirectAttributeColor {
    /// Opaque neutral RGB color.
    pub color: Color,
    /// Native payload form and field location.
    pub carrier: DirectColorCarrier,
}

fn packed_u32(value: i64) -> Option<u32> {
    u32::try_from(value)
        .ok()
        .or_else(|| i32::try_from(value).ok().map(i32::cast_unsigned))
}

fn packed_rgb(packed: u32) -> Option<Color> {
    let red = u8::try_from((packed >> 16) & 0xff).ok()?;
    let green = u8::try_from((packed >> 8) & 0xff).ok()?;
    let blue = u8::try_from(packed & 0xff).ok()?;
    Some(Color::from_rgba8(red, green, blue, 255))
}

/// Decode one well-formed exact direct-color attribute.
///
/// Palette, material-library, inherited truecolor, and malformed records do
/// not define a neutral RGB color.
fn direct_attribute_color(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    record: &Record,
) -> Result<Option<DirectAttributeColor>, cadmpeg_core::CodecError> {
    let Some(payload) = attribute_base(record).map(AttributeBase::payload) else {
        return Ok(None);
    };
    match record.name.as_str() {
        "rgb_color-st-attrib" => {
            let mut tokens = record.tokens.iter();
            let mut field = 0;
            let mut channels = [None; 5];
            for channel in &mut channels {
                *channel = ctx.find_map(&mut tokens, |token| {
                    if token.is_payload_ident() {
                        return Ok(None);
                    }
                    let current = field;
                    field += 1;
                    Ok(match token {
                        Token::Double(value) if current >= payload => Some((current, *value)),
                        _ => None,
                    })
                }, "ASM RGB attribute tokens")?;
                if channel.is_none() {
                    break;
                }
            }
            let [(r_field, r), (g_field, g), (b_field, b)] = match channels {
                [Some(red), Some(green), Some(blue), None, None] => [red, green, blue],
                [Some(red), Some(green), Some(blue), Some((_, 1.0)), None] => [red, green, blue],
                _ => return Ok(None),
            };
            if ![r, g, b]
                .into_iter()
                .all(|value| (0.0..=1.0).contains(&value))
            {
                return Ok(None);
            }
            let Some(red) = cadmpeg_core::convert::f32_from_f64(r) else {
                return Ok(None);
            };
            let Some(green) = cadmpeg_core::convert::f32_from_f64(g) else {
                return Ok(None);
            };
            let Some(blue) = cadmpeg_core::convert::f32_from_f64(b) else {
                return Ok(None);
            };
            let Some(color) = Color::new(red, green, blue, 1.0) else {
                return Ok(None);
            };
            Ok(Some(DirectAttributeColor {
                color,
                carrier: DirectColorCarrier::NormalizedRgb {
                    fields: [r_field, g_field, b_field],
                },
            }))
        }
        "truecolor-adesk-attrib" => {
            let Some((field, packed)) = ctx
                .admit_iter(record.tokens.as_ref(), "ASM truecolor attribute tokens")?
                .filter(|token| !token.is_payload_ident())
                .enumerate()
                .skip(payload)
                .filter_map(|(field, token)| match token {
                    Token::Int64(value) | Token::Long(value) => Some((field, *value)),
                    _ => None,
                })
                .last()
            else {
                return Ok(None);
            };
            let Some(packed) = packed_u32(packed) else {
                return Ok(None);
            };
            // AcCmColor stores its color method in the high byte. Only
            // kByColor carries self-contained RGB channels.
            if packed >> 24 != 0xc2 {
                return Ok(None);
            }
            let Some(color) = packed_rgb(packed) else {
                return Ok(None);
            };
            Ok(Some(DirectAttributeColor {
                color,
                carrier: DirectColorCarrier::AutodeskTrueColor { field },
            }))
        }
        "entatt_color-bt-attrib" => {
            let Some((field, text)) = ctx
                .admit_iter(record.tokens.as_ref(), "ASM decimal attribute tokens")?
                .filter(|token| !token.is_payload_ident())
                .enumerate()
                .skip(payload)
                .filter_map(|(field, token)| match token {
                    Token::Str(value) => Some((field, value.as_str())),
                    _ => None,
                })
                .last()
            else {
                return Ok(None);
            };
            if text.is_empty() || !ctx.all_by(text.as_bytes(), |byte| Ok(byte.is_ascii_digit()), "ASM decimal color digits")? {
                return Ok(None);
            }
            let parsed = ctx.parse_text::<u32>(text, "parse ASM decimal color")?;
            let Ok(packed) = parsed else {
                return Ok(None);
            };
            if packed > 0xff_ffff {
                return Ok(None);
            }
            let Some(color) = packed_rgb(packed) else {
                return Ok(None);
            };
            Ok(Some(DirectAttributeColor {
                color,
                carrier: DirectColorCarrier::DecimalRgb { field },
            }))
        }
        _ => Ok(None),
    }
}

/// The first well-formed exact direct-color carrier on an attribute chain.
pub fn attribute_chain_color_carrier<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    entity: &Record,
    max_steps: usize,
    mut by_index: impl FnMut(i64) -> Option<&'a Record>,
) -> Result<Option<(&'a Record, DirectAttributeColor)>, cadmpeg_core::CodecError> {
    let Some(mut current) = entity.ref_at(0) else {
        return Ok(None);
    };
    let mut visited = HashSet::new();
    let mut storage = ctx.reserve_scoped(0, "ASM color chain visited")?;
    for _ in 0..max_steps {
        ctx.charge_work(1, "ASM color chain walk")?;
        if !storage.with_storage(|| ctx.insert_hash_set(&mut visited, current, "ASM color chain visited"))? {
            break;
        }
        let Some(record) = by_index(current) else {
            return Ok(None);
        };
        if let Some(color) = direct_attribute_color(ctx, record)? {
            return Ok(Some((record, color)));
        }
        let Some(next) = attribute_next(record) else {
            return Ok(None);
        };
        current = next;
    }
    Ok(None)
}

/// The first well-formed exact direct color on `entity`'s attribute chain.
pub fn attribute_chain_color(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    entity: &Record,
    by_index: &HashMap<i64, &Record, RandomState>,
) -> Result<Option<Color>, cadmpeg_core::CodecError> {
    Ok(attribute_chain_color_carrier(ctx, entity, by_index.len(), |index| {
        by_index.get(&index).copied()
    })?
    .map(|(_, decoded)| decoded.color))
}

/// The first non-empty name attribute on `entity`'s attribute chain.
pub fn attribute_chain_name(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    entity: &Record,
    by_index: &HashMap<i64, &Record, RandomState>,
) -> Result<Option<String>, cadmpeg_core::CodecError> {
    let Some(mut current) = entity.ref_at(0) else {
        return Ok(None);
    };
    let mut visited = HashSet::new();
    let mut storage = ctx.reserve_scoped(0, "ASM name chain visited")?;
    for _ in 0..by_index.len() {
        ctx.charge_work(1, "ASM name chain walk")?;
        if !storage.with_storage(|| ctx.insert_hash_set(&mut visited, current, "ASM name chain visited"))? {
            break;
        }
        let Some(record) = by_index.get(&current) else {
            return Ok(None);
        };
        if record.name == "string_attrib-name_attrib-gen-attrib" {
            let mut values = ctx.admit_iter(record.tokens.as_ref(), "ASM name attribute tokens")?.filter_map(|token| match token {
                Token::Str(value) => Some(value.as_str()),
                _ => None,
            });
            let mut previous = None;
            let mut last = None;
            for value in &mut values {
                previous = last;
                last = Some(value);
            }
            if let (Some("name"), Some(value)) = (previous, last) {
                if !value.is_empty() {
                    let name = ctx.copy_retained_text(value, "ASM attribute name")?;
                    return Ok(Some(name));
                }
            }
        }
        let Some(next) = attribute_next(record) else {
            return Ok(None);
        };
        current = next;
    }
    Ok(None)
}

/// The `UnknownId` for a preserved carrier record. Shared by the passthrough
/// `UnknownRecord` and any `SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown)` that links to it, so the
/// reference resolves under validation.
pub fn unknown_record_id(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    rec: &Record,
    format: IdFormat,
) -> Result<UnknownId, cadmpeg_core::CodecError> {
    let mut kind_storage = ctx.reserve_scoped(0, "ASM unknown record kind scratch")?;
    let name = kind_storage.with_storage(|| ctx.copy_retained_text(rec.head(), "ASM unknown record kind"))?;
    let kind = IdentityComponent::try_new(name).map_err(|error| {
        cadmpeg_core::CodecError::malformed(format_args!(
            "invalid ASM source identity component: {error}"
        ))
    })?;
    Ok(UnknownId::from(
        format.try_brep_identity(ctx, &kind, rec.index)?,
    ))
}

#[cfg(test)]
mod tests {
    use super::decode_transform;
    use crate::sab::{Record, Token};

    #[test]
    fn unknown_record_identity_refuses_retained_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let record = Record {
            index: 1,
            name: "mystery".into(),
            tokens: std::sync::Arc::from([]),
            offset: 0,
            len: 0,
        };
        let expected = "f3d:brep:mystery#1";
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::RetainedBytes, "ASM unknown record identity", |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_retained_bytes = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
                super::unknown_record_id(&ctx, &record, crate::asm_format!("f3d"))
            },
        );
        assert_eq!(super::unknown_record_id(
            &cadmpeg_test_support::service_decode_context(), &record, crate::asm_format!("f3d"),
        ).unwrap().as_str(), expected);
        let CodecError::ResourceLimit(refusal) = error else {
            panic!("expected resource refusal, got {error:?}");
        };
        assert_eq!(refusal.dimension, ResourceDimension::RetainedBytes);
        assert_eq!(refusal.operation, "ASM unknown record identity");
    }

    #[test]
    fn unknown_record_kind_uses_scoped_storage() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

        let record = Record {
            index: 1,
            name: "mystery".into(),
            tokens: std::sync::Arc::from([]),
            offset: 0,
            len: 0,
        };
        let expected = "f3d:brep:mystery#1";
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = u64::try_from(expected.len()).unwrap();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let id = super::unknown_record_id(&ctx, &record, crate::asm_format!("f3d")).unwrap();
        assert_eq!(id.as_str(), expected);
    }

    #[test]
    fn attribute_subtype_markers_admit_retained_text() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_ir::attributes::AttributeValue;

        for (token, expected) in [(Token::SubtypeOpen, "subtype_open"), (Token::SubtypeClose, "subtype_close")] {
            let error = cadmpeg_test_support::refusal::resource_limit_at(
                ResourceDimension::RetainedBytes, "ASM attribute subtype marker", |cap| {
                    let arena = DecodeArena::new();
                    let mut policy = DecodePolicy::service();
                    policy.limits.max_retained_bytes = cap;
                    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
                    super::attribute_value(&ctx, &token, crate::asm_format!("f3d"))
                },
            );
            let cadmpeg_core::CodecError::ResourceLimit(limit) = error else { panic!("resource refusal") };
            assert_eq!(limit.operation, "ASM attribute subtype marker");
            assert_eq!(super::attribute_value(&cadmpeg_test_support::service_decode_context(), &token, crate::asm_format!("f3d")).unwrap(), Some(AttributeValue::String(expected.into())));
        }
    }

    #[test]
    fn attribute_chain_walk_refuses_work_before_following_link() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;
        use cadmpeg_ir::attributes::AttributeTarget;
        use std::collections::{HashMap, HashSet};

        let entity = Record {
            index: 0,
            name: "entity".into(),
            tokens: vec![Token::Ref(1)].into(),
            offset: 0,
            len: 0,
        };
        let attribute = Record {
            index: 1,
            name: "empty-st-attrib".into(),
            tokens: Vec::new().into(),
            offset: 0,
            len: 0,
        };
        let by_index = HashMap::from([(1, &attribute)]);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = super::collect_attributes(
            &ctx,
            &entity,
            &AttributeTarget::Document,
            &by_index,
            (&mut HashSet::new(), &mut ctx.reserve_scoped(0, "ASM test emitted attributes").unwrap()),
            &mut Vec::new(),
            crate::asm_format!("f3d"),
        )
        .unwrap_err();
        let CodecError::ResourceLimit(limit) = error else {
            panic!("expected work refusal: {error:?}");
        };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(limit.operation, "ASM attribute chain walk");
    }

    fn transform_record(scale: f64, x: [f64; 3]) -> Record {
        Record {
            index: 0,
            name: "transform".into(),
            tokens: std::sync::Arc::from([
                Token::Vector3(x),
                Token::Vector3([0.0, 1.0, 0.0]),
                Token::Vector3([0.0, 0.0, 1.0]),
                Token::Position([0.0, 0.0, 0.0]),
                Token::Double(scale),
            ]),
            offset: 0,
            len: 0,
        }
    }

    #[test]
    fn transform_decode_propagates_affine_constructor_rejection() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &cadmpeg_core::decode::DecodePolicy::service()).unwrap();
        let identity = transform_record(1.0, [1.0, 0.0, 0.0]);
        assert_eq!(
            decode_transform(&ctx, &identity, 1.0).unwrap(),
            Some(cadmpeg_ir::transform::Transform::identity())
        );
        for scale in [0.0, 2.0, f64::NAN, f64::INFINITY] {
            assert!(decode_transform(&ctx, &transform_record(scale, [1.0, 0.0, 0.0]), 1.0).unwrap().is_none());
        }
        assert!(decode_transform(&ctx, &transform_record(1.0, [f64::NAN, 0.0, 0.0]), 1.0).unwrap().is_none());
        assert!(decode_transform(&ctx, &identity, f64::INFINITY).unwrap().is_none());
    }
}
