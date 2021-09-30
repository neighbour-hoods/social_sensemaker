use hdk::prelude::*;

use rep_lang_core::abstract_syntax::{Expr, Gas};
use rep_lang_runtime::{eval::FlatValue, types::Scheme};

// TODO think carefully on what this should be.
pub type Marker = ();

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
    pub start_gas: Gas,
}

/// input to `create_interchange_entry`
#[derive(Debug, Serialize, Deserialize, SerializedBytes)]
pub struct CreateInterchangeEntryInput {
    pub expr: Expr,
    // TODO `args` should perhaps be of type `InterchangeOperand`. that would
    // allow us to tidily handle non-`InterchangeEntry` args.
    pub args: Vec<InterchangeOperand>,
}
