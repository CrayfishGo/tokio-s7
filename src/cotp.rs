use crate::error::S7Error;
use crate::types::PduType;
use tokio_util::bytes::{Buf, BufMut, BytesMut};

#[derive(Debug, Eq, PartialEq, Clone, Copy)]
pub enum Cotp {
    CotpConnection(CotpConnection),
    CotpData(CotpData),
}

impl Default for Cotp {
    fn default() -> Self {
        Cotp::CotpConnection(CotpConnection::default())
    }
}

/// cotp连接参数 18 字节
#[derive(Debug, Eq, PartialEq, Clone, Copy)]
pub struct CotpConnection {
    /**
     * Length, exclude this length field.
     * 长度（但并不包含length这个字段）<br>
     * 字节大小：1 <br>
     * 字节序数：0
     */
    pub length: u8,

    /**
     * PDU type.
     * PDU类型（CRConnect Request 连接请求）<br>
     * 字节大小：1 <br>
     * 字节序数：1
     */
    pub pdu_type: PduType,

    /**
     * Destination reference, used to uniquely identify the target.
     * 目标引用，用来唯一标识目标 <br>
     * 字节大小：2 <br>
     * 字节序数：2-3
     */
    pub destination_reference: u16,

    /**
     * Source reference.
     * 源引用 <br>
     * 字节大小：2 <br>
     * 字节序数：4-5
     */
    pub source_reference: u16,

    /**
     * Extended format/flow control.
     * 扩展格式/流控制  前四位标识Class，  倒数第二位Extended formats，  倒数第一位No explicit flow control <br>
     * 字节大小：1 <br>
     * 字节序数：6
     */
    pub flags: u8,

    /**
     * Parameter code tpdu size.
     * 参数代码TPDU-Size <br>
     * 字节大小：1 <br>
     * 字节序数：7
     */
    pub parameter_code_tpdu_size: u8,

    /**
     * Tpdu size byte length.
     * 参数长度 <br>
     * 字节大小：1 <br>
     * 字节序数：8
     */
    pub parameter_length1: u8,

    /**
     * TPDU大小 TPDU Size (2^10 = 1024) <br>
     * 字节大小：1 <br>
     * 字节序数：9
     */
    pub tpdu_size: u8,

    /**
     * 参数代码SRC-TASP <br>
     * 字节大小：1 <br>
     * 字节序数：10
     */
    pub parameter_code_src_tsap: u8,

    /**
     * Source tsap byte length.
     * 参数长度 <br>
     * 字节大小：1 <br>
     * 字节序数：11
     */
    pub parameter_length2: u8,

    /**
     * SourceTSAP/Rack <br>
     * 字节大小：2 <br>
     * 字节序数：12-13
     */
    pub source_tsap: u16,

    /**
     * 参数代码DST-TASP <br>
     * 字节大小：1 <br>
     * 字节序数：14
     */
    pub parameter_code_dst_tsap: u8,

    /**
     * Destination tsap byte length.
     * 参数长度 <br>
     * 字节大小：1 <br>
     * 字节序数：15
     */
    pub parameter_length3: u8,

    /**
     * DestinationTSAP/Slot <br>
     * 字节大小：2 <br>
     * 字节序数：16-17
     */
    pub destination_tsap: u16,
}

impl CotpConnection {
    pub const BYTE_LENGTH: usize = 18;
}

impl CotpData {
    pub const BYTE_LENGTH: usize = 3;
}

impl Default for CotpConnection {
    fn default() -> CotpConnection {
        Self {
            length: 0x11,
            pdu_type: PduType::ConnectRequest,
            destination_reference: 0x0000,
            source_reference: 0x0001,
            flags: 0x00,
            parameter_code_tpdu_size: 0xC0,
            parameter_length1: 0x01,
            tpdu_size: 0x0A,
            parameter_code_src_tsap: 0xC1,
            parameter_length2: 0x02,
            source_tsap: 0x0100,
            parameter_code_dst_tsap: 0xC2,
            parameter_length3: 0x02,
            destination_tsap: 0x0100,
        }
    }
}

impl CotpConnection {
    pub fn create_cr_connection(local: u16, remote: u16) -> CotpConnection {
        Self {
            source_tsap: local,
            destination_tsap: remote,
            ..CotpConnection::default()
        }
    }
}

#[derive(Debug, Eq, PartialEq, Clone, Copy)]
pub struct CotpData {
    /**
     * Length, exclude this length field.
     * 长度（但并不包含length这个字段）<br>
     * 字节大小：1 <br>
     * 字节序数：0
     */
    pub length: u8,

    /**
     * PDU type.
     * PDU类型（CRConnect Request 连接请求）<br>
     * 字节大小：1 <br>
     * 字节序数：1
     */
    pub pdu_type: PduType,

    /**
     * TPDU number.
     * TPDU编号 <br>
     * 字节大小：1，后面7位 <br>
     * 字节序数：2
     */
    pub tpdu_number: u16,

    /**
     * Whether the last data unit.
     * 是否最后一个数据单元 <br>
     * 字节大小：1，最高位，7位 <br>
     * 字节序数：2
     */
    pub last_data_unit: bool,
}

impl Default for CotpData {
    fn default() -> Self {
        Self {
            length: 2,
            pdu_type: PduType::DtData,
            tpdu_number: 0,
            last_data_unit: true,
        }
    }
}

