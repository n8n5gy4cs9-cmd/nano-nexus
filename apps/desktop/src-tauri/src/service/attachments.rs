//! Reading files the user attached (dropped or picked). Choosing a file is the intent: it is read once
//! and no grant is created. M05's permission engine adds the full deny list; until then a small guard
//! refuses obviously sensitive locations.

use std::io::Read;
use std::path::{Component, Path};

use nexus_core::chat::{AttachmentInput, AttachmentKind};
use nexus_core::error::NexusError;

use super::Caller;

/// Largest text sent from one file.
pub const MAX_ATTACHMENT_BYTES: usize = 256 * 1024;
/// Files attached to one message.
pub const MAX_ATTACHMENTS: usize = 5;

const SENSITIVE_DIRS: &[&str] = &[
    ".ssh",
    ".gnupg",
    ".aws",
    ".kube",
    ".docker",
    "Keychains",
    ".password-store",
    ".config/gcloud",
];

/// Obviously sensitive locations (private keys, credential stores).
fn is_sensitive(path: &Path) -> bool {
    let comps: Vec<String> = path
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    let joined = comps.join("/");
    SENSITIVE_DIRS
        .iter()
        .any(|d| comps.iter().any(|c| c == d) || joined.contains(&format!("{d}/")))
}

/// Read one text file as an attachment: at most 256 KB (longer files are cut with a visible note),
/// UTF-8 (invalid bytes replaced), and never a binary file.
pub fn read_text_file(caller: &Caller, path: &str) -> Result<AttachmentInput, NexusError> {
    if !caller.is_desktop() {
        return Err(NexusError::ForbiddenRemote);
    }
    let canon = std::fs::canonicalize(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => NexusError::NotFound("file".into()),
        std::io::ErrorKind::PermissionDenied => NexusError::OsPermissionDenied(path.to_owned()),
        _ => NexusError::from(e),
    })?;
    if is_sensitive(&canon) {
        return Err(NexusError::PathDenied(canon.display().to_string()));
    }
    let meta = std::fs::metadata(&canon)?;
    if !meta.is_file() {
        return Err(NexusError::InvalidInput(
            "only files can be attached here (folders arrive with permissions)".into(),
        ));
    }
    let mut buf = Vec::with_capacity(MAX_ATTACHMENT_BYTES.min(meta.len() as usize) + 1);
    std::fs::File::open(&canon)?
        .take(MAX_ATTACHMENT_BYTES as u64)
        .read_to_end(&mut buf)?;
    if buf.iter().take(8192).any(|b| *b == 0) {
        return Err(NexusError::InvalidInput(
            "this looks like a binary file, not text".into(),
        ));
    }
    let mut text = String::from_utf8_lossy(&buf).into_owned();
    if meta.len() as usize > MAX_ATTACHMENT_BYTES {
        text.push_str(&format!(
            "\n[… truncated: the file is {} KB, the first {} KB are shown]",
            meta.len() / 1024,
            MAX_ATTACHMENT_BYTES / 1024
        ));
    }
    let label = canon
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".into());
    Ok(AttachmentInput {
        kind: AttachmentKind::File,
        path: Some(canon.display().to_string()),
        label,
        text: Some(text),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_core::error::ErrorCode;
    use nexus_core::settings::SharePolicy;

    fn code(r: Result<AttachmentInput, NexusError>) -> ErrorCode {
        r.unwrap_err().code()
    }

    #[test]
    fn reads_small_text_files() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("notes.md");
        std::fs::write(&p, "# héllo\nworld").unwrap();
        let a = read_text_file(&Caller::Desktop, p.to_str().unwrap()).unwrap();
        assert_eq!(
            (a.label.as_str(), a.text.as_deref()),
            ("notes.md", Some("# héllo\nworld"))
        );
        assert_eq!(a.kind, AttachmentKind::File);
    }

    #[test]
    fn large_files_are_cut_with_a_note() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("big.txt");
        std::fs::write(&p, "x".repeat(MAX_ATTACHMENT_BYTES + 5000)).unwrap();
        let t = read_text_file(&Caller::Desktop, p.to_str().unwrap())
            .unwrap()
            .text
            .unwrap();
        assert!(t.contains("truncated"));
        assert!(t.len() < MAX_ATTACHMENT_BYTES + 200);
    }

    #[test]
    fn refuses_binary_dirs_missing_sensitive_and_remote() {
        let d = tempfile::tempdir().unwrap();
        let bin = d.path().join("a.bin");
        std::fs::write(&bin, [1u8, 2, 0, 3]).unwrap();
        assert_eq!(
            code(read_text_file(&Caller::Desktop, bin.to_str().unwrap())),
            ErrorCode::InvalidInput
        );
        assert_eq!(
            code(read_text_file(&Caller::Desktop, d.path().to_str().unwrap())),
            ErrorCode::InvalidInput
        );
        assert_eq!(
            code(read_text_file(
                &Caller::Desktop,
                d.path().join("missing").to_str().unwrap()
            )),
            ErrorCode::NotFound
        );
        let ssh = d.path().join(".ssh");
        std::fs::create_dir(&ssh).unwrap();
        std::fs::write(ssh.join("id_rsa"), "PRIVATE").unwrap();
        assert_eq!(
            code(read_text_file(
                &Caller::Desktop,
                ssh.join("id_rsa").to_str().unwrap()
            )),
            ErrorCode::PathDenied
        );
        let remote = Caller::Remote {
            device_id: "d".into(),
            policy: SharePolicy::Full,
        };
        assert_eq!(
            code(read_text_file(&remote, bin.to_str().unwrap())),
            ErrorCode::ForbiddenRemote
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_to_sensitive_files_are_resolved_first() {
        let d = tempfile::tempdir().unwrap();
        let ssh = d.path().join(".ssh");
        std::fs::create_dir(&ssh).unwrap();
        std::fs::write(ssh.join("key"), "PRIVATE").unwrap();
        let link = d.path().join("innocent.txt");
        std::os::unix::fs::symlink(ssh.join("key"), &link).unwrap();
        assert_eq!(
            code(read_text_file(&Caller::Desktop, link.to_str().unwrap())),
            ErrorCode::PathDenied
        );
    }
}
