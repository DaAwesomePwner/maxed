use super::*;
use bytes::Bytes;
use std::{
    cell::RefCell, collections::VecDeque, path::PathBuf, process::Command, sync::OnceLock,
    time::Duration,
};
use transport::{Body, Response, Transport};

struct SigningKey {
    directory: tempfile::TempDir,
    secret: String,
    public: String,
}

fn cli() -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../ui/node_modules/@tauri-apps/cli/tauri.js");
    assert!(
        path.is_file(),
        "Updater signer tests require Node and the locked Tauri CLI. Run npm ci --prefix app/ui from the repository root before cargo test."
    );
    path
}

impl SigningKey {
    fn generate() -> Self {
        let target = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target");
        std::fs::create_dir_all(&target).unwrap();
        let directory = tempfile::Builder::new()
            .prefix("p7-updater-key-")
            .tempdir_in(target)
            .unwrap();
        let path = directory.path().join("disposable.key");
        let output = Command::new("node")
            .arg(cli())
            .args([
                "signer",
                "generate",
                "--ci",
                "--password",
                "p7-disposable",
                "--write-keys",
            ])
            .arg(&path)
            .env_remove("TAURI_SIGNING_PRIVATE_KEY")
            .env_remove("TAURI_SIGNING_PRIVATE_KEY_PASSWORD")
            .env_remove("TAURI_SIGNING_PRIVATE_KEY_PATH")
            .output()
            .expect("Updater signer tests require Node on PATH; restore the CLI with npm ci --prefix app/ui before cargo test.");
        // Never print signer stdout/stderr, even on failure.
        assert!(
            output.status.success(),
            "Disposable Tauri key generation failed"
        );
        let secret = std::fs::read_to_string(&path).unwrap();
        let public = std::fs::read_to_string(path.with_extension("key.pub")).unwrap();
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_file(path.with_extension("key.pub")).unwrap();
        Self {
            directory,
            secret,
            public,
        }
    }

    fn sign(&self, bytes: &[u8], version: Option<&str>) -> String {
        let file = tempfile::Builder::new()
            .prefix("invented-")
            .tempfile_in(self.directory.path())
            .unwrap();
        std::fs::write(file.path(), bytes).unwrap();
        let mut command = Command::new("node");
        command
            .arg(cli())
            .args(["signer", "sign"])
            .arg(file.path())
            .env("TAURI_SIGNING_PRIVATE_KEY", &self.secret)
            .env("TAURI_SIGNING_PRIVATE_KEY_PASSWORD", "p7-disposable")
            .env_remove("TAURI_SIGNING_PRIVATE_KEY_PATH");
        if let Some(version) = version {
            command.args(["--app-version", version]);
        }
        let output = command.output().expect("Updater signer tests require Node on PATH; restore the CLI with npm ci --prefix app/ui before cargo test.");
        assert!(output.status.success(), "Disposable Tauri signing failed");
        let signature_path = PathBuf::from(format!("{}.sig", file.path().display()));
        let signature = std::fs::read_to_string(&signature_path).unwrap();
        std::fs::remove_file(signature_path).unwrap();
        signature
    }
}

fn key() -> &'static SigningKey {
    static KEY: OnceLock<SigningKey> = OnceLock::new();
    KEY.get_or_init(SigningKey::generate)
}

const INVENTED: &[u8] = b"MZ invented non-executable S2 updater bytes; never launch";

pub(crate) fn signed_reply() -> (Vec<u8>, Vec<u8>) {
    (
        manifest("2.0.0", TARGET, &key().sign(INVENTED, Some("2.0.0"))),
        INVENTED.to_vec(),
    )
}

/// Shared by setup/service gate tests. Real verification of invented bytes;
/// callers must replace process launch and never execute this artifact.
pub(crate) fn verified_owner() -> NativeUpdater {
    let signature = key().sign(INVENTED, Some("2.0.0"));
    let mut native = owner("2.0.0", TARGET, &signature, Channel::Stable).unwrap();
    native.accept_artifact(INVENTED.to_vec()).unwrap();
    native
}
pub(crate) fn changed_verified_owner() -> NativeUpdater {
    let mut native = verified_owner();
    native.verified.as_mut().unwrap()[4] ^= 1;
    native
}

