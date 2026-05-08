use proc_macro2::Span;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{Attribute, Expr, Ident, LitStr, Path, Token, Type, braced, parenthesized};

mod kw {
    syn::custom_keyword!(family);
    syn::custom_keyword!(version);
    syn::custom_keyword!(extends);
    syn::custom_keyword!(derive);
    syn::custom_keyword!(meta);
}

pub struct VersionedSpec {
    pub family: Ident,
    pub derives: Vec<Path>,
    pub meta_type: Option<Path>,
    pub versions: Vec<VersionDef>,
}

pub struct VersionDef {
    pub version: LitStr,
    pub extends: Option<LitStr>,
    pub meta: Option<MetaInit>,
    pub structs: Vec<StructDef>,
}

#[derive(Clone)]
pub struct MetaInit {
    pub kw_span: Span,
    pub fields: Vec<MetaField>,
}

#[derive(Clone)]
pub struct MetaField {
    pub name: Ident,
    pub value: Expr,
}

impl Parse for MetaField {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let name: Ident = input.parse()?;
        let _: Token![:] = input.parse()?;
        let value: Expr = input.parse()?;
        Ok(MetaField { name, value })
    }
}

pub struct StructDef {
    pub name: Ident,
    pub items: Vec<StructItem>,
}

pub enum StructItem {
    Field {
        attrs: Vec<Attribute>,
        name: Ident,
        ty: Type,
    },
    Add {
        marker_span: Span,
        attrs: Vec<Attribute>,
        name: Ident,
        ty: Type,
    },
    Edit {
        marker_span: Span,
        attrs: Vec<Attribute>,
        name: Ident,
        ty: Type,
    },
    Delete {
        marker_span: Span,
        name: Ident,
    },
}

impl StructItem {
    pub fn span(&self) -> Span {
        match self {
            StructItem::Field { name, .. } => name.span(),
            StructItem::Add { marker_span, .. }
            | StructItem::Edit { marker_span, .. }
            | StructItem::Delete { marker_span, .. } => *marker_span,
        }
    }
}

impl Parse for VersionedSpec {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let _: kw::family = input.parse()?;
        let family: Ident = input.parse()?;
        let _: Token![;] = input.parse()?;

        let mut derives = Vec::new();
        if input.peek(kw::derive) {
            let _: kw::derive = input.parse()?;
            let content;
            parenthesized!(content in input);
            let paths: Punctuated<Path, Token![,]> = Punctuated::parse_terminated(&content)?;
            derives.extend(paths);
            let _: Token![;] = input.parse()?;
        }

        let meta_type = if input.peek(kw::meta) {
            let _: kw::meta = input.parse()?;
            let path: Path = input.parse()?;
            let _: Token![;] = input.parse()?;
            Some(path)
        } else {
            None
        };

        let mut versions = Vec::new();
        while !input.is_empty() {
            versions.push(input.parse::<VersionDef>()?);
        }
        if versions.is_empty() {
            return Err(input.error("expected at least one `version \"…\" { … }` block"));
        }
        Ok(VersionedSpec {
            family,
            derives,
            meta_type,
            versions,
        })
    }
}

impl Parse for VersionDef {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let _: kw::version = input.parse()?;
        let version: LitStr = input.parse()?;
        let extends = if input.peek(kw::extends) {
            let _: kw::extends = input.parse()?;
            Some(input.parse::<LitStr>()?)
        } else {
            None
        };
        let body;
        braced!(body in input);
        let mut structs = Vec::new();
        let mut meta: Option<MetaInit> = None;
        while !body.is_empty() {
            if body.peek(kw::meta) {
                let kw: kw::meta = body.parse()?;
                if meta.is_some() {
                    return Err(syn::Error::new(
                        kw.span,
                        "duplicate `meta { … }` block in this version",
                    ));
                }
                let inner;
                braced!(inner in body);
                let fields: Punctuated<MetaField, Token![,]> =
                    Punctuated::parse_terminated(&inner)?;
                meta = Some(MetaInit {
                    kw_span: kw.span,
                    fields: fields.into_iter().collect(),
                });
            } else {
                structs.push(body.parse::<StructDef>()?);
            }
        }
        Ok(VersionDef {
            version,
            extends,
            meta,
            structs,
        })
    }
}

