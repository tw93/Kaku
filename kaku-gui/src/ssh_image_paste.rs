//! Pasting a clipboard image while `ssh` is the pane's foreground job (#634).
//!
//! A TUI on the remote host (Claude Code, Codex) reads images from the remote
//! clipboard, which an SSH session does not have, so the Ctrl+V that local
//! image paste forwards finds nothing there. Instead the image is copied to
//! the remote host with `scp`, reusing the connection options the running
//! `ssh` was started with, and its remote path is pasted like typed text.
//! cmux does the same for SSH sessions it detects.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const UPLOAD_TIMEOUT: Duration = Duration::from_secs(60);

/// How to reach the host a running `ssh` is connected to, as `scp` arguments.
#[derive(Debug, PartialEq, Eq)]
pub struct SshTarget {
    /// The `ssh` binary's own directory, so the matching `scp` is used.
    pub scp: PathBuf,
    /// Options carried over from the `ssh` command line.
    pub options: Vec<String>,
    /// `[user@]host` as `scp` expects it before the `:path`.
    pub destination: String,
    /// The host name alone, for messages.
    pub host: String,
    /// The `ssh` process, so a late upload result is pasted only while that
    /// same session is still the pane's foreground job.
    pub pid: u32,
    /// The `ssh` process's working directory, where relative `-i`, `-F` and
    /// `-S` paths resolve.
    pub cwd: PathBuf,
}

/// Remote commands that start a further hop (another host, a container or
/// another user). The image would land on the first host where the TUI
/// cannot read it, so those sessions keep the plain Ctrl+V.
const HOP_COMMANDS: &[&str] = &[
    "ssh",
    "autossh",
    "mosh",
    "et",
    "docker",
    "podman",
    "nerdctl",
    "kubectl",
    "oc",
    "lxc",
    "incus",
    "distrobox",
    "toolbox",
    "vagrant",
    "sudo",
    "su",
    "doas",
    "runuser",
    "machinectl",
    "nsenter",
    "chroot",
];

impl SshTarget {
    /// The upload target when the foreground process is an interactive
    /// `ssh` session.
    pub fn from_process(executable: &Path, argv: &[String], pid: u32, cwd: &Path) -> Option<Self> {
        if executable.file_name()? != "ssh" {
            return None;
        }
        let sibling = executable.with_file_name("scp");
        let scp = if sibling.is_file() {
            sibling
        } else {
            PathBuf::from("/usr/bin/scp")
        };
        let mut target = Self::from_argv(argv.get(1..)?, scp)?;
        target.pid = pid;
        target.cwd = cwd.to_path_buf();
        Some(target)
    }

