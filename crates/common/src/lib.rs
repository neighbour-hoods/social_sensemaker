#![crate_type = "proc-macro"]

use hdk::{
    entry::must_get_valid_element,
    prelude::{holo_hash::DnaHash, *},
};

use combine::{stream::position, EasyParser, StreamOnce};
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
use social_sensemaker_macros::expand_remote_calls;

pub mod util;

pub const OWNER_TAG: &str = "sensemaker_owner";
pub const SENSEMAKER_ZOME_NAME: &str = "sensemaker_main";
pub const SM_COMP_TAG: &str = "sm_comp";
pub const SM_INIT_TAG: &str = "sm_init";
pub const SM_DATA_TAG: &str = "sm_data";

// TODO think carefully on what this should be.
pub type Marker = ();

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SensemakerOperand {
    // these dereference to `SensemakerEntry`
    SensemakerOperand(HeaderHash),
    // these dereference to `FlatThunk`??
    OtherOperand(HeaderHash),
}

impl SensemakerOperand {
    pub fn ppr(&self) -> RcDoc<()> {
        match &self {
            SensemakerOperand::SensemakerOperand(hh) => {
                RcDoc::text(format!("SensemakerOperand({})", hh))
            }
            SensemakerOperand::OtherOperand(hh) => RcDoc::text(format!("OtherOperand({})", hh)),
        }
    }
}

#[hdk_entry(id = "sensemaker_entry")]
#[derive(Clone)]
pub struct SensemakerEntry {
    pub operator: Expr,
    pub operands: Vec<SensemakerOperand>,
    pub output_scheme: Scheme,
    pub output_flat_value: FlatValue<Marker>,
    pub start_gas: Gas,
}

impl SensemakerEntry {
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
            RcDoc::text("SensemakerEntry {"),
            RcDoc::concat(docs).nest(2),
            RcDoc::line(),
            RcDoc::text("}"),
        ])
    }
}

/// input to `create_sensemaker_entry_full`
#[derive(Debug, Serialize, Deserialize, SerializedBytes)]
pub struct CreateSensemakerEntryInput {
    pub expr: Expr,
    pub args: Vec<SensemakerOperand>,
}

#[hdk_entry]
pub struct SchemeRoot;

#[hdk_entry]
pub struct SchemeEntry {
    pub sc: Scheme,
}

// functions

#[hdk_extern]
pub fn get_sensemaker_entries_which_unify(
    opt_target_sc: Option<Scheme>,
) -> ExternResult<Vec<(HeaderHash, SensemakerEntry)>> {
    get_linked_sensemaker_entries_which_unify((hash_entry(SchemeRoot)?, opt_target_sc))
}

// this doesn't really make sense, because the only structure which is guaranteed to have
// the proper Scheme linking layout is the `SchemeRoot`.
#[hdk_extern]
pub fn get_linked_sensemaker_entries_which_unify(
    (target_hash, opt_target_sc): (EntryHash, Option<Scheme>),
) -> ExternResult<Vec<(HeaderHash, SensemakerEntry)>> {
    let scheme_entry_links = get_links(target_hash, None)?;
    let scheme_entry_hashes: Vec<EntryHash> = scheme_entry_links
        .into_iter()
        .map(|lnk| {
            lnk.target
                .into_entry_hash()
                .expect("Link target should be Entry.")
        })
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
        .flat_map(|s_eh| get_links(s_eh, None))
        .flatten()
        .map(|lnk| {
            get_sensemaker_entry(
                lnk.target
                    .into_entry_hash()
                    .expect("Link target should be Entry."),
            )
        })
        .collect()
}

/// this function creates an `SensemakerEntry`, whose `Scheme` is essentially
/// `forall a. List a`.
///
/// all SEs should have compatible `Scheme`s. this function will not check that,
/// but if `create_entry` is used later & type inference fails, the SE won't be
/// created.
pub fn pack_ses_into_list_se(ses: Vec<HeaderHash>) -> ExternResult<SensemakerEntry> {
    let mut es = EvalState::new();

    let fresh_names: Vec<Name> = ses.iter().map(|_| es.fresh_name()).collect();

    // construct the list by `Cons`ing each element onto the accumulator
    let add_cons =
        |acc, nm: &Name| app!(app!(Expr::Prim(PrimOp::Cons), Expr::Var(nm.clone())), acc);
    let app_body = fresh_names.iter().fold(Expr::Prim(PrimOp::Nil), add_cons);

    // create the outer lambda, successively wrapping a lambda which
    // binds each fresh name.
    let wrap_lambda = |acc, nm| lam!(nm, acc);
    let full_lam = fresh_names.into_iter().rev().fold(app_body, wrap_lambda);

    let operands = ses
        .into_iter()
        .map(SensemakerOperand::SensemakerOperand)
        .collect();
    mk_sensemaker_entry(full_lam, operands)
}

