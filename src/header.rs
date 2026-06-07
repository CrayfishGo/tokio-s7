use crate::error::S7Error;
use crate::types::{S7ErrorClass, S7MessageType};
use std::default;
use std::sync::atomic::{AtomicU16, Ordering};
use tokio_util::bytes::{Buf, BufMut, BytesMut};

#[derive(Debug, Eq, PartialEq, Clone, Copy)]
pub struct S7Header {
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
    pub error_class: Option<S7ErrorClass>,

    /**
     * Error code.
     * 错误码，本来是1个字节的，但本质上errorCode（真正） = errorClass + errorCode（原） <br>
     * 字节大小：1 <br>
     * 字节序数：11
     */
    pub error_code: Option<u8>,
}

impl default::Default for S7Header {
    fn default() -> Self {
        S7Header {
            protocol_id: 0x32,
            message_type: S7MessageType::Job,
            reserved: 0x0000,
            pdu_reference: next_pdu_ref(),
            parameter_length: 0x0000,
            data_length: 0x0000,
            error_class: None,
            error_code: None,
        }
    }
}

static NEXT_PDU_REF: AtomicU16 = AtomicU16::new(0);
/// 全局递增 PDU 引用（u16，自动 wrap）
fn next_pdu_ref() -> u16 {
    NEXT_PDU_REF.fetch_add(1, Ordering::Relaxed)
}

impl S7Header {
    pub fn new(message_type: S7MessageType) -> Self {
        S7Header {
            protocol_id: 0x32,
            message_type,
            reserved: 0x0000,
            pdu_reference: next_pdu_ref(),
            parameter_length: 0x0000,
            data_length: 0x0000,
            error_class: None,
            error_code: None,
        }
    }

    pub fn set_data_len(&mut self, len: u16) {
        self.data_length = len;
    }

    pub fn set_parameter_len(&mut self, len: u16) {
        self.parameter_length = len;
    }

    pub fn message_type(&self) -> S7MessageType {
        self.message_type
    }

    pub fn paremater_len(&self) -> u16 {
        self.parameter_length
    }

    pub fn data_len(&self) -> u16 {
        self.data_length
    }

    pub fn byte_len(&self) -> usize {
        if let (Some(_ec), Some(_ecd)) = (self.error_class, self.error_code) {
            12
        } else {
            10
        }
    }

    pub fn to_be_bytes(&self) -> Vec<u8> {
        let mut buf = BytesMut::new();
        buf.put_u8(self.protocol_id);
        buf.put_u8(self.message_type.code());
        buf.put_u16(self.reserved);
        buf.put_u16(self.pdu_reference);
        buf.put_u16(self.parameter_length);
        buf.put_u16(self.data_length);
        if let (Some(ec), Some(ecd)) = (self.error_class, self.error_code) {
            buf.put_u8(ec.code());
            buf.put_u8(ecd);
        }
        buf.to_vec()
    }

    pub fn from_be_bytes(bytes: &[u8]) -> Result<Option<Self>, S7Error> {
        let msg_type = bytes[1];
        let message_type = S7MessageType::from(msg_type)
            .ok_or_else(|| S7Error::Error(format!("Unknown message type: 0x{:02X}", msg_type)))?;
        let mut buf = BytesMut::from(bytes);
        let protocol_id = buf.get_u8();
        let _msg_type = buf.get_u8(); // msg_type，已校验
        let reserved = buf.get_u16();
        let pdu_reference = buf.get_u16();
        let parameter_length = buf.get_u16();
        let data_length = buf.get_u16();
        let (error_class, error_code) = match message_type {
            S7MessageType::Ack | S7MessageType::AckData if buf.remaining() >= 2 => {
                let ec = buf.get_u8();
                let ec = S7ErrorClass::from(buf.get_u8())
                    .ok_or_else(|| S7Error::Error(format!("Unknown error class: 0x{:02X}", ec)))?;
                (Some(ec), Some(buf.get_u8()))
            }
            _ => (None, None),
        };
        Ok(Some(S7Header {
            protocol_id,
            message_type,
            reserved,
            pdu_reference,
            parameter_length,
            data_length,
            error_class,
            error_code,
        }))
    }
}
