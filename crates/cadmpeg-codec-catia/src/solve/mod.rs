//! Combinatorial solvers plus the byte-table readers that feed them.
//!
//! `incidence`, `matching`, and `union_find` are pure combinatorial primitives
//! over integer node indices. `missing_edge` and `mesh_quotient` also carry
//! byte parsers for the standard-family trim-mesh and boundary tables.

pub(crate) mod incidence;
pub(crate) mod matching;
pub(crate) mod mesh_gauge;
pub(crate) mod mesh_quotient;
pub(crate) mod missing_edge;
pub(crate) mod union_find;

pub(crate) use union_find::UnionFind;

#[cfg(test)]
mod tests;
