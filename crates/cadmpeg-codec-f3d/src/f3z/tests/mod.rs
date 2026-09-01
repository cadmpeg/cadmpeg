// SPDX-License-Identifier: Apache-2.0
//! F3Z merge and archive tests.
#![allow(clippy::unwrap_used)]
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::if_not_else,
    clippy::needless_pass_by_value,
    clippy::range_plus_one,
    clippy::semicolon_if_nothing_returned,
    clippy::trivially_copy_pass_by_ref
)]

use cadmpeg_ir::codec::EncodeInput;
use cadmpeg_ir::codec::TargetRequest;
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence, DecodeOptions, Encoder};

use crate::test_support::*;
use crate::{F3dCodec, F3dLossCode};

use crate::records::DesignSketchPlacement;
use cadmpeg_ir::document::Model;
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId};
use cadmpeg_ir::ids::{BodyId, RegionId};
use cadmpeg_ir::topology::{Body, BodyKind, Region};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::{Native, NativeRecord};

mod archive;
mod layers;
mod merge;
