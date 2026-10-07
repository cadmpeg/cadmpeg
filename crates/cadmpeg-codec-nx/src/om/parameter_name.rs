// SPDX-License-Identifier: Apache-2.0
//! Exact parameter spelling with its parsed index and qualifier boundary.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use std::convert::Infallible;

/// A name parsed once at construction. `u32` requires canonical parameter syntax;
/// `Option<u32>` also admits ordinary expression names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ParameterName<S, I = Option<u32>> {
    spelling: S,
    index: I,
    qualifier_start: Option<usize>,
}

impl<S: crate::immutable_text::ImmutableText> ParameterName<S> {
    pub(crate) fn new(spelling: S) -> Self {
        let (index, qualifier_start) = match canonical_parts(
            spelling.as_ref(),
            |text| Ok::<_, Infallible>(text.bytes().take_while(u8::is_ascii_digit).count()),
            |text| {
                Ok::<_, Infallible>(
                    text.bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_'),
                )
            },
            |text| Ok::<_, Infallible>(text.parse::<u32>()),
        ) {
            Ok(parts) => parts,
            Err(error) => match error {},
        }
        .map_or((None, None), |(index, qualifier)| (Some(index), qualifier));
        Self {
            spelling,
            index,
            qualifier_start,
        }
    }
    pub(crate) fn from_wire(ctx: &DecodeContext<'_>, spelling: S) -> Result<Self, CodecError> {
        let (index, qualifier_start) = canonical_parts(
            spelling.as_ref(),
            |text| {
                Ok(ctx
                    .position_by(
                        text.bytes(),
                        |byte| Ok(!byte.is_ascii_digit()),
                        "NX parameter name syntax",
                    )?
                    .unwrap_or(text.len()))
            },
            |text| {
                ctx.all_by(
                    text.bytes(),
                    |byte| Ok(byte.is_ascii_alphanumeric() || byte == b'_'),
                    "NX parameter qualifier syntax",
                )
            },
            |text| ctx.parse_text::<u32>(text, "NX parameter index decimal parse"),
        )?
        .map_or((None, None), |(index, qualifier)| (Some(index), qualifier));
        Ok(Self {
            spelling,
            index,
            qualifier_start,
        })
    }
}

impl<S: crate::immutable_text::ImmutableText> ParameterName<S, u32> {
    pub(crate) fn parse(spelling: S) -> Option<Self> {
        let (index, qualifier_start) = match canonical_parts(
            spelling.as_ref(),
            |text| Ok::<_, Infallible>(text.bytes().take_while(u8::is_ascii_digit).count()),
            |text| {
                Ok::<_, Infallible>(
                    text.bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_'),
                )
            },
            |text| Ok::<_, Infallible>(text.parse::<u32>()),
        ) {
            Ok(parts) => parts?,
            Err(error) => match error {},
        };
        Some(Self {
            spelling,
            index,
            qualifier_start,
        })
    }
    pub(crate) fn parse_wire(
        ctx: &DecodeContext<'_>,
        spelling: S,
    ) -> Result<Option<Self>, CodecError> {
        let Some((index, qualifier_start)) = canonical_parts(
            spelling.as_ref(),
            |text| {
                Ok(ctx
                    .position_by(
                        text.bytes(),
                        |byte| Ok(!byte.is_ascii_digit()),
                        "NX canonical parameter name syntax",
                    )?
                    .unwrap_or(text.len()))
            },
            |text| {
                ctx.all_by(
                    text.bytes(),
                    |byte| Ok(byte.is_ascii_alphanumeric() || byte == b'_'),
                    "NX parameter qualifier syntax",
                )
            },
            |text| ctx.parse_text::<u32>(text, "NX parameter index decimal parse"),
        )?
        else {
            return Ok(None);
        };
        Ok(Some(Self {
            spelling,
            index,
            qualifier_start,
        }))
    }
}

