use std::collections::HashSet;

use proc_macro2::TokenStream;
use quote::{ToTokens, format_ident, quote};
use syn::{GenericArgument, Ident, Path, PathArguments, Type};

use crate::parse::{Codec, CodecDecl, MetaInit};
use crate::resolve::{ResolvedSpec, ResolvedStruct, ResolvedVersion};

pub fn generate(spec: &ResolvedSpec) -> TokenStream {
    let family = &spec.family;

    let derive_attr = if spec.derives.is_empty() {
        quote! {}
    } else {
        let derives = &spec.derives;
        quote! { #[derive(#(#derives),*)] }
    };

    let main_struct_name = spec
        .versions
        .first()
        .and_then(|v| v.structs.first())
        .map(|s| s.name.clone())
        .expect("resolver guarantees at least one version with at least one struct");

    let meta_type = spec.meta_type.clone();
    let version_modules = spec
        .versions
        .iter()
        .map(|v| emit_version_module(v, &main_struct_name, &derive_attr, meta_type.as_ref()));

    let latest = spec.versions.last().expect("at least one version");
    let latest_module = &latest.module_ident;
    let latest_value = latest.version.value();

    let variants = spec.versions.iter().map(|v| {
        let variant = pascal_variant_for(&v.module_ident);
        let module = &v.module_ident;
        let main = &main_struct_name;
        quote! { #variant(#module::#main) }
    });

    let version_strs: Vec<TokenStream> = spec
        .versions
        .iter()
        .map(|v| {
            let s = v.version.value();
            quote! { #s }
        })
        .collect();

    let version_arms = spec.versions.iter().map(|v| {
        let variant = pascal_variant_for(&v.module_ident);
        let value = v.version.value();
        quote! { AnyVersion::#variant(_) => #value }
    });

    let any_helpers = spec.versions.iter().map(|v| {
        let variant = pascal_variant_for(&v.module_ident);
        let module = &v.module_ident;
        let main = &main_struct_name;
        let into_fn = format_ident!("into_{}", v.module_ident);
        let as_fn = format_ident!("as_{}", v.module_ident);
        quote! {
            pub fn #into_fn(self) -> ::core::option::Option<#module::#main> {
                match self {
                    AnyVersion::#variant(v) => ::core::option::Option::Some(v),
                    #[allow(unreachable_patterns)]
                    _ => ::core::option::Option::None,
                }
            }
            pub fn #as_fn(&self) -> ::core::option::Option<&#module::#main> {
                match self {
                    AnyVersion::#variant(v) => ::core::option::Option::Some(v),
                    #[allow(unreachable_patterns)]
                    _ => ::core::option::Option::None,
                }
            }
        }
    });

    let (meta_at_version_fn, any_version_meta_fn) = match &spec.meta_type {
        Some(meta_type) => {
            let arms = spec.versions.iter().map(|v| {
                let module = &v.module_ident;
                let value = v.version.value();
                quote! {
                    #value => ::core::option::Option::Some(#module::meta())
                }
            });
            let any_arms = spec.versions.iter().map(|v| {
                let variant = pascal_variant_for(&v.module_ident);
                let module = &v.module_ident;
                quote! {
                    AnyVersion::#variant(_) => #module::meta()
                }
            });
            (
                quote! {
                    pub fn meta_at_version(version: &str) -> ::core::option::Option<#meta_type> {
                        match version {
                            #(#arms,)*
                            _ => ::core::option::Option::None,
                        }
                    }
                },
                quote! {
                    pub fn meta(&self) -> #meta_type {
                        match self {
                            #(#any_arms),*
                        }
                    }
                },
            )
        }
        None => (quote! {}, quote! {}),
    };

    let parse_arms = spec.versions.iter().map(|v| {
        let variant = pascal_variant_for(&v.module_ident);
        let module = &v.module_ident;
        let main = &main_struct_name;
        let value = v.version.value();
        quote! {
            #value => ::serde_json::from_str::<#module::#main>(input)
                .map(AnyVersion::#variant)
                .map_err(::polyvers::Error::format),
        }
    });

    let any_derives = build_any_derives(&spec.derives);

    let codec_artifacts: Vec<CodecArtifact> = spec
        .codecs
        .iter()
        .map(|c| emit_codec(c, &spec.versions, &main_struct_name))
        .collect();
    let codec_parse_fns = codec_artifacts.iter().map(|a| &a.parse_fn);
    let codec_serialize_methods = codec_artifacts.iter().map(|a| &a.serialize_method);

    // All struct names declared anywhere in the family. Used by the auto-From
    // codegen to decide whether a carried field's type lives in a versioned
    // module (so the assignment needs `.into()` to bridge `v_n::T` and
    // `v_n_plus_1::T`) or is some external type (primitive / std / user-defined
    // outside the family) that we should copy verbatim.
    let family_struct_names: HashSet<String> = spec
        .versions
        .iter()
        .flat_map(|v| v.structs.iter().map(|s| s.name.to_string()))
        .collect();

    // Per (struct_name, hop_idx) → "is this `From` auto-emittable on this
    // hop?". Computed by fixed-point: a struct is emittable iff its hop delta
    // is pure-`#[add]` *and* every family struct it transitively references
    // through its carried fields is also emittable on the same hop. This
    // matters because the auto-From calls `.into()` on carried fields that
    // cross a version boundary — that `.into()` only compiles if the
    // dependency's own `From` was emitted.
    let emittable = compute_emittable(&spec.versions, &family_struct_names);

    // Auto-generated cross-version migrations. Each emitted impl is
    // `impl From<v_prev::Struct> for v_curr::Struct`. Hops with any `#[edit]`
    // / `#[delete]` or with non-emittable nested dependencies are deliberately
    // skipped — the user writes those by hand.
    let from_impls = emit_from_impls(&spec.versions, &family_struct_names, &emittable);

    // `AnyVersion::into_latest(self) -> Latest` chains through the From impls
    // for each older variant up to the newest. We only emit it when the main
    // struct is auto-able on *every* hop; otherwise older variants would need
    // a hand-written `From` that the user hasn't provided yet and the impl
    // would fail to compile. In that case, the user writes their own
    // `into_latest` alongside the macro invocation.
    let main_chain_complete = (1..spec.versions.len()).all(|hop_idx| {
        emittable.contains(&(main_struct_name.to_string(), hop_idx))
    });
    let into_latest_fn = if main_chain_complete {
        let arms = spec.versions.iter().enumerate().map(|(i, v)| {
            let variant = pascal_variant_for(&v.module_ident);
            if i == spec.versions.len() - 1 {
                quote! { AnyVersion::#variant(__inner) => __inner }
            } else {
                // Walk every remaining step explicitly so type inference picks
                // the right hop. `__inner: v_i::Main` becomes
                // `v_{i+1}::Main`, then `v_{i+2}::Main`, ... up to Latest.
                let lets = (i + 1..spec.versions.len()).map(|j| {
                    let step_mod = &spec.versions[j].module_ident;
                    let main = &main_struct_name;
                    quote! { let __inner: #step_mod::#main = ::core::convert::Into::into(__inner); }
                });
                quote! {
                    AnyVersion::#variant(__inner) => {
                        #(#lets)*
                        __inner
                    }
                }
            }
        });
        quote! {
            /// Migrate any known version up to the newest schema (`Latest`).
            /// Older variants chain through the auto-generated
            /// `From<v_prev::X> for v_curr::X` impls one hop at a time; the
            /// newest variant returns its inner value directly.
            pub fn into_latest(self) -> Latest {
                match self {
                    #(#arms),*
                }
            }
        }
    } else {
        quote! {}
    };

    quote! {
        pub mod #family {
            #(#version_modules)*

            #(#from_impls)*

            pub type Latest = #latest_module::#main_struct_name;
            pub const VERSIONS: &[&str] = &[#(#version_strs),*];
            pub const LATEST_VERSION: &str = #latest_value;

            #any_derives
            pub enum AnyVersion {
                #(#variants),*
            }

            impl AnyVersion {
                pub fn version(&self) -> &'static str {
                    match self {
                        #(#version_arms),*
                    }
                }

                #into_latest_fn

                #(#any_helpers)*

                #any_version_meta_fn

                #(#codec_serialize_methods)*
            }

            pub fn parse_at_version(
                version: &str,
                input: &str,
            ) -> ::core::result::Result<AnyVersion, ::polyvers::Error> {
                match version {
                    #(#parse_arms)*
                    other => ::core::result::Result::Err(
                        ::polyvers::Error::unknown_version(other, VERSIONS)
                    ),
                }
            }

            #meta_at_version_fn

            #(#codec_parse_fns)*
        }
    }
}

/// Emit auto-`From<v_prev::Struct> for v_curr::Struct` only for hops/structs
/// whose `emittable` table flag is set. The table already encodes
/// "pure-`#[add]` delta AND every nested family dep is also emittable on this
/// hop", so the impl can rely on `.into()` for every carried family-internal
/// field without dangling.
fn emit_from_impls(
    versions: &[ResolvedVersion],
    family_struct_names: &HashSet<String>,
    emittable: &HashSet<(String, usize)>,
) -> Vec<TokenStream> {
    let mut impls = Vec::new();
    for hop_idx in 1..versions.len() {
        let prev = &versions[hop_idx - 1];
        let curr = &versions[hop_idx];
        for curr_struct in &curr.structs {
            if !emittable.contains(&(curr_struct.name.to_string(), hop_idx)) {
                continue;
            }
            let Some(prev_struct) = prev.structs.iter().find(|s| s.name == curr_struct.name)
            else {
                continue;
            };
            impls.push(emit_from_for_struct(
                prev,
                curr,
                prev_struct,
                curr_struct,
                family_struct_names,
            ));
        }
    }
    impls
}

/// Decide, for every consecutive `(struct_name, hop_idx)` pair, whether the
/// auto-From for that struct on that hop is safe to emit. A struct is
/// emittable iff its delta is pure-`#[add]` *and* every family struct
/// referenced by its carried fields is itself emittable on the same hop.
/// Computed by fixed-point iteration so dependencies propagate.
fn compute_emittable(
    versions: &[ResolvedVersion],
    family_struct_names: &HashSet<String>,
) -> HashSet<(String, usize)> {
    // Seed: pure-add delta candidates.
    let mut candidates: HashSet<(String, usize)> = HashSet::new();
    for hop_idx in 1..versions.len() {
        let prev = &versions[hop_idx - 1];
        let curr = &versions[hop_idx];
        for curr_struct in &curr.structs {
            let Some(prev_struct) = prev.structs.iter().find(|s| s.name == curr_struct.name)
            else {
                continue;
            };
            if is_add_only_delta(prev_struct, curr_struct) {
                candidates.insert((curr_struct.name.to_string(), hop_idx));
            }
        }
    }

    let mut emittable: HashSet<(String, usize)> = HashSet::new();
    loop {
        let prev_size = emittable.len();
        for hop_idx in 1..versions.len() {
            let prev = &versions[hop_idx - 1];
            let curr = &versions[hop_idx];
            for curr_struct in &curr.structs {
                let key = (curr_struct.name.to_string(), hop_idx);
                if emittable.contains(&key) || !candidates.contains(&key) {
                    continue;
                }
                let Some(prev_struct) = prev.structs.iter().find(|s| s.name == curr_struct.name)
                else {
                    continue;
                };
                let all_deps_ok = curr_struct.fields.iter().all(|field| {
                    let carried = prev_struct.fields.iter().any(|pf| pf.name == field.name);
                    if !carried {
                        // Added field — no carried-dep to satisfy.
                        return true;
                    }
                    let deps = field_family_deps(&field.ty, family_struct_names);
                    deps.iter()
                        .all(|dep| emittable.contains(&(dep.clone(), hop_idx)))
                });
                if all_deps_ok {
                    emittable.insert(key);
                }
            }
        }
        if emittable.len() == prev_size {
            break;
        }
    }
    emittable
}

fn is_add_only_delta(prev: &ResolvedStruct, curr: &ResolvedStruct) -> bool {
    // Every prev field must still be present in curr.
    for pf in &prev.fields {
        let Some(cf) = curr.fields.iter().find(|f| f.name == pf.name) else {
            return false; // deleted
        };
        // The field's type must be unchanged at the token level (modulo
        // version-namespacing, which we handle separately via `.into()` for
        // family-internal types). If the *token string* differs, the field
        // was `#[edit]`'d — we can't auto-emit.
        if cf.ty.to_token_stream().to_string() != pf.ty.to_token_stream().to_string() {
            return false;
        }
    }
    true
}

fn emit_from_for_struct(
    prev: &ResolvedVersion,
    curr: &ResolvedVersion,
    prev_struct: &ResolvedStruct,
    curr_struct: &ResolvedStruct,
    family_struct_names: &HashSet<String>,
) -> TokenStream {
    let mut assignments: Vec<TokenStream> = Vec::with_capacity(curr_struct.fields.len());

    for field in &curr_struct.fields {
        let name = &field.name;
        let carried = prev_struct.fields.iter().any(|pf| pf.name == field.name);
        if carried {
            // Family-internal types need `.into()` to bridge the version
            // boundary; everything else copies verbatim.
            let expr = carry_expr(&field.ty, quote! { __from.#name }, family_struct_names);
            assignments.push(quote! { #name: #expr });
        } else {
            // Added — `#[add(default = ...)]` wins; fall back to
            // `Default::default()`.
            let init = match &field.default {
                Some(expr) => quote! { #expr },
                None => quote! { ::core::default::Default::default() },
            };
            assignments.push(quote! { #name: #init });
        }
    }

    let prev_mod = &prev.module_ident;
    let curr_mod = &curr.module_ident;
    let struct_name = &curr_struct.name;

    quote! {
        #[automatically_derived]
        impl ::core::convert::From<#prev_mod::#struct_name>
            for #curr_mod::#struct_name
        {
            fn from(__from: #prev_mod::#struct_name) -> Self {
                Self {
                    #(#assignments,)*
                }
            }
        }
    }
}

/// All family struct names referenced (directly or via generic args) by the
/// given type. Used to compute the auto-emit dependency graph.
fn field_family_deps(ty: &Type, family: &HashSet<String>) -> Vec<String> {
    fn collect(ty: &Type, family: &HashSet<String>, out: &mut Vec<String>) {
        match ty {
            Type::Path(tp) => {
                for seg in &tp.path.segments {
                    let name = seg.ident.to_string();
                    if family.contains(&name) {
                        out.push(name);
                    }
                    if let PathArguments::AngleBracketed(args) = &seg.arguments {
                        for arg in &args.args {
                            if let GenericArgument::Type(inner) = arg {
                                collect(inner, family, out);
                            }
                        }
                    }
                }
            }
            Type::Reference(r) => collect(&r.elem, family, out),
            Type::Tuple(t) => t.elems.iter().for_each(|e| collect(e, family, out)),
            Type::Array(a) => collect(&a.elem, family, out),
            _ => {}
        }
    }
    let mut deps = Vec::new();
    collect(ty, family, &mut deps);
    deps
}

/// Build the expression that copies a carried field value from a previous
/// version's struct into the current one. For types whose path roots a
/// family-internal struct (directly or wrapped in `Option<...>` / `Vec<...>`)
/// the expression calls `.into()` / `.map(Into::into)` / `.into_iter().map(...).collect()`
/// so the value bridges to the current version's namespace. For everything
/// else (primitives, std, user-defined-outside-family) the expression is the
/// value verbatim — no conversion needed.
fn carry_expr(
    ty: &Type,
    value: TokenStream,
    family_struct_names: &HashSet<String>,
) -> TokenStream {
    if type_involves_family_struct(ty, family_struct_names) {
        if let Some(inner) = generic_inner(ty, "Option") {
            if type_involves_family_struct(inner, family_struct_names) {
                let inner_expr = carry_expr(inner, quote! { __inner }, family_struct_names);
                return quote! { (#value).map(|__inner| #inner_expr) };
            }
            return quote! { (#value) };
        }
        if let Some(inner) = generic_inner(ty, "Vec") {
            if type_involves_family_struct(inner, family_struct_names) {
                let inner_expr = carry_expr(inner, quote! { __inner }, family_struct_names);
                return quote! {
                    (#value).into_iter().map(|__inner| #inner_expr).collect()
                };
            }
            return quote! { (#value) };
        }
        // Bare family struct (e.g. `Connection`, `Brand`). Need `.into()` to
        // cross the version boundary.
        return quote! { ::core::convert::Into::into(#value) };
    }
    // Type doesn't reference any family struct → values are identical between
    // versions, copy verbatim.
    value
}

/// True if `ty` is, or contains as a generic argument, a struct declared in
/// the family. Walks one level deep through `Option<T>` and `Vec<T>` — deeper
/// nesting (e.g. `Vec<Option<Brand>>`) is also handled because the walker
/// recurses into the inner type. Anything more exotic returns `true` if the
/// containing path mentions a family name, leaving the user to write a manual
/// `From` if the auto-emit's wrapper handling doesn't fit.
fn type_involves_family_struct(ty: &Type, family_struct_names: &HashSet<String>) -> bool {
    match ty {
        Type::Path(tp) => {
            for seg in &tp.path.segments {
                let name = seg.ident.to_string();
                if family_struct_names.contains(&name) {
                    return true;
                }
                if let PathArguments::AngleBracketed(args) = &seg.arguments {
                    for arg in &args.args {
                        if let GenericArgument::Type(inner) = arg {
                            if type_involves_family_struct(inner, family_struct_names) {
                                return true;
                            }
                        }
                    }
                }
            }
            false
        }
        Type::Reference(r) => type_involves_family_struct(&r.elem, family_struct_names),
        Type::Tuple(t) => t
            .elems
            .iter()
            .any(|e| type_involves_family_struct(e, family_struct_names)),
        Type::Array(a) => type_involves_family_struct(&a.elem, family_struct_names),
        _ => false,
    }
}

/// If `ty` is `<wrapper_name><...angle args...>` with a single type argument,
/// returns the inner type. Used to peel `Option<T>` / `Vec<T>` for smart
/// conversion expressions.
fn generic_inner<'a>(ty: &'a Type, wrapper_name: &str) -> Option<&'a Type> {
    let Type::Path(tp) = ty else { return None };
    let last = tp.path.segments.last()?;
    if last.ident != wrapper_name {
        return None;
    }
    let PathArguments::AngleBracketed(args) = &last.arguments else {
        return None;
    };
    for arg in &args.args {
        if let GenericArgument::Type(inner) = arg {
            return Some(inner);
        }
    }
    None
}

struct CodecArtifact {
    parse_fn: TokenStream,
    serialize_method: TokenStream,
}

fn emit_codec(decl: &CodecDecl, versions: &[ResolvedVersion], main: &Ident) -> CodecArtifact {
    match decl.codec {
        Codec::Rkyv => emit_rkyv(decl, versions, main),
        Codec::Bincode => emit_bincode(decl, versions, main),
        Codec::Postcard => emit_postcard(decl, versions, main),
    }
}

fn emit_rkyv(decl: &CodecDecl, versions: &[ResolvedVersion], main: &Ident) -> CodecArtifact {
    if cfg!(feature = "rkyv-08") {
        emit_rkyv_08(versions, main)
    } else if cfg!(feature = "rkyv-07") {
        emit_rkyv_07(versions, main)
    } else {
        emit_missing_feature(decl.span, "rkyv", "rkyv-08", &["rkyv-08", "rkyv-07"])
    }
}

fn emit_rkyv_08(versions: &[ResolvedVersion], main: &Ident) -> CodecArtifact {
    let parse_arms = versions.iter().map(|v| {
        let variant = pascal_variant_for(&v.module_ident);
        let module = &v.module_ident;
        let value = v.version.value();
        quote! {
            #value => ::rkyv::from_bytes::<#module::#main, ::rkyv::rancor::Error>(bytes)
                .map(AnyVersion::#variant)
                .map_err(::polyvers::Error::format),
        }
    });
    let ser_arms = versions.iter().map(|v| {
        let variant = pascal_variant_for(&v.module_ident);
        quote! {
            AnyVersion::#variant(value) => ::rkyv::to_bytes::<::rkyv::rancor::Error>(value)
                .map(|av| av.to_vec())
                .map_err(::polyvers::Error::format),
        }
    });
    CodecArtifact {
        parse_fn: quote! {
            pub fn parse_at_version_rkyv(
                version: &str,
                bytes: &[u8],
            ) -> ::core::result::Result<AnyVersion, ::polyvers::Error> {
                match version {
                    #(#parse_arms)*
                    other => ::core::result::Result::Err(
                        ::polyvers::Error::unknown_version(other, VERSIONS)
                    ),
                }
            }
        },
        serialize_method: quote! {
            pub fn to_rkyv_bytes(&self) -> ::core::result::Result<::std::vec::Vec<u8>, ::polyvers::Error> {
                match self {
                    #(#ser_arms)*
                }
            }
        },
    }
}

fn emit_rkyv_07(versions: &[ResolvedVersion], main: &Ident) -> CodecArtifact {
    let parse_arms = versions.iter().map(|v| {
        let variant = pascal_variant_for(&v.module_ident);
        let module = &v.module_ident;
        let value = v.version.value();
        quote! {
            #value => ::rkyv::from_bytes::<#module::#main>(bytes)
                .map(AnyVersion::#variant)
                .map_err(|e| ::polyvers::Error::format_str(::std::format!("{e}"))),
        }
    });
    let ser_arms = versions.iter().map(|v| {
        let variant = pascal_variant_for(&v.module_ident);
        quote! {
            AnyVersion::#variant(value) => ::rkyv::to_bytes::<_, 256>(value)
                .map(|av| av.to_vec())
                .map_err(|e| ::polyvers::Error::format_str(::std::format!("{e}"))),
        }
    });
    CodecArtifact {
        parse_fn: quote! {
            pub fn parse_at_version_rkyv(
                version: &str,
                bytes: &[u8],
            ) -> ::core::result::Result<AnyVersion, ::polyvers::Error> {
                match version {
                    #(#parse_arms)*
                    other => ::core::result::Result::Err(
                        ::polyvers::Error::unknown_version(other, VERSIONS)
                    ),
                }
            }
        },
        serialize_method: quote! {
            pub fn to_rkyv_bytes(&self) -> ::core::result::Result<::std::vec::Vec<u8>, ::polyvers::Error> {
                match self {
                    #(#ser_arms)*
                }
            }
        },
    }
}

fn emit_bincode(decl: &CodecDecl, versions: &[ResolvedVersion], main: &Ident) -> CodecArtifact {
    if !cfg!(feature = "bincode-2") {
        return emit_missing_feature(decl.span, "bincode", "bincode-2", &["bincode-2"]);
    }
    let parse_arms = versions.iter().map(|v| {
        let variant = pascal_variant_for(&v.module_ident);
        let module = &v.module_ident;
        let value = v.version.value();
        quote! {
            #value => ::bincode::serde::decode_from_slice::<#module::#main, _>(
                bytes, ::bincode::config::standard()
            )
                .map(|(v, _)| AnyVersion::#variant(v))
                .map_err(::polyvers::Error::format),
        }
    });
    let ser_arms = versions.iter().map(|v| {
        let variant = pascal_variant_for(&v.module_ident);
        quote! {
            AnyVersion::#variant(value) => ::bincode::serde::encode_to_vec(
                value, ::bincode::config::standard()
            ).map_err(::polyvers::Error::format),
        }
    });
    CodecArtifact {
        parse_fn: quote! {
            pub fn parse_at_version_bincode(
                version: &str,
                bytes: &[u8],
            ) -> ::core::result::Result<AnyVersion, ::polyvers::Error> {
                match version {
                    #(#parse_arms)*
                    other => ::core::result::Result::Err(
                        ::polyvers::Error::unknown_version(other, VERSIONS)
                    ),
                }
            }
        },
        serialize_method: quote! {
            pub fn to_bincode_bytes(&self) -> ::core::result::Result<::std::vec::Vec<u8>, ::polyvers::Error> {
                match self {
                    #(#ser_arms)*
                }
            }
        },
    }
}

fn emit_postcard(decl: &CodecDecl, versions: &[ResolvedVersion], main: &Ident) -> CodecArtifact {
    if !cfg!(feature = "postcard-1") {
        return emit_missing_feature(decl.span, "postcard", "postcard-1", &["postcard-1"]);
    }
    let parse_arms = versions.iter().map(|v| {
        let variant = pascal_variant_for(&v.module_ident);
        let module = &v.module_ident;
        let value = v.version.value();
        quote! {
            #value => ::postcard::from_bytes::<#module::#main>(bytes)
                .map(AnyVersion::#variant)
                .map_err(::polyvers::Error::format),
        }
    });
    let ser_arms = versions.iter().map(|v| {
        let variant = pascal_variant_for(&v.module_ident);
        quote! {
            AnyVersion::#variant(value) => ::postcard::to_allocvec(value)
                .map_err(::polyvers::Error::format),
        }
    });
    CodecArtifact {
        parse_fn: quote! {
            pub fn parse_at_version_postcard(
                version: &str,
                bytes: &[u8],
            ) -> ::core::result::Result<AnyVersion, ::polyvers::Error> {
                match version {
                    #(#parse_arms)*
                    other => ::core::result::Result::Err(
                        ::polyvers::Error::unknown_version(other, VERSIONS)
                    ),
                }
            }
        },
        serialize_method: quote! {
            pub fn to_postcard_bytes(&self) -> ::core::result::Result<::std::vec::Vec<u8>, ::polyvers::Error> {
                match self {
                    #(#ser_arms)*
                }
            }
        },
    }
}

fn emit_missing_feature(
    span: proc_macro2::Span,
    codec: &str,
    default_feature: &str,
    all_features: &[&str],
) -> CodecArtifact {
    let feature_list = all_features
        .iter()
        .map(|f| format!("`{f}`"))
        .collect::<Vec<_>>()
        .join(" or ");
    let message = format!(
        "polyvers: `codec {codec};` requires one of the {feature_list} features on the \
         `polyvers` crate. Add `features = [\"{default_feature}\"]` to your Cargo.toml \
         dependency on `polyvers`."
    );
    let err = syn::Error::new(span, message).to_compile_error();
    CodecArtifact {
        parse_fn: err.clone(),
        serialize_method: err,
    }
}

fn emit_version_module(
    v: &ResolvedVersion,
    main_struct_name: &Ident,
    derive_attr: &TokenStream,
    meta_type: Option<&Path>,
) -> TokenStream {
    let module = &v.module_ident;
    let structs = v.structs.iter().map(|s| emit_struct(s, derive_attr));
    let meta_fn = match (meta_type, &v.meta) {
        (Some(ty), Some(init)) => emit_meta_fn(ty, init),
        _ => quote! {},
    };

    // Emit `pub const FIELD_COUNT: usize = N` for the family's main struct so
    // downstream parsers can dispatch on the on-wire length without hand-coding
    // the count alongside the macro invocation.
    let field_count_decl = v
        .structs
        .iter()
        .find(|s| &s.name == main_struct_name)
        .map(|s| {
            let count = s.fields.len();
            quote! { pub const FIELD_COUNT: usize = #count; }
        })
        .unwrap_or_else(|| quote! {});

    quote! {
        pub mod #module {
            #![allow(unused_imports)]
            use super::*;

            #(#structs)*

            #field_count_decl

            #meta_fn
        }
    }
}

fn emit_meta_fn(meta_type: &Path, init: &MetaInit) -> TokenStream {
    let inits = init.fields.iter().map(|f| {
        let name = &f.name;
        let value = &f.value;
        quote! { #name: #value }
    });
    quote! {
        pub fn meta() -> #meta_type {
            #meta_type {
                #(#inits,)*
            }
        }
    }
}

fn emit_struct(s: &ResolvedStruct, derive_attr: &TokenStream) -> TokenStream {
    let name = &s.name;
    let fields = s.fields.iter().map(|f| {
        let attrs = &f.attrs;
        let fname = &f.name;
        let ty = &f.ty;
        quote! {
            #(#attrs)*
            pub #fname: #ty
        }
    });
    quote! {
        #derive_attr
        pub struct #name {
            #(#fields,)*
        }
    }
}

fn pascal_variant_for(module_ident: &Ident) -> Ident {
    let s = module_ident.to_string();
    let mut chars = s.chars();
    let first = chars.next().expect("module ident is non-empty");
    let rest: String = chars.collect();
    let pascal = format!("{}{}", first.to_uppercase(), rest);
    Ident::new(&pascal, module_ident.span())
}

fn build_any_derives(spec_derives: &[Path]) -> TokenStream {
    let mut chosen = Vec::new();
    for name in ["Debug", "Clone", "PartialEq", "Eq", "Hash"] {
        if last_segment_is(spec_derives, name) {
            let id = format_ident!("{}", name);
            chosen.push(id);
        }
    }
    if chosen.is_empty() {
        quote! {}
    } else {
        quote! { #[derive(#(#chosen),*)] }
    }
}

fn last_segment_is(paths: &[Path], name: &str) -> bool {
    paths.iter().any(|p| {
        p.segments
            .last()
            .map(|seg| seg.ident == name)
            .unwrap_or(false)
    })
}
