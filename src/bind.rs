use tokio::process::{Child, Command};

pub fn spawn_bind() -> anyhow::Result<Child> {
    let child = Command::new("named").arg("-f").kill_on_drop(true).spawn()?;

    Ok(child)
}
