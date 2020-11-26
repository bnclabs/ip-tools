use syn::{self, PathArguments::AngleBracketed};

pub fn subty_of_vec<'a>(ty: &'a syn::Type) -> Option<&'a syn::Type> {
    subty_if(ty, |seg| seg.ident == "Vec")
}

pub fn ty_u8<'a>(ty: &'a syn::Type) -> bool {
    let ty = strip_group(ty);
    only_last_segment(ty)
        .filter(|seg| seg.ident == "u8")
        .is_some()
}

fn subty_if<F>(ty: &syn::Type, f: F) -> Option<&syn::Type>
where
    F: FnOnce(&syn::PathSegment) -> bool,
{
    let ty = strip_group(ty);

    only_last_segment(ty)
        .filter(|segment| f(segment))
        .and_then(|segment| {
            if let AngleBracketed(args) = &segment.arguments {
                only_one(args.args.iter()).and_then(|genneric| {
                    if let syn::GenericArgument::Type(ty) = genneric {
                        Some(ty)
                    } else {
                        None
                    }
                })
            } else {
                None
            }
        })
}

// If the struct is placed inside of a macro_rules! declaration,
// in some circumstances, the tokens inside will be enclosed
// in `proc_macro::Group` delimited by invisible `proc_macro::Delimiter::None`.
//
// In syn speak, this is encoded via `*::Group` variants. We don't really care about
// that, so let's just strip it.
//
// Details: https://doc.rust-lang.org/proc_macro/enum.Delimiter.html#variant.None
// See also: https://github.com/TeXitoi/structopt/issues/439
fn strip_group(mut ty: &syn::Type) -> &syn::Type {
    while let syn::Type::Group(group) = ty {
        ty = &*group.elem;
    }

    ty
}

fn only_last_segment(ty: &syn::Type) -> Option<&syn::PathSegment> {
    match ty {
        syn::Type::Path(syn::TypePath {
            qself: None,
            path:
                syn::Path {
                    leading_colon: None,
                    segments,
                },
        }) => only_one(segments.iter()),

        _ => None,
    }
}

fn only_one<I, T>(mut iter: I) -> Option<T>
where
    I: Iterator<Item = T>,
{
    iter.next().filter(|_| iter.next().is_none())
}
