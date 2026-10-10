// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_core::decode::{DecodeArena, ResourceDimension};

#[cfg(test)]
struct IdentityCodec(&'static str);

#[cfg(test)]
impl CodecBackend for IdentityCodec {
    const FORMAT: FormatId = FormatId::new("selected");

    fn detect_impl(&self, _: &DecodeContext<'_>, _: View<'_>) -> Result<Confidence, CodecError> {
        unreachable!("the fixture inspects and decodes only")
    }

    fn inspect_impl(
        &self,
        _: &DecodeContext<'_>,
        _: View<'_>,
    ) -> Result<ContainerSummary, CodecError> {
        Ok(serde_json::from_value(serde_json::json!({
            "identity":{"classification":"unclassified","format":self.0},
            "container_kind":"flat","entries":[],"notes":[],
        }))
        .unwrap())
    }

    fn decode_impl(&self, _: &DecodeContext<'_>, _: View<'_>) -> Result<Decoded, CodecError> {
        let mut ir = CadIr::empty();
        ir.source = Some(
            serde_json::from_value(serde_json::json!({
                "identity":{"classification":"unclassified","format":self.0},
                "attributes":{},
            }))
            .unwrap(),
        );
        Ok(decoded(ir))
    }
}

#[test]
fn sealed_inspect_format_identity_admits_only_the_compared_prefix() {
    // read_root moves its budget after input acquisition. Select a positive
    // byte charge so a new probe address cannot match the zero-work fuse gate.
    let probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        "inspect format identity",
        Some(1),
    );
    let mut options = InspectOptions::default();
    options.limits.max_work_units = u64::MAX;
    let Err(CodecError::ResourceLimit(first)) =
        IdentityCodec("selected").inspect(&mut Cursor::new([]), &options)
    else {
        panic!("the first format byte must have a resource boundary");
    };
    drop(probe);
    assert_eq!(first.operation, "inspect format identity");
    assert_eq!(first.additional, 1);
    let acquisition_work = first.used;
    // The cap==0 case below replays exactly one below that first byte need.
    // Only the input prerequisite is discovered; byte counts stay explicit.
    for (format, visits, matches) in [
        ("xelected", 1, false),
        ("selXcted", 4, false),
        ("selecteX", 8, false),
        ("selected", 8, true),
    ] {
        for cap in 0..=visits {
            let mut options = InspectOptions::default();
            options.limits.max_work_units = acquisition_work + cap;
            let result = IdentityCodec(format).inspect(&mut Cursor::new([]), &options);
            if cap < visits {
                let Err(CodecError::ResourceLimit(first)) = result else {
                    panic!("the next format byte must refuse");
                };
                assert_eq!(first.dimension, ResourceDimension::WorkUnits);
                assert_eq!(first.operation, "inspect format identity");
                assert_eq!((first.used, first.additional), (acquisition_work + cap, 1));
            } else if matches {
                assert_eq!(result.unwrap().format(), format);
            } else {
                let Err(CodecError::ResourceLimit(first)) = result else {
                    panic!(
                        "format comparison must reach its diagnostic without scanning the suffix"
                    );
                };
                assert_eq!(first.operation, "inspect format refusal");
            }
        }
    }
    let mut options = InspectOptions::default();
    options.limits.max_work_units = acquisition_work;
    let Err(CodecError::ResourceLimit(first)) =
        IdentityCodec("short").inspect(&mut Cursor::new([]), &options)
    else {
        panic!("the unequal length reaches the diagnostic");
    };
    assert_eq!(first.operation, "inspect format refusal");
    assert_eq!(first.used, acquisition_work);
    assert!(matches!(
        IdentityCodec("short").inspect(&mut Cursor::new([]), &InspectOptions::default()),
        Err(CodecError::WrongFormat(_))
    ));
}

#[test]
fn sealed_decode_format_identity_preserves_prefix_admission_and_original_fuse() {
    let run = |format, cap| {
        let arena = DecodeArena::new();
        let mut options = DecodeOptions::default();
        options.policy.limits.max_work_units = cap;
        let (ctx, root) = DecodeContext::from_root_bytes(&[], &arena, &options.policy).unwrap();
        let result = IdentityCodec(format).decode_with_context(&ctx, root, &options);
        if let Err(DecodeFailure::Codec(CodecError::ResourceLimit(first))) = &result {
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == *first)
            );
        } else {
            ctx.finish_session().unwrap();
        }
        match result {
            Ok(value) => Ok(value),
            Err(DecodeFailure::Codec(error)) => Err(error),
            Err(DecodeFailure::StrictRejected { .. }) => unreachable!("no fixture losses"),
        }
    };
    for (format, visits, matches) in [
        ("xelected", 1, false),
        ("selXcted", 4, false),
        ("selecteX", 8, false),
        ("selected", 8, true),
    ] {
        let CodecError::ResourceLimit(first) = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::WorkUnits,
            "decode format identity",
            |cap| run(format, cap),
        ) else {
            panic!("the first compared byte has an ordinary resource boundary");
        };
        assert_eq!(first.additional, 1);
        // The probe identifies existing classification/finalization work. Each
        // following comparison byte adds exactly one before access.
        for visited in 0..visits {
            let Err(CodecError::ResourceLimit(next)) = run(format, first.used + visited) else {
                panic!("the next compared byte must refuse");
            };
            assert_eq!(next.operation, "decode format identity");
            assert_eq!((next.used, next.additional), (first.used + visited, 1));
        }
        let result = run(format, first.used + visits);
        if matches {
            assert_eq!(result.unwrap().report().format(), format);
        } else {
            let Err(CodecError::ResourceLimit(next)) = result else {
                panic!("the compared prefix reaches the diagnostic");
            };
            assert_eq!(next.operation, "decode format refusal");
        }
    }
    let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        "decode format identity",
        None,
    );
    assert!(matches!(
        run("short", u64::MAX),
        Err(CodecError::WrongFormat(_))
    ));
}
