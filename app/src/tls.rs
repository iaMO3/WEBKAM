use anyhow::{Context, Result};
use axum_server::tls_rustls::RustlsConfig;
use base64::{Engine, engine::general_purpose::STANDARD};
use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose,
};
use serde::{Deserialize, Serialize};
use std::{fs, net::Ipv4Addr, path::Path};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

const CERTIFICATE_VERSION: u8 = 2;
const CERTIFICATE_NAME: &str = "PocketCam Camera";

#[derive(Deserialize, Serialize)]
pub struct Certificates {
    pub label: String,
    authority: String,
    leaf: String,
    leaf_key: String,
    profile_id: String,
    payload_id: String,
    expires: i64,
    #[serde(default)]
    version: u8,
}

impl Certificates {
    pub fn load(directory: &Path, ip: Ipv4Addr) -> Result<Self> {
        fs::create_dir_all(directory)?;
        let file = directory.join(format!("pairing-{ip}.json"));
        let now = OffsetDateTime::now_utc();
        let saved = fs::read(&file)
            .ok()
            .and_then(|contents| serde_json::from_slice::<Self>(&contents).ok());
        let existing_ids = match saved {
            Some(saved)
                if saved.version == CERTIFICATE_VERSION
                    && saved.expires > (now + Duration::days(1)).unix_timestamp() =>
            {
                return Ok(saved);
            }
            Some(saved) => Uuid::parse_str(&saved.profile_id)
                .ok()
                .zip(Uuid::parse_str(&saved.payload_id).ok()),
            None => None,
        };

        let mut authority = CertificateParams::default();
        authority
            .distinguished_name
            .push(DnType::CommonName, CERTIFICATE_NAME);
        authority.not_before = now - Duration::days(1);
        authority.not_after = now + Duration::days(397);
        authority.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
        authority.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        let authority_key = KeyPair::generate()?;
        let root = authority.self_signed(&authority_key)?;
        let issuer = Issuer::new(authority, authority_key);

        let mut leaf = CertificateParams::new(vec![ip.to_string()])?;
        leaf.distinguished_name
            .push(DnType::CommonName, ip.to_string());
        leaf.not_before = now - Duration::days(1);
        leaf.not_after = now + Duration::days(365);
        leaf.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        leaf.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        leaf.use_authority_key_identifier_extension = true;
        let expires = leaf.not_after.unix_timestamp();
        let key = KeyPair::generate()?;
        let (profile_id, payload_id) =
            existing_ids.unwrap_or_else(|| (Uuid::new_v4(), Uuid::new_v4()));
        let data = Self {
            label: CERTIFICATE_NAME.into(),
            authority: root.pem(),
            leaf: leaf.signed_by(&key, &issuer)?.pem(),
            leaf_key: key.serialize_pem(),
            expires,
            profile_id: profile_id.to_string(),
            payload_id: payload_id.to_string(),
            version: CERTIFICATE_VERSION,
        };
        // The CA signing key is dropped. Only the IP-specific server key is saved.
        let temp = directory.join(format!("{}.tmp", data.profile_id));
        fs::write(&temp, serde_json::to_vec(&data)?)?;
        fs::rename(&temp, &file)?;
        Ok(data)
    }

    pub async fn config(&self) -> Result<RustlsConfig> {
        RustlsConfig::from_pem(
            self.leaf.as_bytes().to_vec(),
            self.leaf_key.as_bytes().to_vec(),
        )
        .await
        .context("Can't load the PocketCam TLS certificate")
    }

    pub fn profile(&self, setup_origin: &str) -> Result<String> {
        let cert = STANDARD.encode(self.der()?);
        let icon = STANDARD.encode(crate::web::TOUCH_ICON);
        let Self {
            label,
            profile_id,
            payload_id,
            ..
        } = self;
        // Keep the Home Screen payload identifier stable when a profile is downloaded again.
        let web_clip_id = Uuid::from_u128(Uuid::parse_str(profile_id)?.as_u128() ^ 1);
        Ok(format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>PayloadType</key><string>Configuration</string>
<key>PayloadVersion</key><integer>1</integer>
<key>PayloadIdentifier</key><string>local.pocketcam.{profile_id}</string>
<key>PayloadUUID</key><string>{profile_id}</string>
<key>PayloadDisplayName</key><string>PocketCam</string>
<key>PayloadOrganization</key><string>PocketCam</string>
<key>PayloadDescription</key><string>Adds PocketCam to your Home Screen and installs the certificate for this PC.</string>
<key>PayloadContent</key><array><dict>
<key>PayloadType</key><string>com.apple.security.root</string>
<key>PayloadVersion</key><integer>1</integer>
<key>PayloadIdentifier</key><string>local.pocketcam.{profile_id}.certificate</string>
<key>PayloadUUID</key><string>{payload_id}</string>
<key>PayloadDisplayName</key><string>{label}</string>
<key>PayloadCertificateFileName</key><string>PocketCam.cer</string>
<key>PayloadContent</key><data>{cert}</data>
</dict><dict>
<key>PayloadType</key><string>com.apple.webClip.managed</string>
<key>PayloadVersion</key><integer>1</integer>
<key>PayloadIdentifier</key><string>local.pocketcam.{profile_id}.webclip</string>
<key>PayloadUUID</key><string>{web_clip_id}</string>
<key>PayloadDisplayName</key><string>PocketCam Home Screen App</string>
<key>Label</key><string>PocketCam</string>
<key>URL</key><string>{setup_origin}/iphone</string>
<key>FullScreen</key><true/>
<key>IgnoreManifestScope</key><true/>
<key>IsRemovable</key><true/>
<key>Precomposed</key><true/>
<key>Icon</key><data>{icon}</data>
</dict></array>
</dict></plist>"#
        ))
    }

    pub fn der(&self) -> Result<Vec<u8>> {
        Ok(pem::parse(&self.authority)?.into_contents())
    }
}
