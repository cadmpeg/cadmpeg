// SPDX-License-Identifier: Apache-2.0
//! Format-neutral CAD documents and the codec interfaces that produce them.
//!
//! [`CadIr`] stores units, tolerances, and flat entity arenas connected by
//! typed IDs. Its B-rep topology follows
//! `body → region → shell → face → loop → coedge → edge → vertex`; topology
//! references geometry carriers instead of nesting them. The document also
//! carries neutral construction features, tessellation, appearance, source
//! attributes, source-native namespaces, and uninterpreted [`UnknownRecord`]s.
//!
//! Start a hand-built document with [`CadIr::empty`], populate its arenas,
//! call [`CadIr::finalize`] to establish canonical identity order, then call
//! [`validate()`] to check structural and numeric invariants. Use
//! [`CadIr::to_canonical_json`] and [`CadIr::from_json`] for the versioned JSON
//! form, and [`diff()`] for identity-based structural comparison.
//!
//! Format crates implement [`Codec`]. Detection selects a codec from a byte
//! prefix, inspection enumerates a container, and decoding returns a
//! [`DecodeResult`]. Operation failures use [`CodecError`]. A successful decode
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
pub mod attributes;
pub mod bytes;
pub mod codec;
pub mod cursor;
pub mod decode;

pub mod diff;
pub mod document;
pub mod drawings;
pub mod eval;
/// Hand-built fixture documents for tests and tooling. Gated behind the
/// `test-support` feature (always on under `cfg(test)`) so a normal library
/// build omits them.
#[cfg(any(test, feature = "test-support"))]
pub mod examples;
pub mod features;
pub mod geometry;

pub mod ids;
pub mod math;
pub mod native;
pub mod pmi;
pub mod presentation;
pub mod product;
pub mod products;
mod provenance;
pub mod report;
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
pub mod wire;

pub use annotations::{AnnotationBuilder, Annotations, ExactnessNote, Provenance};
pub use codec::{
    CadirEncoder, Codec, CodecEntry, CodecError, Confidence, ContainerEntry, ContainerSummary,
    DecodeOptions, DecodeResult, Encoder, ReadSeek,
};
pub use diff::{diff, ArenaDiff, IrDiff, ModifiedEntity};
pub use document::{CadIr, SourceMeta, IR_VERSION};
pub use features::{
    BodyRetentionMode, BodySelection, BodyTrimSide, CoilConstruction, CoilExtent, CoilPlacement,
    CoilResult, CoilSection, CoilSectionPlacement, ConfigurationBodies, ConfigurationId,
    CurveProjectionDirection, CurveProjectionDirectionState, DesignConfiguration, DesignParameter,
    FaceMotion, Feature, FeatureDefinition, FeatureId, ParameterId, ParameterPmi, ParameterValue,
    PmiDimensionSubtype, ScaleCenter, ScaleFactors, SketchSpace,
};
pub use native::{LossCount, Native, NativeConvertError, NativeNamespace, NativeRecord};
pub use pmi::{
    DatumReference, DimensionKind, GeometricToleranceKind, PmiAnnotation, PmiDefinition,
    PmiQuantity, PmiTarget, PmiValue,
};
pub use presentation::{
    CameraState, PresentationDocument, PresentationId, PresentationState, ViewPresentation,
};
pub use presentation::{PresentationItem, PresentationLayer};
pub use product::{OccurrenceParent, Product, ProductOccurrence};
pub use products::{
    AssemblyJoint, Component, ComponentId, ComponentKind, ComponentReference, CopyOnChangePolicy,
    ExternalDocumentReference, ExternalResolution, JointId, JointKind, JointLimits, JointOperand,
    Occurrence, OccurrenceId,
};
/// Source location attached to a [`LossNote`].
pub use provenance::Provenance as LossProvenance;
pub use provenance::{Exactness, SourceObjectAssociation};
pub use report::{
    DecodeReport, ExportReport, LossCategory, LossCode, LossNote, Severity, StrictConsequence,
};
pub use sketches::{
    Sketch, SketchAxis, SketchConstraint, SketchConstraintDefinition, SketchConstraintId,
    SketchCoordinateAxis, SketchDistanceMeasurement, SketchEntity, SketchEntityId, SketchEntityUse,
    SketchGeometry, SketchId, SketchNativeOperand, SketchPlacement, SpatialSketch,
    SpatialSketchEntity, SpatialSketchEntityId, SpatialSketchEntityUse, SpatialSketchGeometry,
    SpatialSketchId, SpatialSketchProfile,
};
pub use source_fidelity::{RetainedSourceRecord, SourceFidelity, SOURCE_FIDELITY_VERSION};
pub use spreadsheets::{Spreadsheet, SpreadsheetDimension, SpreadsheetId, SpreadsheetRange};
pub use subd::{
    SubdEdge, SubdEdgeTag, SubdEdgeUse, SubdFace, SubdScheme, SubdSurface, SubdVertex,
    SubdVertexTag,
};
pub use unknown::{NativeUnknownRecord, UnknownRecord};
pub use validate::{validate, validate_with_source_fidelity, Check, Finding, ValidationReport};

pub mod unknown;

/// Generate the JSON Schema for the current [`CadIr`] representation.
pub fn cadir_json_schema() -> schemars::Schema {
    schemars::schema_for!(CadIr)
}

#[cfg(test)]
mod tests;
