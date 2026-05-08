use std::collections::{HashMap, HashSet};

use proc_macro2::Span;
use syn::{Attribute, Ident, LitStr, Path, Type};

use crate::parse::{MetaInit, StructDef, StructItem, VersionDef, VersionedSpec};

pub struct ResolvedSpec {
    pub family: Ident,
    pub derives: Vec<Path>,
    pub meta_type: Option<Path>,
    pub versions: Vec<ResolvedVersion>,
}

#[derive(Clone)]
pub struct ResolvedVersion {
    pub version: LitStr,
    pub module_ident: Ident,
    pub meta: Option<MetaInit>,
    pub structs: Vec<ResolvedStruct>,
}

#[derive(Clone)]
pub struct ResolvedStruct {
    pub name: Ident,
    pub fields: Vec<ResolvedField>,
}

#[derive(Clone)]
pub struct ResolvedField {
    pub attrs: Vec<Attribute>,
    pub name: Ident,
    pub ty: Type,
}

pub fn resolve(spec: VersionedSpec) -> Result<ResolvedSpec, syn::Error> {
    let mut errors: Vec<syn::Error> = Vec::new();

    let mut seen_versions: HashMap<String, Span> = HashMap::new();
    let mut seen_idents: HashMap<String, (LitStr, Span)> = HashMap::new();
    for v in &spec.versions {
        let value = v.version.value();
        if let Some(prior_span) = seen_versions.get(&value) {
            errors.push(syn::Error::new(
                v.version.span(),
                format!("version `\"{value}\"` declared twice"),
            ));
            let _ = prior_span;
        } else {
            seen_versions.insert(value.clone(), v.version.span());
        }

        let ident_str = mangle_version_string(&value);
        if let Some((other_lit, _)) = seen_idents.get(&ident_str) {
            if other_lit.value() != value {
                errors.push(syn::Error::new(
                    v.version.span(),
                    format!(
                        "version `\"{value}\"` collides with `\"{}\"`: both mangle to module ident `{ident_str}`",
                        other_lit.value()
                    ),
                ));
            }
        } else {
            seen_idents.insert(ident_str, (v.version.clone(), v.version.span()));
        }
    }

    for (i, v) in spec.versions.iter().enumerate() {
        if let Some(extends) = &v.extends {
            let target = extends.value();
            let found_before = spec.versions[..i]
                .iter()
                .any(|earlier| earlier.version.value() == target);
            if !found_before {
                let exists_after = spec.versions[i + 1..]
                    .iter()
                    .any(|later| later.version.value() == target);
                let msg = if exists_after {
                    format!(
                        "extends `\"{target}\"`: that version is declared after this one. \
                         Reorder so the parent comes first."
                    )
                } else {
                    format!("extends `\"{target}\"`: no version with that name was declared")
                };
                errors.push(syn::Error::new(extends.span(), msg));
            }
        } else if i == 0 {
            // base version, no parent — fine
        }
    }

    if spec.meta_type.is_some() {
        for v in &spec.versions {
            if v.meta.is_none() {
                errors.push(syn::Error::new(
                    v.version.span(),
                    format!(
                        "version `\"{}\"` is missing its `meta {{ … }}` block (the macro \
                         declared a metadata type at the top, so every version must \
                         provide one)",
                        v.version.value()
                    ),
                ));
            }
        }
    } else {
        for v in &spec.versions {
            if let Some(meta) = &v.meta {
                errors.push(syn::Error::new(
                    meta.kw_span,
                    "this version has a `meta { … }` block but no `meta TypeName;` was \
                     declared at the top level",
                ));
            }
        }
    }

    let mut resolved: Vec<ResolvedVersion> = Vec::with_capacity(spec.versions.len());
    for (i, v) in spec.versions.iter().enumerate() {
        let parent: Option<&ResolvedVersion> = if i == 0 && v.extends.is_none() {
            None
        } else if let Some(extends) = &v.extends {
            let target = extends.value();
            spec.versions[..i]
                .iter()
                .position(|earlier| earlier.version.value() == target)
                .and_then(|idx| resolved.get(idx))
        } else {
            resolved.last()
        };

        let rv = resolve_version(v, parent, &mut errors);
        resolved.push(rv);
    }

    if let Some(combined) = combine_errors(errors) {
        return Err(combined);
    }
    Ok(ResolvedSpec {
        family: spec.family,
        derives: spec.derives,
        meta_type: spec.meta_type,
        versions: resolved,
    })
}

