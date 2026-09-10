use sdk::prelude::*;
#[derive(Clone)]
pub struct Renamed;
#[server(name = "renamed", version = "1")]
impl Renamed {
    #[tool]
    async fn echo(&self, value: String) -> McpResult<String> {
        Ok(value)
    }
}
