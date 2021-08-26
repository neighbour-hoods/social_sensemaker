use hdk::prelude::*;

#[hdk_extern]
fn entry_defs(_: ()) -> ExternResult<EntryDefsCallbackResult> {
    Ok(EntryDefsCallbackResult::from(vec![
        Path::entry_def(),
    ]))
}

#[derive(Debug, Serialize, Deserialize)]
struct Params {
    param: String,
}

#[hdk_extern]
fn test_output(Params { param }: Params) -> ExternResult<bool> {
    debug!("Got some param {:?}", param);
    Ok(true)
}
