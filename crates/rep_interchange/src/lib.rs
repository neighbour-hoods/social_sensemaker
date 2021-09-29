use combine::{stream::position, EasyParser, StreamOnce};

use hdk::prelude::*;

use rep_lang_concrete_syntax::parse::expr;
use rep_lang_core::{
    abstract_syntax::{Expr, Name},
    app,
};
use rep_lang_runtime::{
    env::Env,
    eval::{
        flat_thunk_to_sto_ref, inject_flatvalue_to_flatthunk, new_term_env, EvalState, FlatValue,
        Sto,
    },
    infer::infer_expr,
    types::Scheme,
};

#[hdk_extern]
fn entry_defs(_: ()) -> ExternResult<EntryDefsCallbackResult> {
    Ok(EntryDefsCallbackResult::from(vec![Path::entry_def()]))
}

#[derive(Debug, Serialize, Deserialize)]
struct Params {
    params_string: String,
}

#[hdk_extern]
fn test_output(params: Params) -> ExternResult<bool> {
    let Params {
        params_string: p_str,
    } = params;
    debug!("received input: {}", p_str);

    match expr().easy_parse(position::Stream::new(&p_str[..])) {
        Err(err) => {
            debug!("parse error:\n\n{}\n", err);
            Ok(false)
        }
        Ok((expr, extra_input)) => {
            if extra_input.is_partial() {
                debug!("error: unconsumed input: {:?}", extra_input);
                Ok(false)
            } else {
                debug!("ast: {:?}\n", expr);
                Ok(true)
            }
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub enum InterchangeOperand {
    // these dereference to `InterchangeEntry`
    InterchangeOperand(EntryHash),
    // these dereference to `FlatThunk`??
    OtherOperand(EntryHash),
}

#[hdk_entry(id = "interchange_entry")]
pub struct InterchangeEntry {
    pub operator: Expr,
    pub operands: Vec<InterchangeOperand>,
    pub output_scheme: Scheme,
    pub output_value: FlatValue,
}

#[hdk_extern]
pub(crate) fn validate_create_entry_interchange_entry(
    validate_data: ValidateData,
) -> ExternResult<ValidateCallbackResult> {
    validate_create_update_entry_interchange_entry(validate_data)
}

#[hdk_extern]
pub(crate) fn validate_update_entry_interchange_entry(
    validate_data: ValidateData,
) -> ExternResult<ValidateCallbackResult> {
    validate_create_update_entry_interchange_entry(validate_data)
}

pub fn validate_create_update_entry_interchange_entry(
    validate_data: ValidateData,
) -> ExternResult<ValidateCallbackResult> {
    let element = validate_data.element.clone();
    let entry = element.into_inner().1;
    let entry = match entry {
        ElementEntry::Present(e) => e,
        _ => return Ok(ValidateCallbackResult::Valid),
    };
    Ok(match InterchangeEntry::try_from(&entry) {
        Ok(_ie) => {
            todo!()
        }
        _ => ValidateCallbackResult::Valid,
    })
}

pub fn create_interchange_entry(expr: Expr, args: &[EntryHash]) -> ExternResult<EntryHash> {
    // don't need result, just a preliminary check before hitting DHT
    let _expr_sc = infer_expr(&Env::new(), &expr).map_err(|type_error| {
        WasmError::Guest(format!("type error in `expr`: {:?}", type_error))
    })?;

    // dereference `args`
    let int_entrs: Vec<InterchangeEntry> = args
        .iter()
        .cloned()
        .map(|arg_hash| {
            let element = (match get(arg_hash.clone(), GetOptions::content())? {
                Some(el) => Ok(el),
                None => Err(WasmError::Guest(format!(
                    "could not dereference arg: {}",
                    arg_hash
                ))),
            })?;
            match element.into_inner().1.to_app_option()? {
                Some(ie) => Ok(ie),
                None => Err(WasmError::Guest(format!("non-present arg: {}", arg_hash))),
            }
        })
        .collect::<ExternResult<_>>()?;

    let mut es = EvalState::new();
    let mut type_env = Env::new();
    // TODO these `Scheme`s must be normalized / sanitized / renamed
    let arg_named_schemes: Vec<(Name, Scheme, FlatValue)> = int_entrs
        .iter()
        .map(|ie| {
            (
                es.fresh(),
                ie.output_scheme.clone(),
                ie.output_value.clone(),
            )
        })
        .collect();
    type_env.extends(arg_named_schemes.iter().map(|t| (t.0.clone(), t.1.clone())));

    let applicator = |bd, nm: Name| app!(bd, Expr::Var(nm));
    let full_application: Expr = arg_named_schemes
        .iter()
        .map(|t| t.0.clone())
        .fold(expr, applicator);

    // don't need result, just a check
    let _full_application_sc = infer_expr(&type_env, &full_application).map_err(|type_error| {
        WasmError::Guest(format!("type error in full application: {:?}", type_error))
    })?;

    let mut term_env = new_term_env();
    // TODO this should be `Void`
    let mut sto: Sto<()> = Sto::new();

    for (nm, flat_val) in arg_named_schemes.iter().map(|t| (t.0.clone(), t.2.clone())) {
        let v_ref =
            flat_thunk_to_sto_ref(&mut es, &mut sto, inject_flatvalue_to_flatthunk(flat_val));
        term_env.insert(nm, v_ref);
    }

    todo!()
}
