//! Pasting a clipboard image while `ssh` is the pane's foreground job (#634).
//!
//! A TUI on the remote host (Claude Code, Codex) reads images from the remote
//! clipboard, which an SSH session does not have, so the Ctrl+V that local
//! image paste forwards finds nothing there. Instead the image is copied to
//! the remote host with `scp`, reusing the connection options the running
//! `ssh` was started with, and its remote path is pasted like typed text.
//! cmux does the same for SSH sessions it detects.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const UPLOAD_TIMEOUT: Duration = Duration::from_secs(30);
/// `ssh -G` only reads configuration; it never connects.
const CONFIG_TIMEOUT: Duration = Duration::from_secs(5);
/// An upload still running after this long gets a toast, so a slow host or a
/// key waiting for a touch is not mistaken for a paste that did nothing.
pub const SLOW_UPLOAD_NOTICE: Duration = Duration::from_secs(1);
/// A failure that took longer than this no longer sends the fallback Ctrl+V:
/// by then the user may be typing something else, where a stray Ctrl+V is
/// bash's quoted-insert or vim's literal-next.
pub const LATE_FALLBACK: Duration = Duration::from_secs(3);

/// How to reach the host a running `ssh` is connected to, as `scp` arguments.
#[derive(Debug, PartialEq, Eq)]
pub struct SshTarget {
    /// The running `ssh` binary, asked for its effective configuration.
    pub ssh: PathBuf,
    /// The arguments that `ssh` was started with.
    pub args: Vec<String>,
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
        target.ssh = executable.to_path_buf();
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
            ssh: PathBuf::new(),
            args: args.to_vec(),
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

    /// True when ssh_config gives this session a `RemoteCommand` or a
    /// non-shell session type, which the command line does not show: the
    /// host then runs something else first (`tmux attach`, a further `ssh`),
    /// so the image may not land where the TUI reads it. Blocks while
    /// `ssh -G` reads the configuration.
    pub fn configured_remote_command(&self) -> bool {
        let mut args = vec!["-G".to_string()];
        args.extend(self.args.iter().cloned());
        match run_with_timeout(&self.ssh, &args, &self.cwd, CONFIG_TIMEOUT, || {}) {
            Ok(config) => config_runs_a_command(&config),
            Err(err) => {
                log::debug!("ssh -G for {} failed: {err:#}", self.host);
                false
            }
        }
    }

