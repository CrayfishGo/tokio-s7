use crate::error::S7Error;
use tokio_util::bytes::{Buf, BufMut, BytesMut};

#[derive(Debug, Eq, PartialEq, Clone, Copy)]
pub struct TPKT {
    /**
     * 版本号，常量0x03 <br>
     * 字节大小：1 <br>
     * 字节序数：0
     */
    pub version: u8,

    /**
     * 预留，默认值0x00 <br>
     * 字节大小：1 <br>
     * 字节序数：1
     */
    pub reserved: u8,

    /**
     * 长度，包括后面负载payload+版本号+预留+长度 <br>
     * 字节大小：2 <br>
     * 字节序数：2-3
     */
    pub length: u16,
}

impl Default for TPKT {
    fn default() -> Self {
        Self {
            version: 0x03,
            reserved: 0x00,
            length: 0x0000,
        }
    }
}

impl TPKT {
    pub const BYTE_LEN: usize = 4;
    pub fn byte_len(&self) -> usize {
        Self::BYTE_LEN
    }

    pub fn to_be_bytes(&self) -> Vec<u8> {
        let mut buf = BytesMut::with_capacity(Self::BYTE_LEN);
        buf.put_u8(self.version);
        buf.put_u8(self.reserved);
        buf.put_u16(self.length);
        buf.to_vec()
    }

    pub fn from_be_bytes(data: &[u8]) -> Result<Self, S7Error> {
        if data.len() < 4 {
            return Err(S7Error::Error(format!("Invalid TPKT len: {}", data.len())));
        }

        let mut buf = BytesMut::from(data);
        let version = buf.get_u8();
        let reserved = buf.get_u8();
        let length = buf.get_u16();

        Ok(TPKT {
            version,
            reserved,
            length,
        })
    }
}
