fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    #[cfg(windows)]
    {
        use anyhow::Context as _;
        use std::io::Write;
        use std::path::Path;
        let profile = std::env::var("PROFILE").unwrap();
        let repo_dir = std::env::current_dir()
            .ok()
            .and_then(|cwd| cwd.parent().map(|p| p.to_path_buf()))
            .unwrap();
        let exe_output_dir = repo_dir.join("target").join(profile);
        let windows_dir = repo_dir.join("assets").join("windows");

        let conhost_dir = windows_dir.join("conhost");
        for name in &["conpty.dll", "OpenConsole.exe"] {
            let dest_name = exe_output_dir.join(name);
            let src_name = conhost_dir.join(name);

            if !dest_name.exists() {
                std::fs::copy(&src_name, &dest_name)
                    .context(format!(
                        "copy {} -> {}",
                        src_name.display(),
                        dest_name.display()
                    ))
                    .unwrap();
            }
        }

        let angle_dir = windows_dir.join("angle");
        for name in &["libEGL.dll", "libGLESv2.dll"] {
            let dest_name = exe_output_dir.join(name);
            let src_name = angle_dir.join(name);

            if !dest_name.exists() {
                std::fs::copy(&src_name, &dest_name)
                    .context(format!(
                        "copy {} -> {}",
                        src_name.display(),
                        dest_name.display()
                    ))
                    .unwrap();
            }
        }

        {
            let dest_mesa = exe_output_dir.join("mesa");
            let _ = std::fs::create_dir(&dest_mesa);
            let dest_name = dest_mesa.join("opengl32.dll");
            let src_name = windows_dir.join("mesa").join("opengl32.dll");
            if !dest_name.exists() {
                std::fs::copy(&src_name, &dest_name)
                    .context(format!(
                        "copy {} -> {}",
                        src_name.display(),
                        dest_name.display()
                    ))
                    .unwrap();
            }
        }

        // If a file named `.tag` is present, we'll take its contents for the
        // version number that we report in wezterm -h.
        let mut ci_tag = String::new();
        if let Ok(tag) = std::fs::read("../.tag") {
            if let Ok(s) = String::from_utf8(tag) {
                ci_tag = s.trim().to_string();
                println!("cargo:rerun-if-changed=../.tag");
            }
        }
        let version = if ci_tag.is_empty() {
            let mut cmd = std::process::Command::new("git");
            cmd.args(&[
                "-c",
                "core.abbrev=8",
                "show",
                "-s",
                "--format=%cd-%h",
                "--date=format:%Y%m%d-%H%M%S",
            ]);
            if let Ok(output) = cmd.output() {
                if output.status.success() {
                    String::from_utf8_lossy(&output.stdout).trim().to_owned()
                } else {
                    "UNKNOWN".to_owned()
                }
            } else {
                "UNKNOWN".to_owned()
            }
        } else {
            ci_tag
        };

        let rcfile_name = Path::new(&std::env::var_os("OUT_DIR").unwrap()).join("resource.rc");
        let mut rcfile = std::fs::File::create(&rcfile_name).unwrap();
        println!("cargo:rerun-if-changed=../assets/windows/terminal.ico");
        write!(
            rcfile,
            r#"
#include <winres.h>
// This ID is coupled with code in window/src/os/windows/window.rs
#define IDI_ICON 0x101
1 RT_MANIFEST "{win}\\manifest.manifest"
IDI_ICON ICON "{win}\\terminal.ico"
VS_VERSION_INFO VERSIONINFO
FILEVERSION     1,0,0,0
PRODUCTVERSION  1,0,0,0
FILEFLAGSMASK   VS_FFI_FILEFLAGSMASK
FILEFLAGS       0
FILEOS          VOS__WINDOWS32
FILETYPE        VFT_APP
FILESUBTYPE     VFT2_UNKNOWN
BEGIN
    BLOCK "StringFileInfo"
    BEGIN
        BLOCK "040904E4"
        BEGIN
            VALUE "CompanyName",      "Tw93\\0"
            VALUE "FileDescription",  "Kaku - A fast terminal for AI coding\\0"
            VALUE "FileVersion",      "{version}\\0"
            VALUE "LegalCopyright",   "Tw93, MIT licensed\\0"
            VALUE "InternalName",     "\\0"
            VALUE "OriginalFilename", "\\0"
            VALUE "ProductName",      "Kaku\\0"
            VALUE "ProductVersion",   "{version}\\0"
        END
    END
    BLOCK "VarFileInfo"
    BEGIN
        VALUE "Translation", 0x409, 1252
    END
END
"#,
            win = windows_dir.display().to_string().replace("\\", "\\\\"),
            version = version,
        )
        .unwrap();
        drop(rcfile);

        // Obtain MSVC environment so that the rc compiler can find the right headers.
        // https://github.com/nabijaczleweli/rust-embed-resource/issues/11#issuecomment-603655972
        let target = std::env::var("TARGET").unwrap();
        if let Some(tool) = cc::windows_registry::find_tool(target.as_str(), "cl.exe") {
            for (key, value) in tool.env() {
                std::env::set_var(key, value);
            }
        }
        embed_resource::compile(rcfile_name);
    }

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
        println!("cargo:rerun-if-changed=assets/macos/Kaku.app/Contents/Info.plist");

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
