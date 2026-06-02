use crate::error::S7Error;
use crate::header::S7Header;
use crate::item::{DataItem, RequestItem};
use crate::packet::S7Data;
use crate::paramter::S7Parameter;
use crate::types::{PduType, PlcType, S7ErrorClass};
use log::{debug, error, info, warn};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU16, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot};

/// S7 connection configuration
#[derive(Debug, Clone)]
pub struct S7Config {
    /// Host address
    pub host: String,
    /// Port (default 102)
    pub port: u16,
    /// PLC type
    pub plc_type: PlcType,
    /// Connection type (PG/OP/S7)
    pub connection_type: u8,
    /// Rack number
    pub rack: u8,
    /// Slot number
    pub slot: u8,
    /// Timeout in milliseconds
    pub timeout_ms: u64,
    /// Maximum PDU size
    pub max_pdu_size: u16,
}

impl Default for S7Config {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 102,
            plc_type: PlcType::S1200,
            connection_type: 0x01, // PG
            rack: 0,
            slot: 2,
            timeout_ms: 5000,
            max_pdu_size: 480,
        }
    }
}

impl S7Config {
    pub fn new(host: &str) -> Self {
        Self {
            host: host.to_string(),
            ..Default::default()
        }
    }

    /// Set PLC type
    pub fn with_plc_type(mut self, plc_type: PlcType) -> Self {
        let (rack, slot) = plc_type.default_rack_slot();
        self.plc_type = plc_type;
        self.rack = rack;
        self.slot = slot;
        self
    }

    /// Set rack and slot
    pub fn with_rack_slot(mut self, rack: u8, slot: u8) -> Self {
        self.rack = rack;
        self.slot = slot;
        self
    }

    /// Set connection type
    pub fn with_connection_type(mut self, conn_type: u8) -> Self {
        self.connection_type = conn_type;
        self
    }

    /// Set port
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Set timeout
    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }
}

/// S7 connection state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    /// Not connected
    Disconnected,
    /// Connecting
    Connecting,
    /// Connected but not negotiated
    Connected,
    /// Connection established and negotiated
    Negotiated,
    /// Communicating
    Communicating,
}

// 命令枚举
pub enum Command {
    Read {
        items: Vec<RequestItem>,
        reply: oneshot::Sender<Result<Vec<DataItem>, S7Error>>,
    },
    Write {
        items: Vec<RequestItem>,
        data: Vec<DataItem>,
        reply: oneshot::Sender<Result<(), S7Error>>,
    },
    Disconnect,
}

#[derive(Debug)]
pub struct S7Client {
    pub pdu_length: u16,
    pub connection_state: ConnectionState,
    pub config: S7Config,
    pub cmd_tx: Option<mpsc::Sender<Command>>,
}

impl S7Client {
    pub fn new(config: S7Config) -> Self {
        Self {
            pdu_length: 240,
            connection_state: ConnectionState::Disconnected,
            config,
            cmd_tx: None,
        }
    }

    pub async fn connect(&mut self) -> Result<(), S7Error> {
        let addr: SocketAddr = format!("{}:{}", self.config.host, self.config.port)
            .parse()
            .map_err(|_| S7Error::ConnectionRefused {
                host: self.config.host.clone(),
                port: self.config.port,
            })?;

        info!(
            "Connecting to S7 PLC at {}:{}",
            self.config.host, self.config.port
        );

        let stream = tokio::time::timeout(
            std::time::Duration::from_millis(self.config.timeout_ms),
            TcpStream::connect(&addr),
        )
        .await
        .map_err(|_| S7Error::ConnectionTimeout {
            host: self.config.host.clone(),
            port: self.config.port,
        })?
        .map_err(|_| S7Error::ConnectionRefused {
            host: self.config.host.clone(),
            port: self.config.port,
        })?;

        self.connection_state = ConnectionState::Connected;

        // 通道：用于接收握手结果（协商后的 PDU 长度）
        let (result_tx, result_rx) = oneshot::channel();
        let (cmd_tx, cmd_rx) = mpsc::channel(32);
        self.cmd_tx = Some(cmd_tx);

        let (local, remote) = match self.config.plc_type {
            PlcType::S200 => (0x4D57, 0x4D57),
            PlcType::S200Smart => (0x1000, 0x3000),
            PlcType::Sinumerik828D => (0x0400, 0x0D04),
            _ => (
                0x0100,
                0x0300 + (0x20 * self.config.rack + self.config.slot) as u16,
            ),
        };

        // 启动后台 Actor
        tokio::spawn(run_actor(
            stream,
            local,
            remote,
            self.pdu_length, // 建议的 PDU 长度
            cmd_rx,
            result_tx,
        ));

        // 等待握手结果
        match result_rx.await {
            Ok(negotiated) => {
                self.pdu_length = negotiated?;
                self.connection_state = ConnectionState::Negotiated;
                info!("S7 connection established, PDU length: {}", self.pdu_length);
                Ok(())
            }
            Err(_) => {
                self.connection_state = ConnectionState::Disconnected;
                Err(S7Error::Error("Actor dropped before handshake".into()))
            }
        }
    }

