use super::ShardedRedisState;
use crate::redis::{Command, RespParser, RespValue};
use bytes::BytesMut;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tracing::{info, warn, error};

pub struct ConnectionHandler {
    stream: TcpStream,
    state: ShardedRedisState,
    buffer: BytesMut,
    client_addr: String,
}

impl ConnectionHandler {
    pub fn new(stream: TcpStream, state: ShardedRedisState, client_addr: String) -> Self {
        ConnectionHandler {
            stream,
            state,
            buffer: BytesMut::with_capacity(4096),
            client_addr,
        }
    }
    
    pub async fn run(mut self) {
        info!("Client connected: {}", self.client_addr);
        
        loop {
            let mut read_buf = vec![0u8; 4096];
            match self.stream.read(&mut read_buf).await {
                Ok(0) => {
                    info!("Client disconnected: {}", self.client_addr);
                    break;
                }
                Ok(n) => {
                    self.buffer.extend_from_slice(&read_buf[..n]);
                    
                    while let Some(response) = self.try_execute_command() {
                        if let Err(e) = self.stream.write_all(&response).await {
                            error!("Failed to write response: {}", e);
                            break;
                        }
                    }
                }
                Err(e) => {
                    error!("Error reading from client {}: {}", self.client_addr, e);
                    break;
                }
            }
        }
    }
    
    fn try_execute_command(&mut self) -> Option<Vec<u8>> {
        match RespParser::parse(&self.buffer) {
            Ok((resp_value, bytes_consumed)) => {
                self.buffer.advance(bytes_consumed);
                
                match Command::from_resp(&resp_value) {
                    Ok(cmd) => {
                        let response = self.state.execute(&cmd);
                        Some(RespParser::encode(&response))
                    }
                    Err(e) => {
                        warn!("Invalid command from {}: {}", self.client_addr, e);
                        let error = RespValue::Error(format!("ERR {}", e));
                        Some(RespParser::encode(&error))
                    }
                }
            }
            Err(_) => None,
        }
    }
}

// Extension trait for BytesMut
trait BytesMutExt {
    fn advance(&mut self, cnt: usize);
}

impl BytesMutExt for BytesMut {
    fn advance(&mut self, cnt: usize) {
        let _ = self.split_to(cnt);
    }
}