fn resolve_version(
    version: &VersionDef,
    parent: Option<&ResolvedVersion>,
    errors: &mut Vec<syn::Error>,
) -> ResolvedVersion {
    let mut structs: Vec<ResolvedStruct> = parent.map(|p| p.structs.clone()).unwrap_or_default();

    let mut seen_in_version: HashSet<String> = HashSet::new();

    for sd in &version.structs {
        let name_str = sd.name.to_string();
        if !seen_in_version.insert(name_str.clone()) {
            errors.push(syn::Error::new(
                sd.name.span(),
                format!(
                    "struct `{}` declared twice in version `\"{}\"`",
                    sd.name,
                    version.version.value()
                ),
            ));
            continue;
        }

        let existing_index = structs.iter().position(|s| s.name == sd.name);
        match existing_index {
            Some(idx) => {
                let parent_fields = structs[idx].fields.clone();
                let new_fields = apply_mutations(sd, &parent_fields, errors);
                structs[idx].fields = new_fields;
            }
            None => {
                let fields = collect_base_fields(sd, errors);
                structs.push(ResolvedStruct {
                    name: sd.name.clone(),
                    fields,
                });
            }
        }
    }

    ResolvedVersion {
        version: version.version.clone(),
        module_ident: mangle_version_to_ident(&version.version),
        meta: version.meta.clone(),
        structs,
    }
}

fn collect_base_fields(sd: &StructDef, errors: &mut Vec<syn::Error>) -> Vec<ResolvedField> {
    let mut fields: Vec<ResolvedField> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for item in &sd.items {
        match item {
            StructItem::Field { attrs, name, ty } => {
                if !seen.insert(name.to_string()) {
                    errors.push(syn::Error::new(
                        name.span(),
                        format!("duplicate field `{name}` in struct `{}`", sd.name),
                    ));
                    continue;
                }
                fields.push(ResolvedField {
                    attrs: attrs.clone(),
                    name: name.clone(),
                    ty: ty.clone(),
                });
            }
            StructItem::Add { .. } | StructItem::Edit { .. } | StructItem::Delete { .. } => {
                errors.push(syn::Error::new(
                    item.span(),
                    format!(
                        "struct `{}` is being declared for the first time here; \
                         mutations (`#[add]`, `#[edit]`, `#[delete]`) cannot be used. \
                         Declare fields directly.",
                        sd.name
                    ),
                ));
            }
        }
    }
    fields
}

fn apply_mutations(
    sd: &StructDef,
    parent_fields: &[ResolvedField],
    errors: &mut Vec<syn::Error>,
) -> Vec<ResolvedField> {
    let mut fields: Vec<ResolvedField> = parent_fields.to_vec();
    let mut seen_mutations: HashSet<String> = HashSet::new();
    for item in &sd.items {
        match item {
            StructItem::Field { name, .. } => {
                errors.push(syn::Error::new(
                    name.span(),
                    format!(
                        "struct `{}` already exists in a previous version; \
                         field declarations are not allowed here. \
                         Use `#[add]`, `#[edit]`, or `#[delete]`.",
                        sd.name
                    ),
                ));
            }
            StructItem::Add {
                attrs, name, ty, ..
            } => {
                if !seen_mutations.insert(name.to_string()) {
                    errors.push(syn::Error::new(
                        name.span(),
                        format!("field `{name}` mutated more than once in this version"),
                    ));
                    continue;
                }
                if fields.iter().any(|f| f.name == *name) {
                    errors.push(syn::Error::new(
                        name.span(),
                        format!(
                            "field `{name}` already exists in the parent version; \
                             use `#[edit]` to change it"
                        ),
                    ));
                    continue;
                }
                fields.push(ResolvedField {
                    attrs: attrs.clone(),
                    name: name.clone(),
                    ty: ty.clone(),
                });
            }
            StructItem::Edit {
                attrs, name, ty, ..
            } => {
                if !seen_mutations.insert(name.to_string()) {
                    errors.push(syn::Error::new(
                        name.span(),
                        format!("field `{name}` mutated more than once in this version"),
                    ));
                    continue;
                }
                match fields.iter_mut().find(|f| f.name == *name) {
                    Some(f) => {
                        f.ty = ty.clone();
                        f.attrs = attrs.clone();
                    }
                    None => {
                        errors.push(syn::Error::new(
                            name.span(),
                            format!(
                                "field `{name}` does not exist in the parent version; \
                                 use `#[add]` to introduce it"
                            ),
                        ));
                    }
                }
            }
            StructItem::Delete { name, .. } => {
                if !seen_mutations.insert(name.to_string()) {
                    errors.push(syn::Error::new(
                        name.span(),
                        format!("field `{name}` mutated more than once in this version"),
                    ));
                    continue;
                }
                let len_before = fields.len();
                fields.retain(|f| f.name != *name);
                if fields.len() == len_before {
                    errors.push(syn::Error::new(
                        name.span(),
                        format!(
                            "field `{name}` does not exist in the parent version; \
                             nothing to delete"
                        ),
                    ));
                }
            }
        }
    }
    fields
}

