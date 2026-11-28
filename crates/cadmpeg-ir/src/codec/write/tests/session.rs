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

struct GeometryWriter;

impl EncoderBackend for GeometryWriter {
    const FORMAT: FormatId = FormatId::new("test");
    const NAME: &'static str = "test";
    const REQUEST: DecodeDepth = DecodeDepth::Geometry;
    const REUSES_SOURCE: bool = false;
    type Target = DialectFree;
    const TARGET: DialectFree = DialectFree;

    fn plan_resolved(
        &self,
        _: &DecodeContext<'_>,
        input: EncodeInput<'_>,
        (): (),
    ) -> Result<ExportBody, CodecError> {
        Ok(ExportBody::synthesized(
            Vec::new(),
            input.ir,
            ArenaCoverage::Complete,
            NativeDispositions::written(),
        ))
    }
}

#[test]
fn geometry_output_reports_stale_current_state() {
    use crate::design::evaluation::{CurrentState, EvaluationStatus};
    use crate::design::refs::{Member, Position};
    use crate::design::scope::DesignScope;
    use std::collections::{BTreeMap, BTreeSet};

    let feature = crate::features::FeatureId::mint("test:design:feature#stale").unwrap();
    let mut ir = unit_cube().unwrap();
    let body = ir.geometry.bodies[0].id.clone();
    ir.design.scopes.push(DesignScope {
        definition: crate::ids::ProductDefinitionId::mint("test:model:product-definition#one")
            .unwrap(),
        roots: Vec::new(),
        active_configuration: None,
        active_arrangements: BTreeSet::new(),
        occurrences: BTreeMap::new(),
        unproduced: BTreeSet::new(),
        current: vec![CurrentState {
            at: Position::After(Member::Feature(feature)),
            bodies: vec![body],
            poses: BTreeMap::new(),
            status: EvaluationStatus::Stale,
            native_ref: None,
        }],
    });
    let plan = GeometryWriter.plan(EncodeInput::new(&ir, None)).unwrap();
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| code_name(&loss.code) == "export:evaluation.stale"
            && loss.message.contains("test:design:feature#stale")));
    let strict = Strictness::default().with_code("export:evaluation.stale");
    assert!(matches!(
        GeometryWriter.plan(EncodeInput {
            strictness: &strict,
            ..EncodeInput::new(&ir, None)
        }),
        Err(EncodeError::StrictRejected(_))
    ));
    assert!(!CadirEncoder
        .plan(EncodeInput::new(&ir, None))
        .unwrap()
        .report()
        .losses
        .iter()
        .any(|loss| code_name(&loss.code) == "export:evaluation.stale"));
    ir.design.scopes[0].current[0].status = EvaluationStatus::Failed;
    assert!(GeometryWriter
        .plan(EncodeInput::new(&ir, None))
        .unwrap()
        .report()
        .losses
        .iter()
        .any(|loss| code_name(&loss.code) == "export:evaluation.stale"));
    ir.design.scopes[0].current[0].status = EvaluationStatus::Current;
    assert!(!GeometryWriter
        .plan(EncodeInput::new(&ir, None))
        .unwrap()
        .report()
        .losses
        .iter()
        .any(|loss| code_name(&loss.code) == "export:evaluation.stale"));
}
