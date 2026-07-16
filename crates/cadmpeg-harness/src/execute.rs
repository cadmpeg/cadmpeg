// SPDX-License-Identifier: Apache-2.0
//! In-child execution of one operation, run twice for determinism.
//!
//! This module holds no isolation machinery: it is the pure work the
//! `harness-runner` binary performs after installing its peak-tracking
//! allocator. Keeping it in the library lets the parent driver share the
//! [`RunnerOutcome`] wire type and lets the logic be unit-tested in-process.

use std::io::Cursor;

use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::{Codec, CodecError, DecodeOptions, DecodeResult, InspectOptions};
use serde::{Deserialize, Serialize};

use crate::model::{Operation, PolicyProfile, ResultClass};

/// Every codec id the harness can dispatch, in baseline-key order.
pub const CODEC_IDS: &[&str] = &["f3d", "sldprt", "catia", "creo", "nx", "rhino"];

/// The machine-readable result the child writes to stdout.
///
/// The parent driver fills the oracle verdicts around this; the child reports
/// only what it can measure from inside its own process. The determinism
/// comparison runs entirely in-child (both runs share the process), so only
/// its verdict crosses the pipe, not the per-run digests it compares.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunnerOutcome {
    /// Classified result label (see [`ResultClass::label`]).
    pub result_class: String,
    /// Whether the second run produced an identical class and digest.
    pub determinism_ok: bool,
    /// Peak process allocation observed by the runner's global allocator.
    pub peak_alloc_bytes: u64,
}

/// The in-process outcome before the allocator peak is attached.
#[derive(Debug, Clone)]
pub struct ExecOutcome {
    /// Classified result of the operation.
    pub result_class: ResultClass,
    /// Whether the two runs agreed on class and digest.
    pub determinism_ok: bool,
}

/// Construct a codec by id, or `None` for an unknown id.
fn codec_for(id: &str) -> Option<Box<dyn Codec>> {
    Some(match id {
        "f3d" => Box::new(cadmpeg_codec_f3d::F3dCodec),
        "sldprt" => Box::new(cadmpeg_codec_sldprt::SldprtCodec),
        "catia" => Box::new(cadmpeg_codec_catia::CatiaCodec),
        "creo" => Box::new(cadmpeg_codec_creo::CreoCodec),
        "nx" => Box::new(cadmpeg_codec_nx::NxCodec),
        "rhino" => Box::new(cadmpeg_codec_rhino::RhinoCodec),
        _ => return None,
    })
}

/// Classify a codec error, tolerating future `#[non_exhaustive]` variants.
fn classify_error(error: &CodecError) -> ResultClass {
    match error {
        CodecError::WrongFormat(_) => ResultClass::WrongFormat,
        CodecError::Malformed(_) => ResultClass::Malformed,
        CodecError::Truncated { .. } => ResultClass::Truncated,
        CodecError::ResourceLimit(_) => ResultClass::ResourceLimit,
        CodecError::NotImplemented(_) => ResultClass::NotImplemented,
        CodecError::Io(_) => ResultClass::Io,
        _ => ResultClass::Other,
    }
}

/// The canonical digest of a successful decode: IR JSON plus the report, so
/// determinism covers geometry, notes, and losses together.
fn decode_digest(result: &DecodeResult) -> String {
    let ir_json = result
        .ir
        .to_canonical_json()
        .unwrap_or_else(|error| format!("ir-json-error:{error}"));
    let report_json = serde_json::to_string(&result.report)
        .unwrap_or_else(|error| format!("report-json-error:{error}"));
    let mut buf = ir_json;
    buf.push('\n');
    buf.push_str(&report_json);
    sha256_hex(buf.as_bytes())
}

/// The result of one single run: enough to classify and to compare for
/// determinism. The digest is compared against the sibling run in-process and
/// is never transmitted.
struct RunOnce {
    class: ResultClass,
    digest: String,
}

/// Perform one operation once against `bytes`.
fn run_once(codec_id: &str, op: Operation, profile: PolicyProfile, bytes: &[u8]) -> RunOnce {
    let Some(codec) = codec_for(codec_id) else {
        return RunOnce {
            class: ResultClass::Other,
            digest: sha256_hex(b"unknown-codec"),
        };
    };

    match op {
        Operation::Detect => {
            let confidence = codec.detect(bytes);
            RunOnce {
                class: ResultClass::from_confidence(confidence),
                digest: sha256_hex(format!("detect:{confidence}").as_bytes()),
            }
        }
        Operation::Inspect => {
            let options = InspectOptions {
                limits: profile.policy().limits,
            };
            let mut reader = Cursor::new(bytes.to_vec());
            match codec.inspect(&mut reader, &options) {
                Ok(summary) => {
                    let json = serde_json::to_string(&summary)
                        .unwrap_or_else(|error| format!("summary-json-error:{error}"));
                    RunOnce {
                        class: ResultClass::Ok,
                        digest: sha256_hex(json.as_bytes()),
                    }
                }
                Err(error) => error_run(&error),
            }
        }
        Operation::ContainerOnly | Operation::FullDecode => {
            let options = DecodeOptions {
                container_only: op == Operation::ContainerOnly,
                policy: profile.policy(),
            };
            let mut reader = Cursor::new(bytes.to_vec());
            match codec.decode(&mut reader, &options) {
                Ok(result) => RunOnce {
                    class: ResultClass::Ok,
                    digest: decode_digest(&result),
                },
                Err(error) => error_run(&error),
            }
        }
    }
}

/// Build a [`RunOnce`] from a codec error, digesting the deterministic label
/// and `Display` so an error path is compared for determinism like any other.
fn error_run(error: &CodecError) -> RunOnce {
    let class = classify_error(error);
    let digest = sha256_hex(format!("err:{}:{error}", class.label()).as_bytes());
    RunOnce { class, digest }
}

/// Run `op` twice and report the classified result and a determinism verdict.
///
/// Two runs share the process, so the caller's peak-allocation measurement
/// spans both; the runs are near-identical, so the observed peak matches a
/// single run's peak.
pub fn execute(codec_id: &str, op: Operation, profile: PolicyProfile, bytes: &[u8]) -> ExecOutcome {
    let first = run_once(codec_id, op, profile, bytes);
    let second = run_once(codec_id, op, profile, bytes);
    let determinism_ok = first.class == second.class && first.digest == second.digest;
    ExecOutcome {
        result_class: first.class,
        determinism_ok,
    }
}
