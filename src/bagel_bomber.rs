use super::drone_gui;
use crate::coin_toss;
use crossbeam_channel::{select_biased, unbounded, Receiver, Sender};
use std::collections::{HashMap, HashSet};
use std::mem;
use wg_2024::controller::*;
use wg_2024::drone::*;
use wg_2024::network::{NodeId, SourceRoutingHeader};
use wg_2024::packet::{FloodRequest, Nack, NackType, NodeType, Packet, PacketType};
use crate::drone_gui::GUIMessage;

enum PacketHandler<'a> {
    Forward(&'a Sender<Packet>),
    Nack(NackType),
    FloodRequest,
    Ignore,
    SendToController,
}

pub struct BagelBomber {
    id: NodeId,
    controller_send: Sender<DroneEvent>,
    controller_recv: Receiver<DroneCommand>,
    packet_recv: Receiver<Packet>,
    packet_send: HashMap<NodeId, Sender<Packet>>,
    pdr: f32,
    active: bool,
    flood_history: HashSet<(NodeId, u64)>,
    #[cfg(feature = "gui")]
    gui_sender: Option<Sender<GUIMessage>>,
}

impl Drone for BagelBomber {
    fn new(
        id: NodeId,
        controller_send: Sender<DroneEvent>,
        controller_recv: Receiver<DroneCommand>,
        packet_recv: Receiver<Packet>,
        packet_send: HashMap<NodeId, Sender<Packet>>,
        pdr: f32,
    ) -> Self {
        BagelBomber {
            id,
            controller_send,
            controller_recv,
            packet_recv,
            packet_send,
            pdr,
            active: false,
            flood_history: HashSet::new(),
            #[cfg(feature = "gui")]
            gui_sender: None,
        }
    }

    fn run(&mut self) {
        self.active = true;
        self.run_internal();
    }
}

impl BagelBomber {
    fn run_internal(&mut self) {
        #[cfg(feature = "debug")]
        println!("BagelBomber {} flying", self.id);

        #[cfg(all(feature = "gui", not(test)))]
        drone_gui::add_gui(self.id, self.pdr, &mut self.gui_sender);

        while self.active {
            select_biased! {
                recv(self.controller_recv) -> command_res => {
                    if let Ok(command) = command_res {
                        self.handle_command(command);
                    }
                }
                recv(self.packet_recv) -> packet_res => {
                    if let Ok(packet) = packet_res {
                        self.handle_packet(packet);
                    }
                }
            }
        }
        #[cfg(all(feature = "gui", not(test)))]
        drone_gui::remove_gui(self.id, &mut self.gui_sender);
    }

    fn handle_command(&mut self, command: DroneCommand) {
        match command {
            DroneCommand::AddSender(id, sender) => {
                self.packet_send.insert(id, sender);
            }
            DroneCommand::Crash => {
                #[cfg(feature = "debug")]
                println!("BagelBomber {} crashed", self.id);
                self.stop();
            }
            DroneCommand::SetPacketDropRate(pdr) => {
                self.pdr = pdr;
                #[cfg(all(feature = "gui", not(test)))]
                drone_gui::change_pdr(self.id, self.pdr, &self.gui_sender);
            }
            DroneCommand::RemoveSender(id) => {
                self.packet_send.remove(&id);
            }
        }
    }

    fn handle_packet(&mut self, packet: Packet) {
        #[cfg(feature = "debug")]
        println!("BagelBomber {} received packet {}", self.id, packet);

        match self.create_packet_handler(packet.clone()) {
            PacketHandler::Forward(sender) => {
                #[cfg(feature = "debug")]
                println!("BagelBomber {} forwarding packet", self.id);
                self.forward(packet, sender);
            }
            PacketHandler::Nack(nack) => {
                #[cfg(feature = "debug")]
                println!("BagelBomber {} sending nack {:?}", self.id, nack);
                let fragment_index = packet.get_fragment_index();
                if let PacketType::Nack(Nack {
                                            nack_type: NackType::Dropped,
                                            ..
                                        }) = &packet.pack_type
                {
                    self.controller_send
                        .send(DroneEvent::PacketDropped(packet.clone()))
                        .ok();
                }
                self.send_back(
                    PacketType::Nack(Nack {
                        fragment_index,
                        nack_type: nack,
                    }),
                    packet.routing_header,
                    packet.session_id,
                );
            }
            PacketHandler::FloodRequest => {
                #[cfg(feature = "debug")]
                println!("BagelBomber {} handling flood request", self.id);
                if let PacketType::FloodRequest(request) = packet.pack_type {
                    self.handle_flood_request(packet.routing_header, packet.session_id, request);
                }
            }
            PacketHandler::SendToController => {
                #[cfg(feature = "debug")]
                println!("BagelBomber {} sending packet to controller", self.id);
                self.controller_send
                    .send(DroneEvent::ControllerShortcut(packet.clone()))
                    .ok();
            }
            PacketHandler::Ignore => {
                #[cfg(feature = "debug")]
                println!("BagelBomber {} ignoring packet", self.id);
            }
        }
    }

