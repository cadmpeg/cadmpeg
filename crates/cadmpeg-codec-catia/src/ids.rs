// SPDX-License-Identifier: Apache-2.0
//! Admission of native identities for neutral history references.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::{Identity, IdentityComponent};

pub(crate) fn neutral_history_id(
    ctx: &DecodeContext<'_>,
    native_id: &str,
    kind: &IdentityComponent,
) -> Result<Identity, CodecError> {
    // The validated native identity is only split; it is dropped before return.
    let mut source_storage = ctx.reserve_scoped(0, "catia_neutral_history_source")?;
    let source = source_storage
        .with_storage(|| ctx.copy_retained_text(native_id, "catia_neutral_history_source"))?;
    let native = match Identity::admit_text(source, |work| {
        ctx.charge_work(work, "catia_neutral_history_validate_source")
    })? {
        Ok(native) => native,
        Err(value) => {
            return Err(CodecError::Malformed(ctx.format_retained(
                format_args!("identity is invalid: {value:?}"),
                "catia_neutral_history_invalid_source",
            )?));
        }
    };
    let (namespace, key) = ctx
        .split_once(native.as_str(), "#", "catia_neutral_history_key")?
        .ok_or_else(|| CodecError::malformed("CATIA history identity has no key"))?;
    let (format, rest) = ctx
        .split_once(namespace, ":", "catia_neutral_history_format")?
        .ok_or_else(|| CodecError::malformed("CATIA history identity has no format"))?;
    let (scope, _) = ctx
        .split_once(rest, ":", "catia_neutral_history_scope")?
        .ok_or_else(|| CodecError::malformed("CATIA history identity has no scope"))?;
    let derived = ctx.format_retained(
        format_args!("{format}:{scope}:{}#{key}", kind.as_str()),
        "catia_neutral_history_derived",
    )?;
    match Identity::admit_text(derived, |work| {
        ctx.charge_work(work, "catia_neutral_history_validate_derived")
    })? {
        Ok(identity) => Ok(identity),
        Err(value) => Err(CodecError::Malformed(ctx.format_retained(
            format_args!("identity is invalid: {value:?}"),
            "catia_neutral_history_invalid_derived",
        )?)),
    }
}

#[cfg(test)]
mod tests {
    use super::neutral_history_id;
    use cadmpeg_core::CodecError;

    #[test]
    fn history_kind_replacement_preserves_colon_keys() {
        let result = crate::test_support::with_service_context(|ctx| {
            neutral_history_id(
                ctx,
                "catia:graph:object#owner:child",
                &cadmpeg_ir::identity_component!("feature"),
            )
        });
        assert_eq!(
            result.expect("valid native identity").as_str(),
            "catia:graph:feature#owner:child"
        );
    }

    #[test]
    fn malformed_native_history_identity_is_refused() {
        for value in ["short", "catia:graph:object#", "catia:graph:object#a#b"] {
            assert!(matches!(
                crate::test_support::with_service_context(|ctx| neutral_history_id(
                    ctx,
                    value,
                    &cadmpeg_ir::identity_component!("feature")
                )),
                Err(CodecError::Malformed(_))
            ));
        }
    }

    #[test]
    fn neutral_history_identity_refuses_retained_limit() {
        let refused = crate::test_support::with_retained_limit(0, |ctx| {
            neutral_history_id(
                ctx,
                "catia:graph:object#owner:child",
                &cadmpeg_ir::identity_component!("feature"),
            )
        });
        assert!(matches!(refused, Err(CodecError::ResourceLimit(limit))
            if limit.operation == "catia_neutral_history_derived"));
        let scoped = crate::test_support::with_materialized_limit(0, |ctx| {
            neutral_history_id(
                ctx,
                "catia:graph:object#owner:child",
                &cadmpeg_ir::identity_component!("feature"),
            )
        });
        assert!(matches!(scoped, Err(CodecError::ResourceLimit(limit))
            if limit.operation == "catia_neutral_history_source"));
        let admitted = crate::test_support::with_service_context(|ctx| {
            neutral_history_id(
                ctx,
                "catia:graph:object#owner:child",
                &cadmpeg_ir::identity_component!("feature"),
            )
        })
        .expect("service profile admits neutral identity");
        assert_eq!(admitted.as_str(), "catia:graph:feature#owner:child");
    }

    #[test]
    fn neutral_history_work_refusals_preserve_each_scan() {
        let mut refused_operations = std::collections::BTreeSet::new();
        for work_limit in 0..512 {
            let (result, refusal) = crate::test_support::with_work_limit(work_limit, |ctx| {
                let result = neutral_history_id(
                    ctx,
                    "catia:graph:object#owner:child",
                    &cadmpeg_ir::identity_component!("feature"),
                );
                (result, ctx.resource_refusal())
            });
            match result {
                Err(CodecError::ResourceLimit(limit)) => {
                    assert_eq!(refusal, Some(limit));
                    refused_operations.insert(limit.operation);
                }
                Ok(identity) => {
                    assert_eq!(identity.as_str(), "catia:graph:feature#owner:child");
                    assert_eq!(refusal, None);
                }
                Err(error) => panic!("unexpected identity error: {error}"),
            }
        }
        for operation in [
            "catia_neutral_history_validate_source",
            "catia_neutral_history_key",
            "catia_neutral_history_format",
            "catia_neutral_history_scope",
            "catia_neutral_history_validate_derived",
        ] {
            assert!(refused_operations.contains(operation), "{operation}");
        }
    }

    #[test]
    fn malformed_history_identity_preserves_error_text_and_format_refusal() {
        for value in [
            "short",
            "catia:graph:object#",
            "catia:graph:object#a#b",
            "catia:graph:object#é\n",
        ] {
            let expected = CodecError::malformed(
                cadmpeg_ir::ids::Identity::new(value).expect_err("invalid identity"),
            );
            let result = crate::test_support::with_service_context(|ctx| {
                neutral_history_id(ctx, value, &cadmpeg_ir::identity_component!("feature"))
            });
            let Err(CodecError::Malformed(actual)) = result else {
                panic!("malformed identity required")
            };
            let CodecError::Malformed(expected) = expected else {
                panic!("malformed expectation required")
            };
            assert_eq!(actual, expected);
        }
        // The native source copy is scoped, so the first retained charge is
        // the formatted error text.
        let (result, original) = crate::test_support::with_retained_limit(0, |ctx| {
            let result =
                neutral_history_id(ctx, "short", &cadmpeg_ir::identity_component!("feature"));
            (result, ctx.resource_refusal())
        });
        let Err(CodecError::ResourceLimit(limit)) = result else {
            panic!("error formatting must refuse")
        };
        assert_eq!(limit.operation, "catia_neutral_history_invalid_source");
        assert_eq!(original, Some(limit));
    }
}
