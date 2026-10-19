use crate::{DeviceStatus, DeviceSummary, DisplayPalette, Error, Result};
pub const INFO: u8 = 1;
pub const CONNECT: u8 = 2;
pub const PING: u8 = 3;
pub const CLOSE: u8 = 4;
pub const IMAGE_BEGIN: u8 = 5;
pub const IMAGE_DATA: u8 = 6;
pub const IMAGE_STATUS: u8 = 7;
pub const AUDIO_BEGIN: u8 = 8;
pub const AUDIO_DATA: u8 = 9;
pub const AUDIO_END: u8 = 10;
pub const AUDIO_STOP: u8 = 11;
pub const STATE: u8 = 12;
pub const RESULT: u8 = 13;
pub const KEY: u8 = 14;
pub const STATUS: u8 = 15;
pub const PALETTE: u8 = 16;
#[derive(Clone, Debug)]
pub struct Packet {
    pub kind: u8,
    pub connection: u64,
    pub sequence: u32,
    pub body: Vec<u8>,
}
impl Packet {
    pub fn new(kind: u8, connection: u64, sequence: u32, body: Vec<u8>) -> Self {
        Self {
            kind,
            connection,
            sequence,
            body,
        }
    }
    pub fn encode(&self) -> Vec<u8> {
        let mut b = vec![b'Z', b'L', 1, self.kind];
        b.extend(self.connection.to_be_bytes());
        b.extend(self.sequence.to_be_bytes());
        b.extend(&self.body);
        b
    }
    pub fn decode(b: &[u8]) -> Option<Self> {
        if !(16..=1200).contains(&b.len()) || b[..3] != [b'Z', b'L', 1] {
            return None;
        }
        Some(Self::new(
            b[3],
            u64::from_be_bytes(b[4..12].try_into().ok()?),
            u32::from_be_bytes(b[12..16].try_into().ok()?),
            b[16..].to_vec(),
        ))
    }
}
pub fn u32_at(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(i..i + 4)?.try_into().ok()?))
}
pub fn code(b: &[u8]) -> Result<()> {
    match b.first() {
        Some(0) => Ok(()),
        Some(1) => Err(Error::Busy),
        Some(3) => Err(Error::Disconnected),
        Some(4) => Err(Error::DeviceFailure),
        Some(2) => Err(Error::InvalidOptions("设备拒绝了请求参数或图片格式".into())),
        Some(5) => Err(Error::ResultUnknown),
        _ => Err(Error::TransportFailure("响应格式错误".into())),
    }
}
pub fn status(p: &Packet) -> Result<DeviceStatus> {
    code(&p.body)?;
    let invalid = || Error::TransportFailure("设备状态响应无效".into());
    let [_, battery, charging, rssi] = p.body.as_slice() else {
        return Err(invalid());
    };
    let battery_percent = match *battery {
        0..=100 => Some(*battery),
        255 => None,
        _ => return Err(invalid()),
    };
    let charging = match *charging {
        0 => Some(false),
        1 => Some(true),
        255 => None,
        _ => return Err(invalid()),
    };
    let wifi_rssi_dbm = match *rssi as i8 {
        -127..=0 => Some(*rssi as i8),
        127 => None,
        _ => return Err(invalid()),
    };
    Ok(DeviceStatus {
        battery_percent,
        charging,
        wifi_rssi_dbm,
    })
}

pub fn palette(p: &Packet) -> Result<DisplayPalette> {
    code(&p.body)?;
    let invalid =
        || Error::TransportFailure("设备色板无效：需要 21 字节响应及非递减的 0～255 亮度".into());
    if p.body.len() != 21 {
        return Err(invalid());
    }
    let gray4: [u8; 4] = p.body[1..5].try_into().unwrap();
    let gray16: [u8; 16] = p.body[5..21].try_into().unwrap();
    let valid = |values: &[u8]| {
        values.first() == Some(&0)
            && values.last() == Some(&255)
            && values.windows(2).all(|pair| pair[0] <= pair[1])
    };
    if !valid(&gray4) || !valid(&gray16) {
        return Err(invalid());
    }
    Ok(DisplayPalette { gray4, gray16 })
}