    fn create_packet_handler(&self, packet: Packet) -> PacketHandler {
        if let PacketType::FloodRequest(_) = &packet.pack_type {
            PacketHandler::FloodRequest
        } else if packet.routing_header.is_empty() {
            PacketHandler::Ignore
        } else if packet.routing_header.current_hop() != Some(self.id) {
            PacketHandler::Nack(NackType::UnexpectedRecipient(self.id))
        } else if packet.routing_header.is_last_hop() {
            if let PacketType::Nack(_) = packet.pack_type {
                PacketHandler::Ignore
            } else {
                PacketHandler::Nack(NackType::DestinationIsDrone)
            }
        } else {
            let next_hop = packet.routing_header.next_hop().unwrap();
            match self.packet_send.get(&next_hop) {
                Some(sender) => {
                    if let PacketType::MsgFragment(_) = &packet.pack_type {
                        if coin_toss::toss_coin(self.pdr) {
                            #[cfg(all(feature = "gui", not(test)))]
                            drone_gui::drop_bagel(self.id, true, &self.gui_sender);
                            PacketHandler::Nack(NackType::Dropped)
                        } else {
                            #[cfg(all(feature = "gui", not(test)))]
                            drone_gui::drop_bagel(self.id, false, &self.gui_sender);
                            PacketHandler::Forward(sender)
                        }
                    } else {
                        PacketHandler::Forward(sender)
                    }
                }
                None => match &packet.pack_type {
                    PacketType::Ack(_) | PacketType::Nack(_) | PacketType::FloodResponse(_) => {
                        PacketHandler::SendToController
                    }
                    _ => PacketHandler::Nack(NackType::ErrorInRouting(next_hop)),
                },
            }
        }
    }

    fn send_back(
        &mut self,
        packet_type: PacketType,
        current_route: SourceRoutingHeader,
        session_id: u64,
    ) {
        let mut new_route = current_route
            .sub_route(..current_route.hop_index + 1)
            .unwrap()
            .get_reversed();
        new_route.reset_hop_index();
        let new_packet = Packet {
            pack_type: packet_type,
            routing_header: new_route,
            session_id,
        };
        self.handle_packet(new_packet);
    }

    fn handle_flood_request(
        &mut self,
        srh: SourceRoutingHeader,
        session_id: u64,
        mut request: FloodRequest,
    ) {
        let flood_id = request.flood_id;
        let initiator_id = request.initiator_id;

        let recipient = request.path_trace.last().map_or(initiator_id, |(id, _)| *id);
        
        request.increment(self.id, NodeType::Drone);

        if self.flood_history.contains(&(initiator_id, flood_id)) {
            let mut response = request.generate_response(session_id);
            response.routing_header = response.routing_header.without_loops();
            self.handle_packet(response);
        } else {
            self.flood_history.insert((initiator_id, flood_id));
            for (id, sender) in self.packet_send.iter() {
                if id == &recipient {
                    continue;
                }

                self.forward(
                    Packet {
                        routing_header: srh.clone(),
                        session_id,
                        pack_type: PacketType::FloodRequest(request.clone()),
                    },
                    sender,
                );
            }
        }
    }
    fn forward(&self, mut packet: Packet, channel: &Sender<Packet>) {
        self.controller_send
            .send(DroneEvent::PacketSent(packet.clone()))
            .ok();
        packet.routing_header.increase_hop_index();
        channel.send(packet).ok();
    }

    fn stop(&mut self) {
        self.active = false;

        self.finish_up();
    }

    fn finish_up(&mut self) {
        let receive = mem::replace(&mut self.packet_recv, unbounded().1);

        for incoming in receive.iter() {
            match &incoming.pack_type {
                PacketType::MsgFragment(fragment) => {
                    let nack = PacketType::Nack(Nack {
                        fragment_index: fragment.fragment_index,
                        nack_type: NackType::ErrorInRouting(self.id),
                    });
                    self.send_back(nack, incoming.routing_header, incoming.session_id);
                }
                PacketType::FloodRequest(_) => {}
                PacketType::Ack(_) | PacketType::Nack(_) | PacketType::FloodResponse(_) => {
                    self.handle_packet(incoming);
                }
            }
        }
    }
}
