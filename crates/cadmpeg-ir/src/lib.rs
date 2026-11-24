// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(test, allow(clippy::unwrap_used))]
//! Format-neutral CAD documents and the codec interfaces that produce them.
//!
//! [`CadIr`] stores units, tolerances, and flat entity arenas connected by
//! typed IDs. Its B-rep topology follows
//! `body → region → shell → face → loop → coedge → edge → vertex`; topology
//! references geometry carriers instead of nesting them. The document also
//! carries neutral construction features, tessellation, appearance, source
//! attributes, source-native namespaces, and uninterpreted [`UnknownRecord`]s.
//!
//! Document state machine: [`draft::ModelDraft`] commits into [`CadIr`];
//! [`validate_neutral()`] then yields a
//! [`report::check::ValidationReport`]. Decode produces [`DecodeResult`]
//! without embedding validation. Start a hand-built document with [`CadIr::empty`], populate its
//! arenas, call [`CadIr::finalize`] to establish canonical identity order, then
//! call [`validate_neutral()`]. Use [`CadIr::to_canonical_json`] (sorted view)
//! and [`CadIr::from_json`] for the versioned JSON form, and [`diff()`] for
//! identity-based structural comparison.
//!
//! Format crates implement [`CodecBackend`]; callers use the sealed [`Codec`]
//! entry points. Detection selects a codec from a byte prefix, inspection
//! enumerates a container, and decoding returns a [`DecodeResult`].
//! [`DecodeFailure`] separates backend [`cadmpeg_core::CodecError`] values from
//! strict-policy refusals that retain the completed report. A successful decode
//! reports partial transfer through [`report::decode::DecodeReport`] and
//! [`report::loss::LossNote`].
//!
//! [`Annotations`] records source locations and fidelity by globally unique
//! entity ID. An omitted exactness entry means byte-exact; explicit entries
//! distinguish derived, inferred, and unknown values. Native namespaces and
//! unknown records retain source-specific data outside the neutral model.
//!
//! Product components, occurrence instancing, and assembly joints have neutral arenas.
//! Product prototypes and occurrence trees retain assembly identity and
//! placement. Joint and mate constraints are reserved.

extern crate self as cadmpeg_ir;

#[macro_use]
mod identity_rewrite;

pub mod annotations;
pub mod appearance;
pub mod assets;
pub mod attributes;
pub mod bounds;
pub mod bytes;
pub mod codec;
pub mod compare;
pub mod container;

pub mod declared_measures;
pub mod design;
pub mod diff;
pub mod document;
pub mod draft;
pub mod drawings;
pub mod eval;
pub mod examples;
pub mod features;
pub mod geometry;
pub mod hash;
pub mod history_body;

pub mod ids;
pub mod index;
pub mod math;
pub mod native;
pub mod physics;
pub mod pmi;
pub mod point_cloud;
pub mod presentation;
pub mod process;
pub mod products;
mod provenance;
pub mod references;
pub mod report;
pub mod scalar;
pub mod schema;
pub mod semantic_annotations;
pub mod sketches;
pub mod source_fidelity;
pub mod spreadsheets;
pub mod structural_order;
pub mod subd;
pub mod tessellation;
pub mod topology;
pub mod transform;
pub mod units;
pub mod validate;

pub use annotations::{AnnotationBuilder, Annotations, ExactnessNote};
pub use cadmpeg_core::codec_id;
pub use codec::{Codec, CodecBackend, Confidence, DecodeFailure, DecodeOptions, DecodeResult};
pub use container::{ContainerKind, ContainerSummary};
pub use diff::{
    diff, ArenaDiff, ArenaKind, AttributeChange, DialectsChange, IrDiff, ModifiedEntity,
    NonEmptyFields, SourceDiff,
};
pub use document::{
    ArenaHome, ArenaName, CadIr, CensusKey, ScopeViolation, SourceMeta, IR_VERSION,
};
pub use draft::ModelDraft;
pub use features::{
    ConfigurationId, Feature, FeatureDefinition, FeatureId, FeatureOperation, LoftGuidance,
    ParameterId,
};
pub use ids::{is_valid_identity, IdentityError};
pub use native::{
    LossCount, Native, NativeConvertError, NativeField, NativeNamespace, NativeRecord,
};
pub use pmi::{
    DatumReference, DatumTargetForm, DimensionModifier, FitStandard, FitZone,
    GeometricToleranceKind, LimitsAndFits, PmiAnnotation, PmiDefinition, PmiDimension,
    PmiDimensionKind, PmiQuantity, PmiTarget, PmiValue,
};
pub use presentation::{
    CameraState, PresentationDocument, PresentationId, PresentationState, PresentationStateKind,
    ViewPresentation,
};
pub use presentation::{PresentationItem, PresentationLayer};
pub use products::{
    AssemblyJoint, ExternalDocument, InstanceBodies, JointConnector, JointId, JointLimits,
    JointOperand, JointOperands, Occurrence, OperandContainer, PairedJointKind, PlaneSense,
    ProductDefinition, ProductDefinitionKind, Storage, TangentContact,
};
/// Source location attached to a [`report::loss::LossNote`].
pub use provenance::{
    AnnotationLocation, AnnotationProvenance, EmptyStreamName, Exactness, Provenance,
    SourceGeometryRole, SourceLocation, SourceObjectAssociation, SourceProvenance,
    StaticStreamName, StreamName,
};
pub use references::{ReferenceSelection, ReferenceTarget};

