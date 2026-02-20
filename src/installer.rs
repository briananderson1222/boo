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

    // Create .app bundle (required for user-notify native notifications on macOS)
    let app_dir = home.join("Applications/Boo.app");
    generate_app_bundle(binary_path, &app_dir)?;
    let bundle_binary = app_dir.join("Contents/MacOS/boo");

    // Create URL scheme handler (boo:// links from browser/HTML artifacts)
    let url_app = home.join("Applications/BooURL.app");
    match generate_url_handler(&bundle_binary, &url_app) {
        Ok(()) => println!("Created BooURL.app for boo:// URL scheme"),
        Err(e) => eprintln!("Warning: could not create URL handler (swiftc required): {e}"),
    }

    let plist_dir = home.join("Library/LaunchAgents");
    std::fs::create_dir_all(&plist_dir)?;
    
    let plist_path = plist_dir.join("com.boo.scheduler.plist");
    let plist_content = generate_plist(&bundle_binary, &boo_dir());
    
    std::fs::write(&plist_path, plist_content)?;
    
    let output = Command::new("launchctl")
        .args(["load", &plist_path.to_string_lossy()])
        .output()?;
    
    if !output.status.success() {
        return Err(BooError::Other(format!("Failed to load launchd service: {}", String::from_utf8_lossy(&output.stderr))));
    }
    
    println!("Created Boo.app at {}", app_dir.display());
    println!("Created BooURL.app for boo:// URL scheme");
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

    let app_dir = home.join("Applications/Boo.app");
    if app_dir.exists() {
        std::fs::remove_dir_all(&app_dir)?;
    }

    let url_app = home.join("Applications/BooURL.app");
    if url_app.exists() {
        std::fs::remove_dir_all(&url_app)?;
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
    
    // Register boo:// URL scheme via .desktop file
    let apps_dir = home.join(".local/share/applications");
    std::fs::create_dir_all(&apps_dir)?;
    std::fs::write(apps_dir.join("boo-handler.desktop"), format!(
        "[Desktop Entry]\nName=Boo\nExec={} %u\nType=Application\nNoDisplay=true\nMimeType=x-scheme-handler/boo;\n",
        binary_path.display()
    ))?;
    let _ = Command::new("xdg-mime").args(["default", "boo-handler.desktop", "x-scheme-handler/boo"]).output();

    // Reload systemd and enable service
    let reload_output = Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .output();
    
    let enable_output = Command::new("systemctl")
        .args(["--user", "enable", "boo"])
        .output();
    
    if let (Ok(reload), Ok(enable)) = (reload_output, enable_output) {
        if !reload.status.success() || !enable.status.success() {
            print_crontab_instructions(binary_path);
        }
    } else {
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

    // Register boo:// URL scheme
    let bin = binary_path.to_string_lossy();
    let _ = Command::new("reg").args(["add", "HKCU\\Software\\Classes\\boo", "/ve", "/d", "URL:Boo Protocol", "/f"]).output();
    let _ = Command::new("reg").args(["add", "HKCU\\Software\\Classes\\boo", "/v", "URL Protocol", "/d", "", "/f"]).output();
    let _ = Command::new("reg").args(["add", "HKCU\\Software\\Classes\\boo\\shell\\open\\command", "/ve", "/d", &format!("\"{}\" \"%1\"", bin), "/f"]).output();
    
    println!("Created startup batch file at: {}", bat_path.display());
    println!("Registered boo:// URL scheme");
    println!("To enable auto-start:");
    println!("1. Press Win+R, type 'shell:startup', press Enter");
    println!("2. Copy {} to the Startup folder", bat_path.display());
    
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


#[cfg(target_os = "macos")]
static EMBEDDED_ICON: &[u8] = include_bytes!("../assets/boo.icns");

#[cfg(target_os = "macos")]
fn generate_app_bundle(binary_path: &std::path::Path, app_dir: &std::path::Path) -> Result<()> {
    let contents = app_dir.join("Contents");
    let macos_dir = contents.join("MacOS");
    let resources = contents.join("Resources");
    std::fs::create_dir_all(&macos_dir)?;
    std::fs::create_dir_all(&resources)?;

    // Copy binary into bundle
    let dest = macos_dir.join("boo");
    if dest.exists() { std::fs::remove_file(&dest)?; }
    std::fs::copy(binary_path, &dest)?;

    // Icon: user override > embedded default
    let icon_dest = resources.join("boo.icns");
    let user_icon = boo_dir().join("icon.icns");
    if user_icon.exists() {
        std::fs::copy(&user_icon, &icon_dest)?;
    } else {
        std::fs::write(&icon_dest, EMBEDDED_ICON)?;
    }

    std::fs::write(contents.join("Info.plist"), r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleIdentifier</key>
	<string>com.boo.scheduler</string>
	<key>CFBundleName</key>
	<string>Boo</string>
	<key>CFBundleExecutable</key>
	<string>boo</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
	<key>CFBundleIconFile</key>
	<string>boo.icns</string>
	<key>LSUIElement</key>
	<true/>
</dict>
</plist>"#)?;

    // Ad-hoc codesign
    let _ = Command::new("codesign")
        .args(["--force", "--sign", "-", &app_dir.to_string_lossy()])
        .output();

    Ok(())
}

#[cfg(target_os = "macos")]
fn generate_url_handler(boo_binary: &std::path::Path, app_dir: &std::path::Path) -> Result<()> {
    let contents = app_dir.join("Contents");
    let macos_dir = contents.join("MacOS");
    std::fs::create_dir_all(&macos_dir)?;

    // Compile the Swift URL handler
    let swift_src = format!(r#"import Cocoa
class D:NSObject,NSApplicationDelegate{{func application(_ a:NSApplication,open urls:[URL]){{for u in urls{{let t=Process();t.executableURL=URL(fileURLWithPath:"{}");t.arguments=[u.absoluteString];try? t.run()}};NSApp.terminate(nil)}}}};let a=NSApplication.shared;a.delegate=D();a.run()"#,
        boo_binary.to_string_lossy());

    let src_path = std::env::temp_dir().join("boo-url-handler.swift");
    std::fs::write(&src_path, &swift_src)?;

    let output = Command::new("swiftc")
        .args([src_path.to_str().unwrap(), "-o", macos_dir.join("BooURL").to_str().unwrap()])
        .output()?;
    if !output.status.success() {
        return Err(BooError::Other(format!("Failed to compile URL handler: {}", String::from_utf8_lossy(&output.stderr))));
    }

    std::fs::write(contents.join("Info.plist"), r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleIdentifier</key>
	<string>com.boo.url-handler</string>
	<key>CFBundleName</key>
	<string>BooURL</string>
	<key>CFBundleExecutable</key>
	<string>BooURL</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
	<key>LSUIElement</key>
	<true/>
	<key>CFBundleURLTypes</key>
	<array>
		<dict>
			<key>CFBundleURLName</key>
			<string>Boo URL Scheme</string>
			<key>CFBundleURLSchemes</key>
			<array>
				<string>boo</string>
			</array>
		</dict>
	</array>
</dict>
</plist>"#)?;

    let _ = Command::new("codesign")
        .args(["--force", "--sign", "-", &app_dir.to_string_lossy()])
        .output();

    // Register with Launch Services
    let _ = Command::new("/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister")
        .args(["-R", &app_dir.to_string_lossy()])
        .output();

    Ok(())
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
