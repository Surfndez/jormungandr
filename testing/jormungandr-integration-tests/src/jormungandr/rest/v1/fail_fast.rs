use crate::common::jormungandr::JormungandrProcess;
use crate::common::{jormungandr::ConfigurationBuilder, startup};
use chain_impl_mockchain::{block::BlockDate, fragment::Fragment};
use jormungandr_testing_utils::testing::fragments::FaultyTransactionBuilder;
use jormungandr_testing_utils::testing::node::assert_bad_request;
use jormungandr_testing_utils::testing::FragmentSenderSetup;
use jormungandr_testing_utils::testing::FragmentVerifier;
use rstest::*;
use std::time::Duration;

#[fixture]
fn world() -> (
    JormungandrProcess,
    Fragment,
    Fragment,
    Fragment,
    Fragment,
    Fragment,
) {
    let mut alice = startup::create_new_account_address();
    let mut bob = startup::create_new_account_address();
    let mut clarice = startup::create_new_account_address();
    let mut david = startup::create_new_account_address();

    let (jormungandr, _stake_pools) = startup::start_stake_pool(
        &[alice.clone()],
        &[bob.clone(), clarice.clone()],
        &mut ConfigurationBuilder::new(),
    )
    .unwrap();

    let alice_fragment = alice
        .transaction_to(
            &jormungandr.genesis_block_hash(),
            &jormungandr.fees(),
            BlockDate::first().next_epoch(),
            bob.address(),
            100.into(),
        )
        .unwrap();

    let bob_fragment = bob
        .transaction_to(
            &jormungandr.genesis_block_hash(),
            &jormungandr.fees(),
            BlockDate::first().next_epoch(),
            alice.address(),
            100.into(),
        )
        .unwrap();
    let clarice_fragment = clarice
        .transaction_to(
            &jormungandr.genesis_block_hash(),
            &jormungandr.fees(),
            BlockDate::first().next_epoch(),
            alice.address(),
            100.into(),
        )
        .unwrap();

    let late_invalid_fragment = david
        .transaction_to(
            &jormungandr.genesis_block_hash(),
            &jormungandr.fees(),
            BlockDate::first().next_epoch(),
            alice.address(),
            100.into(),
        )
        .unwrap();

    let faulty_tx_builder = FaultyTransactionBuilder::new(
        jormungandr.genesis_block_hash(),
        jormungandr.fees(),
        BlockDate::first().next_epoch(),
    );
    let early_invalid_fragment = faulty_tx_builder.unbalanced(&alice, &bob);

    (
        jormungandr,
        alice_fragment,
        bob_fragment,
        clarice_fragment,
        early_invalid_fragment,
        late_invalid_fragment,
    )
}

#[rstest]
pub fn fail_fast_on_all_valid(
    world: (
        JormungandrProcess,
        Fragment,
        Fragment,
        Fragment,
        Fragment,
        Fragment,
    ),
) {
    let (jormungandr, valid_fragment_1, valid_fragment_2, valid_fragment_3, _, _) = world;
    let transaction_sender = jormungandr.fragment_sender(FragmentSenderSetup::resend_3_times());
    let tx_ids = transaction_sender
        .send_batch_fragments(
            vec![valid_fragment_1, valid_fragment_2, valid_fragment_3],
            true,
            &jormungandr,
        )
        .unwrap();

    FragmentVerifier::wait_for_all_fragments(Duration::from_secs(5), &jormungandr).unwrap();

    jormungandr
        .correct_state_verifier()
        .fragment_logs()
        .assert_all_valid(&tx_ids);
}

#[rstest]
pub fn fail_fast_off_all_valid(
    world: (
        JormungandrProcess,
        Fragment,
        Fragment,
        Fragment,
        Fragment,
        Fragment,
    ),
) {
    let (jormungandr, valid_fragment_1, valid_fragment_2, valid_fragment_3, _, _) = world;
    let transaction_sender = jormungandr.fragment_sender(FragmentSenderSetup::resend_3_times());
    let tx_ids = transaction_sender
        .send_batch_fragments(
            vec![valid_fragment_1, valid_fragment_2, valid_fragment_3],
            false,
            &jormungandr,
        )
        .unwrap();

    FragmentVerifier::wait_for_all_fragments(Duration::from_secs(5), &jormungandr).unwrap();

    jormungandr
        .correct_state_verifier()
        .fragment_logs()
        .assert_all_valid(&tx_ids);
}

#[rstest]
pub fn fail_fast_on_first_invalid(
    world: (
        JormungandrProcess,
        Fragment,
        Fragment,
        Fragment,
        Fragment,
        Fragment,
    ),
) {
    let (jormungandr, valid_fragment_1, valid_fragment_2, _, early_invalid_fragment, _) = world;
    assert_bad_request(jormungandr.rest().send_fragment_batch(
        vec![early_invalid_fragment, valid_fragment_1, valid_fragment_2],
        true,
    ));

    FragmentVerifier::wait_for_all_fragments(Duration::from_secs(5), &jormungandr).unwrap();

    jormungandr
        .correct_state_verifier()
        .fragment_logs()
        .assert_no_fragments();
}

