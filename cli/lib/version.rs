// Copyright 2018-2026 the Deno authors. MIT license.

use std::borrow::Cow;

use deno_runtime::deno_telemetry::OtelRuntimeConfig;

use crate::shared::ReleaseChannel;

pub fn otel_runtime_config() -> OtelRuntimeConfig {
  OtelRuntimeConfig {
    runtime_name: Cow::Borrowed("rjs"),
    runtime_version: Cow::Borrowed(crate::version::DENO_VERSION_INFO.deno),
  }
}

const GIT_COMMIT_HASH: &str = env!("GIT_COMMIT_HASH");
const TYPESCRIPT: &str = "6.0.3";

/// The version of this build: the commit sha it was built from.
///
/// rjs has no semantic version numbers. Every commit is built and published,
/// so the sha is the only version there is.
pub const DENO_VERSION: &str = env!("DENO_VERSION");

/// The Node.js version that Deno reports through `process.version` /
/// `process.versions.node`, used to enforce package.json `engines.node`
/// constraints.
///
/// Re-exported from `ext/node`, which is the single source of truth shared with
/// the `process.version` polyfill, so the engines check and the reported value
/// can never drift.
pub use deno_node::NODE_VERSION;

pub static DENO_VERSION_INFO: std::sync::LazyLock<DenoVersionInfo> =
  std::sync::LazyLock::new(|| DenoVersionInfo {
    deno: DENO_VERSION,
    release_channel: ReleaseChannel::Default,
    git_hash: GIT_COMMIT_HASH,
    user_agent: concat!("rjs/", env!("DENO_VERSION")),
    typescript: TYPESCRIPT,
  });

pub struct DenoVersionInfo {
  /// The commit sha this binary was built from.
  pub deno: &'static str,

  pub release_channel: ReleaseChannel,

  /// A full git hash.
  pub git_hash: &'static str,

  /// A user-agent header that will be used in HTTP client.
  pub user_agent: &'static str,

  pub typescript: &'static str,
}

impl DenoVersionInfo {
  /// The commit sha of this build.
  pub fn version_or_git_hash(&self) -> &'static str {
    self.git_hash
  }
}
