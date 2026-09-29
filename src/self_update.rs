//! Shared self-update support for ORES Rust command-line tools.
//!
//! The updater intentionally runs before a product CLI's normal parser. That keeps
//! `self-update` consistent across clap, flags2env, and bespoke argument parsers.

use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::env;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs::{self, File};
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

const GITHUB_API: &str = "https://api.github.com";

const MAX_RELEASE_JSON_BYTES: u64 = 1024 * 1024;
const MAX_CHECKSUM_BYTES: u64 = 256 * 1024;
const MAX_RELEASE_ASSET_BYTES: u64 = 256 * 1024 * 1024;
const MAX_ARCHIVE_ENTRIES: usize = 4096;
const MAX_ARCHIVE_ENTRY_BYTES: u64 = 128 * 1024 * 1024;
const MAX_ARCHIVE_TOTAL_BYTES: u64 = 256 * 1024 * 1024;
const MAX_ARCHIVE_PATH_DEPTH: usize = 32;

/// Integrity policy for downloaded release assets.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum VerificationPolicy {
    /// Require a SHA-256 sidecar/manifest before candidate execution.
    RequireSha256,
    /// Permit a missing checksum asset while still verifying one when present.
    PreferSha256,
}

/// Product-specific metadata needed by the shared updater.
#[derive(Clone, Debug)]
pub struct SelfUpdateConfig {
    /// GitHub repository owner, for example `ORESoftware`.
    pub repo_owner: &'static str,
    /// GitHub repository name, for example `ores-compose`.
    pub repo_name: &'static str,
    /// Installed binary name, without `.exe`.
    pub binary_name: &'static str,
    /// Current semantic version, normally `env!("CARGO_PKG_VERSION")`.
    pub current_version: &'static str,
    /// Integrity policy applied before any downloaded candidate is executed.
    pub verification_policy: VerificationPolicy,
    /// Whether this product explicitly permits the dangerous `--no-verify` override.
    pub allow_no_verify: bool,
}

impl SelfUpdateConfig {
    /// Construct updater metadata for one CLI binary.
    #[must_use]
    pub const fn new(
        repo_owner: &'static str,
        repo_name: &'static str,
        binary_name: &'static str,
        current_version: &'static str,
    ) -> Self {
        return Self {
            repo_owner,
            repo_name,
            binary_name,
            current_version,
            verification_policy: VerificationPolicy::RequireSha256,
            allow_no_verify: false,
        };
    }

    /// Override the checksum policy for a product with a documented compatibility need.
    #[must_use]
    pub const fn with_verification_policy(mut self, policy: VerificationPolicy) -> Self {
        self.verification_policy = policy;
        self
    }

    /// Explicitly permit `--no-verify` for a product. Production CLIs should normally leave this false.
    #[must_use]
    pub const fn allow_unverified_updates(mut self, allow: bool) -> Self {
        self.allow_no_verify = allow;
        self
    }

    fn repository(&self) -> String {
        return format!("{}/{}", self.repo_owner, self.repo_name);
    }
}

/// Result of a completed update command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SelfUpdateOutcome {
    /// The installed binary already matches the requested version.
    AlreadyCurrent { version: String },
    /// `--check` completed without modifying the executable.
    CheckOnly { current: String, target: String },
    /// `--dry-run` completed without modifying the executable.
    DryRun { current: String, target: String },
    /// The executable was replaced and validated.
    Updated { previous: String, current: String },
}

/// Errors produced by the shared updater.
#[derive(Debug)]
pub struct SelfUpdateError {
    message: String,
}

impl SelfUpdateError {
    fn new(message: impl Into<String>) -> Self {
        return Self {
            message: message.into(),
        };
    }
}

impl Display for SelfUpdateError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        return formatter.write_str(&self.message);
    }
}

impl Error for SelfUpdateError {}

impl From<io::Error> for SelfUpdateError {
    fn from(value: io::Error) -> Self {
        return Self::new(value.to_string());
    }
}

#[derive(Debug, Default)]
struct CliArgs {
    version: Option<String>,
    interactive: Option<bool>,
    assume_yes: bool,
    check: bool,
    dry_run: bool,
    verify_required: bool,
    no_verify: bool,
    json: bool,
    help: bool,
}

#[derive(Clone, Debug, Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<ReleaseAsset>,
}

#[derive(Clone, Debug, Deserialize)]
struct ReleaseAsset {
    name: String,
    url: String,
}

