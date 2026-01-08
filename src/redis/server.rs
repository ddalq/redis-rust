use super::resp::RespValue;
use super::{Command, CommandExecutor, RespParser};
use crate::simulator::{Event, EventType, HostId, Simulation};
use std::collections::HashMap;

fn encode_with_request_id(request_id: u64, payload: Vec<u8>) -> Vec<u8> {
    let mut framed = Vec::with_capacity(8 + payload.len());
    framed.extend_from_slice(&request_id.to_le_bytes());
    framed.extend_from_slice(&payload);
    framed
}

fn decode_request_id(data: &[u8]) -> Option<(u64, &[u8])> {
    if data.len() < 8 {
        return None;
    }
    let request_id = u64::from_le_bytes([
        data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
    ]);
    Some((request_id, &data[8..]))
}

pub struct RedisServer {
    host_id: HostId,
    executor: CommandExecutor,
    epoch_initialized: bool,
}

impl RedisServer {
    pub fn new(host_id: HostId) -> Self {
        RedisServer {
            host_id,
            executor: CommandExecutor::new(),
            epoch_initialized: false,
        }
    }

    fn ensure_epoch_initialized(&mut self, sim: &Simulation) {
        if !self.epoch_initialized {
            self.executor
                .set_simulation_start_epoch(sim.simulation_start_epoch());
            self.epoch_initialized = true;
        }
    }

    pub fn handle_event(&mut self, sim: &mut Simulation, event: &Event) {
        if event.host_id != self.host_id {
            return;
        }

        match &event.event_type {
            EventType::NetworkMessage(msg) => {
                self.ensure_epoch_initialized(sim);
                self.executor.set_time(sim.current_time());
                if let Some((request_id, payload)) = decode_request_id(&msg.payload) {
                    if let Ok((resp_value, _)) = RespParser::parse(payload) {
                        if let Ok(cmd) = Command::from_resp(&resp_value) {
                            let response = self.executor.execute(&cmd);
                            let response_bytes = RespParser::encode(&response);
                            let framed_response =
                                encode_with_request_id(request_id, response_bytes);
                            sim.send_message(self.host_id, msg.from, framed_response);
                        }
                    }
                }
            }
            EventType::HostStart => {
                self.ensure_epoch_initialized(sim);
                self.executor.set_time(sim.current_time());
                println!(
                    "[{:?}] Redis server started on host {:?}",
                    sim.current_time(),
                    self.host_id
                );
            }
            _ => {}
        }
    }
}

pub struct RedisClient {
    host_id: HostId,
    server_id: HostId,
    responses: HashMap<u64, RespValue>,
    next_request_id: u64,
}

impl RedisClient {
    pub fn new(host_id: HostId, server_id: HostId) -> Self {
        RedisClient {
            host_id,
            server_id,
            responses: HashMap::new(),
            next_request_id: 0,
        }
    }

    pub fn send_command(&mut self, sim: &mut Simulation, cmd_bytes: Vec<u8>) -> u64 {
        let request_id = self.next_request_id;
        self.next_request_id += 1;

        let framed_message = encode_with_request_id(request_id, cmd_bytes);
        sim.send_message(self.host_id, self.server_id, framed_message);
        request_id
    }

    pub fn handle_event(&mut self, event: &Event) {
        if event.host_id != self.host_id {
            return;
        }

        match &event.event_type {
            EventType::NetworkMessage(msg) => {
                if let Some((request_id, payload)) = decode_request_id(&msg.payload) {
                    if let Ok((resp_value, _)) = RespParser::parse(payload) {
                        self.responses.insert(request_id, resp_value);
                    }
                }
            }
            _ => {}
        }
    }

    pub fn get_response(&self, request_id: u64) -> Option<&RespValue> {
        self.responses.get(&request_id)
    }
}
