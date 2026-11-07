// SPDX-License-Identifier: Apache-2.0
//! Generate the RPC schema: cargo run -p dctx-cli --example rpc_schema.
fn main() -> Result<(), serde_json::Error> {
    print!("{}", dctx_cli::rpc::schema()?);
    Ok(())
}