    /// 异步读取数据（内部通过 Actor 通道）
    pub async fn read(&self, items: Vec<RequestItem>) -> Result<Vec<DataItem>, S7Error> {
        let tx = self
            .cmd_tx
            .as_ref()
            .ok_or_else(|| S7Error::Error("Not connected".into()))?;
        let (reply_tx, reply_rx) = oneshot::channel();
        tx.send(Command::Read {
            items,
            reply: reply_tx,
        })
        .await
        .map_err(|_| S7Error::Error("Actor disconnected".into()))?;
        reply_rx
            .await
            .map_err(|_| S7Error::Error("Actor dropped".into()))?
    }

    /// 异步写入数据
    pub async fn write(&self, items: Vec<RequestItem>, data: Vec<DataItem>) -> Result<(), S7Error> {
        let tx = self
            .cmd_tx
            .as_ref()
            .ok_or_else(|| S7Error::Error("Not connected".into()))?;
        let (reply_tx, reply_rx) = oneshot::channel();
        tx.send(Command::Write {
            items,
            data,
            reply: reply_tx,
        })
        .await
        .map_err(|_| S7Error::Error("Actor disconnected".into()))?;
        reply_rx
            .await
            .map_err(|_| S7Error::Error("Actor dropped".into()))?
    }

    pub async fn read_byte(&self, address: &str) -> Result<u8, S7Error> {
        let bytes = self.read_bytes(address, 1).await?;
        bytes
            .first()
            .copied()
            .ok_or_else(|| S7Error::Error("read_byte: empty response".into()))
    }

    pub async fn write_byte(&self, address: &str, data: u8) -> Result<(), S7Error> {
        let req_item = RequestItem::parse_byte(address, 1)?;
        let data_item = DataItem::create_req_bytes(vec![data])?;
        self.write(vec![req_item], vec![data_item]).await
    }

    pub async fn read_bytes(&self, address: &str, count: u16) -> Result<Vec<u8>, S7Error> {
        let req_item = RequestItem::parse_byte(address, count)?;
        let data_items = self.read(vec![req_item]).await?;
        data_items
            .into_iter()
            .next()
            .map(|item| item.data)
            .ok_or_else(|| S7Error::Error("read_bytes: no data item returned".into()))
    }

    pub async fn write_bytes(&self, address: &str, data: &[u8]) -> Result<(), S7Error> {
        let req_item = RequestItem::parse_byte(address, data.len() as u16)?;
        let data_item = DataItem::create_req_bytes(data.to_vec())?;
        self.write(vec![req_item], vec![data_item]).await
    }

    /// 读取一个位，例如 `DB10.5.3`、`M1.2`
    pub async fn read_bool(&self, address: &str) -> Result<bool, S7Error> {
        let req_item = RequestItem::parse_bit(address)
            .map_err(|e| S7Error::Error(format!("Address parse error: {}", e)))?;
        let bit_index = req_item.bit_address;

        let data_items = self.read(vec![req_item]).await?;
        let byte = data_items
            .first()
            .and_then(|item| item.data.first())
            .copied()
            .ok_or_else(|| S7Error::Error("read_bool: empty response".into()))?;

        Ok((byte >> bit_index) & 0x01 == 1)
    }

