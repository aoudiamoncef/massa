// Copyright (c) 2022 MASSA LABS <info@massa.net>

// RUST_BACKTRACE=1 cargo test test_one_handshake -- --nocapture --test-threads=1

use super::tools::protocol_test;
use massa_models::prehash::{Map, Set};
use massa_models::signed::Signable;
use massa_models::{self, Address, Amount, OperationId, Slot};
use massa_network_exports::NetworkCommand;
use massa_protocol_exports::tests::tools;
use massa_protocol_exports::{BlocksResults, ProtocolEvent, ProtocolPoolEvent};
use serial_test::serial;
use std::str::FromStr;
use std::time::Duration;

#[tokio::test]
#[serial]
async fn test_protocol_sends_valid_operations_it_receives_to_consensus() {
    let protocol_settings = &tools::PROTOCOL_SETTINGS;
    protocol_test(
        protocol_settings,
        async move |mut network_controller,
                    protocol_event_receiver,
                    protocol_command_sender,
                    protocol_manager,
                    mut protocol_pool_event_receiver| {
            // Create 1 node.
            let mut nodes = tools::create_and_connect_nodes(1, &mut network_controller).await;

            let creator_node = nodes.pop().expect("Failed to get node info.");

            // 1. Create an operation
            let operation =
                tools::create_operation_with_expire_period(&creator_node.private_key, 1);

            let expected_operation_id = operation.verify_integrity().unwrap();

            // 3. Send operation to protocol.
            network_controller
                .send_operations(creator_node.id, vec![operation])
                .await;

            // Check protocol sends operations to consensus.
            let received_operations = match tools::wait_protocol_pool_event(
                &mut protocol_pool_event_receiver,
                1000.into(),
                |evt| match evt {
                    evt @ ProtocolPoolEvent::ReceivedOperations { .. } => Some(evt),
                    _ => None,
                },
            )
            .await
            {
                Some(ProtocolPoolEvent::ReceivedOperations { operations, .. }) => operations,
                _ => panic!("Unexpected or no protocol pool event."),
            };
            assert!(received_operations.contains_key(&expected_operation_id));

            (
                network_controller,
                protocol_event_receiver,
                protocol_command_sender,
                protocol_manager,
                protocol_pool_event_receiver,
            )
        },
    )
    .await;
}

#[tokio::test]
#[serial]
async fn test_protocol_does_not_send_invalid_operations_it_receives_to_consensus() {
    let protocol_settings = &tools::PROTOCOL_SETTINGS;
    protocol_test(
        protocol_settings,
        async move |mut network_controller,
                    protocol_event_receiver,
                    protocol_command_sender,
                    protocol_manager,
                    mut protocol_pool_event_receiver| {
            // Create 1 node.
            let mut nodes = tools::create_and_connect_nodes(1, &mut network_controller).await;

            let creator_node = nodes.pop().expect("Failed to get node info.");

            // 1. Create an operation.
            let mut operation =
                tools::create_operation_with_expire_period(&creator_node.private_key, 1);

            // Change the fee, making the signature invalid.
            operation.content.fee = Amount::from_str("111").unwrap();

            // 3. Send operation to protocol.
            network_controller
                .send_operations(creator_node.id, vec![operation])
                .await;

            // Check protocol does not send operations to consensus.
            if let Some(ProtocolPoolEvent::ReceivedOperations { .. }) =
                tools::wait_protocol_pool_event(
                    &mut protocol_pool_event_receiver,
                    1000.into(),
                    |evt| match evt {
                        evt @ ProtocolPoolEvent::ReceivedOperations { .. } => Some(evt),
                        _ => None,
                    },
                )
                .await
            {
                panic!("Protocol send invalid operations.")
            };

            (
                network_controller,
                protocol_event_receiver,
                protocol_command_sender,
                protocol_manager,
                protocol_pool_event_receiver,
            )
        },
    )
    .await;
}

