use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=WIREHUB_BUILD_ID");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/index");
    let git = || -> Option<String> {
        let output = Command::new("git")
            .args(["rev-parse", "--short=12", "HEAD"])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let revision = String::from_utf8(output.stdout).ok()?.trim().to_owned();
        let dirty = Command::new("git")
            .args(["diff", "--quiet", "HEAD"])
            .status()
            .ok()
            .is_some_and(|status| !status.success());
        Some(format!("{revision}{}", if dirty { "-dirty" } else { "" }))
    };
    let id = std::env::var("WIREHUB_BUILD_ID")
        .ok()
        .filter(|id| !id.is_empty())
        .or_else(git)
        .unwrap_or_else(|| "unknown".into());
    assert!(
        id.len() <= 128
            && id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c)),
        "WIREHUB_BUILD_ID must contain at most 128 letters, digits, '.', '_' or '-'"
    );
    println!("cargo:rustc-env=WIREHUB_BUILD_ID={id}");
}
