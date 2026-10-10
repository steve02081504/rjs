// Copyright 2018-2026 the Deno authors. MIT license.

//! This module provides the feature to upgrade the rjs executable.
//!
//! rjs has exactly one release channel, `default`, and a build's version *is*
//! the commit sha it was built from. Every build is published to the rolling
//! `default` release of `steve02081504/rjs` as a per-target zip, so upgrading
//! always means the same thing: look up the newest published sha, download the
//! zip built for this target, verify its checksum and replace the running
//! executable. There are no semver versions, no canary/rc channels and no
//! delta patches.

use std::env;
use std::fs;
use std::io::IsTerminal;
use std::ops::Sub;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use deno_core::anyhow::Context;
use deno_core::anyhow::bail;
use deno_core::error::AnyError;
use deno_core::unsync::spawn;
use deno_core::url::Url;
use deno_lib::shared::ReleaseChannel;
use deno_lib::version;
use once_cell::sync::Lazy;
use sha2::Digest;

use crate::args::Flags;
use crate::args::UpgradeFlags;
use crate::colors;
use crate::factory::CliFactory;
use crate::http_util::HttpClient;
use crate::http_util::HttpClientProvider;
use crate::util::archive;
use crate::util::progress_bar::ProgressBar;
use crate::util::progress_bar::ProgressBarStyle;

/// Every build of rjs is published to the single rolling release tagged
/// `default`, so the download URL never depends on a channel or a version.
static RELEASE_DOWNLOAD_URL: &str =
  "https://github.com/steve02081504/rjs/releases/download/default";

/// Name of the archive published for this build target.
static ARCHIVE_NAME: Lazy<String> =
  Lazy::new(|| format!("rjs-{}.zip", env!("TARGET")));

/// The executable inside the archive produced by the release build. Upgrading
/// replaces this one file; nothing else is installed next to it.
const EXE_NAME: &str = if cfg!(windows) { "rjs.exe" } else { "rjs" };

// How often query server for new version. In hours.
const UPGRADE_CHECK_INTERVAL: i64 = 24;

const UPGRADE_CHECK_FETCH_DELAY: Duration = Duration::from_millis(500);

/// Environment necessary for doing the update checker.
/// An alternate trait implementation can be provided for testing purposes.
trait UpdateCheckerEnvironment: Clone {
  fn read_check_file(&self) -> String;
  fn write_check_file(&self, text: &str);
  fn current_time(&self) -> chrono::DateTime<chrono::Utc>;
}

#[derive(Clone)]
struct RealUpdateCheckerEnvironment {
  cache_file_path: PathBuf,
  current_time: chrono::DateTime<chrono::Utc>,
}

impl RealUpdateCheckerEnvironment {
  pub fn new(cache_file_path: PathBuf) -> Self {
    Self {
      cache_file_path,
      // cache the current time
      current_time: chrono::Utc::now(),
    }
  }
}

impl UpdateCheckerEnvironment for RealUpdateCheckerEnvironment {
  fn read_check_file(&self) -> String {
    std::fs::read_to_string(&self.cache_file_path).unwrap_or_default()
  }

  fn write_check_file(&self, text: &str) {
    let _ = std::fs::write(&self.cache_file_path, text);
  }

  fn current_time(&self) -> chrono::DateTime<chrono::Utc> {
    self.current_time
  }
}

struct UpdateChecker<TEnvironment: UpdateCheckerEnvironment> {
  env: TEnvironment,
  maybe_file: Option<CheckVersionFile>,
}

impl<TEnvironment: UpdateCheckerEnvironment> UpdateChecker<TEnvironment> {
  pub fn new(env: TEnvironment) -> Self {
    let maybe_file = CheckVersionFile::parse(env.read_check_file());
    Self { env, maybe_file }
  }

  pub fn should_check_for_new_version(&self) -> bool {
    let Some(file) = &self.maybe_file else {
      return true;
    };

    let last_check_age = self
      .env
      .current_time()
      .signed_duration_since(file.last_checked);
    last_check_age > chrono::Duration::hours(UPGRADE_CHECK_INTERVAL)
  }

