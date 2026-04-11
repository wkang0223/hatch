//! Install/uninstall the agent as a macOS launchd service.

use anyhow::{Context, Result};
use std::path::PathBuf;
use std::process::Command;
use tracing::info;

const LABEL: &str = "io.neuralmesh.agent";

pub fn plist_path() -> PathBuf {
    PathBuf::from(format!(
        "/Library/LaunchDaemons/{}.plist",
        LABEL
    ))
}

pub fn install(agent_binary_path: &str, config_path: &str) -> Result<()> {
    let plist_content = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{label}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{binary}</string>
        <string>--config</string>
        <string>{config}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>/var/log/neuralmesh/agent.log</string>
    <key>StandardErrorPath</key>
    <string>/var/log/neuralmesh/agent-error.log</string>
    <key>UserName</key>
    <string>root</string>
    <key>ThrottleInterval</key>
    <integer>30</integer>
</dict>
</plist>"#,
        label = LABEL,
        binary = agent_binary_path,
        config = config_path,
    );

    std::fs::create_dir_all("/var/log/neuralmesh")?;
    std::fs::write(plist_path(), plist_content)
        .context("Writing launchd plist")?;

    Command::new("launchctl")
        .args(["load", "-w", plist_path().to_str().unwrap()])
        .status()
        .context("launchctl load")?;

    info!("neuralmesh-agent installed as launchd service ({})", LABEL);
    Ok(())
}

pub fn uninstall() -> Result<()> {
    let path = plist_path();
    if path.exists() {
        Command::new("launchctl")
            .args(["unload", "-w", path.to_str().unwrap()])
            .status()
            .context("launchctl unload")?;
        std::fs::remove_file(&path)?;
    }
    info!("neuralmesh-agent launchd service removed");
    Ok(())
}

pub fn status() -> String {
    let out = Command::new("launchctl")
        .args(["list", LABEL])
        .output()
        .unwrap_or_else(|_| std::process::Output {
            status: std::process::ExitStatus::default(),
            stdout: vec![],
            stderr: vec![],
        });

    if out.status.success() {
        "running".to_string()
    } else {
        "stopped".to_string()
    }
}
