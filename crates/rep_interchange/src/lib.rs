use combine::{stream::position, EasyParser, StreamOnce};
use std::collections::HashMap;

use hdk::{entry::must_get_valid_element, prelude::*};

use common::{
    CreateInterchangeEntryInput, InterchangeEntry, InterchangeOperand, Marker, SchemeEntry,
    SchemeRoot,
};
use rep_lang_concrete_syntax::parse::expr;
use rep_lang_core::{
    abstract_syntax::{Expr, Name},
    app,
};
use rep_lang_runtime::{
    env::Env,
    eval::{
        eval_, flat_thunk_to_sto_ref, inject_flatvalue_to_flatthunk, lookup_sto, new_term_env,
        value_to_flat_value, EvalState, FlatValue, Normalizable, Sto,
    },
    infer,
    infer::{infer_expr_with_is, normalize, unifies, InferState},
    types::Scheme,
};

pub const OWNER_TAG: &str = "rep_interchange_owner";

entry_defs![
    Path::entry_def(),
    InterchangeEntry::entry_def(),
    SchemeEntry::entry_def()
];

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
pub fn get_interchange_entries_which_unify(
    opt_target_sc: Option<Scheme>,
) -> ExternResult<Vec<(HeaderHash, InterchangeEntry)>> {
    get_linked_interchange_entries_which_unify((hash_entry(SchemeRoot)?, opt_target_sc))
}

#[hdk_extern]
pub fn get_linked_interchange_entries_which_unify(
    (target_hash, opt_target_sc): (EntryHash, Option<Scheme>),
) -> ExternResult<Vec<(HeaderHash, InterchangeEntry)>> {
    let scheme_entry_links = get_links(target_hash, None)?;
    let scheme_entry_hashes: Vec<EntryHash> = scheme_entry_links
        .into_iter()
        .map(|lnk| lnk.target)
        .collect();
    let filtered_scheme_entry_hashes: Vec<EntryHash> = match opt_target_sc {
        // if no target Scheme, we do not filter
        None => scheme_entry_hashes,
        // if yes target Scheme, we filter based on unification
        Some(target_sc) => {
            let mut is = InferState::new();
            let Scheme(_, normalized_target_ty) = normalize(&mut is, target_sc);
            scheme_entry_hashes
                .into_iter()
                .filter(|s_eh| {
                    let mut flag = || -> ExternResult<bool> {
                        // retrieve `SchemeEntry` element, decode to entry
                        let element = (match get(s_eh.clone(), GetOptions::content())? {
                            Some(el) => Ok(el),
                            None => Err(WasmError::Guest(format!(
                                "could not dereference hash: {}",
                                s_eh.clone()
                            ))),
                        })?;
                        let scheme_entry: SchemeEntry =
                            match element.into_inner().1.to_app_option()? {
                                Some(se) => Ok(se),
                                None => {
                                    Err(WasmError::Guest(format!("non-present arg: {}", *s_eh)))
                                }
                            }?;
                        // check unification of normalized type
                        let Scheme(_, normalized_candidate_ty) =
                            normalize(&mut is, scheme_entry.sc);
                        // we are only interested in whether a type error occured
                        Ok(unifies(normalized_target_ty.clone(), normalized_candidate_ty).is_ok())
                    };
                    // any `ExternResult` `Err`s are treated as disqualifiers for filtration
                    // purposes.
                    flag().unwrap_or(false)
                })
                .collect()
        }
    };
    filtered_scheme_entry_hashes
        .into_iter()
        .map(|s_eh| get_links(s_eh, None))
        .flatten()
        .flatten()
        .map(|lnk| get_interchange_entry(lnk.target))
        .collect()
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
    let element = validate_data.element;
    let ie: InterchangeEntry = match element.into_inner().1.to_app_option()? {
        Some(ie) => ie,
        None => return Ok(ValidateCallbackResult::Valid),
    };

    let computed_ie = mk_interchange_entry(ie.operator, ie.operands)?;

    if computed_ie.output_scheme != ie.output_scheme {
        return Ok(ValidateCallbackResult::Invalid(format!(
            "InterchangeEntry scheme mismatch:\
        \ncomputed: {:?}\
        \nreceived: {:?}",
            computed_ie.output_scheme, ie.output_scheme
        )));
    }

    if computed_ie.output_flat_value != ie.output_flat_value {
        return Ok(ValidateCallbackResult::Invalid(format!(
            "InterchangeEntry value mismatch:\
        \ncomputed: {:?}\
        \nreceived: {:?}",
            computed_ie.output_flat_value, ie.output_flat_value
        )));
    }

    Ok(ValidateCallbackResult::Valid)
}

#[derive(Debug, Serialize, Deserialize, SerializedBytes)]
pub struct CreateInterchangeEntryInputParse {
    expr: String,
    args: Vec<String>,
}

#[hdk_extern]
pub fn create_interchange_entry_parse(
    input: CreateInterchangeEntryInputParse,
) -> ExternResult<String> {
    let hash = match expr().easy_parse(position::Stream::new(&input.expr[..])) {
        Err(err) => Err(WasmError::Guest(format!("parse error:\n\n{}\n", err))),
        Ok((expr, extra_input)) => {
            if extra_input.is_partial() {
                Err(WasmError::Guest(format!(
                    "error: unconsumed input: {:?}",
                    extra_input
                )))
            } else {
                debug!("ast: {:?}\n", expr);
                create_interchange_entry(CreateInterchangeEntryInput {
                    expr,
                    args: Vec::new(),
                })
            }
        }
    }?;
    Ok(hash.to_string())
}