  /// Returns the newest build's sha if it differs from the running one and the
  /// user hasn't been told about it recently.
  pub fn should_prompt(&self) -> Option<String> {
    let file = self.maybe_file.as_ref()?;
    // If the current version saved is not the actually current version of the binary
    // It means
    // - We already check for a new version today
    // - The user have probably upgraded today
    // So we should not prompt and wait for tomorrow for the latest version to be updated again
    let current_version = version::DENO_VERSION_INFO.deno;
    if file.current_version != current_version {
      return None;
    }
    if file.latest_version == current_version {
      return None;
    }

    let last_prompt_age = self
      .env
      .current_time()
      .signed_duration_since(file.last_prompt);
    if last_prompt_age > chrono::Duration::hours(UPGRADE_CHECK_INTERVAL) {
      Some(file.latest_version.clone())
    } else {
      None
    }
  }

  /// Store that we showed the update message to the user.
  pub fn store_prompted(self) {
    if let Some(file) = self.maybe_file {
      self.env.write_check_file(
        &file.with_last_prompt(self.env.current_time()).serialize(),
      );
    }
  }
}

pub fn upgrade_check_enabled() -> bool {
  matches!(
    env::var("DENO_NO_UPDATE_CHECK"),
    Err(env::VarError::NotPresent)
  )
}

pub fn check_for_upgrades(
  http_client_provider: Arc<HttpClientProvider>,
  cache_file_path: PathBuf,
) {
  if !upgrade_check_enabled() {
    return;
  }

  let env = RealUpdateCheckerEnvironment::new(cache_file_path);
  let update_checker = UpdateChecker::new(env);

  if update_checker.should_check_for_new_version() {
    let env = update_checker.env.clone();
    // do this asynchronously on a separate task
    spawn(async move {
      // Sleep for a small amount of time to not unnecessarily impact startup
      // time.
      tokio::time::sleep(UPGRADE_CHECK_FETCH_DELAY).await;

      fetch_and_store_latest_version(&env, &http_client_provider).await;

      log::debug!("Finished upgrade checker.")
    });
  }

  // Don't bother doing any more computation if we're not in TTY environment.
  let should_prompt =
    log::log_enabled!(log::Level::Info) && std::io::stderr().is_terminal();

  if !should_prompt {
    return;
  }

  // Print a message if a new build is available. There is only the `default`
  // channel, and a build is identified by the sha it was built from.
  if let Some(latest_version) = update_checker.should_prompt() {
    log::info!(
      "{} {} → {} {}",
      colors::green("A new build of rjs is available:"),
      colors::cyan(version::DENO_VERSION_INFO.deno),
      colors::cyan(&latest_version),
      colors::italic_gray("Run `rjs upgrade` to install it.")
    );

    update_checker.store_prompted();
  }
}

/// rjs only ever ships the single `default` channel, so an upgrade offered to
/// the language server is never a canary build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LspVersionUpgradeInfo {
  pub latest_version: String,
  pub is_canary: bool,
}

pub async fn check_for_upgrades_for_lsp(
  http_client_provider: Arc<HttpClientProvider>,
) -> Result<Option<LspVersionUpgradeInfo>, AnyError> {
  if !upgrade_check_enabled() {
    return Ok(None);
  }

  let client = http_client_provider.get_or_create()?;
  let latest_version = fetch_latest_version(&client).await?;

  // A build's version is the sha it was built from, so "a different sha" is
  // the only thing there is to compare.
  if latest_version == version::DENO_VERSION_INFO.deno {
    return Ok(None);
  }

  Ok(Some(LspVersionUpgradeInfo {
    latest_version,
    is_canary: false,
  }))
}

async fn fetch_and_store_latest_version<
  TEnvironment: UpdateCheckerEnvironment,
>(
  env: &TEnvironment,
  http_client_provider: &HttpClientProvider,
) {
  let Ok(client) = http_client_provider.get_or_create() else {
    return;
  };
  let Ok(latest_version) = fetch_latest_version(&client).await else {
    return;
  };

  let version_file = CheckVersionFile {
    // put a date in the past here so that prompt can be shown on next run
    last_prompt: env
      .current_time()
      .sub(chrono::Duration::hours(UPGRADE_CHECK_INTERVAL + 1)),
    last_checked: env.current_time(),
    current_version: version::DENO_VERSION_INFO.deno.to_string(),
    latest_version,
    current_release_channel: version::DENO_VERSION_INFO.release_channel,
  };

  env.write_check_file(&version_file.serialize());
}

