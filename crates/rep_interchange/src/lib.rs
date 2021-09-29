use combine::{stream::position, EasyParser, StreamOnce};

use hdk::prelude::*;

use common::{CreateInterchangeEntryInput, InterchangeEntry, InterchangeOperand, Marker};
use rep_lang_concrete_syntax::parse::expr;
use rep_lang_core::{
    abstract_syntax::{Expr, Name},
    app,
};
use rep_lang_runtime::{
    env::Env,
    eval::{
        eval_, flat_thunk_to_sto_ref, inject_flatvalue_to_flatthunk, lookup_sto, new_term_env,
        normalize_expr, normalize_flat_value, value_to_flat_value, EvalState, FlatValue, Sto,
    },
    infer,
    infer::{infer_expr_with_is, InferState},
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
    let ie: InterchangeEntry = match element.into_inner().1.to_app_option()? {
        Some(ie) => ie,
        None => return Ok(ValidateCallbackResult::Valid),
    };

    let _computed_ie = mk_interchange_entry(ie.operator, ie.operands);

    todo!()
}

#[hdk_extern]
pub fn create_interchange_entry(input: CreateInterchangeEntryInput) -> ExternResult<HeaderHash> {
    let ie = mk_interchange_entry(input.expr, input.args)?;
    create_entry(&ie)
}

pub fn mk_interchange_entry(
    expr: Expr,
    args: Vec<InterchangeOperand>,
) -> ExternResult<InterchangeEntry> {
    let args: Vec<HeaderHash> = args
        .iter()
        .map(|io| match io {
            InterchangeOperand::InterchangeOperand(hh) => hh.clone(),
            InterchangeOperand::OtherOperand(_) => todo!("OtherOperand"),
        })
        .collect();

    // don't need result, just a preliminary check before hitting DHT
    let _expr_sc =
        infer_expr_with_is(&Env::new(), &mut InferState::new(), &expr).map_err(|type_error| {
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

    let mut is = InferState::new();
    let mut es = EvalState::new();
    let mut type_env = Env::new();
    let arg_named_scheme_values: Vec<(Name, Scheme, FlatValue<Marker>)> = int_entrs
        .iter()
        .map(|ie| {
            (
                es.fresh(),
                infer::normalize(&mut is, ie.output_scheme.clone()),
                normalize_flat_value(&mut es, &ie.output_value),
            )
        })
        .collect();
    type_env.extends(
        arg_named_scheme_values
            .iter()
            .map(|t| (t.0.clone(), t.1.clone())),
    );

    let applicator = |bd, nm: Name| app!(bd, Expr::Var(nm));
    let full_application: Expr = {
        let app_expr = arg_named_scheme_values
            .iter()
            .map(|t| t.0.clone())
            .fold(expr.clone(), applicator);
        normalize_expr(&mut es, &app_expr)
    };

    let full_application_sc =
        infer_expr_with_is(&type_env, &mut is, &full_application).map_err(|type_error| {
            WasmError::Guest(format!("type error in full application: {:?}", type_error))
        })?;

    let mut term_env = new_term_env();
    let mut sto: Sto<Marker> = Sto::new();

    for (nm, flat_val) in arg_named_scheme_values
        .iter()
        .map(|t| (t.0.clone(), t.2.clone()))
    {
        let v_ref =
            flat_thunk_to_sto_ref(&mut es, &mut sto, inject_flatvalue_to_flatthunk(flat_val));
        term_env.insert(nm, v_ref);
    }

    let full_application_vr = eval_(&mut term_env, &mut sto, &mut es, &full_application);
    let full_application_val = lookup_sto(&mut es, &full_application_vr, &mut sto);
    let full_application_flat_val = value_to_flat_value(&mut es, &full_application_val, &mut sto);

    let new_ie: InterchangeEntry = InterchangeEntry {
        operator: expr,
        operands: args
            .into_iter()
            .map(InterchangeOperand::InterchangeOperand)
            .collect(),
        output_scheme: full_application_sc,
        output_value: full_application_flat_val,
    };
    Ok(new_ie)
}
