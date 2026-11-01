fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    #[cfg(target_os = "macos")]
    {
        println!("cargo:rustc-link-lib=framework=UniformTypeIdentifiers");
        use anyhow::Context as _;
        let repo_dir = std::env::current_dir()
            .ok()
            .and_then(|cwd| cwd.parent().map(|p| p.to_path_buf()))
            .unwrap();

        // We need to copy the plist to avoid the UNUserNotificationCenter asserting
        // due to not finding the application bundle
        let src_plist = repo_dir
            .join("assets")
            .join("macos")
            .join("Kaku.app")
            .join("Contents")
            .join("Info.plist");

        // Determine the target directory where the binary will be placed
        // Priority: CARGO_TARGET_DIR > derive from OUT_DIR > fallback to target/release
        let build_target_dir = if let Ok(target_dir) = std::env::var("CARGO_TARGET_DIR") {
            std::path::PathBuf::from(target_dir)
        } else {
            // OUT_DIR is like: target/release-opt/build/kaku-gui-xxx/out
            // We need to get to: target/release-opt
            let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR not set");
            let out_path = std::path::PathBuf::from(&out_dir);

            // Navigate up: out -> build -> kaku-gui-xxx -> release-opt
            let mut target = out_path.clone();
            for _ in 0..3 {
                if let Some(parent) = target.parent() {
                    target = parent.to_path_buf();
                } else {
                    break;
                }
            }

            // Verify this looks like a target directory
            if target.file_name().is_some_and(|f| {
                let s = f.to_string_lossy();
                s == "release" || s == "debug" || s == "release-opt" || s.ends_with("-opt")
            }) {
                target
            } else {
                eprintln!(
                    "Warning: Could not derive target dir from OUT_DIR={}, using fallback",
                    out_dir
                );
                repo_dir.join("target").join("release")
            }
        };

        let dest_plist = build_target_dir.join("Info.plist");
        // Cargo resolves a relative rerun path from kaku-gui/, where this file does not
        // exist, and a missing path makes the build script stale on every build.
        println!("cargo:rerun-if-changed={}", src_plist.display());

        // Ensure the destination directory exists before copying
        if let Some(parent) = dest_plist.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        std::fs::copy(&src_plist, &dest_plist)
            .context(format!(
                "copy {} -> {}",
                src_plist.display(),
                dest_plist.display()
            ))
            .unwrap();
    }
}