/// Look up the sha of the newest published build for this target.
async fn find_latest_version_to_upgrade(
  client: &HttpClient,
) -> Result<String, AnyError> {
  log::info!("{}", colors::gray("Looking up the latest rjs build"));

  let latest_version = match fetch_latest_version(client).await {
    Ok(version) => version,
    Err(err) => {
      // The version file for a target only exists once a build for that target
      // has been published. `download_text` wraps the response status in a
      // context, so the whole chain has to be inspected, not the top message.
      let chain = format!("{err:#}").to_lowercase();
      if chain.contains("not found") || chain.contains("404") {
        bail!(
          "No rjs build is available for {} at the moment.",
          env!("TARGET")
        );
      } else {
        return Err(err);
      }
    }
  };

  log::info!("");
  log::info!(
    "Found latest rjs version {}",
    color_print::cformat!("<g>{}</>", latest_version)
  );
  log::info!("");

  Ok(latest_version)
}

/// Read the version file published next to the archives. Its contents are the
/// full 40 character commit sha of the newest build, which is also the version
/// of that build.
async fn fetch_latest_version(client: &HttpClient) -> Result<String, AnyError> {
  let url = get_latest_version_url();
  let text = client.download_text(url.parse()?).await.with_context(|| {
    format!("failed to look up the latest rjs build at {url}")
  })?;
  normalize_version_from_server(&text)
    .with_context(|| format!("unexpected contents at {url}"))
}

/// A build's version is the 40 character commit sha it was built from.
fn is_git_hash(text: &str) -> bool {
  text.len() == 40 && text.bytes().all(|b| b.is_ascii_hexdigit())
}

fn normalize_version_from_server(text: &str) -> Result<String, AnyError> {
  let version = text.trim().to_lowercase();
  if !is_git_hash(&version) {
    bail!(
      "expected a 40 character commit sha, but got: {}",
      colors::gray(text.trim().chars().take(100).collect::<String>())
    );
  }
  Ok(version)
}

/// The version file for this target: `<RELEASE_DOWNLOAD_URL>/rjs-<TARGET>.txt`.
fn get_latest_version_url() -> String {
  format!("{RELEASE_DOWNLOAD_URL}/rjs-{}.txt", env!("TARGET"))
}

/// The checksum file for this target's archive:
/// `<RELEASE_DOWNLOAD_URL>/rjs-<TARGET>.sha256`.
fn get_checksum_url() -> String {
  format!("{RELEASE_DOWNLOAD_URL}/rjs-{}.sha256", env!("TARGET"))
}

fn get_download_url() -> Result<Url, AnyError> {
  let download_url =
    format!("{RELEASE_DOWNLOAD_URL}/{}", ARCHIVE_NAME.as_str());

  Url::parse(&download_url).with_context(|| {
    format!("Failed to parse URL to download new build: {download_url}")
  })
}

/// Fetch the sha256 published next to the archive. The downloaded bytes are
/// only ever installed when they match it.
async fn fetch_expected_checksum(
  client: &HttpClient,
) -> Result<String, AnyError> {
  let url = get_checksum_url();
  let text = client
    .download_text(url.parse()?)
    .await
    .with_context(|| format!("failed to fetch the checksum at {url}"))?;

  // The file holds a bare sha256, optionally followed by the file name.
  let checksum = text
    .split_whitespace()
    .next()
    .unwrap_or_default()
    .to_lowercase();
  if checksum.len() != 64 || !checksum.bytes().all(|b| b.is_ascii_hexdigit()) {
    bail!("unexpected checksum contents at {url}");
  }

  Ok(checksum)
}

/// Compute the SHA-256 hash of the given data, returning it as a lowercase
/// hex string.
fn compute_sha256(data: &[u8]) -> String {
  let digest = sha2::Sha256::digest(data);
  faster_hex::hex_string(&digest)
}

