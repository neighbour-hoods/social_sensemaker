use combine::{stream::position, EasyParser, StreamOnce};

use hdk::prelude::*;

use common::{
    create_interchange_entry_full, get_linked_interchange_entries_which_unify,
    mk_interchange_entry, CreateInterchangeEntryInput, InterchangeEntry, SchemeEntry, SchemeRoot,
};
use rep_lang_concrete_syntax::parse::expr;
use rep_lang_runtime::types::Scheme;

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

#[hdk_extern]
pub fn create_interchange_entry(input: CreateInterchangeEntryInput) -> ExternResult<HeaderHash> {
    create_interchange_entry_full(input).map(|t| t.0)
}
