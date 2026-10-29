// SPDX-License-Identifier: Apache-2.0
//! Caller limits and source-reuse declarations survive the sealed wrapper.

use super::{
    coverage_losses, uniform_dispositions, ArenaCoverage, ArenaDisposition, CadIr, CodecError,
    DecodeDepth, DialectFree, EncodeInput, Encoder, EncoderBackend, ExportBody, FormatId,
    SourceFidelity, TargetRequest,
};
use crate::codec::write::SynthesisCause;
use crate::schema::EntitySchema;
use cadmpeg_core::decode::ResourceLimits;

struct InputProbe<const REUSES: bool> {
    limits: ResourceLimits,
    fidelity_visible: bool,
}

impl<const REUSES: bool> EncoderBackend for InputProbe<REUSES> {
    const FORMAT: FormatId = FormatId::new("test");
    const REQUEST: DecodeDepth = DecodeDepth::Full;
    const REUSES_SOURCE: bool = REUSES;
    type Target = DialectFree;
    const TARGET: DialectFree = DialectFree;

    fn plan_resolved(&self, input: EncodeInput<'_>, (): ()) -> Result<ExportBody, CodecError> {
        assert_eq!(
            input.limits, self.limits,
            "the wrapper forwards all caller ceilings"
        );
        assert_eq!(input.fidelity.is_some(), self.fidelity_visible);
        Ok(ExportBody::synthesized(
            Vec::new(),
            input.ir,
            ArenaCoverage::Complete,
        ))
    }
}

fn probe<const REUSES: bool>(
    ir: &CadIr,
    fidelity: &SourceFidelity,
    expected_visible: bool,
) -> crate::codec::write::ExportPlan {
    let limits = ResourceLimits {
        max_input_bytes: 1,
        max_decompressed_bytes_total: 2,
        max_decompressed_bytes_per_expand: 3,
        max_retained_bytes: 4,
        max_entities: 0,
        max_recursion_depth: 5,
    };
    let mut input = EncodeInput::new(ir, Some(fidelity));
    assert_eq!(input.limits, ResourceLimits::desktop());
    input.limits = limits;
    InputProbe::<REUSES> {
        limits,
        fidelity_visible: expected_visible,
    }
    .plan(input, TargetRequest::Inherit)
    .expect("the empty probe writes no records")
}

#[test]
fn fidelity_filter_preserves_caller_limits_and_only_reusing_backends_stamp_a_cause() {
    let ir = CadIr::empty();
    let mut fidelity = SourceFidelity::default();
    let classes = crate::ArenaClasses::from([crate::ArenaClass::Model, crate::ArenaClass::Design]);
    fidelity
        .insert_retained_record(
            crate::ids::UnknownId::mint("test:source:record#image")
                .expect("source record identity"),
            crate::RetainedSourceRecord::whole("", b"unread model and design".to_vec())
                .with_untyped(classes.clone()),
        )
        .expect("unique source record");
    let reusing = probe::<true>(&ir, &fidelity, false);
    assert_eq!(
        reusing.report().synthesis_cause(),
        Some(&SynthesisCause::SourceUnrepresented { classes })
    );
    let synthesizing = probe::<false>(&ir, &fidelity, false);
    assert!(synthesizing.report().synthesis_cause().is_none());
}

#[test]
fn represented_fidelity_remains_visible_without_a_source_unrepresented_cause() {
    let ir = CadIr::empty();
    let fidelity = SourceFidelity::default();
    for plan in [
        probe::<true>(&ir, &fidelity, true),
        probe::<false>(&ir, &fidelity, true),
    ] {
        assert!(plan.report().synthesis_cause().is_none());
    }
}

#[test]
fn neutral_omission_lists_every_original_id_in_order_after_the_count() {
    let mut ir = crate::examples::unit_cube().expect("valid unit cube fixture");
    let mut expected: Vec<_> = ir
        .model
        .faces
        .iter()
        .map(|face| face.identity().to_owned())
        .collect();
    expected.sort();
    ir.model.faces.reverse();
    let mut dispositions = uniform_dispositions(ArenaDisposition::Written);
    dispositions.faces = ArenaDisposition::Omitted;
    let losses = coverage_losses(&ir, ArenaCoverage::Declared(dispositions));
    assert_eq!(losses.len(), 1);
    assert_eq!(expected.len(), 6);
    assert_eq!(
        losses[0].message,
        format!(
            "the cadir write does not represent model arena `faces`: 6 record(s) omitted: {}",
            expected.join(", ")
        )
    );
    assert!(ir
        .model
        .edges
        .iter()
        .all(|edge| !losses[0].message.contains(edge.identity())));
}

struct SourceReuseProbe {
    write_path: crate::codec::write::WritePath,
}

impl EncoderBackend for SourceReuseProbe {
    const FORMAT: FormatId = FormatId::new("test");
    const REQUEST: DecodeDepth = DecodeDepth::Full;
    const REUSES_SOURCE: bool = true;
    type Target = DialectFree;
    const TARGET: DialectFree = DialectFree;

    fn plan_resolved(&self, input: EncodeInput<'_>, (): ()) -> Result<ExportBody, CodecError> {
        let mut body = ExportBody::synthesized(vec![42], input.ir, ArenaCoverage::Complete);
        body.write_path = self.write_path.clone();
        Ok(body)
    }
}

fn source_reuse_paths() -> [crate::codec::write::WritePath; 2] {
    use crate::codec::write::{PatchConsumption, WritePath};
    [
        WritePath::VerbatimReplay,
        WritePath::Patched {
            consumption: PatchConsumption::Replayed,
        },
    ]
}

#[test]
fn source_reuse_with_supplied_fidelity_keeps_the_computed_path_and_payload() {
    let ir = CadIr::empty();
    let fidelity = SourceFidelity::default();
    for write_path in source_reuse_paths() {
        let expected = write_path.clone().into_report(true);
        let plan = SourceReuseProbe { write_path }
            .plan(
                EncodeInput::new(&ir, Some(&fidelity)),
                TargetRequest::Inherit,
            )
            .expect("source fidelity satisfies the computed reuse invariant");
        assert_eq!(plan.report().write_path(), &expected);
        let mut output = Vec::new();
        plan.write_to(&mut output).unwrap();
        assert_eq!(output, [42]);
    }
}

#[test]
fn source_reuse_without_offered_fidelity_is_a_debug_invariant_and_release_continues() {
    let ir = CadIr::empty();
    for write_path in source_reuse_paths() {
        let expected = write_path.clone().into_report(false);
        let backend = SourceReuseProbe { write_path };
        let result = std::panic::catch_unwind(|| {
            backend.plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        });
        if cfg!(debug_assertions) {
            let payload = result.expect_err("a computed reuse invariant fails in debug builds");
            let message = payload
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| payload.downcast_ref::<&str>().copied())
                .expect("the invariant has a message");
            assert!(
                message.contains("encoder test reported source-byte reuse without source fidelity")
            );
        } else {
            let plan = result
                .expect("a computed invariant does not panic in release builds")
                .expect("a computed invariant does not construct an encode error");
            assert_eq!(plan.report().write_path(), &expected);
            let mut output = Vec::new();
            plan.write_to(&mut output).unwrap();
            assert_eq!(output, [42]);
        }
    }
}
