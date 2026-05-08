//! Procedural macro for the `polyvers` crate. Depend on `polyvers` instead of
//! using this crate directly.

use proc_macro::TokenStream;
use syn::parse_macro_input;

mod codegen;
mod parse;
mod resolve;

/// Declare a versioned struct family. See the `polyvers` crate README for the
/// full DSL and the canonical example.
#[proc_macro]
pub fn versioned(input: TokenStream) -> TokenStream {
    let spec = parse_macro_input!(input as parse::VersionedSpec);
    match resolve::resolve(spec) {
        Ok(resolved) => codegen::generate(&resolved).into(),
        Err(err) => err.to_compile_error().into(),
    }
}