/// Return true when argv requests the reserved `self-update` subcommand.
#[must_use]
pub fn self_update_requested() -> bool {
    return env::args().nth(1).as_deref() == Some("self-update");
}

/// Run the self-update CLI and terminate the process with an appropriate code.
///
/// Call this before a product CLI's normal parser:
///
/// ```no_run
/// use ores_clis_core::self_update::{SelfUpdateConfig, run_self_update_cli, self_update_requested};
///
/// if self_update_requested() {
///     run_self_update_cli(SelfUpdateConfig::new(
///         "ORESoftware",
///         "example-cli",
///         "example",
///         env!("CARGO_PKG_VERSION"),
///     ));
/// }
/// ```
pub fn run_self_update_cli(config: SelfUpdateConfig) -> ! {
    let args = match parse_cli_args(env::args().skip(2)) {
        Ok(args) => args,
        Err(error) => {
            eprintln!("self-update: {error}");
            std::process::exit(2);
        }
    };

    if args.help {
        print_help(config.binary_name);
        std::process::exit(0);
    }

    match execute(config, &args) {
        Ok(outcome) => {
            emit_outcome(&outcome, args.json);
            std::process::exit(0);
        }
        Err(error) => {
            if args.json {
                let body = serde_json::json!({
                    "ok": false,
                    "command": "self-update",
                    "error": error.to_string(),
                });
                eprintln!("{body}");
            } else {
                eprintln!("self-update: {error}");
            }
            std::process::exit(1);
        }
    }
}

fn parse_cli_args<I>(args: I) -> Result<CliArgs, SelfUpdateError>
where
    I: IntoIterator<Item = String>,
{
    let mut parsed = CliArgs::default();

    for arg in args {
        match arg.as_str() {
            "--interactive" => set_interactive(&mut parsed, true)?,
            "--non-interactive" => set_interactive(&mut parsed, false)?,
            "--yes" | "-y" => parsed.assume_yes = true,
            "--check" => parsed.check = true,
            "--dry-run" => parsed.dry_run = true,
            "--verify" => parsed.verify_required = true,
            "--no-verify" => parsed.no_verify = true,
            "--json" => parsed.json = true,
            "--help" | "-h" => parsed.help = true,
            value if value.starts_with('-') => {
                return Err(SelfUpdateError::new(format!("unknown option: {value}")));
            }
            value => {
                if parsed.version.is_some() {
                    return Err(SelfUpdateError::new(
                        "self-update accepts at most one VERSION argument",
                    ));
                }
                parsed.version = Some(value.to_owned());
            }
        }
    }

    if parsed.verify_required && parsed.no_verify {
        return Err(SelfUpdateError::new(
            "--verify and --no-verify cannot be used together",
        ));
    }

    if parsed.assume_yes && parsed.interactive == Some(true) {
        return Err(SelfUpdateError::new(
            "--yes cannot be combined with --interactive",
        ));
    }

    return Ok(parsed);
}

fn set_interactive(args: &mut CliArgs, value: bool) -> Result<(), SelfUpdateError> {
    if let Some(existing) = args.interactive {
        if existing != value {
            return Err(SelfUpdateError::new(
                "--interactive and --non-interactive cannot be used together",
            ));
        }
    }
    args.interactive = Some(value);
    return Ok(());
}

