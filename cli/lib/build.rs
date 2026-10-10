// Copyright 2018-2026 the Deno authors. MIT license.

fn main() {
  let commit_hash = git_commit_hash();
  println!("cargo:rerun-if-env-changed=GIT_COMMIT_HASH");
  println!("cargo:rustc-env=GIT_COMMIT_HASH={}", commit_hash);
  // rjs has no semantic version numbers: the sha of the commit this binary was
  // built from *is* the version.
  println!("cargo:rustc-env=DENO_VERSION={}", commit_hash);
}

fn git_commit_hash() -> String {
  if let Ok(output) = std::process::Command::new("git")
    .arg("rev-list")
    .arg("-1")
    .arg("HEAD")
    .output()
  {
    if output.status.success() {
      std::str::from_utf8(&output.stdout[..40])
        .unwrap()
        .to_string()
    } else {
      // When not in git repository
      // (e.g. when the user install by `cargo install deno`)
      "UNKNOWN".to_string()
    }
  } else {
    // When there is no git command for some reason
    "UNKNOWN".to_string()
  }
}
