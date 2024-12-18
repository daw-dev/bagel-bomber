use crate::bagel_bomber::BagelBomber;
use crossbeam_channel::{Receiver, Sender};
use drone_tester::{create_test_environment, Node, PDRPolicy, TestNode};
use std::collections::HashMap;
use std::thread;
use std::time::Duration;
use wg_2024::controller::{DroneCommand, DroneEvent};
use wg_2024::drone::Drone;
use wg_2024::network::{NodeId, SourceRoutingHeader};
use wg_2024::packet::NodeType::Client;
use wg_2024::packet::{FloodRequest, Fragment, Packet, PacketType, FRAGMENT_DSIZE};

pub fn create_bagel_bomber(
    id: NodeId,
    controller_send: Sender<DroneEvent>,
    controller_recv: Receiver<DroneCommand>,
    packet_recv: Receiver<Packet>,
    packet_send: HashMap<NodeId, Sender<Packet>>,
    pdr: f32,
) -> Box<dyn Node> {
    Box::new(BagelBomber::new(
        id,
        controller_send,
        controller_recv,
        packet_recv,
        packet_send,
        pdr,
    ))
}

pub fn create_none_client_server(
    _id: NodeId,
    _packet_recv: Receiver<Packet>,
    _packet_send: HashMap<NodeId, Sender<Packet>>,
) -> Option<Box<dyn Node>> {
    None
}

#[test]
fn flooding() {
    let client = TestNode::with_random_id(vec![1], |params| {
        println!("Client running");
        params
            .packet_send
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
                    initiator_id: params.id,
                    path_trace: vec![(params.id, Client)],
                }),
            })
            .ok();

        thread::sleep(Duration::from_millis(100));

        let mut response_received = false;

        for packet in params.packet_recv.try_iter() {
            if let PacketType::FloodResponse(response) = packet.pack_type {
                println!("Client {} received {:?}", params.id, response);
                assert_eq!(response.flood_id, 0);
                response_received = true;
            }
        }

        assert!(response_received);

        params.end_simulation()
    });
    create_test_environment(
        "topologies/examples/double-chain/topology.toml",
        vec![client],
        PDRPolicy::Zero,
        create_bagel_bomber,
        create_none_client_server,
        create_none_client_server,
    )
}

#[test]
fn client_server_ping() {
    let client = TestNode::with_node_id(40, vec![3], |params| {
        thread::sleep(Duration::from_millis(1000));
        println!("Client running");
        params
            .packet_send
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

        for packet in params.packet_recv.try_iter() {
            if let PacketType::MsgFragment(response) = packet.pack_type {
                println!("Client {} received {:?}", params.id, response);
                assert_eq!(response.fragment_index, 0);
                assert_eq!(response.total_n_fragments, 1);
                assert_eq!(response.data, [1; FRAGMENT_DSIZE]);
            }
        }

        params.end_simulation()
    });

    let server = TestNode::with_node_id(50, vec![8], |params| {
        thread::sleep(Duration::from_millis(1000));

        println!("Server running");

        for packet in params.packet_recv.try_iter() {
            if let PacketType::MsgFragment(response) = packet.pack_type {
                println!("Server {} received {:?}", params.id, response);

                assert_eq!(response.fragment_index, 0);
                assert_eq!(response.total_n_fragments, 1);
                assert_eq!(response.data, [0; FRAGMENT_DSIZE]);

                params
                    .packet_send
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

        params.end_simulation()
    });

    create_test_environment(
        "topologies/examples/double-chain/topology.toml",
        vec![client, server],
        PDRPolicy::Zero,
        create_bagel_bomber,
        create_none_client_server,
        create_none_client_server,
    )
}