impl Cotp {

    pub fn pdu_type(&self) -> PduType {
        match self {
            Cotp::CotpConnection(c) => c.pdu_type,
            Cotp::CotpData(c) => c.pdu_type,
        }
    }

    pub fn byte_len(&self) -> usize {
        match self {
            Cotp::CotpConnection(_) => CotpConnection::BYTE_LENGTH,
            Cotp::CotpData(_) => CotpData::BYTE_LENGTH,
        }
    }

    pub fn to_be_bytes(&self) -> Vec<u8> {
        match self {
            Cotp::CotpConnection(conn) => {
                let mut buf = BytesMut::with_capacity(CotpConnection::BYTE_LENGTH);
                buf.put_u8(conn.length);
                buf.put_u8(conn.pdu_type.code());
                buf.put_u16(conn.destination_reference);
                buf.put_u16(conn.source_reference);
                buf.put_u8(conn.flags);
                buf.put_u8(conn.parameter_code_tpdu_size);
                buf.put_u8(conn.parameter_length1);
                buf.put_u8(conn.tpdu_size);
                buf.put_u8(conn.parameter_code_src_tsap);
                buf.put_u8(conn.parameter_length2);
                buf.put_u16(conn.source_tsap);
                buf.put_u8(conn.parameter_code_dst_tsap);
                buf.put_u8(conn.parameter_length3);
                buf.put_u16(conn.destination_tsap);
                buf.to_vec()
            }
            Cotp::CotpData(dt) => {
                let mut buf = BytesMut::with_capacity(CotpData::BYTE_LENGTH);
                buf.put_u8(dt.length);
                buf.put_u8(dt.pdu_type.code());
                let bit = if dt.last_data_unit {
                    (((0x00 & 0xFF) | (1 << 7)) & 0xFF) as u8
                } else {
                    ((0x00 & 0xFF) & !(1 << 7) & 0xFF) as u8
                };
                let b = bit | ((dt.tpdu_number & 0xFF) as u8);
                buf.put_u8(b);
                buf.to_vec()
            }
        }
    }

    pub fn from_be_bytes(bytes: &[u8]) -> Result<Option<Self>, S7Error> {
        // 至少需要一个字节确定帧长度
        if bytes.is_empty() {
            return Ok(None);
        }

        // 读取第一个字节（length），但不移动读指针
        let length = bytes[0] as usize;

        // 完整帧长度 = 1 (length字段) + length
        let frame_len = 1 + length;

        // 数据不足，等待更多字节
        if bytes.len() < frame_len {
            return Ok(None);
        }

        // 此时可以安全地获取第二个字节（pdu_type）
        let pdu_code = bytes[1];
        let pdu_type = PduType::from(pdu_code)
            .ok_or_else(|| S7Error::Error(format!("Unknown PDU type: 0x{:02X}", pdu_code)))?;

        let mut buf = BytesMut::from(bytes);
        // 根据 PDU 类型分发解析
        let cotp = match pdu_type {
            PduType::DtData => {
                // CotpData 固定长度：length 必须为 2，总长度 3
                if frame_len != 3 {
                    return Err(S7Error::Error(format!(
                        "Invalid CotpData frame length: expected 3, got {}",
                        frame_len
                    )));
                }
                // 直接读取三个字节并解析
                let len = buf.get_u8(); // length
                let _pdu = buf.get_u8(); // pdu_type，已校验
                let combined = buf.get_u8();
                let last_data_unit = (combined & 0x80) != 0;
                let tpdu_number = (combined & 0x7F) as u16;

                Cotp::CotpData(CotpData {
                    length: len,
                    pdu_type,
                    tpdu_number,
                    last_data_unit,
                })
            }

            PduType::Reject => return Ok(None),

            // 其他所有 PDU 类型都视为 CotpConnection（连接请求/确认/断开等）
            _ => {
                // CotpConnection 固定长度 18
                if frame_len != 18 {
                    return Err(S7Error::Error(format!(
                        "Invalid CotpConnection frame length: expected 18, got {}",
                        frame_len
                    )));
                }

                let length = buf.get_u8();
                let _pdu = buf.get_u8(); // pdu_type，已校验
                let destination_reference = buf.get_u16();
                let source_reference = buf.get_u16();
                let flags = buf.get_u8();
                let parameter_code_tpdu_size = buf.get_u8();
                let parameter_length1 = buf.get_u8();
                let tpdu_size = buf.get_u8();
                let parameter_code_src_tsap = buf.get_u8();
                let parameter_length2 = buf.get_u8();
                let source_tsap = buf.get_u16();
                let parameter_code_dst_tsap = buf.get_u8();
                let parameter_length3 = buf.get_u8();
                let destination_tsap = buf.get_u16();

                Cotp::CotpConnection(CotpConnection {
                    length,
                    pdu_type,
                    destination_reference,
                    source_reference,
                    flags,
                    parameter_code_tpdu_size,
                    parameter_length1,
                    tpdu_size,
                    parameter_code_src_tsap,
                    parameter_length2,
                    source_tsap,
                    parameter_code_dst_tsap,
                    parameter_length3,
                    destination_tsap,
                })
            }
        };

        Ok(Some(cotp))
    }
}
