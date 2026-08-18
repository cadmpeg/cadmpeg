//! Tests for the `endpoints` module.
#![allow(unused_imports)]

use super::super::bindings::normalize_indexed_curve_entities;
use super::super::curves::compact_bounded_curve_tangent;
use super::super::markers::{marker_coordinates, sketch_input_entities};
use super::super::relation_loci::same_dimension_length;
use super::super::selections::marker_local_links;
use super::super::typed_relations::{
    current_undetailed_bounded_curve_is_line, extended_direct_object_line_endpoints,
    legacy_marker104_arc_endpoints, marker_curve_endpoint_markers,
};
use super::super::{
    CLASS_MARKER, LEGACY_EXTENDED_SKETCH_MARKER, LEGACY_SKETCH_MARKER, SKETCH_MARKER,
};
use super::*;
use crate::records::{
    FeatureInputLane, SketchInputEntity, SketchInputKind, SketchInputLink, SketchRelationKind,
};
use cadmpeg_ir::math::Point2;
use std::collections::HashMap;

mod arcs;
mod circles;
mod compact;
mod extended;
mod profile_lines;
mod terminals;
