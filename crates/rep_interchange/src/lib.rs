use combine::{stream::position, EasyParser, StreamOnce};

use hdk::prelude::*;

use rep_lang_concrete_syntax::parse::expr;
use rep_lang_core::abstract_syntax::Expr;

use rep_lang_runtime::{eval::FlatValue, types::Type};

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
    pub output_type: Type,
    pub output: FlatValue,
}

#[hdk_extern]
pub(crate) fn validate_create_entry_interchange_entry(validate_data: ValidateData) -> ExternResult<ValidateCallbackResult> {
    validate_create_update_entry_interchange_entry(validate_data)
}

#[hdk_extern]
pub(crate) fn validate_update_entry_interchange_entry(validate_data: ValidateData) -> ExternResult<ValidateCallbackResult> {
    validate_create_update_entry_interchange_entry(validate_data)
}

pub fn validate_create_update_entry_interchange_entry(validate_data: ValidateData) -> ExternResult<ValidateCallbackResult> {
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

pub fn create_interchange_entry(_expr: Expr, _args: &[EntryHash]) -> ExternResult<EntryHash> {
    todo!()
}
