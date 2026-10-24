//! The class rules. Each module groups the rules for one part of the object
//! graph: `feature` (the named-object header, feature blocks, folders,
//! references, components, parameters, dimensions, bodies and patterns),
//! `sketch`, `ctx`, `refs` and `idrep`.

use super::reader::{Reader, Res, V};

pub(crate) mod ctx;
mod dispatch;
pub(crate) mod feature;
pub(crate) mod idrep;
pub(crate) mod refs;
pub(crate) mod sketch;

pub(crate) use dispatch::rule;

/// A class's rule.
pub(crate) type RuleFn = fn(&mut Reader<'_>) -> Res<V>;

/// The rule state shared across the rule modules.
#[derive(Debug, Clone, Default)]
pub(crate) struct State {
    /// The feature rules' shared flags (`FEATDIM`, `OFFDIM`, `SKBOX`).
    pub(crate) core: feature::CoreState,
    /// The sketch rules' state (`MIRROR`, `BYREF`, ...).
    pub(crate) sk: sketch::SketchState,
    /// The face-reference rule's flag.
    pub(crate) face: bool,
}
