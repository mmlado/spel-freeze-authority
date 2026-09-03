use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{
    Expr, FnArg, ItemFn, MetaNameValue, Token, parse_macro_input, parse_quote,
    punctuated::Punctuated,
};

/// Marker attribute. Framework detects it on a `#[lez_program]` module
/// and merges the freeze instructions into the dispatcher and IDL.
///
/// Attributes expand top to bottom, so if `#[lez_program]` is still
/// visible on the module when this macro runs, the marker was written
/// above it. The framework would never see the marker, silently
/// skipping discovery, so that placement is a hard error.
#[proc_macro_attribute]
pub fn freeze_authority(_attr: TokenStream, item: TokenStream) -> TokenStream {
    if let Ok(module) = syn::parse::<syn::ItemMod>(item.clone())
        && let Some(err) = misplaced_above_lez_program(&module)
    {
        return err.to_compile_error().into();
    }
    item
}

/// The marker was written above `#[lez_program]` when that attribute is
/// still visible on the module the marker expands on.
fn misplaced_above_lez_program(module: &syn::ItemMod) -> Option<syn::Error> {
    if module
        .attrs
        .iter()
        .any(|a| a.path().is_ident("lez_program"))
    {
        Some(syn::Error::new_spanned(
            &module.ident,
            "#[freeze_authority] must come after #[lez_program]: a marker above \
            it expands first and is invisible to the framework",
        ))
    } else {
        None
    }
}

/// Gate: rejects when the program-wide `is_frozen` flag or the caller's
/// per-account frozen PDA is set.
///
/// Kwargs, all optional: `freeze_config = <param>` and
/// `freeze_account = <param>` rename the accounts the gate reads,
/// `caller = <param>` is accepted for parity and unused,
/// `offset = <int>` locates the config window inside the embedding
/// account. Offset defaults to 0, the dedicated layout, and is only
/// ever framework-stamped in embedded mode. The per-account read is
/// offset-free, `FrozenAccountState` never embeds.
#[proc_macro_attribute]
pub fn require_not_frozen(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(
        attr with Punctuated::<MetaNameValue, Token![,]>::parse_terminated
    );

    let mut config_ident = format_ident!("freeze_config");
    let mut per_account_ident = format_ident!("freeze_account");
    let mut offset: usize = 0;

    for pair in args {
        if pair.path.is_ident("offset") {
            let Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Int(i),
                ..
            }) = &pair.value
            else {
                return syn::Error::new_spanned(&pair.value, "offset must be an integer literal")
                    .to_compile_error()
                    .into();
            };
            offset = match i.base10_parse::<usize>() {
                Ok(v) => v,
                Err(e) => return e.to_compile_error().into(),
            };
            continue;
        }
        let value_ident = match &pair.value {
            Expr::Path(p) if p.path.get_ident().is_some() => p.path.get_ident().unwrap().clone(),
            other => {
                return syn::Error::new_spanned(other, "expected a bare parameter name")
                    .to_compile_error()
                    .into();
            }
        };

        if pair.path.is_ident("freeze_config") {
            config_ident = value_ident;
        } else if pair.path.is_ident("freeze_account") {
            per_account_ident = value_ident;
        } else if pair.path.is_ident("caller") {
            let _ = value_ident;
        } else {
            return syn::Error::new_spanned(
                &pair.path,
                "unknown key; expected `freeze_config` or `freeze_account` or `caller` or `offset`",
            )
            .to_compile_error()
            .into();
        }
    }

    let mut func: syn::ItemFn = parse_macro_input!(item as ItemFn);

    let prologue: syn::Stmt = parse_quote! {{
        // Program-wide gate: strict decode; missing config = NotInitialized.
        let __freeze_cfg = ::freeze_authority::FreezeConfig::from_account_at(&#config_ident, #offset)?;
        if __freeze_cfg.is_frozen {
            return Err(::freeze_authority::FreezeError::Frozen.into());
        }
        // Per-account gate: lenient decode; missing PDA = default (unfrozen).
        let __freeze_acc = ::freeze_authority::FrozenAccountState::from_data_or_default(
            &#per_account_ident.account.data
        )?;
        if __freeze_acc.is_frozen {
            return Err(::freeze_authority::FreezeError::AccountFrozen.into());
        }
    }};
    func.block.stmts.insert(0, prologue);
    quote!(#func).into()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_above_lez_program_is_rejected() {
        // #[lez_program] still visible on the module means this marker
        // expanded first, so it sits above and the framework never sees it.
        let module: syn::ItemMod = parse_quote! {
            #[lez_program]
            mod program {}
        };
        let err = misplaced_above_lez_program(&module).expect("must reject");
        assert!(
            err.to_string().contains("must come after #[lez_program]"),
            "unexpected message: {err}"
        );
    }

    #[test]
    fn rejection_names_the_freeze_marker() {
        // The message has to name this crate's marker, not the admin one
        // it was mirrored from, or it sends the reader after the wrong
        // attribute.
        let module: syn::ItemMod = parse_quote! {
            #[lez_program]
            mod program {}
        };
        let err = misplaced_above_lez_program(&module).expect("must reject");
        assert!(
            err.to_string().starts_with("#[freeze_authority]"),
            "unexpected message: {err}"
        );
    }

    #[test]
    fn marker_below_lez_program_passes() {
        // Correct placement: by the time a below-marker could expand,
        // #[lez_program] has consumed itself and is no longer on the module.
        let module: syn::ItemMod = parse_quote! {
            #[doc = "no lez_program attr here"]
            mod program {}
        };
        assert!(misplaced_above_lez_program(&module).is_none());
    }
}
