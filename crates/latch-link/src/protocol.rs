use latch_core::DeviceId;
use latch_protocol::{RequestEnvelope, ResponseEnvelope};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    Hello {
        device_id: DeviceId,
        device_name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        pairing_token: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        device_credential: Option<String>,
    },
    Response {
        request_id: String,
        response: ResponseEnvelope,
    },
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Welcome {
        device_id: DeviceId,
    },
    Request {
        request_id: String,
        request: RequestEnvelope,
    },
    Error {
        code: String,
        message: String,
    },
}

#[cfg(test)]
mod tests {
    use latch_core::DeviceId;
    use serde_json::json;

    use super::*;

    #[test]
    fn authentication_handshake_serializes_with_expected_shape() {
        let device_id = DeviceId::new();
        let message = ClientMessage::Hello {
            device_id,
            device_name: "amaan-laptop".to_owned(),
            pairing_token: Some("test-secret".to_owned()),
            device_credential: None,
        };

        let value = serde_json::to_value(message).unwrap();
        assert_eq!(
            value,
            json!({
                "type": "hello",
                "device_id": device_id,
                "device_name": "amaan-laptop",
                "pairing_token": "test-secret"
            })
        );
    }
}