#[tokio::test]
#[serial]
async fn test_protocol_propagates_operations_to_active_nodes() {
    let protocol_settings = &tools::PROTOCOL_SETTINGS;
    protocol_test(
        protocol_settings,
        async move |mut network_controller,
                    protocol_event_receiver,
                    mut protocol_command_sender,
                    protocol_manager,
                    mut protocol_pool_event_receiver| {
            // Create 2 nodes.
            let nodes = tools::create_and_connect_nodes(2, &mut network_controller).await;

            // 1. Create an operation
            let operation = tools::create_operation_with_expire_period(&nodes[0].private_key, 1);

            // Send operation and wait for the protocol event,
            // just to be sure the nodes are connected before sending the propagate command.
            network_controller
                .send_operations(nodes[0].id, vec![operation.clone()])
                .await;
            let _received_operations = match tools::wait_protocol_pool_event(
                &mut protocol_pool_event_receiver,
                1000.into(),
                |evt| match evt {
                    evt @ ProtocolPoolEvent::ReceivedOperations { .. } => Some(evt),
                    _ => None,
                },
            )
            .await
            {
                Some(ProtocolPoolEvent::ReceivedOperations { operations, .. }) => operations,
                _ => panic!("Unexpected or no protocol pool event."),
            };

            let expected_operation_id = operation.verify_integrity().unwrap();

            let mut ops = Map::default();
            ops.insert(expected_operation_id, operation);
            protocol_command_sender
                .propagate_operations(ops)
                .await
                .unwrap();

            loop {
                match network_controller
                    .wait_command(1000.into(), |cmd| match cmd {
                        cmd @ NetworkCommand::SendOperations { .. } => Some(cmd),
                        _ => None,
                    })
                    .await
                {
                    Some(NetworkCommand::SendOperations { node, operations }) => {
                        let id = operations[0].verify_integrity().unwrap();
                        assert_eq!(id, expected_operation_id);
                        assert_eq!(nodes[1].id, node);
                        break;
                    }
                    _ => panic!("Unexpected or no network command."),
                };
            }
            (
                network_controller,
                protocol_event_receiver,
                protocol_command_sender,
                protocol_manager,
                protocol_pool_event_receiver,
            )
        },
    )
    .await;
}

#[tokio::test]
#[serial]
async fn test_protocol_propagates_operations_only_to_nodes_that_dont_know_about_it() {
    let protocol_settings = &tools::PROTOCOL_SETTINGS;
    protocol_test(
        protocol_settings,
        async move |mut network_controller,
                    protocol_event_receiver,
                    mut protocol_command_sender,
                    protocol_manager,
                    mut protocol_pool_event_receiver| {
            // Create 1 nodes.
            let nodes = tools::create_and_connect_nodes(1, &mut network_controller).await;

            // 1. Create an operation
            let operation = tools::create_operation_with_expire_period(&nodes[0].private_key, 1);

            // Send operation and wait for the protocol event,
            // just to be sure the nodes are connected before sending the propagate command.
            network_controller
                .send_operations(nodes[0].id, vec![operation.clone()])
                .await;
            let _received_operations = match tools::wait_protocol_pool_event(
                &mut protocol_pool_event_receiver,
                1000.into(),
                |evt| match evt {
                    evt @ ProtocolPoolEvent::ReceivedOperations { .. } => Some(evt),
                    _ => None,
                },
            )
            .await
            {
                Some(ProtocolPoolEvent::ReceivedOperations { operations, .. }) => operations,
                _ => panic!("Unexpected or no protocol pool event."),
            };
            // create and connect a node that does not know about the endorsement
            let new_nodes = tools::create_and_connect_nodes(1, &mut network_controller).await;

            // wait for things to settle
            tokio::time::sleep(Duration::from_millis(250)).await;

            let expected_operation_id = operation.verify_integrity().unwrap();

            let mut ops = Map::default();
            ops.insert(expected_operation_id, operation);

            // send endorsement to protocol
            // it should be propagated only to the node that doesn't know about it
            protocol_command_sender
                .propagate_operations(ops)
                .await
                .unwrap();

            loop {
                match network_controller
                    .wait_command(1000.into(), |cmd| match cmd {
                        cmd @ NetworkCommand::SendOperations { .. } => Some(cmd),
                        _ => None,
                    })
                    .await
                {
                    Some(NetworkCommand::SendOperations { node, operations }) => {
                        let id = operations[0].verify_integrity().unwrap();
                        assert_eq!(id, expected_operation_id);
                        assert_eq!(new_nodes[0].id, node);
                        break;
                    }
                    _ => panic!("Unexpected or no network command."),
                };
            }
            (
                network_controller,
                protocol_event_receiver,
                protocol_command_sender,
                protocol_manager,
                protocol_pool_event_receiver,
            )
        },
    )
    .await;
}

