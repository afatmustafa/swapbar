//! First-run plumbing: finding `claude`, `cswap`, `uv` and `brew`, installing claude-swap,
//! and signing Claude Code in to another account. The onboarding window drives these;
//! everything here blocks, so call it from the background executor.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};

use serde::Deserialize;

use crate::i18n::l;
use crate::lf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Claude,
    Cswap,
    Uv,
    Brew,
}

impl Tool {
    fn name(self) -> &'static str {
        match self {
            Tool::Claude => "claude",
            Tool::Cswap => "cswap",
            Tool::Uv => "uv",
            Tool::Brew => "brew",
        }
    }

    /// File names to look for. On Windows npm installs Claude Code as `claude.cmd`
    /// (its extensionless `claude` is a shell script), and there is no Homebrew.
    fn file_names(self) -> Vec<String> {
        if cfg!(target_os = "windows") {
            match self {
                Tool::Brew => vec![],
                Tool::Claude => vec!["claude.exe".into(), "claude.cmd".into()],
                t => vec![format!("{}.exe", t.name())],
            }
        } else {
            vec![self.name().to_string()]
        }
    }

    /// `CLAUDE_BIN`, `CSWAP_BIN`, … point at a specific binary (tests, odd installs).
    fn env_override(self) -> String {
        format!("{}_BIN", self.name().to_uppercase())
    }
}

/// The home folder as cswap and Claude Code see it. A Windows GUI app usually has no
/// HOME, and both tools use USERPROFILE there anyway.
pub fn home() -> PathBuf {
    let var = if cfg!(target_os = "windows") {
        "USERPROFILE"
    } else {
        "HOME"
    };
    PathBuf::from(std::env::var_os(var).unwrap_or_default())
}

/// Where installers put these tools. Apps started from Finder (or at Windows sign-in) get
/// a bare PATH, so we look in the usual places ourselves, then in PATH.
fn search_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![
        home().join(".local").join("bin"),
        home().join(".claude").join("local"),
        home().join(".cargo").join("bin"),
    ];
    if cfg!(target_os = "windows") {
        let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
        dirs.extend(var("APPDATA").map(|d| d.join("npm")));
        dirs.extend(var("LOCALAPPDATA").map(|d| d.join("Microsoft").join("WinGet").join("Links")));
    } else {
        dirs.extend([
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/usr/local/bin"),
        ]);
    }
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    dirs
}

pub fn find(tool: Tool) -> Option<PathBuf> {
    if let Ok(p) = std::env::var(tool.env_override()) {
        let p = PathBuf::from(p);
        return p.exists().then_some(p);
    }
    let names = tool.file_names();
    search_dirs()
        .into_iter()
        .flat_map(|d| names.iter().map(move |n| d.join(n)))
        .find(|p| p.is_file())
}

/// A command with a PATH that includes the install locations, so tools that call each
/// other (uv → python, brew → git) work from an app started by launchd.
pub fn command(program: &Path) -> Command {
    let mut cmd = Command::new(program);
    if let Ok(path) = std::env::join_paths(search_dirs()) {
        cmd.env("PATH", path);
    }
    // Python (cswap, uv's tools) would write the console code page, not UTF-8, to a pipe
    cmd.env("PYTHONUTF8", "1").env("PYTHONIOENCODING", "utf-8");
    no_console(&mut cmd);
    cmd
}

/// Spawned from a GUI app, a console program opens a console window of its own on Windows.
pub fn no_console(cmd: &mut Command) {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(target_os = "windows"))]
    let _ = cmd;
}