fn verify_checksum(
  data: &[u8],
  expected_checksum: &str,
) -> Result<(), AnyError> {
  let computed_hex = compute_sha256(data);
  if computed_hex != expected_checksum {
    bail!(
      "Checksum verification failed.\n  Actual:   {}\n  Expected: {}",
      computed_hex,
      expected_checksum
    );
  }

  log::info!("{}", colors::gray("Checksum verified"));
  Ok(())
}

/// Where the downloaded archive of a build is cached. The cache is keyed by
/// the build's sha and by the target, so the file can never be confused with
/// the archive of another build. There is only the `default` channel, which is
/// the only directory level under the download folder.
fn get_binary_cache_path(dl_dir: &Path, version: &str) -> PathBuf {
  dl_dir
    .join("default")
    .join(version)
    .join(ARCHIVE_NAME.as_str())
}

fn try_read_cached_binary(
  sys: &impl sys_traits::FsRead,
  cache_path: &Path,
) -> Option<Vec<u8>> {
  match sys.fs_read(cache_path) {
    Ok(data) => Some(data.into_owned()),
    Err(_) => None,
  }
}

fn store_cached_binary(
  sys: &impl deno_path_util::fs::AtomicWriteFileSys,
  cache_path: &Path,
  data: &[u8],
) {
  if let Some(parent) = cache_path.parent() {
    let _ = fs::create_dir_all(parent);
  }
  if let Err(err) =
    deno_path_util::fs::atomic_write_file(sys, cache_path, data, 0o644)
  {
    log::debug!("Failed to cache binary: {}", err);
  }
}

async fn download_archive(client: &HttpClient) -> Result<Vec<u8>, AnyError> {
  let download_url = get_download_url()?;
  log::info!("{}", colors::gray(format!("Downloading {download_url}")));

  let progress_bar = ProgressBar::new(ProgressBarStyle::DownloadBars);
  // provide an empty string here in order to prefer the downloading
  // text above which will stay alive after the progress bars are complete
  let progress = progress_bar.update("");
  let response = client
    .download_with_progress_and_retries(
      download_url.clone(),
      &Default::default(),
      &progress,
    )
    .await
    .with_context(|| {
      format!(
        "Failed downloading {download_url}. A build for this platform may not have been published yet."
      )
    })?;

  match response.into_maybe_bytes()? {
    Some(data) => Ok(data),
    None => bail!(
      "No rjs build for {} has been published at {download_url} yet.",
      env!("TARGET")
    ),
  }
}

