#![crate_type = "proc-macro"]

use proc_macro::TokenStream;
use proc_macro2::{Ident, Span};
use quote;
use syn;

// TODO think about hdk_extern and which zome/happ it goes into. will the widgets want
// to invoke a macro, similar to `sensemaker_cell_id_fns`, s.t. the hdk_extern registers
// in their wasm?
#[proc_macro_attribute]
pub fn expand_remote_calls(_attrs: TokenStream, item: TokenStream) -> TokenStream {
    // expand_remote_calls is only valid for functions
    let item_fn = syn::parse_macro_input!(item as syn::ItemFn);
    let fn_name = item_fn.sig.ident.clone();

    let mut new_fn = item_fn.clone();

    new_fn.sig.ident = Ident::new(&format!("remote_{}", fn_name.clone()), Span::call_site());

    {
        let arg_pat_type = match item_fn
            .sig
            .inputs
            .first()
            .expect("hdk fn should have 1 arg")
        {
            syn::FnArg::Typed(pat_type) => pat_type,
            _ => panic!("expand_remote_calls: invalid Receiver FnArg"),
        };
        let arg_pat_type_pat = &arg_pat_type.pat;
        let arg_pat_type_ty = &arg_pat_type.ty;
        let token_stream = (quote::quote! {
            (cell_id, cap_secret, #arg_pat_type_pat): (CellId, Option<CapSecret>, #arg_pat_type_ty)
        })
        .into();
        let tup_arg = syn::parse_macro_input!(token_stream as syn::FnArg);
        assert!(new_fn.sig.inputs.pop().is_some());
        assert!(new_fn.sig.inputs.is_empty());
        new_fn.sig.inputs.push(tup_arg);
    }

    (quote::quote! {
        #[hdk_extern]
        #item_fn

        #[hdk_extern]
        #new_fn
    })
    .into()

    // steps
    //
    // 1. [x] clone ItemFn
    // 2. [x] modify clone's fn name
    // 3. [x] modify clone's fn sig, inserting new `cell_id: CellId` & `cap_secret: Option<CapSecret>` types onto a tuple whose 3rd value is the original arg.
    // 3. [x] keep return type the same
    // 4. [ ] declare/template `call(...)` fn body, which uses cell_id arg appropriately

    // (quote::quote! {
    //     match call(
    //         CallTargetCell::Other($cell_id),
    //         SENSEMAKER_ZOME_NAME.into(),
    //         #fn_name.into(),
    //         $cap_secret,
    //         $payload,
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
