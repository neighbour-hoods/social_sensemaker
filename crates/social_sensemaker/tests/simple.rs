use hdk::prelude::*;
use holochain::conductor::config::ConductorConfig;
// use holochain::sweettest::{SweetConductor, SweetNetwork, SweetZome};
use holochain::sweettest::{SweetAppBatch, SweetConductorBatch, SweetDnaFile};
// use holochain::test_utils::host_fn_caller::Post;
// use holochain::test_utils::wait_for_integration_1m;
// use holochain::test_utils::wait_for_integration_with_others_10s;
// use holochain::test_utils::WaitOps;
use std::path::Path;

const APP_ID: &str = "rep_sensemaker";
const ZOME_NAME: &str = "interpreter";

#[tokio::test(flavor = "multi_thread")]
pub async fn test_creation_retrieval_ie() -> anyhow::Result<()> {
    use holochain::test_utils::consistency_10s;
    use kitsune_p2p::KitsuneP2pConfig;
    use std::sync::Arc;

    use common::{CreateSensemakerEntryInput, SensemakerEntry};
    use rep_lang_core::abstract_syntax::{Expr, Lit};

    let _g = observability::test_run().ok();
    const NUM_CONDUCTORS: usize = 3;

    let mut tuning =
        kitsune_p2p_types::config::tuning_params_struct::KitsuneP2pTuningParams::default();
    tuning.gossip_strategy = "none".to_string();

    let mut network = KitsuneP2pConfig::default();
    network.tuning_params = Arc::new(tuning);
    let mut config = ConductorConfig::default();
    config.network = Some(network);
    let mut conductors = SweetConductorBatch::from_config(NUM_CONDUCTORS, config).await;

    let path = Path::new("../../happs/social_sensemaker/social_sensemaker.dna");
    let dna_file = SweetDnaFile::from_bundle(path).await.unwrap();

    let apps = conductors.setup_app(APP_ID, &[dna_file]).await.unwrap();
    conductors.exchange_peer_info().await;

    let ((alice,), (bobbo,), (carol,)) = apps.into_tuples();

    let expr = Expr::Lit(Lit::LInt(0));
    let ciei = CreateSensemakerEntryInput {
        expr: expr.clone(),
        args: vec![],
    };
    let hh: HeaderHash = conductors[0]
        .call(&alice.zome(ZOME_NAME), "create_sensemaker_entry", ciei)
        .await;

    // wait for gossip to propagate
    // TODO figure out how to avoid an arbitrary hardcoded delay. can we check for consistency
    // async?
    consistency_10s(&[&alice, &bobbo, &carol]).await;

    {
        // assert correct retrieval
        let (_ie_hash, ie): (EntryHash, SensemakerEntry) = conductors[1]
            .call(
                &bobbo.zome(ZOME_NAME),
                "get_sensemaker_entry_by_headerhash",
                hh.clone(),
            )
            .await;
        assert_eq!(ie.operator, expr.clone());
    }

    {
        // assert correct retrieval
        let (_ie_hash, ie): (EntryHash, SensemakerEntry) = conductors[2]
            .call(
                &carol.zome(ZOME_NAME),
                "get_sensemaker_entry_by_headerhash",
                hh,
            )
            .await;
        assert_eq!(ie.operator, expr);
    }

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
pub async fn test_round_robin_incrementation() -> anyhow::Result<()> {
    use holochain::test_utils::consistency_10s;

    use common::{CreateSensemakerEntryInput, SensemakerEntry, SensemakerOperand};
    use rep_lang_core::{
        abstract_syntax::{Expr, Lit, PrimOp},
        app,
    };
    use rep_lang_runtime::eval::{FlatValue, Value};

    const NUM_CONDUCTORS: usize = 4;
    const ROUND_ROBIN_COUNT: usize = 51;

    let (conductors, apps) = setup_conductors_cells(NUM_CONDUCTORS).await;
    let cells = apps.cells_flattened();

    let init_ciei = CreateSensemakerEntryInput {
        expr: Expr::Lit(Lit::LInt(0)),
        args: vec![],
    };
    let hh: HeaderHash = conductors[0]
        .call(
            &cells[0].zome(ZOME_NAME),
            "create_sensemaker_entry",
            init_ciei,
        )
        .await;

    let mut last_ie_hh = hh;
    for idx in 0..ROUND_ROBIN_COUNT {
        // await consistency
        consistency_10s(&cells).await;

        let ciei = CreateSensemakerEntryInput {
            expr: app!(Expr::Prim(PrimOp::Add), Expr::Lit(Lit::LInt(1))),
            args: vec![SensemakerOperand::SensemakerOperand(last_ie_hh)],
        };

        let new_hh: HeaderHash = conductors[idx % NUM_CONDUCTORS]
            .call(
                &cells[idx % NUM_CONDUCTORS].zome(ZOME_NAME),
                "create_sensemaker_entry",
                ciei,
            )
            .await;

        last_ie_hh = new_hh;
    }

    // check final value
    consistency_10s(&cells).await;
    let (_final_ie_hash, final_ie): (EntryHash, SensemakerEntry) = conductors[0]
        .call(
            &cells[0].zome(ZOME_NAME),
            "get_sensemaker_entry_by_headerhash",
            last_ie_hh,
        )
        .await;
    assert_eq!(
        final_ie.output_flat_value,
        FlatValue(Value::VInt(ROUND_ROBIN_COUNT as i64))
    );

    Ok(())
}

/// test arity-2 functions with `fib`.
/// the fibonacci sequences starts off `0, 1, 1, 2, 3, 5, 8, 13 ...`. each term is
/// the sum of the previous 2 terms. we start off by creating 2
/// `SensemakerEntry`s with values `0 :: Int` & `1 :: Int`, respectively.
#[tokio::test(flavor = "multi_thread")]
pub async fn test_round_robin_fibonacci() -> anyhow::Result<()> {
    use holochain::test_utils::consistency_10s;

    use common::{CreateSensemakerEntryInput, SensemakerEntry, SensemakerOperand};
    use rep_lang_core::abstract_syntax::{Expr, Lit, PrimOp};
    use rep_lang_runtime::eval::{FlatValue, Value};

    const NUM_CONDUCTORS: usize = 5;
    const ROUND_ROBIN_COUNT: usize = 37;

    let (conductors, apps) = setup_conductors_cells(NUM_CONDUCTORS).await;
    let cells = apps.cells_flattened();

    // TODO can the commonalities be abstracted? unsure about async closure
    // capturing env.
    let mut hh_0 = {
        let init_ciei = CreateSensemakerEntryInput {
            expr: Expr::Lit(Lit::LInt(0)),
            args: vec![],
        };
        let init_hh: HeaderHash = conductors[0]
            .call(
                &cells[0].zome(ZOME_NAME),
                "create_sensemaker_entry",
                init_ciei,
            )
            .await;
        init_hh
    };
    let mut hh_1 = {
        let init_ciei = CreateSensemakerEntryInput {
            expr: Expr::Lit(Lit::LInt(1)),
            args: vec![],
        };
        let init_hh: HeaderHash = conductors[0]
            .call(
                &cells[0].zome(ZOME_NAME),
                "create_sensemaker_entry",
                init_ciei,
            )
            .await;
        init_hh
    };

    for idx in 1..ROUND_ROBIN_COUNT {
        // await consistency
        consistency_10s(&cells).await;

        let ciei = CreateSensemakerEntryInput {
            expr: Expr::Prim(PrimOp::Add),
            args: vec![
                SensemakerOperand::SensemakerOperand(hh_0.clone()),
                SensemakerOperand::SensemakerOperand(hh_1.clone()),
            ],
        };

        let new_hh: HeaderHash = conductors[idx % NUM_CONDUCTORS]
            .call(
                &cells[idx % NUM_CONDUCTORS].zome(ZOME_NAME),
                "create_sensemaker_entry",
                ciei,
            )
            .await;

        hh_0 = hh_1;
        hh_1 = new_hh;
    }

    // check final value
    consistency_10s(&cells).await;
    let (_final_ie_hash, final_ie): (EntryHash, SensemakerEntry) = conductors[0]
        .call(
            &cells[0].zome(ZOME_NAME),
            "get_sensemaker_entry_by_headerhash",
            hh_1,
        )
        .await;
    assert_eq!(
        final_ie.output_flat_value,
        FlatValue(Value::VInt(nth_fib(ROUND_ROBIN_COUNT as i64)))
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
pub async fn test_round_robin_arity_n_sum() -> anyhow::Result<()> {
    use holochain::test_utils::consistency_10s;

    use common::{CreateSensemakerEntryInput, SensemakerEntry, SensemakerOperand};
    use rep_lang_core::{
        abstract_syntax::{Expr, Lit, Name, PrimOp},
        app,
    };
    use rep_lang_runtime::eval::{FlatValue, Value};

    const NUM_CONDUCTORS: usize = 5;
    const ROUND_ROBIN_COUNT: usize = 37;

    let (conductors, apps) = setup_conductors_cells(NUM_CONDUCTORS).await;
    let cells = apps.cells_flattened();

    let init_ciei = CreateSensemakerEntryInput {
        expr: Expr::Lit(Lit::LInt(1)),
        args: vec![],
    };
    let hh: HeaderHash = conductors[0]
        .call(
            &cells[0].zome(ZOME_NAME),
            "create_sensemaker_entry",
            init_ciei,
        )
        .await;

    let mut arg_hh_s = vec![hh];
    for idx in 0..=ROUND_ROBIN_COUNT {
        // await consistency
        consistency_10s(&cells).await;

        let expr = {
            // generate fresh names
            let names: Vec<Name> = (0..arg_hh_s.len())
                .map(|n| Name(format!("arg_{}", n)))
                .collect();

            // wrap said fresh names into `Expr`s
            let name_vars = names.clone().into_iter().map(Expr::Var);

            // fold a summation over the args, with accumulator 0
            let app_f = |acc, arg| app!(app!(Expr::Prim(PrimOp::Add), acc), arg);
            let app = name_vars.fold(Expr::Lit(Lit::LInt(0)), app_f);

            // fold over the generated freshnames to construct a lambda which will bind the
            // names used in the applicaton
            let lam_f = |bd, nm| Expr::Lam(nm, Box::new(bd));
            names.into_iter().rev().fold(app, lam_f)
        };
        let ciei = CreateSensemakerEntryInput {
            expr,
            args: arg_hh_s
                .iter()
                .cloned()
                .map(SensemakerOperand::SensemakerOperand)
                .collect(),
        };

        let new_hh: HeaderHash = conductors[idx % NUM_CONDUCTORS]
            .call(
                &cells[idx % NUM_CONDUCTORS].zome(ZOME_NAME),
                "create_sensemaker_entry",
                ciei,
            )
            .await;

        arg_hh_s.push(new_hh);
    }

    let final_hh = arg_hh_s.last().expect("args should be non-empty");

    // check final value
    consistency_10s(&cells).await;
    let (_final_ie_hash, final_ie): (EntryHash, SensemakerEntry) = conductors[0]
        .call(
            &cells[0].zome(ZOME_NAME),
            "get_sensemaker_entry_by_headerhash",
            final_hh,
        )
        .await;
    assert_eq!(
        final_ie.output_flat_value,
        FlatValue(Value::VInt(nth_sum_all(ROUND_ROBIN_COUNT as u32)))
    );

    Ok(())
}

////////////////////////////////////////////////////////////////////////////////
// helpers
////////////////////////////////////////////////////////////////////////////////
async fn setup_conductors_cells(num_conductors: usize) -> (SweetConductorBatch, SweetAppBatch) {
    use kitsune_p2p::KitsuneP2pConfig;
    use std::sync::Arc;

    let _g = observability::test_run().ok();

    let mut tuning =
        kitsune_p2p_types::config::tuning_params_struct::KitsuneP2pTuningParams::default();
    tuning.gossip_strategy = "none".to_string();

    let mut network = KitsuneP2pConfig::default();
    network.tuning_params = Arc::new(tuning);
    let mut config = ConductorConfig::default();
    config.network = Some(network);
    let mut conductors = SweetConductorBatch::from_config(num_conductors, config).await;

    let path = Path::new("../../happs/social_sensemaker/social_sensemaker.dna");
    let dna_file = SweetDnaFile::from_bundle(path).await.unwrap();

    let apps = conductors.setup_app(APP_ID, &[dna_file]).await.unwrap();
    conductors.exchange_peer_info().await;

    (conductors, apps)
}

fn nth_fib(mut n: i64) -> i64 {
    let mut x0 = 0;
    let mut x1 = 1;
    while n > 1 {
        n -= 1;
        let tmp = x0 + x1;
        x0 = x1;
        x1 = tmp;
    }
    x1
}

fn nth_sum_all(n: u32) -> i64 {
    2_i64.pow(n)
}
