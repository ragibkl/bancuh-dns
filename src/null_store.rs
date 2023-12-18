#[derive(Clone, Debug)]
pub struct NullStore;

impl NullStore {
    pub async fn is_blocked(&self, name: &str) -> bool {
        matches!(name, "zedo.com." | "doubleclick.net.")
    }
}
