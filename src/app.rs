use anyhow::Result;
use crate::state::State;

pub fn list_vhosts() -> Result<()> {
    let state = State::load()?;
    if state.vhosts.is_empty() {
        println!("Tanımlı vhost yok.");
    }
    for (domain, port) in state.vhosts {
        println!("{domain:<40} → {port}");
    }
    Ok(())
}

pub fn set_ip_block(ip: &str, enabled: bool) -> Result<()> {
    let mut state = State::load()?;
    state.block(ip, enabled)?;
    println!("{}: {ip}", if enabled { "Engellendi" } else { "Engel kaldırıldı" });
    Ok(())
}

pub fn set_whitelist(ip: &str, enabled: bool) -> Result<()> {
    let mut state = State::load()?;
    state.whitelist(ip, enabled)?;
    println!("{}: {ip}", if enabled { "Whitelist'e eklendi" } else { "Whitelist'ten kaldırıldı" });
    Ok(())
}

pub fn set_maintenance(enabled: bool) -> Result<()> {
    let mut cfg = crate::config::load()?;
    cfg.maintenance_mode = enabled;
    crate::config::save(&cfg)?;
    println!("Bakım modu: {}", if enabled { "aktif" } else { "pasif" });
    Ok(())
}
