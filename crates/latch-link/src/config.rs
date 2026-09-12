use std::{
    env,
    path::{Path, PathBuf},
};

use url::Url;

use crate::LinkError;

const ROUTER_URL_ENV: &str = "LATCH_ROUTER_URL";
const PAIRING_TOKEN_ENV: &str = "LATCH_PAIRING_TOKEN";
const DEVICE_NAME_ENV: &str = "LATCH_DEVICE_NAME";
const DEVICE_ID_PATH_ENV: &str = "LATCH_DEVICE_ID_PATH";

pub struct LinkConfig {
    router_url: Url,
    pairing_token: String,
    device_name: String,
    device_id_path: Option<PathBuf>,
}

impl LinkConfig {
    pub fn from_env() -> Result<Self, LinkError> {
        let router_url = required_env(ROUTER_URL_ENV)?;
        let pairing_token = required_env(PAIRING_TOKEN_ENV)?;
        let device_name = required_env(DEVICE_NAME_ENV)?;
        let device_id_path = env::var_os(DEVICE_ID_PATH_ENV).map(PathBuf::from);
        Self::new(&router_url, pairing_token, device_name, device_id_path)
    }

    pub fn new(
        router_url: &str,
        pairing_token: String,
        device_name: String,
        device_id_path: Option<PathBuf>,
    ) -> Result<Self, LinkError> {
        if pairing_token.trim().is_empty() {
            return Err(LinkError::EmptyEnvironment {
                name: PAIRING_TOKEN_ENV,
            });
        }

        let device_name = device_name.trim().to_owned();
        if device_name.is_empty() {
            return Err(LinkError::EmptyEnvironment {
                name: DEVICE_NAME_ENV,
            });
        }
        if device_name.chars().count() > 128 {
            return Err(LinkError::DeviceNameTooLong);
        }

        Ok(Self {
            router_url: normalize_router_url(router_url)?,
            pairing_token,
            device_name,
            device_id_path,
        })
    }

    pub fn router_url(&self) -> &Url {
        &self.router_url
    }

    pub fn pairing_token(&self) -> &str {
        &self.pairing_token
    }

    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    pub fn device_id_path(&self) -> Option<&Path> {
        self.device_id_path.as_deref()
    }
}

fn required_env(name: &'static str) -> Result<String, LinkError> {
    let value = env::var(name).map_err(|_| LinkError::MissingEnvironment { name })?;
    if value.trim().is_empty() {
        return Err(LinkError::EmptyEnvironment { name });
    }
    Ok(value)
}

fn normalize_router_url(input: &str) -> Result<Url, LinkError> {
    let mut url = Url::parse(input)?;
    let target_scheme = match url.scheme() {
        "https" => "wss",
        "http" => "ws",
        "wss" => "wss",
        "ws" => "ws",
        _ => return Err(LinkError::UnsupportedRouterScheme),
    };
    url.set_scheme(target_scheme)
        .map_err(|()| LinkError::UnsupportedRouterScheme)?;

    if url.path().is_empty() || url.path() == "/" {
        url.set_path("/api/link");
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn https_router_url_becomes_secure_websocket_url() {
        let config = LinkConfig::new(
            "https://router.example.com",
            "secret".to_owned(),
            "laptop".to_owned(),
            None,
        )
        .unwrap();

        assert_eq!(
            config.router_url().as_str(),
            "wss://router.example.com/api/link"
        );
    }

    #[test]
    fn empty_pairing_token_is_rejected() {
        let error = LinkConfig::new(
            "https://router.example.com",
            "   ".to_owned(),
            "laptop".to_owned(),
            None,
        )
        .unwrap_err();

        assert!(matches!(
            error,
            LinkError::EmptyEnvironment {
                name: PAIRING_TOKEN_ENV
            }
        ));
    }

    #[test]
    fn explicit_link_path_is_preserved() {
        let config = LinkConfig::new(
            "ws://127.0.0.1:3000/api/link",
            "secret".to_owned(),
            "laptop".to_owned(),
            None,
        )
        .unwrap();
        assert_eq!(config.router_url().as_str(), "ws://127.0.0.1:3000/api/link");
    }
}