pub async fn upgrade(
  flags: Arc<Flags>,
  upgrade_flags: UpgradeFlags,
) -> Result<(), AnyError> {
  let factory = CliFactory::from_flags(flags);
  let cli_options = factory.cli_options()?;
  let http_client_provider = factory.http_client_provider();
  let client = http_client_provider.get_or_create()?;
  let current_exe_path = env::current_exe()
    .context("failed to get the path of the current executable")?;
  let full_path_output_flag = upgrade_flags
    .output
    .as_ref()
    .map(|output| cli_options.initial_cwd().join(output));
  let output_exe_path =
    full_path_output_flag.as_ref().unwrap_or(&current_exe_path);

  let permissions = set_exe_permissions(&current_exe_path, output_exe_path)?;

  // `--output` asks for a file to be written, so it installs even when the
  // running build is already the latest one; `--force` says the same thing
  // explicitly.
  let force = upgrade_flags.force || full_path_output_flag.is_some();

  let current_version = version::DENO_VERSION_INFO.deno;
  log::info!("Current rjs version: {}", colors::cyan(current_version));

  let latest_version = find_latest_version_to_upgrade(&client).await?;
  if !force && latest_version == current_version {
    log::info!(
      "Local rjs version {} is the most recent build",
      color_print::cformat!("<g>{}</>", current_version)
    );
    return Ok(());
  }

  let expected_checksum = fetch_expected_checksum(&client).await?;

  let dl_dir = factory.deno_dir()?.dl_folder_path();
  let cache_path = get_binary_cache_path(&dl_dir, &latest_version);
  let sys = &sys_traits::impls::RealSys;

  let archive_data = match try_read_cached_binary(sys, &cache_path) {
    Some(data) if compute_sha256(&data) == expected_checksum => {
      log::info!(
        "{}",
        colors::gray(format!(
          "Using cached build from {}",
          cache_path.display()
        ))
      );
      data
    }
    maybe_stale => {
      if maybe_stale.is_some() {
        log::warn!(
          "Cached build at '{}' does not match the published checksum; downloading it again",
          cache_path.display()
        );
      }
      let data = download_archive(&client).await?;
      // Never install bytes that don't match the published checksum.
      verify_checksum(&data, &expected_checksum)?;
      store_cached_binary(sys, &cache_path, &data);
      data
    }
  };

  log::info!(
    "{}",
    colors::gray(format!("rjs is upgrading to build {latest_version}"))
  );

  let temp_dir =
    tempfile::TempDir::new().context("failed to create temporary directory")?;

  let new_exe_path = archive::unpack_into_dir(archive::UnpackArgs {
    exe_name: EXE_NAME,
    archive_name: &ARCHIVE_NAME,
    archive_data: &archive_data,
    dest_path: temp_dir.path(),
  })
  .context("failed to extract archive")?;

  fs::set_permissions(&new_exe_path, permissions).with_context(|| {
    format!("failed to set permissions on '{}'", new_exe_path.display())
  })?;
  check_exe(&new_exe_path)?;

  if upgrade_flags.dry_run {
    fs::remove_file(&new_exe_path).with_context(|| {
      format!("failed to remove '{}'", new_exe_path.display())
    })?;
    log::info!("Upgraded successfully (dry run)");
    drop(temp_dir);
    return Ok(());
  }

  #[cfg(windows)]
  kill_running_rjs_lsp_processes();

  let output_result = if *output_exe_path == current_exe_path {
    replace_exe(&new_exe_path, output_exe_path)
  } else {
    fs::rename(&new_exe_path, output_exe_path)
      .or_else(|_| fs::copy(&new_exe_path, output_exe_path).map(|_| ()))
  };
  check_windows_access_denied_error(output_result, output_exe_path)?;

  log::info!(
    "\nUpgraded successfully to rjs build {}\n",
    colors::green(&latest_version)
  );

  drop(temp_dir); // delete the temp dir

  Ok(())
}

fn replace_exe(from: &Path, to: &Path) -> Result<(), std::io::Error> {
  if cfg!(windows) {
    // On windows you cannot replace the currently running executable.
    // so first we rename it to rjs.old.exe
    fs::rename(to, to.with_extension("old.exe"))?;
  } else {
    fs::remove_file(to)?;
  }
  // Windows cannot rename files across device boundaries, so if rename fails,
  // we try again with copy.
  fs::rename(from, to).or_else(|_| fs::copy(from, to).map(|_| ()))?;
  Ok(())
}

fn check_windows_access_denied_error(
  output_result: Result<(), std::io::Error>,
  output_exe_path: &Path,
) -> Result<(), AnyError> {
  let Err(err) = output_result else {
    return Ok(());
  };

  if !cfg!(windows) {
    return Err(err).with_context(|| {
      format!(
        "failed to replace the executable at '{}'",
        output_exe_path.display()
      )
    });
  }

  const WIN_ERROR_ACCESS_DENIED: i32 = 5;
  if err.raw_os_error() != Some(WIN_ERROR_ACCESS_DENIED) {
    return Err(err.into());
  };

  Err(err).with_context(|| {
    format!(
      concat!(
        "Could not replace the rjs executable. This may be because an ",
        "existing rjs process is running. Please ensure there are no ",
        "running rjs processes (ex. Stop-Process -Name rjs ; rjs {}), ",
        "close any editors before upgrading, and ensure you have ",
        "sufficient permission to '{}'."
      ),
      // skip the first argument, which is the executable path
      env::args().skip(1).collect::<Vec<_>>().join(" "),
      output_exe_path.display(),
    )
  })
}

