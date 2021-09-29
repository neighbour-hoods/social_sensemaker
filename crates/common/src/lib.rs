use hdk::prelude::*;

use rep_lang_core::abstract_syntax::Expr;

/// input to `create_interchange_entry`
#[derive(Debug, Serialize, Deserialize, SerializedBytes)]
pub struct CreateInterchangeEntryInput {
    pub expr: Expr,
    // TODO `args` should perhaps be of type `InterchangeOperand`. that would
    // allow us to tidily handle non-`InterchangeEntry` args.
    pub args: Vec<HeaderHash>,
}