    /// 写入一个位（例如 "DB10.5.3" 或 "M1.2"）
    /// 注意：此操作直接写入包含该位的整个字节，位值由 `bit_address` 和 `value` 决定。
    /// 其他位会被置为 0（若不需要影响其他位，请在写入前先读取并修改）。
    pub async fn write_bool(&self, address: &str, value: bool) -> Result<(), S7Error> {
        let req = RequestItem::parse_bit(address)
            .map_err(|e| S7Error::Error(format!("Address parse error: {}", e)))?;
        let bit_index = req.bit_address;
        let byte_value = if value { 1u8 << bit_index } else { 0u8 };
        let data = DataItem::create_req_bytes(vec![byte_value])?;
        self.write(vec![req], vec![data]).await
    }

    // ---------- 16-bit signed/unsigned ----------

    pub async fn read_int16(&self, address: &str) -> Result<i16, S7Error> {
        let bytes = self.read_bytes(address, 2).await?;
        Ok(i16::from_be_bytes([bytes[0], bytes[1]]))
    }

    pub async fn read_uint16(&self, address: &str) -> Result<u16, S7Error> {
        let bytes = self.read_bytes(address, 2).await?;
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
    }

    pub async fn write_int16(&self, address: &str, data: i16) -> Result<(), S7Error> {
        let bytes = data.to_be_bytes();
        let req_item = RequestItem::parse_byte(address, 2)?;
        let data_item = DataItem::create_req_bytes(bytes.to_vec())?;
        self.write(vec![req_item], vec![data_item]).await
    }

    pub async fn write_uint16(&self, address: &str, data: u16) -> Result<(), S7Error> {
        let bytes = data.to_be_bytes();
        let req_item = RequestItem::parse_byte(address, 2)?;
        let data_item = DataItem::create_req_bytes(bytes.to_vec())?;
        self.write(vec![req_item], vec![data_item]).await
    }

    // ---------- 32-bit signed/unsigned ----------

