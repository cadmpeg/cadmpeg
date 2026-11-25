// SPDX-License-Identifier: Apache-2.0
//! Shared synthetic `FCStd` archive fixtures for crate tests.

pub(crate) mod test_archive;

pub(crate) fn assert_retained_refusal_at<T>(
    input: &[u8],
    operation: &str,
    decode: impl Fn(&cadmpeg_core::decode::DecodeContext<'_>) -> Result<T, cadmpeg_core::CodecError>,
) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes,
        operation,
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::default();
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(input, &arena, &policy)
                .expect("test input is within the root limit");
            decode(&ctx)
        },
    );
}

pub(crate) fn validate_native(
    ir: &cadmpeg_ir::document::CadIr,
) -> Vec<cadmpeg_ir::report::check::Finding> {
    let bytes = serde_json::to_vec(ir).expect("test document serializes");
    with_service_context(&bytes, |ctx| {
        crate::validate_native(ctx, ir).expect("validate the native records")
    })
}

pub(crate) fn with_service_context<T>(
    input: &[u8],
    use_context: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T,
) -> T {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(input, &arena, &policy)
        .expect("test input is within service limits");
    use_context(&ctx)
}

/// Builds a checked entry with its content digest for synthetic fixtures.
pub(crate) fn entry_record(
    id: String,
    name: String,
    role: cadmpeg_core::container::ContainerRole,
    references: Vec<String>,
    data: Vec<u8>,
) -> crate::native::EntryRecord {
    crate::native::EntryRecord::new(id, name, role, references, data).expect("valid entry fixture")
}

pub(crate) fn non_placement_losses(
    report: &cadmpeg_ir::report::decode::DecodeReport,
) -> Vec<&cadmpeg_ir::report::loss::LossNote> {
    report
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() != "feature.primitive-placement-unresolved")
        .collect()
}

/// `ir` with its geometry replaced by the state that object `object` stores
/// (§5.4): the state after its feature, which an evaluation or the current
/// state at the feature's position holds, or, with `tool`, the feature's
/// tool body.
pub(crate) fn state_of(
    ir: &cadmpeg_ir::CadIr,
    object: &str,
    tool: bool,
) -> Option<cadmpeg_ir::CadIr> {
    use cadmpeg_ir::design::refs::{Evaluated, Member, Position};
    let native = format!("fcstd:native:object#{object}");
    let feature = ir
        .design
        .features
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some(native.as_str()))?;
    let of = if tool {
        Evaluated::Tool(feature.id.clone())
    } else {
        Evaluated::State(Position::After(Member::Feature(feature.id.clone())))
    };
    let records = match ir
        .design
        .evaluations
        .iter()
        .find(|evaluation| evaluation.of == of)
    {
        Some(evaluation) => ir
            .resolved_evaluation_state(
                &evaluation.id,
                &cadmpeg_core::decode::ResourceLimits::default(),
            )
            .ok()?
            .records()
            .clone(),
        None => {
            let Evaluated::State(at) = of else {
                return None;
            };
            let scope = ir.design.scopes.first()?;
            ir.resolved_current_state(&scope.definition, &at)
                .ok()?
                .records()
                .clone()
        }
    };
    let mut state = ir.clone();
    state.geometry = records;
    Some(state)
}

/// The model that holds the result of legacy feature `feature` and that
/// result's bodies: a free feature's outputs in the current model, or the
/// state after a Body member ([`state_of`]).
pub(crate) fn result_of(
    ir: &cadmpeg_ir::CadIr,
    feature: &cadmpeg_ir::features::Feature,
) -> (cadmpeg_ir::CadIr, Vec<cadmpeg_ir::ids::BodyId>) {
    let outputs = feature.evaluation.outputs();
    if !outputs.is_empty() {
        return (ir.clone(), outputs.clone());
    }
    let object = feature
        .native_ref
        .as_deref()
        .and_then(|native| native.split_once('#'))
        .map_or("", |(_, object)| object);
    match state_of(ir, object, false) {
        Some(state) => {
            let bodies = state
                .geometry
                .bodies
                .iter()
                .map(|body| body.id.clone())
                .collect();
            (state, bodies)
        }
        None => (ir.clone(), Vec::new()),
    }
}
