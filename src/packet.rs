use crate::cotp::Cotp::CotpData;
use crate::cotp::{Cotp, CotpConnection};
use crate::datum::{Datum, ReadWriteDatum};
use crate::error::S7Error;
use crate::header::S7Header;
use crate::item::{DataItem, RequestItem};
use crate::paramter::{ReadWriteParameter, S7Parameter, SetupComParameter};
use crate::tpkt::TPKT;
use crate::types::S7FunctionCode;

#[derive(Debug, Clone)]
pub struct S7Data {
    /// tpkt  4 字节
    pub tpkt: TPKT,

    /// cotp  18 字节
    pub cotp: Option<Cotp>,

    /// header  10 ~ 12  字节
    pub header: Option<S7Header>,

    /// 参数分为：
    /// * Setup 通讯参数  8 字节
    /// * 读/写 参数      4 字节
    /// * 控制PLC  n 字节
    /// * 停止PLC  n 字节
    pub parameter: Option<S7Parameter>,

    /// 这里的数据有两种，数据项DataItem和返回项ReturnItem，两种都可以重复
    ///
    /// * 数据项（DataItem）占多个字节
    /// * 返回项（ReturnItem）只有一个字节；
    pub datum: Option<Datum>,
}

impl S7Data {
    pub fn byte_len(&self) -> usize {
        let mut len = 0usize;
        len += self.tpkt.byte_len();
        if let Some(ref cotp) = self.cotp {
            len += cotp.byte_len();
        }
        if let Some(ref header) = self.header {
            len += header.byte_len();
        }
        if let Some(ref parameter) = self.parameter {
            len += parameter.byte_len();
        }
        if let Some(ref datum) = self.datum {
            len += datum.byte_len();
        }
        len
    }

    pub fn to_be_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.byte_len());
        bytes.extend_from_slice(&*self.tpkt.to_be_bytes());
        if let Some(ref cotp) = self.cotp {
            bytes.extend_from_slice(&cotp.to_be_bytes());
        }
        if let Some(ref header) = self.header {
            bytes.extend_from_slice(&header.to_be_bytes());
        }
        if let Some(ref parameter) = self.parameter {
            bytes.extend_from_slice(&parameter.to_be_bytes());
        }
        if let Some(ref datum) = self.datum {
            bytes.extend_from_slice(&datum.to_be_bytes());
        }
        bytes
    }

    pub fn from_be_bytes(bytes: &[u8]) -> Result<Option<Self>, S7Error> {
        let tpkt_bytes = &bytes[0..TPKT::BYTE_LEN];
        let tpkt = TPKT::from_be_bytes(tpkt_bytes)?;
        let remain_bytes = &bytes[TPKT::BYTE_LEN..];
        let cotp = Cotp::from_be_bytes(remain_bytes)?;
        let mut s7data = S7Data {
            tpkt,
            cotp: None,
            header: None,
            parameter: None,
            datum: None,
        };
        s7data.cotp = cotp;
        if cotp.is_none() || remain_bytes.len() <= cotp.unwrap().byte_len() {
            return Ok(Some(s7data));
        }

        let remain_bytes = &remain_bytes[cotp.unwrap().byte_len()..];
        let header = S7Header::from_be_bytes(remain_bytes)?;
        s7data.header = header;
        if header.is_none() {
            return Ok(Some(s7data));
        }

        let header_len = header.unwrap().byte_len();
        let h_param_len = header.unwrap().paremater_len() as usize;
        let h_data_len = header.unwrap().data_len() as usize;
        if h_param_len > 0 {
            let param_bytes = &remain_bytes[header_len..header_len + h_param_len];
            let p = S7Parameter::from_be_bytes(param_bytes)?;
            s7data.parameter = p;
        }

        if h_data_len > 0 {
            let data_bytes =
                &remain_bytes[header_len + h_param_len..header_len + h_param_len + h_data_len];
            let datum = Datum::from_be_bytes(
                data_bytes,
                header.unwrap().message_type(),
                s7data.clone().parameter.unwrap().function_code(),
            )?;
            s7data.datum = Some(datum);
        }
        Ok(Some(s7data))
    }

    pub fn create_cr_connection(local: u16, remote: u16) -> Self {
        let mut s7data = Self {
            tpkt: Default::default(),
            cotp: Some(Cotp::CotpConnection(CotpConnection::create_cr_connection(
                local, remote,
            ))),
            header: None,
            parameter: None,
            datum: None,
        };
        s7data.self_check();
        s7data
    }

    pub fn create_setup_comm(pdu_len: u16) -> Self {
        let mut s7data = Self {
            tpkt: Default::default(),
            cotp: Some(CotpData(crate::cotp::CotpData::default())),
            header: Some(S7Header::default()),
            parameter: Some(S7Parameter::SetupParameter(SetupComParameter::new(pdu_len))),
            datum: None,
        };
        s7data.self_check();
        s7data
    }

    pub fn create_read_request(request_items: &mut Vec<RequestItem>) -> Self {
        let mut s7data = Self {
            tpkt: Default::default(),
            cotp: Some(CotpData(crate::cotp::CotpData::default())),
            header: Some(S7Header::default()),
            parameter: Some(S7Parameter::ReadWriteParameter(ReadWriteParameter::new(
                S7FunctionCode::ReadVariable,
                request_items,
            ))),
            datum: None,
        };
        s7data.self_check();
        s7data
    }

    pub fn create_write_request(
        request_items: &mut Vec<RequestItem>,
        data_item: Vec<DataItem>,
    ) -> Self {
        let mut s7data = Self {
            tpkt: Default::default(),
            cotp: Some(CotpData(crate::cotp::CotpData::default())),
            header: Some(S7Header::default()),
            parameter: Some(S7Parameter::ReadWriteParameter(ReadWriteParameter::new(
                S7FunctionCode::WriteVariable,
                request_items,
            ))),
            datum: Some(Datum::ReadWrite(ReadWriteDatum::new(data_item))),
        };
        s7data.self_check();
        s7data
    }

    fn self_check(&mut self) {
        if let Some(ref mut h) = self.header {
            h.set_data_len(0);
            h.set_parameter_len(0);

            if let Some(ref parameter) = self.parameter {
                h.set_parameter_len(parameter.byte_len() as u16);
            }

            if let Some(ref datum) = self.datum {
                h.set_data_len(datum.byte_len() as u16);
            }
        }
        self.tpkt.length = self.byte_len() as u16;
    }
}
