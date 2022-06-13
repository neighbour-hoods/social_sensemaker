#![crate_type = "proc-macro"]

use proc_macro::TokenStream;
use proc_macro2::{Ident, Span};
use quote::{TokenStreamExt, ToTokens};
use syn::{parse::{Parse, ParseStream, Result}, punctuated::Punctuated};

// TODO think about hdk_extern and which zome/happ it goes into. will the widgets want
// to invoke a macro, similar to `sensemaker_cell_id_fns`, s.t. the hdk_extern registers
// in their wasm?
#[proc_macro_attribute]
pub fn expand_remote_calls(attrs: TokenStream, item: TokenStream) -> TokenStream {
    // expand_remote_calls is only valid for functions
    let item_fn = syn::parse_macro_input!(item as syn::ItemFn);
    let fn_name = item_fn.sig.ident.clone();

    let new_fn = {
        let mut new_fn = item_fn.clone();

        let new_fn_name = Ident::new(&format!("{}_remote", fn_name.clone()), Span::call_site());

        new_fn.sig.ident = new_fn_name;

        // item_fn.sig.inputs

        new_fn
    };

    (quote::quote! {
        #item_fn

        // #new_fn
    })
    .into()

    // steps
    //
    // 1. [x] clone ItemFn
    // 2. [x] modify clone's fn name
    // 3. [ ] modify clone's fn sig, inserting new `cell_id: CellId` type on front of
    //        inputs list
    // 3. [x] keep return type the same
    // 4. [ ] declare/template `call(...)` fn body, which uses cell_id arg appropriately

    // (quote::quote! {
    //     match call(
    //         CallTargetCell::Other(cell_id),
    //         SENSEMAKER_ZOME_NAME.into(),
    //         #fn_name.into(),
    //         None,
    //         $args,
    //     )? {
    //         ZomeCallResponse::Ok(response) => Ok(response.decode()?),
    //         err => {
    //             error!("ZomeCallResponse error: {:?}", err);
    //             Err(WasmError::Guest(format!("#fn_name_remote: {:?}", err)))
    //         }
    //     }
    // })
    // .into()
}
