use super::{policy, Error, Result};
use bytes::Bytes;
use reqwest::{header, Client, Url};
use std::time::Duration;
use tokio::time::{timeout, Instant};

pub(super) const CONNECT: Duration = Duration::from_secs(10);
pub(super) const METADATA: Duration = Duration::from_secs(30);
pub(super) const DOWNLOAD: Duration = Duration::from_secs(300);
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);

// Internal injectable seam: tests exercise the actual redirect/size/deadline loop
// without a socket, TLS bypass, localhost allowance or production fake backend.
pub(super) trait Body {
    async fn chunk(&mut self) -> Result<Option<Bytes>>;
}

pub(super) struct Response<B> {
    pub status: u16,
    pub location: Option<String>,
    pub length: Option<u64>,
    pub body: B,
}

pub(super) trait Transport {
    type Body: Body;
    async fn get(&self, url: Url) -> Result<Response<Self::Body>>;
}

pub(super) struct Https {
    client: Client,
    #[cfg(test)]
    pub(super) script: Option<std::sync::Arc<scripted::Script>>,
}
#[cfg(test)]
#[path = "../../tests/support/update_transport.rs"]
pub(super) mod scripted;
#[cfg(test)]
use scripted::HttpsBody;
#[cfg(not(test))]
type HttpsBody = reqwest::Response;

impl Https {
    pub fn new() -> Result<Self> {
        // Load local trust roots, then validate entirely inside rustls. The default
        // Windows platform verifier can fetch CRL/AIA URLs outside our origin policy.
        let native = rustls_native_certs::load_native_certs();
        if !native.errors.is_empty() {
            return Err(Error::Network);
        }
        let mut roots = rustls::RootCertStore::empty();
        let (added, _) = roots.add_parsable_certificates(native.certs);
        if added == 0 {
            return Err(Error::Network);
        }
        Self::from_roots(roots)
    }

    pub(super) fn from_roots(roots: rustls::RootCertStore) -> Result<Self> {
        let tls = rustls::ClientConfig::builder_with_provider(std::sync::Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(|_| Error::Network)?
        .with_root_certificates(roots)
        .with_no_client_auth();
        let client = Client::builder()
            .tls_backend_preconfigured(tls)
            .https_only(true)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(CONNECT)
            .timeout(DOWNLOAD)
            .referer(false)
            .connection_verbose(false)
            .user_agent("Maxed-Updater")
            .build()
            .map_err(|_| Error::Network)?;
        Ok(Self {
            client,
            #[cfg(test)]
            script: None,
        })
    }
}

impl Body for reqwest::Response {
    async fn chunk(&mut self) -> Result<Option<Bytes>> {
        self.chunk().await.map_err(|_| Error::Network)
    }
}

impl Transport for Https {
    type Body = HttpsBody;
    async fn get(&self, url: Url) -> Result<Response<Self::Body>> {
        #[cfg(test)]
        if let Some(script) = &self.script {
            return script.get().await;
        }
        let response = self
            .client
            .get(url)
            .header(header::ACCEPT_ENCODING, "identity")
            .send()
            .await
            .map_err(|_| Error::Network)?;
        // No compressed body expansion and no URL-bearing reqwest errors in IPC/logs.
        if response
            .headers()
            .get(header::CONTENT_ENCODING)
            .is_some_and(|v| v != "identity")
        {
            return Err(Error::Network);
        }
        let location = response
            .headers()
            .get(header::LOCATION)
            .map(|v| v.to_str().map(str::to_owned).map_err(|_| Error::Url))
            .transpose()?;
        let length = response
            .headers()
            .get(header::CONTENT_LENGTH)
            .map(|v| {
                v.to_str()
                    .ok()
                    .and_then(|v| v.parse::<u64>().ok())
                    .ok_or(Error::Size)
            })
            .transpose()?;
        let status = response.status().as_u16();
        #[cfg(test)]
        let response = response.into();
        Ok(Response {
            status,
            location,
            length,
            body: response,
        })
    }
}

pub(super) async fn fetch<T: Transport>(
    transport: &T,
    initial: &str,
    expected: &str,
    limit: usize,
    deadline: Duration,
    mut progress: impl FnMut(usize),
) -> Result<Vec<u8>> {
    // One total deadline wraps DNS, connection, every redirect, headers and body.
    timeout(deadline, async {
        let mut url = policy::initial(initial, expected)?;
        let mut delivery_path = None;
        let mut redirects = 0;
        loop {
            let mut response = transport.get(url).await?;
            if [301, 302, 303, 307, 308].contains(&response.status) {
                if redirects == 5 {
                    return Err(Error::Redirects);
                }
                url = policy::redirect(
                    response.location.as_deref().ok_or(Error::Url)?,
                    delivery_path.as_deref(),
                )?;
                delivery_path.get_or_insert_with(|| url.path().to_owned());
                redirects += 1;
                continue;
            }
            if response.status != 200 {
                return Err(Error::Network);
            }
            if response.length.is_some_and(|len| len > limit as u64) {
                return Err(Error::Size);
            }
            // Reserve the fixed bound once: no geometric growth, duplicate artifact
            // or realloc-copy peak. A request owns its sole buffer until verified.
            let mut bytes = Vec::new();
            bytes.try_reserve_exact(limit).map_err(|_| Error::Size)?;
            let mut last_progress = Instant::now();
            while let Some(chunk) = response.body.chunk().await? {
                if chunk.len() > limit - bytes.len() {
                    return Err(Error::Size);
                }
                bytes.extend_from_slice(&chunk);
                if last_progress.elapsed() >= PROGRESS_INTERVAL {
                    progress(bytes.len());
                    last_progress = Instant::now();
                }
            }
            if response.length.is_some_and(|len| len != bytes.len() as u64) {
                return Err(Error::Size);
            }
            // Completion is a state transition, not an extra progress event.
            return Ok(bytes);
        }
    })
    .await
    .map_err(|_| Error::Deadline)?
}
