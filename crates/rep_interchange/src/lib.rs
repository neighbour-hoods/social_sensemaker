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
        eval_, flat_thunk_to_sto_ref, inject_flatvalue_to_flatthunk, lookup_sto, new_term_env,
        value_to_flat_value, EvalState, FlatValue, Sto,
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

// TODO is `HeaderHash` right?
#[derive(Debug, Serialize, Deserialize)]
pub enum InterchangeOperand {
    // these dereference to `InterchangeEntry`
    InterchangeOperand(HeaderHash),
    // these dereference to `FlatThunk`??
    OtherOperand(HeaderHash),
}

#[hdk_entry(id = "interchange_entry")]
pub struct InterchangeEntry {
    pub operator: Expr,
    pub operands: Vec<InterchangeOperand>,
    pub output_scheme: Scheme,
    pub output_value: FlatValue<Marker>,
}

// TODO think carefully on what this should be.
type Marker = ();

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

/// input to `create_interchange_entry`
#[derive(Debug, Serialize, Deserialize, SerializedBytes)]
pub struct CreateInterchangeEntryInput {
    pub expr: Expr,
    // TODO `args` should perhaps be of type `InterchangeOperand`. that would
    // allow us to tidily handle non-`InterchangeEntry` args.
    pub args: Vec<HeaderHash>,
}

#[hdk_extern]
pub fn create_interchange_entry(input: CreateInterchangeEntryInput) -> ExternResult<HeaderHash> {
    let expr = input.expr;
    let args = input.args;
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
    let arg_named_schemes: Vec<(Name, Scheme, FlatValue<Marker>)> = int_entrs
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
        .fold(expr.clone(), applicator);

    let full_application_sc = infer_expr(&type_env, &full_application).map_err(|type_error| {
        WasmError::Guest(format!("type error in full application: {:?}", type_error))
    })?;

    let mut term_env = new_term_env();
    let mut sto: Sto<Marker> = Sto::new();

    for (nm, flat_val) in arg_named_schemes.iter().map(|t| (t.0.clone(), t.2.clone())) {
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
            .iter()
            .cloned()
            .map(InterchangeOperand::InterchangeOperand)
            .collect(),
        output_scheme: full_application_sc,
        output_value: full_application_flat_val,
    };
    create_entry(&new_ie)
}
