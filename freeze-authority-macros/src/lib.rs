use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, FnArg, ItemFn};

#[proc_macro_attribute]
pub fn freeze_authority(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}

#[proc_macro_attribute]
pub fn require_not_frozen(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}

#[proc_macro_attribute]
pub fn freeze_exempt(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}

/// No-op `#[instruction]` for path-dep-scanned freeze-authority fns. Strips
/// `#[account(...)]` helper attrs from params so rustc accepts the
/// freeze-authority crate compile. The path-dep scanner reads raw source
/// via `syn::parse_file` and sees the `#[account(...)]` attrs intact.
#[proc_macro_attribute]
pub fn instruction(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut func = parse_macro_input!(item as ItemFn);
    for arg in &mut func.sig.inputs {
        if let FnArg::Typed(pt) = arg {
            pt.attrs.retain(|a| !a.path().is_ident("account"));
        }
    }
    quote!(#func).into()
}