fn combine_errors(errors: Vec<syn::Error>) -> Option<syn::Error> {
    let mut iter = errors.into_iter();
    let mut combined = iter.next()?;
    for e in iter {
        combined.combine(e);
    }
    Some(combined)
}

fn mangle_version_string(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() + 1);
    out.push('v');
    for c in raw.chars() {
        match c {
            '.' => out.push('_'),
            '-' => out.push_str("__"),
            c if c.is_ascii_alphanumeric() => out.push(c.to_ascii_lowercase()),
            _ => out.push('_'),
        }
    }
    out
}

pub fn mangle_version_to_ident(v: &LitStr) -> Ident {
    let mangled = mangle_version_string(&v.value());
    Ident::new(&mangled, v.span())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::VersionedSpec;
    use quote::quote;

    fn parse(tokens: proc_macro2::TokenStream) -> VersionedSpec {
        syn::parse2(tokens).expect("parses")
    }

    #[test]
    fn resolves_canonical_example() {
        let spec = parse(quote! {
            family the_config;
            derive(Debug, Clone);

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
        });
        let resolved = resolve(spec).expect("resolves");
        assert_eq!(resolved.versions.len(), 2);

        let v01 = &resolved.versions[0];
        assert_eq!(v01.version.value(), "0.1");
        assert_eq!(v01.module_ident.to_string(), "v0_1");
        assert_eq!(v01.structs.len(), 2);
        let main_v01 = &v01.structs[0];
        assert_eq!(main_v01.name.to_string(), "Main");
        let names: Vec<_> = main_v01.fields.iter().map(|f| f.name.to_string()).collect();
        assert_eq!(names, vec!["some_prop", "sub", "another"]);

        let v02 = &resolved.versions[1];
        assert_eq!(v02.module_ident.to_string(), "v0_2");
        let main_v02 = &v02.structs[0];
        let names: Vec<_> = main_v02.fields.iter().map(|f| f.name.to_string()).collect();
        assert_eq!(names, vec!["sub", "another", "added"]);
        let just_v02 = &v02.structs[1];
        assert_eq!(just_v02.fields[0].name.to_string(), "prop");
    }

    #[test]
    fn duplicate_version_is_error() {
        let spec = parse(quote! {
            family x;
            version "0.1" { struct M { a: u32 } }
            version "0.1" extends "0.1" { struct M { #[add] b: u32 } }
        });
        let err = resolve(spec).err().unwrap();
        assert!(err.to_string().contains("declared twice"));
    }

    #[test]
    fn dangling_extends_is_error() {
        let spec = parse(quote! {
            family x;
            version "0.1" { struct M { a: u32 } }
            version "0.2" extends "9.9" { struct M { #[add] b: u32 } }
        });
        let err = resolve(spec).err().unwrap();
        assert!(err.to_string().contains("no version with that name"));
    }

    #[test]
    fn forward_extends_is_error() {
        let spec = parse(quote! {
            family x;
            version "0.1" extends "0.2" { struct M { a: u32 } }
            version "0.2" { struct M { b: u32 } }
        });
        let err = resolve(spec).err().unwrap();
        assert!(err.to_string().contains("declared after"));
    }

    #[test]
    fn mutation_in_base_is_error() {
        let spec = parse(quote! {
            family x;
            version "0.1" { struct M { #[add] a: u32 } }
        });
        let err = resolve(spec).err().unwrap();
        assert!(err.to_string().contains("declared for the first time"));
    }

    #[test]
    fn base_field_in_diff_is_error() {
        let spec = parse(quote! {
            family x;
            version "0.1" { struct M { a: u32 } }
            version "0.2" extends "0.1" { struct M { b: u32 } }
        });
        let err = resolve(spec).err().unwrap();
        assert!(
            err.to_string()
                .contains("already exists in a previous version")
        );
    }

    #[test]
    fn add_existing_is_error() {
        let spec = parse(quote! {
            family x;
            version "0.1" { struct M { a: u32 } }
            version "0.2" extends "0.1" { struct M { #[add] a: u64 } }
        });
        let err = resolve(spec).err().unwrap();
        assert!(err.to_string().contains("already exists in the parent"));
    }

    #[test]
    fn edit_missing_is_error() {
        let spec = parse(quote! {
            family x;
            version "0.1" { struct M { a: u32 } }
            version "0.2" extends "0.1" { struct M { #[edit] missing: u64 } }
        });
        let err = resolve(spec).err().unwrap();
        assert!(err.to_string().contains("does not exist"));
    }

    #[test]
    fn delete_missing_is_error() {
        let spec = parse(quote! {
            family x;
            version "0.1" { struct M { a: u32 } }
            version "0.2" extends "0.1" { struct M { #[delete] missing } }
        });
        let err = resolve(spec).err().unwrap();
        assert!(err.to_string().contains("does not exist"));
    }

    #[test]
    fn duplicate_struct_in_version_is_error() {
        let spec = parse(quote! {
            family x;
            version "0.1" { struct M { a: u32 } struct M { b: u32 } }
        });
        let err = resolve(spec).err().unwrap();
        assert!(err.to_string().contains("declared twice"));
    }

    #[test]
    fn mangling_collision_is_error() {
        let spec = parse(quote! {
            family x;
            version "0.1" { struct M { a: u32 } }
            version "0_1" extends "0.1" { struct M { #[add] b: u32 } }
        });
        let err = resolve(spec).err().unwrap();
        assert!(err.to_string().contains("collides"));
    }

    #[test]
    fn implicit_extends_inherits_from_previous() {
        let spec = parse(quote! {
            family x;
            version "0.1" { struct M { a: u32 } }
            version "0.2" { struct M { #[add] b: u32 } }
        });
        let resolved = resolve(spec).expect("resolves with implicit extends");
        let names: Vec<_> = resolved.versions[1].structs[0]
            .fields
            .iter()
            .map(|f| f.name.to_string())
            .collect();
        assert_eq!(names, vec!["a", "b"]);
    }

    #[test]
    fn multiple_errors_combined() {
        let spec = parse(quote! {
            family x;
            version "0.1" { struct M { a: u32 } }
            version "0.2" extends "0.1" {
                struct M {
                    #[edit] missing_a: u64,
                    #[delete] missing_b,
                }
            }
        });
        let err = resolve(spec).err().unwrap();
        let s = err.to_string();
        // combine surfaces both messages
        assert!(s.contains("missing_a") || err.into_iter().count() >= 2);
    }

    #[test]
    fn struct_unmentioned_at_v2_inherits() {
        let spec = parse(quote! {
            family x;
            version "0.1" { struct A { x: u32 } struct B { y: u32 } }
            version "0.2" extends "0.1" { struct A { #[add] z: u32 } }
        });
        let resolved = resolve(spec).expect("ok");
        let v2 = &resolved.versions[1];
        let b = v2
            .structs
            .iter()
            .find(|s| s.name == "B")
            .expect("B inherited");
        let names: Vec<_> = b.fields.iter().map(|f| f.name.to_string()).collect();
        assert_eq!(names, vec!["y"]);
    }

    #[test]
    fn new_struct_in_diff_version_allowed() {
        let spec = parse(quote! {
            family x;
            version "0.1" { struct A { x: u32 } }
            version "0.2" extends "0.1" {
                struct A { #[add] z: u32 }
                struct B { q: String }
            }
        });
        let resolved = resolve(spec).expect("ok");
        let v2 = &resolved.versions[1];
        assert!(v2.structs.iter().any(|s| s.name == "B"));
    }

    #[test]
    fn meta_required_when_type_declared() {
        let spec = parse(quote! {
            family x;
            meta Info;
            version "0.1" {
                meta { a: 1 }
                struct Main { x: u32 }
            }
            version "0.2" extends "0.1" {
                struct Main { #[add] y: u32 }
            }
        });
        let err = resolve(spec).err().unwrap();
        assert!(err.to_string().contains("missing its `meta"));
    }

    #[test]
    fn meta_block_without_top_level_type_is_error() {
        let spec = parse(quote! {
            family x;
            version "0.1" {
                meta { a: 1 }
                struct Main { x: u32 }
            }
        });
        let err = resolve(spec).err().unwrap();
        assert!(err.to_string().contains("no `meta TypeName"));
    }

    #[test]
    fn meta_value_threads_through_to_resolved() {
        let spec = parse(quote! {
            family x;
            meta Info;
            version "0.1" {
                meta { a: 42 }
                struct Main { x: u32 }
            }
        });
        let resolved = resolve(spec).expect("ok");
        assert!(resolved.meta_type.is_some());
        let v01 = &resolved.versions[0];
        let meta = v01.meta.as_ref().expect("meta preserved");
        assert_eq!(meta.fields.len(), 1);
        assert_eq!(meta.fields[0].name.to_string(), "a");
    }

    #[test]
    fn mangle_examples() {
        assert_eq!(mangle_version_string("0.1"), "v0_1");
        assert_eq!(mangle_version_string("0.1.0"), "v0_1_0");
        assert_eq!(mangle_version_string("1.0-rc1"), "v1_0__rc1");
    }
}
