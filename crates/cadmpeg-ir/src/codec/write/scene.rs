// SPDX-License-Identifier: Apache-2.0
//! Scene settings that the caller of an export selects.

use crate::up_axis::UpAxis;

/// Scene settings for an encoder of a scene format. An encoder whose format
/// has no such setting does not read them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneOptions {
    /// The up axis of the scene. The encoder turns the model so that its
    /// up direction is this axis. `None` keeps the up axis of the source, and writes Z where the
    /// source states none.
    pub up_axis: Option<UpAxis>,
    /// Metres per scene unit, finite and positive. `None` selects the
    /// default of the format.
    pub meters_per_unit: Option<crate::scalar::PositiveReal>,
    /// Whether exact B-rep geometry is written beside the meshes.
    pub exact_brep: bool,
    /// Whether each material gets a `MaterialX` shader beside its
    /// interchange shader.
    pub materialx: bool,
}

impl SceneOptions {
    /// The default of each setting: the source up axis, the format's units,
    /// exact B-reps, and `MaterialX` shaders.
    pub const DEFAULT: Self = Self {
        up_axis: None,
        meters_per_unit: None,
        exact_brep: true,
        materialx: true,
    };
}

impl Default for SceneOptions {
    fn default() -> Self {
        Self::DEFAULT
    }
}