#[tokio::test]
#[serial]
async fn test_protocol_propagates_operations_only_to_nodes_that_dont_know_about_it_block_integration(
) {
    let protocol_settings = &tools::PROTOCOL_SETTINGS;
    protocol_test(
        protocol_settings,
        async move |mut network_controller,
                    mut protocol_event_receiver,
                    mut protocol_command_sender,
                    protocol_manager,
                    protocol_pool_event_receiver| {
            // Create 1 node.
            let nodes = tools::create_and_connect_nodes(1, &mut network_controller).await;

            let address = Address::from_public_key(&nodes[0].id.0);
            let serialization_context = massa_models::get_serialization_context();
            let thread = address.get_thread(serialization_context.thread_count);

            let operation = tools::create_operation_with_expire_period(&nodes[0].private_key, 1);
            let operation_id = operation.content.compute_id().unwrap();

            let block = tools::create_block_with_operations(
                &nodes[0].private_key,
                &nodes[0].id.0,
                Slot::new(1, thread),
                vec![operation.clone()],
            );
            let block_id = block.header.content.compute_id().unwrap();

            network_controller
                .send_ask_for_block(nodes[0].id, vec![block_id])
                .await;

            // Wait for the event to be sure that the node is connected,
            // and noted as interested in the block.
            let _ = tools::wait_protocol_event(&mut protocol_event_receiver, 1000.into(), |evt| {
                match evt {
                    evt @ ProtocolEvent::GetBlocks { .. } => Some(evt),
                    _ => None,
                }
            })
            .await;

            // Integrate the block,
            // this should note the node as knowning about the endorsement.
            protocol_command_sender
                .integrated_block(
                    block_id,
                    vec![operation_id].into_iter().collect(),
                    Default::default(),
                )
                .await
                .unwrap();

            match network_controller
                .wait_command(1000.into(), |cmd| match cmd {
                    cmd @ NetworkCommand::SendBlock { .. } => Some(cmd),
                    _ => None,
                })
                .await
            {
                Some(NetworkCommand::SendBlock { node, block_id }) => {
                    assert_eq!(node, nodes[0].id);
                    assert_eq!(
                        block
                            .header
                            .compute_id()
                            .expect("Fail to get block id"),
                        block_id
                    );
                }
                Some(_) => panic!("Unexpected network command.."),
                None => panic!("Block not sent."),
            };

            // Send the endorsement to protocol
            // it should not propagate to the node that already knows about it
            // because of the previously integrated block.
            let mut ops = Map::default();
            ops.insert(operation_id, operation);
            protocol_command_sender
                .propagate_operations(ops)
                .await
                .unwrap();

            match network_controller
                .wait_command(1000.into(), |cmd| match cmd {
                    cmd @ NetworkCommand::SendOperations { .. } => Some(cmd),
                    _ => None,
                })
                .await
            {
                Some(NetworkCommand::SendOperations { node, operations }) => {
                    let id = operations[0].content.compute_id().unwrap();
                    assert_eq!(id, operation_id);
                    assert_eq!(nodes[0].id, node);
                    panic!("Unexpected propagated of operation.");
                }
                None => {}
                Some(cmd) => panic!("Unexpected network command.{:?}", cmd),
            };

            (
                network_controller,
                protocol_event_receiver,
                protocol_command_sender,
                protocol_manager,
                protocol_pool_event_receiver,
            )
        },
    )
    .await;
}

