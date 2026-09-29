//! Translation between Yaesu operating modes and Hamlib rigctld mode tokens.

use scu_cat::Mode;

/// Map a Yaesu mode to the Hamlib token rigctld clients expect.
pub fn mode_to_hamlib(mode: Mode) -> &'static str {
    match mode {
        Mode::Lsb => "LSB",
        Mode::Usb => "USB",
        Mode::CwU => "CW",
        Mode::CwL => "CW-R",
        Mode::Fm => "FM",
        Mode::Am => "AM",
        Mode::RttyL => "RTTY",
        Mode::RttyU => "RTTY-R",
        Mode::DataL => "PKTLSB",
        Mode::DataU => "PKTUSB",
        Mode::DataFm => "PKTFM",
        Mode::FmN => "FMN",
        Mode::AmN => "AMN",
        Mode::Psk => "PSK",
        Mode::DataFmN => "PKTFMN",
    }
}

/// Parse a Hamlib mode token into a Yaesu mode.
///
/// Accepts the common aliases (`CWR`, `RTTYR`) as well as the packet tokens
/// WSJT-X uses for the digital modes.
pub fn mode_from_hamlib(name: &str) -> Option<Mode> {
    Some(match name.trim().to_ascii_uppercase().as_str() {
        "LSB" => Mode::Lsb,
        "USB" => Mode::Usb,
        "CW" | "CWU" => Mode::CwU,
        "CW-R" | "CWR" => Mode::CwL,
        "FM" => Mode::Fm,
        "AM" => Mode::Am,
        "RTTY" | "RTTYL" => Mode::RttyL,
        "RTTY-R" | "RTTYR" => Mode::RttyU,
        "PKTLSB" | "DATA-L" | "DATA-LSB" => Mode::DataL,
        "PKTUSB" | "DATA-U" | "DATA-USB" => Mode::DataU,
        "PKTFM" | "DATA-FM" => Mode::DataFm,
        "FMN" | "FM-N" => Mode::FmN,
        "AMN" | "AM-N" => Mode::AmN,
        "PSK" | "PSK-U" => Mode::Psk,
        "PKTFMN" | "DATA-FM-N" => Mode::DataFmN,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_every_mode() {
        for mode in Mode::ALL {
            assert_eq!(mode_from_hamlib(mode_to_hamlib(mode)), Some(mode));
        }
    }

    #[test]
    fn packet_tokens_map_to_data_modes() {
        assert_eq!(mode_from_hamlib("PKTUSB"), Some(Mode::DataU));
        assert_eq!(mode_from_hamlib("PKTLSB"), Some(Mode::DataL));
        assert_eq!(mode_from_hamlib("PKTFM"), Some(Mode::DataFm));
        assert_eq!(mode_from_hamlib("FMN"), Some(Mode::FmN));
        assert_eq!(mode_from_hamlib("cwr"), Some(Mode::CwL));
        assert_eq!(mode_from_hamlib("nonsense"), None);
    }
}
