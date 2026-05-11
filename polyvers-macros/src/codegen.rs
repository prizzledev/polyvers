use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{Ident, Path};

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
        .map(|v| emit_version_module(v, &derive_attr, meta_type.as_ref()));

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

    quote! {
        pub mod #family {
            #(#version_modules)*

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
    derive_attr: &TokenStream,
    meta_type: Option<&Path>,
) -> TokenStream {
    let module = &v.module_ident;
    let structs = v.structs.iter().map(|s| emit_struct(s, derive_attr));
    let meta_fn = match (meta_type, &v.meta) {
        (Some(ty), Some(init)) => emit_meta_fn(ty, init),
        _ => quote! {},
    };
    quote! {
        pub mod #module {
            #![allow(unused_imports)]
            use super::*;

            #(#structs)*

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
