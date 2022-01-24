use hdk::prelude::*;
use pretty::RcDoc;

use rep_lang_concrete_syntax::pretty::ppr_expr;
use rep_lang_core::abstract_syntax::{Expr, Gas};
use rep_lang_runtime::{eval::FlatValue, types::Scheme};

// TODO think carefully on what this should be.
pub type Marker = ();

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InterchangeOperand {
    // these dereference to `InterchangeEntry`
    InterchangeOperand(HeaderHash),
    // these dereference to `FlatThunk`??
    OtherOperand(HeaderHash),
}

impl InterchangeOperand {
    pub fn ppr(&self) -> RcDoc<()> {
        match &self {
            InterchangeOperand::InterchangeOperand(hh) => {
                RcDoc::text(format!("InterchangeOperand({})", hh))
            }
            InterchangeOperand::OtherOperand(hh) => RcDoc::text(format!("OtherOperand({})", hh)),
        }
    }
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

impl InterchangeEntry {
    pub fn ppr(&self) -> RcDoc<()> {
        let docs = vec![
            RcDoc::line(),
            RcDoc::concat(vec![
                RcDoc::text("operator:"),
                RcDoc::line(),
                ppr_expr(&self.operator).nest(1),
                RcDoc::text(","),
            ])
            .nest(1)
            .group(),
            RcDoc::line(),
            RcDoc::concat(vec![
                RcDoc::text("operands: ["),
                RcDoc::concat(
                    self.operands
                        .iter()
                        .map(|operand| RcDoc::line().append(operand.ppr().append(RcDoc::text(","))))
                        .collect::<Vec<_>>(),
                )
                .nest(1)
                .group(),
                RcDoc::line(),
                RcDoc::text("],"),
            ])
            .nest(1)
            .group(),
            RcDoc::line(),
            RcDoc::concat(vec![
                RcDoc::text("output_scheme:"),
                RcDoc::line(),
                self.output_scheme.ppr().nest(1),
                RcDoc::text(","),
            ])
            .nest(1)
            .group(),
            RcDoc::line(),
            RcDoc::concat(vec![
                RcDoc::text("output_flat_value:"),
                RcDoc::line(),
                self.output_flat_value.ppr().nest(1),
                RcDoc::text(","),
            ])
            .nest(1)
            .group(),
            RcDoc::line(),
            RcDoc::concat(vec![
                RcDoc::text("start_gas:"),
                RcDoc::line(),
                RcDoc::text(format!("{}", self.start_gas)).nest(1),
                RcDoc::text(","),
            ])
            .nest(1)
            .group(),
        ];
        RcDoc::concat(vec![
            RcDoc::text("InterchangeEntry {"),
            RcDoc::concat(docs).nest(2),
            RcDoc::line(),
            RcDoc::text("}"),
        ])
    }
}

/// input to `create_interchange_entry`
#[derive(Debug, Serialize, Deserialize, SerializedBytes)]
pub struct CreateInterchangeEntryInput {
    pub expr: Expr,
    pub args: Vec<InterchangeOperand>,
}