impl Parse for StructDef {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let _: Token![struct] = input.parse()?;
        let name: Ident = input.parse()?;
        let body;
        braced!(body in input);
        let items: Punctuated<StructItem, Token![,]> = Punctuated::parse_terminated(&body)?;
        Ok(StructDef {
            name,
            items: items.into_iter().collect(),
        })
    }
}

impl Parse for StructItem {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let attrs = input.call(Attribute::parse_outer)?;

        let mut marker: Option<Marker> = None;
        let mut other_attrs = Vec::new();
        for attr in attrs {
            match match_marker(&attr)? {
                Some(found) => {
                    if marker.is_some() {
                        return Err(syn::Error::new_spanned(
                            attr,
                            "only one of `#[add]`, `#[edit]`, `#[delete]` may appear on a field",
                        ));
                    }
                    marker = Some(found);
                }
                None => other_attrs.push(attr),
            }
        }

        if input.peek(Token![pub]) {
            return Err(input.error(
                "polyvers does not support visibility modifiers on fields; all generated fields are `pub`",
            ));
        }

        match marker {
            None => {
                let name: Ident = input.parse()?;
                let _: Token![:] = input.parse()?;
                let ty: Type = input.parse()?;
                Ok(StructItem::Field {
                    attrs: other_attrs,
                    name,
                    ty,
                })
            }
            Some(Marker::Add(marker_span)) => {
                let name: Ident = input.parse()?;
                let _: Token![:] = input.parse()?;
                let ty: Type = input.parse()?;
                Ok(StructItem::Add {
                    marker_span,
                    attrs: other_attrs,
                    name,
                    ty,
                })
            }
            Some(Marker::Edit(marker_span)) => {
                let name: Ident = input.parse()?;
                let _: Token![:] = input.parse()?;
                let ty: Type = input.parse()?;
                Ok(StructItem::Edit {
                    marker_span,
                    attrs: other_attrs,
                    name,
                    ty,
                })
            }
            Some(Marker::Delete(marker_span)) => {
                if !other_attrs.is_empty() {
                    return Err(syn::Error::new_spanned(
                        &other_attrs[0],
                        "`#[delete]` cannot carry other attributes on the same line",
                    ));
                }
                let name: Ident = input.parse()?;
                if input.peek(Token![:]) {
                    return Err(
                        input.error("`#[delete]` takes a bare identifier; do not include `: Type`")
                    );
                }
                Ok(StructItem::Delete { marker_span, name })
            }
        }
    }
}

enum Marker {
    Add(Span),
    Edit(Span),
    Delete(Span),
}