/// The system curl: /usr/bin/curl on macOS, System32's curl.exe on Windows 10 and later.
pub fn curl() -> Command {
    let program = if cfg!(target_os = "windows") {
        let root = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
        PathBuf::from(root).join("System32").join("curl.exe")
    } else {
        PathBuf::from("/usr/bin/curl")
    };
    let mut cmd = Command::new(program);
    no_console(&mut cmd);
    cmd
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Tools {
    pub claude: Option<PathBuf>,
    pub cswap: Option<PathBuf>,
    /// `cswap --version`, e.g. "0.26.0".
    pub cswap_version: Option<String>,
    pub uv: Option<PathBuf>,
    pub brew: Option<PathBuf>,
}

pub fn detect() -> Tools {
    let cswap = find(Tool::Cswap);
    let cswap_version = cswap.as_ref().and_then(|p| {
        let out = command(p).arg("--version").output().ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        text.split_whitespace().last().map(str::to_string)
    });
    Tools {
        claude: find(Tool::Claude),
        cswap,
        cswap_version,
        uv: find(Tool::Uv),
        brew: find(Tool::Brew),
    }
}

/// Astral's installer; it puts `uv` in ~/.local/bin.
#[cfg(not(target_os = "windows"))]
pub const UV_INSTALLER: &str = "curl -LsSf https://astral.sh/uv/install.sh | sh";
#[cfg(not(target_os = "windows"))]
pub const CLAUDE_INSTALLER: &str = "curl -fsSL https://claude.ai/install.sh | bash";
/// The PowerShell installers; uv goes to %USERPROFILE%\.local\bin there too.
#[cfg(target_os = "windows")]
pub const UV_INSTALLER: &str = "irm https://astral.sh/uv/install.ps1 | iex";
#[cfg(target_os = "windows")]
pub const CLAUDE_INSTALLER: &str = "irm https://claude.ai/install.ps1 | iex";

/// The shell that runs the installers.
fn installer_shell(script: &str) -> Command {
    if cfg!(target_os = "windows") {
        let mut cmd = command(Path::new("powershell.exe"));
        cmd.args([
            "-NoProfile",
            "-ExecutionPolicy",
            "ByPass",
            "-Command",
            script,
        ]);
        cmd
    } else {
        let mut cmd = command(Path::new("/bin/sh"));
        cmd.args(["-c", script]);
        cmd
    }
}

/// The shell commands `install_cswap` will run, for the window to show up front.
pub fn install_plan(tools: &Tools) -> Vec<String> {
    let mut steps = Vec::new();
    if tools.uv.is_none() {
        steps.push(match tools.brew {
            Some(_) => "brew install uv".to_string(),
            None => UV_INSTALLER.to_string(),
        });
    }
    steps.push("uv tool install claude-swap".to_string());
    steps
}

/// Log sink shared by the stdout and stderr readers.
pub type Log<'a> = &'a (dyn Fn(String) + Sync);

/// Run `cmd`, sending each output line to `log` as it arrives. Errors name the command.
fn run_logged(mut cmd: Command, label: &str, log: Log) -> Result<(), String> {
    log(format!("$ {label}"));
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("{label}: {e}"))?;
    let (out, err) = (child.stdout.take(), child.stderr.take());
    // uv and brew report progress on stderr, so read both streams at once
    std::thread::scope(|scope| {
        if let Some(err) = err {
            scope.spawn(move || {
                for line in BufReader::new(err).lines().map_while(Result::ok) {
                    log(line);
                }
            });
        }
        if let Some(out) = out {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                log(line);
            }
        }
    });
    let status = child.wait().map_err(|e| format!("{label}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(lf!(
            "`{label}` failed ({status})",
            "`{label}` başarısız oldu ({status})"
        ))
    }
}

/// Install uv if needed, then claude-swap with it.
pub fn install_cswap(log: Log) -> Result<(), String> {
    let tools = detect();
    let uv = match tools.uv {
        Some(uv) => uv,
        None => {
            match tools.brew {
                Some(brew) => {
                    let mut cmd = command(&brew);
                    cmd.args(["install", "uv"]);
                    run_logged(cmd, "brew install uv", log)?;
                }
                None => run_logged(installer_shell(UV_INSTALLER), UV_INSTALLER, log)?,
            }
            find(Tool::Uv).ok_or(l(
                "uv was installed but can't be found",
                "uv kuruldu ama bulunamıyor",
            ))?
        }
    };
    let mut cmd = command(&uv);
    cmd.args(["tool", "install", "claude-swap"]);
    run_logged(cmd, "uv tool install claude-swap", log)?;
    if find(Tool::Cswap).is_none() {
        return Err(l(
            "claude-swap was installed but `cswap` can't be found",
            "claude-swap kuruldu ama `cswap` bulunamıyor",
        )
        .into());
    }
    Ok(())
}

// ---- Claude Code sign-in -------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AuthStatus {
    #[serde(default)]
    pub logged_in: bool,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub org_name: Option<String>,
}

/// Who Claude Code is signed in as (`claude auth status --json`).
pub fn auth_status() -> Result<AuthStatus, String> {
    let claude =
        find(Tool::Claude).ok_or(l("Claude Code isn't installed", "Claude Code kurulu değil"))?;
    let out = command(&claude)
        .args(["auth", "status", "--json"])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("claude auth status: {e}"))?;
    parse_auth_status(&String::from_utf8_lossy(&out.stdout))
}

