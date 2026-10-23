//! The class rules, ported line for line from the research grammar
//! (`research/sldprt-mfc-archive/grammar/g_*.py`). Each Python module is a
//! Rust module of the same role: `feat` (`g_feat.py`), `sketch`
//! (`g_sketch.py`), `ctx` (`g_ctx.py`), `refs` (`g_ref.py`) and `idrep`
//! (`g_idrep.py`). Function names are the Python names.

use super::reader::{Reader, Res, V};

pub(crate) mod ctx;
mod dispatch;
pub(crate) mod feat;
pub(crate) mod idrep;
pub(crate) mod refs;
pub(crate) mod sketch;

pub(crate) use dispatch::rule;

/// A class's rule.
pub(crate) type RuleFn = fn(&mut Reader<'_>) -> Res<V>;

/// The rule state the Python keeps in module globals.
#[derive(Debug, Clone, Default)]
pub(crate) struct State {
    /// `g_feat.py`'s shared flags (`FEATDIM`, `OFFDIM`, `SKBOX`).
    pub(crate) core: feat::CoreState,
    /// State of the first half of `g_feat.py`'s rules.
    pub(crate) fa: feat::AState,
    /// State of the second half of `g_feat.py`'s rules.
    pub(crate) fb: feat::BState,
    /// `g_sketch.py`'s state (`MIRROR`, `BYREF`, ...).
    pub(crate) sk: sketch::SketchState,
    /// `g_ref.py`'s `FACE[0]`.
    pub(crate) face: bool,
}