#[tokio::test]
#[serial]
async fn test_protocol_propagates_operations_only_to_nodes_that_dont_know_about_it_get_block_results(
) {
    let protocol_settings = &tools::PROTOCOL_SETTINGS;
    protocol_test(
        protocol_settings,
        async move |mut network_controller,
                    mut protocol_event_receiver,
                    mut protocol_command_sender,
                    protocol_manager,
                    protocol_pool_event_receiver| {
            // Create 1 node.
            let nodes = tools::create_and_connect_nodes(1, &mut network_controller).await;

            let address = Address::from_public_key(&nodes[0].id.0);
            let serialization_context = massa_models::get_serialization_context();
            let thread = address.get_thread(serialization_context.thread_count);

            let operation = tools::create_operation_with_expire_period(&nodes[0].private_key, 1);
            let operation_id = operation.content.compute_id().unwrap();

            let block = tools::create_block_with_operations(
                &nodes[0].private_key,
                &nodes[0].id.0,
                Slot::new(1, thread),
                vec![operation.clone()],
            );
            let expected_block_id = block.header.content.compute_id().unwrap();

            network_controller
                .send_ask_for_block(nodes[0].id, vec![expected_block_id])
                .await;

            // Wait for the event to be sure that the node is connected,
            // and noted as interested in the block.
            let _ = tools::wait_protocol_event(&mut protocol_event_receiver, 1000.into(), |evt| {
                match evt {
                    evt @ ProtocolEvent::GetBlocks { .. } => Some(evt),
                    _ => None,
                }
            })
            .await;

            // Send the block as search results.
            let mut results: BlocksResults = Map::default();
            let mut ops = Set::<OperationId>::default();
            ops.insert(operation_id);
            results.insert(expected_block_id, Some((Some(ops), None)));

            protocol_command_sender
                .send_get_blocks_results(results)
                .await
                .unwrap();

            match network_controller
                .wait_command(1000.into(), |cmd| match cmd {
                    cmd @ NetworkCommand::SendBlock { .. } => Some(cmd),
                    _ => None,
                })
                .await
            {
                Some(NetworkCommand::SendBlock { node, block_id }) => {
                    assert_eq!(node, nodes[0].id);
                    assert_eq!(block_id, expected_block_id);
                }
                Some(_) => panic!("Unexpected network command.."),
                None => panic!("Block not sent."),
            };

            // Send the endorsement to protocol
            // it should not propagate to the node that already knows about it
            // because of the previously integrated block.
            let mut ops = Map::default();
            ops.insert(operation_id, operation);
            protocol_command_sender
                .propagate_operations(ops)
                .await
                .unwrap();

            match network_controller
                .wait_command(1000.into(), |cmd| match cmd {
                    cmd @ NetworkCommand::SendOperations { .. } => Some(cmd),
                    _ => None,
                })
                .await
            {
                Some(NetworkCommand::SendOperations { node, operations }) => {
                    let id = operations[0].content.compute_id().unwrap();
                    assert_eq!(id, operation_id);
                    assert_eq!(nodes[0].id, node);
                    panic!("Unexpected propagated of operation.");
                }
                None => {}
                Some(cmd) => panic!("Unexpected network command.{:?}", cmd),
            };

            (
                network_controller,
                protocol_event_receiver,
                protocol_command_sender,
                protocol_manager,
                protocol_pool_event_receiver,
            )
        },
    )
    .await;
}

