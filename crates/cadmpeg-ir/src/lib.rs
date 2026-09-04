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
//! [`ValidationReport`]. Decode produces [`DecodeResult`] without embedding
//! validation. Start a hand-built document with [`CadIr::empty`], populate its
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
//! reports partial transfer through [`DecodeReport`] and [`LossNote`].
//!
//! [`Annotations`] records source locations and fidelity by globally unique
//! entity ID. An omitted exactness entry means byte-exact; explicit entries
//! distinguish derived, inferred, and unknown values. Native namespaces and
//! unknown records retain source-specific data outside the neutral model.
//!
//! Product components, occurrence instancing, and assembly joints have neutral arenas.
//! Product prototypes and occurrence trees retain assembly identity and
//! placement. Joint and mate constraints are reserved.

pub mod annotations;
pub mod appearance;
pub mod assets;
pub mod attributes;
pub mod bytes;
pub mod codec;
pub mod compare;
pub mod container;

pub mod diff;
pub mod document;
pub mod draft;
pub mod drawings;
pub mod eval;
// `test` keeps fixtures available for this crate's unit tests; dependents that
// need them in their test graphs still enable the `examples` feature explicitly.
#[cfg(any(feature = "examples", test))]
pub mod examples;
pub mod features;
pub mod geometry;
pub mod hash;

pub mod ids;
pub mod index;
pub mod math;
pub mod native;
pub mod pmi;
pub mod presentation;
pub mod products;
mod provenance;
pub mod references;
pub mod report;
pub mod schema;
pub mod semantic_annotations;
pub mod sketches;
pub mod source_fidelity;
pub mod spreadsheets;
pub mod subd;
pub mod tessellation;
pub mod topology;
pub mod transform;
pub mod units;
pub mod validate;

pub use annotations::{AnnotationBuilder, Annotations, ExactnessNote};
pub use codec::{Codec, CodecBackend, Confidence, DecodeFailure, DecodeOptions, DecodeResult};
pub use container::ContainerSummary;
pub use diff::{diff, ArenaDiff, AttributeChange, IrDiff, ModifiedEntity, SourceDiff};
pub use document::{CadIr, SourceMeta, IR_VERSION};
pub use draft::ModelDraft;
pub use features::{
    BodyRetentionMode, BodySelection, BodyTrimSide, CoilConstruction, CoilExtent, CoilPlacement,
    CoilResult, CoilSection, CoilSectionPlacement, ConfigurationBodies, ConfigurationId,
    ConfigurationName, CurveProjectionDirection, CurveProjectionDirectionState,
    DesignConfiguration, DesignParameter, FaceMotion, Feature, FeatureDefinition, FeatureId,
    ParameterId, ParameterPmi, ParameterValue, PmiDimensionSubtype, ScaleCenter, ScaleFactors,
};
pub use ids::{format_identity, is_valid_identity, IdentityError};
pub use native::{LossCount, Native, NativeConvertError, NativeNamespace, NativeRecord};
pub use pmi::{
    DatumReference, DatumTargetForm, DimensionKind, DimensionTolerance, GeometricToleranceKind,
    PmiAnnotation, PmiDefinition, PmiQuantity, PmiTarget, PmiValue,
};
pub use presentation::{
    CameraState, PresentationDocument, PresentationId, PresentationState, ViewPresentation,
};
pub use presentation::{PresentationItem, PresentationLayer};
pub use products::{
    AssemblyGraph, AssemblyGraphError, AssemblyJoint, CopyOnChange, CopyOnChangePolicy,
    ExternalDocument, ExternalDocumentReference, JointConnector, JointId, JointLimits,
    JointOperand, JointOperands, LinkState, NonEmptyString, Occurrence, OperandContainer,
    PairedJointKind, ProductDefinition, ProductDefinitionKind, PrototypeReference,
};
/// Source location attached to a [`LossNote`].
pub use provenance::{
    AnnotationLocation, AnnotationProvenance, Exactness, Provenance, SourceLocation,
    SourceObjectAssociation, SourceProvenance,
};
pub use references::{ReferenceSelection, ReferenceTarget};

pub use report::{
    CensusBasis, Check, Coverage, CoverageKey, DecodeReport, DecodeTransfer, EntityCensus,
    ExportReport, FidelityResolution, Finding, HexByteCoverageKey, IndexedCoverageKey,
    LossCategory, LossKind, LossNote, LossTaxonomy, Severity, StrictConsequence, ValidationReport,
    WritePath, SHARED_LOSS_NAMESPACE,
};
pub use sketches::{
    Sketch, SketchAxis, SketchConstraint, SketchConstraintDefinition, SketchConstraintId,
    SketchCoordinateAxis, SketchDistanceMeasurement, SketchDistancePair, SketchEntity,
    SketchEntityId, SketchEntityUse, SketchGeometry, SketchId, SketchNativeOperand,
    SketchPlacement, SketchSolverScalar, SolverScalarClass, SpatialSketch, SpatialSketchEntity,
    SpatialSketchEntityId, SpatialSketchEntityUse, SpatialSketchGeometry, SpatialSketchId,
    SpatialSketchProfile,
};
pub use source_fidelity::{
    decode_sidecar_path, DecodeSidecar, DecodeSidecarParseError, RetainedSourceRecord,
    SourceFidelity, DECODE_SIDECAR_VERSION, DECODE_SIDECAR_VERSION_V1, DECODE_SIDECAR_VERSION_V2,
    DECODE_SIDECAR_VERSION_V3, SOURCE_FIDELITY_VERSION,
};
pub use spreadsheets::{Spreadsheet, SpreadsheetDimension, SpreadsheetId, SpreadsheetRange};
pub use subd::{
    SubdEdge, SubdEdgeTag, SubdEdgeUse, SubdFace, SubdGripDirection, SubdGripWedge, SubdPlaneFrame,
    SubdRadialMapSelector, SubdRadialSymmetryMap, SubdScheme, SubdSecondaryGrip, SubdSurface,
    SubdSymmetry, SubdSymmetryKind, SubdVertex, SubdVertexGripLayout, SubdVertexTag,
};
pub use unknown::{NativeUnknownRecord, UnknownRecord};
pub use validate::admit::{
    admit, admit_with_additional_native_identities, admit_with_annotations, filter_checks,
    CATIA_ADMISSION_CHECKS, DRAFT_CORE_CHECKS, RHINO_DRAFT_CHECKS, RHINO_INSTANCE_CHECKS,
    SLDPRT_EXPORT_PRECONDITION_CHECKS,
};
pub use validate::{entity_census, validate_neutral, validate_neutral_with_source_fidelity};

pub mod unknown;

/// Generate the JSON Schema for the current [`CadIr`] representation.
///
/// Requires the `schema` feature.
#[cfg(feature = "schema")]
pub fn cadir_json_schema() -> schemars::Schema {
    schemars::schema_for!(CadIr)
}

/// Generate the JSON Schema for the decode sidecar (`<stem>.fidelity.json`).
///
/// Requires the `schema` feature.
#[cfg(feature = "schema")]
pub fn decode_sidecar_json_schema() -> schemars::Schema {
    schemars::schema_for!(source_fidelity::DecodeSidecar)
}

#[cfg(test)]
mod integration_tests;