const DELIVERY: &str = "https://release-assets.githubusercontent.com/github-production-release-asset/123456/12345678-abcd-4321-9876-123456789abc";

fn manifest(version: &str, target: &str, signature: &str) -> Vec<u8> {
    // Keep invalid-version cases syntactically valid JSON with an otherwise plausible path.
    let artifact = format!("https://github.com/{REPOSITORY}/releases/download/v{version}/Maxed_{version}_x64-setup.exe");
    serde_json::to_vec(&serde_json::json!({"version": version, "platforms": {
        target: {"url": artifact, "signature": signature}
    }}))
    .unwrap()
}

fn owner(version: &str, target: &str, signature: &str, channel: Channel) -> Result<NativeUpdater> {
    let current = Version::parse("1.0.0").unwrap();
    let offer = parse_offer(&manifest(version, target, signature), &current, channel, 0)?;
    Ok(NativeUpdater {
        // No socket is used in owner/verifier tests. An empty trust store makes
        // the otherwise-real client independent of the sandbox's Windows profile;
        // it cannot validate any remote certificate, so this is not a TLS bypass.
        http: Https::from_roots(rustls::RootCertStore::empty())?,
        key: verification::public_key(&key().public)?,
        current,
        channel,
        generation: 0,
        offer: Some(offer),
        verified: None,
    })
}

#[test]
fn update_signature_matrix() {
    let good = key().sign(INVENTED, Some("1.1.0"));
    let wrong_key = SigningKey::generate().sign(INVENTED, Some("1.1.0"));
    let unsigned_version = key().sign(INVENTED, None);
    let wrong_version = key().sign(INVENTED, Some("1.0.1"));
    let mut changed = INVENTED.to_vec();
    changed[4] ^= 1;
    let mut launches = 0;
    for (bytes, signature, version, target, expected) in [
        (INVENTED, good.as_str(), "1.1.0", TARGET, true),
        (changed.as_slice(), good.as_str(), "1.1.0", TARGET, false),
        (&INVENTED[..10], good.as_str(), "1.1.0", TARGET, false),
        (INVENTED, "", "1.1.0", TARGET, false),
        (INVENTED, wrong_key.as_str(), "1.1.0", TARGET, false),
        (
            INVENTED,
            good.as_str(),
            "1.1.0",
            "windows-aarch64-nsis",
            false,
        ),
        (INVENTED, good.as_str(), "banana", TARGET, false),
        (INVENTED, good.as_str(), "v1.1.0", TARGET, false),
        (INVENTED, good.as_str(), "0.9.0", TARGET, false),
        (INVENTED, good.as_str(), "1.0.0", TARGET, false),
        (INVENTED, good.as_str(), "1.0.0+metadata", TARGET, false),
        (INVENTED, unsigned_version.as_str(), "1.1.0", TARGET, false),
        (INVENTED, wrong_version.as_str(), "1.1.0", TARGET, false),
    ] {
        let result = owner(version, target, signature, Channel::Stable).and_then(|mut updater| {
            updater.accept_artifact(bytes.to_vec())?;
            updater.install_with(
                tempfile::tempdir().unwrap().path(),
                || Ok(()),
                |_| {
                    launches += 1;
                    Ok(())
                },
            )
        });
        assert_eq!(result.is_ok(), expected, "case {version}/{target}");
    }
    assert_eq!(launches, 1);
}

