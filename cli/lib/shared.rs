// Copyright 2018-2026 the Deno authors. MIT license.

use thiserror::Error;

/// This module is shared between build script and the binaries. Use it sparsely.
#[derive(Debug, Error)]
#[error("Unrecognized release channel: {0}")]
pub struct UnrecognizedReleaseChannelError(pub String);

/// rjs has exactly one release channel: `default`.
///
/// There is no stable/canary/rc split to pick from — every commit is built and
/// published, and the commit sha is the version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseChannel {
  #[allow(unused, reason = "shared between build script and binaries")]
  Default,
}

impl ReleaseChannel {
  #[allow(unused, reason = "shared between build script and binaries")]
  pub fn name(&self) -> &str {
    match self {
      Self::Default => "default",
    }
  }

  #[allow(unused, reason = "shared between build script and binaries")]
  pub fn serialize(&self) -> String {
    match self {
      Self::Default => "default",
    }
    .to_string()
  }

  #[allow(unused, reason = "shared between build script and binaries")]
  pub fn deserialize(
    str_: &str,
  ) -> Result<Self, UnrecognizedReleaseChannelError> {
    Ok(match str_ {
      "default" => Self::Default,
      unknown => {
        return Err(UnrecognizedReleaseChannelError(unknown.to_string()));
      }
    })
  }
}
