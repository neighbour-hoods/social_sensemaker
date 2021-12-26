use hdk_derive::*;
use holochain_types::*;

use rep_lang_core::abstract_syntax::{Expr, Gas};
use rep_lang_runtime::{eval::FlatValue, types::Scheme};

// TODO think carefully on what this should be.
pub type Marker = ();

#[hdk_entry]
pub struct SchemeRoot;

#[hdk_entry]
pub struct SchemeEntry {
    pub sc: Scheme,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InterchangeOperand {
    // these dereference to `InterchangeEntry`
    InterchangeOperand(HeaderHash),
    // these dereference to `FlatThunk`??
    OtherOperand(HeaderHash),
}

#[hdk_entry(id = "interchange_entry")]
#[derive(Clone)]
pub struct InterchangeEntry {
    pub operator: Expr,
    pub operands: Vec<InterchangeOperand>,
    pub output_scheme: Scheme,
    pub output_flat_value: FlatValue<Marker>,
    pub start_gas: Gas,
}

/// input to `create_interchange_entry`
#[derive(Debug, Serialize, Deserialize, SerializedBytes)]
pub struct CreateInterchangeEntryInput {
    pub expr: Expr,
    pub args: Vec<InterchangeOperand>,
}