#[test]
fn trusted_comment_tampering_is_rejected_before_reading_version() {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let signed = key().sign(INVENTED, Some("1.1.0"));
    let text = String::from_utf8(STANDARD.decode(signed.trim()).unwrap()).unwrap();
    let forged = STANDARD.encode(text.replace("version:1.1.0", "version:1.9.0"));
    let mut updater = owner("1.9.0", TARGET, &forged, Channel::Stable).unwrap();
    assert_eq!(
        updater.accept_artifact(INVENTED.to_vec()),
        Err(Error::Verification(
            verification::VerificationError::Signature
        ))
    );
    assert!(updater
        .install_with::<()>(
            tempfile::tempdir().unwrap().path(),
            || Ok(()),
            |_| panic!("unverified launch")
        )
        .is_err());
}

#[test]
fn staged_bytes_are_reverified_before_launch() {
    let mut updater = verified_owner();
    updater.verified.as_mut().unwrap()[4] ^= 1;
    let mut stopped = false;
    let result = updater.install_with::<()>(
        tempfile::tempdir().unwrap().path(),
        || {
            stopped = true;
            Ok(())
        },
        |_| panic!("changed bytes reached launch"),
    );
    assert!(stopped);
    assert_eq!(result, Err(Error::Install));
    assert!(updater.offer.is_none());
}

#[test]
fn stable_beta_and_no_downgrade_use_semver_precedence() {
    for (current, offered, channel, accepted) in [
        ("1.0.0", "1.1.0-beta.1", Channel::Stable, false),
        ("1.0.0", "1.1.0-beta.1", Channel::Beta, true),
        ("1.1.0-beta.1", "1.1.0-beta.2", Channel::Beta, true),
        ("1.1.0-beta.1", "1.0.1", Channel::Stable, false),
        ("1.1.0-beta.1", "1.1.0", Channel::Stable, true),
        ("1.1.0-beta.1", "1.1.0", Channel::Beta, true),
        ("1.1.0", "1.1.0+new", Channel::Beta, false),
        ("1.1.0", "1.1.0-beta.9", Channel::Beta, false),
    ] {
        assert_eq!(
            parse_offer(
                &manifest(offered, TARGET, "placeholder"),
                &Version::parse(current).unwrap(),
                channel,
                0
            )
            .is_ok(),
            accepted
        );
    }
}

#[test]
fn stale_offer_cannot_install() {
    let signed = key().sign(INVENTED, Some("1.1.0"));
    let mut updater = owner("1.1.0", TARGET, &signed, Channel::Stable).unwrap();
    updater.accept_artifact(INVENTED.to_vec()).unwrap();
    assert_eq!(
        updater.install_with::<()>(
            tempfile::tempdir().unwrap().path(),
            || Err(Error::Shutdown),
            |_| panic!("shutdown bypass")
        ),
        Err(Error::Shutdown)
    );
    updater.set_channel(Channel::Beta).unwrap();
    assert!(updater.offer.is_none() && updater.verified.is_none());
    assert_eq!(
        updater.install_with::<()>(
            tempfile::tempdir().unwrap().path(),
            || Ok(()),
            |_| panic!("stale launch")
        ),
        Err(Error::Stale)
    );
    updater.set_channel(Channel::Stable).unwrap();
    assert_eq!(
        updater.install_with::<()>(
            tempfile::tempdir().unwrap().path(),
            || Ok(()),
            |_| panic!("returning to stable revived an old offer")
        ),
        Err(Error::Stale)
    );
    assert_eq!(NSIS_ARGS, ["/P", "/UPDATE"]);
}

#[test]
fn configured_production_key_is_accepted_without_network() {
    let config: serde_json::Value =
        serde_json::from_str(include_str!("../../tauri.conf.json")).unwrap();
    assert_eq!(
        config.pointer("/plugins/updater/pubkey").and_then(|v| v.as_str()),
        Some("dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEJGMTU3MEMwRTlBQUJGMUIKUldRYnY2cnB3SEFWdjRSdzY5M1JTZGxIelVjU3FneHN6K1hLRXo0RHV0aElMQzRvMjl1YktFem4K")
    );
    for value in ["", "UNRESOLVED", "not base64", "dGVzdA=="] {
        assert!(verification::public_key(value).is_err());
    }
    // Construction validates the compiled key and creates a client only; no
    // update check or download is invoked, so this test performs no network I/O.
    let _updater = NativeUpdater::production().expect("configured production key must be accepted");
}

