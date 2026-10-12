//! Explicit runtime input encoding, independent of persisted OpenCollection messages.
use base64::{Engine as _, engine::general_purpose::STANDARD};

use crate::SessionData;

/// How a caller supplies one runtime WebSocket message.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum WebSocketInputFormat {
    #[default]
    Text,
    /// Standard Base64 input, decoded to binary bytes before sending.
    Binary,
}

impl WebSocketInputFormat {
    /// Convert literal input without variable interpolation or hidden mode changes.
    pub fn decode(self, input: &str) -> Result<SessionData, base64::DecodeError> {
        match self {
            Self::Text => Ok(SessionData::Text(input.to_owned())),
            Self::Binary => STANDARD.decode(input).map(SessionData::Binary),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_literal_and_is_the_default() {
        for text in ["", "{{token}}\n/text", "AAEC", "--help"] {
            assert_eq!(
                WebSocketInputFormat::default().decode(text).unwrap(),
                SessionData::Text(text.into())
            );
        }
    }

    #[test]
    fn binary_decodes_standard_base64_exactly() {
        for (input, bytes) in [
            ("", vec![]),
            ("AAEC", vec![0, 1, 2]),
            ("/w==", vec![255]),
            ("+/8=", vec![251, 255]),
        ] {
            assert_eq!(
                WebSocketInputFormat::Binary.decode(input).unwrap(),
                SessionData::Binary(bytes)
            );
        }
    }

    #[test]
    fn binary_rejects_invalid_nonstandard_or_noncanonical_input() {
        for input in ["!", "_w==", "-w==", "/w", "AAEC\n", "AA EC", "/x=="] {
            assert!(WebSocketInputFormat::Binary.decode(input).is_err());
        }
    }
}
