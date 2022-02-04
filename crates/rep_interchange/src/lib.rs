use combine::{stream::position, EasyParser, StreamOnce};

use hdk::prelude::*;

use common::{
    get_linked_interchange_entries_which_unify, mk_interchange_entry, CreateInterchangeEntryInput,
    InterchangeEntry, SchemeEntry, SchemeRoot,
};
use rep_lang_concrete_syntax::parse::expr;
use rep_lang_runtime::types::Scheme;

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

/// INFO this is incomplete and doesn't currently parse the `args`
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
                    // TODO parse `input.args`.
                    // parsing a HeaderHash seems like it should be possible, but I've not done
                    // it before. we might also want some indicator of which of
                    // InterchangeOperand constructor is desired?
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
