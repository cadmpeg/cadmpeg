// SPDX-License-Identifier: Apache-2.0
//! Shared synthetic 3DM byte-fixture builders for `#[cfg(test)]` suites.
//!
//! Helpers hand-build archive bytes only; owner suites own the assertions.
#![allow(clippy::unwrap_used)]

pub(crate) mod test_archive;
pub(crate) mod test_dump;

/// A finite fixture point.
pub(crate) fn point3(coordinates: [f64; 3]) -> crate::settings::Point3 {
    crate::settings::Point3(
        cadmpeg_ir::units::FiniteVector::new(coordinates).expect("finite fixture point"),
    )
}

/// The millimetre scale a fixture states, bound through the custom unit route
/// a scanned document uses.
pub(crate) fn millimeter_scale(millimeters_per_unit: f64) -> crate::settings::MillimeterScale {
    let unit = crate::settings::UnitSystem::custom(
        millimeters_per_unit / 1000.0,
        "fixture unit".to_owned(),
    )
    .expect("test millimetre scale");
    let units = crate::settings::UnitsAndTolerances {
        unit,
        absolute_tolerance: 0.01,
        angular_tolerance: 0.1,
        relative_tolerance: 0.01,
        distance_display: None,
    };
    let crate::settings::UnitBinding::Millimeters(scale) =
        crate::settings::UnitBinding::from_units(Some(&units))
    else {
        panic!("a custom unit binds a millimetre scale");
    };
    scale
}

/// Plans a write at one archive version, the request the command line builds
/// for an explicit target.
///
/// The suites assert what the writer produces at a version, not how the request
/// that names it is spelled, so the spelling lives in one place. `Inherit` is a
/// different question — it resolves against the source and cannot be pinned per
/// version — and `writer/tests/targets.rs` owns it.
pub(crate) fn plan_at(
    version: crate::RhinoArchiveVersion,
    ir: &cadmpeg_ir::document::CadIr,
) -> Result<cadmpeg_ir::codec::write::ExportPlan, cadmpeg_core::CodecError> {
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};

    crate::RhinoCodec.plan(
        EncodeInput::new(ir, None),
        TargetRequest::Explicit(version.descriptor().id.as_str()),
    )
}
