use combine::{stream::position, EasyParser, StreamOnce};
use hdk::{entry::must_get_valid_element, prelude::*};
use pretty::RcDoc;
use std::collections::HashMap;

use rep_lang_concrete_syntax::{parse::expr, pretty::ppr_expr};
use rep_lang_core::{
    abstract_syntax::{Expr, Gas, Name, PrimOp},
    app, lam,
};
use rep_lang_runtime::{
    env::Env,
    eval::{
        eval_, flat_thunk_to_sto_ref, inject_flatvalue_to_flatthunk, lookup_sto, new_term_env,
        value_to_flat_value, EvalState, FlatValue, Normalizable, Sto,
    },
    infer::{self, infer_expr_with_is, normalize, unifies, InferState},
    types::Scheme,
};

pub const OWNER_TAG: &str = "rep_interchange_owner";

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

/// input to `create_interchange_entry_full`
#[derive(Debug, Serialize, Deserialize, SerializedBytes)]
pub struct CreateInterchangeEntryInput {
    pub expr: Expr,
    pub args: Vec<InterchangeOperand>,
}

#[hdk_entry]
pub struct SchemeRoot;

#[hdk_entry]
pub struct SchemeEntry {
    pub sc: Scheme,
}

// functions

#[hdk_extern]
pub fn get_interchange_entries_which_unify(
    opt_target_sc: Option<Scheme>,
) -> ExternResult<Vec<(HeaderHash, InterchangeEntry)>> {
    get_linked_interchange_entries_which_unify((hash_entry(SchemeRoot)?, opt_target_sc))
}

// this doesn't really make sense, because the only structure which is guaranteed to have
// the proper Scheme linking layout is the `SchemeRoot`.
#[hdk_extern]
pub fn get_linked_interchange_entries_which_unify(
    (target_hash, opt_target_sc): (EntryHash, Option<Scheme>),
) -> ExternResult<Vec<(HeaderHash, InterchangeEntry)>> {
    let scheme_entry_links = get_links(target_hash, None)?;
    let scheme_entry_hashes: Vec<EntryHash> = scheme_entry_links
        .into_iter()
        .map(|lnk| lnk.target)
        .collect();
    let filtered_scheme_entry_hashes: Vec<EntryHash> = match opt_target_sc {
        // if no target Scheme, we do not filter
        None => scheme_entry_hashes,
        // if yes target Scheme, we filter based on unification
        Some(target_sc) => {
            let mut is = InferState::new();
            let Scheme(_, normalized_target_ty) = normalize(&mut is, target_sc);
            scheme_entry_hashes
                .into_iter()
                .filter(|s_eh| {
                    let mut flag = || -> ExternResult<bool> {
                        // retrieve `SchemeEntry` element, decode to entry
                        let element = (match get(s_eh.clone(), GetOptions::content())? {
                            Some(el) => Ok(el),
                            None => Err(WasmError::Guest(format!(
                                "could not dereference hash: {}",
                                s_eh.clone()
                            ))),
                        })?;
                        let scheme_entry: SchemeEntry =
                            match element.into_inner().1.to_app_option()? {
                                Some(se) => Ok(se),
                                None => {
                                    Err(WasmError::Guest(format!("non-present arg: {}", *s_eh)))
                                }
                            }?;
                        // check unification of normalized type
                        let Scheme(_, normalized_candidate_ty) =
                            normalize(&mut is, scheme_entry.sc);
                        // we are only interested in whether a type error occured
                        Ok(unifies(normalized_target_ty.clone(), normalized_candidate_ty).is_ok())
                    };
                    // any `ExternResult` `Err`s are treated as disqualifiers for filtration
                    // purposes.
                    flag().unwrap_or(false)
                })
                .collect()
        }
    };
    filtered_scheme_entry_hashes
        .into_iter()
        .map(|s_eh| get_links(s_eh, None))
        .flatten()
        .flatten()
        .map(|lnk| get_interchange_entry(lnk.target))
        .collect()
}

/// this function creates an `InterchangeEntry`, whose `Scheme` is essentially
/// `forall a. List a`.
///
/// all IEs should have compatible `Scheme`s. this function will not check that,
/// but if `create_entry` is used later & type inference fails, the IE won't be
/// created.
pub fn pack_ies_into_list_ie(ies: Vec<HeaderHash>) -> ExternResult<InterchangeEntry> {
    let mut es = EvalState::new();

    let fresh_names: Vec<Name> = ies.iter().map(|_| es.fresh_name()).collect();

    // construct the list by `Cons`ing each element onto the accumulator
    let add_cons =
        |acc, nm: &Name| app!(Expr::Prim(PrimOp::Cons), app!(Expr::Var(nm.clone()), acc));
    let app_body = fresh_names.iter().fold(Expr::Prim(PrimOp::Nil), add_cons);

    // create the outer lambda, successively wrapping a lambda which
    // binds each fresh name.
    let wrap_lambda = |acc, nm| lam!(nm, acc);
    let full_lam = fresh_names.into_iter().rev().fold(app_body, wrap_lambda);

    let operands = ies
        .into_iter()
        .map(InterchangeOperand::InterchangeOperand)
        .collect();
    mk_interchange_entry(full_lam, operands)
}

