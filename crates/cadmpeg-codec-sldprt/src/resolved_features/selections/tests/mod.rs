//! Tests for the `selections` module.
#![allow(unused_imports)]

use super::super::component_paths::{compact_edge_path_value, compact_edge_selection_set_value};
use super::super::{CLASS_MARKER, LEGACY_SKETCH_MARKER};
use super::selection_vector_tail;
use super::*;
use crate::classification::FeatureClass;
use crate::records::{
    Feature, FeatureHistory, FeatureInputClass, FeatureInputClassRole,
    FeatureInputComponentPathEntry, FeatureInputEdgeSelection, FeatureInputLane, FeatureInputName,
    FeatureInputScalar, FeatureInputScalarRole,
};
use std::collections::{BTreeMap, HashSet};

mod compact_edge;
mod surfaces;
mod writer;
