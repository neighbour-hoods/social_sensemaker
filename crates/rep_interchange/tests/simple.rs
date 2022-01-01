use hdk::prelude::*;
use holochain::conductor::config::ConductorConfig;
// use holochain::sweettest::{SweetConductor, SweetNetwork, SweetZome};
use holochain::sweettest::{SweetConductorBatch, SweetDnaFile};
// use holochain::test_utils::host_fn_caller::Post;
// use holochain::test_utils::wait_for_integration_1m;
// use holochain::test_utils::wait_for_integration_with_others_10s;
// use holochain::test_utils::WaitOps;

#[tokio::test(flavor = "multi_thread")]
pub async fn basic() {
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn my_test() {
    assert!(true);
}

#[tokio::test(flavor = "multi_thread")]
pub async fn test0() -> anyhow::Result<()> {
    use holochain::test_utils::{consistency_10s, inline_zomes::simple_create_read_zome};
    use kitsune_p2p::KitsuneP2pConfig;
    use std::sync::Arc;

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

    let (dna_file, _) = SweetDnaFile::unique_from_inline_zome("zome1", simple_create_read_zome())
        .await
        .unwrap();

    let apps = conductors.setup_app("app", &[dna_file]).await.unwrap();
    conductors.exchange_peer_info().await;

    let ((alice,), (bobbo,), (carol,)) = apps.into_tuples();

    // Call the "create" zome fn on Alice's app
    let hash: HeaderHash = conductors[0].call(&alice.zome("zome1"), "create", ()).await;

    // Wait long enough for Bob to receive gossip
    consistency_10s(&[&alice, &bobbo, &carol]).await;

    // Verify that bobbo can run "read" on his cell and get alice's Header
    let element: Option<Element> = conductors[1].call(&bobbo.zome("zome1"), "read", hash).await;
    let element = element.expect("Element was None: bobbo couldn't `get` it");

    // Assert that the Element bobbo sees matches what alice committed
    assert_eq!(element.header().author(), alice.agent_pubkey());
    assert_eq!(
        *element.entry(),
        ElementEntry::Present(Entry::app(().try_into().unwrap()).unwrap())
    );

    Ok(())
}