/// assumes that the first `HeaderHash` is the operator, and that successive
/// `HeaderHash`es are operands. applies them in that order. does not check
/// whether types match up.
pub fn mk_application_ie(hh_s: Vec<HeaderHash>) -> ExternResult<InterchangeEntry> {
    // there must be at least an operator
    if hh_s.len() <= 1 {
        return Err(WasmError::Guest("no operator provided".into()));
    }

    let mut es = EvalState::new();

    let fresh_names: Vec<Name> = hh_s.iter().map(|_| es.fresh_name()).collect();

    let apply_vars = |acc, nm: &Name| app!(acc, Expr::Var(nm.clone()));
    // we pull out the operator, so it may be applied to the others
    let init_acc = Expr::Var(fresh_names[0].clone());
    // we skip the operator, since it's the init_acc
    let app_body = fresh_names.iter().skip(1).fold(init_acc, apply_vars);

    // create the outer lambda, successively wrapping a lambda which
    // binds each fresh name.
    let wrap_lambda = |acc, nm| lam!(nm, acc);
    let full_lam = fresh_names.into_iter().rev().fold(app_body, wrap_lambda);

    let operands = hh_s
        .into_iter()
        .map(InterchangeOperand::InterchangeOperand)
        .collect();
    mk_interchange_entry(full_lam, operands)
}

#[hdk_extern]
pub fn get_interchange_entry(arg_hash: EntryHash) -> ExternResult<(HeaderHash, InterchangeEntry)> {
    let element = (match get(arg_hash.clone(), GetOptions::content())? {
        Some(el) => Ok(el),
        None => Err(WasmError::Guest(format!(
            "could not dereference arg: {}",
            arg_hash
        ))),
    })?;
    let hh = element.header_address().clone();
    match element.into_inner().1.to_app_option()? {
        Some(ie) => Ok((hh, ie)),
        None => Err(WasmError::Guest(format!("non-present arg: {}", arg_hash))),
    }
}

pub fn mk_interchange_entry(
    expr: Expr,
    args: Vec<InterchangeOperand>,
) -> ExternResult<InterchangeEntry> {
    let arg_hh_s: Vec<HeaderHash> = args
        .iter()
        .map(|io| match io {
            InterchangeOperand::InterchangeOperand(hh) => hh.clone(),
            InterchangeOperand::OtherOperand(_) => todo!("OtherOperand"),
        })
        .collect();

    // don't need result, just a preliminary check before hitting DHT
    let _expr_sc =
        infer_expr_with_is(&Env::new(), &mut InferState::new(), &expr).map_err(|type_error| {
            WasmError::Guest(format!("type error in `expr`: {:?}", type_error))
        })?;

    // dereference `arg_hh_s`
    let int_entrs: Vec<InterchangeEntry> = arg_hh_s
        .iter()
        .cloned()
        .map(|arg_hash| {
            let element = must_get_valid_element(arg_hash.clone())?;
            match element.into_inner().1.to_app_option()? {
                Some(ie) => Ok(ie),
                None => Err(WasmError::Guest(format!("non-present arg: {}", arg_hash))),
            }
        })
        .collect::<ExternResult<_>>()?;

    let mut is = InferState::new();
    let mut es = EvalState::new();
    let mut type_env = Env::new();
    // we normalize up here, before conjuring fresh names for the `args`, in order to avoid
    // potential contamination. I'm not sure it is necessary, but doing it to be safe.
    let normalized_expr = expr.normalize(&mut HashMap::new(), &mut es);
    let arg_named_scheme_values: Vec<(Name, Scheme, FlatValue<Marker>)> = int_entrs
        .iter()
        .map(|ie| {
            (
                es.fresh_name(),
                infer::normalize(&mut is, ie.output_scheme.clone()),
                ie.output_flat_value.normalize(&mut HashMap::new(), &mut es),
            )
        })
        .collect();
    type_env.extends(
        arg_named_scheme_values
            .iter()
            .map(|t| (t.0.clone(), t.1.clone())),
    );

    let applicator = |bd, nm: Name| app!(bd, Expr::Var(nm));
    let full_application: Expr = arg_named_scheme_values
        .iter()
        .map(|t| t.0.clone())
        .fold(normalized_expr, applicator);

    // TODO substantiate whether this Scheme will have high-indexed `Tv`s, which might be
    // unintuitive / cause issues with programmatic `Scheme` matching.
    let full_application_sc =
        infer_expr_with_is(&type_env, &mut is, &full_application).map_err(|type_error| {
            WasmError::Guest(format!("type error in full application: {:?}", type_error))
        })?;

    let mut term_env = new_term_env();
    let mut sto: Sto<Marker> = Sto::new();

    for (nm, flat_val) in arg_named_scheme_values
        .iter()
        .map(|t| (t.0.clone(), t.2.clone()))
    {
        let v_ref =
            flat_thunk_to_sto_ref(&mut es, &mut sto, inject_flatvalue_to_flatthunk(flat_val));
        term_env.insert(nm, v_ref);
    }

    let full_application_vr = eval_(&term_env, &mut sto, &mut es, &full_application);
    let full_application_val = lookup_sto(&mut es, &full_application_vr, &mut sto);
    let full_application_flat_val = value_to_flat_value(&mut es, &full_application_val, &mut sto);

    let new_ie: InterchangeEntry = InterchangeEntry {
        operator: expr,
        operands: arg_hh_s
            .into_iter()
            .map(InterchangeOperand::InterchangeOperand)
            .collect(),
        output_scheme: full_application_sc,
        output_flat_value: full_application_flat_val,
        start_gas: es.current_gas_count(),
    };
    Ok(new_ie)
}