pub fn info(p: &Packet, address: String) -> Result<DeviceSummary> {
    code(&p.body)?;
    let b = &p.body;
    let invalid = || Error::TransportFailure("设备信息无效".into());
    if p.connection == 0 || b.len() < 3 {
        return Err(invalid());
    }
    let n = b[2] as usize;
    let id = std::str::from_utf8(b.get(3..3 + n).ok_or_else(invalid)?).map_err(|_| invalid())?;
    let m = *b.get(3 + n).ok_or_else(invalid)? as usize;
    let name =
        std::str::from_utf8(b.get(4 + n..4 + n + m).ok_or_else(invalid)?).map_err(|_| invalid())?;
    let dims = b.get(4 + n + m..).ok_or_else(invalid)?;
    if dims != [1, 144, 1, 44, 0, 0, 0, 0, 0, 0, 0, 0] {
        return Err(Error::InvalidOptions(
            "需要支持 400×300 完整画布的固件，请配套升级固件与 SDK".into(),
        ));
    }
    Ok(DeviceSummary {
        id: id.into(),
        name: name.into(),
        address,
        busy: b[1] != 0,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn palette_layout_accepts_nonuniform_and_duplicate_levels_but_rejects_invalid_data() {
        let mut body = vec![0, 0, 40, 180, 255];
        body.extend([
            0, 9, 9, 30, 50, 70, 90, 100, 130, 150, 170, 190, 210, 225, 240, 255,
        ]);
        let decode = |body| palette(&Packet::new(PALETTE | 128, 1, 7, body));
        let result = decode(body.clone()).unwrap();
        assert_eq!(result.gray4, [0, 40, 180, 255]);
        assert_eq!(result.gray16[1], result.gray16[2]);
        for position in [1, 4, 5, 20] {
            let mut bad = body.clone();
            bad[position] ^= 1;
            assert!(decode(bad).is_err());
        }
        let mut bad = body.clone();
        bad[3] = 1;
        assert!(decode(bad).is_err());
        let mut bad = body.clone();
        bad[10] = 0;
        assert!(decode(bad).is_err());
        let mut bad = body.clone();
        bad.push(0);
        assert!(decode(bad).is_err());
        body.pop();
        assert!(decode(body).is_err());
        assert!(matches!(decode(vec![4]), Err(Error::DeviceFailure)));
    }
    #[test]
    fn full_canvas_info_rejects_legacy_reserved_strip() {
        let mut body = vec![0, 0, 12];
        body.extend(b"112233aabbcc");
        body.push(4);
        body.extend(b"test");
        let prefix = body.clone();
        body.extend([1, 144, 1, 44, 0, 0, 0, 0, 0, 0, 0, 0]);
        let packet = Packet::new(INFO | 128, 1, 1, body);
        assert_eq!(
            info(&packet, "127.0.0.1".into()).unwrap().id,
            "112233aabbcc"
        );
        let mut legacy = prefix;
        legacy.extend([1, 144, 1, 44, 0, 0, 0, 0, 1, 144, 0, 32]);
        assert!(matches!(
            info(&Packet::new(INFO | 128, 1, 1, legacy), "127.0.0.1".into()),
            Err(Error::InvalidOptions(_))
        ));
    }
    #[test]
    fn header_is_network_order_golden() {
        let p = Packet::new(14, 0x0102030405060708, 0x090a0b0c, vec![3, 1]);
        assert_eq!(
            p.encode(),
            vec![0x5a, 0x4c, 1, 14, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 3, 1]
        );
    }
    #[test]
    fn malformed_datagrams_are_rejected() {
        for n in 0..16 {
            assert!(Packet::decode(&vec![0; n]).is_none())
        }
        let mut p = Packet::new(1, 0, 1, vec![]).encode();
        p[2] = 2;
        assert!(Packet::decode(&p).is_none());
        assert!(Packet::decode(&vec![0; 1201]).is_none());
    }
    #[test]
    fn status_values_preserve_zero_false_and_signed_rssi() {
        let decode = |body| status(&Packet::new(STATUS | 128, 1, 1, body));
        assert_eq!(
            decode(vec![0, 0, 0, 129]).unwrap(),
            DeviceStatus {
                battery_percent: Some(0),
                charging: Some(false),
                wifi_rssi_dbm: Some(-127),
            }
        );
        assert_eq!(
            decode(vec![0, 100, 1, 0]).unwrap(),
            DeviceStatus {
                battery_percent: Some(100),
                charging: Some(true),
                wifi_rssi_dbm: Some(0),
            }
        );
        assert_eq!(
            decode(vec![0, 255, 255, 127]).unwrap(),
            DeviceStatus::default()
        );
        for body in [
            vec![],
            vec![0],
            vec![0, 1, 0],
            vec![0, 1, 0, 195, 0],
            vec![0, 101, 0, 195],
            vec![0, 50, 2, 195],
            vec![0, 50, 0, 1],
            vec![0, 50, 0, 128],
        ] {
            assert!(decode(body).is_err());
        }
    }
}