    /// Copies `local` to the remote temporary directory and returns the
    /// remote path. Blocks until `scp` exits or the timeout passes; `on_slow`
    /// runs once if that takes longer than `SLOW_UPLOAD_NOTICE`.
    pub fn upload(&self, local: &Path, on_slow: impl FnOnce()) -> anyhow::Result<String> {
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
            on_slow,
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

/// Whether `ssh -G` output sets a remote command or a non-shell session.
fn config_runs_a_command(config: &str) -> bool {
    config.lines().any(|line| {
        let (key, value) = line.split_once(' ').unwrap_or((line, ""));
        let value = value.trim();
        match key.to_ascii_lowercase().as_str() {
            "remotecommand" => !value.is_empty() && !value.eq_ignore_ascii_case("none"),
            "sessiontype" => !value.eq_ignore_ascii_case("default"),
            _ => false,
        }
    })
}

/// The line of `scp`'s stderr that says why it failed. ssh prints notices
/// before the real error (an sshd `Banner`, OpenSSH 10's post-quantum
/// warning, a new known_hosts entry) and scp ends with a generic
/// `Connection closed`, so neither the first nor the last line will do.
fn failure_reason(stderr: &str) -> Option<String> {
    let lines: Vec<&str> = stderr
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| !line.starts_with("**"))
        .filter(|line| !line.starts_with("Warning: Permanently added"))
        // `LogLevel DEBUG` in ssh_config.
        .filter(|line| !line.starts_with("debug1:") && !line.starts_with("debug2:"))
        .filter(|line| !line.starts_with("debug3:"))
        .collect();
    let generic = |line: &str| {
        let line = line.trim_start_matches("scp: ");
        line.eq_ignore_ascii_case("Connection closed")
            || line.eq_ignore_ascii_case("lost connection")
    };
    lines
        .iter()
        .rev()
        .find(|line| !generic(line))
        .or_else(|| lines.last())
        .map(|line| line.to_string())
}

/// Reads a child's pipe on its own thread. Waiting for the child before
/// reading would let a chatty `LogLevel DEBUG3` fill the pipe and stall
/// scp; reading to the end on the waiting thread would hang when a
/// `ProxyCommand` leaves a process holding the pipe open.
struct PipeReader {
    buffer: Arc<Mutex<(Vec<u8>, bool)>>,
}

impl PipeReader {
    fn spawn(mut pipe: impl Read + Send + 'static) -> Self {
        let buffer = Arc::new(Mutex::new((Vec::new(), false)));
        let shared = Arc::clone(&buffer);
        std::thread::spawn(move || {
            let mut chunk = [0u8; 4096];
            loop {
                match pipe.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let mut buffer = shared.lock().unwrap();
                        buffer.0.extend_from_slice(&chunk[..n]);
                        // Keep the tail, where the reason is, and only drain
                        // the rest.
                        if buffer.0.len() > 128 * 1024 {
                            let excess = buffer.0.len() - 64 * 1024;
                            buffer.0.drain(..excess);
                        }
                    }
                }
            }
            shared.lock().unwrap().1 = true;
        });
        Self { buffer }
    }

    /// What was read, waiting briefly for the end of the stream.
    fn take(self) -> String {
        let deadline = Instant::now() + Duration::from_millis(500);
        while !self.buffer.lock().unwrap().1 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        let buffer = self.buffer.lock().unwrap();
        String::from_utf8_lossy(&buffer.0).into_owned()
    }
}