#[test]
fn verified_non_mz_artifact_never_calls_shutdown() {
    let bytes = b"signed but not an NSIS executable";
    let signature = key().sign(bytes, Some("1.1.0"));
    let mut updater = owner("1.1.0", TARGET, &signature, Channel::Stable).unwrap();
    updater.accept_artifact(bytes.to_vec()).unwrap();
    let mut shutdown_called = false;
    let result = updater.install(tempfile::tempdir().unwrap().path(), || {
        shutdown_called = true;
        Ok(())
    });
    assert!(matches!(result, Err(Error::Install)));
    assert!(
        !shutdown_called,
        "staging failure must precede durable shutdown"
    );
}

#[test]
fn staged_installer_is_cleaned_when_spawn_fails_after_shutdown() {
    let signature = key().sign(INVENTED, Some("1.1.0"));
    let mut updater = owner("1.1.0", TARGET, &signature, Channel::Stable).unwrap();
    updater.accept_artifact(INVENTED.to_vec()).unwrap();
    let shutdown_called = std::cell::Cell::new(false);
    let mut staged_path = None;
    let result = updater.install_with::<()>(
        tempfile::tempdir().unwrap().path(),
        || {
            shutdown_called.set(true);
            Ok(())
        },
        |(directory, path)| {
            assert!(shutdown_called.get());
            assert_eq!(std::fs::read(&path).unwrap(), INVENTED);
            staged_path = Some(directory.path().to_owned());
            Err(Error::Install)
        },
    );
    assert_eq!(result, Err(Error::Install));
    assert!(!staged_path.unwrap().exists());
}

#[test]
fn raw_nsis_staging_is_closed_and_cleaned_without_executing_invented_bytes() {
    assert!(matches!(
        staging::Namespace::acquire(tempfile::tempdir().unwrap().path())
            .unwrap()
            .stage(b"not an exe"),
        Err(Error::Install)
    ));
    let root = tempfile::tempdir().unwrap();
    let (directory, path) = staging::Namespace::acquire(root.path())
        .unwrap()
        .stage(INVENTED)
        .unwrap();
    assert_eq!(path.extension().unwrap(), "exe");
    assert_eq!(std::fs::read(&path).unwrap(), INVENTED);
    let parent = directory.path().to_owned();
    // A staging or launch failure drops this same directory guard. No executable is run.
    drop(directory);
    assert!(!parent.exists() && !path.exists());
}

struct FakeBody {
    chunks: VecDeque<Bytes>,
    delay: Duration,
}
impl Body for FakeBody {
    async fn chunk(&mut self) -> Result<Option<Bytes>> {
        if self.chunks.is_empty() {
            return Ok(None);
        }
        tokio::time::sleep(self.delay).await;
        Ok(self.chunks.pop_front())
    }
}
#[derive(Default)]
struct Fake {
    responses: RefCell<VecDeque<Response<FakeBody>>>,
    requests: RefCell<Vec<String>>,
    delay: Duration,
}
impl Transport for Fake {
    type Body = FakeBody;
    async fn get(&self, url: reqwest::Url) -> Result<Response<Self::Body>> {
        self.requests.borrow_mut().push(url.to_string());
        tokio::time::sleep(self.delay).await;
        self.responses
            .borrow_mut()
            .pop_front()
            .ok_or(Error::Network)
    }
}
fn response(
    status: u16,
    location: Option<String>,
    length: Option<u64>,
    chunks: Vec<Bytes>,
) -> Response<FakeBody> {
    Response {
        status,
        location,
        length,
        body: FakeBody {
            chunks: chunks.into(),
            delay: Duration::ZERO,
        },
    }
}
fn fake(responses: Vec<Response<FakeBody>>) -> Fake {
    Fake {
        responses: RefCell::new(responses.into()),
        ..Default::default()
    }
}
async fn get(fake: &Fake, limit: usize) -> Result<Vec<u8>> {
    let url = policy::feed(Channel::Stable);
    fetch(fake, &url, &url, limit, transport::METADATA, |_| {}).await
}

