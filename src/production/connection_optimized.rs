use super::ShardedActorState;
use super::connection_pool::BufferPoolAsync;
use crate::redis::{Command, RespValue, RespCodec, RespValueZeroCopy};
use bytes::{BytesMut, Bytes, BufMut};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tracing::{info, warn, error};

pub struct OptimizedConnectionHandler {
    stream: TcpStream,
    state: ShardedActorState,
    buffer: BytesMut,
    write_buffer: BytesMut,
    client_addr: String,
    buffer_pool: Arc<BufferPoolAsync>,
}

impl OptimizedConnectionHandler {
    pub fn new(
        stream: TcpStream,
        state: ShardedActorState,
        client_addr: String,
        buffer_pool: Arc<BufferPoolAsync>,
    ) -> Self {
        let buffer = buffer_pool.acquire();
        let write_buffer = buffer_pool.acquire();
        OptimizedConnectionHandler {
            stream,
            state,
            buffer,
            write_buffer,
            client_addr,
            buffer_pool,
        }
    }
    
    pub async fn run(mut self) {
        info!("Client connected: {}", self.client_addr);
        
        let mut read_buf = [0u8; 8192];
        
        loop {
            match self.stream.read(&mut read_buf).await {
                Ok(0) => {
                    info!("Client disconnected: {}", self.client_addr);
                    break;
                }
                Ok(n) => {
                    self.buffer.extend_from_slice(&read_buf[..n]);
                    
                    while let Some(()) = self.try_execute_command().await {
                        if let Err(e) = self.stream.write_all(&self.write_buffer).await {
                            error!("Failed to write response: {}", e);
                            break;
                        }
                        self.write_buffer.clear();
                    }
                }
                Err(e) => {
                    error!("Error reading from client {}: {}", self.client_addr, e);
                    break;
                }
            }
        }
        
        self.buffer_pool.release(self.buffer);
        self.buffer_pool.release(self.write_buffer);
    }
    
    async fn try_execute_command(&mut self) -> Option<()> {
        match RespCodec::parse(&mut self.buffer) {
            Ok(Some(resp_value)) => {
                match Command::from_resp_zero_copy(&resp_value) {
                    Ok(cmd) => {
                        let response = self.state.execute(&cmd).await;
                        Self::encode_resp_into(&response, &mut self.write_buffer);
                        Some(())
                    }
                    Err(e) => {
                        warn!("Invalid command from {}: {}", self.client_addr, e);
                        Self::encode_error_into(&e, &mut self.write_buffer);
                        Some(())
                    }
                }
            }
            Ok(None) => None,
            Err(e) => {
                warn!("Parse error from {}: {}", self.client_addr, e);
                None
            }
        }
    }

    fn encode_resp_into(value: &RespValue, buf: &mut BytesMut) {
        match value {
            RespValue::SimpleString(s) => {
                buf.put_u8(b'+');
                buf.extend_from_slice(s.as_bytes());
                buf.extend_from_slice(b"\r\n");
            }
            RespValue::Error(s) => {
                buf.put_u8(b'-');
                buf.extend_from_slice(s.as_bytes());
                buf.extend_from_slice(b"\r\n");
            }
            RespValue::Integer(n) => {
                buf.put_u8(b':');
                buf.extend_from_slice(n.to_string().as_bytes());
                buf.extend_from_slice(b"\r\n");
            }
            RespValue::BulkString(None) => {
                buf.extend_from_slice(b"$-1\r\n");
            }
            RespValue::BulkString(Some(data)) => {
                buf.put_u8(b'$');
                buf.extend_from_slice(data.len().to_string().as_bytes());
                buf.extend_from_slice(b"\r\n");
                buf.extend_from_slice(data);
                buf.extend_from_slice(b"\r\n");
            }
            RespValue::Array(None) => {
                buf.extend_from_slice(b"*-1\r\n");
            }
            RespValue::Array(Some(elements)) => {
                buf.put_u8(b'*');
                buf.extend_from_slice(elements.len().to_string().as_bytes());
                buf.extend_from_slice(b"\r\n");
                for elem in elements {
                    Self::encode_resp_into(elem, buf);
                }
            }
        }
    }

    fn encode_error_into(msg: &str, buf: &mut BytesMut) {
        buf.put_u8(b'-');
        buf.extend_from_slice(b"ERR ");
        buf.extend_from_slice(msg.as_bytes());
        buf.extend_from_slice(b"\r\n");
    }
}
