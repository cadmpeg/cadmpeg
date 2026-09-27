//! Combinatorial solvers plus the byte-table readers that feed them.
//!
//! `incidence`, `matching`, and `union_find` are pure combinatorial primitives
//! over integer node indices. `missing_edge` and `mesh_quotient` also carry
//! byte parsers for the standard-family trim-mesh and boundary tables.

#[cfg(test)]
macro_rules! catia_test_context {
    ($ctx:ident) => {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::service();
        let ($ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("service decode context");
    };
}

pub(crate) mod incidence;
pub(crate) mod matching;
pub(crate) mod mesh_gauge;
pub(crate) mod mesh_quotient;
pub(crate) mod missing_edge;
pub(crate) mod union_find;

#[cfg(test)]
mod tests;
