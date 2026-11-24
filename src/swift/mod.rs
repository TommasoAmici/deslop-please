//! Swift packages: their targets, the `scip-swift` index, and the model built from it.

pub mod indexing;
pub mod model;
pub mod source;
pub mod symbol;
pub mod workspace;

pub use workspace::Workspace;
