// SPDX-License-Identifier: Apache-2.0
//! Depth harness: a geometry-depth decode agrees with the full decode on
//! everything a geometry encoder reads.
//!
//! [`assert_geometry_agrees_with_full`] decodes every input at
//! [`DecodeDepth::Full`] and at [`DecodeDepth::Geometry`] through the public
//! [`Codec::decode`] path and asserts that:
//!
//! - each document holds every populated arena class;
//! - missing requested classes have a `depth.not-delivered` loss;
//! - the documents agree on every model arena outside the design class, on
//!   units, on tolerances, and on source identity;
//! - every validation finding over the geometry document, neutral and
//!   codec-native, is also a finding over the full one, so each of its
//!   references, `native_ref` included, resolves wherever the full one's does;
//! - the geometry decode's loss codes are a subset of the full decode's.
//!
//! The comparison is exact. Both decodes run in one process over the same
//! bytes, so a cut that leaves geometry untouched reproduces it bit for bit.
//! Native namespaces are not compared: a cut may omit typed native records
//! produced only during design projection. Model PMI is compared through its
//! resolved canonical nominal; attributes owned by design entities are outside
//! the geometry projection. Every document source attribute remains content.
//!
//! A cut is sound because source records absent from every typed arena are
//! declared in `unrepresented`. Patch and replay cannot consume such a source;
//! synthesis writes only content the document holds. The full decode's
//! unrepresented classes must therefore be a subset of the geometry decode's.
//! A codec may overdeliver design for a geometry request; [`DepthSummary::cut`]
//! counts the inputs for which it delivers model scope without design.

use std::collections::BTreeSet;
use std::io::Cursor;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodeDepth, DecodePolicy};
use cadmpeg_ir::codec::{Codec, DecodeFailure, DecodeOptions, DecodeResult};
use cadmpeg_ir::validate::validate_neutral;
use cadmpeg_ir::{ArenaClass, CadIr};
use serde_json::Value;

/// Outcome counts of one depth-harness run.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DepthSummary {
    /// Inputs compared.
    pub inputs: usize,
    /// Inputs whose geometry request delivered model scope without design.
    pub cut: usize,
    /// Inputs both decodes refused with the same message.
    pub refused: usize,
}

/// Decodes every input at full and at geometry depth and asserts the two agree
/// outside the design arenas.
///
/// `inputs` are `(name, bytes)` pairs, as [`crate::golden::Harness`] yields
/// them. Every divergence is collected before the assertion, so one run names
/// every input that fails.
///
/// # Panics
///
/// Panics when `inputs` is empty, or when any input violates one of the checks
/// in the module documentation.
pub fn assert_geometry_agrees_with_full<N: AsRef<str>>(
    codec: &dyn Codec,
    inputs: &[(N, Vec<u8>)],
) -> DepthSummary {
    assert_geometry_agrees_with_full_with(codec, inputs, |_| {})
}