#[tokio::test]
#[serial]
async fn test_protocol_propagates_operations_only_to_nodes_that_dont_know_about_it_indirect_knowledge_via_header(
) {
    let protocol_settings = &tools::PROTOCOL_SETTINGS;
    protocol_test(
        protocol_settings,
        async move |mut network_controller,
                    mut protocol_event_receiver,
                    mut protocol_command_sender,
                    protocol_manager,
                    protocol_pool_event_receiver| {
            // Create 2 nodes.
            let nodes = tools::create_and_connect_nodes(2, &mut network_controller).await;

            let address = Address::from_public_key(&nodes[0].id.0);
            let serialization_context = massa_models::get_serialization_context();
            let thread = address.get_thread(serialization_context.thread_count);

            let operation = tools::create_operation_with_expire_period(&nodes[0].private_key, 1);
            let operation_id = operation.content.compute_id().unwrap();

            let block = tools::create_block_with_operations(
                &nodes[0].private_key,
                &nodes[0].id.0,
                Slot::new(1, thread),
                vec![operation.clone()],
            );

            let block_id = block
                .header
                .compute_id()
                .expect("Fail to compute block id");

            // Node 2 sends block, resulting in operations and endorsements noted in block info.
            network_controller.send_block(nodes[1].id, block_id).await;

            // Node 1 sends header, resulting in protocol using the block info to determine
            // the node knows about the operations contained in the block.
            network_controller
                .send_header(nodes[0].id, block.header.clone())
                .await;

            // Wait for the event to be sure that the node is connected,
            // and noted as knowing the block and its operations.
            let _ = tools::wait_protocol_event(&mut protocol_event_receiver, 1000.into(), |evt| {
                match evt {
                    evt @ ProtocolEvent::ReceivedBlockHeader { .. } => Some(evt),
                    _ => None,
                }
            })
            .await;

            // Send the operation to protocol
            // it should not propagate to the node that already knows about it
            // because of the previously received header.
            let mut ops = Map::default();
            ops.insert(operation_id, operation);
            protocol_command_sender
                .propagate_operations(ops)
                .await
                .unwrap();

            match network_controller
                .wait_command(1000.into(), |cmd| match cmd {
                    cmd @ NetworkCommand::SendOperations { .. } => Some(cmd),
                    _ => None,
                })
                .await
            {
                Some(NetworkCommand::SendOperations { node, operations }) => {
                    let id = operations[0].content.compute_id().unwrap();
                    assert_eq!(id, operation_id);
                    assert_eq!(nodes[0].id, node);
                    panic!("Unexpected propagated of operation.");
                }
                None => {}
                Some(cmd) => panic!("Unexpected network command.{:?}", cmd),
            };

            (
                network_controller,
                protocol_event_receiver,
                protocol_command_sender,
                protocol_manager,
                protocol_pool_event_receiver,
            )
        },
    )
    .await;
}

