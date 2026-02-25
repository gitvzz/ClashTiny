use std::process::Command;

const PROXY_HOST: &str = "127.0.0.1";
const PROXY_PORT: &str = "7890";

/// Get list of active network services (e.g. "Wi-Fi", "Ethernet")
fn get_active_services() -> Vec<String> {
    let output = Command::new("networksetup")
        .args(["-listallnetworkservices"])
        .output();

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            stdout
                .lines()
                .skip(1) // skip header line
                .filter(|line| !line.starts_with('*')) // skip disabled services
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        }
        Err(e) => {
            eprintln!("[ClashTiny] Failed to list network services: {e}");
            vec!["Wi-Fi".to_string(), "Ethernet".to_string()]
        }
    }
}

/// Enable system proxy (HTTP + HTTPS + SOCKS5) on all active network services
pub fn enable_system_proxy() -> Result<(), String> {
    let services = get_active_services();
    if services.is_empty() {
        return Err("No active network services found".to_string());
    }

    let mut errors = Vec::new();
    for service in &services {
        // HTTP proxy
        let r1 = Command::new("networksetup")
            .args(["-setwebproxy", service, PROXY_HOST, PROXY_PORT])
            .status();
        // HTTPS proxy
        let r2 = Command::new("networksetup")
            .args(["-setsecurewebproxy", service, PROXY_HOST, PROXY_PORT])
            .status();
        // SOCKS5 proxy
        let r3 = Command::new("networksetup")
            .args(["-setsocksfirewallproxy", service, PROXY_HOST, PROXY_PORT])
            .status();

        let r4 = Command::new("networksetup")
            .args([
                "-setproxybypassdomains", service,
                "localhost", "127.0.0.1", "::1",
                "192.168.0.0/16", "10.0.0.0/8", "172.16.0.0/12",
                "*.local", "<local>",
            ])
            .status();

        for (cmd, result) in [
            ("setwebproxy", r1),
            ("setsecurewebproxy", r2),
            ("setsocksfirewallproxy", r3),
            ("setproxybypassdomains", r4),
        ] {
            match result {
                Ok(s) if s.success() => {}
                Ok(s) => errors.push(format!("{} on '{}' exited with {}", cmd, service, s)),
                Err(e) => errors.push(format!("{} on '{}' failed: {}", cmd, service, e)),
            }
        }
    }

    if errors.is_empty() {
        println!("[ClashTiny] System proxy enabled on {} services", services.len());
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

/// Disable system proxy on all active network services
pub fn disable_system_proxy() -> Result<(), String> {
    let services = get_active_services();

    let mut errors = Vec::new();
    for service in &services {
        let r1 = Command::new("networksetup")
            .args(["-setwebproxystate", service, "off"])
            .status();
        let r2 = Command::new("networksetup")
            .args(["-setsecurewebproxystate", service, "off"])
            .status();
        let r3 = Command::new("networksetup")
            .args(["-setsocksfirewallproxystate", service, "off"])
            .status();

        for (cmd, result) in [("webproxy off", r1), ("securewebproxy off", r2), ("socksFirewallProxy off", r3)] {
            match result {
                Ok(s) if s.success() => {}
                Ok(s) => errors.push(format!("{} on '{}' exited with {}", cmd, service, s)),
                Err(e) => errors.push(format!("{} on '{}' failed: {}", cmd, service, e)),
            }
        }
    }

    if errors.is_empty() {
        println!("[ClashTiny] System proxy disabled");
        Ok(())
    } else {
        // Non-fatal: log but don't block
        eprintln!("[ClashTiny] Some proxy disable errors: {}", errors.join("; "));
        Ok(())
    }
}
