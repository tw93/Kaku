use std::path::Path;

/// Document and media file extensions that should open with the macOS
/// default app instead of being forced into VS Code. Text and source files
/// are intentionally excluded so they still open in the editor.
pub const NON_TEXT_FILE_EXTENSIONS: &[&str] = &[
    "xls", "xlsx", "doc", "docx", "ppt", "pptx", "pages", "numbers", "key", "odt", "ods", "odp",
    "pdf", "png", "jpg", "jpeg", "gif", "bmp", "tiff", "tif", "heic", "heif", "webp", "icns",
    "psd", "mp3", "m4a", "wav", "aac", "flac", "mp4", "mov", "avi", "mkv", "m4v", "webm", "zip",
    "dmg", "pkg", "rar", "7z", "gz", "tar", "bz2", "xz", "iso",
];

/// Returns true when the file extension is in the non-text whitelist.
pub fn has_non_text_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| NON_TEXT_FILE_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// Returns true when Cmd+Click should route the file to the system default app.
pub fn should_open_with_default_app(path: &Path) -> bool {
    if has_non_text_extension(path) {
        return true;
    }

    #[cfg(target_os = "macos")]
    {
        if path.is_file() {
            return uti_indicates_non_text_file(path);
        }
    }

    false
}

#[cfg(target_os = "macos")]
fn objc_bool(value: cocoa::base::BOOL) -> bool {
    value != cocoa::base::NO
}

#[cfg(target_os = "macos")]
fn uti_indicates_non_text_file(path: &Path) -> bool {
    let Some(uttype) = uttype_for_path(path) else {
        return false;
    };

    if uttype_conforms_to_text_or_source(uttype) {
        return false;
    }

    uttype_conforms_to_known_non_text(uttype)
}

#[cfg(target_os = "macos")]
fn uttype_for_path(path: &Path) -> Option<cocoa::base::id> {
    use cocoa::base::{id, nil};
    use cocoa::foundation::NSString;
    use objc::{class, msg_send, sel, sel_impl};

    unsafe {
        if let Some(ext) = path.extension().and_then(|ext| ext.to_str()) {
            let ext_ns = NSString::alloc(nil).init_str(ext);
            let uttype: id = msg_send![class!(UTType), typeWithFilenameExtension: ext_ns];
            if uttype != nil {
                return Some(uttype);
            }
        }

        if !path.is_file() {
            return None;
        }

        let path_ns = NSString::alloc(nil).init_str(&path.to_string_lossy());
        let url: id = msg_send![class!(NSURL), fileURLWithPath: path_ns];
        let resource_key = NSString::alloc(nil).init_str("NSURLContentTypeKey");
        let keys: id = msg_send![class!(NSArray), arrayWithObject: resource_key];
        let mut error: id = nil;
        let values: id = msg_send![url, resourceValuesForKeys: keys error: &mut error];
        if values == nil {
            return None;
        }

        let identifier: id = msg_send![values, objectForKey: resource_key];
        if identifier == nil {
            return None;
        }

        let identifier = if msg_send![identifier, isKindOfClass: class!(NSString)] {
            identifier
        } else {
            let description: id = msg_send![identifier, description];
            if description == nil {
                return None;
            }
            description
        };

        let uttype: id = msg_send![class!(UTType), typeWithIdentifier: identifier];
        if uttype == nil {
            None
        } else {
            Some(uttype)
        }
    }
}

#[cfg(target_os = "macos")]
fn uttype_conforms_to_text_or_source(uttype: cocoa::base::id) -> bool {
    use cocoa::base::{id, nil};
    use objc::{class, msg_send, sel, sel_impl};

    unsafe {
        let plain_text: id = msg_send![class!(UTType), plainTextType];
        if plain_text != nil {
            let conforms: cocoa::base::BOOL = msg_send![uttype, conformsToType: plain_text];
            if objc_bool(conforms) {
                return true;
            }
        }

        let source_code: id = msg_send![class!(UTType), sourceCodeType];
        if source_code != nil {
            let conforms: cocoa::base::BOOL = msg_send![uttype, conformsToType: source_code];
            if objc_bool(conforms) {
                return true;
            }
        }

        let text: id = msg_send![class!(UTType), textType];
        if text != nil {
            let conforms: cocoa::base::BOOL = msg_send![uttype, conformsToType: text];
            return objc_bool(conforms);
        }

        false
    }
}

#[cfg(target_os = "macos")]
fn uttype_conforms_to_known_non_text(uttype: cocoa::base::id) -> bool {
    use cocoa::base::{id, nil};
    use cocoa::foundation::NSString;
    use objc::{class, msg_send, sel, sel_impl};

    const NON_TEXT_UTI_IDENTIFIERS: &[&str] = &[
        "public.image",
        "public.movie",
        "public.audio",
        "public.archive",
        "com.adobe.pdf",
        "org.openxmlformats.spreadsheetml.sheet",
        "org.openxmlformats.wordprocessingml.document",
        "org.openxmlformats.presentationml.presentation",
        "com.microsoft.excel",
        "com.microsoft.word",
        "com.microsoft.powerpoint",
    ];

    unsafe {
        for identifier in NON_TEXT_UTI_IDENTIFIERS {
            let identifier_ns = NSString::alloc(nil).init_str(identifier);
            let candidate: id = msg_send![class!(UTType), typeWithIdentifier: identifier_ns];
            if candidate != nil {
                let conforms: cocoa::base::BOOL = msg_send![uttype, conformsToType: candidate];
                if objc_bool(conforms) {
                    return true;
                }
            }
        }
    }

    false
}

#[cfg(target_os = "macos")]
pub fn open_with_default_app(path: &Path) -> anyhow::Result<bool> {
    use cocoa::base::{id, nil, YES};
    use cocoa::foundation::NSString;
    use objc::{class, msg_send, sel, sel_impl};

    let path_str = path.to_string_lossy();
    unsafe {
        let path_ns = NSString::alloc(nil).init_str(&path_str);
        let url: id = msg_send![class!(NSURL), fileURLWithPath: path_ns];
        let workspace: id = msg_send![class!(NSWorkspace), sharedWorkspace];
        let opened: cocoa::base::BOOL = msg_send![workspace, openURL: url];
        Ok(opened == YES)
    }
}

#[cfg(test)]
mod tests {
    use super::{has_non_text_extension, NON_TEXT_FILE_EXTENSIONS};
    use std::path::Path;

    #[test]
    fn non_text_extension_whitelist_covers_issue_regression() {
        assert!(has_non_text_extension(Path::new("/tmp/test.xlsx")));
        assert!(has_non_text_extension(Path::new("/tmp/test.PDF")));
        assert!(!has_non_text_extension(Path::new("/tmp/demo.rs")));
        assert!(!has_non_text_extension(Path::new("/tmp/README")));
    }

    #[test]
    fn non_text_extension_whitelist_entries_are_lowercase() {
        for ext in NON_TEXT_FILE_EXTENSIONS {
            assert!(
                ext.chars().all(|c| !c.is_ascii_uppercase()),
                "extension `{ext}` should be lowercase",
                ext = ext
            );
        }
    }
}