#[tokio::test]
#[serial]
async fn test_protocol_propagates_operations_only_to_nodes_that_dont_know_about_it_indirect_knowledge_via_header_wrong_root_hash(
) {
    let protocol_settings = &tools::PROTOCOL_SETTINGS;
    protocol_test(
        protocol_settings,
        async move |mut network_controller,
                    mut protocol_event_receiver,
                    mut protocol_command_sender,
                    protocol_manager,
                    protocol_pool_event_receiver| {
            // Create 3 nodes.
            let nodes = tools::create_and_connect_nodes(3, &mut network_controller).await;

            let address = Address::from_public_key(&nodes[0].id.0);
            let serialization_context = massa_models::get_serialization_context();
            let thread = address.get_thread(serialization_context.thread_count);

            let operation = tools::create_operation_with_expire_period(&nodes[0].private_key, 1);

            let operation_2 = tools::create_operation_with_expire_period(&nodes[0].private_key, 1);
            let operation_id_2 = operation_2.content.compute_id().unwrap();

            let mut block = tools::create_block_with_operations(
                &nodes[0].private_key,
                &nodes[0].id.0,
                Slot::new(1, thread),
                vec![operation.clone()],
            );

            // Change the root operation hash
            block.operations = vec![operation_2.clone()];

            let block_id = block
                .header
                .compute_id()
                .expect("Fail to compute block id");
            // Node 2 sends block, not resulting in operations and endorsements noted in block info,
            // because of the invalid root hash.
            network_controller.send_block(nodes[1].id, block_id).await;

            // Node 3 sends block, resulting in operations and endorsements noted in block info.
            network_controller.send_block(nodes[2].id, block_id).await;

            // Node 1 sends header, but the block is empty.
            network_controller
                .send_header(nodes[0].id, block.header.clone())
                .await;

            // Wait for the event to be sure that the node is connected.
            let _ = tools::wait_protocol_event(&mut protocol_event_receiver, 1000.into(), |evt| {
                match evt {
                    evt @ ProtocolEvent::ReceivedBlockHeader { .. } => Some(evt),
                    _ => None,
                }
            })
            .await;

            // Send the operation to protocol
            // it should propagate to the node because it isn't in the block.
            let mut ops = Map::default();
            ops.insert(operation_id_2, operation_2);
            protocol_command_sender
                .propagate_operations(ops)
                .await
                .unwrap();

            match network_controller
                .wait_command(1000.into(), |cmd| match cmd {
                    cmd @ NetworkCommand::SendOperations { .. } => Some(cmd),
                    _ => None,
                })
                .await
            {
                Some(NetworkCommand::SendOperations { node, operations }) => {
                    let id = operations[0].content.compute_id().unwrap();
                    assert_eq!(id, operation_id_2);
                    assert_eq!(nodes[0].id, node);
                }
                None => panic!("Operation not propagated."),
                Some(cmd) => panic!("Unexpected network command.{:?}", cmd),
            };

            (
                network_controller,
                protocol_event_receiver,
                protocol_command_sender,
                protocol_manager,
                protocol_pool_event_receiver,
            )
        },
    )
    .await;
}

#[tokio::test]
#[serial]
async fn test_protocol_does_not_propagates_operations_when_receiving_those_inside_a_block() {
    let protocol_settings = &tools::PROTOCOL_SETTINGS;
    protocol_test(
        protocol_settings,
        async move |mut network_controller,
                    protocol_event_receiver,
                    protocol_command_sender,
                    protocol_manager,
                    mut protocol_pool_event_receiver| {
            // Create 2 nodes.
            let mut nodes = tools::create_and_connect_nodes(2, &mut network_controller).await;

            let creator_node = nodes.pop().expect("Failed to get node info.");

            // 1. Create an operation
            let operation =
                tools::create_operation_with_expire_period(&creator_node.private_key, 1);

            let address = Address::from_public_key(&creator_node.id.0);
            let serialization_context = massa_models::get_serialization_context();
            let thread = address.get_thread(serialization_context.thread_count);

            // 2. Create a block coming from node creator_node, and including the operation.
            let block = tools::create_block_with_operations(
                &creator_node.private_key,
                &creator_node.id.0,
                Slot::new(1, thread),
                vec![operation.clone()],
            );

            // 4. Send block to protocol.
            network_controller
                .send_block(
                    creator_node.id,
                    block
                        .header
                        .compute_id()
                        .expect("Fail to compute block id"),
                )
                .await;

            // 5. Check that the operation included in the block is not propagated.
            match tools::wait_protocol_pool_event(
                &mut protocol_pool_event_receiver,
                1000.into(),
                |evt| match evt {
                    evt @ ProtocolPoolEvent::ReceivedOperations { .. } => Some(evt),
                    _ => None,
                },
            )
            .await
            {
                None => panic!("Protocol did not send operations to pool."),
                Some(ProtocolPoolEvent::ReceivedOperations {
                    propagate,
                    operations,
                }) => {
                    let expected_id = operation.verify_integrity().unwrap();
                    assert!(!propagate);
                    assert_eq!(
                        operations
                            .get(&expected_id)
                            .unwrap()
                            .verify_integrity()
                            .unwrap(),
                        expected_id
                    );
                    assert_eq!(operations.len(), 1);
                }
                Some(_) => panic!("Unexpected protocol pool event."),
            }
            (
                network_controller,
                protocol_event_receiver,
                protocol_command_sender,
                protocol_manager,
                protocol_pool_event_receiver,
            )
        },
    )
    .await;
}