    fn from_argv(args: &[String], scp: PathBuf) -> Option<Self> {
        let mut options = vec![];
        let mut user = None;
        let mut port = None;
        let mut iter = args.iter();
        let mut destination = None;
        let mut command: Vec<&String> = vec![];

        // ssh reads options before the destination and again right after it
        // (`ssh devbox -p 2222`); the first plain word after the destination
        // starts the remote command.
        while let Some(arg) = iter.next() {
            if arg == "--" {
                if destination.is_none() {
                    destination = iter.next();
                }
                command.extend(iter.by_ref());
                break;
            }
            let Some(cluster) = arg.strip_prefix('-').filter(|c| !c.is_empty()) else {
                if destination.is_none() {
                    destination = Some(arg);
                    continue;
                }
                command.push(arg);
                command.extend(iter.by_ref());
                break;
            };
            for (idx, flag) in cluster.char_indices() {
                match flag {
                    // Options with a value, attached (-p22) or as the next word.
                    'B' | 'b' | 'c' | 'D' | 'E' | 'e' | 'F' | 'I' | 'i' | 'J' | 'L' | 'l' | 'm'
                    | 'O' | 'o' | 'P' | 'p' | 'Q' | 'R' | 'S' | 'W' | 'w' => {
                        let rest = &cluster[idx + flag.len_utf8()..];
                        let value = if rest.is_empty() {
                            iter.next()?.clone()
                        } else {
                            rest.to_string()
                        };
                        let as_option = |name: &str| ["-o".to_string(), format!("{name}={value}")];
                        match flag {
                            // Not a session that takes typed input.
                            'O' | 'Q' | 'W' => return None,
                            'F' | 'i' | 'J' | 'o' | 'c' => {
                                options.push(format!("-{flag}"));
                                options.push(value);
                            }
                            // The first -p and -l win, as in ssh.
                            'p' => {
                                port.get_or_insert(value);
                            }
                            'l' => {
                                user.get_or_insert(value);
                            }
                            // scp's -S names the ssh program; ssh's is the control socket.
                            'S' => options.extend(as_option("ControlPath")),
                            'b' => options.extend(as_option("BindAddress")),
                            'B' => options.extend(as_option("BindInterface")),
                            'm' => options.extend(as_option("MACs")),
                            'I' => options.extend(as_option("PKCS11Provider")),
                            // Selects `Match tagged` blocks in ssh_config.
                            'P' => options.extend(as_option("Tag")),
                            // Forwardings, escape char and log file do not
                            // affect reaching the host.
                            _ => {}
                        }
                        break;
                    }
                    '4' | '6' | 'C' => options.push(format!("-{flag}")),
                    'K' => options.extend([
                        "-o".to_string(),
                        "GSSAPIAuthentication=yes".to_string(),
                        "-o".to_string(),
                        "GSSAPIDelegateCredentials=yes".to_string(),
                    ]),
                    // Background, no-command, config-dump and subsystem modes.
                    'f' | 'N' | 'G' | 'V' | 's' => return None,
                    _ => {}
                }
            }
        }

        let starts_a_hop = command
            .iter()
            .flat_map(|arg| arg.split_whitespace())
            // Quotes and shell operators around a word: `bash -lc 'ssh inner'`.
            .map(|word| word.trim_matches(|c: char| "'\"`;&|(){}".contains(c)))
            .any(|word| HOP_COMMANDS.contains(&word.rsplit('/').next().unwrap_or(word)));
        if starts_a_hop {
            return None;
        }

        let destination = destination?;
        let (dest_user, host, dest_port) = split_destination(destination)?;
        if let Some(port) = port.or(dest_port) {
            options.push("-P".into());
            options.push(port);
        }
        let user = user.or(dest_user);
        let host_part = if host.contains(':') {
            format!("[{host}]")
        } else {
            host.clone()
        };
        let destination = match user {
            Some(user) => format!("{user}@{host_part}"),
            None => host_part,
        };
        Some(Self {
            scp,
            options,
            destination,
            host,
            pid: 0,
            cwd: PathBuf::new(),
        })
    }

    /// The `scp` arguments that copy `local` to `remote_path`. The local file
    /// is 0600 and `-p` keeps that mode on the remote copy.
    pub fn scp_args(&self, local: &Path, remote_path: &str) -> Vec<String> {
        // No -q: it also silences ssh's diagnostics, the reason the toast shows.
        let mut args: Vec<String> = ["-p", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        args.extend(self.options.iter().cloned());
        args.push("--".into());
        args.push(local.to_string_lossy().into_owned());
        args.push(format!("{}:{remote_path}", self.destination));
        args
    }

    /// Copies `local` to the remote temporary directory and returns the
    /// remote path. Blocks until `scp` exits or the timeout passes.
    pub fn upload(&self, local: &Path) -> anyhow::Result<String> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        let extension = local.extension().and_then(|e| e.to_str()).unwrap_or("png");
        let remote_path = format!("/tmp/kaku-clipboard-{nanos}.{extension}");
        run_with_timeout(
            &self.scp,
            &self.scp_args(local, &remote_path),
            &self.cwd,
            UPLOAD_TIMEOUT,
        )?;
        Ok(remote_path)
    }
}

/// `[user@]host[:port]` from an ssh destination, which is either
/// `[user@]host` or `ssh://[user@]host[:port]`.
fn split_destination(dest: &str) -> Option<(Option<String>, String, Option<String>)> {
    let (uri, rest) = match dest.strip_prefix("ssh://") {
        Some(rest) => (true, rest.trim_end_matches('/')),
        None => (false, dest),
    };
    let (user, host_port) = match rest.rsplit_once('@') {
        Some((user, host)) => (Some(user.to_string()), host),
        None => (None, rest),
    };
    let (host, port) = if !uri {
        (host_port.to_string(), None)
    } else if let Some(bracketed) = host_port.strip_prefix('[') {
        let (host, after) = bracketed.split_once(']')?;
        let port = after.strip_prefix(':').map(str::to_string);
        (host.to_string(), port)
    } else {
        match host_port.split_once(':') {
            Some((host, port)) => (host.to_string(), Some(port.to_string())),
            None => (host_port.to_string(), None),
        }
    };
    // A host never starts with '-' or holds '/'; either would turn the scp
    // target into an option or a local path.
    if host.is_empty() || host.starts_with('-') || host.contains('/') {
        return None;
    }
    Some((user, host, port))
}

