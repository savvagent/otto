#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    mcp_connect_fixture::run().await
}
