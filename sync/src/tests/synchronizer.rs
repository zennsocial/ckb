use bigint::U256;
use ckb_chain::chain::{ChainBuilder, ChainController};
use ckb_chain_spec::consensus::Consensus;
use ckb_core::block::BlockBuilder;
use ckb_core::header::HeaderBuilder;
use ckb_core::transaction::{CellInput, CellOutput, TransactionBuilder};
use ckb_db::memorydb::MemoryKeyValueDB;
use ckb_notify::NotifyService;
use ckb_protocol::SyncMessage;
use ckb_shared::shared::{ChainProvider, Shared, SharedBuilder};
use ckb_shared::store::ChainKVStore;
use ckb_time::now_ms;
use flatbuffers::get_root;
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::thread;
use synchronizer::BLOCK_FETCH_TOKEN;
use tests::TestNode;
use {Config, Synchronizer, SYNC_PROTOCOL_ID};

#[test]
fn basic_sync() {
    let (mut node1, shared1) = setup_node(1);
    let (mut node2, shared2) = setup_node(3);

    node1.connect(&mut node2, SYNC_PROTOCOL_ID);

    let (signal_tx1, signal_rx1) = channel();
    thread::spawn(move || {
        node1.start(signal_tx1, |data| {
            let msg = get_root::<SyncMessage>(data);
            // terminate thread after 3 blocks
            msg.payload_as_block()
                .map(|block| block.header().unwrap().number() == 3)
                .unwrap_or(false)
        });
    });

    let (signal_tx2, _) = channel();
    thread::spawn(move || {
        node2.start(signal_tx2, |_| false);
    });

    // Wait node1 receive block from node2
    let _ = signal_rx1.recv();

    assert_eq!(shared1.tip_header().read().number(), 3);
    assert_eq!(
        shared1.tip_header().read().number(),
        shared2.tip_header().read().number()
    );
}

fn setup_node(height: u64) -> (TestNode, Shared<ChainKVStore<MemoryKeyValueDB>>) {
    let mut block = BlockBuilder::default().with_header_builder(
        HeaderBuilder::default()
            .timestamp(now_ms())
            .difficulty(&U256::from(1000)),
    );

    let consensus = Consensus::default().set_genesis_block(block.clone());
    let shared = SharedBuilder::<ChainKVStore<MemoryKeyValueDB>>::new_memory()
        .consensus(consensus)
        .build();
    let (chain_controller, chain_receivers) = ChainController::new();
    let (_handle, notify) = NotifyService::default().start::<&str>(None);

    let chain_service = ChainBuilder::new(shared.clone())
        .notify(notify.clone())
        .build();
    let _handle = chain_service.start::<&str>(None, chain_receivers);

    for _i in 0..height {
        let number = block.header().number() + 1;
        let timestamp = block.header().timestamp() + 1;
        let difficulty = shared.calculate_difficulty(&block.header()).unwrap();
        let cellbase = TransactionBuilder::default()
            .input(CellInput::new_cellbase_input(number))
            .output(CellOutput::default())
            .build();

        let header_builder = HeaderBuilder::default()
            .parent_hash(&block.header().hash())
            .number(number)
            .timestamp(timestamp)
            .difficulty(&difficulty)
            .cellbase_id(&cellbase.hash());

        block = BlockBuilder::default()
            .commit_transaction(cellbase)
            .with_header_builder(header_builder);

        chain_controller
            .process_block(Arc::new(block.clone()))
            .expect("process block should be OK");
    }

    let synchronizer = Synchronizer::new(chain_controller, shared.clone(), Config::default());
    let mut node = TestNode::default();
    node.add_protocol(
        SYNC_PROTOCOL_ID,
        Arc::new(synchronizer),
        vec![BLOCK_FETCH_TOKEN],
    );
    (node, shared)
}
