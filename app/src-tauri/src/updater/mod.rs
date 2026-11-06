//! Manual-only bounded native updates. No startup requests or WebView transport.
mod policy;
pub mod service;
pub(crate) mod staging;
mod transport;
pub mod verification;

use minisign_verify::PublicKey;
use semver::Version;
use serde::Deserialize;
use std::{collections::BTreeMap, path::Path, process::Child};
use transport::{fetch, Https};

// Build-time coordinate; runtime input cannot replace it.
pub const REPOSITORY: &str = "daplabs/maxed";
pub const TARGET: &str = "windows-x86_64-nsis";
pub const FEED_LIMIT: usize = 65_536;
pub const ARTIFACT_LIMIT: usize = 20_000_000;
pub const NSIS_ARGS: [&str; 2] = ["/P", "/UPDATE"];

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error("Update build configuration is unresolved or invalid.")]
    Configuration,
    #[error("Another update installer owns staging.")]
    Busy,
    #[error("Update destination is not allowed.")]
    Url,
    #[error("Update redirect limit exceeded.")]
    Redirects,
    #[error("Update response size is invalid.")]
    Size,
    #[error("Update request failed.")]
    Network,
    #[error("Update deadline exceeded.")]
    Deadline,
    #[error("Update metadata is invalid or targets another platform.")]
    Manifest,
    #[error("Update does not support this platform.")]
    Target,
    #[error("No eligible update is available.")]
    Ineligible,
    #[error("Update offer is no longer current.")]
    Stale,
    #[error("Update requires durable shutdown before installation.")]
    Shutdown,
    #[error("Update installer could not be staged or launched.")]
    Install,
    #[error(transparent)]
    Verification(#[from] verification::VerificationError),
}
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Channel {
    #[default]
    Stable,
    Beta,
}

#[derive(Deserialize)]
struct Manifest {
    version: String,
    #[serde(default)]
    notes: String,
    platforms: BTreeMap<String, Platform>,
}

#[derive(Deserialize)]
struct Platform {
    url: String,
    signature: String,
}

// URLs, signatures and bytes never come from the WebView and are not exported.
struct Offer {
    version: Version,
    notes: String,
    platform: Platform,
    generation: u64,
}

fn parse_offer(
    bytes: &[u8],
    current: &Version,
    channel: Channel,
    generation: u64,
) -> Result<Offer> {
    if bytes.len() > FEED_LIMIT {
        return Err(Error::Size);
    }
    let manifest: Manifest = serde_json::from_slice(bytes).map_err(|_| Error::Manifest)?;
    if manifest.version.len() > 128 || manifest.notes.len() > 8192 {
        return Err(Error::Manifest);
    }
    let version = Version::parse(&manifest.version).map_err(|_| Error::Manifest)?;
    if version.to_string() != manifest.version {
        return Err(Error::Manifest);
    }
    if !version.cmp_precedence(current).is_gt()
        || (channel == Channel::Stable && !version.pre.is_empty())
    {
        return Err(Error::Ineligible);
    }
    let platform = manifest
        .platforms
        .into_iter()
        .find_map(|(target, platform)| (target == TARGET).then_some(platform))
        .ok_or(Error::Target)?;
    policy::initial(&platform.url, &policy::artifact(&version))?;
    if platform.signature.trim().is_empty() || platform.signature.len() > 8192 {
        return Err(verification::VerificationError::Signature.into());
    }
    Ok(Offer {
        version,
        notes: manifest.notes,
        platform,
        generation,
    })
}

/// One native owner holds at most one artifact. Futures borrow it exclusively;
/// cancel/drop an in-flight future before changing channel. No detached network tasks.
pub struct NativeUpdater {
    http: Https,
    key: PublicKey,
    current: Version,
    channel: Channel,
    generation: u64,
    offer: Option<Offer>,
    verified: Option<Vec<u8>>,
}

impl NativeUpdater {
    /// Fail closed before constructing a client if the compiled key is unresolved.
    /// Runtime environment variables cannot replace the compiled public key.
    pub fn production() -> Result<Self> {
        validate_coordinate(REPOSITORY)?;
        let config: serde_json::Value = serde_json::from_str(include_str!("../../tauri.conf.json"))
            .map_err(|_| Error::Manifest)?;
        let encoded = config
            .pointer("/plugins/updater/pubkey")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let key = verification::public_key(encoded)?;
        let current = Version::parse(env!("CARGO_PKG_VERSION")).map_err(|_| Error::Manifest)?;
        Ok(Self {
            http: Https::new()?,
            key,
            current,
            channel: Channel::Stable,
            generation: 0,
            offer: None,
            verified: None,
        })
    }

    pub fn set_channel(&mut self, channel: Channel) -> Result<()> {
        // Retire first, including on generation exhaustion.
        self.offer = None;
        self.verified = None;
        self.generation = self.generation.checked_add(1).ok_or(Error::Stale)?;
        self.channel = channel;
        Ok(())
    }

    pub fn offer_notes(&self) -> &str {
        self.offer.as_ref().map_or("", |offer| offer.notes.as_str())
    }

