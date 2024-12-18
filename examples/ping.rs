use std::collections::HashMap;
use std::thread;
use std::time::Duration;
use crossbeam_channel::{Receiver, Sender};
use drone_tester::{create_test_environment, Node, PDRPolicy, TestNode};
use wg_2024::controller::{DroneCommand, DroneEvent};
use wg_2024::drone::Drone;
use wg_2024::network::{NodeId, SourceRoutingHeader};
use wg_2024::packet::{Fragment, Packet, PacketType};
use bagel_bomber::BagelBomber;

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

fn main() {
    let ping_count = 3600;

    let client = TestNode::with_node_id(40, vec![3], move |params| {
        println!("Client running");

        for i in 0..ping_count {
            let packet = Packet::new_fragment(
                SourceRoutingHeader::with_first_hop(vec![40, 3, 4, 6, 8, 50]),
                0,
                Fragment::from_string(i, ping_count, "Hello, world!".to_string()),
            );

            params.packet_send.get(&3).unwrap().send(packet).ok();

            thread::sleep(Duration::from_millis(1000));

            for packet in params.packet_recv.try_iter() {
                if let PacketType::MsgFragment(response) = packet.pack_type {
                    println!("Client {} received {}", params.id, response);
                }
            }
        }

        println!("Client {} ending simulation", params.id);

        params.end_simulation()
    });

    let server = TestNode::with_node_id(50, vec![8], |params| {
        println!("Server running");

        thread::sleep(Duration::from_millis(500));

        for in_packet in params.packet_recv.iter() {
            if let PacketType::MsgFragment(request) = in_packet.pack_type {
                println!("Server {} received {}", params.id, request);

                let packet = Packet::new_fragment(
                    SourceRoutingHeader::with_first_hop(vec![50, 8, 7, 5, 3, 40]),
                    0,
                    request.clone(),
                );

                let send = params.packet_send.get(&8).unwrap();

                send.send(packet).ok();

                if request.fragment_index == request.total_n_fragments - 1 {
                    while !send.is_empty() {
                        thread::sleep(Duration::from_millis(100));
                    }
                    break;
                }
            }
        }
        println!("Server {} ending simulation", params.id);

        thread::sleep(Duration::from_millis(1000));
        params.end_simulation()
    });

    create_test_environment(
        "topologies/examples/double-chain/topology.toml",
        vec![client, server],
        PDRPolicy::Medium,
        create_bagel_bomber,
        create_none_client_server,
        create_none_client_server,
    )
}

#[test]
fn continuous_ping_ten_seconds() {
    let ping_count = 10;

    let client = TestNode::with_node_id(40, vec![3], move |params| {
        println!("Client running");

        for i in 0..ping_count {
            let packet = Packet::new_fragment(
                SourceRoutingHeader::with_first_hop(vec![40, 3, 4, 6, 8, 50]),
                0,
                Fragment::from_string(i, ping_count, "Hello, world!".to_string()),
            );

            params.packet_send.get(&3).unwrap().send(packet).ok();

            thread::sleep(Duration::from_millis(1000));

            for packet in params.packet_recv.try_iter() {
                if let PacketType::MsgFragment(response) = packet.pack_type {
                    println!("Client {} received {}", params.id, response);
                }
            }
        }

        println!("Client {} ending simulation", params.id);

        params.end_simulation()
    });

    let server = TestNode::with_node_id(50, vec![8], |params| {
        println!("Server running");

        thread::sleep(Duration::from_millis(500));

        for in_packet in params.packet_recv.iter() {
            if let PacketType::MsgFragment(request) = in_packet.pack_type {
                println!("Server {} received {}", params.id, request);

                let packet = Packet::new_fragment(
                    SourceRoutingHeader::with_first_hop(vec![50, 8, 7, 5, 3, 40]),
                    0,
                    request.clone(),
                );

                let send = params.packet_send.get(&8).unwrap();

                send.send(packet).ok();

                if request.fragment_index == request.total_n_fragments - 1 {
                    while !send.is_empty() {
                        thread::sleep(Duration::from_millis(100));
                    }
                    break;
                }
            }
        }
        println!("Server {} ending simulation", params.id);

        thread::sleep(Duration::from_millis(1000));
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