use tokio::process::{Child, Command};

pub fn spawn_unbound() -> anyhow::Result<Child> {
    // -d keeps unbound in the foreground so it stays a supervised child process
    let child = Command::new("unbound")
        .arg("-d")
        .arg("-c")
        .arg("/etc/unbound/unbound.conf")
        .kill_on_drop(true)
        .spawn()?;

    Ok(child)
}
