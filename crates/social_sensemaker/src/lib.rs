use hdk::prelude::*;

use common::{
    create_sensemaker_entry_full, mk_sensemaker_entry, CreateSensemakerEntryInput, SchemeEntry,
    SchemeRoot, SensemakerEntry,
};

entry_defs![
    Path::entry_def(),
    PathEntry::entry_def(),
    SensemakerEntry::entry_def(),
    SchemeEntry::entry_def(),
    SchemeRoot::entry_def()
];

#[hdk_extern]
pub fn init(_: ()) -> ExternResult<InitCallbackResult> {
    let mut functions = GrantedFunctions::new();
    functions.insert((zome_info()?.name, "get_sensemaker_entry_by_path".into()));
    functions.insert((
        zome_info()?.name,
        "set_sensemaker_entry_parse_rl_expr".into(),
    ));
    functions.insert((zome_info()?.name, "initialize_sm_data".into()));

    let grant = ZomeCallCapGrant {
        access: CapAccess::Unrestricted,
        functions,
        tag: "".into(),
    };
    create_cap_grant(grant)?;

    Ok(InitCallbackResult::Pass)
}

#[hdk_extern]
pub(crate) fn validate_create_entry_sensemaker_entry(
    op: Op,
) -> ExternResult<ValidateCallbackResult> {
    validate_create_update_entry_sensemaker_entry(op)
}

#[hdk_extern]
pub(crate) fn validate_update_entry_sensemaker_entry(
    op: Op,
) -> ExternResult<ValidateCallbackResult> {
    validate_create_update_entry_sensemaker_entry(op)
}

pub fn validate_create_update_entry_sensemaker_entry(
    op: Op,
) -> ExternResult<ValidateCallbackResult> {
    let entry: Entry = match op {
        Op::StoreEntry {
            entry: entry @ Entry::App(_),
            header: _,
        } => entry,
        Op::RegisterUpdate {
            update: _,
            new_entry,
            original_header: _,
            original_entry: _,
        } => new_entry,
        _ => {
            return Ok(ValidateCallbackResult::Invalid(
                "Unexpected op: not StoreEntry or RegisterUpdate".into(),
            ))
        }
    };

    let se: SensemakerEntry = match entry_to_struct(&entry)? {
        Some(se) => Ok(se),
        None => Err(WasmError::Guest(format!(
            "Couldn't convert Entry {:?} into SensemakerEntry",
            entry
        ))),
    }?;

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

pub fn entry_to_struct<A: TryFrom<SerializedBytes, Error = SerializedBytesError>>(
    entry: &Entry,
) -> Result<Option<A>, SerializedBytesError> {
    match entry {
        Entry::App(eb) => Ok(Some(A::try_from(SerializedBytes::from(eb.to_owned()))?)),
        _ => Ok(None),
    }
}
