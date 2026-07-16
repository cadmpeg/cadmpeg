// SPDX-License-Identifier: Apache-2.0
//! Shared sweep dimensions: operations, versioned policy profiles, and the
//! classified result of one operation.
//!
//! These are the stable string-keyed dimensions of every baseline entry. The
//! labels are the wire format between the parent driver and the child runner
//! and the on-disk baseline key, so they change only with a deliberate baseline
//! re-bless.

use cadmpeg_ir::{Confidence, DecodePolicy};

/// Version tag for the acceptance envelope recorded alongside baselines, per
/// the versioned-profile requirement (`envelope-v2`, the Phase 2 freeze).
pub const ENVELOPE_VERSION: &str = "envelope-v2";

/// One decode-platform entry point exercised by the sweep.
///
/// The four operations are distinct oracle surfaces: detection reads only a
/// prefix, inspection walks the container directory, container-only decode
/// stops at the container layer, and full decode drives entity decode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Operation {
    /// [`Codec::detect`](cadmpeg_ir::Codec::detect) over a byte prefix.
    Detect,
    /// [`CodecEntry::inspect`](cadmpeg_ir::CodecEntry::inspect) over a seekable reader.
    Inspect,
    /// [`CodecEntry::decode`](cadmpeg_ir::CodecEntry::decode) with `container_only` set.
    ContainerOnly,
    /// [`CodecEntry::decode`](cadmpeg_ir::CodecEntry::decode) driving full entity decode.
    FullDecode,
}

impl Operation {
    /// Every operation, in baseline-key order.
    pub const ALL: [Operation; 4] = [
        Operation::Detect,
        Operation::Inspect,
        Operation::ContainerOnly,
        Operation::FullDecode,
    ];

    /// The stable wire/baseline label.
    pub fn id(self) -> &'static str {
        match self {
            Operation::Detect => "detect",
            Operation::Inspect => "inspect",
            Operation::ContainerOnly => "container-only",
            Operation::FullDecode => "full-decode",
        }
    }

    /// Parse a label produced by [`Operation::id`].
    pub fn from_id(label: &str) -> Option<Operation> {
        Operation::ALL.into_iter().find(|op| op.id() == label)
    }
}

/// A versioned decode policy profile.
///
/// The version suffix is part of the baseline key so a profile retune shows up
/// as a new key rather than a silent baseline shift, per the versioned-profile
/// requirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PolicyProfile {
    /// Generous interactive ceilings — the platform default (`desktop-v1`).
    DesktopV1,
    /// Tight unattended-service ceilings (`service-v1`).
    ServiceV1,
}

impl PolicyProfile {
    /// Every profile, in baseline-key order.
    pub const ALL: [PolicyProfile; 2] = [PolicyProfile::DesktopV1, PolicyProfile::ServiceV1];

    /// The stable wire/baseline label.
    pub fn id(self) -> &'static str {
        match self {
            PolicyProfile::DesktopV1 => "desktop-v1",
            PolicyProfile::ServiceV1 => "service-v1",
        }
    }

    /// Parse a label produced by [`PolicyProfile::id`].
    pub fn from_id(label: &str) -> Option<PolicyProfile> {
        PolicyProfile::ALL.into_iter().find(|p| p.id() == label)
    }

    /// The concrete policy this profile resolves to.
    pub fn policy(self) -> DecodePolicy {
        match self {
            PolicyProfile::DesktopV1 => DecodePolicy::desktop(),
            PolicyProfile::ServiceV1 => DecodePolicy::service(),
        }
    }
}

/// The classified outcome of one operation.
///
/// Detection cannot fail, so it classifies by [`Confidence`]; inspection and
/// decode classify `Ok` or the [`CodecError`](cadmpeg_ir::CodecError) variant.
/// This is recorded beside the four stage-1 oracles. It is not one of those
/// four falsifiable-property oracles, but the regression check gates it as a
/// ratchet dimension: any divergence from the blessed class is flagged, so a
/// codec silently switching a fixture between `ok` and an error class fails the
/// gate until re-blessed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultClass {
    /// The operation produced a value.
    Ok,
    /// Detection: not this format.
    DetectNo,
    /// Detection: weak signal.
    DetectLow,
    /// Detection: plausible.
    DetectMedium,
    /// Detection: strong signal.
    DetectHigh,
    /// `WrongFormat`.
    WrongFormat,
    /// `Malformed`.
    Malformed,
    /// `Truncated`.
    Truncated,
    /// `ResourceLimit`.
    ResourceLimit,
    /// `NotImplemented`.
    NotImplemented,
    /// `Io`.
    Io,
    /// A `#[non_exhaustive]` variant added after this harness was written.
    Other,
}

impl ResultClass {
    /// The stable wire/baseline label.
    pub fn label(self) -> &'static str {
        match self {
            ResultClass::Ok => "ok",
            ResultClass::DetectNo => "detect_no",
            ResultClass::DetectLow => "detect_low",
            ResultClass::DetectMedium => "detect_medium",
            ResultClass::DetectHigh => "detect_high",
            ResultClass::WrongFormat => "wrong_format",
            ResultClass::Malformed => "malformed",
            ResultClass::Truncated => "truncated",
            ResultClass::ResourceLimit => "resource_limit",
            ResultClass::NotImplemented => "not_implemented",
            ResultClass::Io => "io",
            ResultClass::Other => "other",
        }
    }

    /// Classify a detection [`Confidence`].
    pub fn from_confidence(confidence: Confidence) -> ResultClass {
        match confidence {
            Confidence::No => ResultClass::DetectNo,
            Confidence::Low => ResultClass::DetectLow,
            Confidence::Medium => ResultClass::DetectMedium,
            Confidence::High => ResultClass::DetectHigh,
        }
    }
}