/// Compare both decode requests and check each successful result with the
/// codec's own loss-classification assertions, without decoding inputs again.
pub fn assert_geometry_agrees_with_full_with<N: AsRef<str>>(
    codec: &dyn Codec,
    inputs: &[(N, Vec<u8>)],
    check: impl Fn(&DecodeResult),
) -> DepthSummary {
    assert!(
        !inputs.is_empty(),
        "no input to compare; the depth harness would pass vacuously"
    );
    let mut summary = DepthSummary::default();
    let mut failures = Vec::new();
    for (name, bytes) in inputs {
        let name = name.as_ref();
        summary.inputs += 1;
        match (
            decode_at_depth(codec, bytes, DecodeDepth::Full),
            decode_at_depth(codec, bytes, DecodeDepth::Geometry),
        ) {
            (Ok(full), Ok(geometry)) => {
                check(&full);
                check(&geometry);
                if geometry.ir().scope == cadmpeg_ir::ContentScope::geometry() {
                    summary.cut += 1;
                }
                failures.extend(
                    compare(codec, bytes, &full, &geometry)
                        .into_iter()
                        .map(|failure| format!("{name}: {failure}")),
                );
            }
            (Err(full), Err(geometry)) if full.to_string() == geometry.to_string() => {
                summary.refused += 1;
            }
            (Err(full), Err(geometry)) => failures.push(format!(
                "{name}: the decodes refused differently: full `{full}`, geometry `{geometry}`"
            )),
            (Ok(_), Err(geometry)) => failures.push(format!(
                "{name}: only the geometry decode refused: {geometry}"
            )),
            (Err(full), Ok(_)) => {
                failures.push(format!("{name}: only the full decode refused: {full}"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} input(s) diverge between full and geometry decodes:\n{}",
        failures.len(),
        inputs.len(),
        failures.join("\n")
    );
    summary
}

fn decode_at_depth(
    codec: &dyn Codec,
    bytes: &[u8],
    depth: DecodeDepth,
) -> Result<DecodeResult, DecodeFailure> {
    crate::golden::decode(
        codec,
        &mut Cursor::new(bytes),
        &DecodeOptions {
            depth,
            ..DecodeOptions::default()
        },
    )
}

fn compare(
    codec: &dyn Codec,
    bytes: &[u8],
    full: &DecodeResult,
    geometry: &DecodeResult,
) -> Vec<String> {
    let mut failures = Vec::new();
    for (requested, result) in [(DecodeDepth::Full, full), (DecodeDepth::Geometry, geometry)] {
        if let Err(error) = result.ir().check_scope() {
            failures.push(error.to_string());
        }
        if result.ir().scope != result.report().scope() {
            failures.push("document and report scopes differ".into());
        }
        let required = requested.required_scope();
        let expected = usize::from(!result.ir().scope.missing_from(required).is_empty());
        let actual = result
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code.to_string() == "decode/depth.not-delivered")
            .count();
        if actual != expected {
            failures.push(format!(
                "a {requested} request requires {expected} missing-class loss, found {actual}"
            ));
        }
    }
    if !full
        .source_fidelity()
        .unrepresented()
        .iter()
        .all(|class| geometry.source_fidelity().unrepresented().contains(class))
    {
        failures.push(
            "the full decode has unrepresented classes absent from the geometry decode".into(),
        );
    }
    if let Some(path) = first_difference(&outside_design(full.ir()), &outside_design(geometry.ir()))
    {
        failures.push(format!(
            "the documents differ outside the design arenas at `{path}`"
        ));
    }
    match (
        findings(codec, bytes, full.ir()),
        findings(codec, bytes, geometry.ir()),
    ) {
        (Ok(full), Ok(geometry)) => {
            let extra = geometry.difference(&full).cloned().collect::<Vec<_>>();
            if !extra.is_empty() {
                failures.push(format!(
                    "validation finds over the geometry document what it does not over the full one: {}",
                    extra.join("; ")
                ));
            }
        }
        (Err(error), _) | (_, Err(error)) => {
            failures.push(format!("validation refused: {error}"));
        }
    }
    let full_codes = loss_codes(full);
    let extra = loss_codes(geometry)
        .difference(&full_codes)
        .cloned()
        .collect::<Vec<_>>();
    if !extra.is_empty() {
        failures.push(format!(
            "the geometry decode reports loss codes the full decode does not: {}",
            extra.join(", ")
        ));
    }
    failures
}

/// Model projection with design-owned references closed before design is removed.
fn outside_design(ir: &CadIr) -> Value {
    crate::contract::neutral_view(ir, &[ArenaClass::Design])
}

/// Every neutral and codec-native finding over `ir`, as `check: message`.
fn findings(
    codec: &dyn Codec,
    bytes: &[u8],
    ir: &CadIr,
) -> Result<BTreeSet<String>, cadmpeg_core::CodecError> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::default())?;
    let mut findings = validate_neutral(ir, Vec::new())?.findings;
    findings.extend(codec.validate_native(&ctx, ir, None)?);
    Ok(findings
        .into_iter()
        .map(|finding| format!("{:?}: {}", finding.check, finding.message))
        .collect())
}

fn loss_codes(result: &DecodeResult) -> BTreeSet<String> {
    result
        .report()
        .losses
        .iter()
        .map(|loss| loss.code.to_string())
        .collect()
}