#[cfg(windows)]
fn kill_running_rjs_lsp_processes() {
  // limit this to `rjs lsp` invocations to avoid killing important programs someone might be running
  let is_debug = log::log_enabled!(log::Level::Debug);
  let get_pipe = || {
    if is_debug {
      std::process::Stdio::inherit()
    } else {
      std::process::Stdio::null()
    }
  };
  let _ = Command::new("powershell.exe")
    .args([
      "-Command",
      r#"Get-WmiObject Win32_Process | Where-Object {
    $_.Name -eq 'rjs.exe' -and
    $_.CommandLine -match '^(?:\"[^\"]+\"|\S+)\s+lsp\b'
} | ForEach-Object {
  if ($_.Terminate()) {
    Write-Host 'Terminated:' $_.ProcessId
  }
}"#,
    ])
    .stdout(get_pipe())
    .stderr(get_pipe())
    .output();
}

fn set_exe_permissions(
  current_exe_path: &Path,
  output_exe_path: &Path,
) -> Result<std::fs::Permissions, AnyError> {
  let Ok(metadata) = fs::metadata(output_exe_path) else {
    let metadata = fs::metadata(current_exe_path).with_context(|| {
      format!(
        "failed to get metadata of the current executable at '{}'",
        current_exe_path.display()
      )
    })?;
    return Ok(metadata.permissions());
  };

  let permissions = metadata.permissions();
  if permissions.readonly() {
    bail!(
      "You do not have write permission to {}",
      output_exe_path.display()
    );
  }
  #[cfg(unix)]
  if std::os::unix::fs::MetadataExt::uid(&metadata) == 0
    && !nix::unistd::Uid::effective().is_root()
  {
    bail!(
      concat!(
        "You don't have write permission to {} because it's owned by root.\n",
        "Consider updating rjs through your package manager if its installed from it.\n",
        "Otherwise run `rjs upgrade` as root.",
      ),
      output_exe_path.display()
    );
  }
  Ok(permissions)
}

fn check_exe(exe_path: &Path) -> Result<(), AnyError> {
  let output = Command::new(exe_path)
    .arg("-V")
    .stderr(std::process::Stdio::inherit())
    .output()
    .with_context(|| format!("failed to run '{}'", exe_path.display()))?;
  if !output.status.success() {
    bail!(
      "Failed to validate rjs executable. This may be because your OS is unsupported or the executable is corrupted"
    )
  } else {
    Ok(())
  }
}

#[derive(Debug)]
struct CheckVersionFile {
  pub last_prompt: chrono::DateTime<chrono::Utc>,
  pub last_checked: chrono::DateTime<chrono::Utc>,
  pub current_version: String,
  pub latest_version: String,
  pub current_release_channel: ReleaseChannel,
}

impl CheckVersionFile {
  /// Parse the update checker's cache file.
  ///
  /// The file lives in the deno dir and may hold whatever an older build (or a
  /// plain Deno) left there, so anything that this version of rjs could not
  /// have written is reported as "no cache". The next check then simply
  /// refreshes it; a stale file must never make the upgrade checker fail.
  pub fn parse(content: String) -> Option<Self> {
    let split_content = content.split('!').collect::<Vec<_>>();

    if split_content.len() != 5 {
      return None;
    }

    let latest_version = split_content[2].trim().to_owned();
    if !is_git_hash(&latest_version) {
      return None;
    }
    let current_version = split_content[3].trim().to_owned();
    if !is_git_hash(&current_version) {
      return None;
    }
    // Only the `default` channel exists now; files naming a channel that has
    // been removed are ignored instead of trusted.
    let current_release_channel =
      ReleaseChannel::deserialize(split_content[4].trim()).ok()?;

    let last_prompt = chrono::DateTime::parse_from_rfc3339(split_content[0])
      .map(|dt| dt.with_timezone(&chrono::Utc))
      .ok()?;
    let last_checked = chrono::DateTime::parse_from_rfc3339(split_content[1])
      .map(|dt| dt.with_timezone(&chrono::Utc))
      .ok()?;

    Some(CheckVersionFile {
      last_prompt,
      last_checked,
      current_version,
      latest_version,
      current_release_channel,
    })
  }

  fn serialize(&self) -> String {
    format!(
      "{}!{}!{}!{}!{}",
      self.last_prompt.to_rfc3339(),
      self.last_checked.to_rfc3339(),
      self.latest_version,
      self.current_version,
      self.current_release_channel.serialize()
    )
  }

  fn with_last_prompt(self, dt: chrono::DateTime<chrono::Utc>) -> Self {
    Self {
      last_prompt: dt,
      ..self
    }
  }
}