#[rstest]
pub fn fail_fast_on_first_late_invalid(
    world: (
        JormungandrProcess,
        Fragment,
        Fragment,
        Fragment,
        Fragment,
        Fragment,
    ),
) {
    let (jormungandr, valid_fragment_1, valid_fragment_2, _, _, late_invalid_fragment) = world;
    let transaction_sender = jormungandr.fragment_sender(FragmentSenderSetup::resend_3_times());
    let tx_ids = transaction_sender
        .send_batch_fragments(
            vec![late_invalid_fragment, valid_fragment_1, valid_fragment_2],
            true,
            &jormungandr,
        )
        .unwrap();

    FragmentVerifier::wait_for_all_fragments(Duration::from_secs(5), &jormungandr).unwrap();

    jormungandr
        .correct_state_verifier()
        .fragment_logs()
        .assert_invalid(&tx_ids[0])
        .assert_valid(&tx_ids[1])
        .assert_valid(&tx_ids[2]);
}

#[rstest]
pub fn fail_fast_off_first_invalid(
    world: (
        JormungandrProcess,
        Fragment,
        Fragment,
        Fragment,
        Fragment,
        Fragment,
    ),
) {
    let (jormungandr, valid_fragment_1, valid_fragment_2, _, early_invalid_fragment, _) = world;
    let tx_ids = assert_bad_request(jormungandr.rest().send_fragment_batch(
        vec![valid_fragment_1, valid_fragment_2, early_invalid_fragment],
        true,
    ));

    FragmentVerifier::wait_for_all_fragments(Duration::from_secs(5), &jormungandr).unwrap();

    jormungandr
        .correct_state_verifier()
        .fragment_logs()
        .assert_not_exist(&tx_ids[2])
        .assert_valid(&tx_ids[0])
        .assert_valid(&tx_ids[1]);
}

#[rstest]
pub fn fail_fast_off_invalid_in_middle(
    world: (
        JormungandrProcess,
        Fragment,
        Fragment,
        Fragment,
        Fragment,
        Fragment,
    ),
) {
    let (jormungandr, valid_fragment_1, valid_fragment_2, _, early_invalid_fragment, _) = world;
    let tx_ids = assert_bad_request(jormungandr.rest().send_fragment_batch(
        vec![valid_fragment_1, early_invalid_fragment, valid_fragment_2],
        false,
    ));

    FragmentVerifier::wait_for_all_fragments(Duration::from_secs(5), &jormungandr).unwrap();

    jormungandr
        .correct_state_verifier()
        .fragment_logs()
        .assert_valid(&tx_ids[0])
        .assert_valid(&tx_ids[2])
        .assert_not_exist(&tx_ids[1]);
}

#[rstest]
pub fn fail_fast_on_invalid_in_middle(
    world: (
        JormungandrProcess,
        Fragment,
        Fragment,
        Fragment,
        Fragment,
        Fragment,
    ),
) {
    let (jormungandr, valid_fragment_1, valid_fragment_2, _, early_invalid_fragment, _) = world;
    let tx_ids = assert_bad_request(jormungandr.rest().send_fragment_batch(
        vec![valid_fragment_1, early_invalid_fragment, valid_fragment_2],
        true,
    ));

    FragmentVerifier::wait_for_all_fragments(Duration::from_secs(5), &jormungandr).unwrap();

    jormungandr
        .correct_state_verifier()
        .fragment_logs()
        .assert_valid(&tx_ids[0])
        .assert_not_exist(&tx_ids[1])
        .assert_not_exist(&tx_ids[2]);
}
#[rstest]
pub fn fail_fast_on_last_invalid(
    world: (
        JormungandrProcess,
        Fragment,
        Fragment,
        Fragment,
        Fragment,
        Fragment,
    ),
) {
    let (jormungandr, valid_fragment_1, valid_fragment_2, _, early_invalid_fragment, _) = world;
    let tx_ids = assert_bad_request(jormungandr.rest().send_fragment_batch(
        vec![valid_fragment_1, valid_fragment_2, early_invalid_fragment],
        true,
    ));

    FragmentVerifier::wait_for_all_fragments(Duration::from_secs(5), &jormungandr).unwrap();

    jormungandr
        .correct_state_verifier()
        .fragment_logs()
        .assert_valid(&tx_ids[0])
        .assert_valid(&tx_ids[1])
        .assert_not_exist(&tx_ids[2]);
}

#[rstest]
pub fn fail_fast_off_last_invalid(
    world: (
        JormungandrProcess,
        Fragment,
        Fragment,
        Fragment,
        Fragment,
        Fragment,
    ),
) {
    let (jormungandr, valid_fragment_1, valid_fragment_2, _, early_invalid_fragment, _) = world;
    let tx_ids = assert_bad_request(jormungandr.rest().send_fragment_batch(
        vec![valid_fragment_1, valid_fragment_2, early_invalid_fragment],
        false,
    ));

    FragmentVerifier::wait_for_all_fragments(Duration::from_secs(5), &jormungandr).unwrap();

    jormungandr
        .correct_state_verifier()
        .fragment_logs()
        .assert_valid(&tx_ids[0])
        .assert_valid(&tx_ids[1])
        .assert_not_exist(&tx_ids[2]);
}
