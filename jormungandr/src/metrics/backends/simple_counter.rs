use crate::blockchain::Ref;
use crate::metrics::MetricsBackend;

use chain_impl_mockchain::block::Block;
use chain_impl_mockchain::fragment::Fragment;
use chain_impl_mockchain::transaction::Transaction;
use chain_impl_mockchain::value::{Value, ValueError};
use jormungandr_lib::interfaces::NodeStats;
use jormungandr_lib::time::{SecondsSinceUnixEpoch, SystemTime};

use std::convert::TryInto;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use arc_swap::ArcSwapOption;

pub struct SimpleCounter {
    tx_recv_cnt: AtomicUsize,
    block_recv_cnt: AtomicUsize,
    slot_start_time: AtomicU64,
    peers_connected_cnt: AtomicUsize,
    peers_quarantined_cnt: AtomicUsize,
    peers_available_cnt: AtomicUsize,
    tip_block: ArcSwapOption<BlockCounters>,
    start_time: Instant,
}

struct BlockCounters {
    block_tx_count: u64,
    block_input_sum: u64,
    block_fee_sum: u64,
    content_size: u32,
    date: String,
    hash: String,
    chain_length: String,
    time: SystemTime,
}

impl SimpleCounter {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn get_stats(&self) -> NodeStats {
        let peer_available_cnt = self.peers_available_cnt.load(Ordering::Relaxed);
        let peer_quarantined_cnt = self.peers_quarantined_cnt.load(Ordering::SeqCst);
        let peer_total_cnt = peer_available_cnt + peer_quarantined_cnt;

        let block_data = self.tip_block.load();
        let block_data = block_data.as_deref();

        NodeStats {
            block_recv_cnt: self
                .block_recv_cnt
                .load(Ordering::Relaxed)
                .try_into()
                .unwrap(),
            last_block_content_size: block_data.map(|bd| bd.content_size).unwrap_or_default(),
            last_block_date: block_data.map(|bd| bd.date.clone()),
            last_block_fees: block_data.map(|bd| bd.block_fee_sum).unwrap_or_default(),
            last_block_hash: block_data.map(|bd| bd.hash.clone()),
            last_block_height: block_data.map(|bd| bd.chain_length.clone()),
            last_block_sum: block_data.map(|bd| bd.block_input_sum).unwrap_or_default(),
            last_block_time: block_data.map(|bd| bd.time),
            last_block_tx: block_data.map(|bd| bd.block_tx_count).unwrap_or_default(),
            last_received_block_time: Some(SystemTime::from_secs_since_epoch(
                self.slot_start_time.load(Ordering::Relaxed),
            )),
            peer_available_cnt,
            peer_connected_cnt: self.peers_connected_cnt.load(Ordering::Relaxed),
            peer_quarantined_cnt,
            peer_total_cnt,
            tx_recv_cnt: self.tx_recv_cnt.load(Ordering::Relaxed).try_into().unwrap(),
            uptime: Some(self.start_time.elapsed().as_secs()),
        }
    }
}

impl Default for SimpleCounter {
    fn default() -> Self {
        Self {
            tx_recv_cnt: Default::default(),
            block_recv_cnt: Default::default(),
            slot_start_time: Default::default(),
            peers_connected_cnt: Default::default(),
            peers_quarantined_cnt: Default::default(),
            peers_available_cnt: Default::default(),
            tip_block: Default::default(),
            start_time: Instant::now(),
        }
    }
}

impl MetricsBackend for SimpleCounter {
    fn add_tx_recv_cnt(&self, count: usize) {
        self.tx_recv_cnt.fetch_add(count, Ordering::SeqCst);
    }

    fn add_block_recv_cnt(&self, count: usize) {
        self.block_recv_cnt.fetch_add(count, Ordering::SeqCst);
    }

    fn add_peer_connected_cnt(&self, count: usize) {
        self.peers_connected_cnt.fetch_add(count, Ordering::SeqCst);
    }

    fn sub_peer_connected_cnt(&self, count: usize) {
        self.peers_connected_cnt.fetch_sub(count, Ordering::SeqCst);
    }

    fn add_peer_quarantined_cnt(&self, count: usize) {
        self.peers_quarantined_cnt
            .fetch_add(count, Ordering::SeqCst);
    }

    fn sub_peer_quarantined_cnt(&self, count: usize) {
        self.peers_quarantined_cnt
            .fetch_sub(count, Ordering::SeqCst);
    }

    fn add_peer_available_cnt(&self, count: usize) {
        self.peers_available_cnt.fetch_add(count, Ordering::SeqCst);
    }

    fn sub_peer_available_cnt(&self, count: usize) {
        self.peers_available_cnt.fetch_sub(count, Ordering::SeqCst);
    }

    fn set_slot_start_time(&self, time: SecondsSinceUnixEpoch) {
        self.slot_start_time.store(time.to_secs(), Ordering::SeqCst);
    }

    fn set_tip_block(&self, block: &Block, block_ref: &Ref) {
        let mut block_tx_count = 0;
        let mut block_input_sum = Value::zero();
        let mut block_fee_sum = Value::zero();

        block
            .contents
            .iter()
            .try_for_each::<_, Result<(), ValueError>>(|fragment| {
                fn totals<T>(t: &Transaction<T>) -> Result<(Value, Value), ValueError> {
                    Ok((t.total_input()?, t.total_output()?))
                }

                let (total_input, total_output) = match &fragment {
                    Fragment::Transaction(tx) => totals(tx),
                    Fragment::OwnerStakeDelegation(tx) => totals(tx),
                    Fragment::StakeDelegation(tx) => totals(tx),
                    Fragment::PoolRegistration(tx) => totals(tx),
                    Fragment::PoolRetirement(tx) => totals(tx),
                    Fragment::PoolUpdate(tx) => totals(tx),
                    Fragment::VotePlan(tx) => totals(tx),
                    Fragment::VoteCast(tx) => totals(tx),
                    Fragment::VoteTally(tx) => totals(tx),
                    Fragment::EncryptedVoteTally(tx) => totals(tx),
                    Fragment::Initial(_)
                    | Fragment::OldUtxoDeclaration(_)
                    | Fragment::UpdateProposal(_)
                    | Fragment::UpdateVote(_) => return Ok(()),
                }?;
                block_tx_count += 1;
                block_input_sum = (block_input_sum + total_input)?;
                let fee = (total_input - total_output).unwrap_or_else(|_| Value::zero());
                block_fee_sum = (block_fee_sum + fee)?;
                Ok(())
            })
            .expect("should be good");

        let block_data = BlockCounters {
            block_tx_count,
            block_input_sum: block_input_sum.0,
            block_fee_sum: block_fee_sum.0,
            content_size: block.header.block_content_size(),
            date: block.header.block_date().to_string(),
            hash: block.header.hash().to_string(),
            chain_length: block.header.chain_length().to_string(),
            time: SystemTime::from(block_ref.time()),
        };

        self.tip_block.store(Some(Arc::new(block_data)));
    }
}