#[test]
fn every_redirect_is_validated() {
    run(async {
        let good = format!("{DELIVERY}?secret=never-log");
        let bad = [
        "http://release-assets.githubusercontent.com/github-production-release-asset/123456/12345678-abcd-4321-9876-123456789abc".to_owned(),
        good.replace(".com/", ".com:443/"), good.replace("https://", "https://user@"),
        format!("{good}#fragment"), good.replace("release-assets", "objects"),
        good.replace(".com/", ".com.evil/"), good.replace("/123456/", "/../"),
        good.replace("/123456/", "/%31%32%33/"), good.replace("12345678-abcd", "not-a-uuid"),
        good.replace("github-production-release-asset", "other"),
        policy::feed(Channel::Stable), "file:///tmp/update".into(), "/relative".into(),
    ];
        for url in bad {
            let transport = fake(vec![response(302, Some(url), None, vec![])]);
            let error = get(&transport, 10).await.unwrap_err();
            assert_eq!(error, Error::Url);
            assert!(!format!("{error:?}").contains("secret"));
            assert_eq!(transport.requests.borrow().len(), 1);
        }
        let transport = fake(vec![
            response(302, Some(good.clone()), None, vec![]),
            response(307, Some(format!("{DELIVERY}?new-query")), None, vec![]),
            response(200, None, None, vec![Bytes::from_static(b"ok")]),
        ]);
        assert_eq!(get(&transport, 2).await.unwrap(), b"ok");
        let transport = fake(vec![
            response(302, Some(good.clone()), None, vec![]),
            response(302, Some(good.replace("/123456/", "/98765/")), None, vec![]),
        ]);
        assert_eq!(get(&transport, 10).await, Err(Error::Url));
        assert_eq!(transport.requests.borrow().len(), 2);
    });
}

#[test]
fn initial_feed_and_artifact_paths_are_exact_before_any_io() {
    run(async {
        let expected = policy::feed(Channel::Stable);
        for bad in [
            expected.replace("DaAwesomePwner", "other"),
            expected.replace("DaAwesomePwner/maxed", "other-owner/maxed"),
            expected.replace("DaAwesomePwner/maxed", "DaAwesomePwner/maxed-rotmg"),
            expected.replace("stable", "beta"),
            format!("{expected}?x"),
            expected.replace(".com/", ".com:443/"),
        ] {
            let transport = Fake::default();
            assert_eq!(
                fetch(
                    &transport,
                    &bad,
                    &expected,
                    FEED_LIMIT,
                    transport::METADATA,
                    |_| {}
                )
                .await,
                Err(Error::Url)
            );
            assert!(transport.requests.borrow().is_empty());
        }
        let mut json: serde_json::Value =
            serde_json::from_slice(&manifest("1.1.0", TARGET, "sig")).unwrap();
        for bad in ["https://github.com/other-owner/maxed/releases/download/v1.1.0/Maxed_1.1.0_x64-setup.exe", "https://github.com/other/repo/releases/download/v1.1.0/Maxed_1.1.0_x64-setup.exe", "https://github.com/DaAwesomePwner/maxed/releases/download/update-stable/Maxed_1.1.0_x64-setup.exe", "https://github.com/DaAwesomePwner/maxed-rotmg/releases/download/v1.1.0/Maxed_1.1.0_x64-setup.exe"] {
        json["platforms"][TARGET]["url"] = bad.into();
        assert!(matches!(parse_offer(&serde_json::to_vec(&json).unwrap(), &Version::parse("1.0.0").unwrap(), Channel::Stable, 0), Err(Error::Url)));
    }
    });
}

