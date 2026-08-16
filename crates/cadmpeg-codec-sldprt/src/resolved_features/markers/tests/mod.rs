//! Tests for the `markers` module.
#![allow(unused_imports)]

use super::super::selections::coordinate_marker_local_links;
use super::super::{
    CLASS_MARKER, LEGACY_EXTENDED_SKETCH_MARKER, LEGACY_SKETCH_MARKER, SKETCH_MARKER,
};
use super::*;
use crate::records::{
    FeatureInputClass, FeatureInputClassRole, FeatureInputOperand, FeatureInputOperandKind,
    FeatureInputScalar, FeatureInputScalarRole, SketchInputEntity, SketchInputKind,
    SketchRelationKind,
};
use cadmpeg_ir::math::Point3;

mod lanes;
mod profile_curves;
mod profile_points;
mod spatial;
