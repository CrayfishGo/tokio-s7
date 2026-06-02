use crate::error::S7Error;
use crate::types::{S7ErrorClass, S7MessageType};
use std::default;
use std::sync::atomic::{AtomicU16, Ordering};
use tokio_util::bytes::{Buf, BufMut, BytesMut};

#[derive(Debug, Eq, PartialEq, Clone, Copy)]
pub enum S7Header {
    ReqHeader(ReqHeader),
    AckHeader(AckHeader),
}

impl default::Default for S7Header {
    fn default() -> Self {
        S7Header::ReqHeader(ReqHeader::default())
    }
}

/// S7请求头 10 字节
#[derive(Debug, Eq, PartialEq, Clone, Copy)]
pub struct ReqHeader {
    /**
     * Protocol id.
     * 协议id <br>
     * 字节大小：1 <br>
     * 字节序数：0
     */
    pub protocol_id: u8,

    /**
     * Message type.
     * pdu（协议数据单元（Protocol Data Unit））的类型 <br>
     * 字节大小：1 <br>
     * 字节序数：1
     */
    pub message_type: S7MessageType,

    /**
     * Reserved.
     * 保留 <br>
     * 字节大小：2 <br>
     * 字节序数：2-3
     */
    pub reserved: u16,

    /**
     * Pdu reference, incremental with each new transmission, big-endian.
     * pdu的参考–由主站生成，每次新传输递增，大端 <br>
     * 字节大小：2 <br>
     * 字节序数：4-5
     */
    pub pdu_reference: u16,

    /**
     * Parameter length.
     * 参数的长度（大端） <br>
     * 字节大小：2 <br>
     * 字节序数：6-7
     */
    pub parameter_length: u16,

    /**
     * Data length.
     * 数据的长度（大端） <br>
     * 字节大小：2 <br>
     * 字节序数：8-9
     */
    pub data_length: u16,
}

#[derive(Debug, Eq, PartialEq, Clone, Copy)]
pub struct AckHeader {
    /**
     * Protocol id.
     * 协议id <br>
     * 字节大小：1 <br>
     * 字节序数：0
     */
    pub protocol_id: u8,

    /**
     * Message type.
     * pdu（协议数据单元（Protocol Data Unit））的类型 <br>
     * 字节大小：1 <br>
     * 字节序数：1
     */
    pub message_type: S7MessageType,

    /**
     * Reserved.
     * 保留 <br>
     * 字节大小：2 <br>
     * 字节序数：2-3
     */
    pub reserved: u16,

    /**
     * Pdu reference, incremental with each new transmission, big-endian.
     * pdu的参考–由主站生成，每次新传输递增，大端 <br>
     * 字节大小：2 <br>
     * 字节序数：4-5
     */
    pub pdu_reference: u16,

    /**
     * Parameter length.
     * 参数的长度（大端） <br>
     * 字节大小：2 <br>
     * 字节序数：6-7
     */
    pub parameter_length: u16,

    /**
     * Data length.
     * 数据的长度（大端） <br>
     * 字节大小：2 <br>
     * 字节序数：8-9
     */
    pub data_length: u16,

    /**
     * Error class.
     * 错误类型 <br>
     * 字节大小：1 <br>
     * 字节序数：10
     */
    pub error_class: S7ErrorClass,

    /**
     * Error code.
     * 错误码，本来是1个字节的，但本质上errorCode（真正） = errorClass + errorCode（原） <br>
     * 字节大小：1 <br>
     * 字节序数：11
     */
    pub error_code: u8,
}

static NEXT_PDU_REF: AtomicU16 = AtomicU16::new(0);
/// 全局递增 PDU 引用（u16，自动 wrap）
fn next_pdu_ref() -> u16 {
    NEXT_PDU_REF.fetch_add(1, Ordering::Relaxed)
}

impl Default for AckHeader {
    fn default() -> AckHeader {
        AckHeader {
            protocol_id: 0x32,
            message_type: S7MessageType::AckData, // 0x03
            reserved: 0x0000,
            pdu_reference: next_pdu_ref(),
            parameter_length: 0x0000,
            data_length: 0x0000,
            error_class: S7ErrorClass::NoError,
            error_code: 0x0000,
        }
    }
}

impl Default for ReqHeader {
    fn default() -> ReqHeader {
        ReqHeader {
            protocol_id: 0x32,
            message_type: S7MessageType::Job,
            reserved: 0x0000,
            pdu_reference: next_pdu_ref(),
            parameter_length: 0x0000,
            data_length: 0x0000,
        }
    }
}

