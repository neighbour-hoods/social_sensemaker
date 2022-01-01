use hdk::prelude::*;
use holochain::conductor::config::ConductorConfig;
// use holochain::sweettest::{SweetConductor, SweetNetwork, SweetZome};
use holochain::sweettest::{SweetAppBatch, SweetConductorBatch, SweetDnaFile};
// use holochain::test_utils::host_fn_caller::Post;
// use holochain::test_utils::wait_for_integration_1m;
// use holochain::test_utils::wait_for_integration_with_others_10s;
// use holochain::test_utils::WaitOps;
use std::path::Path;

const APP_ID: &str = "rep_interchange";
const ZOME_NAME: &str = "interpreter";

#[tokio::test(flavor = "multi_thread")]
pub async fn test_creation_retrieval_ie() -> anyhow::Result<()> {
    use holochain::test_utils::consistency_10s;
    use kitsune_p2p::KitsuneP2pConfig;
    use std::sync::Arc;

    use common::{CreateInterchangeEntryInput, InterchangeEntry};
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

    let path = Path::new("../../happs/rep_interchange/rep_interchange.dna");
    let dna_file = SweetDnaFile::from_bundle(path).await.unwrap();

    let apps = conductors.setup_app(APP_ID, &[dna_file]).await.unwrap();
    conductors.exchange_peer_info().await;

    let ((alice,), (bobbo,), (carol,)) = apps.into_tuples();

    let expr = Expr::Lit(Lit::LInt(0));
    let ciei = CreateInterchangeEntryInput {
        expr: expr.clone(),
        args: vec![],
    };
    let hh: HeaderHash = conductors[0]
        .call(&alice.zome(ZOME_NAME), "create_interchange_entry", ciei)
        .await;

    // wait for gossip to propagate
    // TODO figure out how to avoid an arbitrary hardcoded delay. can we check for consistency
    // async?
    consistency_10s(&[&alice, &bobbo, &carol]).await;

    {
        // assert correct retrieval
        let (_ie_hash, ie): (EntryHash, InterchangeEntry) = conductors[1]
            .call(
                &bobbo.zome(ZOME_NAME),
                "get_interchange_entry_by_headerhash",
                hh.clone(),
            )
            .await;
        assert_eq!(ie.operator, expr.clone());
    }

    {
        // assert correct retrieval
        let (_ie_hash, ie): (EntryHash, InterchangeEntry) = conductors[2]
            .call(
                &carol.zome(ZOME_NAME),
                "get_interchange_entry_by_headerhash",
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

    use common::{CreateInterchangeEntryInput, InterchangeEntry, InterchangeOperand};
    use rep_lang_core::{
        abstract_syntax::{Expr, Lit, PrimOp},
        app,
    };
    use rep_lang_runtime::eval::{FlatValue, Value};

    const NUM_CONDUCTORS: usize = 4;
    const ROUND_ROBIN_COUNT: usize = 51;

    let (conductors, apps) = setup_conductors_cells(NUM_CONDUCTORS).await;
    let cells = apps.cells_flattened();

    let init_ciei = CreateInterchangeEntryInput {
        expr: Expr::Lit(Lit::LInt(0)),
        args: vec![],
    };
    let hh: HeaderHash = conductors[0]
        .call(
            &cells[0].zome(ZOME_NAME),
            "create_interchange_entry",
            init_ciei,
        )
        .await;

    let mut last_ie_hh = hh;
    for idx in 0..ROUND_ROBIN_COUNT {
        // await consistency
        consistency_10s(&cells).await;

        let ciei = CreateInterchangeEntryInput {
            expr: app!(Expr::Prim(PrimOp::Add), Expr::Lit(Lit::LInt(1))),
            args: vec![InterchangeOperand::InterchangeOperand(last_ie_hh)],
        };

        let new_hh: HeaderHash = conductors[idx % NUM_CONDUCTORS]
            .call(
                &cells[idx % NUM_CONDUCTORS].zome(ZOME_NAME),
                "create_interchange_entry",
                ciei,
            )
            .await;

        last_ie_hh = new_hh;
    }

    // check final value
    consistency_10s(&cells).await;
    let (_final_ie_hash, final_ie): (EntryHash, InterchangeEntry) = conductors[0]
        .call(
            &cells[0].zome(ZOME_NAME),
            "get_interchange_entry_by_headerhash",
            last_ie_hh,
        )
        .await;
    assert_eq!(
        final_ie.output_flat_value,
        FlatValue(Value::VInt(ROUND_ROBIN_COUNT as i64))
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

    let path = Path::new("../../happs/rep_interchange/rep_interchange.dna");
    let dna_file = SweetDnaFile::from_bundle(path).await.unwrap();

    let apps = conductors.setup_app(APP_ID, &[dna_file]).await.unwrap();
    conductors.exchange_peer_info().await;

    (conductors, apps)
}