fn execute(config: SelfUpdateConfig, args: &CliArgs) -> Result<SelfUpdateOutcome, SelfUpdateError> {
    let release = resolve_release(&config, args.version.as_deref())?;
    let target = normalize_version(&release.tag_name);
    let current = normalize_version(config.current_version);

    if versions_equal(&current, &target) {
        return Ok(SelfUpdateOutcome::AlreadyCurrent { version: target });
    }

    if args.check {
        return Ok(SelfUpdateOutcome::CheckOnly { current, target });
    }

    let asset = select_asset(&config, &release)?;
    let interactive = args.interactive.unwrap_or_else(|| {
        return !args.assume_yes && io::stdin().is_terminal() && io::stdout().is_terminal();
    });

    if interactive {
        print_plan(&config, &current, &target, &asset.name);
        if !confirm()? {
            return Err(SelfUpdateError::new("update cancelled"));
        }
    }

    if args.dry_run {
        return Ok(SelfUpdateOutcome::DryRun { current, target });
    }

    if args.no_verify && !config.allow_no_verify {
        return Err(SelfUpdateError::new(
            "--no-verify is disabled by this product's self-update policy",
        ));
    }

    let temp = tempfile::Builder::new()
        .prefix("ores-self-update-")
        .tempdir()
        .map_err(|error| SelfUpdateError::new(format!("create update tempdir: {error}")))?;
    let downloaded = temp.path().join(&asset.name);
    download_asset(&asset.url, &downloaded)?;

    if !args.no_verify {
        let required = args.verify_required
            || matches!(
                config.verification_policy,
                VerificationPolicy::RequireSha256
            );
        verify_checksum(&release, &asset, &downloaded, required)?;
    }

    let candidate = prepare_candidate(&config, &downloaded, temp.path())?;
    validate_candidate(&candidate, &target)?;

    let current_exe = env::current_exe()
        .map_err(|error| SelfUpdateError::new(format!("locate current executable: {error}")))?;
    let backup = temp
        .path()
        .join(format!("{}.backup", executable_name(config.binary_name)));
    fs::copy(&current_exe, &backup)
        .map_err(|error| SelfUpdateError::new(format!("backup current executable: {error}")))?;

    self_replace::self_replace(&candidate)
        .map_err(|error| SelfUpdateError::new(format!("replace executable: {error}")))?;

    if let Err(validation_error) = validate_candidate(&current_exe, &target) {
        let rollback_result = self_replace::self_replace(&backup);
        return match rollback_result {
            Ok(()) => Err(SelfUpdateError::new(format!(
                "new executable failed validation and was rolled back: {validation_error}"
            ))),
            Err(rollback_error) => Err(SelfUpdateError::new(format!(
                "new executable failed validation ({validation_error}); rollback also failed: {rollback_error}"
            ))),
        };
    }

    return Ok(SelfUpdateOutcome::Updated {
        previous: current,
        current: target,
    });
}

#[derive(Debug)]
enum ReleaseLookupError {
    NotFound,
    Other(SelfUpdateError),
}

fn resolve_release(
    config: &SelfUpdateConfig,
    requested: Option<&str>,
) -> Result<Release, SelfUpdateError> {
    let repository = config.repository();
    if requested.is_none() || requested == Some("latest") {
        let url = format!("{GITHUB_API}/repos/{repository}/releases/latest");
        return get_release(&url).map_err(|error| match error {
            ReleaseLookupError::NotFound => {
                SelfUpdateError::new(format!("latest release does not exist for {repository}"))
            }
            ReleaseLookupError::Other(error) => error,
        });
    }

    let version = requested.unwrap_or_default();
    let normalized = normalize_version(version);
    let normalized_tag = format!("v{normalized}");
    let with_v = format!("{GITHUB_API}/repos/{repository}/releases/tags/{normalized_tag}");
    match get_release(&with_v) {
        Ok(release) => return Ok(release),
        Err(ReleaseLookupError::NotFound) => {}
        Err(ReleaseLookupError::Other(error)) => return Err(error),
    }

    let raw_tag = version.to_owned();
    let raw = format!("{GITHUB_API}/repos/{repository}/releases/tags/{raw_tag}");
    return match get_release(&raw) {
        Ok(release) => Ok(release),
        Err(ReleaseLookupError::NotFound) => Err(SelfUpdateError::new(format!(
            "release {normalized} does not exist for {repository} (tried {normalized_tag} and {raw_tag})"
        ))),
        Err(ReleaseLookupError::Other(error)) => Err(error),
    };
}

fn get_release(url: &str) -> Result<Release, ReleaseLookupError> {
    let response = match github_get(url, "application/vnd.github+json").call() {
        Ok(response) => response,
        Err(ureq::Error::Status(404, _)) => return Err(ReleaseLookupError::NotFound),
        Err(error) => {
            return Err(ReleaseLookupError::Other(SelfUpdateError::new(format!(
                "GitHub release request failed: {error}"
            ))));
        }
    };
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(MAX_RELEASE_JSON_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            ReleaseLookupError::Other(SelfUpdateError::new(format!(
                "read GitHub release response: {error}"
            )))
        })?;
    if bytes.len() as u64 > MAX_RELEASE_JSON_BYTES {
        return Err(ReleaseLookupError::Other(SelfUpdateError::new(
            "GitHub release response exceeds 1 MiB limit",
        )));
    }
    return serde_json::from_slice::<Release>(&bytes).map_err(|error| {
        ReleaseLookupError::Other(SelfUpdateError::new(format!(
            "decode GitHub release response: {error}"
        )))
    });
}

fn github_get(url: &str, accept: &str) -> ureq::Request {
    let mut request = ureq::get(url)
        .set("User-Agent", "ores-clis-core-self-update")
        .set("Accept", accept)
        .set("X-GitHub-Api-Version", "2022-11-28");

    if let Some(token) = github_token() {
        request = request.set("Authorization", &format!("Bearer {token}"));
    }

    return request;
}

