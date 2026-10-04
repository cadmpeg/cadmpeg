// SPDX-License-Identifier: Apache-2.0
//! Literals used by the NX object-model decoder.

use crate::om::ExpressionUnit;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

/// Root OM entity marker.
pub(crate) const ROOT_MARKER: &[u8] = b"\x04\x01\x0eNX ";
/// Section marker required before numeric expressions are decoded.
pub(crate) const HOST_GLOBALS: &[u8] = b"hostglobalvariables";
/// Registered class-definition name prefix.
pub(crate) const CLASS_NAME_PREFIX: &[u8] = b"UGS::";
/// Numeric-expression payload prefix.
pub(crate) const NUMBER_PREFIX: &[u8] = b"(Number [";

/// Resolve a numeric-expression unit token.
pub(crate) fn unit_for(
    ctx: &DecodeContext<'_>,
    token: &str,
) -> Result<Option<ExpressionUnit>, CodecError> {
    if token.is_empty() || !token.bytes().all(|byte| byte.is_ascii_graphic()) {
        return Ok(None);
    }
    Ok(Some(match token {
        "mm" => ExpressionUnit::Millimeter,
        "in" => ExpressionUnit::Inch,
        "degrees" => ExpressionUnit::Degree,
        token => ExpressionUnit::Native(ctx.format_retained(
            format_args!("{token}"),
            "NX native expression unit",
        )?),
    }))
}

#[cfg(test)]
mod tests {
    use super::unit_for;
    use crate::om::ExpressionUnit;

    #[test]
    fn resolves_known_and_native_units() {
        crate::test_support::with_decode_context(|ctx| {
            assert_eq!(unit_for(ctx, "mm").unwrap(), Some(ExpressionUnit::Millimeter));
            assert_eq!(unit_for(ctx, "in").unwrap(), Some(ExpressionUnit::Inch));
            assert_eq!(unit_for(ctx, "degrees").unwrap(), Some(ExpressionUnit::Degree));
            assert_eq!(
                unit_for(ctx, "custom/unit").unwrap(),
                Some(ExpressionUnit::Native("custom/unit".into()))
            );
            assert_eq!(unit_for(ctx, "").unwrap(), None);
            assert_eq!(unit_for(ctx, "custom unit").unwrap(), None);
        });
    }

    fn native_unit_format_refusal(dimension: cadmpeg_core::decode::ResourceDimension) {
        use cadmpeg_core::decode::ResourceDimension;
        crate::test_support::with_decode_context_over(
            &[],
            |policy| match dimension {
                ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
                ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = 0,
                _ => panic!("unit formatting uses work and retained bytes"),
            },
            |ctx| {
                let cadmpeg_core::CodecError::ResourceLimit(limit) = unit_for(ctx, "custom/unit").unwrap_err() else {
                    panic!("native unit formatting must propagate refusal");
                };
                assert_eq!(limit.operation, "NX native expression unit");
                assert_eq!(limit.dimension, dimension);
                assert_eq!(ctx.resource_refusal(), Some(limit));
            },
        );
    }

    #[test]
    fn native_unit_format_refuses_work() {
        native_unit_format_refusal(cadmpeg_core::decode::ResourceDimension::WorkUnits);
    }

    #[test]
    fn native_unit_format_refuses_retained_bytes() {
        native_unit_format_refusal(cadmpeg_core::decode::ResourceDimension::RetainedBytes);
    }
}