/// The JSON pointer of the first place `left` and `right` differ.
fn first_difference(left: &Value, right: &Value) -> Option<String> {
    match (left, right) {
        (Value::Object(left), Value::Object(right)) => {
            let keys = left.keys().chain(right.keys()).collect::<BTreeSet<_>>();
            keys.into_iter()
                .find_map(|key| match (left.get(key), right.get(key)) {
                    (Some(left), Some(right)) => {
                        first_difference(left, right).map(|path| format!("/{key}{path}"))
                    }
                    _ => Some(format!("/{key}")),
                })
        }
        (Value::Array(left), Value::Array(right)) => left
            .iter()
            .zip(right)
            .enumerate()
            .find_map(|(index, (left, right))| {
                first_difference(left, right).map(|path| format!("/{index}{path}"))
            })
            .or_else(|| {
                (left.len() != right.len()).then(|| format!("/{}", left.len().min(right.len())))
            }),
        _ => (left != right).then(String::new),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use cadmpeg_core::decode::{DecodeContext, DecodeDepth, View};
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::codec::{CodecBackend, Confidence, DecodeBody, Decoded, FormatId};
    use cadmpeg_ir::examples::{directed_subd_sum, unit_cube};
    use cadmpeg_ir::features::{DesignParameter, ParameterId};
    use cadmpeg_ir::report::decode::DecodeTransfer;
    use cadmpeg_ir::report::loss::{LossKind, LossNote, LossTaxonomy};
    use cadmpeg_ir::{CadIr, ContainerSummary, SourceFidelity};

    use super::{assert_geometry_agrees_with_full, DepthSummary};

    /// A backend whose decode at each requested depth is scripted by the test.
    struct ScriptedCodec(fn(DecodeDepth) -> Decoded);

    impl CodecBackend for ScriptedCodec {
        const FORMAT: FormatId = FormatId::new("test");

        fn detect_impl(
            &self,
            _ctx: &DecodeContext<'_>,
            _prefix: View<'_>,
        ) -> Result<Confidence, CodecError> {
            Ok(Confidence::No)
        }

        fn inspect_impl(
            &self,
            _ctx: &DecodeContext<'_>,
            _root: View<'_>,
        ) -> Result<ContainerSummary, CodecError> {
            panic!("the depth harness does not inspect")
        }

        fn decode_impl(
            &self,
            ctx: &DecodeContext<'_>,
            _root: View<'_>,
        ) -> Result<Decoded, CodecError> {
            Ok((self.0)(ctx.depth()))
        }
    }

    fn decoded(mut ir: CadIr, transfer: DecodeTransfer) -> Decoded {
        ir.set_scope(transfer.scope)
            .expect("scripted scope agrees with its arenas");
        Decoded {
            ir,
            body: DecodeBody::new(transfer),
            source_fidelity: SourceFidelity::default(),
        }
    }

    fn cube() -> CadIr {
        unit_cube().expect("valid unit cube fixture")
    }

    fn cube_with_parameter() -> CadIr {
        let mut ir = cube();
        ir.model.parameters.push(DesignParameter {
            id: ParameterId::mint("synthetic:test:parameter#depth").expect("valid parameter id"),
            owner: None,
            ordinal: 0,
            name: "D1".into(),
            expression: "1mm".into(),
            display: None,
            value: None,
            driving: true,
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: None,
        });
        ir
    }

    /// Full decodes carry a design parameter; geometry decodes cut it.
    fn sound_cut(depth: DecodeDepth) -> Decoded {
        if depth < DecodeDepth::Full {
            decoded(
                cube(),
                DecodeTransfer {
                    scope: cadmpeg_ir::ContentScope::geometry(),
                    geometry_transferred: true,
                },
            )
        } else {
            decoded(
                cube_with_parameter(),
                DecodeTransfer {
                    scope: cadmpeg_ir::ContentScope::full(),
                    geometry_transferred: true,
                },
            )
        }
    }

    fn inputs() -> [(&'static str, Vec<u8>); 1] {
        [("scripted", vec![1, 2, 3, 4])]
    }

    #[test]
    fn a_cut_that_omits_only_design_records_agrees() {
        assert_eq!(
            assert_geometry_agrees_with_full(&ScriptedCodec(sound_cut), &inputs()),
            DepthSummary {
                inputs: 1,
                cut: 1,
                refused: 0,
            }
        );
    }

    #[test]
    #[should_panic(expected = "the documents differ outside the design arenas at `/model")]
    fn a_cut_that_changes_a_model_arena_diverges() {
        fn changes_geometry(depth: DecodeDepth) -> Decoded {
            if depth < DecodeDepth::Full {
                decoded(
                    directed_subd_sum().expect("valid subd fixture"),
                    DecodeTransfer {
                        scope: cadmpeg_ir::ContentScope::geometry(),
                        geometry_transferred: true,
                    },
                )
            } else {
                sound_cut(depth)
            }
        }
        assert_geometry_agrees_with_full(&ScriptedCodec(changes_geometry), &inputs());
    }

    #[test]
    #[should_panic(expected = "reports loss codes the full decode does not")]
    fn a_cut_that_adds_a_loss_diverges() {
        fn adds_a_loss(depth: DecodeDepth) -> Decoded {
            let mut decoded = sound_cut(depth);
            if depth < DecodeDepth::Full {
                decoded.body.losses.push(LossNote::new(
                    LossKind::shared(LossTaxonomy::TopologyNotTransferred),
                    "scripted",
                ));
            }
            decoded
        }
        assert_geometry_agrees_with_full(&ScriptedCodec(adds_a_loss), &inputs());
    }

    #[test]
    #[should_panic(expected = "the documents differ outside the design arenas")]
    fn an_under_delivery_diverges() {
        fn delivers_container(depth: DecodeDepth) -> Decoded {
            if depth < DecodeDepth::Full {
                decoded(
                    CadIr::empty(),
                    DecodeTransfer {
                        scope: cadmpeg_ir::ContentScope::container(),
                        geometry_transferred: false,
                    },
                )
            } else {
                sound_cut(depth)
            }
        }
        assert_geometry_agrees_with_full(&ScriptedCodec(delivers_container), &inputs());
    }
}