fn parse_auth_status(raw: &str) -> Result<AuthStatus, String> {
    serde_json::from_str(raw.trim()).map_err(|e| format!("claude auth status: {e}"))
}

/// A running `claude auth login`. It opens the browser and exits once the sign-in
/// completes; if the browser can't hand the code back, the user pastes it instead.
pub struct Login {
    child: Mutex<Option<Child>>,
    stdin: Mutex<Option<ChildStdin>>,
    /// The sign-in page, once claude has printed it.
    pub url: Arc<Mutex<Option<String>>>,
}

impl Login {
    pub fn start(email: Option<&str>) -> Result<Login, String> {
        let claude = find(Tool::Claude)
            .ok_or(l("Claude Code isn't installed", "Claude Code kurulu değil"))?;
        let mut cmd = command(&claude);
        cmd.args(["auth", "login", "--claudeai"]);
        if let Some(email) = email {
            cmd.args(["--email", email]);
        }
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("claude auth login: {e}"))?;
        let url = Arc::new(Mutex::new(None));
        for stream in [
            child
                .stdout
                .take()
                .map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
            child
                .stderr
                .take()
                .map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
        ]
        .into_iter()
        .flatten()
        {
            let url = url.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(stream).lines().map_while(Result::ok) {
                    if let Some(found) = sign_in_url(&line) {
                        *url.lock().unwrap() = Some(found);
                    }
                }
            });
        }
        Ok(Login {
            stdin: Mutex::new(child.stdin.take()),
            child: Mutex::new(Some(child)),
            url,
        })
    }

    /// Hand claude the code from the browser's fallback page.
    pub fn paste_code(&self, code: &str) -> Result<(), String> {
        let mut stdin = self.stdin.lock().unwrap();
        let pipe = stdin
            .as_mut()
            .ok_or(l("sign-in is no longer running", "giriş artık çalışmıyor"))?;
        writeln!(pipe, "{}", code.trim()).map_err(|e| e.to_string())
    }

    /// Block until claude exits; `Ok` when it signed in.
    pub fn wait(&self) -> Result<(), String> {
        loop {
            {
                let mut guard = self.child.lock().unwrap();
                let Some(child) = guard.as_mut() else {
                    return Err(l("Sign-in was cancelled", "Giriş iptal edildi").into());
                };
                if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                    *guard = None;
                    return if status.success() {
                        Ok(())
                    } else {
                        Err(lf!(
                            "Sign-in didn't finish ({status})",
                            "Giriş tamamlanmadı ({status})"
                        ))
                    };
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
    }

    pub fn cancel(&self) {
        if let Some(mut child) = self.child.lock().unwrap().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for Login {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// "If the browser didn't open, visit: https://…" → the URL.
fn sign_in_url(line: &str) -> Option<String> {
    let start = line.find("https://")?;
    let url = line[start..].split_whitespace().next()?;
    Some(url.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_plan_installs_uv_only_when_missing() {
        let mut t = Tools::default();
        assert_eq!(
            install_plan(&t),
            [UV_INSTALLER, "uv tool install claude-swap"]
        );
        t.brew = Some("/opt/homebrew/bin/brew".into());
        assert_eq!(
            install_plan(&t),
            ["brew install uv", "uv tool install claude-swap"]
        );
        t.uv = Some("/opt/homebrew/bin/uv".into());
        assert_eq!(install_plan(&t), ["uv tool install claude-swap"]);
    }

    #[test]
    fn parses_auth_status() {
        let s = parse_auth_status(
            r#"{"loggedIn": true, "authMethod": "claude.ai", "email": "a@x.com", "orgName": "X"}"#,
        )
        .unwrap();
        assert!(s.logged_in);
        assert_eq!(s.email.as_deref(), Some("a@x.com"));
        let out = parse_auth_status(r#"{"loggedIn": false}"#).unwrap();
        assert!(!out.logged_in && out.email.is_none());
    }

    #[test]
    fn finds_the_sign_in_url() {
        assert_eq!(
            sign_in_url("If the browser didn't open, visit: https://claude.com/cai/oauth/authorize?code=true&x=1").as_deref(),
            Some("https://claude.com/cai/oauth/authorize?code=true&x=1")
        );
        assert_eq!(sign_in_url("Opening browser to sign in…"), None);
    }
}
