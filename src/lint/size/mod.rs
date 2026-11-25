//! The lints of size: how much of one thing a reader takes in at once, in a
//! type, a struct or enum, a module, a function, a directory, or a file. Past
//! what they hold, the thing wants smaller things beside it, by what they are
//! about.

pub mod functions;
pub mod layout;
pub mod wide_module;
pub mod wide_struct;
pub mod wide_type;
