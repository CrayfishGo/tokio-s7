use crate::bytes_to_hex;
use crate::datum::Datum;
use crate::error::S7Error;
use crate::item::{DataItem, RequestItem};
use crate::packet::S7Data;
use crate::paramter::S7Parameter;
use crate::types::{
    CommunicationInfo, CpuInfo, OrderCode, PduType, PlcStatus, PlcType, S7ErrorClass,
};
use log::{error, info, warn};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot};
use tokio::time::sleep;
use tokio_util::bytes::{Buf, BufMut, BytesMut};

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

    pub auto_reconnect: bool,
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
            max_pdu_size: 960,
            auto_reconnect: false,
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

    pub fn with_auto_reconnect(mut self, auto_reconnect: bool) -> Self {
        self.auto_reconnect = auto_reconnect;
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
    ReadSzl {
        szl_id: u16,
        szl_index: u16,
        reply: oneshot::Sender<Result<S7Data, S7Error>>,
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
            pdu_length: config.max_pdu_size,
            connection_state: ConnectionState::Disconnected,
            config,
            cmd_tx: None,
        }
    }

    pub async fn connect(&mut self) -> Result<(), S7Error> {
        // 通道：用于接收握手结果（协商后的 PDU 长度）
        let (result_tx, result_rx) = oneshot::channel();
        let (cmd_tx, cmd_rx) = mpsc::channel(32);
        self.cmd_tx = Some(cmd_tx);

        // 启动后台 Actor
        tokio::spawn(run_actor(
            self.config.clone(),
            self.pdu_length,
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
        let data_items = self.read(vec![req_item]).await?;
        let byte = data_items
            .first()
            .and_then(|item| item.data.first())
            .copied()
            .ok_or_else(|| S7Error::Error("read_bool: empty response".into()))?;

        Ok(((byte & 0xFF) & (1 << 0)) != 0)
    }

    /// 写入一个位（例如 "DB10.5.3" 或 "M1.2"）
    /// 注意：此操作直接写入包含该位的整个字节，位值由 `bit_address` 和 `value` 决定。
    /// 其他位会被置为 0（若不需要影响其他位，请在写入前先读取并修改）。
    pub async fn write_bool(&self, address: &str, value: bool) -> Result<(), S7Error> {
        let req = RequestItem::parse_bit(address)
            .map_err(|e| S7Error::Error(format!("Address parse error: {}", e)))?;
        let data = DataItem::create_req_bool(value)?;
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

    pub async fn szl_read(&self, szl_id: u16, szl_index: u16) -> Result<S7Data, S7Error> {
        let tx = self
            .cmd_tx
            .as_ref()
            .ok_or_else(|| S7Error::Error("Not connected".into()))?;
        let (reply_tx, reply_rx) = oneshot::channel();
        tx.send(Command::ReadSzl {
            szl_id,
            szl_index,
            reply: reply_tx,
        })
        .await
        .map_err(|_| S7Error::Error("Actor disconnected".into()))?;
        reply_rx
            .await
            .map_err(|_| S7Error::Error("Actor dropped".into()))?
    }

    pub async fn get_order_code(&self) -> Result<OrderCode, S7Error> {
        let resp_data = self.szl_read(0x0011, 0x0000).await?;
        match resp_data.datum {
            None => Err(S7Error::Error("Response has no any data".into()))?,
            Some(d) => {
                match d {
                    Datum::Szl(s) => {
                        let payload = s.all_data.unwrap();
                        let n = payload.len();
                        let (v1, v2, v3) = if n >= 3 {
                            (payload[n - 3], payload[n - 2], payload[n - 1])
                        } else {
                            (0, 0, 0)
                        };

                        let szl_id = s.szl_id;
                        let _szl_index = s.szl_index;
                        let entry_len = s.partlist_byte_len.unwrap() as usize;
                        let entry_count = s.partlist_count.unwrap() as usize;

                        let mut buf = BytesMut::from(payload.as_slice());
                        if szl_id == 0x0011 && entry_len >= 4 && entry_count > 0 {
                            for _ in 0..entry_count {
                                if buf.remaining() < entry_len {
                                    break;
                                }
                                let entry_idx = buf.get_u16();
                                let string_len = entry_len - 2;
                                let raw = buf.copy_to_bytes(string_len);
                                if entry_idx == 0x0001 {
                                    let null_end =
                                        raw.iter().position(|&x| x == 0).unwrap_or(string_len);
                                    let code = String::from_utf8_lossy(&raw[..null_end])
                                        .trim()
                                        .to_string();
                                    if !code.is_empty() {
                                        return Ok(OrderCode { code, v1, v2, v3 });
                                    }
                                }
                            }
                        }

                        // Fallback: scan for "6ES"/"6AV"/"6GK" pattern anywhere in payload.
                        let code = scan_ascii_fields(&payload, 10, 4)
                            .into_iter()
                            .find(|s| {
                                let su = s.to_uppercase();
                                (su.starts_with("6ES")
                                    || su.starts_with("6AV")
                                    || su.starts_with("6GK"))
                                    && s.len() >= 10
                                    && s.bytes().all(|c| c.is_ascii_graphic() || c == b' ')
                            })
                            .unwrap_or_default();
                        Ok(OrderCode { code, v1, v2, v3 })
                    }
                    _ => Err(S7Error::Error("Datum is not Szl".into()))?,
                }
            }
        }
    }

    pub async fn get_cpu_info(&self) -> Result<CpuInfo, S7Error> {
        let resp_data = self.szl_read(0x001C, 0x0000).await?;
        match resp_data.datum {
            None => Err(S7Error::Error("Response has no any data".into()))?,
            Some(d) => {
                match d {
                    Datum::Szl(s) => {
                        let payload = s.all_data.unwrap();
                        let szl_id = s.szl_id;
                        let _szl_index = s.szl_index;
                        let entry_len = s.partlist_byte_len.unwrap() as usize;
                        let entry_count = s.partlist_count.unwrap() as usize;

                        let mut buf = BytesMut::from(payload.as_slice());
                        if szl_id == 0x001C && entry_len >= 4 && entry_count > 0 {
                            let mut module_type = String::new();
                            let mut serial_number = String::new();
                            let mut as_name = String::new();
                            let mut copyright = String::new();
                            let mut module_name = String::new();
                            let mut sn_mc = String::new();
                            for _ in 0..entry_count {
                                if buf.remaining() < entry_len {
                                    break;
                                }
                                let entry_idx = buf.get_u16();
                                let string_len = entry_len - 2;
                                let raw = buf.copy_to_bytes(string_len);
                                let null_end =
                                    raw.iter().position(|&x| x == 0).unwrap_or(string_len);
                                let val =
                                    String::from_utf8_lossy(&raw[..null_end]).trim().to_string();

                                match entry_idx {
                                    0x0001 => {
                                        if as_name.is_empty() {
                                            as_name = val;
                                        }
                                    }
                                    // 0x0002 is module type on S7-300, AS name on S7-1500 — only use if
                                    // 0x0007 is absent (module_type_canonical will override below).
                                    0x0002 => {
                                        if module_name.is_empty() {
                                            module_name = val;
                                        }
                                    }
                                    0x0003 => {
                                        if module_name.is_empty() {
                                            module_name = val;
                                        }
                                    }
                                    0x0004 => {
                                        if copyright.is_empty() {
                                            copyright = val;
                                        }
                                    }
                                    0x0005 => {
                                        if serial_number.is_empty() {
                                            serial_number = val;
                                        }
                                    }
                                    // 0x0007 is always the true module type name (both S7-300 and S7-1500)
                                    0x0007 => {
                                        if module_type.is_empty() {
                                            module_type = val;
                                        }
                                    }
                                    // 0x0008 is SMC memory card on S7-1500 — do not use for module_name
                                    0x0008 => {
                                        if sn_mc.is_empty() {
                                            sn_mc = val;
                                        }
                                    }
                                    _ => {}
                                }
                            }

                            if module_name.is_empty() && !as_name.is_empty() {
                                module_name = as_name.clone();
                            }

                            if !module_type.is_empty()
                                || !serial_number.is_empty()
                                || !as_name.is_empty()
                            {
                                return Ok(CpuInfo {
                                    module_type,
                                    serial_number,
                                    as_name,
                                    copyright,
                                    module_name,
                                });
                            }
                        }

                        // S7-1500 and some firmware variants use a tagged sub-record format.
                        // Fall back to scanning the raw payload for tagged string fields.
                        let data = payload.as_ref();
                        let (module_type, serial_number, as_name, copyright, module_name) =
                            parse_sub_record_fields(data);

                        if !module_type.is_empty() || !serial_number.is_empty() {
                            return Ok(CpuInfo {
                                module_type,
                                serial_number,
                                as_name,
                                copyright,
                                module_name,
                            });
                        }

                        // Last-resort scan: extract printable strings and apply heuristics.
                        let mut module_type = String::new();
                        let mut serial_number = String::new();
                        let mut as_name = String::new();
                        let mut copyright = String::new();
                        let mut module_name = String::new();

                        let mut scan = 0;
                        while scan < data.len() {
                            if data[scan].is_ascii_graphic() || data[scan] == b' ' {
                                let start = scan;
                                while scan < data.len()
                                    && (data[scan].is_ascii_graphic() || data[scan] == b' ')
                                {
                                    scan += 1;
                                }
                                let val = String::from_utf8_lossy(&data[start..scan])
                                    .trim()
                                    .to_string();
                                if val.len() >= 3 {
                                    let tag = if start >= 2 && data[start - 2] == 0x00 {
                                        Some(data[start - 1])
                                    } else {
                                        None
                                    };
                                    let su = val.to_uppercase();
                                    if su.contains("BOOT")
                                        || su.starts_with("P B")
                                        || su.starts_with("HBOOT")
                                    {
                                        // skip firmware label
                                    } else if tag == Some(0x07) && module_type.is_empty() {
                                        module_type = val;
                                    } else if tag == Some(0x08) && module_name.is_empty() {
                                        module_name = val;
                                    } else if tag == Some(0x05) && as_name.is_empty() {
                                        as_name = val;
                                    } else if tag == Some(0x06) && copyright.is_empty() {
                                        copyright = val;
                                    } else if tag == Some(0x04) && serial_number.is_empty() {
                                        serial_number = val;
                                    } else if val.contains('-')
                                        && val.chars().filter(|c| c.is_ascii_digit()).count() >= 4
                                        && !val.starts_with("6ES7")
                                        && serial_number.is_empty()
                                    {
                                        serial_number = val;
                                    } else if su.contains("CPU")
                                        && su.contains("PN")
                                        && module_type.is_empty()
                                    {
                                        module_type = val;
                                    } else if module_type.is_empty()
                                        && val.len() >= 8
                                        && !su.contains("MC_")
                                    {
                                        module_type = val;
                                    }
                                }
                            } else {
                                scan += 1;
                            }
                        }

                        Ok(CpuInfo {
                            module_type,
                            serial_number,
                            as_name,
                            copyright,
                            module_name,
                        })
                    }
                    _ => Err(S7Error::Error("Datum is not Szl".into()))?,
                }
            }
        }
    }

    pub async fn get_communication_info(&self) -> Result<CommunicationInfo, S7Error> {
        let resp_data = self.szl_read(0x0131, 0x0001).await?;
        match resp_data.datum {
            None => Err(S7Error::Error("Response has no any data".into()))?,
            Some(d) => {
                match d {
                    Datum::Szl(s) => {
                        let payload = s.all_data.unwrap();

                        let szl_id = s.szl_id;
                        let _szl_index = s.szl_index;
                        let entry_len = s.partlist_byte_len.unwrap() as usize;
                        let entry_count = s.partlist_count.unwrap() as usize;

                        let mut buf = BytesMut::from(payload.as_slice());
                        if szl_id == 0x0131
                            && entry_len >= 12
                            && entry_count >= 1
                            && buf.remaining() >= entry_len
                        {
                            let _entry_idx = buf.get_u16();
                            let max_pdu_len = buf.get_u16() as u32;
                            let max_connections = buf.get_u16() as u32;
                            let max_mpi_rate = buf.get_u32();
                            let max_bus_rate = buf.get_u32();
                            return Ok(CommunicationInfo {
                                max_pdu_len,
                                max_connections,
                                max_mpi_rate,
                                max_bus_rate,
                            });
                        }

                        // Fallback: scan for any parseable numeric data
                        Ok(CommunicationInfo {
                            max_pdu_len: 0,
                            max_connections: 0,
                            max_mpi_rate: 0,
                            max_bus_rate: 0,
                        })
                    }
                    _ => Err(S7Error::Error("Datum is not Szl".into()))?,
                }
            }
        }
    }

    pub async fn get_plc_status(&self) -> Result<PlcStatus, S7Error> {
        let resp_data = self.szl_read(0x0424, 0x0000).await?;
        match resp_data.datum {
            None => Err(S7Error::Error("Response has no any data".into()))?,
            Some(d) => {
                match d {
                    Datum::Szl(s) => {
                        let payload = s.all_data.unwrap();

                        let szl_id = s.szl_id;
                        let _szl_index = s.szl_index;

                        if szl_id == 0x0424 {
                            // payload[0..1]: ereig
                            // payload[2]: ae
                            // payload[3]: bzu-id requested model: Run, Stop ...
                            let status_byte = payload[3];
                            return match status_byte {
                                0x00 => Ok(PlcStatus::Unknown),
                                0x04 => Ok(PlcStatus::Stop),
                                0x08 => Ok(PlcStatus::Run),
                                // Old CPUs sometimes encode STOP as 0x03
                                0x03 => Ok(PlcStatus::Stop),
                                _ => Ok(PlcStatus::Stop),
                            };
                        }
                        // Fallback: scan for any parseable numeric data
                        Ok(PlcStatus::Unknown)
                    }
                    _ => Err(S7Error::Error("Datum is not Szl".into()))?,
                }
            }
        }
    }
}

fn scan_ascii_fields(data: &[u8], max_count: usize, min_len: usize) -> Vec<String> {
    let mut fields = Vec::new();
    let mut i = 0;
    while i < data.len() && fields.len() < max_count {
        // Skip bytes that are not visible ASCII (0x20-0x7E)
        if !data[i].is_ascii_graphic() && data[i] != b' ' {
            i += 1;
            continue;
        }
        // Collect a run of visible ASCII
        let start = i;
        while i < data.len() && (data[i].is_ascii_graphic() || data[i] == b' ') {
            i += 1;
        }
        let s = String::from_utf8_lossy(&data[start..i]).trim().to_string();
        if s.len() >= min_len {
            fields.push(s);
        }
    }
    fields
}

/// Parse the S7-300 sub-record format used in SZL 0x001C responses.
///
/// This format uses tagged records: `[00 <tag> <string>] ...` where
/// known tags are:
/// - 0x01: order code / module identification
/// - 0x05: plant identification (AS name)
/// - 0x06: serial number
/// - 0x07: module type name
/// - 0x08: module name
fn parse_sub_record_fields(b: &[u8]) -> (String, String, String, String, String) {
    let mut module_type = String::new();
    let mut serial_number = String::new();
    let mut as_name = String::new();
    let mut copyright = String::new();
    let mut module_name = String::new();

    let mut i = 0;
    while i + 2 < b.len() {
        // Look for 00 <tag> pattern with a known sub-record tag (1..=8)
        if b[i] == 0x00 && (1..=8).contains(&b[i + 1]) {
            let tag = b[i + 1];
            let start = i + 2;

            // Find end of string: next 0x00 byte (including 00 C0)
            let mut end = start;
            while end < b.len() && b[end] != 0x00 {
                end += 1;
            }

            let raw = &b[start..end];
            let val = String::from_utf8_lossy(raw).trim().to_string();

            // Skip empty and firmware-label values
            let su = val.to_uppercase();
            if !val.is_empty() && !su.contains("BOOT") && !su.starts_with("P B") {
                match tag {
                    0x01 => {
                        // Tag 0x01 may be order code (starts with "6ES") or module type.
                        if !val.starts_with("6ES") && module_type.is_empty() {
                            module_type = val;
                        }
                    }
                    0x05 => {
                        if as_name.is_empty() {
                            as_name = val;
                        }
                    }
                    0x06 => {
                        if serial_number.is_empty() {
                            serial_number = val;
                        }
                    }
                    0x07 => {
                        if module_type.is_empty() {
                            module_type = val;
                        }
                    }
                    0x08 => {
                        if module_name.is_empty() {
                            module_name = val;
                        }
                    }
                    _ => {}
                }
            }

            i = end;
        } else {
            i += 1;
        }
    }

    // Also scan for free-standing printable strings that look like copyright
    // (e.g. "Boot Loader" appearing after the tagged records).
    if copyright.is_empty() {
        let mut scan = 0;
        while scan < b.len() {
            if b[scan].is_ascii_graphic() || b[scan] == b' ' {
                let s = scan;
                while scan < b.len() && (b[scan].is_ascii_graphic() || b[scan] == b' ') {
                    scan += 1;
                }
                let val = String::from_utf8_lossy(&b[s..scan]).trim().to_string();
                let su = val.to_uppercase();
                if val.len() >= 3 {
                    if su.contains("BOOT") || su.starts_with("P B") {
                        copyright = val;
                        break;
                    }
                }
            } else {
                scan += 1;
            }
        }
    }

    (module_type, serial_number, as_name, copyright, module_name)
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
//
// /// 后台 actor 的主循环，负责 TCP 通信与协议处理
// async fn run_actor(
//     config: S7Config,
//     pdu_length: u16,
//     mut cmd_rx: mpsc::Receiver<Command>,
//     result_tx: oneshot::Sender<Result<u16, S7Error>>,
// ) {
//     let addr: SocketAddr = format!("{}:{}", config.host, config.port)
//         .parse()
//         .map_err(|_| S7Error::ConnectionRefused {
//             host: config.host.clone(),
//             port: config.port,
//         })?;
//
//     info!("Connecting to S7 PLC at {}:{}", config.host, config.port);
//
//     let stream = tokio::time::timeout(
//         std::time::Duration::from_millis(config.timeout_ms),
//         TcpStream::connect(&addr),
//     )
//     .await
//     .map_err(|_| S7Error::ConnectionTimeout {
//         host: config.host.clone(),
//         port: config.port,
//     })?
//     .map_err(|_| S7Error::ConnectionRefused {
//         host: config.host.clone(),
//         port: config.port,
//     })?;
//
//     self.connection_state = ConnectionState::Connected;
//
//     let negotiated_pdu = match handshake(&mut stream, local, remote, pdu_length).await {
//         Ok(v) => {
//             let _ = result_tx.send(Ok(v));
//             v
//         }
//         Err(e) => {
//             println!("Handshake failed: {}", e);
//             let _ = result_tx.send(Err(e));
//             return;
//         }
//     };
//
//     let mut decoder = FrameDecoder::new();
//     let next_pdu_ref = AtomicU16::new(0);
//
//     while let Some(cmd) = cmd_rx.recv().await {
//         match cmd {
//             Command::Read { mut items, reply } => {
//                 let res = handle_read(
//                     &mut stream,
//                     &mut decoder,
//                     negotiated_pdu,
//                     &next_pdu_ref,
//                     &mut items,
//                 )
//                 .await;
//                 let _ = reply.send(res);
//             }
//             Command::Write {
//                 mut items,
//                 data,
//                 reply,
//             } => {
//                 let res = handle_write(
//                     &mut stream,
//                     &mut decoder,
//                     negotiated_pdu,
//                     &next_pdu_ref,
//                     &mut items,
//                     data,
//                 )
//                 .await;
//                 let _ = reply.send(res);
//             }
//             Command::ReadSzl {
//                 szl_id,
//                 szl_index,
//                 reply,
//             } => {
//                 let res = handle_read_szl(
//                     &mut stream,
//                     &mut decoder,
//                     negotiated_pdu,
//                     szl_id,
//                     szl_index,
//                     &next_pdu_ref,
//                 )
//                 .await;
//                 let _ = reply.send(res);
//             }
//             Command::Disconnect => break,
//         }
//     }
// }

async fn run_actor(
    config: S7Config,
    initial_pdu: u16,
    mut cmd_rx: mpsc::Receiver<Command>,
    result_tx: oneshot::Sender<Result<u16, S7Error>>, // 原始参数
) {
    // 包装为 Option，方便安全取出
    let mut result_tx = Some(result_tx);
    let mut pdu_length = initial_pdu;

    loop {
        // ---------- 1. 建立 TCP 连接 ----------
        let addr: SocketAddr = match format!("{}:{}", config.host, config.port).parse() {
            Ok(a) => a,
            Err(e) => {
                // 如果首次连接，必须报告错误
                if let Some(tx) = result_tx.take() {
                    let _ = tx.send(Err(S7Error::Error(format!("Invalid address: {}", e))));
                    return;
                }
                // 重连时地址错误则等待重试
                if !config.auto_reconnect {
                    return;                 // 禁止重连，结束
                }
                sleep(Duration::from_secs(2)).await;
                continue;
            }
        };

        let mut stream = match tokio::time::timeout(
            Duration::from_millis(config.timeout_ms),
            TcpStream::connect(&addr),
        )
        .await
        {
            Ok(Ok(s)) => s,
            _ => {
                if let Some(tx) = result_tx.take() {
                    let _ = tx.send(Err(S7Error::ConnectionRefused {
                        host: config.host.clone(),
                        port: config.port,
                    }));
                    return;
                }
                warn!("Reconnect failed, retrying...");
                if !config.auto_reconnect {
                    return;                 // 禁止重连，结束
                }
                sleep(Duration::from_secs(2)).await;
                continue;
            }
        };

        let (local, remote) = match config.plc_type {
            PlcType::S200 => (0x4D57, 0x4D57),
            PlcType::S200Smart => (0x1000, 0x3000),
            PlcType::Sinumerik828D => (0x0400, 0x0D04),
            _ => (0x0100, 0x0300 + (0x20 * config.rack + config.slot) as u16),
        };

        // ---------- 2. 握手 ----------
        let handshake_result = handshake(&mut stream, local, remote, pdu_length).await;

        // ---------- 3. 处理结果（首次连接 vs 重连）----------
        if let Some(tx) = result_tx.take() {
            // 首次连接：必须通知调用者
            match handshake_result {
                Ok(p) => {
                    pdu_length = p;
                    let _ = tx.send(Ok(p));
                }
                Err(e) => {
                    let _ = tx.send(Err(e));
                    return; // 首次失败，终止 actor
                }
            }
        } else {
            // 重连
            match handshake_result {
                Ok(p) => {
                    pdu_length = p;
                    info!("Reconnected, PDU length: {}", p);
                }
                Err(e) => {
                    warn!("Reconnect handshake failed: {}", e);
                    if !config.auto_reconnect {
                        return;                 // 禁止重连，结束
                    }
                    sleep(Duration::from_secs(2)).await;
                    continue;
                }
            }
        }

        // ---------- 4. 进入命令处理循环 ----------
        if let Err(e) = serve_commands(stream, pdu_length, &mut cmd_rx).await {
            info!("Connection lost: {}, reconnecting...", e);
            if !config.auto_reconnect {
                return;                 // 禁止重连，结束
            }
            sleep(Duration::from_secs(2)).await;
        }
    }
}

async fn serve_commands(
    mut stream: TcpStream,
    pdu_limit: u16,
    cmd_rx: &mut mpsc::Receiver<Command>,
) -> Result<(), S7Error> {
    let mut decoder = FrameDecoder::new();
    let next_pdu_ref = AtomicU16::new(0);
    while let Some(cmd) = cmd_rx.recv().await {
        match cmd {
            Command::Read { mut items, reply } => {
                let res = handle_read(
                    &mut stream,
                    &mut decoder,
                    pdu_limit,
                    &next_pdu_ref,
                    &mut items,
                )
                .await;
                let _ = reply.send(res.clone());
                if res.is_err() {
                    return Err(res.unwrap_err());
                }
            }
            Command::Write {
                mut items,
                data,
                reply,
            } => {
                let res = handle_write(
                    &mut stream,
                    &mut decoder,
                    pdu_limit,
                    &next_pdu_ref,
                    &mut items,
                    data,
                )
                .await;
                let _ = reply.send(res.clone());
                if res.is_err() {
                    return Err(res.unwrap_err());
                }
            }
            Command::ReadSzl {
                szl_id,
                szl_index,
                reply,
            } => {
                let res = handle_read_szl(
                    &mut stream,
                    &mut decoder,
                    pdu_limit,
                    &next_pdu_ref,
                    szl_id,
                    szl_index,
                )
                .await;
                let _ = reply.send(res.clone());
                if res.is_err() {
                    return Err(res.unwrap_err());
                }
            }
            Command::Disconnect => return Err(S7Error::ConnectionClosed),
        }
    }
    Ok(())
}

// ---------- 读取处理 ----------

async fn handle_read(
    stream: &mut TcpStream,
    decoder: &mut FrameDecoder,
    pdu_limit: u16,
    next_ref: &AtomicU16,
    items: &mut Vec<RequestItem>,
) -> Result<Vec<DataItem>, S7Error> {
    // 构造请求
    let mut req = S7Data::create_read_request(items);
    if let Some(h) = &mut req.header {
        h.pdu_reference = next_ref.fetch_add(1, Ordering::Relaxed);
    }

    let bytes = req.to_be_bytes();
    // 检查 PDU 长度（不含 TPKT/COTP 头）
    if bytes.len() - 7 > pdu_limit as usize {
        return Err(S7Error::Error("Read request exceeds PDU limit".into()));
    }

    send_frame(stream, &bytes, "ReadRequest").await?;
    let raw = decoder.read_frame(stream).await?;
    let resp = S7Data::from_be_bytes(&raw)
        .map_err(|e| S7Error::Error(format!("Parse S7Data: {}", e)))?
        .ok_or_else(|| S7Error::Error("Not a valid S7 data".to_string()))?;
    check_response(&resp)?;

    // 提取数据
    if let Some(Datum::ReadWrite(datum)) = &resp.datum {
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
    decoder: &mut FrameDecoder,
    pdu_limit: u16,
    next_ref: &AtomicU16,
    items: &mut Vec<RequestItem>,
    data: Vec<DataItem>,
) -> Result<(), S7Error> {
    let mut req = S7Data::create_write_request(items, data);
    if let Some(h) = &mut req.header {
        h.pdu_reference = next_ref.fetch_add(1, Ordering::Relaxed);
    }

    let bytes = req.to_be_bytes();
    if bytes.len() - 7 > pdu_limit as usize {
        return Err(S7Error::Error("Write request exceeds PDU limit".into()));
    }

    send_frame(stream, &bytes, "WriteRequest").await?;
    let raw = decoder.read_frame(stream).await?;
    let resp = S7Data::from_be_bytes(&raw)
        .map_err(|e| S7Error::Error(format!("Parse S7Data: {}", e)))?
        .ok_or_else(|| S7Error::Error("Not a valid S7 data".to_string()))?;
    check_response(&resp)?;
    Ok(())
}

async fn handle_read_szl(
    stream: &mut TcpStream,
    decoder: &mut FrameDecoder,
    pdu_limit: u16,
    next_ref: &AtomicU16,
    szl_id: u16,
    szl_index: u16,
) -> Result<S7Data, S7Error> {
    // 构造请求
    let mut req = S7Data::create_szl_request(szl_id, szl_index);
    if let Some(h) = &mut req.header {
        h.pdu_reference = next_ref.fetch_add(1, Ordering::Relaxed);
    }

    let bytes = req.to_be_bytes();
    // 检查 PDU 长度（不含 TPKT/COTP 头）
    if bytes.len() - 7 > pdu_limit as usize {
        return Err(S7Error::Error("Read request exceeds PDU limit".into()));
    }

    send_frame(stream, &bytes, "ReadSzlRequest").await?;
    let raw = decoder.read_frame(stream).await?;
    let resp = S7Data::from_be_bytes(&raw)
        .map_err(|e| S7Error::Error(format!("Parse S7Data: {}", e)))?
        .ok_or_else(|| S7Error::Error("Not a valid S7 data".to_string()))?;
    check_response(&resp)?;
    Ok(resp)
}

// ---------- 响应校验 ----------

fn check_response(resp: &S7Data) -> Result<(), S7Error> {
    if let Some(h) = &resp.header {
        if h.error_class.is_some()
            && h.error_code.is_some()
            && h.error_class.unwrap() != S7ErrorClass::NoError
        {
            return Err(S7Error::Error(format!(
                "Response error: class={:?}, code={}",
                h.error_class,
                h.error_code.unwrap()
            )));
        }
    }
    Ok(())
}

/// 发送数据（带日志）
async fn send_frame(stream: &mut TcpStream, data: &[u8], desc: &str) -> Result<(), S7Error> {
    info!("Send {}:  {}", desc, bytes_to_hex(data));
    stream.write_all(data).await?;
    Ok(())
}

async fn handshake(
    stream: &mut TcpStream,
    local: u16,
    remote: u16,
    pdu_length: u16,
) -> Result<u16, S7Error> {
    let mut decoder = FrameDecoder::new();
    // ---------- COTP 连接 ----------
    let req = S7Data::create_cr_connection(local, remote);
    send_frame(stream, &req.to_be_bytes(), "COTP").await?;

    let raw = decoder.read_frame(stream).await?;
    let cotp_resp = S7Data::from_be_bytes(&raw)
        .map_err(|e| S7Error::Error(format!("Parse S7Data: {}", e)))?
        .ok_or_else(|| S7Error::Error("Not a valid S7 data".to_string()))?;
    check_response(&cotp_resp)?;

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

    let raw = decoder.read_frame(stream).await?;
    let setup_resp = S7Data::from_be_bytes(&raw)
        .map_err(|e| S7Error::Error(format!("Parse S7Data: {}", e)))?
        .ok_or_else(|| S7Error::Error("Not a valid S7 data".to_string()))?;
    check_response(&setup_resp)?;
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
    if let Some(h) = &setup_resp.header {
        if h.error_class.is_some()
            && h.error_code.is_some()
            && h.error_class.unwrap() != S7ErrorClass::NoError
        {
            error!(
                "Setup 响应错误: {:?} code={}",
                h.error_class,
                h.error_code.unwrap()
            );
            return Err(S7Error::Error(format!(
                "Setup failed: class={:?}, code={}",
                h.error_class,
                h.error_code.unwrap()
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

pub struct FrameDecoder {
    buffer: BytesMut,
}

impl FrameDecoder {
    pub fn new() -> Self {
        Self {
            buffer: BytesMut::with_capacity(4096),
        }
    }

    // /// 从流中读取一个完整的S7协议数据（根据帧头的长度字段）
    // async fn read_full_frame(stream: &mut TcpStream) -> Result<Vec<u8>, S7Error> {
    //     let mut header = [0u8; 4];
    //     stream.read_exact(&mut header).await?;
    //     let len = u16::from_be_bytes([header[2], header[3]]) as usize;
    //     if len < 4 {
    //         return Err(S7Error::Error("Invalid TPKT length".to_string()));
    //     }
    //     let mut buf = vec![0u8; len];
    //     buf[..4].copy_from_slice(&header);
    //     stream.read_exact(&mut buf[4..]).await?;
    //     Ok(buf)
    // }
    //

    /// 尝试从缓冲区中解码一帧（可能包含完整的 TPKT 头）
    pub fn try_decode(&mut self) -> Option<Vec<u8>> {
        if self.buffer.len() < 4 {
            return None;
        }

        let (frame_len, has_tpkt) = if self.buffer[0] == 0x03 && self.buffer[1] == 0x00 {
            let len = u16::from_be_bytes([self.buffer[2], self.buffer[3]]) as usize;
            if len < 4 {
                self.buffer.clear();
                return None;
            }
            (len, true)
        } else {
            // 无 TPKT 头，首字节为 COTP 长度字段
            let cotp_len = self.buffer[0] as usize;
            (1 + cotp_len, false)
        };

        if self.buffer.len() < frame_len {
            // 预留空间，等待更多数据
            self.buffer.reserve(frame_len - self.buffer.len());
            return None;
        }

        let raw = self.buffer.split_to(frame_len);
        if !has_tpkt {
            // 补全 TPKT 头，保证上层总是得到标准帧
            let tpkt_len = 4 + raw.len();
            let mut full = BytesMut::with_capacity(tpkt_len);
            full.put_u8(0x03);
            full.put_u8(0x00);
            full.put_u16(tpkt_len as u16);
            full.put(raw);
            full.to_vec()
        } else {
            raw.to_vec()
        }
        .into()
    }

    /// 从流中读取数据到缓冲区，直到可以解码出一帧，然后返回该帧
    pub async fn read_frame(&mut self, stream: &mut TcpStream) -> Result<Vec<u8>, S7Error> {
        loop {
            if let Some(frame) = self.try_decode() {
                info!("response data:     {}", bytes_to_hex(frame.as_slice()));
                return Ok(frame);
            }
            // 读取新数据
            let mut tmp = [0u8; 1024];
            let n = stream.read(&mut tmp).await?;
            if n == 0 {
                return Err(S7Error::ConnectionClosed);
            }
            self.buffer.extend_from_slice(&tmp[..n]);
        }
    }

    /// 清空缓冲区（连接重建时调用）
    pub fn clear(&mut self) {
        self.buffer.clear();
    }


}
