use hdi::prelude::holo_hash::*;
use hdi::prelude::*;

use rep_lang_core::abstract_syntax::{Expr, Gas};
use rep_lang_runtime::{eval::FlatValue, types::Scheme};

// TODO think carefully on what this should be.
pub type Marker = ();

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SensemakerOperand {
    // these dereference to `SensemakerEntry`
    SensemakerOperand(EntryHash),
    // these dereference to `FlatThunk`??
    OtherOperand(EntryHash),
}

#[hdk_entry_helper]
#[derive(Clone)]
pub struct SensemakerEntry {
    pub operator: Expr,
    pub operands: Vec<SensemakerOperand>,
    pub output_scheme: Scheme,
    pub output_flat_value: FlatValue<Marker>,
    pub start_gas: Gas,
}

#[hdk_entry_helper]
pub struct SchemeRoot;

#[hdk_entry_helper]
pub struct SchemeEntry {
    pub sc: Scheme,
}

#[hdk_entry_helper]
#[derive(Clone)]
pub struct SensemakerCellId {
    // must include extension
    pub dna_hash: DnaHash,
    // encoded file bytes payload
    pub agent_pubkey: AgentPubKey,
}