fn github_token() -> Option<String> {
    if let Some(token) = env::var("GITHUB_TOKEN")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            env::var("GH_TOKEN")
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
    {
        return Some(token);
    }

    return github_cli_token();
}

fn github_cli_token() -> Option<String> {
    let output = Command::new("gh")
        .args(["auth", "token"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let token = String::from_utf8(output.stdout).ok()?;
    let token = token.trim();
    if token.is_empty() {
        return None;
    }

    return Some(token.to_owned());
}

fn select_asset(
    config: &SelfUpdateConfig,
    release: &Release,
) -> Result<ReleaseAsset, SelfUpdateError> {
    let mut candidates: Vec<(u16, &ReleaseAsset)> = release
        .assets
        .iter()
        .filter_map(|asset| score_asset(config, asset).map(|score| (score, asset)))
        .collect();
    candidates.sort_by(|left, right| right.0.cmp(&left.0));

    return candidates
        .first()
        .map(|(_, asset)| (*asset).clone())
        .ok_or_else(|| {
            SelfUpdateError::new(format!(
                "no release asset for binary={} os={} arch={} in {}",
                config.binary_name,
                env::consts::OS,
                env::consts::ARCH,
                release.tag_name
            ))
        });
}

fn score_asset(config: &SelfUpdateConfig, asset: &ReleaseAsset) -> Option<u16> {
    let name = asset.name.to_ascii_lowercase();
    if is_checksum_asset(&name) {
        return None;
    }

    let normalized_name = name.replace('_', "-");
    let binary = config.binary_name.to_ascii_lowercase().replace('_', "-");
    if !normalized_name.contains(&binary) {
        return None;
    }

    let os_score = match env::consts::OS {
        "macos" if contains_any(&normalized_name, &["macos", "darwin", "apple-darwin"]) => 40,
        "linux" if contains_any(&normalized_name, &["linux", "unknown-linux"]) => 40,
        "windows" if contains_any(&normalized_name, &["windows", "pc-windows", "win64"]) => 40,
        _ => 0,
    };
    if os_score == 0 {
        return None;
    }

    let arch_score = match env::consts::ARCH {
        "x86_64" if contains_any(&normalized_name, &["x86-64", "x86_64", "amd64"]) => 30,
        "aarch64" if contains_any(&normalized_name, &["aarch64", "arm64"]) => 30,
        "x86" if contains_any(&normalized_name, &["i686", "x86"]) => 30,
        other if normalized_name.contains(other) => 30,
        _ => 0,
    };

    let packaging_score = if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        8
    } else if name.ends_with(".zip") {
        7
    } else {
        10
    };

    return Some(50 + os_score + arch_score + packaging_score);
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    return needles.iter().any(|needle| haystack.contains(needle));
}

fn executable_name(binary_name: &str) -> String {
    if cfg!(windows) {
        return format!("{binary_name}.exe");
    }
    return binary_name.to_owned();
}

fn download_asset(url: &str, destination: &Path) -> Result<(), SelfUpdateError> {
    let response = github_get(url, "application/octet-stream")
        .call()
        .map_err(|error| SelfUpdateError::new(format!("download release asset: {error}")))?;
    if let Some(length) = response.header("Content-Length") {
        let length = length
            .parse::<u64>()
            .map_err(|_| SelfUpdateError::new("release asset Content-Length is invalid"))?;
        if length > MAX_RELEASE_ASSET_BYTES {
            return Err(SelfUpdateError::new("release asset exceeds 256 MiB limit"));
        }
    }
    let mut reader = response.into_reader().take(MAX_RELEASE_ASSET_BYTES + 1);
    let mut file = File::create(destination)
        .map_err(|error| SelfUpdateError::new(format!("create downloaded asset: {error}")))?;
    let copied = io::copy(&mut reader, &mut file)
        .map_err(|error| SelfUpdateError::new(format!("write downloaded asset: {error}")))?;
    if copied > MAX_RELEASE_ASSET_BYTES {
        return Err(SelfUpdateError::new(
            "release asset exceeds 256 MiB streaming limit",
        ));
    }
    file.flush()?;
    return Ok(());
}

fn verify_checksum(
    release: &Release,
    selected: &ReleaseAsset,
    downloaded: &Path,
    required: bool,
) -> Result<(), SelfUpdateError> {
    let checksum_asset = find_checksum_asset(release, selected);
    let Some(checksum_asset) = checksum_asset else {
        if required {
            return Err(SelfUpdateError::new(format!(
                "no SHA-256 checksum asset found for {}",
                selected.name
            )));
        }
        return Ok(());
    };

    let response = github_get(&checksum_asset.url, "application/octet-stream")
        .call()
        .map_err(|error| SelfUpdateError::new(format!("download checksum: {error}")))?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(MAX_CHECKSUM_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| SelfUpdateError::new(format!("read checksum: {error}")))?;
    if bytes.len() as u64 > MAX_CHECKSUM_BYTES {
        return Err(SelfUpdateError::new(
            "checksum response exceeds 256 KiB limit",
        ));
    }
    let text = String::from_utf8(bytes)
        .map_err(|_| SelfUpdateError::new("checksum response is not valid UTF-8"))?;

    let expected = parse_expected_checksum(&text, &selected.name).ok_or_else(|| {
        return SelfUpdateError::new(format!(
            "checksum file {} does not contain a SHA-256 for {}",
            checksum_asset.name, selected.name
        ));
    })?;
    let actual = sha256_file(downloaded)?;

    if expected != actual {
        return Err(SelfUpdateError::new(format!(
            "SHA-256 mismatch for {}: expected {expected}, got {actual}",
            selected.name
        )));
    }

    return Ok(());
}

fn find_checksum_asset<'a>(
    release: &'a Release,
    selected: &ReleaseAsset,
) -> Option<&'a ReleaseAsset> {
    let direct_names = [
        format!("{}.sha256", selected.name),
        format!("{}.sha256sum", selected.name),
        format!("{}.sha256.txt", selected.name),
    ];

    if let Some(asset) = release.assets.iter().find(|asset| {
        direct_names
            .iter()
            .any(|name| asset.name.eq_ignore_ascii_case(name))
    }) {
        return Some(asset);
    }

    return release.assets.iter().find(|asset| {
        let lower = asset.name.to_ascii_lowercase();
        return matches!(
            lower.as_str(),
            "sha256sums" | "sha256sums.txt" | "checksums.txt" | "checksums.sha256"
        );
    });
}

