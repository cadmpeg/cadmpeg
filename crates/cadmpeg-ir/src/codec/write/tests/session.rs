// SPDX-License-Identifier: Apache-2.0
//! One encode is one session: a ceiling that refuses a read of the write
//! refuses the encode.

use super::*;
use cadmpeg_core::decode::{LimitScope, ResourceDimension};

/// A backend that reads past the input ceiling, drops the refusal, and
/// either plans its write or returns a different error.
struct DroppedRefusal {
    fail_after_refusal: bool,
}

impl EncoderBackend for DroppedRefusal {
    const FORMAT: FormatId = FormatId::new("cadir");
    const NAME: &'static str = "CADIR";
    const REQUEST: DecodeDepth = DecodeDepth::Full;
    const REUSES_SOURCE: bool = false;
    type Target = DialectFree;
    const TARGET: DialectFree = DialectFree;

    fn plan_resolved(
        &self,
        ctx: &DecodeContext<'_>,
        input: EncodeInput<'_>,
        (): (),
    ) -> Result<ExportBody, CodecError> {
        assert!(ctx.charge_input(u64::MAX, "the test read").is_err());
        if self.fail_after_refusal {
            return Err(CodecError::NotImplemented(
                "cannot produce this output".into(),
            ));
        }
        Ok(ExportBody::synthesized(
            Vec::new(),
            input.ir,
            ArenaCoverage::Complete,
            NativeDispositions::written(),
        ))
    }
}

#[test]
fn a_refusal_the_backend_drops_refuses_the_encode() {
    let ir = CadIr::empty();
    for fail_after_refusal in [false, true] {
        let refused = DroppedRefusal { fail_after_refusal }.plan(EncodeInput::new(&ir, None));
        let Err(EncodeError::ResourceLimit(limit)) = refused else {
            panic!("expected a resource refusal: {:?}", refused.map(drop));
        };
        assert_eq!(
            (limit.dimension, limit.scope, limit.operation),
            (
                ResourceDimension::InputBytes,
                LimitScope::Session,
                "the test read"
            )
        );
    }
}
