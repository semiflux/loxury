//! Runtime value representation.
//!
//! The default is a tagged enum; the `nan-boxing` feature swaps in a
//! single-word representation with the same public API.

#[cfg(feature = "nan-boxing")]
pub mod nanboxed;
#[cfg(not(feature = "nan-boxing"))]
mod tagged;

#[cfg(feature = "nan-boxing")]
pub use nanboxed::Value;
#[cfg(not(feature = "nan-boxing"))]
pub use tagged::Value;
