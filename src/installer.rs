use crate::config::boo_dir;
use crate::error::{BooError, Result};
use std::path::PathBuf;
use std::process::Command;

/// Install boo daemon as auto-start service for current platform.
/// Returns the path of the installed service file.
pub fn install() -> Result<std::path::PathBuf> {
    let binary_path = get_boo_binary_path()?;
    
    #[cfg(target_os = "macos")]
    {
        install_macos(&binary_path)
    }
    
    #[cfg(target_os = "linux")]
    {
        install_linux(&binary_path)
    }
    
    #[cfg(target_os = "windows")]
    {
        install_windows(&binary_path)
    }
    
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        Err(BooError::Other("Unsupported platform".to_string()))
    }
}

/// Remove boo daemon from auto-start.
pub fn uninstall() -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        uninstall_macos()
    }
    
    #[cfg(target_os = "linux")]
    {
        uninstall_linux()
    }
    
    #[cfg(target_os = "windows")]
    {
        uninstall_windows()
    }
    
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        Err(BooError::Other("Unsupported platform".to_string()))
    }
}

/// Check if boo is installed as auto-start service.
pub fn is_installed() -> bool {
    #[cfg(target_os = "macos")]
    {
        is_installed_macos()
    }
    
    #[cfg(target_os = "linux")]
    {
        is_installed_linux()
    }
    
    #[cfg(target_os = "windows")]
    {
        is_installed_windows()
    }
    
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        false
    }
}

fn get_boo_binary_path() -> Result<PathBuf> {
    std::env::current_exe().map_err(BooError::Io)
}

#[cfg(target_os = "macos")]
fn install_macos(binary_path: &std::path::Path) -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| BooError::Other("Could not determine home directory".to_string()))?;
    let plist_dir = home.join("Library/LaunchAgents");
    std::fs::create_dir_all(&plist_dir)?;
    
    let plist_path = plist_dir.join("com.boo.scheduler.plist");
    let plist_content = generate_plist(binary_path, &boo_dir());
    
    std::fs::write(&plist_path, plist_content)?;
    
    // Load the service
    let output = Command::new("launchctl")
        .args(["load", &plist_path.to_string_lossy()])
        .output()?;
    
    if !output.status.success() {
        return Err(BooError::Other(format!("Failed to load launchd service: {}", String::from_utf8_lossy(&output.stderr))));
    }
    
    Ok(plist_path)
}

#[cfg(target_os = "macos")]
fn uninstall_macos() -> Result<()> {
    let home = dirs::home_dir().ok_or_else(|| BooError::Other("Could not determine home directory".to_string()))?;
    let plist_path = home.join("Library/LaunchAgents/com.boo.scheduler.plist");
    
    if plist_path.exists() {
        let _ = Command::new("launchctl")
            .args(["unload", &plist_path.to_string_lossy()])
            .output();
        
        std::fs::remove_file(plist_path)?;
    }
    
    Ok(())
}

#[cfg(target_os = "macos")]
fn is_installed_macos() -> bool {
    let home = dirs::home_dir().unwrap_or_default();
    home.join("Library/LaunchAgents/com.boo.scheduler.plist").exists()
}

#[cfg(target_os = "linux")]
fn install_linux(binary_path: &std::path::Path) -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| BooError::Other("Could not determine home directory".to_string()))?;
    let systemd_dir = home.join(".config/systemd/user");
    std::fs::create_dir_all(&systemd_dir)?;
    
    let service_path = systemd_dir.join("boo.service");
    let service_content = generate_systemd_unit(binary_path);
    
    std::fs::write(&service_path, service_content)?;
    
    // Reload systemd and enable service
    let reload_output = Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .output();
    
    let enable_output = Command::new("systemctl")
        .args(["--user", "enable", "boo"])
        .output();
    
    // Check if systemd commands failed
    if let (Ok(reload), Ok(enable)) = (reload_output, enable_output) {
        if !reload.status.success() || !enable.status.success() {
            // Fallback to crontab instructions
            print_crontab_instructions(binary_path);
        }
    } else {
        // Systemd not available, print crontab instructions
        print_crontab_instructions(binary_path);
    }
    
    Ok(service_path)
}

