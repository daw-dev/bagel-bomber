use std::collections::HashMap;
use std::thread;
use std::time::Duration;
use crossbeam_channel::{unbounded, Receiver, Sender};
use drone_tester::{create_test_environment, DummyNode, PDRPolicy, Runnable, TestNodeInstructions};
use wg_2024::controller::DroneCommand;
use wg_2024::drone::Drone;
use wg_2024::network::{NodeId, SourceRoutingHeader};
use wg_2024::packet::{Fragment, Packet, PacketType};
use bagel_bomber::BagelBomber;

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

fn main() {
    let ping_count = 3600;

    let client = TestNodeInstructions::with_node_id(40, vec![3], move |id: NodeId, packet_recv: Receiver<Packet>, packet_send: HashMap<NodeId, Sender<Packet>>| {
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

    let server = TestNodeInstructions::with_node_id(50, vec![8], |id: NodeId, packet_recv: Receiver<Packet>, packet_send: HashMap<NodeId, Sender<Packet>>| {
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
                    break;
                }
            }
        }
        println!("Server {} ending simulation", id);

        thread::sleep(Duration::from_millis(1000));
    });

    create_test_environment(
        "topologies/examples/double-chain/topology.toml",
        vec![client, server],
        PDRPolicy::Medium,
        create_bagel_bomber,
        DummyNode::create_client_server,
        DummyNode::create_client_server,
    )
}