#[hdk_extern]
pub fn get_interchange_entry_by_headerhash(
    arg_hash: HeaderHash,
) -> ExternResult<(EntryHash, InterchangeEntry)> {
    let element = (match get(arg_hash.clone(), GetOptions::content())? {
        Some(el) => Ok(el),
        None => Err(WasmError::Guest(format!(
            "could not dereference arg: {}",
            arg_hash
        ))),
    })?;
    match element.into_inner().1.to_app_option()? {
        Some(ie) => {
            let ie_hash = hash_entry(&ie)?;
            Ok((ie_hash, ie))
        }
        None => Err(WasmError::Guest(format!("non-present arg: {}", arg_hash))),
    }
}

#[hdk_extern]
pub fn get_interchange_entry(arg_hash: EntryHash) -> ExternResult<(HeaderHash, InterchangeEntry)> {
    let element = (match get(arg_hash.clone(), GetOptions::content())? {
        Some(el) => Ok(el),
        None => Err(WasmError::Guest(format!(
            "could not dereference arg: {}",
            arg_hash
        ))),
    })?;
    let hh = element.header_address().clone();
    match element.into_inner().1.to_app_option()? {
        Some(ie) => Ok((hh, ie)),
        None => Err(WasmError::Guest(format!("non-present arg: {}", arg_hash))),
    }
}

#[hdk_extern]
pub fn create_interchange_entry(input: CreateInterchangeEntryInput) -> ExternResult<HeaderHash> {
    let ie = mk_interchange_entry(input.expr, input.args)?;

    // create SchemeRoot (if needed)
    match get(hash_entry(&SchemeRoot)?, GetOptions::content())? {
        None => {
            let _hh = create_entry(&SchemeRoot)?;
        }
        Some(_) => {}
    };

    // create Scheme entry & link from SchemeRoot (if needed)
    let scheme_entry = SchemeEntry {
        sc: ie.output_scheme.clone(),
    };
    let scheme_entry_hash = hash_entry(&scheme_entry)?;
    match get(scheme_entry_hash.clone(), GetOptions::content())? {
        None => {
            let _hh = create_entry(&scheme_entry)?;
            create_link(
                hash_entry(SchemeRoot)?,
                scheme_entry_hash.clone(),
                LinkTag::new(OWNER_TAG),
            )?;
        }
        Some(_) => {}
    };

    // create IE & link from Scheme entry (if needed)
    let ie_hash = hash_entry(&ie)?;
    match get(ie_hash.clone(), GetOptions::content())? {
        None => {
            let hh = create_entry(&ie)?;
            create_link(scheme_entry_hash, ie_hash, LinkTag::new(OWNER_TAG))?;
            Ok(hh)
        }
        Some(element) => Ok(element.header_address().clone()),
    }
}

pub fn mk_interchange_entry(
    expr: Expr,
    args: Vec<InterchangeOperand>,
) -> ExternResult<InterchangeEntry> {
    let arg_hh_s: Vec<HeaderHash> = args
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

    // dereference `arg_hh_s`
    let int_entrs: Vec<InterchangeEntry> = arg_hh_s
        .iter()
        .cloned()
        .map(|arg_hash| {
            let element = must_get_valid_element(arg_hash.clone())?;
            match element.into_inner().1.to_app_option()? {
                Some(ie) => Ok(ie),
                None => Err(WasmError::Guest(format!("non-present arg: {}", arg_hash))),
            }
        })
        .collect::<ExternResult<_>>()?;

    let mut is = InferState::new();
    let mut es = EvalState::new();
    let mut type_env = Env::new();
    // we normalize up here, before conjuring fresh names for the `args`, in order to avoid
    // potential contamination. I'm not sure it is necessary, but doing it to be safe.
    let normalized_expr = expr.normalize(&mut HashMap::new(), &mut es);
    let arg_named_scheme_values: Vec<(Name, Scheme, FlatValue<Marker>)> = int_entrs
        .iter()
        .map(|ie| {
            (
                es.fresh_name(),
                infer::normalize(&mut is, ie.output_scheme.clone()),
                ie.output_flat_value.normalize(&mut HashMap::new(), &mut es),
            )
        })
        .collect();
    type_env.extends(
        arg_named_scheme_values
            .iter()
            .map(|t| (t.0.clone(), t.1.clone())),
    );

    let applicator = |bd, nm: Name| app!(bd, Expr::Var(nm));
    let full_application: Expr = arg_named_scheme_values
        .iter()
        .map(|t| t.0.clone())
        .fold(normalized_expr, applicator);

    // TODO substantiate whether this Scheme will have high-indexed `Tv`s, which might be
    // unintuitive / cause issues with programmatic `Scheme` matching.
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

    let full_application_vr = eval_(&term_env, &mut sto, &mut es, &full_application);
    let full_application_val = lookup_sto(&mut es, &full_application_vr, &mut sto);
    let full_application_flat_val = value_to_flat_value(&mut es, &full_application_val, &mut sto);

    let new_ie: InterchangeEntry = InterchangeEntry {
        operator: expr,
        operands: arg_hh_s
            .into_iter()
            .map(InterchangeOperand::InterchangeOperand)
            .collect(),
        output_scheme: full_application_sc,
        output_flat_value: full_application_flat_val,
        start_gas: es.current_gas_count(),
    };
    Ok(new_ie)
}
