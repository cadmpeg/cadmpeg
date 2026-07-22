// SPDX-License-Identifier: Apache-2.0
//! Typed Siemens NX object-model records retained in the native namespace.

use std::collections::{BTreeMap, BTreeSet};

use flate2::read::ZlibDecoder;
use serde::{Deserialize, Serialize};

use cadmpeg_ir::hash::sha256_hex;

use crate::container::Container;
use crate::parasolid::{Stream, StreamKind};

mod display_jt;
mod features;
mod model;
mod om;
mod parasolid;
mod segments;

pub use display_jt::*;
pub use features::*;
pub(crate) use model::*;
pub use om::*;
pub use parasolid::*;
pub use segments::*;
