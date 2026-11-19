use super::Credential;

const SERVICE: &str = "dev.caelum.desktop.boat";
const ACCOUNT: &str = "api-key";
const MISSING: &str = "Connect Boat with an API key in Settings → Remote machines.";

pub(super) async fn read() -> Result<Credential, String> {
    platform::read().await.and_then(|bytes| {
        String::from_utf8(bytes)
            .map(Credential::new)
            .map_err(|_| MISSING.into())
    })
}

pub(super) use platform::store;

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use security_framework::passwords::{
        delete_generic_password, get_generic_password, set_generic_password,
    };

    pub(super) async fn read() -> Result<Vec<u8>, String> {
        get_generic_password(SERVICE, ACCOUNT).map_err(|e| {
            if e.code() == -25300 {
                MISSING.to_owned()
            } else {
                format!("Could not read Boat key from Keychain: {e}")
            }
        })
    }
    pub(in crate::remote) async fn store(key: Option<&Credential>) -> Result<(), String> {
        match key {
            Some(key) => set_generic_password(SERVICE, ACCOUNT, key.0.as_bytes()),
            None => delete_generic_password(SERVICE, ACCOUNT).or_else(|e| {
                if e.code() == -25300 {
                    Ok(())
                } else {
                    Err(e)
                }
            }),
        }
        .map_err(|e| format!("Could not update Boat key in Keychain: {e}"))
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use secret_service::{EncryptionType, SecretService};
    use std::collections::HashMap;

    async fn service() -> Result<SecretService<'static>, String> {
        SecretService::connect(EncryptionType::Dh)
            .await
            .map_err(|e| format!("Could not open Secret Service: {e}"))
    }
    fn attributes() -> HashMap<&'static str, &'static str> {
        HashMap::from([("service", SERVICE), ("account", ACCOUNT)])
    }
    pub(super) async fn read() -> Result<Vec<u8>, String> {
        let service = service().await?;
        let items = service
            .search_items(attributes())
            .await
            .map_err(|e| e.to_string())?;
        let item = items
            .unlocked
            .first()
            .or(items.locked.first())
            .ok_or(MISSING)?;
        item.unlock().await.map_err(|e| e.to_string())?;
        item.get_secret().await.map_err(|e| e.to_string())
    }
    pub(in crate::remote) async fn store(key: Option<&Credential>) -> Result<(), String> {
        let service = service().await?;
        if let Some(key) = key {
            let collection = service
                .get_default_collection()
                .await
                .map_err(|e| e.to_string())?;
            collection.unlock().await.map_err(|e| e.to_string())?;
            collection
                .create_item(
                    "Caelum · Boat API key",
                    attributes(),
                    key.0.as_bytes(),
                    true,
                    "text/plain",
                )
                .await
                .map_err(|e| e.to_string())?;
        } else {
            let items = service
                .search_items(attributes())
                .await
                .map_err(|e| e.to_string())?;
            for item in items.unlocked.into_iter().chain(items.locked) {
                item.unlock().await.map_err(|e| e.to_string())?;
                item.delete().await.map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }
}