/// assumes that the first `HeaderHash` is the operator, and that successive
/// `HeaderHash`es are operands. applies them in that order. does not check
/// whether types match up.
pub fn mk_application_se(hh_s: Vec<HeaderHash>) -> ExternResult<SensemakerEntry> {
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
        .map(SensemakerOperand::SensemakerOperand)
        .collect();
    mk_sensemaker_entry(full_lam, operands)
}

#[hdk_extern]
pub fn get_sensemaker_entry(arg_hash: EntryHash) -> ExternResult<(HeaderHash, SensemakerEntry)> {
    let element = (match get(arg_hash.clone(), GetOptions::content())? {
        Some(el) => Ok(el),
        None => Err(WasmError::Guest(format!(
            "could not dereference arg: {}",
            arg_hash
        ))),
    })?;
    let hh = element.header_address().clone();
    match element.into_inner().1.to_app_option()? {
        Some(se) => Ok((hh, se)),
        None => Err(WasmError::Guest(format!("non-present arg: {}", arg_hash))),
    }
}

pub fn mk_sensemaker_entry(
    expr: Expr,
    args: Vec<SensemakerOperand>,
) -> ExternResult<SensemakerEntry> {
    let arg_hh_s: Vec<HeaderHash> = args
        .iter()
        .map(|io| match io {
            SensemakerOperand::SensemakerOperand(hh) => hh.clone(),
            SensemakerOperand::OtherOperand(_) => todo!("OtherOperand"),
        })
        .collect();

    // don't need result, just a preliminary check before hitting DHT
    let _expr_sc =
        infer_expr_with_is(&Env::new(), &mut InferState::new(), &expr).map_err(|type_error| {
            WasmError::Guest(format!("type error in `expr`: {:?}", type_error))
        })?;

    // dereference `arg_hh_s`
    let int_entrs: Vec<SensemakerEntry> = arg_hh_s
        .iter()
        .cloned()
        .map(|arg_hash| {
            let element = must_get_valid_element(arg_hash.clone())?;
            match element.into_inner().1.to_app_option()? {
                Some(se) => Ok(se),
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
        .map(|se| {
            (
                es.fresh_name(),
                infer::normalize(&mut is, se.output_scheme.clone()),
                se.output_flat_value.normalize(&mut HashMap::new(), &mut es),
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

    let new_se: SensemakerEntry = SensemakerEntry {
        operator: expr,
        operands: arg_hh_s
            .into_iter()
            .map(SensemakerOperand::SensemakerOperand)
            .collect(),
        output_scheme: full_application_sc,
        output_flat_value: full_application_flat_val,
        start_gas: es.current_gas_count(),
    };
    Ok(new_se)
}

#[derive(Debug, Serialize, Deserialize, SerializedBytes)]
pub struct CreateSensemakerEntryInputParse {
    pub expr: String,
    pub args: Vec<String>,
}

/// INFO this is incomplete and doesn't currently parse the `args`
#[hdk_extern]
pub fn create_sensemaker_entry_parse(
    input: CreateSensemakerEntryInputParse,
) -> ExternResult<(HeaderHash, SensemakerEntry)> {
    let (hh, _eh, se) = match expr().easy_parse(position::Stream::new(&input.expr[..])) {
        Err(err) => Err(WasmError::Guest(format!("parse error:\n\n{}\n", err))),
        Ok((expr, extra_input)) => {
            if extra_input.is_partial() {
                Err(WasmError::Guest(format!(
                    "error: unconsumed input: {:?}",
                    extra_input
                )))
            } else {
                create_sensemaker_entry_full(CreateSensemakerEntryInput {
                    expr,
                    // TODO parse `input.args`.
                    // parsing a HeaderHash seems like it should be possible, but I've not done
                    // it before. we might also want some indicator of which of
                    // SensemakerOperand constructor is desired?
                    args: Vec::new(),
                })
            }
        }
    }?;
    Ok((hh, se))
}

pub fn create_sensemaker_entry_full(
    input: CreateSensemakerEntryInput,
) -> ExternResult<(HeaderHash, EntryHash, SensemakerEntry)> {
    let se = mk_sensemaker_entry(input.expr, input.args)?;

    // create SchemeRoot (if needed)
    match get(hash_entry(&SchemeRoot)?, GetOptions::content())? {
        None => {
            let _hh = create_entry(&SchemeRoot)?;
        }
        Some(_) => {}
    };

    // create Scheme entry & link from SchemeRoot (if needed)
    let scheme_entry = SchemeEntry {
        sc: se.output_scheme.clone(),
    };
    let scheme_entry_hash = hash_entry(&scheme_entry)?;
    match get(scheme_entry_hash.clone(), GetOptions::content())? {
        None => {
            let _hh = create_entry(&scheme_entry)?;
            create_link(
                hash_entry(SchemeRoot)?,
                scheme_entry_hash.clone(),
                LinkType::new(0),
                LinkTag::new(OWNER_TAG),
            )?;
        }
        Some(_) => {}
    };

    // create SE & link from Scheme entry (if needed)
    let se_eh = hash_entry(&se)?;
    match get(se_eh.clone(), GetOptions::content())? {
        None => {
            let hh = create_entry(&se)?;
            create_link(
                scheme_entry_hash,
                se_eh.clone(),
                LinkType::new(0),
                LinkTag::new(OWNER_TAG),
            )?;
            Ok((hh, se_eh, se))
        }
        Some(element) => Ok((element.header_address().clone(), se_eh, se)),
    }
}

#[hdk_extern]
pub fn get_sensemaker_entry_by_headerhash(
    arg_hash: HeaderHash,
) -> ExternResult<(EntryHash, SensemakerEntry)> {
    let element = (match get(arg_hash.clone(), GetOptions::content())? {
        Some(el) => Ok(el),
        None => Err(WasmError::Guest(format!(
            "could not dereference arg: {}",
            arg_hash
        ))),
    })?;
    match element.into_inner().1.to_app_option()? {
        Some(se) => {
            let se_hash = hash_entry(&se)?;
            Ok((se_hash, se))
        }
        None => Err(WasmError::Guest(format!("non-present arg: {}", arg_hash))),
    }
}

pub fn get_latest_path_entry(
    path_string: String,
    link_tag_string: String,
) -> ExternResult<Option<EntryHash>> {
    let path = Path::from(path_string);
    get_latest_linked_entry(path.path_entry_hash()?, link_tag_string)
}

pub fn get_latest_linked_entry(
    target: EntryHash,
    link_tag_string: String,
) -> ExternResult<Option<EntryHash>> {
    let links = get_links(target, Some(LinkTag::new(link_tag_string)))?;
    match links
        .into_iter()
        .max_by(|x, y| x.timestamp.cmp(&y.timestamp))
    {
        None => Ok(None),
        Some(link) => Ok(Some(
            link.target.into_entry_hash().expect("Should be an entry."),
        )),
    }
}

////////////////////////////////////////////////////////////////////////////////
// "remote" code, to be imported-by / called-in widgets
////////////////////////////////////////////////////////////////////////////////

#[hdk_entry]
#[derive(Clone)]
pub struct SensemakerCellId {
    // must include extension
    pub dna_hash: DnaHash,
    // encoded file bytes payload
    pub agent_pubkey: AgentPubKey,
}

impl SensemakerCellId {
    pub fn to_cell_id(self) -> CellId {
        CellId::new(self.dna_hash, self.agent_pubkey)
    }
}

pub fn sensemaker_cell_id_anchor() -> ExternResult<EntryHash> {
    anchor("sensemaker_cell_id".into(), "".into())
}

#[macro_export]
macro_rules! sensemaker_cell_id_fns {
    () => {
        #[hdk_extern]
        fn set_sensemaker_cell_id(
            (dna_hash, agent_pubkey): (DnaHash, AgentPubKey),
        ) -> ExternResult<HeaderHash> {
            let sensemaker_cell_id: SensemakerCellId = SensemakerCellId {
                dna_hash,
                agent_pubkey,
            };
            let sensemaker_cell_id_hh = create_entry(sensemaker_cell_id.clone())?;
            let sensemaker_cell_id_eh = hash_entry(sensemaker_cell_id)?;
            create_link(
                sensemaker_cell_id_anchor()?,
                sensemaker_cell_id_eh,
                LinkType(0),
                LinkTag::new(OWNER_TAG),
            )?;

            Ok(sensemaker_cell_id_hh)
        }

        #[hdk_extern]
        fn get_sensemaker_cell_id(_: ()) -> ExternResult<CellId> {
            match get_latest_linked_entry(sensemaker_cell_id_anchor()?, OWNER_TAG.into())? {
                Some(entryhash) => {
                    let sensemaker_cell_id_entry: SensemakerCellId =
                        util::try_get_and_convert(entryhash.clone(), GetOptions::content())?;
                    Ok(sensemaker_cell_id_entry.to_cell_id())
                }
                None => Err(WasmError::Guest(
                    "get_sensemaker_cell_id: no cell_id".into(),
                )),
            }
        }
    };
}

#[expand_remote_calls]
#[hdk_extern]
pub fn get_sensemaker_entry_by_path(
    (path_string, link_tag_string): (String, String),
) -> ExternResult<Option<(EntryHash, SensemakerEntry)>> {
    match get_latest_path_entry(path_string, link_tag_string)? {
        Some(entryhash) => {
            let sensemaker_entry =
                util::try_get_and_convert(entryhash.clone(), GetOptions::content())?;
            Ok(Some((entryhash, sensemaker_entry)))
        }
        None => Ok(None),
    }
}

#[expand_remote_calls]
#[hdk_extern]
pub fn set_sensemaker_entry(
    (path_string, link_tag_string, target_eh): (String, String, EntryHash),
) -> ExternResult<()> {
    let path = Path::try_from(path_string)?;
    path.ensure()?;
    let anchor_hash = path.path_entry_hash()?;
    create_link(
        anchor_hash,
        target_eh,
        LinkType(0),
        LinkTag::new(link_tag_string),
    )?;
    Ok(())
}

#[expand_remote_calls]
#[hdk_extern]
pub fn set_sensemaker_entry_parse_rl_expr(
    (path_string, link_tag_string, expr_str): (String, String, String),
) -> ExternResult<()> {
    let (_, sensemaker_entry) = create_sensemaker_entry_parse(CreateSensemakerEntryInputParse {
        expr: expr_str,
        args: vec![],
    })?;
    let sensemaker_entryhash = hash_entry(sensemaker_entry)?;

    set_sensemaker_entry((path_string, link_tag_string, sensemaker_entryhash))
}

#[expand_remote_calls]
#[hdk_extern]
pub fn initialize_sm_data((path_string, target_eh): (String, EntryHash)) -> ExternResult<()> {
    let target_path_string = format!("{}.{}", path_string, target_eh);
    match get_latest_path_entry(path_string, SM_INIT_TAG.into())? {
        None => Err(WasmError::Guest("initialize_sm_data: no sm_init".into())),
        Some(init_eh) => set_sensemaker_entry((target_path_string, SM_DATA_TAG.into(), init_eh)),
    }
}

#[expand_remote_calls]
#[hdk_extern]
pub fn step_sm((path_string, entry_hash, act): (String, EntryHash, String)) -> ExternResult<()> {
    let sm_data_path: String = format!("{}.{}", path_string, entry_hash);

    // fetch sm_data
    let (sm_data_eh, _sm_data_entry) =
        match get_sensemaker_entry_by_path((sm_data_path.clone(), "sm_data".into()))? {
            Some(pair) => Ok(pair),
            None => Err(WasmError::Guest("sm_data: invalid".into())),
        }?;

    // fetch sm_comp
    let (sm_comp_eh, _sm_comp_entry) =
        match get_sensemaker_entry_by_path((path_string, "sm_comp".into()))? {
            Some(pair) => Ok(pair),
            None => Err(WasmError::Guest("sm_comp: invalid".into())),
        }?;

    let sm_comp_hh = util::get_hh(sm_comp_eh, GetOptions::content())?;
    let sm_data_hh = util::get_hh(sm_data_eh, GetOptions::content())?;

    // create action SensemakerEntry
    let (act_se_hh, _act_se) = create_sensemaker_entry_parse(CreateSensemakerEntryInputParse {
        expr: act,
        args: vec![],
    })?;

    // compose application SensemakerEntry & create it
    let application_se = mk_application_se(vec![sm_comp_hh, sm_data_hh, act_se_hh])?;
    debug!("{:?}", application_se);
    let _application_se_hh = create_entry(&application_se)?;
    let application_se_eh = hash_entry(&application_se)?;
    debug!("{:?}", application_se_eh);
    {
        let path = Path::from(sm_data_path);
        path.ensure()?;
        let path_hash = path.path_entry_hash()?;
        let hh = create_link(
            path_hash,
            application_se_eh,
            LinkType(0),
            LinkTag::new("sm_data"),
        );
        debug!("create_link hh : {:?}", hh);
    }
    Ok(())
}
