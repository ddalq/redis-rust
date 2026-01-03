use super::ShardedActorState;
use super::connection_pool::BufferPoolAsync;
use crate::redis::{Command, RespValue, RespCodec};
use bytes::{BytesMut, BufMut};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tracing::{info, warn, error, debug};

const MAX_BUFFER_SIZE: usize = 1024 * 1024;

pub struct OptimizedConnectionHandler {
    stream: TcpStream,
    state: ShardedActorState,
    buffer: BytesMut,
    write_buffer: BytesMut,
    client_addr: String,
    buffer_pool: Arc<BufferPoolAsync>,
}

impl OptimizedConnectionHandler {
    #[inline]
    pub fn new(
        stream: TcpStream,
        state: ShardedActorState,
        client_addr: String,
        buffer_pool: Arc<BufferPoolAsync>,
    ) -> Self {
        let buffer = buffer_pool.acquire();
        let write_buffer = buffer_pool.acquire();
        debug_assert!(buffer.capacity() > 0, "Buffer pool returned zero-capacity buffer");
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

        // Enable TCP_NODELAY for lower latency (disable Nagle's algorithm)
        if let Err(e) = self.stream.set_nodelay(true) {
            warn!("Failed to set TCP_NODELAY: {}", e);
        }

        let mut read_buf = [0u8; 8192];

        loop {
            match self.stream.read(&mut read_buf).await {
                Ok(0) => {
                    info!("Client disconnected: {}", self.client_addr);
                    break;
                }
                Ok(n) => {
                    if self.buffer.len() + n > MAX_BUFFER_SIZE {
                        error!("Buffer overflow from {}, closing connection", self.client_addr);
                        Self::encode_error_into("buffer overflow", &mut self.write_buffer);
                        let _ = self.stream.write_all(&self.write_buffer).await;
                        break;
                    }

                    self.buffer.extend_from_slice(&read_buf[..n]);

                    // Process ALL available commands (pipelining support)
                    let mut commands_executed = 0;
                    let mut had_parse_error = false;

                    loop {
                        match self.try_execute_command().await {
                            CommandResult::Executed => {
                                commands_executed += 1;
                                // Don't flush yet - continue processing pipeline
                            }
                            CommandResult::NeedMoreData => break,
                            CommandResult::ParseError(e) => {
                                warn!("Parse error from {}: {}, draining buffer", self.client_addr, e);
                                self.buffer.clear();
                                Self::encode_error_into("protocol error", &mut self.write_buffer);
                                had_parse_error = true;
                                break;
                            }
                        }
                    }

                    // Flush ALL responses at once (critical for pipelining performance)
                    if !self.write_buffer.is_empty() {
                        if let Err(e) = self.stream.write_all(&self.write_buffer).await {
                            error!("Write failed to {}: {}", self.client_addr, e);
                            break;
                        }
                        // Ensure data is sent immediately
                        if let Err(e) = self.stream.flush().await {
                            error!("Flush failed to {}: {}", self.client_addr, e);
                            break;
                        }
                        self.write_buffer.clear();
                    }

                    if had_parse_error {
                        // Continue to next read after parse error
                    }

                    debug!("Processed {} commands in pipeline batch", commands_executed);
                }
                Err(e) => {
                    debug!("Read error from {}: {}", self.client_addr, e);
                    break;
                }
            }
        }

        self.buffer_pool.release(self.buffer);
        self.buffer_pool.release(self.write_buffer);
    }

    #[inline]
    async fn try_execute_command(&mut self) -> CommandResult {
        match RespCodec::parse(&mut self.buffer) {
            Ok(Some(resp_value)) => {
                match Command::from_resp_zero_copy(&resp_value) {
                    Ok(cmd) => {
                        let response = self.state.execute(&cmd).await;
                        Self::encode_resp_into(&response, &mut self.write_buffer);
                        CommandResult::Executed
                    }
                    Err(e) => {
                        Self::encode_error_into(&e, &mut self.write_buffer);
                        CommandResult::Executed
                    }
                }
            }
            Ok(None) => CommandResult::NeedMoreData,
            Err(e) => CommandResult::ParseError(e),
        }
    }

    #[inline]
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

    #[inline]
    fn encode_error_into(msg: &str, buf: &mut BytesMut) {
        buf.put_u8(b'-');
        buf.extend_from_slice(b"ERR ");
        buf.extend_from_slice(msg.as_bytes());
        buf.extend_from_slice(b"\r\n");
    }
}

enum CommandResult {
    Executed,
    NeedMoreData,
    ParseError(String),
}