fn is_checksum_asset(name: &str) -> bool {
    return name.ends_with(".sha256")
        || name.ends_with(".sha256sum")
        || name.ends_with(".sha256.txt")
        || name.contains("checksum")
        || name.contains("sha256sum");
}

fn parse_expected_checksum(text: &str, selected_name: &str) -> Option<String> {
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let tokens: Vec<&str> = trimmed.split_whitespace().collect();
        if tokens.is_empty() {
            continue;
        }

        let first = tokens[0].trim_start_matches("SHA256:");
        if first.len() == 64
            && first.chars().all(|character| character.is_ascii_hexdigit())
            && (tokens.len() == 1 || trimmed.contains(selected_name))
        {
            return Some(first.to_ascii_lowercase());
        }
    }
    return None;
}

fn sha256_file(path: &Path) -> Result<String, SelfUpdateError> {
    let mut file = File::open(path)
        .map_err(|error| SelfUpdateError::new(format!("open asset for hashing: {error}")))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];

    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| SelfUpdateError::new(format!("hash release asset: {error}")))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    let digest = hasher.finalize();
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut encoded, "{byte:02x}")
            .map_err(|error| SelfUpdateError::new(format!("encode SHA-256: {error}")))?;
    }
    return Ok(encoded);
}

fn prepare_candidate(
    config: &SelfUpdateConfig,
    downloaded: &Path,
    temp_root: &Path,
) -> Result<PathBuf, SelfUpdateError> {
    let file_name = downloaded
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| SelfUpdateError::new("release asset has a non-UTF8 filename"))?
        .to_ascii_lowercase();

    if file_name.ends_with(".tar.gz") || file_name.ends_with(".tgz") {
        let extract_dir = temp_root.join("tar-extract");
        fs::create_dir_all(&extract_dir)?;
        let file = File::open(downloaded)?;
        let decoder = flate2::read::GzDecoder::new(file);
        let mut archive = tar::Archive::new(decoder);
        let wanted = executable_name(config.binary_name);
        let mut total = 0_u64;
        let mut seen = 0_usize;
        for entry in archive
            .entries()
            .map_err(|error| SelfUpdateError::new(format!("read tar archive: {error}")))?
        {
            seen += 1;
            if seen > MAX_ARCHIVE_ENTRIES {
                return Err(SelfUpdateError::new(
                    "tar archive exceeds entry-count limit",
                ));
            }
            let mut entry =
                entry.map_err(|error| SelfUpdateError::new(format!("read tar entry: {error}")))?;
            let path = entry
                .path()
                .map_err(|error| SelfUpdateError::new(format!("read tar entry path: {error}")))?;
            validate_archive_path(&path)?;
            let entry_type = entry.header().entry_type();
            if !(entry_type.is_file() || entry_type.is_dir()) {
                return Err(SelfUpdateError::new(
                    "tar archive contains a non-regular entry",
                ));
            }
            let size = entry
                .header()
                .size()
                .map_err(|error| SelfUpdateError::new(format!("read tar entry size: {error}")))?;
            if size > MAX_ARCHIVE_ENTRY_BYTES {
                return Err(SelfUpdateError::new("tar entry exceeds per-entry limit"));
            }
            total = total
                .checked_add(size)
                .ok_or_else(|| SelfUpdateError::new("tar archive size overflow"))?;
            if total > MAX_ARCHIVE_TOTAL_BYTES {
                return Err(SelfUpdateError::new(
                    "tar archive exceeds total extracted-byte limit",
                ));
            }
            if entry_type.is_dir() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            if name != wanted && name != config.binary_name {
                continue;
            }
            let output = extract_dir.join(&wanted);
            let mut destination = File::create(&output)?;
            let copied = io::copy(
                &mut entry.take(MAX_ARCHIVE_ENTRY_BYTES + 1),
                &mut destination,
            )?;
            if copied > MAX_ARCHIVE_ENTRY_BYTES {
                return Err(SelfUpdateError::new(
                    "tar executable exceeds per-entry limit",
                ));
            }
            destination.flush()?;
            set_executable(&output)?;
            return Ok(output);
        }
        return Err(SelfUpdateError::new(format!(
            "tar archive does not contain {wanted}"
        )));
    }

    if file_name.ends_with(".zip") {
        let extract_dir = temp_root.join("zip-extract");
        fs::create_dir_all(&extract_dir)?;
        extract_zip_binary(downloaded, &extract_dir, config.binary_name)?;
        return find_executable(&extract_dir, config.binary_name);
    }

    return Ok(downloaded.to_path_buf());
}