/// The PATH a child gets: Kaku's own, plus the usual Homebrew and user bin
/// directories. A Kaku opened from the Dock has launchd's minimal PATH, and
/// a `ProxyCommand` or `Match exec` in ssh_config often names a Homebrew
/// tool (`cloudflared`, `tsh`, `aws`).
fn child_path() -> String {
    let mut dirs: Vec<String> = std::env::var("PATH")
        .unwrap_or_default()
        .split(':')
        .filter(|dir| !dir.is_empty())
        .map(str::to_string)
        .collect();
    let mut extra = vec![];
    if let Some(home) = dirs_next::home_dir() {
        extra.push(home.join(".local/bin").to_string_lossy().into_owned());
    }
    extra.extend(["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"].map(str::to_string));
    for dir in extra {
        if !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    dirs.join(":")
}

/// Runs `program` and returns its stdout. Fails with the reason from its
/// stderr when it exits unsuccessfully, or when `timeout` passes.
fn run_with_timeout(
    program: &Path,
    args: &[String],
    cwd: &Path,
    timeout: Duration,
    on_slow: impl FnOnce(),
) -> anyhow::Result<String> {
    let mut command = Command::new(program);
    if cwd.is_dir() {
        command.current_dir(cwd);
    }
    let mut child = command
        .args(args)
        .env("PATH", child_path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| anyhow::anyhow!("{}: {err}", program.display()))?;
    let stdout = child.stdout.take().map(PipeReader::spawn);
    let stderr = child.stderr.take().map(PipeReader::spawn);
    let started = Instant::now();
    let mut on_slow = Some(on_slow);
    loop {
        if let Some(status) = child.try_wait()? {
            if status.success() {
                return Ok(stdout.map(PipeReader::take).unwrap_or_default());
            }
            let stderr = stderr.map(PipeReader::take).unwrap_or_default();
            let reason = failure_reason(&stderr)
                .unwrap_or_else(|| format!("{} exited with {status}", program.display()));
            anyhow::bail!("{reason}");
        }
        let elapsed = started.elapsed();
        if elapsed > timeout {
            child.kill().ok();
            child.wait().ok();
            anyhow::bail!("timed out after {}s", timeout.as_secs());
        }
        if elapsed > SLOW_UPLOAD_NOTICE {
            if let Some(notice) = on_slow.take() {
                notice();
            }
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
    fn upload_reports_the_remote_path_or_why_it_failed() {
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
        let remote = t
            .upload(&image, || panic!("a quick copy needs no notice"))
            .unwrap();
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

        // OpenSSH 10 warns before the real error, and scp closes with a
        // generic line after it.
        std::fs::write(
            &fake_scp,
            "#!/bin/sh\necho >&2\n\
             echo '** WARNING: connection is not using a post-quantum key exchange algorithm.' >&2\n\
             echo '** This session may be vulnerable to \"store now, decrypt later\" attacks.' >&2\n\
             echo 'Welcome to the build farm' >&2\n\
             echo 'me@host: Permission denied (publickey).' >&2\n\
             echo 'scp: Connection closed' >&2\nexit 1\n",
        )
        .unwrap();
        let err = t.upload(&image, || {}).unwrap_err().to_string();
        assert_eq!(err, "me@host: Permission denied (publickey).");

        // More stderr than a pipe holds must not stall scp until the timeout.
        std::fs::write(
            &fake_scp,
            "#!/bin/sh\nyes 'debug3: chatty' | head -c 300000 >&2\n\
             echo 'ssh: connect to host host port 22: Connection refused' >&2\nexit 255\n",
        )
        .unwrap();
        let started = Instant::now();
        let err = t.upload(&image, || {}).unwrap_err().to_string();
        assert!(started.elapsed() < Duration::from_secs(10), "{}", err);
        assert_eq!(err, "ssh: connect to host host port 22: Connection refused");

        // A slow copy announces itself once.
        std::fs::write(&fake_scp, "#!/bin/sh\nsleep 1.5\nexit 1\n").unwrap();
        let mut notices = 0;
        t.upload(&image, || notices += 1).unwrap_err();
        assert_eq!(notices, 1);
    }

    #[test]
    fn failure_reason_skips_notices_and_the_closing_line() {
        assert_eq!(failure_reason(""), None);
        assert_eq!(
            failure_reason("scp: Connection closed\n").as_deref(),
            Some("scp: Connection closed")
        );
        assert_eq!(
            failure_reason(
                "Warning: Permanently added 'h' (ED25519) to the list of known hosts.\n\
                 ssh: connect to host h port 22: Operation timed out\nlost connection\n"
            )
            .as_deref(),
            Some("ssh: connect to host h port 22: Operation timed out")
        );
    }

    #[test]
    fn a_configured_remote_command_keeps_ctrl_v() {
        assert!(!config_runs_a_command(
            "user me\nhostname h\nrequesttty auto\nsessiontype default\n"
        ));
        assert!(config_runs_a_command(
            "sessiontype default\nremotecommand tmux attach\n"
        ));
        assert!(config_runs_a_command("sessiontype none\n"));
        assert!(config_runs_a_command("sessiontype subsystem\n"));

        // Asks the session's own ssh, with the arguments it started with.
        let dir = tempfile::tempdir().unwrap();
        let fake_ssh = dir.path().join("ssh");
        std::fs::write(
            &fake_ssh,
            format!(
                "#!/bin/sh\necho \"$@\" > '{}/args'\n\
                 echo 'sessiontype default'\necho 'remotecommand ssh inner'\n",
                dir.path().display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(
            &fake_ssh,
            std::os::unix::fs::PermissionsExt::from_mode(0o755),
        )
        .unwrap();
        let mut t = target(&["-p", "2222", "bastion"]).unwrap();
        t.ssh = fake_ssh;
        assert!(t.configured_remote_command());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("args"))
                .unwrap()
                .trim(),
            "-G -p 2222 bastion"
        );
    }
}