fn match_marker(attr: &Attribute) -> syn::Result<Option<Marker>> {
    let path = attr.path();
    let kind = if path.is_ident("add") {
        Marker::Add(attr.span())
    } else if path.is_ident("edit") {
        Marker::Edit(attr.span())
    } else if path.is_ident("delete") {
        Marker::Delete(attr.span())
    } else {
        return Ok(None);
    };
    if !matches!(attr.meta, syn::Meta::Path(_)) {
        let name = match &kind {
            Marker::Add(_) => "add",
            Marker::Edit(_) => "edit",
            Marker::Delete(_) => "delete",
        };
        return Err(syn::Error::new_spanned(
            attr,
            format!("`#[{name}]` does not take arguments in polyvers v0.1.0"),
        ));
    }
    Ok(Some(kind))
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote;

    #[test]
    fn parses_canonical_example() {
        let tokens = quote! {
            family the_config;
            derive(Debug, Clone, serde::Serialize, serde::Deserialize);

            version "0.1" {
                struct Main {
                    some_prop: String,
                    sub: JustAStruct,
                    another: Option<usize>,
                }
                struct JustAStruct {
                    prop: String,
                }
            }

            version "0.2" extends "0.1" {
                struct Main {
                    #[add]    added: String,
                    #[edit]   sub: JustAStruct,
                    #[delete] some_prop,
                }
                struct JustAStruct {
                    #[edit] prop: u64,
                }
            }
        };
        let spec: VersionedSpec = syn::parse2(tokens).expect("parses");
        assert_eq!(spec.family.to_string(), "the_config");
        assert_eq!(spec.derives.len(), 4);
        assert_eq!(spec.versions.len(), 2);
        assert_eq!(spec.versions[0].version.value(), "0.1");
        assert!(spec.versions[0].extends.is_none());
        assert_eq!(spec.versions[1].extends.as_ref().unwrap().value(), "0.1");
        assert_eq!(spec.versions[0].structs.len(), 2);
        let main_v01 = &spec.versions[0].structs[0];
        assert_eq!(main_v01.name.to_string(), "Main");
        assert_eq!(main_v01.items.len(), 3);
        assert!(matches!(main_v01.items[0], StructItem::Field { .. }));

        let main_v02 = &spec.versions[1].structs[0];
        assert_eq!(main_v02.items.len(), 3);
        assert!(matches!(main_v02.items[0], StructItem::Add { .. }));
        assert!(matches!(main_v02.items[1], StructItem::Edit { .. }));
        assert!(matches!(main_v02.items[2], StructItem::Delete { .. }));
    }

    #[test]
    fn rejects_double_marker() {
        let tokens = quote! {
            family x;
            version "0.1" { struct M { #[add] #[edit] f: u32 } }
        };
        let err = syn::parse2::<VersionedSpec>(tokens).err().unwrap();
        assert!(err.to_string().contains("only one of"));
    }

    #[test]
    fn rejects_visibility() {
        let tokens = quote! {
            family x;
            version "0.1" { struct M { pub f: u32 } }
        };
        let err = syn::parse2::<VersionedSpec>(tokens).err().unwrap();
        assert!(err.to_string().contains("visibility"));
    }

    #[test]
    fn parses_meta_clause_and_blocks() {
        let tokens = quote! {
            family the_config;
            derive(Debug, Clone);
            meta VersionInfo;

            version "0.1" {
                meta {
                    released: "2024-01-01".to_string(),
                    counter: 1,
                }
                struct Main { a: u32 }
            }

            version "0.2" extends "0.1" {
                struct Main { #[add] b: u32 }
                meta {
                    released: "2024-06-01".to_string(),
                    counter: 2,
                }
            }
        };
        let spec: VersionedSpec = syn::parse2(tokens).expect("parses");
        assert!(spec.meta_type.is_some());
        let v0 = &spec.versions[0];
        let meta = v0.meta.as_ref().expect("meta in v0.1");
        assert_eq!(meta.fields.len(), 2);
        assert_eq!(meta.fields[0].name.to_string(), "released");
        assert_eq!(meta.fields[1].name.to_string(), "counter");
    }

    #[test]
    fn rejects_duplicate_meta_blocks_in_one_version() {
        let tokens = quote! {
            family x;
            meta M;
            version "0.1" {
                meta { a: 1 }
                meta { a: 2 }
                struct M { x: u32 }
            }
        };
        let err = syn::parse2::<VersionedSpec>(tokens).err().unwrap();
        assert!(err.to_string().contains("duplicate"));
    }

    #[test]
    fn rejects_delete_with_type() {
        let tokens = quote! {
            family x;
            version "0.2" extends "0.1" { struct M { #[delete] f: u32 } }
        };
        let err = syn::parse2::<VersionedSpec>(tokens).err().unwrap();
        assert!(err.to_string().contains("bare identifier"));
    }
}