#[test]
fn five_redirects_allowed_sixth_is_not_requested() {
    run(async {
        for (count, succeeds) in [(5, true), (6, false)] {
            let mut responses = Vec::new();
            for _ in 0..count {
                responses.push(response(302, Some(DELIVERY.into()), None, vec![]));
            }
            responses.push(response(
                200,
                None,
                Some(2),
                vec![Bytes::from_static(b"ok")],
            ));
            let transport = fake(responses);
            assert_eq!(get(&transport, 2).await.is_ok(), succeeds);
            assert_eq!(transport.requests.borrow().len(), 6);
        }
    });
}

#[test]
fn metadata_and_download_bounds() {
    run(async {
        for limit in [FEED_LIMIT, ARTIFACT_LIMIT] {
            let mut oversized = response(
                200,
                None,
                Some(limit as u64 + 1),
                vec![Bytes::from_static(b"x")],
            );
            // If the downloader polls this body instead of rejecting the header,
            // it times out: this proves early rejection, not just the final count.
            oversized.body.delay = Duration::from_secs(40);
            let transport = fake(vec![oversized]);
            assert_eq!(get(&transport, limit).await, Err(Error::Size));
            let transport = fake(vec![response(
                200,
                None,
                None,
                vec![Bytes::from(vec![1; limit]), Bytes::from_static(b"!")],
            )]);
            assert_eq!(get(&transport, limit).await, Err(Error::Size));
            let transport = fake(vec![response(
                200,
                None,
                None,
                vec![Bytes::from(vec![1; limit])],
            )]);
            assert_eq!(get(&transport, limit).await.unwrap().len(), limit);
        }
        let transport = fake(vec![response(
            200,
            None,
            Some(3),
            vec![Bytes::from_static(b"ab")],
        )]);
        assert_eq!(get(&transport, 10).await, Err(Error::Size));
    });
}

#[test]
fn total_deadlines_include_all_redirects_and_slow_body_and_progress_is_throttled() {
    run(async {
        let mut transport = fake(vec![
            response(302, Some(DELIVERY.into()), None, vec![]),
            response(200, None, None, vec![]),
        ]);
        transport.delay = Duration::from_secs(16);
        assert_eq!(get(&transport, 10).await, Err(Error::Deadline));
        let mut slow = response(
            200,
            None,
            None,
            vec![Bytes::from_static(b"a"), Bytes::from_static(b"b")],
        );
        slow.body.delay = Duration::from_secs(151);
        let transport = fake(vec![slow]);
        let url = policy::feed(Channel::Beta);
        assert_eq!(
            fetch(&transport, &url, &url, 10, transport::DOWNLOAD, |_| {}).await,
            Err(Error::Deadline)
        );
        let mut fast = response(200, None, None, vec![Bytes::from_static(b"x"); 50]);
        fast.body.delay = Duration::from_millis(20);
        let transport = fake(vec![fast]);
        let mut times = Vec::new();
        fetch(&transport, &url, &url, 100, transport::METADATA, |_| {
            times.push(tokio::time::Instant::now())
        })
        .await
        .unwrap();
        assert!(!times.is_empty() && times.len() <= 4);
        assert!(times
            .windows(2)
            .all(|pair| pair[1] - pair[0] >= Duration::from_millis(250)));
        assert_eq!(transport::CONNECT, Duration::from_secs(10));
    });
}

#[test]
fn cancellation_drops_work_and_failures_have_no_http_fallback() {
    run(async {
        let mut transport = Fake {
            delay: Duration::from_secs(100),
            ..Default::default()
        };
        let task = get(&transport, 10);
        assert!(tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .is_err());
        tokio::time::sleep(Duration::from_secs(200)).await;
        assert_eq!(transport.requests.borrow().len(), 1);
        transport.delay = Duration::ZERO;
        // Empty fake produces a transport error, analogous to certificate validation failure.
        assert_eq!(get(&transport, 10).await, Err(Error::Network));
        assert!(transport
            .requests
            .borrow()
            .iter()
            .all(|url| url.starts_with("https://")));
    });
}

fn run(future: impl std::future::Future<Output = ()>) {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(future);
}