impl<S: crate::immutable_text::ImmutableText, I: Copy> ParameterName<S, I> {
    pub(crate) fn as_str(&self) -> &str {
        self.spelling.as_ref()
    }

    pub(crate) fn index(&self) -> I {
        self.index
    }

    pub(crate) fn qualifier(&self) -> Option<&str> {
        self.qualifier_start
            .map(|start| &self.spelling.as_ref()[start..])
    }

    #[cfg(test)]
    pub(crate) fn into_spelling(self) -> S {
        self.spelling
    }
}

fn canonical_parts<E>(
    name: &str,
    mut count_digits: impl FnMut(&str) -> Result<usize, E>,
    mut valid_qualifier: impl FnMut(&str) -> Result<bool, E>,
    mut parse_decimal: impl FnMut(&str) -> Result<Result<u32, std::num::ParseIntError>, E>,
) -> Result<Option<(u32, Option<usize>)>, E> {
    let Some(tail) = name.strip_prefix('p') else {
        return Ok(None);
    };
    let digit_count = count_digits(tail)?;
    if digit_count == 0 {
        return Ok(None);
    }
    let Ok(index) = parse_decimal(&tail[..digit_count])? else {
        return Ok(None);
    };
    match &tail[digit_count..] {
        "" => Ok(Some((index, None))),
        suffix => {
            let Some(qualifier) = suffix.strip_prefix('_') else {
                return Ok(None);
            };
            if qualifier.is_empty() || !valid_qualifier(qualifier)? {
                return Ok(None);
            }
            Ok(Some((index, Some(name.len() - qualifier.len()))))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ParameterName;

    #[test]
    fn parameter_index_decimal_parse_refusal_propagates() {
        for name in ["p12_face_A", "p4294967296"] {
            for canonical in [false, true] {
                let error = crate::test_support::resource_refusal_at(
                    &[],
                    cadmpeg_core::decode::ResourceDimension::WorkUnits,
                    "NX parameter index decimal parse",
                    |ctx| {
                        if canonical {
                            ParameterName::<_, u32>::parse_wire(ctx, name)
                                .map(|name| name.is_some())
                        } else {
                            ParameterName::from_wire(ctx, name).map(|name| name.index().is_some())
                        }
                    },
                );
                assert!(
                    matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.additional == if name == "p12_face_A" { 2 } else { 10 })
                );
            }
        }
    }

    #[test]
    fn parameter_name_immutable_storage_retains_the_checked_parts() {
        let text = "p12_face_A";
        let borrowed = ParameterName::<_, u32>::parse(text).unwrap();
        let owned = ParameterName::<_, u32>::parse(text.to_owned()).unwrap();
        for _ in 0..3 {
            assert_eq!(borrowed.as_str(), text);
            assert_eq!(owned.as_str(), text);
            assert_eq!(
                (borrowed.index(), borrowed.qualifier()),
                (12, Some("face_A"))
            );
            assert_eq!((owned.index(), owned.qualifier()), (12, Some("face_A")));
        }
    }
    #[test]
    fn parameter_name_iteration_refusal_propagates() {
        use cadmpeg_core::decode::ResourceDimension;
        use cadmpeg_core::CodecError;
        for name in ["p12_face_A", "p12_bad-"] {
            for canonical in [false, true] {
                let operation = if canonical {
                    "NX canonical parameter name syntax"
                } else {
                    "NX parameter name syntax"
                };
                let error = crate::test_support::resource_refusal_at(
                    &[],
                    ResourceDimension::WorkUnits,
                    operation,
                    |ctx| {
                        if canonical {
                            ParameterName::<_, u32>::parse_wire(ctx, name)
                                .map(|value| value.is_some())
                        } else {
                            ParameterName::from_wire(ctx, name).map(|value| value.index().is_some())
                        }
                    },
                );
                assert!(matches!(error, CodecError::ResourceLimit(limit)
                    if limit.dimension == ResourceDimension::WorkUnits && limit.operation == operation));
            }
        }
    }
}
