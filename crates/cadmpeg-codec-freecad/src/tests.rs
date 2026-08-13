#![allow(unused_imports)]

use std::io::Cursor;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::features::{
    Angle, BooleanOp, ExtrudeExtent, ExtrudeSide, ExtrusionDirectionSource, FeatureDefinition,
    InnerWireTaper, Length, PathRef, RevolveExtent, ShellJoin, ShellMode, SweepOrientation,
    SweepTransformation, SweepTransition, Termination,
};
use cadmpeg_ir::semantic_annotations::SemanticAnnotationKind as Kind;
use cadmpeg_ir::{Codec, CodecBackend, Confidence, DecodeOptions, Encoder};
use zip::write::SimpleFileOptions;

use crate::FcstdCodec;

pub(crate) use crate::test_support::*;

#[path = "integration_tests.rs"]
mod integration_tests;
