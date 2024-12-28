use drone_tester::{create_test_environment, DummyNode, PDRPolicy, Runnable, TestNodeInstructions};
use crossbeam_channel::{unbounded, Receiver, Sender};
use std::collections::HashMap;
use std::thread;
use std::time::Duration;
use wg_2024::controller::DroneCommand;
use wg_2024::drone::Drone;
use wg_2024::network::{NodeId, SourceRoutingHeader};
use wg_2024::packet::NodeType::Client;
use wg_2024::packet::{FloodRequest, Fragment, Packet, PacketType, FRAGMENT_DSIZE};

use crate::BagelBomber;

pub fn create_bagel_bomber(
    id: NodeId,
    controller_recv: Receiver<DroneCommand>,
    packet_recv: Receiver<Packet>,
    packet_send: HashMap<NodeId, Sender<Packet>>,
    pdr: f32,
) -> Box<dyn Runnable> {
    Box::new(BagelBomber::new(
        id,
        unbounded().0,
        controller_recv,
        packet_recv,
        packet_send,
        pdr,
    ))
}

#[test]
fn flooding() {
    let client = TestNodeInstructions::with_random_id(vec![1], |id: NodeId, packet_recv: Receiver<Packet>, packet_send: HashMap<NodeId, Sender<Packet>>| {
        println!("Client running");
        packet_send
            .get(&1)
            .unwrap()
            .send(Packet {
                session_id: 0,
                routing_header: SourceRoutingHeader {
                    hops: Vec::new(),
                    hop_index: 0,
                },
                pack_type: PacketType::FloodRequest(FloodRequest {
                    flood_id: 0,
                    initiator_id: id,
                    path_trace: vec![(id, Client)],
                }),
            })
            .ok();

        thread::sleep(Duration::from_millis(100));

        let mut response_received = false;

        for packet in packet_recv.try_iter() {
            if let PacketType::FloodResponse(response) = packet.pack_type {
                println!("Client {} received {:?}", id, response);
                assert_eq!(response.flood_id, 0);
                response_received = true;
            }
        }

        assert!(response_received);
    });
    create_test_environment(
        "topologies/examples/double-chain/topology.toml",
        vec![client],
        PDRPolicy::Zero,
        create_bagel_bomber,
        DummyNode::create_client_server,
        DummyNode::create_client_server,
    )
}

#[test]
fn client_server_ping() {
    let client = TestNodeInstructions::with_node_id(40, vec![3], |id: NodeId, packet_recv: Receiver<Packet>, packet_send: HashMap<u8, Sender<Packet>>| {
        thread::sleep(Duration::from_millis(1000));
        println!("Client running");
        packet_send
            .get(&3)
            .unwrap()
            .send(Packet {
                session_id: 0,
                routing_header: SourceRoutingHeader {
                    hops: vec![40, 3, 4, 6, 8, 50],
                    hop_index: 1,
                },
                pack_type: PacketType::MsgFragment(Fragment {
                    fragment_index: 0,
                    total_n_fragments: 1,
                    length: FRAGMENT_DSIZE as u8,
                    data: [0; FRAGMENT_DSIZE],
                }),
            })
            .ok();

        thread::sleep(Duration::from_millis(5000));

        let mut response_received = false;

        for packet in packet_recv.try_iter() {
            if let PacketType::MsgFragment(response) = packet.pack_type {
                println!("Client {} received {:?}", id, response);
                assert_eq!(response.fragment_index, 0);
                assert_eq!(response.total_n_fragments, 1);
                assert_eq!(response.data, [1; FRAGMENT_DSIZE]);
                response_received = true;
            }
        }

        assert!(response_received);
    });

    let server = TestNodeInstructions::with_node_id(50, vec![8], |id: NodeId, packet_recv: Receiver<Packet>, packet_send: HashMap<u8, Sender<Packet>>| {
        thread::sleep(Duration::from_millis(1000));

        println!("Server running");

        let mut request_received = false;

        for packet in packet_recv.try_iter() {
            if let PacketType::MsgFragment(response) = packet.pack_type {
                println!("Server {} received {:?}", id, response);

                assert_eq!(response.fragment_index, 0);
                assert_eq!(response.total_n_fragments, 1);
                assert_eq!(response.data, [0; FRAGMENT_DSIZE]);

                request_received = true;

                packet_send
                    .get(&8)
                    .unwrap()
                    .send(Packet {
                        session_id: 0,
                        routing_header: SourceRoutingHeader {
                            hops: vec![50, 8, 7, 5, 3, 40],
                            hop_index: 1,
                        },
                        pack_type: PacketType::MsgFragment(Fragment {
                            fragment_index: 0,
                            total_n_fragments: 1,
                            length: FRAGMENT_DSIZE as u8,
                            data: [1; FRAGMENT_DSIZE],
                        }),
                    })
                    .ok();
            }
        }

        assert!(request_received);
    });

    create_test_environment(
        "topologies/examples/double-chain/topology.toml",
        vec![client, server],
        PDRPolicy::Zero,
        create_bagel_bomber,
        DummyNode::create_client_server,
        DummyNode::create_client_server,
    )
}

#[test]
fn continuous_ping() {
    let ping_count = 600;

    let client = TestNodeInstructions::with_node_id(40, vec![3], move |id: NodeId, packet_recv: Receiver<Packet>, packet_send: HashMap<u8, Sender<Packet>>| {
        println!("Client running");

        for i in 0..ping_count {
            let packet = Packet::new_fragment(
                SourceRoutingHeader::with_first_hop(vec![40, 3, 4, 6, 8, 50]),
                0,
                Fragment::from_string(i, ping_count, "Hello, world!".to_string()),
            );

            packet_send.get(&3).unwrap().send(packet).ok();

            thread::sleep(Duration::from_millis(1000));

            for packet in packet_recv.try_iter() {
                if let PacketType::MsgFragment(response) = packet.pack_type {
                    println!("Client {} received {}", id, response);
                }
            }
        }

        println!("Client {} ending simulation", id);
    });

    let server = TestNodeInstructions::with_node_id(50, vec![8], |id: NodeId, packet_recv: Receiver<Packet>, packet_send: HashMap<u8, Sender<Packet>>| {
        println!("Server running");

        thread::sleep(Duration::from_millis(500));

        for in_packet in packet_recv.iter() {
            if let PacketType::MsgFragment(request) = in_packet.pack_type {
                println!("Server {} received {}", id, request);

                let packet = Packet::new_fragment(
                    SourceRoutingHeader::with_first_hop(vec![50, 8, 7, 5, 3, 40]),
                    0,
                    request.clone(),
                );

                let send = packet_send.get(&8).unwrap();

                send.send(packet).ok();

                if request.fragment_index == request.total_n_fragments - 1 {
                    while !send.is_empty() {
                        thread::sleep(Duration::from_millis(100));
                    }
                }
            }
        }

        thread::sleep(Duration::from_millis(1000));

        println!("Server {} ending simulation", id);
    });

    create_test_environment(
        "topologies/examples/double-chain/topology.toml",
        vec![client, server],
        PDRPolicy::Severe,
        create_bagel_bomber,
        DummyNode::create_client_server,
        DummyNode::create_client_server,
    )
}
