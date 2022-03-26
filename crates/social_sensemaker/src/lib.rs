use combine::{stream::position, EasyParser, StreamOnce};

use hdk::prelude::*;

use common::{
    create_sensemaker_entry_full, mk_sensemaker_entry, CreateSensemakerEntryInput, SchemeEntry,
    SensemakerEntry,
};
use rep_lang_concrete_syntax::parse::expr;

entry_defs![
    Path::entry_def(),
    SensemakerEntry::entry_def(),
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
pub(crate) fn validate_create_entry_sensemaker_entry(
    validate_data: ValidateData,
) -> ExternResult<ValidateCallbackResult> {
    validate_create_update_entry_sensemaker_entry(validate_data)
}

#[hdk_extern]
pub(crate) fn validate_update_entry_sensemaker_entry(
    validate_data: ValidateData,
) -> ExternResult<ValidateCallbackResult> {
    validate_create_update_entry_sensemaker_entry(validate_data)
}

pub fn validate_create_update_entry_sensemaker_entry(
    validate_data: ValidateData,
) -> ExternResult<ValidateCallbackResult> {
    let element = validate_data.element;
    let se: SensemakerEntry = match element.into_inner().1.to_app_option()? {
        Some(se) => se,
        None => return Ok(ValidateCallbackResult::Valid),
    };

    let computed_se = mk_sensemaker_entry(se.operator, se.operands)?;

    if computed_se.output_scheme != se.output_scheme {
        return Ok(ValidateCallbackResult::Invalid(format!(
            "SensemakerEntry scheme mismatch:\
        \ncomputed: {:?}\
        \nreceived: {:?}",
            computed_se.output_scheme, se.output_scheme
        )));
    }

    if computed_se.output_flat_value != se.output_flat_value {
        return Ok(ValidateCallbackResult::Invalid(format!(
            "SensemakerEntry value mismatch:\
        \ncomputed: {:?}\
        \nreceived: {:?}",
            computed_se.output_flat_value, se.output_flat_value
        )));
    }

    Ok(ValidateCallbackResult::Valid)
}

#[hdk_extern]
pub fn create_sensemaker_entry(input: CreateSensemakerEntryInput) -> ExternResult<HeaderHash> {
    create_sensemaker_entry_full(input).map(|t| t.0)
}
