extern crate proc_macro;

use proc_macro2::TokenStream;
use proc_macro_error::{abort_call_site, proc_macro_error};
use quote::quote;
use syn::*;

#[proc_macro_derive(Cborize, attributes(cbor))]
#[proc_macro_error]
pub fn cborize_type(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input: DeriveInput = syn::parse(input).unwrap();
    let gen = impl_cborize_type(&input);
    gen.into()
}

fn impl_cborize_type(input: &DeriveInput) -> TokenStream {
    let name = &input.ident;
    match &input.data {
        Data::Struct(ast) => match &ast.fields {
            Fields::Named(fields) => {
                let mut ts = from_type_to_cbor(name, fields);
                ts.extend(from_cbor_to_type(name, fields));
                ts
            }
            _ => abort_call_site!("cbor only supports named fields"),
        },
        _ => abort_call_site!("cbor only supports named structs"),
    }
}

fn from_type_to_cbor(name: &Ident, fields: &FieldsNamed) -> TokenStream {
    let mut token_builder = quote! {};
    for field in fields.named.iter() {
        token_builder.extend(to_json_property(field));
    }

    quote! {
        impl ::std::convert::From<#name> for ::jsondata::Json {
            fn from(value: #name) -> ::jsondata::Json {
                let mut props: Vec<::jsondata::Property> = vec![];
                #token_builder;
                ::jsondata::Json::new(props)
            }
        }
    }
}

fn to_json_property(field: &Field) -> TokenStream {
    match &field.ident {
        Some(field_name) => {
            let key = field_name.to_string().to_lowercase();
            let is_from_str = get_from_str(&field.attrs);
            match (is_from_str, get_try_into(&field.attrs)) {
                (true, _) => quote! {
                    let v: Json = value.#field_name.to_string().into();
                    props.push(::jsondata::Property::new(#key, v));
                },
                (false, Some(intr_type)) => quote! {
                    let v: #intr_type = value.#field_name.try_into().unwrap();
                    let v: Json = v.into();
                    props.push(::jsondata::Property::new(#key, v));
                },
                (false, None) => quote! {
                    let v = value.#field_name.into();
                    props.push(::jsondata::Property::new(#key, v));
                },
            }
        }
        None => TokenStream::new(),
    }
}

fn get_from_str(attrs: &[syn::Attribute]) -> bool {
    if attrs.len() == 0 {
        return false;
    }
    match attrs[0].parse_meta().unwrap() {
        syn::Meta::List(meta_list) => {
            let mut iter = meta_list.nested.iter();
            'outer: loop {
                if let Some(syn::NestedMeta::Meta(syn::Meta::Path(p))) = iter.next() {
                    for seg in p.segments.iter() {
                        if seg.ident.to_string() == "from_str" {
                            break 'outer true;
                        } else if seg.ident.to_string() == "to_string" {
                            break 'outer true;
                        }
                    }
                } else {
                    break 'outer false;
                }
            }
        }
        _ => false,
    }
}

fn get_try_into(attrs: &[syn::Attribute]) -> Option<syn::Type> {
    if attrs.len() == 0 {
        return None;
    }
    let nv = match attrs[0].parse_meta().unwrap() {
        syn::Meta::List(meta_list) => {
            let mut iter = meta_list.nested.iter();
            loop {
                match iter.next()? {
                    syn::NestedMeta::Meta(syn::Meta::NameValue(nv)) => break Some(nv.clone()),
                    _ => continue,
                }
            }
        }
        _ => None,
    }?;

    let segs: Vec<&syn::PathSegment> = nv.path.segments.iter().collect();
    if segs.first().unwrap().ident.to_string() == "try_into" {
        Some(match &nv.lit {
            syn::Lit::Str(s) => s.parse().unwrap(),
            _ => panic!("invalid literal"),
        })
    } else {
        None
    }
}

fn from_json_to_type(name: &Ident, fields: &FieldsNamed) -> TokenStream {
    let mut token_builder = quote! {};
    for field in fields.named.iter() {
        token_builder.extend(to_type_field(field));
    }

    quote! {
        impl ::std::convert::TryFrom<::jsondata::Json> for #name {
            type Error = ::jsondata::Error;

            fn try_from(value: ::jsondata::Json) -> ::std::result::Result<#name, Self::Error> {
                use ::std::convert::TryInto;

                Ok(#name {
                    #token_builder
                })
            }
        }
    }
}

fn to_type_field(field: &Field) -> TokenStream {
    match &field.ident {
        Some(field_name) => {
            let key = field_name.to_string().to_lowercase();
            let is_from_str = get_from_str(&field.attrs);
            match (is_from_str, get_try_into(&field.attrs)) {
                (true, _) => quote! {
                    #field_name: {
                        let v: String = match value.get(&("/".to_string() + #key))?.try_into() {
                            Ok(v) => Ok(v),
                            Err(err) => Err(::jsondata::Error::InvalidType(#key.to_string())),
                        }?;
                        match v.parse() {
                            Ok(v) => Ok(v),
                            Err(err) => Err(::jsondata::Error::InvalidType(#key.to_string())),
                        }?
                    },
                },
                (false, Some(intr_type)) => quote! {
                    #field_name: {
                        let v: #intr_type = match value.get(&("/".to_string() + #key))?.try_into() {
                            Ok(v) => Ok(v),
                            Err(err) => Err(::jsondata::Error::InvalidType(#key.to_string())),
                        }?;
                        match v.try_into() {
                            Ok(v) => Ok(v),
                            Err(err) => Err(::jsondata::Error::InvalidType(#key.to_string())),
                        }?
                    },
                },
                (false, None) => quote! {
                    #field_name: match value.get(&("/".to_string() + #key))?.try_into() {
                        Ok(v) => Ok(v),
                        Err(err) => {
                            let msg = format!("{} err: {}", #key.to_string(), err);
                            Err(::jsondata::Error::InvalidType(msg))
                        }
                    }?,
                },
            }
        }
        None => TokenStream::new(),
    }
}