    /// Explicit manual request. Calling this never downloads or installs an artifact.
    pub async fn check(&mut self) -> Result<Option<String>> {
        self.offer = None;
        self.verified = None;
        let endpoint = policy::feed(self.channel);
        let bytes = fetch(
            &self.http,
            &endpoint,
            &endpoint,
            FEED_LIMIT,
            transport::METADATA,
            |_| {},
        )
        .await?;
        match parse_offer(&bytes, &self.current, self.channel, self.generation) {
            Ok(offer) => {
                let version = offer.version.to_string();
                self.offer = Some(offer);
                Ok(Some(version))
            }
            Err(Error::Ineligible) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Explicit download action. Failure/cancellation leaves no verified artifact.
    pub async fn download(&mut self, progress: impl FnMut(usize)) -> Result<()> {
        self.verified = None;
        let offer = self.offer.as_ref().ok_or(Error::Stale)?;
        if offer.generation != self.generation {
            return Err(Error::Stale);
        }
        let bytes = fetch(
            &self.http,
            &offer.platform.url,
            &policy::artifact(&offer.version),
            ARTIFACT_LIMIT,
            transport::DOWNLOAD,
            progress,
        )
        .await?;
        self.accept_artifact(bytes)
    }

    fn accept_artifact(&mut self, bytes: Vec<u8>) -> Result<()> {
        self.verified = None;
        let offer = self.offer.as_ref().ok_or(Error::Stale)?;
        if offer.generation != self.generation {
            return Err(Error::Stale);
        }
        if bytes.is_empty() || bytes.len() > ARTIFACT_LIMIT {
            return Err(Error::Size);
        }
        verification::verify(&bytes, &offer.platform.signature, &self.key, &offer.version)?;
        self.verified = Some(bytes);
        Ok(())
    }

    /// Explicit install action. The caller must keep the returned receipt alive and
    /// wait off the UI thread for cleanup. The child controls the NSIS handoff.
    /// The native lifecycle owner (never the WebView) supplies the durable gate.
    pub fn install(
        &mut self,
        app_data: &Path,
        durable_shutdown: impl FnOnce() -> Result<()>,
    ) -> Result<RunningInstaller> {
        self.install_with(app_data, durable_shutdown, RunningInstaller::launch)
    }

    pub(crate) fn install_with<T>(
        &mut self,
        app_data: &Path,
        durable_shutdown: impl FnOnce() -> Result<()>,
        launch: impl FnOnce((staging::Staged, std::path::PathBuf)) -> Result<T>,
    ) -> Result<T> {
        let offer = self.offer.as_ref().ok_or(Error::Stale)?;
        if offer.generation != self.generation || self.verified.is_none() {
            return Err(Error::Stale);
        }
        let namespace = staging::Namespace::acquire(app_data)?;
        let bytes = self.verified.take().ok_or(Error::Stale)?;
        let staged = namespace.stage(&bytes)?;
        if durable_shutdown().is_err() {
            self.verified = Some(bytes);
            return Err(Error::Shutdown);
        }
        // Read through the deny-write/delete handle after shutdown; keep that
        // handle and both directory guards alive across the launch handoff.
        drop(bytes);
        let offer = self.offer.take().ok_or(Error::Install)?;
        let checked = staged.0.read().map_err(|_| Error::Install)?;
        verification::verify(
            &checked,
            &offer.platform.signature,
            &self.key,
            &offer.version,
        )
        .map_err(|_| Error::Install)?;
        launch(staged).map_err(|_| Error::Install)
    }
}

fn validate_coordinate(value: &str) -> Result<()> {
    if resolved_coordinate(value) {
        Ok(())
    } else {
        Err(Error::Configuration)
    }
}

fn resolved_coordinate(value: &str) -> bool {
    let parts: Vec<_> = value.split('/').collect();
    parts.len() == 2
        && parts.iter().all(|part| {
            !part.is_empty()
                && !part.to_ascii_lowercase().contains("placeholder")
                && !part.to_ascii_lowercase().contains("unresolved")
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        })
}

/// RAII staging: private app-data directory and immutable image handle through launch.
/// NSIS arguments exactly match Tauri 2.13.1 passive + restart_after_install(false).
/// Safe Command::spawn replaces ShellExecuteW; no shell, elevation, /R or /ARGS.
#[must_use = "Wait for the installer off the UI thread to complete staging cleanup"]
pub struct RunningInstaller {
    child: Child,
    directory: staging::Staged,
}

impl RunningInstaller {
    pub(crate) fn launch((directory, path): (staging::Staged, std::path::PathBuf)) -> Result<Self> {
        let child = std::process::Command::new(&path)
            .args(NSIS_ARGS)
            .spawn()
            .map_err(|_| Error::Install)?;
        Ok(Self { child, directory })
    }

    /// Blocking: call from the future native update worker, never the UI thread.
    pub fn wait(mut self) -> Result<()> {
        let status = self.child.wait().map_err(|_| Error::Install)?;
        self.directory.close().map_err(|_| Error::Install)?;
        if status.success() {
            Ok(())
        } else {
            Err(Error::Install)
        }
    }
}

#[cfg(test)]
#[path = "tests.rs"]
pub(crate) mod tests;

#[cfg(test)]
#[path = "../../tests/support/update_staging.rs"]
mod staging_tests;