fn extract_zip_binary(
    archive_path: &Path,
    extract_dir: &Path,
    binary_name: &str,
) -> Result<(), SelfUpdateError> {
    let file = File::open(archive_path)?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|error| SelfUpdateError::new(format!("open zip archive: {error}")))?;
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        return Err(SelfUpdateError::new(
            "zip archive exceeds entry-count limit",
        ));
    }
    let wanted = executable_name(binary_name);
    let mut total = 0_u64;

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| SelfUpdateError::new(format!("read zip entry: {error}")))?;
        let path = Path::new(entry.name());
        validate_archive_path(path)?;
        if entry.is_dir() {
            continue;
        }
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(SelfUpdateError::new("zip archive contains a symlink"));
        }
        let size = entry.size();
        if size > MAX_ARCHIVE_ENTRY_BYTES {
            return Err(SelfUpdateError::new("zip entry exceeds per-entry limit"));
        }
        total = total
            .checked_add(size)
            .ok_or_else(|| SelfUpdateError::new("zip archive size overflow"))?;
        if total > MAX_ARCHIVE_TOTAL_BYTES {
            return Err(SelfUpdateError::new(
                "zip archive exceeds total extracted-byte limit",
            ));
        }
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if name != wanted && name != binary_name {
            continue;
        }

        let output = extract_dir.join(&wanted);
        let mut destination = File::create(&output)?;
        let copied = io::copy(
            &mut entry.take(MAX_ARCHIVE_ENTRY_BYTES + 1),
            &mut destination,
        )?;
        if copied > MAX_ARCHIVE_ENTRY_BYTES {
            return Err(SelfUpdateError::new(
                "zip executable exceeds per-entry limit",
            ));
        }
        destination.flush()?;
        set_executable(&output)?;
        return Ok(());
    }

    return Err(SelfUpdateError::new(format!(
        "zip archive does not contain {wanted}"
    )));
}

fn validate_archive_path(path: &Path) -> Result<(), SelfUpdateError> {
    if path.is_absolute() {
        return Err(SelfUpdateError::new("archive entry path must be relative"));
    }
    let mut depth = 0_usize;
    for component in path.components() {
        match component {
            Component::Normal(_) => {
                depth += 1;
                if depth > MAX_ARCHIVE_PATH_DEPTH {
                    return Err(SelfUpdateError::new("archive entry path is too deep"));
                }
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(SelfUpdateError::new(
                    "archive entry path contains traversal or absolute components",
                ));
            }
        }
    }
    if depth == 0 {
        return Err(SelfUpdateError::new("archive entry path is empty"));
    }
    return Ok(());
}