impl ReqHeader {
    pub const BYTE_LENGTH: usize = 10;
}

impl AckHeader {
    pub const BYTE_LENGTH: usize = 12;
}

impl S7Header {
    pub fn set_data_len(&mut self, len: u16) {
        match self {
            S7Header::ReqHeader(h) => h.data_length = len,
            S7Header::AckHeader(h) => h.data_length = len,
        }
    }

    pub fn set_parameter_len(&mut self, len: u16) {
        match self {
            S7Header::ReqHeader(h) => h.parameter_length = len,
            S7Header::AckHeader(h) => h.parameter_length = len,
        }
    }

    pub fn message_type(&self) -> S7MessageType {
        match self {
            S7Header::ReqHeader(h) => h.message_type,
            S7Header::AckHeader(h) => h.message_type,
        }
    }

    pub fn paremater_len(&self) -> u16 {
        match self {
            S7Header::ReqHeader(h) => h.parameter_length,
            S7Header::AckHeader(h) => h.parameter_length,
        }
    }

    pub fn data_len(&self) -> u16 {
        match self {
            S7Header::ReqHeader(h) => h.data_length,
            S7Header::AckHeader(h) => h.data_length,
        }
    }

    pub fn byte_len(&self) -> usize {
        match self {
            S7Header::ReqHeader(_) => ReqHeader::BYTE_LENGTH,
            S7Header::AckHeader(_) => AckHeader::BYTE_LENGTH,
        }
    }

    pub fn to_be_bytes(&self) -> Vec<u8> {
        match self {
            S7Header::ReqHeader(h) => {
                let mut buf = BytesMut::with_capacity(ReqHeader::BYTE_LENGTH);
                buf.put_u8(h.protocol_id);
                buf.put_u8(h.message_type.code());
                buf.put_u16(h.reserved);
                buf.put_u16(h.pdu_reference);
                buf.put_u16(h.parameter_length);
                buf.put_u16(h.data_length);
                buf.to_vec()
            }
            S7Header::AckHeader(h) => {
                let mut buf = BytesMut::with_capacity(AckHeader::BYTE_LENGTH);
                buf.put_u8(h.protocol_id);
                buf.put_u8(h.message_type.code());
                buf.put_u16(h.reserved);
                buf.put_u16(h.pdu_reference);
                buf.put_u16(h.parameter_length);
                buf.put_u16(h.data_length);
                buf.put_u8(h.error_class.code());
                buf.put_u8(h.error_code);
                buf.to_vec()
            }
        }
    }

    pub fn from_be_bytes(bytes: &[u8]) -> Result<Option<Self>, S7Error> {
        let msg_type = bytes[1];
        let message_type = S7MessageType::from(msg_type)
            .ok_or_else(|| S7Error::Error(format!("Unknown message type: 0x{:02X}", msg_type)))?;
        let mut buf = BytesMut::from(bytes);
        let s7header = match message_type {
            S7MessageType::Job => {
                let protocol_id = buf.get_u8();
                let _msg_type = buf.get_u8(); // msg_type，已校验
                let reserved = buf.get_u16();
                let pdu_reference = buf.get_u16();
                let parameter_length = buf.get_u16();
                let data_length = buf.get_u16();
                S7Header::ReqHeader(ReqHeader {
                    protocol_id,
                    message_type,
                    reserved,
                    pdu_reference,
                    parameter_length,
                    data_length,
                })
            }
            S7MessageType::Ack | S7MessageType::AckData => {
                let protocol_id = buf.get_u8();
                let _msg_type = buf.get_u8(); // msg_type，已校验
                let reserved = buf.get_u16();
                let pdu_reference = buf.get_u16();
                let parameter_length = buf.get_u16();
                let data_length = buf.get_u16();
                let error_class = buf.get_u8();
                let error_code = buf.get_u8();
                S7Header::AckHeader(AckHeader {
                    protocol_id,
                    message_type,
                    reserved,
                    pdu_reference,
                    parameter_length,
                    data_length,
                    error_class: S7ErrorClass::from(error_class).ok_or_else(|| {
                        S7Error::Error(format!("Unknown error class: 0x{:02X}", error_class))
                    })?,
                    error_code,
                })
            }
            S7MessageType::UserData => return Ok(None),
        };
        Ok(Some(s7header))
    }
}
