// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(unused_imports)] // Path-included children import these names through `use super::*;`.
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::if_not_else,
    clippy::needless_pass_by_value,
    clippy::range_plus_one,
    clippy::semicolon_if_nothing_returned,
    clippy::trivially_copy_pass_by_ref
)]
//! Tests over synthetic byte fixtures. No real CAD files exist in this repo and
//! none may be added, so every fixture is hand-built here to exercise a real
//! decode path that can fail if the code regresses.

use std::io::{Cursor, Read, Seek, Write};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, InspectOptions};
use cadmpeg_ir::codec::{Codec, Confidence, DecodeOptions, Encoder};
use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;
pub(super) use cadmpeg_ir::report::LossTaxonomy;
use cadmpeg_ir::report::{LossKind as LossCode, Severity};
use zip::CompressionMethod;

use crate::bytes::lp_utf16_bytes;
use crate::container::{self, role};
use crate::test_support::*;
use crate::F3dCodec;
use cadmpeg_asm::asm_header;

#[path = "tests_native.rs"]
mod native;

#[path = "tests_writer.rs"]
mod writer;

#[path = "tests_design.rs"]
mod design;

#[path = "tests_decode.rs"]
mod decode;

#[path = "tests_surfaces.rs"]
mod surfaces;

#[path = "tests_procedural.rs"]
mod procedural;

#[path = "tests_curves.rs"]
mod curves;

#[path = "tests_materials.rs"]
mod materials;

#[path = "tests_assembly.rs"]
mod assembly;
use assembly::*;

#[path = "golden_tests.rs"]
mod golden;

#[path = "integration_tests.rs"]
mod integration_tests;