fn run_with_timeout(
    program: &Path,
    args: &[String],
    cwd: &Path,
    timeout: Duration,
) -> anyhow::Result<()> {
    let mut command = Command::new(program);
    if cwd.is_dir() {
        command.current_dir(cwd);
    }
    let mut child = command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| anyhow::anyhow!("{}: {err}", program.display()))?;
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            if status.success() {
                return Ok(());
            }
            let mut stderr = String::new();
            if let Some(mut pipe) = child.stderr.take() {
                use std::io::Read;
                pipe.read_to_string(&mut stderr).ok();
            }
            let reason = stderr
                .lines()
                .map(str::trim)
                .find(|l| !l.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| format!("scp exited with {status}"));
            anyhow::bail!("{reason}");
        }
        if started.elapsed() > timeout {
            child.kill().ok();
            child.wait().ok();
            anyhow::bail!("timed out after {}s", timeout.as_secs());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(args: &[&str]) -> Option<SshTarget> {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        SshTarget::from_argv(&args, PathBuf::from("/usr/bin/scp"))
    }

    fn dest_and_options(args: &[&str]) -> Option<(String, Vec<String>)> {
        target(args).map(|t| (t.destination, t.options))
    }

    #[test]
    fn plain_destinations_keep_their_user() {
        assert_eq!(
            dest_and_options(&["devbox"]),
            Some(("devbox".into(), vec![]))
        );
        assert_eq!(
            dest_and_options(&["me@devbox", "-t", "tmux", "attach"]),
            Some(("me@devbox".into(), vec![]))
        );
        assert_eq!(
            dest_and_options(&["-l", "me", "devbox"]),
            Some(("me@devbox".into(), vec![]))
        );
    }

    #[test]
    fn connection_options_carry_over_to_scp() {
        let (dest, options) = dest_and_options(&[
            "-p2222",
            "-i",
            "~/.ssh/work",
            "-o",
            "IdentitiesOnly=yes",
            "-J",
            "bastion",
            "-tt",
            "-4A",
            "-S",
            "/tmp/ctl",
            "-L",
            "8080:localhost:80",
            "host",
        ])
        .unwrap();
        assert_eq!(dest, "host");
        assert_eq!(
            options,
            [
                "-i",
                "~/.ssh/work",
                "-o",
                "IdentitiesOnly=yes",
                "-J",
                "bastion",
                "-4",
                "-o",
                "ControlPath=/tmp/ctl",
                "-P",
                "2222",
            ]
        );
    }

    #[test]
    fn uri_and_ipv6_destinations_become_scp_targets() {
        assert_eq!(
            dest_and_options(&["ssh://me@host:2200"]),
            Some(("me@host".into(), vec!["-P".into(), "2200".into()]))
        );
        assert_eq!(
            dest_and_options(&["ssh://[::1]:2200/"]),
            Some(("[::1]".into(), vec!["-P".into(), "2200".into()]))
        );
        assert_eq!(
            dest_and_options(&["me@fe80::1"]),
            Some(("me@[fe80::1]".into(), vec![]))
        );
    }

    #[test]
    fn sessions_without_typed_input_are_skipped() {
        for args in [
            &["-N", "-L", "8080:localhost:80", "host"][..],
            &["-f", "host", "sleep", "60"],
            &["-W", "target:22", "bastion"],
            &["-O", "check", "host"],
            &["-G", "host"],
            &["-s", "host", "sftp"],
            &["-p"],
            &[],
            &["--", "-oProxyCommand=evil"],
            &["a/b"],
            // Read after the destination too.
            &["host", "-N", "-L", "8080:localhost:80"],
            &["host", "-W", "target:22"],
        ] {
            assert_eq!(target(args), None, "{args:?}");
        }
    }

    #[test]
    fn options_after_the_destination_still_apply() {
        let (dest, options) = dest_and_options(&["devbox", "-p", "2222", "-i", "key.pem"]).unwrap();
        assert_eq!(dest, "devbox");
        assert_eq!(options, ["-i", "key.pem", "-P", "2222"]);
        // The first plain word after the destination starts the command, so
        // its own flags are not ssh's.
        assert_eq!(
            dest_and_options(&["devbox", "tmux", "-p", "9"]),
            Some(("devbox".into(), vec![]))
        );
    }

    #[test]
    fn the_first_user_and_port_win_like_ssh() {
        assert_eq!(
            dest_and_options(&["-l", "alice", "bob@host"]),
            Some(("alice@host".into(), vec![]))
        );
        assert_eq!(
            dest_and_options(&["-p", "3333", "ssh://host:2200"]),
            Some(("host".into(), vec!["-P".into(), "3333".into()]))
        );
        assert_eq!(
            dest_and_options(&["-p", "1", "host", "-p", "2"]),
            Some(("host".into(), vec!["-P".into(), "1".into()]))
        );
    }

    #[test]
    fn tags_and_gssapi_reach_scp() {
        assert_eq!(
            dest_and_options(&["-K", "-P", "work", "host"]).unwrap().1,
            [
                "-o",
                "GSSAPIAuthentication=yes",
                "-o",
                "GSSAPIDelegateCredentials=yes",
                "-o",
                "Tag=work",
            ]
        );
    }

    #[test]
    fn a_further_hop_keeps_ctrl_v() {
        for args in [
            &["-t", "bastion", "ssh", "inner"][..],
            &["-t", "host", "sudo", "-iu", "svc", "claude"],
            &["-t", "host", "docker", "exec", "-it", "c", "claude"],
            &["-t", "host", "bash -lc 'ssh inner'"],
            &["-t", "host", "/usr/bin/ssh", "inner"],
        ] {
            assert_eq!(target(args), None, "{args:?}");
        }
        assert!(target(&["-t", "host", "tmux", "new", "-As", "main"]).is_some());
    }

    #[test]
    fn scp_args_end_the_options_before_the_paths() {
        let t = target(&["-p", "22", "me@host"]).unwrap();
        assert_eq!(
            t.scp_args(Path::new("/tmp/a.png"), "/tmp/kaku-clipboard-1.png"),
            [
                "-p",
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=10",
                "-P",
                "22",
                "--",
                "/tmp/a.png",
                "me@host:/tmp/kaku-clipboard-1.png",
            ]
        );
    }

    #[test]
    fn upload_reports_the_remote_path_or_the_first_error_line() {
        let dir = tempfile::tempdir().unwrap();
        let image = dir.path().join("clip.png");
        std::fs::write(&image, b"png").unwrap();
        let fake_scp = dir.path().join("scp");
        // Copies to <dir>/<remote path> so the test can see what arrived,
        // and records the directory it ran in.
        std::fs::write(
            &fake_scp,
            format!(
                "#!/bin/sh\npwd -P > '{root}/cwd'\nfor last; do :; done\n\
                 src=\"$(eval echo \\${{$(($#-1))}})\"\n\
                 mkdir -p '{root}/tmp' && cp \"$src\" '{root}'\"${{last#*:}}\"\n",
                root = dir.path().display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(
            &fake_scp,
            std::os::unix::fs::PermissionsExt::from_mode(0o755),
        )
        .unwrap();
        let ssh_cwd = dir.path().join("keys");
        std::fs::create_dir(&ssh_cwd).unwrap();
        let mut t = target(&["me@host"]).unwrap();
        t.scp = fake_scp.clone();
        t.cwd = ssh_cwd.clone();
        let remote = t.upload(&image).unwrap();
        // Relative -i/-F paths resolve where ssh was started.
        assert_eq!(
            std::fs::read_to_string(dir.path().join("cwd"))
                .unwrap()
                .trim(),
            std::fs::canonicalize(&ssh_cwd).unwrap().to_str().unwrap()
        );
        assert!(remote.starts_with("/tmp/kaku-clipboard-") && remote.ends_with(".png"));
        assert_eq!(
            std::fs::read(dir.path().join(remote.trim_start_matches('/'))).unwrap(),
            b"png"
        );

        std::fs::write(
            &fake_scp,
            "#!/bin/sh\necho >&2\necho 'me@host: Permission denied (publickey).' >&2\nexit 1\n",
        )
        .unwrap();
        let err = t.upload(&image).unwrap_err().to_string();
        assert_eq!(err, "me@host: Permission denied (publickey).");
    }
}