pub use sketches::{
    NativeOperandField, Sketch, SketchAxis, SketchConstraint, SketchConstraintDefinition,
    SketchConstraintId, SketchCoordinateAxis, SketchDistanceMeasurement, SketchDistancePair,
    SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry, SketchId, SketchNativeOperand,
    SketchPlacement, SpatialSketch, SpatialSketchEntity, SpatialSketchEntityId,
    SpatialSketchEntityUse, SpatialSketchGeometry, SpatialSketchId, SpatialSketchProfile,
};
pub use source_fidelity::{
    decode_sidecar_path, DecodeSidecar, DecodeSidecarParseError, RetainedBytes,
    RetainedSourceRecord, SourceFidelity,
};
pub use spreadsheets::{
    CellAddress, Spreadsheet, SpreadsheetCell, SpreadsheetDimension, SpreadsheetId,
    SpreadsheetRange,
};
pub use subd::{
    SubdEdge, SubdEdgeTag, SubdEdgeUse, SubdFace, SubdFaceSide, SubdGripDirection, SubdGripWedge,
    SubdPlaneFrame, SubdRadialMapSelector, SubdRadialSymmetryMap, SubdScheme, SubdSecondaryGrip,
    SubdSurface, SubdSymmetry, SubdSymmetryKind, SubdVertex, SubdVertexGripLayout, SubdVertexTag,
};
pub use unknown::{NativeUnknownRecord, UnknownRecord};
pub use validate::admit::{
    admit, admit_with_additional_native_identities, admit_with_annotations, Admission,
    CATIA_ADMISSION, DRAFT_CORE, RHINO_DRAFT, RHINO_INSTANCE,
};
pub use validate::{validate_neutral, validate_neutral_with_source_fidelity};

pub mod unknown;

/// The JSON Schema of the current [`CadIr`] representation: the pretty JSON
/// text of `cadir_json_schema()`, which requires the `schema` feature.
pub const CADIR_JSON_SCHEMA: &str = include_str!("../schema/cadir.schema.json");

/// The JSON Schema of the decode sidecar (`<stem>.fidelity.json`): the pretty
/// JSON text of `decode_sidecar_json_schema()`, which requires the `schema`
/// feature.
pub const DECODE_SIDECAR_JSON_SCHEMA: &str = include_str!("../schema/decode-sidecar.schema.json");

/// Generate the JSON Schema for the current [`CadIr`] representation.
///
/// Requires the `schema` feature.
#[cfg(feature = "schema")]
pub fn cadir_json_schema() -> schemars::Schema {
    wire_schema::<CadIr>()
}

/// Generate the JSON Schema for the decode sidecar (`<stem>.fidelity.json`).
///
/// Requires the `schema` feature.
#[cfg(feature = "schema")]
pub fn decode_sidecar_json_schema() -> schemars::Schema {
    wire_schema::<source_fidelity::DecodeSidecar>()
}

/// The schema of `T`'s JSON form. A member that its object does not require
/// is omitted for `None` and is never `null` (§2.5); a list element, an array
/// element, and a map value keep their `null`.
#[cfg(feature = "schema")]
fn wire_schema<T: schemars::JsonSchema>() -> schemars::Schema {
    schemars::generate::SchemaSettings::draft2020_12()
        .with_transform(schemars::transform::RecursiveTransform(
            omit_null_from_optional_members,
        ))
        .into_generator()
        .into_root_schema_for::<T>()
}

#[cfg(feature = "schema")]
fn omit_null_from_optional_members(schema: &mut schemars::Schema) {
    use serde_json::Value;

    let Some(object) = schema.as_object_mut() else {
        return;
    };
    let required = match object.get("required") {
        Some(Value::Array(names)) => names.clone(),
        _ => Vec::new(),
    };
    let Some(Value::Object(members)) = object.get_mut("properties") else {
        return;
    };
    for (name, member) in members.iter_mut() {
        if let Value::Object(member) = member {
            if !required.iter().any(|required| required == name.as_str()) {
                omit_null(member);
            }
        }
    }
}

/// Remove the `null` arm that `schemars` gives an `Option`: from a `type`
/// list, from an `enum`, or from an `anyOf`, whose one remaining arm then
/// becomes the member's schema.
#[cfg(feature = "schema")]
fn omit_null(member: &mut serde_json::Map<String, serde_json::Value>) {
    use serde_json::Value;

    if let Some(Value::Array(kinds)) = member.get_mut("type") {
        kinds.retain(|kind| kind != "null");
        if let [kind] = kinds.as_slice() {
            let kind = kind.clone();
            member.insert("type".into(), kind);
        }
    }
    if let Some(Value::Array(values)) = member.get_mut("enum") {
        values.retain(|value| !value.is_null());
        if let [value] = values.as_slice() {
            let value = value.clone();
            member.remove("enum");
            member.insert("const".into(), value);
        }
    }
    let null = |arm: &Value| arm.get("type").and_then(Value::as_str) == Some("null");
    let Some(Value::Array(arms)) = member.get_mut("anyOf") else {
        return;
    };
    if !arms.iter().any(null) {
        return;
    }
    arms.retain(|arm| !null(arm));
    if let [Value::Object(arm)] = arms.as_slice() {
        let arm = arm.clone();
        if arm
            .keys()
            .all(|key| key == "anyOf" || !member.contains_key(key))
        {
            member.remove("anyOf");
            member.extend(arm);
        }
    }
}

#[cfg(test)]
mod integration_tests;

#[cfg(test)]
mod test_support;

/// Reference edges used by graph queries.
pub mod reference_walk;
/// Reproducible document fragments and stable finding signatures.
pub mod repro;