    pub async fn read_int32(&self, address: &str) -> Result<i32, S7Error> {
        let bytes = self.read_bytes(address, 4).await?;
        Ok(i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// S7 的 UInt32 直接返回 u32（0 ~ 4_294_967_295）
    pub async fn read_uint32(&self, address: &str) -> Result<u32, S7Error> {
        let bytes = self.read_bytes(address, 4).await?;
        Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    pub async fn write_int32(&self, address: &str, value: i32) -> Result<(), S7Error> {
        let req = RequestItem::parse_byte(address, 4)
            .map_err(|e| S7Error::Error(format!("Address parse error: {}", e)))?;
        let data = DataItem::create_req_bytes(value.to_be_bytes().to_vec())?;
        self.write(vec![req], vec![data]).await
    }

    pub async fn write_uint32(&self, address: &str, value: u32) -> Result<(), S7Error> {
        let req = RequestItem::parse_byte(address, 4)
            .map_err(|e| S7Error::Error(format!("Address parse error: {}", e)))?;
        let data = DataItem::create_req_bytes(value.to_be_bytes().to_vec())?;
        self.write(vec![req], vec![data]).await
    }

    // ---------- 64-bit signed/unsigned ----------

    pub async fn read_int64(&self, address: &str) -> Result<i64, S7Error> {
        let bytes = self.read_bytes(address, 8).await?;
        Ok(i64::from_be_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    pub async fn read_uint64(&self, address: &str) -> Result<u64, S7Error> {
        let bytes = self.read_bytes(address, 8).await?;
        Ok(u64::from_be_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    pub async fn write_int64(&self, address: &str, value: i64) -> Result<(), S7Error> {
        let req = RequestItem::parse_byte(address, 8)
            .map_err(|e| S7Error::Error(format!("Address parse error: {}", e)))?;
        let data = DataItem::create_req_bytes(value.to_be_bytes().to_vec())?;
        self.write(vec![req], vec![data]).await
    }

    pub async fn write_uint64(&self, address: &str, value: u64) -> Result<(), S7Error> {
        let req = RequestItem::parse_byte(address, 8)
            .map_err(|e| S7Error::Error(format!("Address parse error: {}", e)))?;
        let data = DataItem::create_req_bytes(value.to_be_bytes().to_vec())?;
        self.write(vec![req], vec![data]).await
    }

    // ---------- Float (32-bit) / Double (64-bit) ----------

    pub async fn read_float32(&self, address: &str) -> Result<f32, S7Error> {
        let bytes = self.read_bytes(address, 4).await?;
        Ok(f32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    pub async fn read_float64(&self, address: &str) -> Result<f64, S7Error> {
        let bytes = self.read_bytes(address, 8).await?;
        Ok(f64::from_be_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    pub async fn write_float32(&self, address: &str, value: f32) -> Result<(), S7Error> {
        let req = RequestItem::parse_byte(address, 4)
            .map_err(|e| S7Error::Error(format!("Address parse error: {}", e)))?;
        let data = DataItem::create_req_bytes(value.to_be_bytes().to_vec())?;
        self.write(vec![req], vec![data]).await
    }

    pub async fn write_float64(&self, address: &str, value: f64) -> Result<(), S7Error> {
        let req = RequestItem::parse_byte(address, 8)
            .map_err(|e| S7Error::Error(format!("Address parse error: {}", e)))?;
        let data = DataItem::create_req_bytes(value.to_be_bytes().to_vec())?;
        self.write(vec![req], vec![data]).await
    }

    // ---------- STRING / WSTRING ----------

    /// 读取西门子 STRING 类型（ASCII/ISO-8859-1）
    /// `max_len` 为声明最大字符数，默认为 254。
    pub async fn read_string(&self, address: &str, max_len: u16) -> Result<String, S7Error> {
        // STRING 存储格式：max_len(1B) + actual_len(1B) + chars(actual_len B)
        let total_len = max_len + 2;
        let bytes = self.read_bytes(address, total_len).await?;
        if bytes.len() < 2 {
            return Err(S7Error::Error("read_string: response too short".into()));
        }
        let actual_len = bytes[1] as usize;
        if bytes.len() < 2 + actual_len {
            return Err(S7Error::Error("read_string: data truncated".into()));
        }
        let chars = &bytes[2..2 + actual_len];
        Ok(String::from_utf8_lossy(chars).into_owned()) // 使用 ISO-8859-1 兼容
    }

    /// 读取西门子 STRING 类型（默认最大长度 254）
    pub async fn read_string_default(&self, address: &str) -> Result<String, S7Error> {
        self.read_string(address, 254).await
    }

    /// 读取西门子 WSTRING 类型（UTF-16BE）
    /// `max_len` 为声明最大字符数，默认为 254。
    pub async fn read_wstring(&self, address: &str, max_len: u16) -> Result<String, S7Error> {
        let total_len = max_len * 2 + 4;
        let bytes = self.read_bytes(address, total_len).await?;
        if bytes.len() < 4 {
            return Err(S7Error::Error("read_wstring: response too short".into()));
        }
        let max_len_resp = u16::from_be_bytes([bytes[0], bytes[1]]) as usize;
        let mut actual_len = u16::from_be_bytes([bytes[2], bytes[3]]) as usize;
        if actual_len > max_len_resp {
            actual_len = max_len_resp;
        }
        let data_start = 4;
        let data_end = data_start + actual_len * 2;
        if bytes.len() < data_end {
            return Err(S7Error::Error("read_wstring: data truncated".into()));
        }

        // 将 UTF-16BE 字节流转换为 u16 数组
        let u16s: Vec<u16> = bytes[data_start..data_end]
            .chunks_exact(2) // 将数据区按每 2 字节一组分割。
            .map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]])) // 对于每组 2 字节，用 u16::from_be_bytes 组装成大端序的 16 位无符号整数（UTF‑16 代码单元）
            .collect();

        String::from_utf16(&u16s)
            .map_err(|_| S7Error::Error("read_wstring: invalid UTF-16 data".into()))
    }

    /// 读取西门子 WSTRING 类型（默认最大长度 254）
    pub async fn read_wstring_default(&self, address: &str) -> Result<String, S7Error> {
        self.read_wstring(address, 254).await
    }


    /// 写入西门子 STRING（Latin-1 编码，不支持中文）
    /// `max_len` 为 PLC 中声明的最大字符数。
    /// 若字符串长度超过 `max_len` 或包含无法编码的字符，返回错误。
    pub async fn write_string(
        &self,
        address: &str,
        max_len: u16,
        value: &str,
    ) -> Result<(), S7Error> {
        let latin1_bytes = str_to_latin1(value)?;
        let actual_len = latin1_bytes.len() as u16;
        if actual_len > max_len {
            return Err(S7Error::Error(format!(
                "String too long: {} bytes, max allowed {}",
                actual_len, max_len
            )));
        }
        // 构造 STRING 格式字节：max_len(1B) + actual_len(1B) + data
        let total_len = (max_len + 2) as usize;
        let mut buf = vec![0u8; total_len];
        buf[0] = max_len as u8;
        buf[1] = actual_len as u8;
        buf[2..2 + actual_len as usize].copy_from_slice(&latin1_bytes);
        // 剩余部分已经是 0
        let req = RequestItem::parse_byte(address, total_len as u16)
            .map_err(|e| S7Error::Error(format!("Address parse error: {}", e)))?;
        let data = DataItem::create_req_bytes(buf)?;
        self.write(vec![req], vec![data]).await
    }

    /// 写入西门子 WSTRING（UTF-16BE 编码，支持所有 Unicode 字符）
    /// `max_len` 为 PLC 中声明的最大字符数（不是字节数）。
    /// 若字符串代码单元数超过 `max_len`，返回错误。
    pub async fn write_wstring(
        &self,
        address: &str,
        max_len: u16,
        value: &str,
    ) -> Result<(), S7Error> {
        // 获取 UTF-16 代码单元数（注意：一个字符可能对应 1 个或 2 个代码单元）
        let utf16_units: Vec<u16> = value.encode_utf16().collect();
        let actual_len = utf16_units.len() as u16;
        if actual_len > max_len {
            return Err(S7Error::Error(format!(
                "WString too long: {} code units, max allowed {}",
                actual_len, max_len
            )));
        }
        // 构造 WSTRING 字节：max_len(2B big-endian) + actual_len(2B big-endian) + UTF-16BE 数据
        let total_bytes = (max_len * 2 + 4) as usize;
        let mut buf = vec![0u8; total_bytes];
        buf[0..2].copy_from_slice(&max_len.to_be_bytes());
        buf[2..4].copy_from_slice(&actual_len.to_be_bytes());
        for (i, unit) in utf16_units.iter().enumerate() {
            let pos = 4 + i * 2;
            buf[pos..pos + 2].copy_from_slice(&unit.to_be_bytes());
        }
        // 剩余位置保持 0
        let req = RequestItem::parse_byte(address, total_bytes as u16)
            .map_err(|e| S7Error::Error(format!("Address parse error: {}", e)))?;
        let data = DataItem::create_req_bytes(buf)?;
        self.write(vec![req], vec![data]).await
    }

}

/// 将 Rust 字符串转换为 Latin-1 (ISO-8859-1) 字节序列。
/// 若包含无法编码的字符则返回错误。
fn str_to_latin1(s: &str) -> Result<Vec<u8>, S7Error> {
    let mut bytes = Vec::with_capacity(s.len());
    for c in s.chars() {
        if c as u32 > 0xFF {
            return Err(S7Error::Error(format!(
                "Character '{}' cannot be encoded in Latin-1 / STRING",
                c
            )));
        }
        bytes.push(c as u8);
    }
    Ok(bytes)
}


/// 后台 actor 的主循环，负责 TCP 通信与协议处理
async fn run_actor(
    mut stream: TcpStream,
    local: u16,
    remote: u16,
    pdu_length: u16,
    mut cmd_rx: mpsc::Receiver<Command>,
    result_tx: oneshot::Sender<Result<u16, S7Error>>,
) {
    let negotiated_pdu = match handshake(&mut stream, local, remote, pdu_length).await {
        Ok(v) => {
            let _ = result_tx.send(Ok(v));
            v
        }
        Err(e) => {
            println!("Handshake failed: {}", e);
            let _ = result_tx.send(Err(e));
            return;
        }
    };

    let next_pdu_ref = AtomicU16::new(0);

    while let Some(cmd) = cmd_rx.recv().await {
        match cmd {
            Command::Read { mut items, reply } => {
                let res = handle_read(&mut stream, negotiated_pdu, &next_pdu_ref, &mut items).await;
                let _ = reply.send(res);
            }
            Command::Write {
                mut items,
                data,
                reply,
            } => {
                let res =
                    handle_write(&mut stream, negotiated_pdu, &next_pdu_ref, &mut items, data)
                        .await;
                let _ = reply.send(res);
            }
            Command::Disconnect => break,
        }
    }
}

// ---------- 读取处理 ----------

async fn handle_read(
    stream: &mut TcpStream,
    pdu_limit: u16,
    next_ref: &AtomicU16,
    items: &mut Vec<RequestItem>,
) -> Result<Vec<DataItem>, S7Error> {
    // 构造请求
    let mut req = S7Data::create_read_request(items);
    if let Some(S7Header::ReqHeader(hdr)) = &mut req.header {
        hdr.pdu_reference = next_ref.fetch_add(1, Ordering::Relaxed);
    }

    let bytes = req.to_be_bytes();
    // 检查 PDU 长度（不含 TPKT/COTP 头）
    if bytes.len() - 7 > pdu_limit as usize {
        return Err(S7Error::Error("Read request exceeds PDU limit".into()));
    }

    send_frame(stream, &bytes, "ReadRequest").await?;
    let resp = read_s7_frame(stream).await?;
    check_response(&resp)?;

    // 提取数据
    if let Some(crate::datum::Datum::ReadWrite(datum)) = &resp.datum {
        let mut data_items = Vec::new();
        for item in &datum.return_items {
            if let crate::item::S7ReturnItem::Data(di) = item {
                data_items.push(di.clone());
            } else {
                warn!("Unexpected non-data return item in read response");
            }
        }
        Ok(data_items)
    } else {
        Err(S7Error::Error("No data in read response".into()))
    }
}

// ---------- 写入处理 ----------
async fn handle_write(
    stream: &mut TcpStream,
    pdu_limit: u16,
    next_ref: &AtomicU16,
    items: &mut Vec<RequestItem>,
    data: Vec<DataItem>,
) -> Result<(), S7Error> {
    let mut req = S7Data::create_write_request(items, data);
    if let Some(S7Header::ReqHeader(hdr)) = &mut req.header {
        hdr.pdu_reference = next_ref.fetch_add(1, Ordering::Relaxed);
    }

    let bytes = req.to_be_bytes();
    if bytes.len() - 7 > pdu_limit as usize {
        return Err(S7Error::Error("Write request exceeds PDU limit".into()));
    }

    send_frame(stream, &bytes, "WriteRequest").await?;
    let resp = read_s7_frame(stream).await?;
    check_response(&resp)?;
    Ok(())
}

// ---------- 响应校验 ----------

fn check_response(resp: &S7Data) -> Result<(), S7Error> {
    if let Some(S7Header::AckHeader(ack)) = &resp.header {
        if ack.error_class != S7ErrorClass::NoError {
            return Err(S7Error::Error(format!(
                "Response error: class={:?}, code={}",
                ack.error_class, ack.error_code
            )));
        }
    }
    // 可增加 PDU reference 匹配检查
    Ok(())
}

/// 从流中读取一个完整的S7协议数据（根据帧头的长度字段）
async fn read_full_frame(stream: &mut TcpStream) -> Result<Vec<u8>, S7Error> {
    let mut header = [0u8; 4];
    stream.read_exact(&mut header).await?;
    let len = u16::from_be_bytes([header[2], header[3]]) as usize;
    if len < 4 {
        return Err(S7Error::Error("Invalid TPKT length".to_string()));
    }
    let mut buf = vec![0u8; len];
    buf[..4].copy_from_slice(&header);
    stream.read_exact(&mut buf[4..]).await?;
    Ok(buf)
}

async fn read_s7_frame(stream: &mut TcpStream) -> Result<S7Data, S7Error> {
    let raw = read_full_frame(stream).await?;
    debug!("response data: {}", bytes_to_hex(&raw));
    S7Data::from_be_bytes(&raw)
        .map_err(|e| S7Error::Error(format!("Parse S7Data: {}", e)))?
        .ok_or_else(|| S7Error::Error("Not a valid S7 data".to_string()))
}

/// 发送数据（带日志）
async fn send_frame(stream: &mut TcpStream, data: &[u8], desc: &str) -> Result<(), S7Error> {
    debug!("Send {}: {}", desc, bytes_to_hex(data));
    stream
        .write_all(data)
        .await
        .map_err(|e| S7Error::IoErr(e))?;
    Ok(())
}

async fn handshake(
    stream: &mut TcpStream,
    local: u16,
    remote: u16,
    pdu_length: u16,
) -> Result<u16, S7Error> {
    // ---------- COTP 连接 ----------
    let req = S7Data::create_cr_connection(local, remote);
    send_frame(stream, &req.to_be_bytes(), "COTP").await?;

    let cotp_resp = read_s7_frame(stream).await?;
    info!(
        "Receive CTOP: {}",
        bytes_to_hex(cotp_resp.to_be_bytes().as_ref())
    );
    let cotp = cotp_resp
        .cotp
        .as_ref()
        .ok_or_else(|| S7Error::Error("Missing COTP in response".to_string()))?;
    if cotp.pdu_type() != PduType::ConnectConfirm {
        error!("COTP 连接被拒绝: {:?}", cotp.pdu_type());
        return Err(S7Error::Error("COTP connection refused".to_string()));
    }

    info!("COTP Connection Confirmed");

    // ---------- S7 Setup 协商 ----------
    let req = S7Data::create_setup_comm(pdu_length);
    send_frame(stream, &req.to_be_bytes(), "SetupComm").await?;

    let setup_resp = read_s7_frame(stream).await?;
    info!(
        "Receive SetupComm: {}",
        bytes_to_hex(setup_resp.to_be_bytes().as_ref())
    );
    // 检查 COTP 类型
    let setup_cotp = setup_resp
        .cotp
        .as_ref()
        .ok_or_else(|| S7Error::Error("Missing COTP in setup response".to_string()))?;
    if setup_cotp.pdu_type() != PduType::DtData {
        error!("Setup 响应 COTP 类型错误: {:?}", setup_cotp.pdu_type());
        return Err(S7Error::Error("Setup response COTP type error".to_string()));
    }

    // 检查头部错误
    if let Some(S7Header::AckHeader(ack)) = &setup_resp.header {
        if ack.error_class != S7ErrorClass::NoError {
            error!(
                "Setup 响应错误: {:?} code={}",
                ack.error_class, ack.error_code
            );
            return Err(S7Error::Error(format!(
                "Setup failed: class={:?}, code={}",
                ack.error_class, ack.error_code
            )));
        }
    } else {
        error!("Setup 响应Header 缺失");
        return Err(S7Error::Error(
            "Missing header in setup response".to_string(),
        ));
    }

    // 提取协商后的 PDU 长度
    let negotiated_pdu = match setup_resp
        .parameter
        .as_ref()
        .ok_or_else(|| S7Error::Error("Missing parameter in setup response".to_string()))?
    {
        S7Parameter::SetupParameter(s) => {
            info!("S7 Setup complete，negotiated PDU length: {}", s.pdu_length);
            s.pdu_length
        }
        _ => {
            error!("Setup 响应参数类型不匹配");
            return Err(S7Error::Error("Setup parameter type mismatch".to_string()));
        }
    };

    Ok(negotiated_pdu)
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{:02X}", b))
        .collect::<Vec<_>>()
        .join(" ")
}
