// SPDX-License-Identifier: Apache-2.0
//! The walk over datums (§6.2).

use cadmpeg_core::CodecError;

use crate::design::ops::datum::{Datum, DatumElement};

use super::{Collect, Walk};

impl Walk for Datum {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        let Self {
            element,
            link,
            native_options,
            source_settings,
        } = self;
        element.walk(into)?;
        link.walk(into)?;
        native_options.walk(into)?;
        source_settings.walk(into)?;
        Ok(())
    }
}

impl Walk for DatumElement {
    fn walk<'a>(&'a self, into: &mut Collect<'a, '_>) -> Result<(), CodecError> {
        match self {
            Self::Points(f0) => f0.walk(into),
            Self::Axis(f0) => f0.walk(into),
            Self::Plane(f0) => f0.walk(into),
            Self::Frame(f0) => f0.walk(into),
        }
    }
}