fn find_executable(root: &Path, binary_name: &str) -> Result<PathBuf, SelfUpdateError> {
    let wanted = executable_name(binary_name);
    let mut stack = vec![root.to_path_buf()];

    while let Some(directory) = stack.pop() {
        for entry in fs::read_dir(&directory)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            if name == wanted || name == binary_name {
                set_executable(&path)?;
                return Ok(path);
            }
        }
    }

    return Err(SelfUpdateError::new(format!(
        "release archive does not contain {wanted}"
    )));
}

#[cfg(unix)]
fn set_executable(path: &Path) -> Result<(), SelfUpdateError> {
    use std::os::unix::fs::PermissionsExt;
    let metadata = fs::metadata(path)?;
    let mut permissions = metadata.permissions();
    permissions.set_mode(permissions.mode() | 0o755);
    fs::set_permissions(path, permissions)?;
    return Ok(());
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> Result<(), SelfUpdateError> {
    return Ok(());
}

fn validate_candidate(path: &Path, expected_version: &str) -> Result<(), SelfUpdateError> {
    set_executable(path)?;
    let output = Command::new(path)
        .arg("--version")
        .stdin(Stdio::null())
        .output()
        .map_err(|error| SelfUpdateError::new(format!("execute candidate --version: {error}")))?;

    if !output.status.success() {
        return Err(SelfUpdateError::new(format!(
            "candidate --version exited with {}",
            output.status
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}\n{stderr}");
    if !version_output_matches(&combined, expected_version) {
        return Err(SelfUpdateError::new(format!(
            "candidate version output does not report expected semantic version {expected_version}"
        )));
    }

    return Ok(());
}

fn version_output_matches(output: &str, expected_version: &str) -> bool {
    let Ok(expected) = semver::Version::parse(&normalize_version(expected_version)) else {
        return false;
    };
    output.split_whitespace().any(|token| {
        let token = token.trim_matches(|character: char| {
            !character.is_ascii_alphanumeric() && !matches!(character, '.' | '-' | '+')
        });
        let token = token.strip_prefix('v').unwrap_or(token);
        semver::Version::parse(token)
            .map(|candidate| candidate == expected)
            .unwrap_or(false)
    })
}

fn normalize_version(value: &str) -> String {
    let trimmed = value.trim();
    return trimmed.strip_prefix('v').unwrap_or(trimmed).to_owned();
}

fn versions_equal(left: &str, right: &str) -> bool {
    match (semver::Version::parse(left), semver::Version::parse(right)) {
        (Ok(left), Ok(right)) => return left == right,
        _ => return left == right,
    }
}

fn confirm() -> Result<bool, SelfUpdateError> {
    print!("Proceed with self-update? [y/N] ");
    io::stdout().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    return Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ));
}

fn print_plan(config: &SelfUpdateConfig, current: &str, target: &str, asset: &str) {
    println!("Self-update plan:");
    println!("  repository: {}", config.repository());
    println!("  binary:     {}", config.binary_name);
    println!("  current:    {current}");
    println!("  target:     {target}");
    println!("  asset:      {asset}");
}

fn emit_outcome(outcome: &SelfUpdateOutcome, json: bool) {
    if json {
        let body = match outcome {
            SelfUpdateOutcome::AlreadyCurrent { version } => serde_json::json!({
                "ok": true,
                "status": "already-current",
                "version": version,
            }),
            SelfUpdateOutcome::CheckOnly { current, target } => serde_json::json!({
                "ok": true,
                "status": "check-only",
                "current": current,
                "target": target,
                "update_available": current != target,
            }),
            SelfUpdateOutcome::DryRun { current, target } => serde_json::json!({
                "ok": true,
                "status": "dry-run",
                "current": current,
                "target": target,
            }),
            SelfUpdateOutcome::Updated { previous, current } => serde_json::json!({
                "ok": true,
                "status": "updated",
                "previous": previous,
                "current": current,
            }),
        };
        println!("{body}");
        return;
    }

    match outcome {
        SelfUpdateOutcome::AlreadyCurrent { version } => {
            println!("Already on {version}.");
        }
        SelfUpdateOutcome::CheckOnly { current, target } => {
            if current == target {
                println!("Already on {current}.");
            } else {
                println!("Update available: {current} -> {target}");
            }
        }
        SelfUpdateOutcome::DryRun { current, target } => {
            println!("Dry run: would update {current} -> {target}");
        }
        SelfUpdateOutcome::Updated { previous, current } => {
            println!("Updated {previous} -> {current}");
        }
    }
}

fn print_help(binary_name: &str) {
    println!("Usage: {binary_name} self-update [VERSION] [OPTIONS]");
    println!();
    println!("Update this CLI from its GitHub Releases assets.");
    println!();
    println!("Arguments:");
    println!(
        "  VERSION              Target release (for example 1.2.3 or v1.2.3); default: latest"
    );
    println!();
    println!("Options:");
    println!("  --interactive        Always prompt before replacing the executable");
    println!("  --non-interactive    Never prompt");
    println!("  -y, --yes            Assume yes without prompting");
    println!("  --check              Report whether an update is available");
    println!("  --dry-run            Resolve and select the release without replacing the binary");
    println!("  --verify             Require a matching SHA-256 release asset");
    println!("  --no-verify          Skip checksum discovery and verification");
    println!("  --json               Emit machine-readable status");
    println!("  -h, --help           Print help");
    println!();
    println!("Environment:");
    println!("  GITHUB_TOKEN / GH_TOKEN   Token used for private GitHub release access");
    println!("  gh auth token              Used automatically when env tokens are unset");
}

#[cfg(test)]
mod tests {
    use super::{
        ReleaseAsset, SelfUpdateConfig, VerificationPolicy, normalize_version, parse_cli_args,
        parse_expected_checksum, score_asset, version_output_matches,
    };

    #[test]
    fn default_config_requires_sha256_and_disallows_unverified_updates() {
        let config = SelfUpdateConfig::new("owner", "repo", "tool", "1.0.0");
        assert_eq!(
            config.verification_policy,
            VerificationPolicy::RequireSha256
        );
        assert!(!config.allow_no_verify);

        let compatibility = config
            .clone()
            .with_verification_policy(VerificationPolicy::PreferSha256)
            .allow_unverified_updates(true);
        assert_eq!(
            compatibility.verification_policy,
            VerificationPolicy::PreferSha256
        );
        assert!(compatibility.allow_no_verify);
    }

    #[test]
    fn candidate_version_matching_is_semver_exact_not_substring_based() {
        assert!(version_output_matches("tool 1.2.3\n", "1.2.3"));
        assert!(version_output_matches("tool v1.2.3 (build abc)\n", "1.2.3"));
        assert!(!version_output_matches("tool 11.2.30\n", "1.2.3"));
        assert!(!version_output_matches("tool version=1.2.3-dev\n", "1.2.3"));
    }

    #[test]
    fn parses_version_and_non_interactive_mode() {
        let args = parse_cli_args([
            "1.2.3".to_owned(),
            "--non-interactive".to_owned(),
            "--verify".to_owned(),
        ])
        .expect("arguments should parse");
        assert_eq!(args.version.as_deref(), Some("1.2.3"));
        assert_eq!(args.interactive, Some(false));
        assert!(args.verify_required);
    }

    #[test]
    fn rejects_conflicting_interactive_modes() {
        let result = parse_cli_args(["--interactive".to_owned(), "--non-interactive".to_owned()]);
        assert!(result.is_err());
    }

    #[test]
    fn parses_standard_checksum_line() {
        let checksum = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  tool-linux-x86_64.tar.gz\n";
        let parsed = parse_expected_checksum(checksum, "tool-linux-x86_64.tar.gz");
        assert_eq!(
            parsed.as_deref(),
            Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        );
    }

    #[test]
    fn version_normalization_is_stable_for_raw_and_prefixed_versions() {
        assert_eq!(normalize_version("1.2.3"), "1.2.3");
        assert_eq!(normalize_version("v1.2.3"), "1.2.3");
    }

    #[test]
    fn asset_scoring_requires_binary_and_platform() {
        let config = SelfUpdateConfig::new("owner", "repo", "tool", "1.0.0");
        let current_os = std::env::consts::OS;
        let current_arch = std::env::consts::ARCH;
        let platform = match current_os {
            "macos" => "darwin",
            "linux" => "linux",
            "windows" => "windows",
            other => other,
        };
        let asset = ReleaseAsset {
            name: format!("tool-{platform}-{current_arch}.tar.gz"),
            url: "https://example.invalid/asset".to_owned(),
        };
        assert!(score_asset(&config, &asset).is_some());
    }
}