#[cfg(target_os = "linux")]
fn uninstall_linux() -> Result<()> {
    let home = dirs::home_dir().ok_or_else(|| BooError::Other("Could not determine home directory".to_string()))?;
    let service_path = home.join(".config/systemd/user/boo.service");
    
    if service_path.exists() {
        // Disable and stop service
        let _ = Command::new("systemctl")
            .args(["--user", "disable", "boo"])
            .output();
        
        let _ = Command::new("systemctl")
            .args(["--user", "stop", "boo"])
            .output();
        
        std::fs::remove_file(service_path)?;
        
        // Reload systemd
        let _ = Command::new("systemctl")
            .args(["--user", "daemon-reload"])
            .output();
    }
    
    Ok(())
}

#[cfg(target_os = "linux")]
fn is_installed_linux() -> bool {
    let home = dirs::home_dir().unwrap_or_default();
    home.join(".config/systemd/user/boo.service").exists()
}

#[cfg(target_os = "linux")]
fn print_crontab_instructions(binary_path: &std::path::Path) {
    println!("Systemd not available. To enable auto-start, add this line to your crontab:");
    println!("@reboot {}", binary_path.display());
    println!("Run: crontab -e");
}

#[cfg(target_os = "windows")]
fn install_windows(binary_path: &std::path::Path) -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| BooError::Other("Could not determine home directory".to_string()))?;
    let bat_path = home.join("boo-startup.bat");
    
    let bat_content = format!("@echo off\n\"{}\" daemon\n", binary_path.display());
    std::fs::write(&bat_path, bat_content)?;
    
    println!("Created startup batch file at: {}", bat_path.display());
    println!("To enable auto-start:");
    println!("1. Press Win+R, type 'shell:startup', press Enter");
    println!("2. Copy {} to the Startup folder", bat_path.display());
    println!("Or add to registry Run key manually.");
    
    Ok(bat_path)
}

#[cfg(target_os = "windows")]
fn uninstall_windows() -> Result<()> {
    let home = dirs::home_dir().ok_or_else(|| BooError::Other("Could not determine home directory".to_string()))?;
    let bat_path = home.join("boo-startup.bat");
    
    if bat_path.exists() {
        std::fs::remove_file(bat_path)?;
    }
    
    println!("Removed startup batch file. If you added it to the Startup folder, remove it manually.");
    Ok(())
}

#[cfg(target_os = "windows")]
fn is_installed_windows() -> bool {
    let home = dirs::home_dir().unwrap_or_default();
    home.join("boo-startup.bat").exists()
}

pub fn generate_plist(binary_path: &std::path::Path, boo_dir: &std::path::Path) -> String {
    format!(r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.boo.scheduler</string>
    <key>ProgramArguments</key>
    <array>
        <string>{}</string>
        <string>daemon</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>{}/daemon.log</string>
    <key>StandardErrorPath</key>
    <string>{}/daemon.log</string>
    <key>EnvironmentVariables</key>
    <dict>
        <key>PATH</key>
        <string>/usr/local/bin:/usr/bin:/bin:/opt/homebrew/bin</string>
    </dict>
</dict>
</plist>"#, binary_path.display(), boo_dir.display(), boo_dir.display())
}

pub fn generate_systemd_unit(binary_path: &std::path::Path) -> String {
    format!(r#"[Unit]
Description=Boo Scheduler Daemon
After=default.target

[Service]
ExecStart={} daemon
Restart=always
RestartSec=5

[Install]
WantedBy=default.target
"#, binary_path.display())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_generate_plist() {
        let binary_path = PathBuf::from("/usr/local/bin/boo");
        let boo_dir = PathBuf::from("/Users/test/.boo");
        let result = generate_plist(&binary_path, &boo_dir);
        
        assert!(result.contains("com.boo.scheduler"));
        assert!(result.contains("/usr/local/bin/boo"));
        assert!(result.contains("daemon"));
        assert!(result.contains("/Users/test/.boo/daemon.log"));
        assert!(result.contains("/opt/homebrew/bin"));
    }

    #[test]
    fn test_generate_systemd_unit() {
        let binary_path = PathBuf::from("/usr/local/bin/boo");
        let result = generate_systemd_unit(&binary_path);
        
        assert!(result.contains("Boo Scheduler Daemon"));
        assert!(result.contains("/usr/local/bin/boo daemon"));
        assert!(result.contains("Restart=always"));
        assert!(result.contains("WantedBy=default.target"));
    }
}