#[derive(Debug, Serialize, Deserialize, SerializedBytes)]
pub struct CreateInterchangeEntryInputParse {
    pub expr: String,
    pub args: Vec<String>,
}

/// INFO this is incomplete and doesn't currently parse the `args`
#[hdk_extern]
pub fn create_interchange_entry_parse(
    input: CreateInterchangeEntryInputParse,
) -> ExternResult<(HeaderHash, InterchangeEntry)> {
    let (hh, _eh, ie) = match expr().easy_parse(position::Stream::new(&input.expr[..])) {
        Err(err) => Err(WasmError::Guest(format!("parse error:\n\n{}\n", err))),
        Ok((expr, extra_input)) => {
            if extra_input.is_partial() {
                Err(WasmError::Guest(format!(
                    "error: unconsumed input: {:?}",
                    extra_input
                )))
            } else {
                create_interchange_entry_full(CreateInterchangeEntryInput {
                    expr,
                    // TODO parse `input.args`.
                    // parsing a HeaderHash seems like it should be possible, but I've not done
                    // it before. we might also want some indicator of which of
                    // InterchangeOperand constructor is desired?
                    args: Vec::new(),
                })
            }
        }
    }?;
    Ok((hh, ie))
}

pub fn create_interchange_entry_full(
    input: CreateInterchangeEntryInput,
) -> ExternResult<(HeaderHash, EntryHash, InterchangeEntry)> {
    let ie = mk_interchange_entry(input.expr, input.args)?;

    // create SchemeRoot (if needed)
    match get(hash_entry(&SchemeRoot)?, GetOptions::content())? {
        None => {
            let _hh = create_entry(&SchemeRoot)?;
        }
        Some(_) => {}
    };

    // create Scheme entry & link from SchemeRoot (if needed)
    let scheme_entry = SchemeEntry {
        sc: ie.output_scheme.clone(),
    };
    let scheme_entry_hash = hash_entry(&scheme_entry)?;
    match get(scheme_entry_hash.clone(), GetOptions::content())? {
        None => {
            let _hh = create_entry(&scheme_entry)?;
            create_link(
                hash_entry(SchemeRoot)?,
                scheme_entry_hash.clone(),
                LinkTag::new(OWNER_TAG),
            )?;
        }
        Some(_) => {}
    };

    // create IE & link from Scheme entry (if needed)
    let ie_eh = hash_entry(&ie)?;
    match get(ie_eh.clone(), GetOptions::content())? {
        None => {
            let hh = create_entry(&ie)?;
            create_link(scheme_entry_hash, ie_eh.clone(), LinkTag::new(OWNER_TAG))?;
            Ok((hh, ie_eh, ie))
        }
        Some(element) => Ok((element.header_address().clone(), ie_eh, ie)),
    }
}

#[hdk_extern]
pub fn get_interchange_entry_by_headerhash(
    arg_hash: HeaderHash,
) -> ExternResult<(EntryHash, InterchangeEntry)> {
    let element = (match get(arg_hash.clone(), GetOptions::content())? {
        Some(el) => Ok(el),
        None => Err(WasmError::Guest(format!(
            "could not dereference arg: {}",
            arg_hash
        ))),
    })?;
    match element.into_inner().1.to_app_option()? {
        Some(ie) => {
            let ie_hash = hash_entry(&ie)?;
            Ok((ie_hash, ie))
        }
        None => Err(WasmError::Guest(format!("non-present arg: {}", arg_hash))),
    }
}
