//! Runtime value representation.
//!
//! The default is a NaN-boxed single word; `--no-default-features` swaps in
//! a tagged enum with the same public API.

#[cfg(feature = "nan-boxing")]
mod nanboxed;
#[cfg(not(feature = "nan-boxing"))]
mod tagged;

#[cfg(feature = "nan-boxing")]
pub use nanboxed::Value;
#[cfg(not(feature = "nan-boxing"))]
pub use tagged::Value;
