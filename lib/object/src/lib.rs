//! Object creator for Wasm Compilations.
//!
//! Given a compilation result (this is, the result when calling `Compiler::compile_module`)
//! this exposes functions to create an Object file for a given target.

#![deny(missing_docs, trivial_numeric_casts, unused_extern_crates)]
#![warn(unused_import_braces)]
#![cfg_attr(feature = "cargo-clippy", allow(clippy::new_without_default))]
#![cfg_attr(not(feature = "std"), no_std)]
#![cfg_attr(
    feature = "cargo-clippy",
    warn(
        clippy::float_arithmetic,
        clippy::mut_mut,
        clippy::nonminimal_bool,
        clippy::map_unwrap_or,
        clippy::print_stdout,
        clippy::unicode_not_nfc,
        clippy::use_self
    )
)]
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

#![feature(error_in_core)]
#![feature(core_intrinsics)]

#[cfg(all(feature = "std", feature = "core"))]
compile_error!(
    "The `std` and `core` features are both enabled, which is an error. Please enable only once."
);

#[cfg(all(not(feature = "std"), not(feature = "core")))]
compile_error!("Both the `std` and `core` features are disabled. Please enable one of them.");

mod error;
mod module;

pub use crate::error::ObjectError;
pub use crate::module::{emit_compilation, emit_data, emit_serialized, get_object_for_target};
pub use object::{self, write::Object};
