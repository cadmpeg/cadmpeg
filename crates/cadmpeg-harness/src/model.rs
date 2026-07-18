// SPDX-License-Identifier: Apache-2.0
//! Shared sweep dimensions: operations, versioned policy profiles, and the
//! classified result of one operation.
//!
//! The labels are the wire format between the parent driver and child runner.

use cadmpeg_ir::{Confidence, DecodePolicy};

/// One decode-platform entry point exercised by the sweep.
///
/// The four operations exercise distinct surfaces: detection reads only a
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
    /// Every operation, in wire order.
    pub const ALL: [Operation; 4] = [
        Operation::Detect,
        Operation::Inspect,
        Operation::ContainerOnly,
        Operation::FullDecode,
    ];

    /// The stable wire label.
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PolicyProfile {
    /// Generous interactive ceilings — the platform default (`desktop-v1`).
    DesktopV1,
    /// Tight unattended-service ceilings (`service-v1`).
    ServiceV1,
}

impl PolicyProfile {
    /// Every profile, in wire order.
    pub const ALL: [PolicyProfile; 2] = [PolicyProfile::DesktopV1, PolicyProfile::ServiceV1];

    /// The stable wire label.
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
/// This is reported beside the four subprocess safety checks.
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
    /// The stable wire label.
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
