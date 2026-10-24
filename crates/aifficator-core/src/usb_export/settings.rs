//! Minimal display preferences for an exported USB, independently encoded from
//! the public DEVSETTING format. This does not change desktop preferences.

use super::{invalid, Result};

fn crc16(bytes: &[u8]) -> u16 {
    let mut crc = 0u16;
    for byte in bytes {
        crc ^= u16::from(*byte) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 == 0 {
                crc << 1
            } else {
                (crc << 1) ^ 0x1021
            };
        }
    }
    crc
}

pub fn rgb_device_settings() -> [u8; 140] {
    let mut out = [0u8; 140];
    out[0..4].copy_from_slice(&96u32.to_le_bytes());
    out[4..14].copy_from_slice(b"PIONEER DJ");
    // Header identifiers describe the file format, not the exporting software.
    out[36..45].copy_from_slice(b"rekordbox");
    out[68..73].copy_from_slice(b"7.0.0");
    out[100..104].copy_from_slice(&32u32.to_le_bytes());
    out[104..108].copy_from_slice(&0x12345678u32.to_le_bytes());
    out[108] = 1;
    out[112] = 1;
    out[113] = 1; // Half overview.
    out[114] = 3; // RGB; the MK2 does not support the 3Band display preference.
    out[115] = 1;
    out[116] = 1; // Classic key names.
    out[117] = 1; // Centered position marker.
    let crc = crc16(&out[104..136]);
    out[136..138].copy_from_slice(&crc.to_le_bytes());
    out
}

pub fn verify_rgb_device_settings(bytes: &[u8]) -> Result<()> {
    if bytes.len() != 140
        || bytes[0..4] != 96u32.to_le_bytes()
        || bytes[100..104] != 32u32.to_le_bytes()
        || bytes[114] != 3
        || bytes[136..138] != crc16(&bytes[104..136]).to_le_bytes()
    {
        return Err(invalid("invalid RGB device display settings"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn checksum_matches_xmodem_standard_and_detects_body_damage() {
        assert_eq!(super::crc16(b"123456789"), 0x31c3);
        let mut bytes = super::rgb_device_settings();
        super::verify_rgb_device_settings(&bytes).unwrap();
        bytes[117] ^= 1;
        assert!(super::verify_rgb_device_settings(&bytes).is_err());
    }
}
